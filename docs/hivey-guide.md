# hivey — getting started

hivey is a terminal workspace for **swarms of AI agents** (Claude Code and Codex), forked from
[herdr](https://github.com/herdrdev/herdr). It runs next to herdr without touching it.

> **The terminal runtime is swappable.** hivey's own parts (the swarm engine, message bus,
> agent adapters, addons and the `hivey` CLI) talk to the terminal runtime only through a small
> set of API calls. Today that runtime is herdr's core, compiled in. Other multiplexers such as
> **tmux** or **cmux** can plug in as *runtime drivers* in the same way an addon plugs in.
> That is designed ([`hivey-addons.md`](hivey-addons.md)) but **not built yet**: today you run
> hivey on its herdr core.

- What it is and how the parts fit: [`diagrams/hivey-architecture.html`](diagrams/hivey-architecture.html)
- Addons and how to write one: [`../plugins/README.md`](../plugins/README.md)
- What's done and what's next: [`hivey-status.md`](hivey-status.md)

---

## 1. Install and update

Needs git, Rust ([rustup.rs](https://rustup.rs)), Zig **0.16.0**
([ziglang.org/download](https://ziglang.org/download), or `ZIG=/path/to/zig`) and python3,
on macOS or Linux.

```bash
git clone git@github.com:jcsancho/hivey.git ~/hivey
cd ~/hivey && ./install.sh
```

`install.sh` builds hivey, installs `~/.local/bin/hivey`, installs the hivey skill, links the
bundled plugins (dashboard, Slack relay, GitHub, team template, swarm creator, agent creator, skills), turns on the
swarm sidebar and Option keys, asks for your Slack bot token (`hivey slack connect`), your
Obsidian folder for the agents' wiki vaults (and which vault's look new vaults copy) and a
desktop pet (macOS), and sets up the hivey agent (`--no-setup` skips all that). Run it again
any time.

**Desktop pet (macOS, optional):** a small character on your desktop that acts out what your
agents do: it reacts when a swarm starts, while agents work, when one finishes or needs you,
and to every message (also from Slack), and says the important ones out loud. Choose one of
three: **Hivey** (a 3D H whose sections slide apart), **Hivey System** (a dot whose agents ride a
signal wave) or **Hivey Prompt** (a `>_` sphere leading three agents), or none. Change it any
time: right-click the pet (Switch pet, Turn off pet, Speak aloud), hivey's menu → **pets**,
`⌥P`, or `hivey pet` (`hivey pet use hivey-dot`, `hivey pet off`). The pet comes with hivey:
it appears when you open hivey and leaves a few seconds after the last hivey window closes.
**Click the pet** to chat with the hivey agent: type in the box above it and press Enter; the
answer appears in the pet's bubble and is said aloud (Esc closes the box; drag still moves the
pet). Needs Apple's command
line tools to build it (`xcode-select --install`).

**Update:** `hivey update` gets the latest hivey, installs it and moves running sessions to it
live (agents keep running): `hivey updated: hivey 0.9.3 (fc6681b0) → hivey 0.9.3 (92e4d32a)`.
Maintainers use `hivey update --check`: it also merges new herdr commits, runs the tests and a
smoke test, and asks before pushing and installing.

**The hivey agent:** `install.sh` also sets up `~/.hivey/agent`, an always-on Claude that is
your main way into hivey (⬢ `hivey`, the first space). Ask it to launch swarms, check on agents
or ask a swarm's coordinator how it's going. `hivey home setup --slack` adds the Slack channel
`#hivey`, so you can reach it from anywhere. `hivey home` shows its status.

Check the install:

```bash
hivey --version          # hivey 0.9.3
hivey status             # server: running
hivey plugin list        # hivey.slack-relay, hivey.dashboard, hivey.github, hivey.team-template, hivey.agent-creator
```

The **hivey skill** teaches Claude Code and Codex to run hivey for you (sessions, swarms,
plugins, writing new plugins). It ships inside the binary, source in [`../skills/hivey/`](../skills/hivey/).
`hivey skill install` (re)installs it into `~/.claude/skills/hivey` and `~/.codex/skills/hivey`
(`--claude`, `--codex` or `--dir DIR` to choose); run it again after upgrading hivey.

The `/swarm` skill links its own `swarm.watcher` addon the first time it launches in hivey.
The Slack relay's token comes from `~/.config/hivey/plugins/config/hivey.slack-relay/config.json`
(`token_command`, already set to your envsave key).

## 2. Start hivey

Open a terminal tab (a herdr pane works too: hivey ignores herdr's environment) and start it
in a project folder:

```bash
cd ~/projects/<some-project>
hivey                          # the default session
hivey --session <project>      # or one hivey per project
```

One hivey per project folder, as a shell function in `~/.zshrc`:

```zsh
hv() { hivey --session "$(basename "$PWD" | command tr 'A-Z .' 'a-z--')" "$@"; }
```

## 3. Launch a swarm

hivey doesn't decide how a team is designed: a **setup provider** does (a plugin; see
[`../plugins/README.md`](../plugins/README.md#setup-providers)). Yours is the `/swarm` skill:

```bash
cd ~/projects/<some-project>
hivey swarm new "<your task>"        # opens a Claude coordinator running /swarm <task>
hivey swarm providers                # installed providers; --provider <id> --default picks one
```

To write your own provider (a different way of designing teams), see
[`hivey-providers.md`](hivey-providers.md); the flow is drawn in
[`diagrams/hivey-new-swarm.html`](diagrams/hivey-new-swarm.html).

Or, already in a hivey pane, start Claude as you normally do for a coordinator and run
`/swarm <your task>`.

Inside hivey (`HIVEY_ENV=1`) the skill:

- moves your pane into a **new space named after the swarm**, as the master (pane 1)
- opens the agents beside it, each in its own folder `<root>/<agent>/`
- starts three addons first: **Slack relay**, **dashboard**, **watcher**
- writes the agents' briefs with hivey messaging (`hivey msg send …`)

Without the skill, the same launch by hand:

```bash
hivey swarm launch <root> --slug <slug> scout critic \
    --models scout=sonnet --channel <SLACK_CHANNEL_ID> \
    --addon hivey.slack-relay --addon hivey.dashboard
```

Mixed swarms: `--kinds scout=codex` runs that agent with Codex (it reads `AGENTS.md`, which hivey
links to the agent's `CLAUDE.md`). The `/swarm` skill doesn't pass kinds yet.

## 4. While it runs

### Keys (one press, Option = ⌥)

`hivey swarm setup` installs these. Your terminal must send Option as Alt (iTerm: Profiles →
Keys → "Use Option as Meta"; Ghostty: `macos-option-as-alt = true`). Cmd keys can't be used:
macOS terminals keep them and never pass them to programs.

| Keys | What |
|---|---|
| `⌥S` | pick a swarm and jump to its master |
| `⌥M` | jump to this swarm's master |
| `⌥A` | send a message (to an agent, `@all`, `@role:…`; normal / fyi / urgent) |
| `⌥I` | swarm info: agents, Slack channel, Obsidian vault, addons, budget, tasks, repos |
| `⌥L` | this swarm's message log |
| `⌥P` | choose your desktop pet, or none (macOS) |
| `⌥F` | zoom the focused pane to full size, and back |
| `⌥Q` (or `ctrl+b` `q`) | quit (detach; everything keeps running; `hivey` reattaches) |

`⌥F` replaces the shell's "forward word" inside hivey. herdr's `ctrl+b` prefix keys still work, including `ctrl+b q` (quit) and `ctrl+b z` (zoom).
If `⌥` keys type symbols (œ, ƒ…), set your terminal to use Option as Meta (Terminal.app:
Profiles → Keyboard → "Use Option as Meta key"; iTerm2: Profiles → Keys → Left Option = Esc+).

### Mouse

- **Click a swarm** in the sidebar: its panes show and **its coordinator is focused**, ready to type.
- **Hover a swarm**: a card shows its master and agents (kind, model, state), Slack channel,
  Obsidian vault, addons, budget, tasks, GitHub repos and folder.
- **Double-click a pane's title bar** (the `◆ coordinator · demo` line): that agent's terminal
  fills the screen; double-click again to put it back. (Double-click *inside* a terminal still
  selects a word.)

### The screen

- **Left sidebar, "swarms":** one row per swarm, with a summary underneath: `●2/3` working
  of total, `⚠` agents needing attention, `✉` waiting messages, `⏸` paused. The agents themselves
  are the terminals on the right (no separate agents list). Turned on by `hivey swarm setup`
  (`ui.swarm_sidebar = true`).
- **Pane titles:** `◆ coordinator · app · opus` (gold master, blue workers, purple critic,
  gray `▷` scripts/addons). `✉2` means two messages are waiting for that agent.
- **Dashboard pane:** each agent's state and for how long, token bars, a tokens-per-10 s chart,
  budget, task counts and recent messages. `q` closes it;
  `hivey swarm addon <swarm> hivey.dashboard` reopens it.

### Talking to agents

```bash
hivey msg send <swarm>/coordinator "…"     # the master
hivey msg send <swarm>/scout "…"           # one agent
hivey msg send @all --swarm <swarm> "…"    # everyone in the swarm
hivey msg send <swarm>/scout --fyi "…"     # informational: never wakes it
hivey msg send <swarm>/scout --urgent "…"  # now, even mid-task (never into a question dialog)
hivey msg log --swarm <swarm>              # history
```

Normal messages are **delivered when the agent is idle**, never mid-task. Several waiting
messages arrive together as one prompt. If an agent is blocked on a question, its messages are
held and **the master is told once**. Everything is logged in `<root>/.swarm/bus.jsonl`.

**From Slack** (`#swarm-<slug>`): `@scout …` reaches that agent, `@all …` everyone, and a
message with no mention goes to the master. Messages to and from the master are copied back
to the channel. Set `"mirror": "all"` in the relay's `config.json` to see everything.

### Managing a swarm

```bash
hivey swarm list                     # swarms, agents, states, waiting messages
hivey swarm pause <swarm>            # hold all deliveries (⏸); messages still queue
hivey swarm resume <swarm>           # release them
hivey swarm relaunch <swarm>         # restart stopped agents (continuing their conversation)
                                     # and reopen dead addons
hivey swarm relaunch <swarm> <agent> # one agent; --fresh starts a new conversation
hivey swarm addon <swarm> <plugin>   # add an addon to a running swarm
hivey swarm master <swarm> --focus   # jump to its master
hivey swarm info [<swarm>]           # what the swarm uses: agents, Slack, vault, addons…
```

The skill's own pause/restart (`swarm_ctl.py pause|restart`) also works inside hivey.

### Scheduled checks (wake the master on a timer)

The `/swarm` skill asks at plan time whether the master should run scheduled checks. You can
also set them yourself:

```bash
hivey swarm launch … --heartbeat 15m                 # monitoring pass every 15 min, from the start
hivey swarm schedule add <swarm> --every 2h "run the test suite and file failures"
hivey swarm schedule add <swarm> --at 09:00 "summarize yesterday's progress for the user"
hivey swarm schedule add <swarm> --every 30m --to critic "review anything marked DONE"
hivey swarm schedule list [<swarm>]
hivey swarm schedule run <swarm> <id>                 # fire one now
hivey swarm schedule remove <swarm> <id>
```

When one is due, the master (or `--to` agent) gets a `[hivey]` message with the task plus a
status snapshot: the summary, who needs attention (blocked, finished, gone), and how many
messages arrived since the last check. It's **delivered only when that agent is idle**, so it
never interrupts work. A wake-up that's still waiting is never duplicated, and paused swarms are
skipped. Each wake-up costs one turn of that agent's model. Schedules live in
`<root>/.swarm/schedules.json` and show up in the hover card and `hivey swarm info`.

## 5. After restarting hivey (or the Mac)

The layout comes back, and agents with a conversation resume **with their original flags**
(permission mode, model, folders). Then run:

```bash
hivey swarm relaunch <swarm>
```

to restart any agent that came back as a plain shell. Addons (dashboard, Slack relay) are
reopened by hivey on its own about 45 seconds after the server starts.

## 6. Publish to GitHub (two repos)

The product and the swarm workspace are published separately:

- `<you>/<slug>`: the product folder (default `<root>/app`, or `"product_dir"` in
  `.swarm/agents.json`)
- `<you>/<slug>-swarm`: the workspace (agent folders, briefs, logs), without the product, the
  builders' worktrees, secrets or runtime files

```bash
hivey swarm addon <swarm> hivey.github
```

The pane shows the plan and **creates nothing until you answer `y`**. gitleaks scans what's
committed before each first push; any finding stops it. Afterwards, `p` pushes both.
Try it safely first with `HIVEY_GITHUB_DRY_RUN=1`.

## 7. Before you rely on it

- **It's new.** Everything was tested live on small Haiku swarms, not yet on a real
  multi-hour swarm. Start with a small real task (2–3 agents, Sonnet) and watch the dashboard.
- **Leave running herdr swarms in herdr.** Importing one into hivey only shows its agents as
  "gone" (they live in herdr's server). Start new swarms in hivey.
- **Don't run `hivey integration install`.** The Claude/Codex hooks show as "outdated" only
  because hivey is built from a newer herdr; they're shared with your herdr and work for both.
- **Back up the skill:** `/save-skill swarm` syncs `~/.claude/skills/swarm` to `~/SKILLS`
  (a pre-hivey copy is in `~/.claude/skill-backups/`).
- **Repo rules for future sessions:** in `~/projects/hivey`, run
  `ln -sfh HIVEY.md CLAUDE.md`.

## Troubleshooting

| Symptom | Fix |
|---|---|
| A message never arrives | `hivey swarm list`: is the agent `working` (it waits) or `blocked` (the master was told)? Is the swarm `⏸` paused? |
| `"x" needs a swarm` | Add `--swarm <slug>` or address it as `<swarm>/<agent>` |
| Agent shows `gone` | `hivey swarm relaunch <swarm>` |
| Agents asking for permission after a restart | Relaunch them once: `hivey swarm relaunch <swarm> <agent>` (records their flags) |
| Dashboard closed | `hivey swarm addon <swarm> hivey.dashboard` |
| `⌥Q` types a symbol (œ) instead of closing hivey | Your terminal doesn't send Option as Alt: use `ctrl+b` then `q`, or close the terminal tab (agents keep running). To fix ⌥ keys: Terminal.app → Settings → Profiles → Keyboard → "Use Option as Meta key"; iTerm2 → Profiles → Keys → Left Option = Esc+; Ghostty: `macos-option-as-alt = true` |
| The desktop pet doesn't leave after closing hivey | A hivey window is still open somewhere (another tab, or ⌥Q didn't close it, see above). It leaves ~6s after the last one closes. Find them: `ls ~/.hivey/windows` (one file per open window, named by its process id) and `ps -o tty= -p <id>` for its terminal |
| Agents waking each other forever | Can't happen: two agents can wake each other at most 6 times in 5 min; further messages become FYI |
