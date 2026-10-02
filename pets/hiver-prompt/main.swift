import AppKit

// Hiver Prompt — a satin charcoal-teal sphere with a mint `>_` face, leading three agent spheres.
// Watches hiver: its swarms, agents and messages drive attend / work / emit / complete.
// CLI:  hiver-prompt            → launch the pet
//       hiver-prompt say "hi"   → make the running pet say something
//       hiver-prompt attend|work|emit|complete ["text"]   → play that move (and say the text)

let sayNote = Notification.Name("com.hiver.prompt.say")
let eventNote = Notification.Name("com.hiver.prompt.event")
let petEvents = ["attend", "work", "emit", "complete"]

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

// One Hiver Prompt per desktop
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

/// Shading ramp for a satin sphere lit from the upper left
struct Tone {
    let light: NSColor, base: NSColor, dark: NSColor, bounce: NSColor

    /// Push toward the background haze (for spheres further away)
    func dimmed(_ k: CGFloat) -> Tone {
        let haze = NSColor(hex: 0x1a2426)
        return Tone(light: light.blended(withFraction: k, of: haze)!, base: base.blended(withFraction: k, of: haze)!,
                    dark: dark.blended(withFraction: k * 0.5, of: haze)!, bounce: bounce.blended(withFraction: k, of: haze)!)
    }
}

enum Palette {
    static let mint = NSColor(hex: 0x8ff0cc)
    static let mintGlow = NSColor(hex: 0xc4ffea)
    static let ink = NSColor(hex: 0x14191b)
    static let paper = NSColor(hex: 0xfafcfb)

    static let hiver = Tone(light: NSColor(hex: 0x5d8c88), base: NSColor(hex: 0x24403f),
                            dark: NSColor(hex: 0x0a1314), bounce: NSColor(hex: 0x3f7a6c))
    static let emerald = Tone(light: NSColor(hex: 0x8af0c4), base: NSColor(hex: 0x23a874),
                              dark: NSColor(hex: 0x0b4a33), bounce: NSColor(hex: 0x5fd6a6))
    static let mutedMint = Tone(light: NSColor(hex: 0xd6f2e6), base: NSColor(hex: 0x86bfa9),
                                dark: NSColor(hex: 0x345a4c), bounce: NSColor(hex: 0xb4e2cf))
}

func lerp(_ a: CGFloat, _ b: CGFloat, _ k: CGFloat) -> CGFloat { a + (b - a) * k }
func easeInOut(_ k: CGFloat) -> CGFloat { k * k * (3 - 2 * k) }

let sRGB = CGColorSpace(name: CGColorSpace.sRGB)!

func gradient(_ colors: [NSColor], _ locations: [CGFloat]) -> CGGradient {
    CGGradient(colorsSpace: sRGB, colors: colors.map(\.cgColor) as CFArray, locations: locations)!
}

/// Satin sphere: directional key light, dark falloff, bounce light on the underside, soft specular
func drawSphere(_ ctx: CGContext, _ c: CGPoint, _ r: CGFloat, _ tone: Tone) {
    ctx.saveGState()
    ctx.addEllipse(in: CGRect(x: c.x - r, y: c.y - r, width: r * 2, height: r * 2))
    ctx.clip()

    ctx.drawRadialGradient(gradient([tone.light, tone.base, tone.dark], [0, 0.48, 1]),
                           startCenter: CGPoint(x: c.x - r * 0.38, y: c.y + r * 0.42), startRadius: 0,
                           endCenter: c, endRadius: r * 1.1, options: [.drawsAfterEndLocation])

    // Light bouncing up from the floor onto the lower edge
    ctx.drawRadialGradient(gradient([tone.bounce.withAlphaComponent(0.55), tone.bounce.withAlphaComponent(0)], [0, 1]),
                           startCenter: CGPoint(x: c.x + r * 0.2, y: c.y - r * 1.15), startRadius: 0,
                           endCenter: CGPoint(x: c.x + r * 0.2, y: c.y - r * 1.15), endRadius: r * 0.85, options: [])

    // Broad satin sheen + small crisp highlight
    let spec = CGPoint(x: c.x - r * 0.36, y: c.y + r * 0.46)
    ctx.drawRadialGradient(gradient([NSColor.white.withAlphaComponent(0.28), NSColor.white.withAlphaComponent(0)], [0, 1]),
                           startCenter: spec, startRadius: 0, endCenter: spec, endRadius: r * 0.55, options: [])
    ctx.drawRadialGradient(gradient([NSColor.white.withAlphaComponent(0.55), NSColor.white.withAlphaComponent(0)], [0, 1]),
                           startCenter: spec, startRadius: 0, endCenter: spec, endRadius: r * 0.16, options: [])
    ctx.restoreGState()
}

/// Soft contact shadow on the floor
func drawShadow(_ ctx: CGContext, _ c: CGPoint, width: CGFloat, alpha: CGFloat) {
    ctx.saveGState()
    ctx.translateBy(x: c.x, y: c.y)
    ctx.scaleBy(x: 1, y: 0.22)
    ctx.drawRadialGradient(gradient([NSColor(white: 0, alpha: alpha), NSColor(white: 0, alpha: 0)], [0, 1]),
                           startCenter: .zero, startRadius: 0, endCenter: .zero, endRadius: width / 2, options: [])
    ctx.restoreGState()
}

// MARK: - Pet

final class PetView: NSView {
    enum Mode { case idle, gather, swarm }

    struct Agent {
        let tone: Tone
        let size: CGFloat
        let seed: Double
        var pos: CGPoint
        var z: CGFloat               // -1 = behind Hiver, +1 = nearer the viewer
        var vel = CGVector.zero
        var vz: CGFloat = 0
    }
    struct Pulse { let from: Int, to: Int; let start: Double }   // index -1 = Hiver

    let center = CGPoint(x: 120, y: 74)
    let radius: CGFloat = 28
    private var floorY: CGFloat { center.y - radius - 9 }

    // Target spots per mode: (x, y, z) for each of the three agents
    private let spots: [Mode: [(CGFloat, CGFloat, CGFloat)]] = [
        .idle: [(68, 54, 0.75), (178, 104, -0.65), (52, 108, -0.25)],
        .gather: [(90, 50, 0.85), (152, 96, -0.55), (86, 100, -0.45)],
        .swarm: [(36, 80, 0.2), (204, 80, 0.2), (120, 132, -0.7)],
    ]

    var talking = false
    var onPoke: (() -> Void)?

    private var t = 0.0
    private var mode = Mode.idle
    private var modeUntil = 0.0
    private var agents: [Agent] = []
    private var pulses: [Pulse] = []
    private var nextPulse = 4.0
    private var nodStart = -10.0
    private var glowUntil = -1.0
    private var tilt: CGFloat = 0
    private var typing: CGFloat = 1

    private var dragStart: NSPoint?
    private var windowStart = NSPoint.zero
    private var moved = false

    override init(frame: NSRect) {
        super.init(frame: frame)
        let tones = [Palette.emerald, Palette.mutedMint, Palette.emerald]
        let sizes: [CGFloat] = [9, 8, 7.5]
        for (i, s) in spots[.idle]!.enumerated() {
            agents.append(Agent(tone: tones[i], size: sizes[i], seed: Double(i) * 2.1, pos: CGPoint(x: s.0, y: s.1), z: s.2))
        }
        let timer = Timer(timeInterval: 1.0 / 60, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated { self?.tick() }
        }
        RunLoop.main.add(timer, forMode: .common)
    }

    required init?(coder: NSCoder) { fatalError() }

    // MARK: Behaviors

    /// Receiving instructions: Hiver tilts in attention, agents gather close
    func attend(for seconds: Double = 3) { setMode(.gather, for: seconds) }

    /// Swarm at work: agents spread into formation, messages flow, cursor pulses
    func work(for seconds: Double = 8) {
        setMode(.swarm, for: seconds)
        nextPulse = t + 0.6
    }

    /// hiver agents are working: stay in formation (an attention gather is not interrupted)
    func keepWorking() {
        switch mode {
        case .gather: return
        case .swarm: modeUntil = max(modeUntil, t + 2.5)
        case .idle: work(for: 2.5)
        }
    }

    /// One message: a single pulse between two agents (or an agent and Hiver)
    func pulse() {
        let a = Int.random(in: 0..<agents.count)
        var b = Int.random(in: -1..<agents.count - 1)
        if b >= a { b += 1 }
        pulses.append(Pulse(from: a, to: b, start: t))
    }

    /// Dispatch: a message pulse from Hiver to each agent
    func emit() {
        for i in agents.indices { pulses.append(Pulse(from: -1, to: i, start: t + Double(i) * 0.12)) }
    }

    /// Task complete: agents report in, Hiver gives a restrained nod and a brief glow
    func complete() {
        setMode(.idle, for: 0)
        for i in agents.indices { pulses.append(Pulse(from: i, to: -1, start: t + Double(i) * 0.1)) }
        nodStart = t + 0.35
        glowUntil = t + 1.5
    }

    private func setMode(_ m: Mode, for seconds: Double) {
        mode = m
        modeUntil = t + seconds
    }

    private var working: Bool { mode == .swarm }

    // MARK: Animation

    private func tick() {
        guard window?.isVisible == true else { return }   // hidden: skip animation work
        let dt = 1.0 / 60
        t += dt
        if mode != .idle && t >= modeUntil { mode = .idle }

        tilt += ((mode == .gather ? -0.13 : 0) - tilt) * CGFloat(min(1, dt * 5))

        // Deliberate spring toward each agent's spot, plus a quiet hover
        let targets = spots[mode]!
        for i in agents.indices {
            var a = agents[i]
            let s = targets[i]
            let sync = mode == .swarm ? sin(t * 2.4) * 2 : sin(t * 1.1 + a.seed) * 2.4
            let goal = CGPoint(x: s.0 + CGFloat(sin(t * 0.7 + a.seed * 1.7)) * (mode == .swarm ? 0 : 1.5),
                               y: s.1 + CGFloat(sync))
            let k: CGFloat = 9, damping = CGFloat(exp(-6.0 * dt))
            a.vel.dx = (a.vel.dx + (goal.x - a.pos.x) * k * CGFloat(dt)) * damping
            a.vel.dy = (a.vel.dy + (goal.y - a.pos.y) * k * CGFloat(dt)) * damping
            a.vz = (a.vz + (s.2 - a.z) * k * CGFloat(dt)) * damping
            a.pos.x += a.vel.dx * CGFloat(dt)
            a.pos.y += a.vel.dy * CGFloat(dt)
            a.z += a.vz * CGFloat(dt)
            agents[i] = a
        }

        // Agents talk: steady while the swarm works, occasional otherwise
        pulses.removeAll { t - $0.start > 0.7 }
        if t >= nextPulse {
            let a = Int.random(in: 0..<agents.count)
            var b = Int.random(in: -1..<agents.count - 1)
            if b >= a { b += 1 }
            pulses.append(Pulse(from: a, to: b, start: t))
            nextPulse = t + (working ? Double.random(in: 0.35...0.8) : Double.random(in: 4...8))
        }

        // While speaking, the `_` flickers like text being typed
        typing = talking ? CGFloat.random(in: 0.35...1.3) * 0.25 + typing * 0.75 : typing + (1 - typing) * 0.2

        needsDisplay = true
    }

    /// 0 → 1 → 0 over the nod
    private var nodAmount: CGFloat {
        let k = (t - nodStart) / 0.8
        guard k >= 0 && k < 1 else { return 0 }
        return CGFloat(sin(k * .pi))
    }

    /// Cursor opacity: slow blink at rest, steady pulse while working
    private var cursorAlpha: CGFloat {
        if working { return 0.55 + 0.45 * CGFloat(sin(t * 6)) }
        return t.truncatingRemainder(dividingBy: 1.6) < 1.0 ? 1 : 0.15
    }

    private func agentRadius(_ a: Agent) -> CGFloat { a.size * (1 + 0.18 * a.z) }

    // MARK: Drawing

    override func draw(_ dirtyRect: NSRect) {
        let ctx = NSGraphicsContext.current!.cgContext
        let rise = 1.8 * CGFloat(sin(t * 1.4))
        let c = CGPoint(x: center.x, y: center.y + rise - 2.5 * nodAmount)

        // Floor shadows: further agents sit higher on the floor plane
        drawShadow(ctx, CGPoint(x: center.x + 3, y: floorY), width: 66 - rise * 1.5, alpha: 0.42)
        for a in agents {
            let height = max(0, a.pos.y - floorY)
            drawShadow(ctx, CGPoint(x: a.pos.x + 2, y: floorY - a.z * 9), width: agentRadius(a) * 2.6,
                       alpha: 0.28 * (1 - min(1, height / 140)))
        }

        let order = agents.indices.sorted { agents[$0].z < agents[$1].z }
        for i in order where agents[i].z < 0 { drawAgent(ctx, agents[i]) }
        drawBody(ctx, at: c)
        for i in order where agents[i].z >= 0 { drawAgent(ctx, agents[i]) }

        for p in pulses where t >= p.start {
            let a = p.from < 0 ? c : agents[p.from].pos
            let b = p.to < 0 ? c : agents[p.to].pos
            drawPulse(p, from: a, to: b)
        }
    }

    private func drawAgent(_ ctx: CGContext, _ a: Agent) {
        drawSphere(ctx, a.pos, agentRadius(a), a.tone.dimmed(max(0, -a.z) * 0.35))
    }

    private func drawBody(_ ctx: CGContext, at c: CGPoint) {
        let glowing = t < glowUntil
        if glowing {
            // Brief mint glow around the body
            let k = CGFloat((glowUntil - t) / 1.5)
            ctx.saveGState()
            ctx.setShadow(offset: .zero, blur: 18, color: Palette.mint.withAlphaComponent(0.85 * k).cgColor)
            Palette.ink.setFill()
            NSBezierPath(ovalIn: NSRect(x: c.x - radius + 1, y: c.y - radius + 1, width: radius * 2 - 2, height: radius * 2 - 2)).fill()
            ctx.restoreGState()
        }
        drawSphere(ctx, c, radius, Palette.hiver)

        // `>_` face, tilting with attention and dipping with the nod
        ctx.saveGState()
        ctx.translateBy(x: c.x + tilt * 18, y: c.y - 1.5 * nodAmount)
        ctx.rotate(by: tilt)
        ctx.setShadow(offset: .zero, blur: 4, color: Palette.mint.withAlphaComponent(0.55).cgColor)

        Palette.mint.setStroke()
        let chevron = NSBezierPath()
        chevron.move(to: NSPoint(x: -11, y: 7))
        chevron.line(to: NSPoint(x: -3, y: 1))
        chevron.line(to: NSPoint(x: -11, y: -5))
        chevron.lineWidth = 3
        chevron.lineCapStyle = .round
        chevron.lineJoinStyle = .round
        chevron.stroke()

        Palette.mint.withAlphaComponent(talking ? 1 : cursorAlpha).setFill()
        NSBezierPath(roundedRect: NSRect(x: 1, y: -6.5, width: 11 * typing, height: 3), xRadius: 1.5, yRadius: 1.5).fill()
        ctx.restoreGState()
    }

    private func drawPulse(_ p: Pulse, from a: CGPoint, to b: CGPoint) {
        let k = CGFloat((t - p.start) / 0.7)
        let e = easeInOut(k)
        for trail in 0..<4 {
            let kk = max(0, e - CGFloat(trail) * 0.06)
            let pt = NSPoint(x: lerp(a.x, b.x, kk), y: lerp(a.y, b.y, kk) + 5 * sin(kk * .pi))
            let r = 2.1 - CGFloat(trail) * 0.42
            NSGraphicsContext.saveGraphicsState()
            let glow = NSShadow()
            glow.shadowColor = Palette.mintGlow
            glow.shadowBlurRadius = 7
            glow.set()
            NSColor.white.withAlphaComponent((1 - CGFloat(trail) * 0.22) * sin(k * .pi)).setFill()
            NSBezierPath(ovalIn: NSRect(x: pt.x - r, y: pt.y - r, width: r * 2, height: r * 2)).fill()
            NSGraphicsContext.restoreGraphicsState()
        }
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
    let pet = PetView(frame: NSRect(x: 0, y: 0, width: 240, height: 150))
    let bubble = BubbleView(frame: NSRect(x: 0, y: 134, width: 240, height: 80))
    private var speech: Process?
    private var hideBubble: DispatchWorkItem?
    private var loginItem: NSMenuItem!
    private var watchItem: NSMenuItem!
    private var voiceItem: NSMenuItem!
    private let hiverItem = NSMenuItem(title: "hiver: looking…", action: nil, keyEquivalent: "")
    private let watcher = HiverWatcher()
    private lazy var switcher = PetSwitcher(current: "hiver-prompt", say: { [weak self] in self?.say($0, aloud: false) },
                                            turnOffHere: { [weak self] in self?.turnOff() })
    private var watching: Bool {
        get { UserDefaults.standard.object(forKey: "watchHiver") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "watchHiver") }
    }
    private var speakAloud: Bool {
        get { UserDefaults.standard.object(forKey: "speakAloud") as? Bool ?? true }
        set { UserDefaults.standard.set(newValue, forKey: "speakAloud") }
    }

    private let pokeLines = ["Ready.", "Listening.", "Standing by.", "Terminal and Slack are in sync.", "What's next?"]

    private let launchAgent = URL(fileURLWithPath: NSHomeDirectory() + "/Library/LaunchAgents/com.hiver.prompt.plist")

    func applicationDidFinishLaunching(_ notification: Notification) {
        let size = NSSize(width: 240, height: 214)
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

        if !window.setFrameUsingName("HiverPromptPet"), let screen = NSScreen.main {
            let v = screen.visibleFrame
            window.setFrameOrigin(NSPoint(x: v.maxX - size.width - 520, y: v.minY + 10))
        }
        window.setContentSize(size)
        window.setFrameAutosaveName("HiverPromptPet")

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
        menu.addItem(withTitle: "Give instructions", action: #selector(instruct), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Start swarm", action: #selector(startWork), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Dispatch message", action: #selector(dispatch), keyEquivalent: "").target = self
        menu.addItem(withTitle: "Task complete", action: #selector(taskDone), keyEquivalent: "").target = self
        menu.addItem(.separator())
        switcher.addItems(to: menu)
        loginItem = menu.addItem(withTitle: "Open at Login", action: #selector(toggleLogin), keyEquivalent: "")
        loginItem.target = self
        menu.addItem(withTitle: "Quit Hiver Prompt", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "")
        pet.menu = menu
        pet.onPoke = { [weak self] in
            guard let self else { return }
            pet.attend()
            say(pokeLines.randomElement()!, aloud: false)
        }

        // `hiver-prompt attend|work|emit|complete ["text"]`
        DistributedNotificationCenter.default().addObserver(forName: eventNote, object: nil, queue: .main) { [weak self] note in
            guard let raw = note.object as? String else { return }
            let parts = raw.split(separator: "\n", maxSplits: 1, omittingEmptySubsequences: false)
            let text = parts.count > 1 ? String(parts[1]) : ""
            MainActor.assumeIsolated { self?.play(String(parts[0]), text) }
        }

        watcher.onUpdate = { [weak self] snapshot, events in
            MainActor.assumeIsolated { self?.react(snapshot, events) }
        }
        if watching { watcher.start() } else { hiverItem.title = "hiver: not watching" }

        DistributedNotificationCenter.default().addObserver(forName: sayNote, object: nil, queue: .main) { [weak self] note in
            guard let text = note.object as? String, !text.isEmpty else { return }
            MainActor.assumeIsolated {
                self?.pet.attend()
                self?.say(text)
            }
        }

        // First launch from an .app turns on Open at Login, so Hiver Prompt greets you on every startup
        if Bundle.main.bundlePath.hasSuffix(".app") && !UserDefaults.standard.bool(forKey: "loginConfigured") {
            UserDefaults.standard.set(true, forKey: "loginConfigured")
            setLogin(true)
        }
        loginItem.state = FileManager.default.fileExists(atPath: launchAgent.path) ? .on : .off

        // Appear: fade in, Hiver checks in with its team, welcome
        window.alphaValue = 0
        window.orderFrontRegardless()
        NSAnimationContext.runAnimationGroup { $0.duration = 0.7; window.animator().alphaValue = 1 }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in
            self?.pet.emit()
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

    /// One move by name (event CLI), optionally saying `text`
    private func play(_ event: String, _ text: String) {
        switch event {
        case "attend": pet.attend()
        case "work": pet.work()
        case "emit": pet.emit()
        case "complete": pet.complete()
        default: return
        }
        if !text.isEmpty { say(text) }
    }

    /// hiver changed: act it out with attend / work / emit / complete; one line per look
    private func react(_ snapshot: HiverSnapshot, _ events: [HiverEvent]) {
        guard watching else { return }
        if watcher.hiverPath == nil {
            hiverItem.title = "hiver: not installed"
        } else if snapshot.swarms.isEmpty {
            hiverItem.title = "hiver: nothing running"
        } else {
            hiverItem.title = "hiver: \(snapshot.status.count) agents, \(snapshot.working) working"
        }
        if snapshot.working > 0 { pet.keepWorking() }

        var line: (text: String, aloud: Bool, rank: Int)?
        func offer(_ text: String, aloud: Bool, rank: Int) {
            if line == nil || rank > line!.rank { line = (text, aloud, rank) }
        }
        var pulses = 0
        for event in events {
            switch event {
            case .launched(let slug):
                pet.work()
                offer("\(slug) is starting.", aloud: true, rank: 3)
            case .needsYou(let names):
                pet.attend(for: 4)
                offer(needsYouLine(names), aloud: true, rank: 4)
            case .finished(let name):
                pet.complete()
                offer("\(name) finished.", aloud: false, rank: 1)
            case .message(let fromHuman, let replyFrom):
                if fromHuman { pet.attend() }
                if let replyFrom { offer("\(replyFrom) replied.", aloud: true, rank: 2) }
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
            hiverItem.title = "hiver: not watching"
        }
    }

    /// Without hiver: stop opening at login and quit
    private func turnOff() {
        setLogin(false)
        NSApp.terminate(nil)
    }

    @objc func instruct() { pet.attend() }
    @objc func startWork() { pet.work() }
    @objc func dispatch() { pet.emit() }
    @objc func taskDone() { pet.complete() }

    @objc func toggleLogin() {
        setLogin(loginItem.state != .on)
        loginItem.state = FileManager.default.fileExists(atPath: launchAgent.path) ? .on : .off
    }

    private func setLogin(_ on: Bool) {
        guard on else { try? FileManager.default.removeItem(at: launchAgent); return }
        let plist: [String: Any] = [
            "Label": "com.hiver.prompt",
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
