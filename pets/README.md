# hiver pets (macOS)

Desktop pets that act out what hiver's agents do. Choose one (or none) with `hiver pet choose`,
`⌥P`, hiver's menu → pets, or `install.sh`; switch from the pet's right-click menu.

| id | name | look |
|---|---|---|
| `hiver-h` | Hiver H | a 3D H; its sections slide apart to hand out work |
| `hiver-dot` | Hiver | a graphite dot; its agents ride a signal wave around it |
| `hiver-prompt` | Hiver Prompt | a `>_` sphere leading three agent spheres |

- `<id>/main.swift` + `<id>/build.sh`: the pet (AppKit + Core Graphics); each `CLAUDE.md` is
  its character spec and behavior, read it before changing a pet.
- `shared/HiverWatch.swift`: compiled into every pet. `HiverWatcher` polls hiver
  (`swarm list` / `msg log`), `HiverSnapshot` turns two looks into `HiverEvent`s (launched,
  needs you, finished, message), `PetSwitcher` is the Switch pet / Turn off pet menu.
- `pet.py`: `hiver pet` (build, install to `~/Applications`, switch, off, show). The
  choice is in `~/.hiver/pet.json`; `hiver update` runs `hiver pet refresh` to rebuild it when
  its source changed.
- Lifecycle: every hiver window writes `~/.hiver/windows/<pid>` while open and runs
  `hiver pet show`, so the pet appears with the first window; the pet quits itself ~6s after the
  last window closes (`HiverWatcher.onLastWindowClosed`). Pets don't open at login.
