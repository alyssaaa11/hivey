# Hiver — desktop pet

## READ FIRST — before running, building, or connecting Hiver to anything

1. Read this whole file. The **Character spec** below is the source of truth for how Hiver looks, moves and behaves; never change its identity without the user asking.
2. **Show the user the full "Future capabilities" section** — the event table and every planned building block — before connecting anything. Also tell them what Hiver does **today** (see Behavior) and point them to the demo GIF `docs/capabilities-demo.gif`, which acts out every capability.
3. Let the user choose: connect **one** capability, **several**, or **all**. Do not connect Hiver to hooks, swarms, Slack or any external service until they choose — connecting changes what happens on their desktop. If they choose all, build the blocks in the listed order, one at a time, and verify each before starting the next.
4. When adding a capability: drive it through the existing animations (`addAgent`, `formSwarm`, `celebrate`, `say`, the pulses between dots) — new behavior must reuse the character's movement vocabulary and stay professional (dark graphite body, mint accents, signal wave, no cheeks, mouth only while speaking). Rebuild with `./build.sh` and verify with a screenshot of the window.
5. After a capability ships, move it from "Future capabilities" into "Behavior".

## Character spec

Hiver is a small, soft dot with expressive eyes and a calm, curious personality. It represents the main agent—the friendly conductor that creates agents, coordinates swarms, and keeps everyone connected. Its look is professional, in "harness" mode: a dark graphite satin body with glowing mint eyes, no cheeks, and a mouth that appears only while it speaks.

Small mint-green dots travel around its body, representing the agents it manages. They ride a delicate glowing **signal wave**—a loop that ripples like a live signal, with a bright packet running along it—giving Hiver a gentle sense of life even while idle.

When Hiver creates an agent, a new dot appears on its signal. As it coordinates a swarm, the dots move into formation. When agents exchange messages, tiny pulses of light travel between them. When a task finishes, Hiver gives a small, restrained lift and its companions briefly glow.

Its design stays simple and recognizable: one thoughtful little dot, surrounded by a world of teamwork.

## Future capabilities (not built yet)

| Capability | Notes |
|---|---|
| Claude Code hooks | React to Claude sessions *outside* hiver via hooks in `~/.claude/settings.json` calling the event CLI. Changes every Claude session on the Mac: only with the user's OK. |
| Slack summary | Slack posts already pulse (they reach the bus from `human`); optionally `say` a short summary. |

## Overview

Professional "harness" look: a dark graphite satin sphere (key light upper-left, mint bounce light underneath) with glowing mint-white eyes; small mint dots (its agents) ride a glowing **signal wave** — a tilted loop whose radius carries a travelling sine wave, with a brighter signal packet running along it. Mouth only while speaking, no cheeks. Dark terminal-style speech bubble (monospaced). Watches hiver: its dots are hiver's agents. `main.swift` (AppKit + Core Graphics) plus the shared `../shared/HiverWatch.swift`. Previous ivory version saved in `backup/main.ivory-orbit.swift`.

## Files
- `main.swift` — the whole app: Hiver + orbit drawing/animation, speech bubble, `say` voice, Open at Login.
- `build.sh` — builds universal (arm64 + x86_64, macOS 12+) ad-hoc-signed `Hiver.app` and `dist/Hiver.zip` for sharing. Run after every change.

## Behavior
- Lives in the hiver repo (`pets/hiver-dot/`). Built and installed by `hiver pet use hiver-dot` (into `~/Applications`, opens at login; the other pets are quit); `./build.sh` compiles `main.swift` with the shared `../shared/HiverWatch.swift`.
- Right-click: hiver status line ("hiver: N agents, M working"), Watch hiver, Speak aloud, …, Switch pet ▸ (Hiver / Hiver Prompt / Hiver H, via `hiver pet use`), Turn off pet (`hiver pet off`), Open at Login, Quit.
- Watches hiver through the shared `HiverWatcher` (every 2s: `hiver swarm list --json` + `hiver msg log --json`, default session, never starts a server, nothing replayed on first look). Spoken aloud: "<slug> is starting.", "<agent> needs you.", "<agent> replied."; "<agent> finished." is bubble-only; one line per look, most urgent first.
- Debug: `HIVER_BIN=/path/to/fake-hiver HIVER_H_DEBUG=1 <app>/Contents/MacOS/<exe>` logs each look's events.
- hiver → Hiver: dots on the wave = hiver's agents (max 9; `showAgents`: new dots fly out, gone ones fade off via `removeAgent`) · swarm/agent starts → `formSwarm` · agents working → `busy` (frequent pulses) · message → `pulse()` between two dots · message from the user → bubble "On it." · agent finished → `celebrate`.
- Event CLI: `Hiver.app/Contents/MacOS/hiver agent|leave|swarm|pulse|done ["text"]` (DistributedNotification `com.hiver.pet.event`).
- On launch: fades in, small restrained lift + mint glow, says "Welcome! I'm Hiver." (bubble + voice). Always visible, floats above windows, on all Spaces.
- Idle: agents travel the signal wave (`signalPoint`, depth-sorted behind/in front of Hiver), breathing, blinking, eyes follow the cursor, occasional message pulse between agents.
- Click = small lift + glow + short silent line.
- Right-click menu (demos of future behaviors): New agent (dot flies out, max 9), Swarm formation (dots arc above Hiver, frequent pulses, 5s), Task finished (lift + glow), Open at Login, Quit.
- A crew-motion variant (no orbit; idle groups, errands, listen/swarm/tidy formations) was tried on 2026-10-02 and rejected — user prefers the orbit.
- Drag = move (position autosaved as `HiverPet`).
- `Hiver.app/Contents/MacOS/hiver say "text"` makes the running pet talk.
- One instance per Mac.

## Sharing / login
- First launch from an `.app` writes `~/Library/LaunchAgents/com.hiver.pet.plist` (`open -a <app path>`, RunAtLoad), so Hiver greets on every login. Toggle via the menu.
- To share: send `dist/Hiver.zip`. It is ad-hoc signed, not notarized → recipients must move it to Applications and right-click → Open the first time (or `xattr -dr com.apple.quarantine Hiver.app`).
