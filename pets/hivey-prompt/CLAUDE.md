# Hivey Prompt — desktop pet

## READ FIRST — before running, building, or connecting Hivey Prompt to anything

1. Read this whole file. The **Character spec** below is the source of truth for how Hivey and its three agents look, move and behave; never change its identity without the user asking.
2. Tell the user, in a few lines, what Hivey Prompt can do **today** (see Behavior) and which **future capabilities** below are not built yet.
3. Ask which capability to add next. Do not connect it to anything new Hivey Prompt to hooks, swarms, Slack or any external service until the user picks one — connecting changes what happens on their desktop.
4. When adding a capability: drive it through the existing animations (`attend`, `work`, `emit`, `complete`, `say`) and the agent `spots` modes (`idle` / `gather` / `swarm`) — new behavior must reuse the character's movement vocabulary, not invent a new look. No circular orbits or planetary rings, ever. Rebuild with `./build.sh` and verify with a screenshot (use `screencapture -R` on the real desktop for shadows; see note under Behavior).
5. After a capability ships, move it from "Future capabilities" into "Behavior".

## Character spec

Hivey is a three-dimensional dot with a deep charcoal-teal body and a crisp mint `>_` face. Its surface has a satin finish, with directional lighting, subtle highlights, and rich shading across its sides and underside. A soft shadow beneath it gives it weight and depth.

Three smaller emerald and muted mint agent dots surround Hivey at different heights and depths. Each is a solid, shaded sphere. Some sit slightly behind Hivey, while others float nearer the viewer, creating a dimensional group.

The agents are animated: they gently hover nearby, gather when Hivey receives instructions, and separate into coordinated formations when a swarm starts working. Short light pulses pass between them when they communicate. Their movement feels deliberate and organized, without circular orbits or planetary rings.

Hivey's cursor blinks slowly while idle and pulses while working. Its movements remain restrained and professional. The overall impression is a composed terminal conductor surrounded by its active team—dark, tactile, and clearly three-dimensional.

## Future capabilities (not built yet)

| Capability | Notes |
|---|---|
| Live team size | Show the real number of agents (fixed at three today) by adding/removing agent spheres with new `spots`, keeping the dimensional group (mix of behind and in front, no rings). |
| Claude Code hooks | React to Claude sessions *outside* hivey via hooks in `~/.claude/settings.json` calling the event CLI. Only with the user's OK. |
| Slack summary | Slack posts already pulse; optionally `say` a short summary. |

## Overview

A three-dimensional satin sphere: deep charcoal-teal body, crisp mint `>_` face, directional lighting (key light upper-left, floor bounce light underneath, soft specular), soft contact shadow. Three smaller shaded agent spheres (emerald + muted mint) surround it at different heights and depths — some behind Hivey, some nearer the viewer. No orbits or rings. Professional, composed, precise. Watches hivey and acts out its swarms, agents and messages. Sibling of the other pets in `pets/` (separate app, bundle id `com.hivey.prompt`, so all can run at once). Single `main.swift` (AppKit + Core Graphics).

## Files
- `main.swift` — the whole app: `drawSphere`/`drawShadow` 3D shading, pet animation, terminal-style speech bubble, `say` voice.
- `build.sh` — builds universal (arm64 + x86_64, macOS 12+) ad-hoc-signed `Hivey Prompt.app` and `dist/HiveyPrompt.zip`. Run after every change.

## Behavior
- Click = chat box above the pet (`HiveyChat`): Enter sends to the hivey agent (`hivey msg send hivey/master`), Esc / clicking away closes; the agent's reply to `human` is shown in the bubble and said aloud. Drag still moves the pet.
- Lives in the hivey repo (`pets/hivey-prompt/`). Built and installed by `hivey pet use hivey-prompt` (into `~/Applications`; the other pets are quit); `./build.sh` compiles `main.swift` with the shared `../shared/HiveyWatch.swift`.
- Right-click: hivey status line ("hivey: N agents, M working"), Watch hivey, Speak aloud, Effects (preview only) ▸ (plays the pet's moves; nothing happens in hivey), Switch pet ▸ (Hivey / Hivey Prompt / Hivey, via `hivey pet use`), Turn off pet (`hivey pet off`), Quit.
- Watches hivey through the shared `HiveyWatcher` (every 2s: `hivey swarm list --json` + `hivey msg log --json`, default session, never starts a server, nothing replayed on first look). Spoken aloud: "<slug> is starting.", "<agent> needs you.", "<agent> replied."; "<agent> finished." is bubble-only; one line per look, most urgent first.
- Debug: `HIVEY_BIN=/path/to/fake-hivey HIVEY_H_DEBUG=1 <app>/Contents/MacOS/<exe>` logs each look's events.
- hivey → Hivey Prompt: swarm/agent starts → `work` · agents working → `keepWorking` (formation renewed; an `attend` gather is not interrupted) · agent needs you / message from the user → `attend` · message → `pulse()` between agents · agent finished → `complete`.
- Event CLI: `"Hivey Prompt.app/Contents/MacOS/hivey-prompt" attend|work|emit|complete ["text"]` (DistributedNotification `com.hivey.prompt.event`).
- Agents spring toward per-mode spots `(x, y, z)` in `spots` (`idle` / `gather` / `swarm`); z sets draw order, size and haze. Modes expire back to idle.
- Launch: fades in, message pulses out to agents, agents report back, restrained nod + brief mint glow, says "Welcome. I'm Hivey." (bubble + voice). Always visible, floats above windows, on all Spaces.
- Idle: Hivey rises and settles; `_` cursor blinks slowly; agents hover at different depths; occasional pulse between agents or to Hivey.
- Instructions (click, which also opens the chat box; `say`; menu): face tilts in attention, agents gather close (3s).
- Start swarm (menu, 8s): agents spread into a triangle formation, bob in sync, steady message pulses; cursor pulses.
- Dispatch message: a pulse from Hivey to each agent.
- Task complete: agents report back (pulses to Hivey), nod + brief glow, back to idle.
- While speaking the `_` flickers like typing (no mouth).
- Drag = move (autosaved as `HiveyPromptPet`). Effects (preview only) ▸: Give instructions, Start swarm, Dispatch message, Task complete.
- `"Hivey Prompt.app/Contents/MacOS/hivey-prompt" say "text"` makes it talk. One instance per Mac.
- Window-only screenshots (`screencapture -l`) show the floor shadow wrongly light on dark backgrounds; it composites correctly on the real desktop.

## Sharing / lifecycle
- Shows while hivey is open: each hivey window writes `~/.hivey/windows/<pid>` and runs `hivey pet show`; the pet quits itself ~6s after the last live window is gone (`HiveyWatcher.onLastWindowClosed`). No Open at Login: switching removes old `~/Library/LaunchAgents/com.hivey.prompt.plist` items.
- Share it through hivey (`hivey pet use hivey-prompt`): ad-hoc signed, built on the user's Mac, so no quarantine prompt.
