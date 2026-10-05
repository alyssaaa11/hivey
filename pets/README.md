# hivey pets (macOS)

Desktop pets that act out what hivey's agents do. Choose one (or none) with `hivey pet choose`,
`⌥P`, hivey's menu → pets, or `install.sh`; switch from the pet's right-click menu.

| id | name | look |
|---|---|---|
| `hivey-h` | Hivey | a 3D H; its sections slide apart to hand out work |
| `hivey-dot` | Hivey System | a graphite dot; its agents ride a signal wave around it |
| `hivey-prompt` | Hivey Prompt | a `>_` sphere leading three agent spheres |

- `<id>/main.swift` + `<id>/build.sh`: the pet (AppKit + Core Graphics); each `CLAUDE.md` is
  its character spec and behavior, read it before changing a pet.
- `shared/HiveyWatch.swift`: compiled into every pet. `HiveyWatcher` polls hivey
  (`swarm list` / `msg log`), `HiveySnapshot` turns two looks into `HiveyEvent`s (launched,
  needs you, finished, message), `PetSwitcher` is the Switch pet / Turn off pet menu.
- `pet.py`: `hivey pet` (build, install to `~/Applications`, switch, off, show). The
  choice is in `~/.hivey/pet.json`; `hivey update` runs `hivey pet refresh` to rebuild it when
  its source changed.
- Chat: clicking a pet opens `HiveyChat` (a box above it); Enter sends the text to the hivey
  agent (`hivey msg send hivey/master`), whose reply to `human` the pet shows and says
  (`replyLine`). Other agents' replies are announced as "<agent> replied.".
- Lifecycle: every hivey window writes `~/.hivey/windows/<pid>` while open and runs
  `hivey pet show`, so the pet appears with the first window; the pet quits itself ~6s after the
  last window closes (`HiveyWatcher.onLastWindowClosed`). Pets don't open at login.
