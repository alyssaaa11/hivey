# Hiver H — desktop pet

## READ FIRST — before running, building, or connecting Hiver to anything

1. Read this whole file. The **Character spec** below is the source of truth for how Hiver looks, moves and behaves; never change its identity without the user asking.
2. Tell the user, in a few lines, what Hiver can do **today** (see Behavior) and which **future capabilities** below are not built yet.
3. Ask which capability to add next. Do not connect Hiver to hooks, swarms, Slack or any external service until the user picks one — connecting changes what happens on their desktop.
4. When adding a capability: drive it through the existing animations (`assign`, `listen`, `think`, `message`, `complete`, `say`) — new behavior must reuse the character's movement vocabulary, not invent a new look. Rebuild with `./build.sh` and verify with a screenshot of the window.
5. After a capability ships, move it from "Future capabilities" into "Behavior".

## Character spec

Hiver is a living, three-dimensional "H"—a composed software conductor whose body represents the way it connects agents and coordinates swarms.

Its silhouette consists of two upright forms joined by a central bridge, with softly beveled edges and a dark graphite satin finish. Deep shading and subtle highlights give it weight and dimension. A pair of small, attentive mint eyes sits in the bridge, placing its personality at the point where everything connects.

Its body is modular, built from a few precisely fitted sections. **The character is animated:** when assigning work, sections gently slide outward while fine mint connections remain visible between them. When agents communicate, brief pulses travel through the central bridge. As tasks finish, the sections settle back into a complete "H."

While idle, Hiver makes small, deliberate shifts and its eyes blink naturally. When listening, it leans forward slightly; when thinking, a soft pulse moves across its bridge. Success brings a restrained nod and a brief mint glow.

Hiver feels professional, observant, and quietly confident. Its identity comes from its recognizable "H" silhouette and coordinated movement—one connected character that can organize many moving parts.

## Future capabilities (not built yet)

| Capability | Notes |
|---|---|
| Claude Code hooks | React to Claude sessions *outside* hiver: hooks in `~/.claude/settings.json` (`UserPromptSubmit` → listen, `Stop` → complete, `Notification` → listen + say) calling the event CLI. Changes every Claude session on the Mac: only with the user's OK. Model it on Sprout's `~/projects/desktop-pet/notify.sh`. |
| Other hiver sessions | The watcher follows the `default` session only; named sessions (`hiver --session NAME`) are not watched. |
| Slack summary | Slack posts already pulse `message` (they reach the bus as messages from `human`); optionally `say` a short summary of them. |

Preview: `docs/capabilities-demo.gif` acts out the event → reaction table under Behavior (recorded from a temporary demo build; the app itself has no demo mode).

## Overview

A living, three-dimensional "H": the software conductor whose body shows how it connects agents and coordinates swarms. Five precisely fitted sections (left/right uprights split into top + bottom, plus the central bridge), softly beveled, dark graphite satin finish, extruded depth toward the lower right, soft floor shadow. Two small attentive mint eyes sit in the bridge. Professional, observant, quietly confident. Watches hiver and acts out what its swarms and agents do. Sibling of `~/projects/hiver_pet` and `~/projects/hiver_pet_2` (separate app, bundle id `com.hiver.h`, so all can run at once). Single `main.swift` (AppKit + Core Graphics).

## Files
- `main.swift` — the whole app: `sections` geometry + `spread` vectors, `drawSide` (extrusion) / `drawFace` (satin + bevel), connections, pulses, agent dots, terminal-style speech bubble, `say` voice, event CLI, `AppDelegate.react` (maps hiver events onto animations). The watcher (`HiverSnapshot` → `HiverEvent`s, `HiverWatcher`) and the pet switcher are in the shared `../shared/HiverWatch.swift`.
- `build.sh` — builds universal (arm64 + x86_64, macOS 12+) ad-hoc-signed `Hiver H.app` and `dist/HiverH.zip`. Run after every change.

## Behavior
- Lives in the hiver repo (`pets/hiver-h/`). Built and installed by `hiver pet use hiver-h` (into `~/Applications`; the other pets are quit); `./build.sh` compiles `main.swift` with the shared `../shared/HiverWatch.swift`.
- Right-click: hiver status line ("hiver: N agents, M working"), Watch hiver, Speak aloud, …, Switch pet ▸ (Hiver / Hiver Prompt / Hiver H, via `hiver pet use`), Turn off pet (`hiver pet off`), Quit.
- Watches hiver through the shared `HiverWatcher` (every 2s: `hiver swarm list --json` + `hiver msg log --json`, default session, never starts a server, nothing replayed on first look). Spoken aloud: "<slug> is starting.", "<agent> needs you.", "<agent> replied."; "<agent> finished." is bubble-only; one line per look, most urgent first.
- Debug: `HIVER_BIN=/path/to/fake-hiver HIVER_H_DEBUG=1 <app>/Contents/MacOS/<exe>` logs each look's events.
- Launch: fades in while the sections settle from open into a complete H, restrained nod + brief mint glow, says "Welcome. I'm Hiver." (bubble + voice). Always visible, floats above windows, on all Spaces.
- Idle: small deliberate shifts (x offset + slight rotation every 3.5–6.5s), gentle rise and settle, natural blinks, eyes drift toward the cursor.
- Listen (click, `say`, menu): leans forward slightly (scale up, eyes look straight ahead).
- Think (menu; also while speaking): a soft mint pulse sweeps across the bridge.
- Assign work (menu, 6s): sections spring outward, fine mint connections link bridge ↔ sections and top ↔ bottom halves; pulses run through the bridge and along the connections.
- Send message: one pulse travels through the bridge.
- Task complete: sections settle back into the H, restrained nod, brief mint glow.
- Drag = move (autosaved as `HiverHPet`). Right-click: hiver status line, Watch hiver, Speak aloud, Assign work, Listen, Think, Send message, Task complete, Quit.
- `"Hiver H.app/Contents/MacOS/hiver-h" say "text"` makes it talk. One instance per Mac.
- Event CLI: `hiver-h assign|listen|think|message|complete ["text"]` plays that animation (and says the text) in the running pet, via a DistributedNotification (`com.hiver.h.event`, object `"<event>\n<text>"`). `hiver-h --help` lists it.
- Watches hiver (menu "Watch hiver", on by default): every 2s it runs `hiver swarm list --json` and `hiver msg log --json` against the `default` session (finds `hiver` via `$HIVER_BIN`, `~/.local/bin`, Homebrew paths, `$PATH`; never starts a server; first look and reconnects replay nothing). The menu's first line shows `hiver: N agents, M working`.

  | hiver event | Hiver reaction |
  |---|---|
  | A new swarm or agent appears | `assign` + "<slug> is starting." aloud |
  | Any agent is working | `think`, renewed while work goes on |
  | An agent turns `blocked` (needs the user) | `listen` + "<agent> needs you." aloud (several: "a and b need you." / "N agents need you.") |
  | An agent goes `working` → `idle`/`done` | `complete` + bubble "<agent> finished." |
  | A bus message (agents, Slack relay) | `message` pulse (max 3 per look, staggered) |
  | A message from `human` (the user, also from Slack) | `listen` + pulse |
  | An agent writes to `human` (a reply, also posted to Slack) | pulse + "<agent> replied." aloud |
  | Agent count | mint dots on the floor under the H: bright breathing = working, dim = idle/done/blocked (max 12) |

  One line per look, the most urgent wins (needs you > starting > replied > finished); "finished" is bubble-only. Menu "Speak aloud" (on by default) mutes the voice; bubbles stay. Script panes (relays) are not agents.
- Debug: `HIVER_BIN=/path/to/fake-hiver HIVER_H_DEBUG=1 "Hiver H.app/Contents/MacOS/hiver-h"` logs each look's events to stderr; a fake `hiver` that `cat`s prepared `swarm list` / `msg log` JSON replays any scenario.

## Sharing / lifecycle
- Shows while hiver is open: each hiver window writes `~/.hiver/windows/<pid>` and runs `hiver pet show`; the pet quits itself ~6s after the last live window is gone (`HiverWatcher.onLastWindowClosed`). No Open at Login: switching removes old `~/Library/LaunchAgents/com.hiver.h.plist` items.
- Share it through hiver (`hiver pet use hiver-h`): ad-hoc signed, built on the user's Mac, so no quarantine prompt.
