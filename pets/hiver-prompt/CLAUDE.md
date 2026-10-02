# Hiver Prompt — desktop pet

## READ FIRST — before running, building, or connecting Hiver Prompt to anything

1. Read this whole file. The **Character spec** below is the source of truth for how Hiver and its three agents look, move and behave; never change its identity without the user asking.
2. Tell the user, in a few lines, what Hiver Prompt can do **today** (see Behavior) and which **future capabilities** below are not built yet.
3. Ask which capability to add next. Do not connect it to anything new Hiver Prompt to hooks, swarms, Slack or any external service until the user picks one — connecting changes what happens on their desktop.
4. When adding a capability: drive it through the existing animations (`attend`, `work`, `emit`, `complete`, `say`) and the agent `spots` modes (`idle` / `gather` / `swarm`) — new behavior must reuse the character's movement vocabulary, not invent a new look. No circular orbits or planetary rings, ever. Rebuild with `./build.sh` and verify with a screenshot (use `screencapture -R` on the real desktop for shadows; see note under Behavior).
5. After a capability ships, move it from "Future capabilities" into "Behavior".

## Character spec

Hiver is a three-dimensional dot with a deep charcoal-teal body and a crisp mint `>_` face. Its surface has a satin finish, with directional lighting, subtle highlights, and rich shading across its sides and underside. A soft shadow beneath it gives it weight and depth.

Three smaller emerald and muted mint agent dots surround Hiver at different heights and depths. Each is a solid, shaded sphere. Some sit slightly behind Hiver, while others float nearer the viewer, creating a dimensional group.

The agents are animated: they gently hover nearby, gather when Hiver receives instructions, and separate into coordinated formations when a swarm starts working. Short light pulses pass between them when they communicate. Their movement feels deliberate and organized, without circular orbits or planetary rings.

Hiver's cursor blinks slowly while idle and pulses while working. Its movements remain restrained and professional. The overall impression is a composed terminal conductor surrounded by its active team—dark, tactile, and clearly three-dimensional.

## Future capabilities (not built yet)

| Capability | Notes |
|---|---|
| Live team size | Show the real number of agents (fixed at three today) by adding/removing agent spheres with new `spots`, keeping the dimensional group (mix of behind and in front, no rings). |
| Claude Code hooks | React to Claude sessions *outside* hiver via hooks in `~/.claude/settings.json` calling the event CLI. Only with the user's OK. |
| Slack summary | Slack posts already pulse; optionally `say` a short summary. |

## Overview

A three-dimensional satin sphere: deep charcoal-teal body, crisp mint `>_` face, directional lighting (key light upper-left, floor bounce light underneath, soft specular), soft contact shadow. Three smaller shaded agent spheres (emerald + muted mint) surround it at different heights and depths — some behind Hiver, some nearer the viewer. No orbits or rings. Professional, composed, precise. Watches hiver and acts out its swarms, agents and messages. Sibling of `~/projects/hiver_pet` (separate app, bundle id `com.hiver.prompt`, so both can run at once). Single `main.swift` (AppKit + Core Graphics).

## Files
- `main.swift` — the whole app: `drawSphere`/`drawShadow` 3D shading, pet animation, terminal-style speech bubble, `say` voice, Open at Login.
- `build.sh` — builds universal (arm64 + x86_64, macOS 12+) ad-hoc-signed `Hiver Prompt.app` and `dist/HiverPrompt.zip`. Run after every change.

## Behavior
- Lives in the hiver repo (`pets/hiver-prompt/`). Built and installed by `hiver pet use hiver-prompt` (into `~/Applications`, opens at login; the other pets are quit); `./build.sh` compiles `main.swift` with the shared `../shared/HiverWatch.swift`.
- Right-click: hiver status line ("hiver: N agents, M working"), Watch hiver, Speak aloud, …, Switch pet ▸ (Hiver / Hiver Prompt / Hiver H, via `hiver pet use`), Turn off pet (`hiver pet off`), Open at Login, Quit.
- Watches hiver through the shared `HiverWatcher` (every 2s: `hiver swarm list --json` + `hiver msg log --json`, default session, never starts a server, nothing replayed on first look). Spoken aloud: "<slug> is starting.", "<agent> needs you.", "<agent> replied."; "<agent> finished." is bubble-only; one line per look, most urgent first.
- Debug: `HIVER_BIN=/path/to/fake-hiver HIVER_H_DEBUG=1 <app>/Contents/MacOS/<exe>` logs each look's events.
- hiver → Hiver Prompt: swarm/agent starts → `work` · agents working → `keepWorking` (formation renewed; an `attend` gather is not interrupted) · agent needs you / message from the user → `attend` · message → `pulse()` between agents · agent finished → `complete`.
- Event CLI: `"Hiver Prompt.app/Contents/MacOS/hiver-prompt" attend|work|emit|complete ["text"]` (DistributedNotification `com.hiver.prompt.event`).
- Agents spring toward per-mode spots `(x, y, z)` in `spots` (`idle` / `gather` / `swarm`); z sets draw order, size and haze. Modes expire back to idle.
- Launch: fades in, message pulses out to agents, agents report back, restrained nod + brief mint glow, says "Welcome. I'm Hiver." (bubble + voice). Always visible, floats above windows, on all Spaces.
- Idle: Hiver rises and settles; `_` cursor blinks slowly; agents hover at different depths; occasional pulse between agents or to Hiver.
- Instructions (click, `say`, menu): face tilts in attention, agents gather close (3s).
- Start swarm (menu, 8s): agents spread into a triangle formation, bob in sync, steady message pulses; cursor pulses.
- Dispatch message: a pulse from Hiver to each agent.
- Task complete: agents report back (pulses to Hiver), nod + brief glow, back to idle.
- While speaking the `_` flickers like typing (no mouth).
- Drag = move (autosaved as `HiverPromptPet`). Right-click: Give instructions, Start swarm, Dispatch message, Task complete, Open at Login, Quit.
- `"Hiver Prompt.app/Contents/MacOS/hiver-prompt" say "text"` makes it talk. One instance per Mac.
- Window-only screenshots (`screencapture -l`) show the floor shadow wrongly light on dark backgrounds; it composites correctly on the real desktop.

## Sharing / login
- First launch from an `.app` writes `~/Library/LaunchAgents/com.hiver.prompt.plist` (`open -a <app path>`, RunAtLoad). Toggle via the menu.
- Share `dist/HiverPrompt.zip`. Ad-hoc signed, not notarized → recipients right-click → Open the first time (or `xattr -dr com.apple.quarantine "Hiver Prompt.app"`).
