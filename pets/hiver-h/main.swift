import AppKit

// Hiver H — a living, three-dimensional "H": five fitted graphite sections, mint eyes in the bridge.
// Animated desktop pet that watches hiver: swarms, agents and their messages drive its animations.
// CLI:  hiver-h                    → launch the pet
//       hiver-h say "hi"           → make the running pet say something
//       hiver-h assign|listen|think|message|complete ["text"]
//                                  → play that animation (and say the text, if any)

let sayNote = Notification.Name("com.hiver.h.say")
let eventNote = Notification.Name("com.hiver.h.event")
let petEvents = ["assign", "listen", "think", "message", "complete", "chat"]

let cliArgs = CommandLine.arguments
if cliArgs.count >= 2 && cliArgs[1] == "say" {
    let text = cliArgs.dropFirst(2).joined(separator: " ")
    DistributedNotificationCenter.default().postNotificationName(sayNote, object: text, userInfo: nil, deliverImmediately: true)
    exit(0)
}
if cliArgs.count >= 2 && petEvents.contains(cliArgs[1]) {
    // "event\ntext": the object is a plain string, which distributed notifications always carry
    let text = cliArgs.dropFirst(2).joined(separator: " ")
    DistributedNotificationCenter.default().postNotificationName(eventNote, object: cliArgs[1] + "\n" + text, userInfo: nil, deliverImmediately: true)
    exit(0)
}
if cliArgs.count >= 2 && ["-h", "--help", "help"].contains(cliArgs[1]) {
    print("""
    usage: hiver-h                         launch the pet (it watches hiver on its own)
           hiver-h say "text"              make the running pet say something
           hiver-h assign|listen|think|message|complete ["text"]
                                           play that animation, optionally saying the text
    """)
    exit(0)
}

// One Hiver H per desktop
if let id = Bundle.main.bundleIdentifier,
   NSRunningApplication.runningApplications(withBundleIdentifier: id).count > 1 {
    exit(0)
}

// MARK: - Palette

extension NSColor {
    convenience init(hex: Int, alpha: CGFloat = 1) {
        self.init(srgbRed: CGFloat((hex >> 16) & 0xff) / 255,
                  green: CGFloat((hex >> 8) & 0xff) / 255,
                  blue: CGFloat(hex & 0xff) / 255,
                  alpha: alpha)
    }
}

enum Palette {
    static let faceLight = NSColor(hex: 0x5b676d)
    static let face = NSColor(hex: 0x262d31)
    static let faceDark = NSColor(hex: 0x101315)
    static let sideNear = NSColor(hex: 0x171b1e)
    static let sideFar = NSColor(hex: 0x050607)
    static let mint = NSColor(hex: 0x8ff0cc)
    static let mintGlow = NSColor(hex: 0xc4ffea)
    static let ink = NSColor(hex: 0x111518)
    static let paper = NSColor(hex: 0xe6f4ee)
}

let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!

func gradient(_ colors: [NSColor], _ locations: [CGFloat]) -> CGGradient {
    CGGradient(colorsSpace: sRGB, colors: colors.map(\.cgColor) as CFArray, locations: locations)!
}

func lerp(_ a: CGFloat, _ b: CGFloat, _ k: CGFloat) -> CGFloat { a + (b - a) * k }
func easeInOut(_ k: CGFloat) -> CGFloat { k * k * (3 - 2 * k) }

// MARK: - Pet

final class PetView: NSView {
    /// One fitted section of the H; `spread` is where it slides when the H opens up
    struct Section { let rect: CGRect; let spread: CGVector }
    /// A light pulse travelling along a polyline (through the bridge or along a connection)
    struct Pulse { let start: Double; let route: Int }   // route -1 = through the bridge, 0...3 = to a section

    // Assembled H: two uprights (each a top and bottom section) joined by the bridge
    private let sections: [Section] = [
        Section(rect: CGRect(x: 70, y: 80, width: 20, height: 39), spread: CGVector(dx: -13, dy: 7)),    // left top
        Section(rect: CGRect(x: 70, y: 41, width: 20, height: 39), spread: CGVector(dx: -13, dy: -7)),   // left bottom
        Section(rect: CGRect(x: 130, y: 80, width: 20, height: 39), spread: CGVector(dx: 13, dy: 7)),    // right top
        Section(rect: CGRect(x: 130, y: 41, width: 20, height: 39), spread: CGVector(dx: 13, dy: -7)),   // right bottom
        Section(rect: CGRect(x: 90, y: 70, width: 40, height: 20), spread: .zero),                       // bridge
    ]
    private var bridge: CGRect { sections[4].rect }
    private let pivot = CGPoint(x: 110, y: 41)   // base of the H: leans and nods pivot here

    var talking = false
    var onPoke: (() -> Void)?
    /// hiver agents shown as mint dots on the floor: bright = working, dim = idle
    var agentCount = (working: 0, idle: 0) { didSet { needsDisplay = true } }

    private var t = 0.0
    private var spread: CGFloat = 1        // starts open; settles into an H on launch
    private var spreadVel: CGFloat = 0
    private var assignUntil = -1.0
    private var listenUntil = -1.0
    private var thinkUntil = -1.0
    private var lean: CGFloat = 0
    private var shift = CGPoint.zero       // idle micro-shifts: x offset, rotation
    private var shiftGoal = CGPoint.zero
    private var nextShift = 3.0
    private var nodStart = -10.0
    private var glowUntil = -1.0
    private var pulses: [Pulse] = []
    private var nextPulse = 0.0
    private var nextBlink = 2.5
    private var blinkUntil = 0.0
    private var look = CGPoint.zero

    private var dragStart: NSPoint?
    private var windowStart = NSPoint.zero
    private var moved = false

    override init(frame: NSRect) {
        super.init(frame: frame)
        let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    required init?(coder: NSCoder) { fatalError() }

    // MARK: Behaviors

    /// Assigning work: sections slide outward, linked by fine mint connections; messages flow
    func assign(for seconds: Double = 6) {
        assignUntil = t + seconds
        nextPulse = t + 0.5
    }

    /// Listening: lean forward slightly
    func listen(for seconds: Double = 2.5) { listenUntil = t + seconds }

    /// Thinking: a soft pulse moves across the bridge
    func think(for seconds: Double = 4) { thinkUntil = t + seconds }

    /// Agents communicating: a brief pulse travels through the bridge
    func message() { pulses.append(Pulse(start: t, route: -1)) }

    /// Task finished: sections settle back into a complete H, restrained nod, brief mint glow
    func complete() {
        assignUntil = -1
        thinkUntil = -1
        nodStart = t + 0.45
        glowUntil = t + 1.6
    }

    private var assigning: Bool { t < assignUntil }
    private var thinking: Bool { t < thinkUntil || talking }

    // MARK: Animation

    private func tick() {
        guard window?.isVisible == true else { return }   // hidden: skip animation work
        let dt = 1.0 / 60
        t += dt

        // Sections spring open/closed with a small, precise settle
        let goal: CGFloat = assigning ? 1 : 0
        spreadVel = (spreadVel + (goal - spread) * 46 * CGFloat(dt)) * CGFloat(exp(-8.5 * dt))
        spread += spreadVel * CGFloat(dt)

        lean += ((t < listenUntil ? 1 : 0) - lean) * CGFloat(min(1, dt * 6))

        // Small, deliberate idle shifts
        if t >= nextShift {
            nextShift = t + Double.random(in: 3.5...6.5)
            shiftGoal = CGPoint(x: .random(in: -2.5...2.5), y: .random(in: -0.035...0.035))
        }
        shift.x += (shiftGoal.x - shift.x) * CGFloat(min(1, dt * 2.2))
        shift.y += (shiftGoal.y - shift.y) * CGFloat(min(1, dt * 2.2))

        // While assigning, messages run through the bridge and out along the connections
        pulses.removeAll { t - $0.start > 0.8 }
        if assigning && t >= nextPulse {
            pulses.append(Pulse(start: t, route: Int.random(in: -1...3)))
            nextPulse = t + Double.random(in: 0.35...0.75)
        }

        if t >= nextBlink {
            blinkUntil = t + 0.13
            nextBlink = t + Double.random(in: 2.5...5.5)
        }

        // Observant eyes drift toward the cursor (listening: straight at you)
        if let window {
            let m = NSEvent.mouseLocation
            let c = window.convertPoint(toScreen: convert(CGPoint(x: bridge.midX, y: bridge.midY), to: nil))
            let dx = m.x - c.x, dy = m.y - c.y
            let dist = max(1, hypot(dx, dy))
            let reach = min(1, dist / 260) * (1 - lean)
            look.x += (dx / dist * reach * 3 - look.x) * 0.12
            look.y += (dy / dist * reach * 1.6 - look.y) * 0.12
        }

        needsDisplay = true
    }

    private var nodAmount: CGFloat {
        let k = (t - nodStart) / 0.8
        guard k >= 0 && k < 1 else { return 0 }
        return CGFloat(sin(k * .pi))
    }

    private func placed(_ i: Int) -> CGRect {
        let s = sections[i]
        return s.rect.offsetBy(dx: s.spread.dx * spread, dy: s.spread.dy * spread)
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        let ctx = NSGraphicsContext.current!.cgContext
        let rise = 1.2 * CGFloat(sin(t * 1.3))

        // Soft floor shadow, widening as the sections spread
        let sw = 104 + 30 * spread
        ctx.saveGState()
        ctx.translateBy(x: pivot.x + 3, y: pivot.y - 9)
        ctx.scaleBy(x: 1, y: 0.18)
        ctx.drawRadialGradient(gradient([NSColor(white: 0, alpha: 0.45), NSColor(white: 0, alpha: 0)], [0, 1]),
                               startCenter: .zero, startRadius: 0, endCenter: .zero, endRadius: sw / 2, options: [])
        ctx.restoreGState()
        drawAgentDots(ctx)

        // Whole-body pose: idle shift, lean forward, nod — pivoting at the base
        ctx.saveGState()
        ctx.translateBy(x: pivot.x + shift.x, y: pivot.y + rise - 2 * lean - 2.5 * nodAmount)
        ctx.rotate(by: shift.y)
        let s = 1 + 0.045 * lean
        ctx.scaleBy(x: s, y: s * (1 - 0.02 * nodAmount))
        ctx.translateBy(x: -pivot.x, y: -pivot.y)

        let rects = sections.indices.map { placed($0) }
        for r in rects { drawSide(ctx, r) }            // extrusions first, so faces always sit on top
        drawConnections(ctx, rects)
        for r in rects { drawFace(ctx, r) }
        drawBridgeLife(ctx, rects[4])
        for p in pulses { drawPulse(ctx, p, rects) }
        ctx.restoreGState()
    }

    /// Active hiver agents: a row of small mint dots on the floor below the H (max 12 drawn)
    private func drawAgentDots(_ ctx: CGContext) {
        let working = min(agentCount.working, 12)
        let idle = min(agentCount.idle, 12 - working)
        let n = working + idle
        guard n > 0 else { return }
        let gap: CGFloat = 7
        let x0 = pivot.x - gap * CGFloat(n - 1) / 2
        ctx.saveGState()
        for i in 0..<n {
            let busy = i < working
            // Working dots breathe gently, out of phase, so the row feels alive
            let k = busy ? 0.75 + 0.25 * CGFloat(sin(t * 2.4 + Double(i) * 0.9)) : 0.45
            let r: CGFloat = busy ? 1.9 : 1.5
            let c = CGPoint(x: x0 + gap * CGFloat(i), y: pivot.y - 21)
            ctx.setShadow(offset: .zero, blur: busy ? 5 : 0, color: Palette.mint.withAlphaComponent(0.8 * k).cgColor)
            ctx.setFillColor(Palette.mint.withAlphaComponent(k).cgColor)
            ctx.fillEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2))
        }
        ctx.restoreGState()
    }

    private func path(_ r: CGRect) -> CGPath {
        CGPath(roundedRect: r, cornerWidth: 4, cornerHeight: 4, transform: nil)
    }

    /// Extruded depth toward the lower right
    private func drawSide(_ ctx: CGContext, _ r: CGRect) {
        let steps = 6
        for k in stride(from: steps, through: 1, by: -1) {
            let f = CGFloat(k) / CGFloat(steps)
            ctx.addPath(path(r.offsetBy(dx: 0.6 * CGFloat(k), dy: -0.8 * CGFloat(k))))
            ctx.setFillColor(Palette.sideNear.blended(withFraction: f, of: Palette.sideFar)!.cgColor)
            ctx.fillPath()
        }
    }

    /// Satin front face with a beveled edge
    private func drawFace(_ ctx: CGContext, _ r: CGRect) {
        let p = path(r)
        if t < glowUntil {
            let k = CGFloat((glowUntil - t) / 1.6)
            ctx.saveGState()
            ctx.setShadow(offset: .zero, blur: 14, color: Palette.mint.withAlphaComponent(0.9 * k).cgColor)
            ctx.addPath(p)
            ctx.setFillColor(Palette.face.cgColor)
            ctx.fillPath()
            ctx.restoreGState()
        }

        ctx.saveGState()
        ctx.addPath(p)
        ctx.clip()
        let tl = CGPoint(x: r.minX, y: r.maxY), br = CGPoint(x: r.maxX, y: r.minY)
        ctx.drawLinearGradient(gradient([Palette.faceLight, Palette.face, Palette.faceDark], [0, 0.45, 1]),
                               start: tl, end: br, options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
        let sheen = CGPoint(x: r.minX + r.width * 0.3, y: r.maxY - 3)
        ctx.drawRadialGradient(gradient([NSColor.white.withAlphaComponent(0.13), NSColor.white.withAlphaComponent(0)], [0, 1]),
                               startCenter: sheen, startRadius: 0, endCenter: sheen, endRadius: max(r.width, r.height) * 0.7, options: [])

        // Bevel: light catches the upper-left edge, the lower-right edge falls into shadow
        ctx.addPath(CGPath(roundedRect: r.insetBy(dx: 1.2, dy: 1.2), cornerWidth: 3, cornerHeight: 3, transform: nil))
        ctx.setLineWidth(2.4)
        ctx.replacePathWithStrokedPath()
        ctx.clip()
        ctx.drawLinearGradient(gradient([NSColor.white.withAlphaComponent(0.34), NSColor.white.withAlphaComponent(0),
                                         NSColor.black.withAlphaComponent(0.5)], [0, 0.5, 1]),
                               start: tl, end: br, options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
        ctx.restoreGState()
    }

    /// Endpoints of the fine mint links between the bridge and each upright section
    private func connection(_ i: Int, _ rects: [CGRect]) -> (CGPoint, CGPoint) {
        let b = rects[4], s = rects[i]
        let left = i < 2, top = i % 2 == 0
        let from = CGPoint(x: left ? b.minX : b.maxX, y: b.midY + (top ? 5 : -5))
        let to = CGPoint(x: left ? s.maxX - 3 : s.minX + 3, y: top ? s.minY + 6 : s.maxY - 6)
        return (from, to)
    }

    private func drawConnections(_ ctx: CGContext, _ rects: [CGRect]) {
        guard spread > 0.04 else { return }
        let alpha = min(1, spread * 1.4)
        ctx.saveGState()
        ctx.setShadow(offset: .zero, blur: 4, color: Palette.mint.withAlphaComponent(0.8 * alpha).cgColor)
        ctx.setStrokeColor(Palette.mint.withAlphaComponent(0.85 * alpha).cgColor)
        ctx.setLineWidth(1)
        ctx.setLineCap(.round)
        for i in 0..<4 {
            let (a, b) = connection(i, rects)
            ctx.move(to: a)
            ctx.addLine(to: b)
        }
        // Each upright's two halves stay linked too
        for (top, bottom) in [(0, 1), (2, 3)] {
            let x = rects[top].midX
            ctx.move(to: CGPoint(x: x, y: rects[top].minY + 2))
            ctx.addLine(to: CGPoint(x: x, y: rects[bottom].maxY - 2))
        }
        ctx.strokePath()
        ctx.restoreGState()
    }

    /// Eyes, plus the thinking pulse that sweeps across the bridge
    private func drawBridgeLife(_ ctx: CGContext, _ b: CGRect) {
        if thinking {
            let phase = CGFloat((t * 0.75).truncatingRemainder(dividingBy: 1))
            let x = b.minX - 10 + (b.width + 20) * easeInOut(phase)
            ctx.saveGState()
            ctx.addPath(path(b))
            ctx.clip()
            let c = CGPoint(x: x, y: b.midY)
            ctx.drawRadialGradient(gradient([Palette.mint.withAlphaComponent(0.6), Palette.mint.withAlphaComponent(0)], [0, 1]),
                                   startCenter: c, startRadius: 0, endCenter: c, endRadius: 17, options: [])
            ctx.restoreGState()
        }

        let eyeY = b.midY + look.y - 1.5 * lean - 1.2 * nodAmount
        ctx.saveGState()
        ctx.setShadow(offset: .zero, blur: 5, color: Palette.mint.withAlphaComponent(0.85).cgColor)
        ctx.setFillColor(Palette.mint.cgColor)
        for side: CGFloat in [-1, 1] {
            let ex = b.midX + side * 7 + look.x
            let h: CGFloat = t < blinkUntil ? 1.3 : 6.6
            ctx.addPath(CGPath(roundedRect: CGRect(x: ex - 2.3, y: eyeY - h / 2, width: 4.6, height: h),
                               cornerWidth: min(2.3, h / 2), cornerHeight: min(2.3, h / 2), transform: nil))
        }
        ctx.fillPath()
        ctx.restoreGState()
    }

    private func drawPulse(_ ctx: CGContext, _ p: Pulse, _ rects: [CGRect]) {
        let k = CGFloat((t - p.start) / 0.8)
        guard k >= 0 && k <= 1 else { return }
        let a: CGPoint, b: CGPoint
        if p.route < 0 {
            // Through the bridge, upright to upright
            a = CGPoint(x: rects[0].midX, y: rects[4].midY)
            b = CGPoint(x: rects[2].midX, y: rects[4].midY)
        } else {
            (a, b) = connection(p.route, rects)
        }
        let e = easeInOut(k)
        ctx.saveGState()
        ctx.setShadow(offset: .zero, blur: 7, color: Palette.mintGlow.cgColor)
        for trail in 0..<4 {
            let kk = max(0, e - CGFloat(trail) * 0.07)
            let pt = CGPoint(x: lerp(a.x, b.x, kk), y: lerp(a.y, b.y, kk))
            let r = 2 - CGFloat(trail) * 0.4
            ctx.setFillColor(NSColor.white.withAlphaComponent((1 - CGFloat(trail) * 0.22) * sin(k * .pi)).cgColor)
            ctx.fillEllipse(in: CGRect(x: pt.x - r, y: pt.y - r, width: r * 2, height: r * 2))
        }
        ctx.restoreGState()
    }

    // MARK: Mouse: drag to move, click to poke

    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }

    override func mouseDown(with event: NSEvent) {
        dragStart = NSEvent.mouseLocation
        windowStart = window?.frame.origin ?? .zero
        moved = false
    }

    override func mouseDragged(with event: NSEvent) {
        guard let start = dragStart, let window else { return }
        let p = NSEvent.mouseLocation
        if abs(p.x - start.x) + abs(p.y - start.y) > 3 { moved = true }
        window.setFrameOrigin(NSPoint(x: windowStart.x + p.x - start.x, y: windowStart.y + p.y - start.y))
    }

    override func mouseUp(with event: NSEvent) {
        if !moved { onPoke?() }
        dragStart = nil
    }
}

// MARK: - Speech bubble

final class BubbleView: NSView {
    var text = "" { didSet { needsDisplay = true } }
    override var isFlipped: Bool { true }

    private let attrs: [NSAttributedString.Key: Any] = [
        .font: NSFont.monospacedSystemFont(ofSize: 12, weight: .medium),
        .foregroundColor: Palette.paper,
    ]

    override func draw(_ dirtyRect: NSRect) {
        guard !text.isEmpty else { return }
        let maxW = bounds.width - 30
        let str = NSAttributedString(string: text, attributes: attrs)
        let tb = str.boundingRect(with: NSSize(width: maxW, height: 1000), options: .usesLineFragmentOrigin)
        let textH = min(ceil(tb.height), bounds.height - 30)
        let w = ceil(tb.width) + 24, h = textH + 14
        let box = NSRect(x: (bounds.width - w) / 2, y: bounds.height - 12 - h, width: w, height: h)

        // Terminal-style dark bubble with a mint edge
        let shape = NSBezierPath(roundedRect: box, xRadius: 7, yRadius: 7)
        let tail = NSBezierPath()
        tail.move(to: NSPoint(x: bounds.midX - 6, y: box.maxY - 1))
        tail.line(to: NSPoint(x: bounds.midX, y: bounds.height - 3))
        tail.line(to: NSPoint(x: bounds.midX + 6, y: box.maxY - 1))
        tail.close()
        shape.append(tail)

        NSGraphicsContext.saveGraphicsState()
        let glow = NSShadow()
        glow.shadowColor = Palette.mint.withAlphaComponent(0.5)
        glow.shadowBlurRadius = 8
        glow.set()
        Palette.ink.setFill()
        shape.fill()
        NSGraphicsContext.restoreGraphicsState()
        // Wide stroke then refill: only the outer half shows, so the box/tail seam disappears
        Palette.mint.setStroke()
        shape.lineWidth = 2.4
        shape.stroke()
        Palette.ink.setFill()
        shape.fill()

        str.draw(with: NSRect(x: box.minX + 12, y: box.minY + 7, width: maxW, height: textH),
                 options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }
}

// MARK: - App

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let pet = PetView(frame: NSRect(x: 0, y: 0, width: 220, height: 150))
    let bubble = BubbleView(frame: NSRect(x: 0, y: 130, width: 220, height: 80))
    private var speech: Process?
    private var hideBubble: DispatchWorkItem?
    private var watchItem: NSMenuItem!
    private var voiceItem: NSMenuItem!
    private var speakAloud: Bool {
        get { UserDefaults.standard.object(forKey: "speakAloud") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "speakAloud") }
    }
    private let hiverItem = NSMenuItem(title: "hiver: looking…", action: nil, keyEquivalent: "")
    private let watcher = HiverWatcher()
    private let chat = HiverChat()
    private lazy var switcher = PetSwitcher(current: "hiver-h", say: { [weak self] in self?.say($0, aloud: false) },
                                            turnOffHere: { [weak self] in self?.turnOff() })
    private var watching: Bool {
        get { UserDefaults.standard.object(forKey: "watchHiver") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "watchHiver") }
    }

    private let launchAgent = URL(fileURLWithPath: NSHomeDirectory() + "/Library/LaunchAgents/com.hiver.h.plist")

    func applicationDidFinishLaunching(_ notification: Notification) {
        let size = NSSize(width: 220, height: 210)
        window = NSWindow(contentRect: NSRect(origin: .zero, size: size), styleMask: .borderless, backing: .buffered, defer: false)
        window.isOpaque = false
        window.backgroundColor = .clear
        window.hasShadow = false
        window.level = .floating
        window.collectionBehavior = [.canJoinAllSpaces, .stationary, .ignoresCycle]

        let root = NSView(frame: NSRect(origin: .zero, size: size))
        root.addSubview(pet)
        root.addSubview(bubble)
        window.contentView = root

        if !window.setFrameUsingName("HiverHPet"), let screen = NSScreen.main {
            let v = screen.visibleFrame
            window.setFrameOrigin(NSPoint(x: v.maxX - size.width - 760, y: v.minY + 10))
        }
        window.setContentSize(size)
        window.setFrameAutosaveName("HiverHPet")

        let menu = NSMenu()
        hiverItem.isEnabled = false
        menu.addItem(hiverItem)
        watchItem = menu.addItem(withTitle: "Watch hiver", action: #selector(toggleWatch), keyEquivalent: "")
        watchItem.target = self
        watchItem.state = watching ? .on : .off
        voiceItem = menu.addItem(withTitle: "Speak aloud", action: #selector(toggleVoice), keyEquivalent: "")
        voiceItem.target = self
        voiceItem.state = speakAloud ? .on : .off
        menu.addItem(.separator())
        menu.addItem(withTitle: "Assign work", action: #selector(assignWork), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Listen", action: #selector(listen), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Think", action: #selector(think), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Send message", action: #selector(sendMessage), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Task complete", action: #selector(taskDone), keyEquivalent: "").target = self
        menu.addItem(.separator())
        switcher.addItems(to: menu)
        menu.addItem(withTitle: "Quit Hiver", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "")
        pet.menu = menu
        // Click: talk to the hiver agent (a chat box above the pet); drag still moves it
        pet.onPoke = { [weak self] in
            guard let self else { return }
            pet.listen()
            chat.toggle(above: window)
        }
        chat.say = { [weak self] text, aloud in self?.say(text, aloud: aloud) }
        chat.onSent = { [weak self] in self?.pet.listen(for: 3) }

        DistributedNotificationCenter.default().addObserver(forName: sayNote, object: nil, queue: .main) { [weak self] note in
            guard let text = note.object as? String, !text.isEmpty else { return }
            MainActor.assumeIsolated {
                self?.pet.listen()
                self?.say(text)
            }
        }

        // `hiver-h assign|listen|think|message|complete ["text"]`
        DistributedNotificationCenter.default().addObserver(forName: eventNote, object: nil, queue: .main) { [weak self] note in
            guard let raw = note.object as? String else { return }
            let parts = raw.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
            let text = parts.count > 1 ? String(parts[1]) : ""
            MainActor.assumeIsolated { self?.play(String(parts[0]), text) }
        }

        watcher.onUpdate = { [weak self] snapshot, events in
            MainActor.assumeIsolated { self?.react(snapshot, events) }
        }
        watcher.onLastWindowClosed = { NSApp.terminate(nil) }   // back with the next hiver window
        if watching { watcher.start() } else { hiverItem.title = "hiver: not watching" }


        // Appear: fade in while the sections settle into a complete H, then nod, glow and welcome
        window.alphaValue = 0
        window.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { $0.duration = 0.7; window.animator().alphaValue = 1 }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
            self?.pet.complete()
            self?.say("Welcome. I'm Hiver.")
        }
    }

    func say(_ text: String, aloud: Bool = true) {
        speech?.terminate()
        hideBubble?.cancel()
        bubble.text = text
        guard aloud && speakAloud else { scheduleHide(after: 2.5); return }
        pet.talking = true

        // `say` reads text from stdin, so text starting with "-" is never parsed as a flag
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/say")
        if let voice = UserDefaults.standard.string(forKey: "voice") { p.arguments = ["-v", voice] }
        let pipe = Pipe()
        p.standardInput = pipe
        p.terminationHandler = { [weak self] proc in
            DispatchQueue.main.async { self?.finished(proc) }
        }
        speech = p
        do { try p.run() } catch { finished(p); return }
        pipe.fileHandleForWriting.write(Data(text.utf8))
        try? pipe.fileHandleForWriting.close()
    }

    private func finished(_ p: Process) {
        guard speech === p else { return }   // a newer utterance replaced this one
        speech = nil
        pet.talking = false
        scheduleHide(after: 2)
    }

    private func scheduleHide(after seconds: Double) {
        let work = DispatchWorkItem { [weak self] in self?.bubble.text = "" }
        hideBubble = work
        DispatchQueue.main.asyncAfter(deadline: .now() + seconds, execute: work)
    }

    /// One animation by name (event CLI), optionally saying `text`
    private func play(_ event: String, _ text: String) {
        switch event {
        case "assign": pet.assign()
        case "listen": pet.listen()
        case "think": pet.think()
        case "message": pet.message()
        case "complete": pet.complete()
        case "chat": chat.toggle(above: window); return   // same as clicking the pet
        default: return
        }
        if !text.isEmpty { say(text) }
    }

    /// hiver changed: act it out with the existing movements; one bubble per look, most urgent first
    private func react(_ snapshot: HiverSnapshot, _ events: [HiverEvent]) {
        guard watching else { return }
        pet.agentCount = (snapshot.working, snapshot.idle)
        if watcher.hiverPath == nil {
            hiverItem.title = "hiver: not installed"
        } else if snapshot.swarms.isEmpty {
            hiverItem.title = "hiver: nothing running"
        } else {
            hiverItem.title = "hiver: \(snapshot.status.count) agents, \(snapshot.working) working"
        }
        if snapshot.working > 0 { pet.think(for: 2.5) }   // renewed every look while agents work

        var line: (text: String, aloud: Bool, rank: Int)?
        func offer(_ text: String, aloud: Bool, rank: Int) {
            if line == nil || rank > line!.rank { line = (text, aloud, rank) }
        }
        var pulses = 0
        for event in events {
            switch event {
            case .launched(let slug):
                pet.assign()
                offer("\(slug) is starting.", aloud: true, rank: 3)
            case .needsYou(let names):
                pet.listen(for: 4)
                offer(needsYouLine(names), aloud: true, rank: 4)
            case .finished(let name):
                pet.complete()
                offer("\(name) finished.", aloud: false, rank: 1)
            case .message(let fromHuman, let replyFrom, let text):
                if fromHuman { pet.listen() }
                if let replyFrom { offer(replyLine(from: replyFrom, text: text), aloud: true, rank: 2) }
                // A few pulses, staggered so each one reads; a burst of messages stays calm
                if pulses < 3 {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.35 * Double(pulses)) { [weak self] in
                        self?.pet.message()
                    }
                    pulses += 1
                }
            }
        }
        if let line { say(line.text, aloud: line.aloud) }
    }

    private func needsYouLine(_ names: [String]) -> String {
        switch names.count {
        case 1: return "\(names[0]) needs you."
        case 2: return "\(names[0]) and \(names[1]) need you."
        default: return "\(names.count) agents need you."
        }
    }

    @objc func toggleVoice() {
        speakAloud.toggle()
        voiceItem.state = speakAloud ? .on : .off
    }

    @objc func toggleWatch() {
        watching.toggle()
        watchItem.state = watching ? .on : .off
        if watching {
            hiverItem.title = "hiver: looking…"
            watcher.start()
        } else {
            watcher.stop()
            pet.agentCount = (0, 0)
            hiverItem.title = "hiver: not watching"
        }
    }

    @objc func assignWork() { pet.assign() }
    @objc func listen() { pet.listen() }
    @objc func think() { pet.think() }
    @objc func sendMessage() { pet.message() }
    @objc func taskDone() { pet.complete() }

    /// Without hiver: remove any old login item and quit
    private func turnOff() {
        setLogin(false)
        NSApp.terminate(nil)
    }

    private func setLogin(_ on: Bool) {
        guard on else { try? FileManager.default.removeItem(at: launchAgent); return }
        let plist: [String: Any] = [
            "Label": "com.hiver.h",
            "ProgramArguments": ["/usr/bin/open", "-a", Bundle.main.bundlePath],
            "RunAtLoad": true,
        ]
        try? FileManager.default.createDirectory(at: launchAgent.deletingLastPathComponent(), withIntermediateDirectories: true)
        if let data = try? PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0) {
            try? data.write(to: launchAgent)
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let delegate = AppDelegate()
app.delegate = delegate
app.run()
