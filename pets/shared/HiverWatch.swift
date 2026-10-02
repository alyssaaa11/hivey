// Shared by every hiver pet (compiled next to its main.swift by build.sh): the hiver watcher,
// which turns one look at hiver into events each pet acts out with its own moves, and the
// pet switcher behind the "Switch pet" menu.

import AppKit


/// What the pet should act out after one look at hiver
enum HiverEvent: Equatable {
    case launched(String)        // a new swarm or agent → assign
    case needsYou([String])      // agents waiting for the user → listen + say
    case finished(String)        // an agent went from working to idle/done → complete
    case message(fromHuman: Bool, replyFrom: String?)   // replyFrom: an agent writing to the user
}

/// One look at hiver: every agent's status ("slug/key" → status) and every swarm
struct HiverSnapshot {
    var swarms: Set<String> = []
    var status: [String: String] = [:]
    var names: [String: String] = [:]   // "slug/key" → how the pet calls it

    init(swarmList: [String: Any]) {
        for swarm in swarmList["swarms"] as? [[String: Any]] ?? [] {
            guard let slug = swarm["slug"] as? String else { continue }
            swarms.insert(slug)
            let solo = swarm["solo"] as? Bool ?? false
            for agent in swarm["agents"] as? [[String: Any]] ?? [] {
                guard let key = agent["key"] as? String, agent["role"] as? String != "script" else { continue }
                let id = "\(slug)/\(key)"
                status[id] = agent["status"] as? String ?? "gone"
                names[id] = solo ? slug : "\(slug) \(key)"
            }
        }
    }

    var working: Int { status.values.filter { $0 == "working" }.count }
    var idle: Int { status.values.filter { ["idle", "done", "blocked"].contains($0) }.count }

    /// Changes since `old` worth a reaction; nothing on the first look
    func events(since old: HiverSnapshot?) -> [HiverEvent] {
        guard let old else { return [] }
        var events: [HiverEvent] = swarms.subtracting(old.swarms).sorted().map { .launched($0) }
        var waiting: [String] = []
        for (id, now) in status.sorted(by: { $0.key < $1.key }) {
            let before = old.status[id]
            if now == "blocked" && before != "blocked" { waiting.append(names[id] ?? id) }
            if before == "working" && (now == "idle" || now == "done") { events.append(.finished(names[id] ?? id)) }
        }
        if !waiting.isEmpty { events.append(.needsYou(waiting)) }
        return events
    }
}

/// Watches the default hiver session by polling its CLI every 2s (off the main thread).
/// No hiver, or no hiver server running: it waits quietly and never starts one.
final class HiverWatcher {
    var onUpdate: ((HiverSnapshot, [HiverEvent]) -> Void)?
    private(set) var hiverPath: String?
    private var last: HiverSnapshot?
    private var lastMessageTs: Double?
    private let queue = DispatchQueue(label: "com.hiver.h.watch")
    private var timer: DispatchSourceTimer?
    private let vars = ProcessInfo.processInfo.environment
    private var debug: Bool { vars["HIVER_H_DEBUG"] != nil }

    func findHiver() -> String? {
        var candidates = [vars["HIVER_BIN"], NSHomeDirectory() + "/.local/bin/hiver",
                          "/opt/homebrew/bin/hiver", "/usr/local/bin/hiver"].compactMap { $0 }
        candidates += (vars["PATH"] ?? "").split(separator: ":").map { "\($0)/hiver" }
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    func start() {
        stop()
        let t = DispatchSource.makeTimerSource(queue: queue)
        t.schedule(deadline: .now() + 1, repeating: 2)
        t.setEventHandler { [weak self] in self?.poll() }
        t.resume()
        timer = t
    }

    func stop() {
        timer?.cancel()
        timer = nil
        queue.async { [weak self] in
            self?.last = nil
            self?.lastMessageTs = nil
        }
    }

    private func run(_ args: [String]) -> [String: Any]? {
        guard let hiverPath else { return nil }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: hiverPath)
        p.arguments = args
        p.currentDirectoryURL = URL(fileURLWithPath: "/")
        // Not a pane of any swarm: hiver answers for all swarms of the default session
        p.environment = vars.filter { !$0.key.hasPrefix("HERDR_") && !$0.key.hasPrefix("HIVER_") }
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        do { try p.run() } catch { return nil }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    private func poll() {
        if hiverPath == nil { hiverPath = findHiver() }
        guard let list = run(["swarm", "list", "--json"]) else {
            // hiver gone or its server stopped: start fresh when it's back (no replay of old events)
            if last != nil { report(HiverSnapshot(swarmList: [:]), []) }
            last = nil
            lastMessageTs = nil
            return
        }
        let snapshot = HiverSnapshot(swarmList: list)
        var events = snapshot.events(since: last)
        let firstLook = last == nil
        last = snapshot

        let records = run(["msg", "log", "--limit", "40", "--json"])?["records"] as? [[String: Any]] ?? []
        let messages = records.filter { $0["ev"] as? String == "msg" }
        let newest = messages.compactMap { $0["ts"] as? Double }.max() ?? 0
        if !firstLook, let since = lastMessageTs {
            for message in messages where (message["ts"] as? Double ?? 0) > since {
                let from = message["from"] as? String ?? ""
                let toHuman = message["to"] as? String == "human"
                events.append(.message(fromHuman: from == "human",
                                       replyFrom: toHuman && from != "human" ? speaker(from, swarm: message["log"] as? String) : nil))
            }
        }
        lastMessageTs = max(newest, lastMessageTs ?? 0)
        report(snapshot, events)
    }

    /// How the pet names a message's sender: "swarm/agent" → "agent" in its swarm, solo → its slug
    private func speaker(_ from: String, swarm: String?) -> String {
        let parts = from.split(separator: "/").map(String.init)
        let slug = parts.count == 2 ? parts[0] : (swarm ?? "")
        let key = parts.last ?? from
        if key == slug || slug.isEmpty { return key }
        return last?.names["\(slug)/\(key)"] ?? "\(slug) \(key)"
    }

    private func report(_ snapshot: HiverSnapshot, _ events: [HiverEvent]) {
        if debug && !events.isEmpty {
            FileHandle.standardError.write(Data("hiver pet: \(snapshot.working) working, \(snapshot.idle) idle: \(events)\n".utf8))
        }
        DispatchQueue.main.async { [weak self] in self?.onUpdate?(snapshot, events) }
    }
}


// MARK: - Switching pets

/// The hiver pets (folders under pets/ in the hiver repo); `hiver pet use <id>` swaps them.
let hiverPets: [(id: String, name: String)] = [
    ("hiver-dot", "Hiver"),
    ("hiver-prompt", "Hiver Prompt"),
    ("hiver-h", "Hiver H"),
]

/// "Switch pet ▸" and "Turn off pet" for a pet's right-click menu. Both run the hiver CLI
/// (`hiver pet use <id>` / `hiver pet off`), which builds, swaps and remembers the choice.
final class PetSwitcher: NSObject {
    private let current: String
    private let say: (String) -> Void
    private let turnOffHere: () -> Void

    /// `say`: the pet announces the switch; `turnOffHere`: what "Turn off" does without hiver
    init(current: String, say: @escaping (String) -> Void, turnOffHere: @escaping () -> Void) {
        self.current = current
        self.say = say
        self.turnOffHere = turnOffHere
    }

    func addItems(to menu: NSMenu) {
        let switchItem = NSMenuItem(title: "Switch pet", action: nil, keyEquivalent: "")
        let submenu = NSMenu()
        for pet in hiverPets {
            let item = submenu.addItem(withTitle: pet.name, action: #selector(choose(_:)), keyEquivalent: "")
            item.target = self
            item.representedObject = pet.id
            item.state = pet.id == current ? .on : .off
        }
        switchItem.submenu = submenu
        menu.addItem(switchItem)
        menu.addItem(withTitle: "Turn off pet", action: #selector(turnOff), keyEquivalent: "").target = self
    }

    @objc private func choose(_ item: NSMenuItem) {
        guard let id = item.representedObject as? String, id != current else { return }
        let name = hiverPets.first { $0.id == id }?.name ?? id
        guard runHiver(["pet", "use", id]) else { say("I need hiver to switch pets."); return }
        say("Switching to \(name)…")   // hiver quits this pet once the new one is ready
    }

    @objc private func turnOff() {
        if !runHiver(["pet", "off"]) { turnOffHere() }
    }

    /// Runs `hiver <args>` detached (it may outlive this pet); false when hiver isn't installed.
    private func runHiver(_ args: [String]) -> Bool {
        guard let hiver = HiverWatcher().findHiver() else { return false }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: hiver)
        p.arguments = args
        p.currentDirectoryURL = URL(fileURLWithPath: NSHomeDirectory())
        // What happened lands in ~/.hiver/pet.log (a switch builds for a minute; errors matter)
        let log = URL(fileURLWithPath: NSHomeDirectory() + "/.hiver/pet.log")
        try? FileManager.default.createDirectory(at: log.deletingLastPathComponent(), withIntermediateDirectories: true)
        if !FileManager.default.fileExists(atPath: log.path) { FileManager.default.createFile(atPath: log.path, contents: nil) }
        if let handle = try? FileHandle(forWritingTo: log) {
            handle.seekToEndOfFile()
            handle.write(Data("\n\(Date()) hiver \(args.joined(separator: " "))\n".utf8))
            p.standardOutput = handle
            p.standardError = handle
        } else {
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
        }
        return (try? p.run()) != nil
    }
}
