# hiver — getting started

hiver is a terminal workspace for **swarms of AI agents** (Claude Code and Codex), forked from
[herdr](https://github.com/herdrdev/herdr). It runs next to herdr without touching it.

> **The terminal runtime is swappable.** hiver's own parts (the swarm engine, message bus,
> agent adapters, addons and the `hiver` CLI) talk to the terminal runtime only through a small
> set of API calls. Today that runtime is herdr's core, compiled in. Other multiplexers such as
> **tmux** or **cmux** can plug in as *runtime drivers* in the same way an addon plugs in.
> That is designed ([`hiver-addons.md`](hiver-addons.md)) but **not built yet**: today you run
> hiver on its herdr core.

- What it is and how the parts fit: [`diagrams/hiver-architecture.html`](diagrams/hiver-architecture.html)
- Addons and how to write one: [`../plugins/README.md`](../plugins/README.md)
- What's done and what's next: [`hiver-status.md`](hiver-status.md)

---

## 1. Check the install

```bash
hiver --version          # hiver 0.9.3 (herdr fork)
hiver status             # server: running
hiver plugin list        # hiver.slack-relay, hiver.dashboard, hiver.github, swarm.watcher
```

If something is missing:

```bash
cd ~/projects/swarmAgents/hiver
cargo build --release && cp target/release/hiver ~/.local/bin/hiver.new && mv ~/.local/bin/hiver.new ~/.local/bin/hiver
hiver plugin link ~/projects/swarmAgents/hiver/plugins/slack-relay
hiver plugin link ~/projects/swarmAgents/hiver/plugins/dashboard
hiver plugin link ~/projects/swarmAgents/hiver/plugins/github
hiver swarm setup                        # swarm sidebar + Option keys (see section 4)
```

The `/swarm` skill links its own `swarm.watcher` addon the first time it launches in hiver.
The Slack relay's token comes from `~/.config/hiver/plugins/config/hiver.slack-relay/config.json`
(`token_command`, already set to your envsave key).

## 2. Start hiver

Open a terminal tab (a herdr pane works too: hiver ignores herdr's environment) and start it
in a project folder:

```bash
cd ~/projects/<some-project>
hiver                          # the default session
hiver --session <project>      # or one hiver per project
```

One hiver per project folder, as a shell function in `~/.zshrc`:

```zsh
hv() { hiver --session "$(basename "$PWD" | command tr 'A-Z .' 'a-z--')" "$@"; }
```

## 3. Launch a swarm

hiver doesn't decide how a team is designed: a **setup provider** does (a plugin; see
[`../plugins/README.md`](../plugins/README.md#setup-providers)). Yours is the `/swarm` skill:

```bash
cd ~/projects/<some-project>
hiver swarm new "<your task>"        # opens a Claude coordinator running /swarm <task>
hiver swarm providers                # installed providers; --provider <id> --default picks one
```

To write your own provider (a different way of designing teams), see
[`hiver-providers.md`](hiver-providers.md); the flow is drawn in
[`diagrams/hiver-new-swarm.html`](diagrams/hiver-new-swarm.html).

Or, already in a hiver pane, start Claude as you normally do for a coordinator and run
`/swarm <your task>`.

Inside hiver (`HIVER_ENV=1`) the skill:

- moves your pane into a **new space named after the swarm**, as the master (pane 1)
- opens the agents beside it, each in its own folder `<root>/<agent>/`
- starts three addons first: **Slack relay**, **dashboard**, **watcher**
- writes the agents' briefs with hiver messaging (`hiver msg send …`)

Without the skill, the same launch by hand:

```bash
hiver swarm launch <root> --slug <slug> scout critic \
    --models scout=sonnet --channel <SLACK_CHANNEL_ID> \
    --addon hiver.slack-relay --addon hiver.dashboard
```

Mixed swarms: `--kinds scout=codex` runs that agent with Codex (it reads `AGENTS.md`, which hiver
links to the agent's `CLAUDE.md`). The `/swarm` skill doesn't pass kinds yet.

## 4. While it runs

### Keys (one press, Option = ⌥)

`hiver swarm setup` installs these. Your terminal must send Option as Alt (iTerm: Profiles →
Keys → "Use Option as Meta"; Ghostty: `macos-option-as-alt = true`). Cmd keys can't be used:
macOS terminals keep them and never pass them to programs.

| Keys | What |
|---|---|
| `⌥S` | pick a swarm and jump to its master |
| `⌥M` | jump to this swarm's master |
| `⌥A` | send a message (to an agent, `@all`, `@role:…`; normal / fyi / urgent) |
| `⌥I` | swarm info: agents, Slack channel, Obsidian vault, addons, budget, tasks, repos |
| `⌥L` | this swarm's message log |
| `⌥F` | zoom the focused pane to full size, and back |
| `⌥Q` | quit (detach; everything keeps running; `hiver` reattaches) |

`⌥F` replaces the shell's "forward word" inside hiver. herdr's `ctrl+b` prefix keys still work.

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
  are the terminals on the right (no separate agents list). Turned on by `hiver swarm setup`
  (`ui.swarm_sidebar = true`).
- **Pane titles:** `◆ coordinator · app · opus` (gold master, blue workers, purple critic,
  gray `▷` scripts/addons). `✉2` means two messages are waiting for that agent.
- **Dashboard pane:** each agent's state and for how long, token bars, a tokens-per-10 s chart,
  budget, task counts and recent messages. `q` closes it;
  `hiver swarm addon <swarm> hiver.dashboard` reopens it.

### Talking to agents

```bash
hiver msg send <swarm>/coordinator "…"     # the master
hiver msg send <swarm>/scout "…"           # one agent
hiver msg send @all --swarm <swarm> "…"    # everyone in the swarm
hiver msg send <swarm>/scout --fyi "…"     # informational: never wakes it
hiver msg send <swarm>/scout --urgent "…"  # now, even mid-task (never into a question dialog)
hiver msg log --swarm <swarm>              # history
```

Normal messages are **delivered when the agent is idle**, never mid-task. Several waiting
messages arrive together as one prompt. If an agent is blocked on a question, its messages are
held and **the master is told once**. Everything is logged in `<root>/.swarm/bus.jsonl`.

**From Slack** (`#swarm-<slug>`): `@scout …` reaches that agent, `@all …` everyone, and a
message with no mention goes to the master. Messages to and from the master are copied back
to the channel. Set `"mirror": "all"` in the relay's `config.json` to see everything.

### Managing a swarm

```bash
hiver swarm list                     # swarms, agents, states, waiting messages
hiver swarm pause <swarm>            # hold all deliveries (⏸); messages still queue
hiver swarm resume <swarm>           # release them
hiver swarm relaunch <swarm>         # restart stopped agents (continuing their conversation)
                                     # and reopen dead addons
hiver swarm relaunch <swarm> <agent> # one agent; --fresh starts a new conversation
hiver swarm addon <swarm> <plugin>   # add an addon to a running swarm
hiver swarm master <swarm> --focus   # jump to its master
hiver swarm info [<swarm>]           # what the swarm uses: agents, Slack, vault, addons…
```

The skill's own pause/restart (`swarm_ctl.py pause|restart`) also works inside hiver.

### Scheduled checks (wake the master on a timer)

The `/swarm` skill asks at plan time whether the master should run scheduled checks. You can
also set them yourself:

```bash
hiver swarm launch … --heartbeat 15m                 # monitoring pass every 15 min, from the start
hiver swarm schedule add <swarm> --every 2h "run the test suite and file failures"
hiver swarm schedule add <swarm> --at 09:00 "summarize yesterday's progress for the user"
hiver swarm schedule add <swarm> --every 30m --to critic "review anything marked DONE"
hiver swarm schedule list [<swarm>]
hiver swarm schedule run <swarm> <id>                 # fire one now
hiver swarm schedule remove <swarm> <id>
```

When one is due, the master (or `--to` agent) gets a `[hiver]` message with the task plus a
status snapshot: the summary, who needs attention (blocked, finished, gone), and how many
messages arrived since the last check. It's **delivered only when that agent is idle**, so it
never interrupts work. A wake-up that's still waiting is never duplicated, and paused swarms are
skipped. Each wake-up costs one turn of that agent's model. Schedules live in
`<root>/.swarm/schedules.json` and show up in the hover card and `hiver swarm info`.

## 5. After restarting hiver (or the Mac)

The layout comes back, and agents with a conversation resume **with their original flags**
(permission mode, model, folders). Then run:

```bash
hiver swarm relaunch <swarm>
```

to restart any agent that came back as a plain shell, and to reopen the dashboard and relay.

## 6. Publish to GitHub (two repos)

The product and the swarm workspace are published separately:

- `<you>/<slug>`: the product folder (default `<root>/app`, or `"product_dir"` in
  `.swarm/agents.json`)
- `<you>/<slug>-swarm`: the workspace (agent folders, briefs, logs), without the product, the
  builders' worktrees, secrets or runtime files

```bash
hiver swarm addon <swarm> hiver.github
```

The pane shows the plan and **creates nothing until you answer `y`**. gitleaks scans what's
committed before each first push; any finding stops it. Afterwards, `p` pushes both.
Try it safely first with `HIVER_GITHUB_DRY_RUN=1`.

## 7. Before you rely on it

- **It's new.** Everything was tested live on small Haiku swarms, not yet on a real
  multi-hour swarm. Start with a small real task (2–3 agents, Sonnet) and watch the dashboard.
- **Leave running herdr swarms in herdr.** Importing one into hiver only shows its agents as
  "gone" (they live in herdr's server). Start new swarms in hiver.
- **Don't run `hiver integration install`.** The Claude/Codex hooks show as "outdated" only
  because hiver is built from a newer herdr; they're shared with your herdr and work for both.
- **Back up the skill:** `/save-skill swarm` syncs `~/.claude/skills/swarm` to `~/SKILLS`
  (a pre-hiver copy is in `~/.claude/skill-backups/`).
- **Repo rules for future sessions:** in `~/projects/swarmAgents/hiver`, run
  `ln -sfh HIVER.md CLAUDE.md`.

## Troubleshooting

| Symptom | Fix |
|---|---|
| A message never arrives | `hiver swarm list`: is the agent `working` (it waits) or `blocked` (the master was told)? Is the swarm `⏸` paused? |
| `"x" needs a swarm` | Add `--swarm <slug>` or address it as `<swarm>/<agent>` |
| Agent shows `gone` | `hiver swarm relaunch <swarm>` |
| Agents asking for permission after a restart | Relaunch them once: `hiver swarm relaunch <swarm> <agent>` (records their flags) |
| Dashboard closed | `hiver swarm addon <swarm> hiver.dashboard` |
| Agents waking each other forever | Can't happen: two agents can wake each other at most 6 times in 5 min; further messages become FYI |
