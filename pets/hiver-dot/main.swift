import AppKit

// Hiver — a dark graphite dot with mint agents riding a signal wave around it. Watches hiver:
// its dots are hiver's agents, and swarms, messages and finished work drive its moves.
// CLI:  hiver            → launch the pet
//       hiver say "hi"   → make the running pet say something
//       hiver agent|leave|swarm|pulse|done ["text"]   → play that move (and say the text)

let sayNote = Notification.Name("com.hiver.pet.say")
let eventNote = Notification.Name("com.hiver.pet.event")
let petEvents = ["agent", "leave", "swarm", "pulse", "done", "chat"]

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

// One Hiver per desktop
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
    static let bodyLight = NSColor(hex: 0x56636a)
    static let body = NSColor(hex: 0x1f2529)
    static let bodyDark = NSColor(hex: 0x08090b)
    static let eye = NSColor(hex: 0xd6fff0)
    static let mint = NSColor(hex: 0x4fd1a5)
    static let mintGlow = NSColor(hex: 0x9cf5d4)
    static let ink = NSColor(hex: 0x101417)
    static let text = NSColor(hex: 0xe6f4ee)
}

func lerp(_ a: CGFloat, _ b: CGFloat, _ k: CGFloat) -> CGFloat { a + (b - a) * k }
func lerp(_ a: NSPoint, _ b: NSPoint, _ k: CGFloat) -> NSPoint { NSPoint(x: lerp(a.x, b.x, k), y: lerp(a.y, b.y, k)) }
func easeInOut(_ k: CGFloat) -> CGFloat { k * k * (3 - 2 * k) }
func easeOutBack(_ k: CGFloat) -> CGFloat {
    let c = 1.9, x = Double(k) - 1
    return CGFloat(1 + (c + 1) * x * x * x + c * x * x)
}

// MARK: - Pet

final class PetView: NSView {
    struct Agent { var phase: CGFloat; let born: Double; var leaving = -1.0 }
    struct Pulse { let from: Int, to: Int; let start: Double }

    // Geometry (non-flipped view coordinates)
    let center = NSPoint(x: 120, y: 82)
    let radius: CGFloat = 28
    let orbit = (rx: CGFloat(74), ry: CGFloat(22), tilt: CGFloat(-0.12))

    var talking = false
    var onPoke: (() -> Void)?
    /// hiver agents are working: messages flow between the dots more often
    var busy = false

    private var t = 0.0
    private var spin: CGFloat = 0
    private var agents: [Agent] = []
    private var pulses: [Pulse] = []
    private var nextPulse = 2.0
    private var formation: CGFloat = 0
    private var formationUntil = -1.0
    private var bounceStart = -10.0
    private var glowUntil = -1.0
    private var nextBlink = 2.5
    private var blinkUntil = 0.0
    private var mouthOpen = false
    private var nextMouth = 0.0
    private var look = NSPoint.zero

    private var dragStart: NSPoint?
    private var windowStart = NSPoint.zero
    private var moved = false

    override init(frame: NSRect) {
        super.init(frame: frame)
        for _ in 0..<3 { addAgent(animated: false) }
        let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    required init?(coder: NSCoder) { fatalError() }

    // MARK: Behaviors

    func addAgent(animated: Bool = true) {
        guard agents.filter({ $0.leaving < 0 }).count < 9 else { return }
        // Arrive at the gap after the last dot; phases then spread out evenly
        let phase = agents.last.map { $0.phase + .pi / CGFloat(agents.count) } ?? 0
        agents.append(Agent(phase: phase, born: animated ? t : -10))
    }

    /// An agent left: its dot fades off the wave
    func removeAgent() {
        guard let i = agents.lastIndex(where: { $0.leaving < 0 }) else { return }
        agents[i].leaving = t
    }

    /// Dots on the wave = hiver's agents (max 9); new ones fly out, gone ones fade off
    func showAgents(_ count: Int) {
        let goal = min(count, 9)
        var present = agents.filter { $0.leaving < 0 }.count
        while present < goal { addAgent(); present += 1 }
        while present > goal { removeAgent(); present -= 1 }
    }

    /// One message: a pulse of light between two dots
    func pulse() {
        let live = agents.indices.filter { agents[$0].leaving < 0 }
        guard live.count >= 2 else { return }
        let pair = live.shuffled().prefix(2)
        pulses.append(Pulse(from: pair.first!, to: pair.last!, start: t))
    }

    func formSwarm(for seconds: Double = 5) {
        formationUntil = t + seconds
        nextPulse = t + 0.6
    }

    func celebrate() {
        bounceStart = t
        glowUntil = t + 1.4
    }

    private var happy: Bool { t < glowUntil }

    // MARK: Animation

    private func tick() {
        guard window?.isVisible == true else { return }   // hidden: skip animation work
        let dt = 1.0 / 60
        t += dt
        let before = agents.count
        agents.removeAll { $0.leaving >= 0 && t - $0.leaving > 0.8 }   // faded off the wave
        if agents.count != before { pulses.removeAll() }                 // their dot indices moved

        let target: CGFloat = t < formationUntil ? 1 : 0
        formation += (target - formation) * CGFloat(min(1, dt * 2.5))
        spin += CGFloat(dt) * 0.55 * (1 - formation)

        // Spread the agents evenly around the orbit
        let n = CGFloat(agents.count)
        for i in agents.indices {
            let goal = 2 * .pi * CGFloat(i) / n
            var d = (goal - agents[i].phase).truncatingRemainder(dividingBy: 2 * .pi)
            if d > .pi { d -= 2 * .pi } else if d < -.pi { d += 2 * .pi }
            agents[i].phase += d * CGFloat(dt) * 2.5
        }

        // Messages between agents: frequent in formation, occasional while idle
        pulses.removeAll { t - $0.start > 0.7 }
        if t >= nextPulse && agents.count >= 2 {
            let a = Int.random(in: 0..<agents.count)
            var b = Int.random(in: 0..<agents.count - 1)
            if b >= a { b += 1 }
            pulses.append(Pulse(from: a, to: b, start: t))
            nextPulse = t + (formation > 0.5 ? Double.random(in: 0.25...0.6)
                             : busy ? Double.random(in: 0.8...1.6) : Double.random(in: 3...7))
        }

        if t >= nextBlink {
            blinkUntil = t + 0.13
            nextBlink = t + Double.random(in: 2.5...5.5)
        }

        if talking {
            if t >= nextMouth {
                mouthOpen.toggle()
                nextMouth = t + Double.random(in: 0.08...0.18)
            }
        } else {
            mouthOpen = false
        }

        // Curious eyes follow the cursor
        if let window {
            let m = NSEvent.mouseLocation
            let c = window.convertPoint(toScreen: convert(center, to: nil))
            let dx = m.x - c.x, dy = m.y - c.y
            let dist = max(1, hypot(dx, dy))
            let reach = min(1, dist / 260) * 2.6
            let goal = NSPoint(x: dx / dist * reach, y: dy / dist * reach)
            look = lerp(look, goal, 0.12)
        }

        needsDisplay = true
    }

    private var bounceHeight: CGFloat {
        let k = CGFloat((t - bounceStart) / 0.9)
        guard k >= 0 && k < 1 else { return 0 }
        return 6 * sin(k * .pi)
    }

    /// Point on the signal track: a tilted ellipse whose radius carries a travelling sine wave
    private func signalPoint(_ theta: CGFloat) -> NSPoint {
        let wave = 5 * sin(theta * 7 - CGFloat(t) * 2.6)   // pushes outward from the loop
        let ox = (orbit.rx + wave) * cos(theta), oy = (orbit.ry + wave * 0.55) * sin(theta)
        return NSPoint(x: center.x + ox * cos(orbit.tilt) - oy * sin(orbit.tilt),
                       y: center.y + ox * sin(orbit.tilt) + oy * cos(orbit.tilt))
    }

    /// Screen position of agent i plus its depth (+1 = far side of the track, -1 = near side).
    private func agentPosition(_ i: Int) -> (point: NSPoint, depth: CGFloat) {
        let a = agents[i]
        let theta = spin + a.phase
        let orbitPoint = signalPoint(theta)

        // Formation: a gentle arc above Hiver, bobbing in sync
        let n = CGFloat(max(agents.count - 1, 1))
        let arc = agents.count == 1 ? CGFloat.pi / 2 : CGFloat.pi * (0.88 - 0.76 * CGFloat(i) / n)
        let formPoint = NSPoint(x: center.x + 66 * cos(arc),
                                y: center.y + 10 + 46 * sin(arc) + 2.5 * CGFloat(sin(t * 4 + Double(i) * 0.7)))

        let f = easeInOut(formation)
        var p = lerp(orbitPoint, formPoint, f)
        let born = CGFloat((t - a.born) / 0.55)
        if born < 1 { p = lerp(center, p, easeInOut(max(0, born))) }   // newborns fly out of Hiver
        return (p, lerp(sin(theta), -0.5, f))
    }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        let ctx = NSGraphicsContext.current!.cgContext
        let hop = bounceHeight
        let bodyCenter = NSPoint(x: center.x, y: center.y + hop + 1.5 * CGFloat(sin(t * 1.6)))

        // Soft floor shadow, smaller while in the air
        let sw = 46 - hop * 1.2
        NSColor(white: 0, alpha: 0.24 - hop * 0.012).setFill()
        NSBezierPath(ovalIn: NSRect(x: center.x - sw / 2, y: center.y - radius - 14, width: sw, height: 7)).fill()

        let positions = agents.indices.map { agentPosition($0) }
        drawOrbitPath(back: true)
        for i in agents.indices where positions[i].depth > 0 { drawAgent(i, positions[i]) }
        drawBody(at: bodyCenter, hop: hop, ctx: ctx)
        drawOrbitPath(back: false)
        for i in agents.indices where positions[i].depth <= 0 { drawAgent(i, positions[i]) }
        for p in pulses where p.from < agents.count && p.to < agents.count {
            drawPulse(p, from: positions[p.from].point, to: positions[p.to].point, ctx: ctx)
        }
    }

    private func drawOrbitPath(back: Bool) {
        let alpha = 0.5 * (1 - formation)
        guard alpha > 0.01 else { return }
        let start: CGFloat = back ? 0 : .pi
        func trace(_ from: CGFloat, _ to: CGFloat) -> NSBezierPath {
            let path = NSBezierPath()
            let steps = max(2, Int((to - from) * 40))
            for s in 0...steps {
                let p = signalPoint(from + (to - from) * CGFloat(s) / CGFloat(steps))
                s == 0 ? path.move(to: p) : path.line(to: p)
            }
            path.lineCapStyle = .round
            path.lineJoinStyle = .round
            return path
        }

        NSGraphicsContext.saveGraphicsState()
        let glow = NSShadow()
        glow.shadowColor = Palette.mint.withAlphaComponent(0.8)
        glow.shadowBlurRadius = 5
        glow.set()
        let line = trace(start, start + .pi)
        line.lineWidth = 1.1
        Palette.mint.withAlphaComponent(alpha * (back ? 0.55 : 1)).setStroke()
        line.stroke()

        // A brighter signal packet travelling along the wave
        let head = CGFloat(t * 1.3).truncatingRemainder(dividingBy: 2 * .pi)
        let from = max(start, head - 0.55), to = min(start + .pi, head)
        if to > from {
            let packet = trace(from, to)
            packet.lineWidth = 1.8
            glow.shadowColor = Palette.mintGlow
            glow.shadowBlurRadius = 8
            glow.set()
            Palette.mintGlow.withAlphaComponent(min(1, alpha * 1.8) * (back ? 0.6 : 1)).setStroke()
            packet.stroke()
        }
        NSGraphicsContext.restoreGraphicsState()
    }

    private func drawAgent(_ i: Int, _ pos: (point: NSPoint, depth: CGFloat)) {
        let born = CGFloat((t - agents[i].born) / 0.55)
        let leave = agents[i].leaving < 0 ? 0 : CGFloat((t - agents[i].leaving) / 0.8)
        let scale = (born < 1 ? max(0, easeOutBack(born)) : 1) * max(0, 1 - leave)
        let r = 4.8 * (1 - 0.22 * pos.depth) * scale
        guard r > 0.2 else { return }

        let glowing = happy || born < 1
        let shine = glowing ? 0.5 + 0.5 * sin(t * 10) : 0
        let fill = Palette.mint.blended(withFraction: 0.25 + 0.5 * shine, of: Palette.mintGlow)!
            .withAlphaComponent(0.8 - 0.15 * pos.depth)

        NSGraphicsContext.saveGraphicsState()
        let glow = NSShadow()
        glow.shadowColor = (glowing ? Palette.mintGlow : Palette.mint).withAlphaComponent(glowing ? 1 : 0.8)
        glow.shadowBlurRadius = glowing ? 12 : 6
        glow.set()
        fill.setFill()
        NSBezierPath(ovalIn: NSRect(x: pos.point.x - r, y: pos.point.y - r, width: r * 2, height: r * 2)).fill()
        NSGraphicsContext.restoreGraphicsState()

        // Tiny highlight
        NSColor.white.withAlphaComponent(0.45).setFill()
        let h = r * 0.38
        NSBezierPath(ovalIn: NSRect(x: pos.point.x - r * 0.45, y: pos.point.y + r * 0.15, width: h, height: h)).fill()

        // Birth ring
        if born < 1 {
            let ring = r + 14 * born
            Palette.mintGlow.withAlphaComponent(0.8 * (1 - born)).setStroke()
            let p = NSBezierPath(ovalIn: NSRect(x: pos.point.x - ring, y: pos.point.y - ring, width: ring * 2, height: ring * 2))
            p.lineWidth = 1.2
            p.stroke()
        }
    }

    private func drawPulse(_ p: Pulse, from a: NSPoint, to b: NSPoint, ctx: CGContext) {
        let k = CGFloat((t - p.start) / 0.7)
        let e = easeInOut(k)
        for trail in 0..<4 {
            let kk = max(0, e - CGFloat(trail) * 0.06)
            let pt = NSPoint(x: lerp(a.x, b.x, kk), y: lerp(a.y, b.y, kk) + 10 * sin(kk * .pi))   // slight arc
            let r = 2.2 - CGFloat(trail) * 0.45
            NSGraphicsContext.saveGraphicsState()
            let glow = NSShadow()
            glow.shadowColor = Palette.mintGlow
            glow.shadowBlurRadius = 8
            glow.set()
            NSColor.white.withAlphaComponent((1 - CGFloat(trail) * 0.22) * sin(k * .pi)).setFill()
            NSBezierPath(ovalIn: NSRect(x: pt.x - r, y: pt.y - r, width: r * 2, height: r * 2)).fill()
            NSGraphicsContext.restoreGraphicsState()
        }
    }

    private func drawBody(at c: NSPoint, hop: CGFloat, ctx: CGContext) {
        // Breathing plus a little stretch while airborne
        let breath = 1 + 0.015 * CGFloat(sin(t * 1.6))
        let rx = radius * breath * (1 - hop * 0.003)
        let ry = radius * breath * (1 + hop * 0.005)
        let rect = NSRect(x: c.x - rx, y: c.y - ry, width: rx * 2, height: ry * 2)

        NSGraphicsContext.saveGraphicsState()
        let glow = NSShadow()
        glow.shadowColor = Palette.mint.withAlphaComponent(happy ? 0.95 : 0.35)
        glow.shadowBlurRadius = happy ? 18 : 9
        glow.set()
        Palette.body.setFill()
        NSBezierPath(ovalIn: rect).fill()
        NSGraphicsContext.restoreGraphicsState()

        // Satin shading: key light upper-left, mint bounce light on the underside, soft sheen
        let space = CGColorSpace(name: CGColorSpace.sRGB)!
        func grad(_ cs: [NSColor], _ locs: [CGFloat]) -> CGGradient {
            CGGradient(colorsSpace: space, colors: cs.map(\.cgColor) as CFArray, locations: locs)!
        }
        ctx.saveGState()
        ctx.addEllipse(in: rect)
        ctx.clip()
        let key = CGPoint(x: c.x - rx * 0.38, y: c.y + ry * 0.42)
        ctx.drawRadialGradient(grad([Palette.bodyLight, Palette.body, Palette.bodyDark], [0, 0.5, 1]),
                               startCenter: key, startRadius: 0, endCenter: c, endRadius: rx * 1.1, options: [.drawsAfterEndLocation])
        let bounce = CGPoint(x: c.x + rx * 0.2, y: c.y - ry * 1.15)
        ctx.drawRadialGradient(grad([Palette.mint.withAlphaComponent(0.28), Palette.mint.withAlphaComponent(0)], [0, 1]),
                               startCenter: bounce, startRadius: 0, endCenter: bounce, endRadius: rx * 0.8, options: [])
        ctx.drawRadialGradient(grad([NSColor.white.withAlphaComponent(0.22), NSColor.white.withAlphaComponent(0)], [0, 1]),
                               startCenter: key, startRadius: 0, endCenter: key, endRadius: rx * 0.5, options: [])
        ctx.restoreGState()

        // Eyes
        let eyeY = c.y + 3 + look.y
        for side: CGFloat in [-1, 1] {
            let ex = c.x + side * 9.5 + look.x
            NSGraphicsContext.saveGraphicsState()
            let eyeGlow = NSShadow()
            eyeGlow.shadowColor = Palette.mint.withAlphaComponent(0.8)
            eyeGlow.shadowBlurRadius = 5
            eyeGlow.set()
            Palette.eye.setFill()
            if t < blinkUntil {
                NSBezierPath(roundedRect: NSRect(x: ex - 3.4, y: eyeY - 0.8, width: 6.8, height: 1.6),
                             xRadius: 0.8, yRadius: 0.8).fill()
            } else {
                NSBezierPath(roundedRect: NSRect(x: ex - 3, y: eyeY - 4.4, width: 6, height: 8.8), xRadius: 3, yRadius: 3).fill()
            }
            NSGraphicsContext.restoreGraphicsState()
        }

        // Mouth only while speaking
        guard talking else { return }
        let mx = c.x + look.x * 0.6, my = eyeY - 9
        Palette.eye.setFill()
        let mh: CGFloat = mouthOpen ? 3.4 : 1.4
        NSBezierPath(roundedRect: NSRect(x: mx - 2.4, y: my - mh / 2, width: 4.8, height: mh),
                     xRadius: mh / 2, yRadius: mh / 2).fill()
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
        .foregroundColor: Palette.text,
    ]

    override func draw(_ dirtyRect: NSRect) {
        guard !text.isEmpty else { return }
        let maxW = bounds.width - 40
        let str = NSAttributedString(string: text, attributes: attrs)
        let tb = str.boundingRect(with: NSSize(width: maxW, height: 1000), options: .usesLineFragmentOrigin)
        let textH = min(ceil(tb.height), bounds.height - 30)
        let w = ceil(tb.width) + 28, h = textH + 16
        let box = NSRect(x: (bounds.width - w) / 2, y: bounds.height - 12 - h, width: w, height: h)

        let shape = NSBezierPath(roundedRect: box, xRadius: 7, yRadius: 7)
        let tail = NSBezierPath()
        tail.move(to: NSPoint(x: bounds.midX - 7, y: box.maxY - 1))
        tail.line(to: NSPoint(x: bounds.midX, y: bounds.height - 2))
        tail.line(to: NSPoint(x: bounds.midX + 7, y: box.maxY - 1))
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

        str.draw(with: NSRect(x: box.minX + 14, y: box.minY + 8, width: maxW, height: textH),
                 options: [.usesLineFragmentOrigin, .truncatesLastVisibleLine])
    }
}

// MARK: - App

final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow!
    let pet = PetView(frame: NSRect(x: 0, y: 0, width: 240, height: 170))
    let bubble = BubbleView(frame: NSRect(x: 0, y: 150, width: 240, height: 90))
    private var speech: Process?
    private var hideBubble: DispatchWorkItem?
    private var watchItem: NSMenuItem!
    private var voiceItem: NSMenuItem!
    private let hiverItem = NSMenuItem(title: "hiver: looking…", action: nil, keyEquivalent: "")
    private let watcher = HiverWatcher()
    private let chat = HiverChat()
    private lazy var switcher = PetSwitcher(current: "hiver-dot", say: { [weak self] in self?.say($0, aloud: false) },
                                            turnOffHere: { [weak self] in self?.turnOff() })
    private var watching: Bool {
        get { UserDefaults.standard.object(forKey: "watchHiver") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "watchHiver") }
    }
    private var speakAloud: Bool {
        get { UserDefaults.standard.object(forKey: "speakAloud") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "speakAloud") }
    }

    private let launchAgent = URL(fileURLWithPath: NSHomeDirectory() + "/Library/LaunchAgents/com.hiver.pet.plist")

    func applicationDidFinishLaunching(_ notification: Notification) {
        let size = NSSize(width: 240, height: 240)
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

        if !window.setFrameUsingName("HiverPet"), let screen = NSScreen.main {
            let v = screen.visibleFrame
            window.setFrameOrigin(NSPoint(x: v.maxX - size.width - 260, y: v.minY + 10))
        }
        window.setContentSize(size)
        window.setFrameAutosaveName("HiverPet")

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
        // Preview the pet's moves: they only animate the pet, nothing happens in hiver
        let effects = NSMenu()
        effects.addItem(withTitle: "New agent", action: #selector(newAgent), keyEquivalent: "").target = self
        effects.addItem(withTitle: "Swarm formation", action: #selector(swarm), keyEquivalent: "").target = self
        effects.addItem(withTitle: "Task finished", action: #selector(taskDone), keyEquivalent: "").target = self
        let effectsItem = NSMenuItem(title: "Effects (preview only)", action: nil, keyEquivalent: "")
        effectsItem.submenu = effects
        menu.addItem(effectsItem)
        menu.addItem(.separator())
        switcher.addItems(to: menu)
        menu.addItem(withTitle: "Quit Hiver", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "")
        pet.menu = menu
        // Click: talk to the hiver agent (a chat box above the pet); drag still moves it
        pet.onPoke = { [weak self] in
            guard let self else { return }
            pet.celebrate()
            chat.toggle(above: window)
        }
        chat.say = { [weak self] text, aloud in self?.say(text, aloud: aloud) }
        chat.onSent = { [weak self] in self?.pet.pulse() }

        // `hiver agent|leave|swarm|pulse|done ["text"]`
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

        DistributedNotificationCenter.default().addObserver(forName: sayNote, object: nil, queue: .main) { [weak self] note in
            guard let text = note.object as? String, !text.isEmpty else { return }
            MainActor.assumeIsolated { self?.say(text) }
        }


        // Appear: fade in, happy bounce, welcome
        window.alphaValue = 0
        window.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { $0.duration = 0.7; window.animator().alphaValue = 1 }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in
            self?.pet.celebrate()
            self?.say("Welcome! I'm Hiver.")
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

    /// One move by name (event CLI), optionally saying `text`
    private func play(_ event: String, _ text: String) {
        switch event {
        case "agent": pet.addAgent()
        case "leave": pet.removeAgent()
        case "swarm": pet.formSwarm()
        case "pulse": pet.pulse()
        case "done": pet.celebrate()
        case "chat": chat.toggle(above: window); return   // same as clicking the pet
        default: return
        }
        if !text.isEmpty { say(text) }
    }

    /// hiver changed: the dots are its agents; one line per look, most urgent first
    private func react(_ snapshot: HiverSnapshot, _ events: [HiverEvent]) {
        guard watching else { return }
        if watcher.hiverPath == nil {
            hiverItem.title = "hiver: not installed"
        } else if snapshot.swarms.isEmpty {
            hiverItem.title = "hiver: nothing running"
        } else {
            hiverItem.title = "hiver: \(snapshot.status.count) agents, \(snapshot.working) working"
            pet.showAgents(snapshot.status.count)
        }
        pet.busy = snapshot.working > 0

        var line: (text: String, aloud: Bool, rank: Int)?
        func offer(_ text: String, aloud: Bool, rank: Int) {
            if line == nil || rank > line!.rank { line = (text, aloud, rank) }
        }
        var pulses = 0
        for event in events {
            switch event {
            case .launched(let slug):
                pet.formSwarm()
                offer("\(slug) is starting.", aloud: true, rank: 3)
            case .needsYou(let names):
                offer(needsYouLine(names), aloud: true, rank: 4)
            case .finished(let name):
                pet.celebrate()
                offer("\(name) finished.", aloud: false, rank: 1)
            case .message(let fromHuman, let replyFrom, let text):
                if fromHuman { offer("On it.", aloud: false, rank: 0) }
                if let replyFrom { offer(replyLine(from: replyFrom, text: text), aloud: true, rank: 2) }
                if pulses < 3 {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.3 * Double(pulses)) { [weak self] in
                        self?.pet.pulse()
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
            pet.busy = false
            hiverItem.title = "hiver: not watching"
        }
    }

    /// Without hiver: remove any old login item and quit
    private func turnOff() {
        setLogin(false)
        NSApp.terminate(nil)
    }

    @objc func newAgent() { pet.addAgent() }
    @objc func swarm() { pet.formSwarm() }
    @objc func taskDone() { pet.celebrate() }

    private func setLogin(_ on: Bool) {
        guard on else { try? FileManager.default.removeItem(at: launchAgent); return }
        let plist: [String: Any] = [
            "Label": "com.hiver.pet",
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
