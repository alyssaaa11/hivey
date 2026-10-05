# Hivey System — desktop pet

## READ FIRST — before running, building, or connecting Hivey to anything

1. Read this whole file. The **Character spec** below is the source of truth for how Hivey looks, moves and behaves; never change its identity without the user asking.
2. **Show the user the full "Future capabilities" section** — the event table and every planned building block — before connecting anything. Also tell them what Hivey does **today** (see Behavior) and point them to the demo GIF `docs/capabilities-demo.gif`, which acts out every capability.
3. Let the user choose: connect **one** capability, **several**, or **all**. Do not connect Hivey to hooks, swarms, Slack or any external service until they choose — connecting changes what happens on their desktop. If they choose all, build the blocks in the listed order, one at a time, and verify each before starting the next.
4. When adding a capability: drive it through the existing animations (`addAgent`, `formSwarm`, `celebrate`, `say`, the pulses between dots) — new behavior must reuse the character's movement vocabulary and stay professional (dark graphite body, mint accents, signal wave, no cheeks, mouth only while speaking). Rebuild with `./build.sh` and verify with a screenshot of the window.
5. After a capability ships, move it from "Future capabilities" into "Behavior".

## Character spec

Hivey is a small, soft dot with expressive eyes and a calm, curious personality. It represents the main agent—the friendly conductor that creates agents, coordinates swarms, and keeps everyone connected. Its look is professional, in "harness" mode: a dark graphite satin body with glowing mint eyes, no cheeks, and a mouth that appears only while it speaks.

Small mint-green dots travel around its body, representing the agents it manages. They ride a delicate glowing **signal wave**—a loop that ripples like a live signal, with a bright packet running along it—giving Hivey a gentle sense of life even while idle.

When Hivey creates an agent, a new dot appears on its signal. As it coordinates a swarm, the dots move into formation. When agents exchange messages, tiny pulses of light travel between them. When a task finishes, Hivey gives a small, restrained lift and its companions briefly glow.

Its design stays simple and recognizable: one thoughtful little dot, surrounded by a world of teamwork.

## Future capabilities (not built yet)

| Capability | Notes |
|---|---|
| Claude Code hooks | React to Claude sessions *outside* hivey via hooks in `~/.claude/settings.json` calling the event CLI. Changes every Claude session on the Mac: only with the user's OK. |
| Slack summary | Slack posts already pulse (they reach the bus from `human`); optionally `say` a short summary. |

## Overview

Professional "harness" look: a dark graphite satin sphere (key light upper-left, mint bounce light underneath) with glowing mint-white eyes; small mint dots (its agents) ride a glowing **signal wave** — a tilted loop whose radius carries a travelling sine wave, with a brighter signal packet running along it. Mouth only while speaking, no cheeks. Dark terminal-style speech bubble (monospaced). Watches hivey: its dots are hivey's agents. `main.swift` (AppKit + Core Graphics) plus the shared `../shared/HiveyWatch.swift`. Previous ivory version saved in `backup/main.ivory-orbit.swift`.

## Files
- `main.swift` — the whole app: Hivey + orbit drawing/animation, speech bubble, `say` voice.
- `build.sh` — builds universal (arm64 + x86_64, macOS 12+) ad-hoc-signed `Hivey System.app` and `dist/HiveySystem.zip` for sharing. Run after every change.

## Behavior
- Click = chat box above the pet (`HiveyChat`): Enter sends to the hivey agent (`hivey msg send hivey/master`), Esc / clicking away closes; the agent's reply to `human` is shown in the bubble and said aloud. Drag still moves the pet.
- Lives in the hivey repo (`pets/hivey-dot/`). Built and installed by `hivey pet use hivey-dot` (into `~/Applications`; the other pets are quit); `./build.sh` compiles `main.swift` with the shared `../shared/HiveyWatch.swift`.
- Right-click: hivey status line ("hivey: N agents, M working"), Watch hivey, Speak aloud, Effects (preview only) ▸ (plays the pet's moves; nothing happens in hivey), Switch pet ▸ (Hivey / Hivey Prompt / Hivey, via `hivey pet use`), Turn off pet (`hivey pet off`), Quit.
- Watches hivey through the shared `HiveyWatcher` (every 2s: `hivey swarm list --json` + `hivey msg log --json`, default session, never starts a server, nothing replayed on first look). Spoken aloud: "<slug> is starting.", "<agent> needs you.", "<agent> replied."; "<agent> finished." is bubble-only; one line per look, most urgent first.
- Debug: `HIVEY_BIN=/path/to/fake-hivey HIVEY_H_DEBUG=1 <app>/Contents/MacOS/<exe>` logs each look's events.
- hivey → Hivey: dots on the wave = hivey's agents (max 9; `showAgents`: new dots fly out, gone ones fade off via `removeAgent`) · swarm/agent starts → `formSwarm` · agents working → `busy` (frequent pulses) · message → `pulse()` between two dots · message from the user → bubble "On it." · agent finished → `celebrate`.
- Event CLI: `Hivey.app/Contents/MacOS/hivey agent|leave|swarm|pulse|done ["text"]` (DistributedNotification `com.hivey.pet.event`).
- On launch: fades in, small restrained lift + mint glow, says "Welcome! I'm Hivey." (bubble + voice). Always visible, floats above windows, on all Spaces.
- Idle: agents travel the signal wave (`signalPoint`, depth-sorted behind/in front of Hivey), breathing, blinking, eyes follow the cursor, occasional message pulse between agents.
- Click = small lift + glow, and the chat box opens (see above).
- Effects (preview only) ▸: New agent (dot flies out, max 9), Swarm formation (dots arc above Hivey, frequent pulses, 5s), Task finished (lift + glow), Quit.
- A crew-motion variant (no orbit; idle groups, errands, listen/swarm/tidy formations) was tried on 2026-10-02 and rejected — user prefers the orbit.
- Drag = move (position autosaved as `HiveyPet`).
- `Hivey.app/Contents/MacOS/hivey say "text"` makes the running pet talk.
- One instance per Mac.

## Sharing / lifecycle
- Shows while hivey is open: each hivey window writes `~/.hivey/windows/<pid>` and runs `hivey pet show`; the pet quits itself ~6s after the last live window is gone (`HiveyWatcher.onLastWindowClosed`). No Open at Login: switching removes old `~/Library/LaunchAgents/com.hivey.pet.plist` items.
- Share it through hivey (`hivey pet use hivey-dot`): ad-hoc signed, built on the user's Mac, so no quarantine prompt.
