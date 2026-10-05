---
name: hivey
description: Operate hivey, the terminal workspace for swarms of AI agents (a herdr fork). Use when the user says hivey (or "hyver"/"hybrid" meaning hivey) and wants to start, attach, detach, close or stop hivey sessions; create, launch, inspect, pause, relaunch or message swarms; schedule master check-ins; list, install, link, enable or configure plugins/addons (Slack relay, dashboard, GitHub, watcher); or write a new hivey plugin (an addon pane or a swarm setup provider). Also for hivey troubleshooting (keys, click-to-master, missing features after an upgrade).
---

# hivey

hivey runs swarms of Claude Code / Codex agents: **one space per swarm, pane 1 = the master
(coordinator)**, a message bus between agents (`hivey msg`), addons (plugins) in the swarm's space,
and a "swarms" sidebar. It's a fork of herdr; every `herdr` command works as `hivey`.

- Binary: `hivey` (`~/.local/bin/hivey`). Repo: the clone hivey was installed from (`hivey update` uses it; `HIVEY_REPO` overrides)
  (guide `docs/hivey-guide.md`, plugins `plugins/`, provider guide `docs/hivey-providers.md`).
- Config: `~/.config/hivey/config.toml`. Plugin configs: `hivey plugin config-dir <id>`.
- The original `herdr` (separate binary and server) may also be running: never touch it.

## Rules

- **Stopping a session stops its agents.** Detaching doesn't. Ask before `session stop`,
  `server stop`, closing panes, or `swarm forget` on a swarm that is working.
- Prefer detach + attach. A stopped swarm comes back with `hivey swarm relaunch <slug>`
  (agents continue their conversations).
- Confirm before anything outward-facing (Slack posts, GitHub repos); the GitHub addon already asks.
- Never run `hivey integration install` (it rewrites shared agent hooks). `hivey update` is safe
  (builds from the user's hivey clone, hands sessions off live); `--check` pushes, so ask first.
- Secrets: never write tokens into files or prompts; plugins read them via `token_command`
  (e.g. envsave). Key *names* are fine.

## Sessions: start, attach, detach, close

Each project can have its own session (server + its swarms).

| Want | Do |
|---|---|
| open hivey (default session) | `hivey` |
| per-project session | `hivey --session <name>` (start or attach) |
| list sessions | `hivey session list` |
| **detach** (leave, everything keeps running) | `⌥Q` or `ctrl+b` then `q` in the hivey terminal |
| re-attach | `hivey session attach <name>` / `hivey` |
| **stop** a session (agents stop too, ask first) | `hivey session stop <name>`; default: `hivey server stop` |
| delete a stopped session | `hivey session delete <name>` |

Target a session from outside it: `hivey --session <name> <command>` (e.g.
`hivey --session test-project swarm list`). Inside a hivey pane, commands go to that session.

Keys (after `hivey swarm setup`): `⌥S` swarms picker, `⌥M` master, `⌥A` all agents, `⌥I` info,
`⌥L` message log, `⌥F` / double-click a pane title = zoom, `⌥Q` quit. The `ctrl+b` prefix keys
still work. If `⌥` types symbols, the terminal must send Option as Meta (Terminal.app: Profiles →
Keyboard → "Use Option as Meta key"; iTerm2: Profiles → Keys → Left Option = Esc+).
Mouse: click a swarm in the sidebar → focuses its master; hover → info card.

## Swarms

```bash
hivey swarm new "<task>"                      # the swarm creator designs + launches it (asks first)
hivey swarm new --agent "<task>"              # one solo agent in this folder (agent creator)
hivey swarm providers [--default <id>]        # swarm / agent creators; * = used (settings → plugins)
hivey skills [list|guide|copy|add|find|dir|online]  # skills library for new agents (settings → skills)
hivey swarm list [--json]                     # swarms, agents, roles, states, queued messages
hivey swarm info [<slug>]                     # agents, Slack channel, vault, addons, budget, tasks…
hivey swarm master [<slug>] [--focus]         # the master pane (focus it)
hivey swarm pause|resume <slug>               # hold / release message delivery
hivey swarm relaunch <slug> [<agent>...]      # restart agents (resume conversations) + dead addons
hivey swarm relaunch <slug> --addons-only     # only reopen addons; --fresh: new conversations
hivey swarm register <root> | unregister <slug>  # (alias import / forget; files kept)
hivey swarm directory [--json]                # every swarm + solo agent: what it does, skills,
                                              # tools, busy/idle, address (<slug>/master)
hivey swarm profile <slug> --description "…" --skills a,b --tools x,y   # set its entry
```

Lower level (what providers call): `hivey swarm launch <root> --slug S <agent>... [--models a=sonnet]
[--kinds b=codex] [--addon ID]... [--channel C0…] [--heartbeat 15m --heartbeat-task "…"]
[--budget-min N] [--kickoff "…"]`. Run it from the pane that should become the master.

Scheduled check-ins (wake the master, or `--to AGENT`, with a task):
```bash
hivey swarm schedule add <slug> --every 30m "Check progress; unblock anyone stuck"
hivey swarm schedule add <slug> --at 09:00 --id morning "Morning status report to the user"
hivey swarm schedule list|remove|run [<slug>] [<id>]
```

Messages (delivered when the target is idle; `--fyi` never wakes; `--urgent` now):
```bash
hivey msg send <agent|@all|@role:worker|@masters|<swarm>/<agent>|human> "text" [--fyi|--urgent] [--swarm S]
hivey msg inbox [--agent S/A]      hivey msg log [--swarm S] [--limit N]      hivey msg compose
```

Panes/agents (herdr layer): `hivey pane list`, `hivey pane read <pane> --source visible`,
`hivey pane send-text <pane> "…"` + `hivey pane send-keys <pane> enter`, `hivey agent list`.

## Plugins (addons)

```bash
hivey plugin list                               # installed, enabled, source, config dir
hivey plugin link <dir>                         # a local plugin folder (edits apply on reopen)
hivey plugin install <owner/repo[/subdir]> [--ref R]
hivey plugin enable|disable|unlink|uninstall <id>
hivey plugin config-dir <id>                    # where its config.json lives
hivey plugin log <id>                           # its command logs
hivey swarm addon <slug> <id>[:<pane>]...       # open addons in a running swarm
hivey swarm launch … --addon <id>               # open them at launch
```

Bundled in the repo's `plugins/` (linked by `install.sh`): the seven `hivey.*` ones. The others
come with the /swarm and /agent-creator skills and are only there when those are installed;
check `hivey plugin list` / `hivey swarm providers` before relying on them.

| id | what |
|---|---|
| `hivey.slack-relay` | Slack ⇄ bus bridge (config `token_command`, channel per swarm) |
| `hivey.dashboard` | agents, states, tokens per agent, active time vs budget, tasks, messages |
| `hivey.github` | product + swarm repos, asks before creating anything |
| `hivey.team-template` | example setup provider (builder + critic); copy it |
| `hivey.swarm-creator` | built-in Swarm creator: a coordinator designs a team for any task (always there) |
| `hivey.agent-creator` | built-in Agent creator: one solo agent in the current folder (always there) |
| `hivey.skills` | built-in skills plugin: how creators pick, find and install each agent's skills (`hivey skills guide`) |
| `swarm.watcher` | /swarm skill addon: stalled-agent and budget alerts to the master |
| `swarm.creator` | Swarm creator from the /swarm skill (setup provider) |
| `agent.creator` | Agent creator from the /agent-creator skill: one solo agent in the current folder |

**Creating a swarm or agent: always ask the user three things first** (one question each, then
act on the answers):
1. **Name?** Suggest one from the task; normalize to `[a-z][a-z0-9-]{0,31}`. It's the one name
   used everywhere: the swarm/agent slug (`--slug`), its Slack channel `#<name>` and its
   Obsidian vault `<name>-wiki` (`new_wiki.py <name>`). Never pick a different name for any.
2. **Slack channel?** (see below)
3. **Obsidian wiki memory?** A vault in the user's Obsidian where it keeps what it learns. If
   yes and you don't know where their Obsidian is, ask once (default `~/Obsidian`; it's then
   remembered in `~/.hivey/wiki.json`). Create it with the `agents-create-wiki` skill if it's
   installed, else `python3 <this skill>/scripts/new_wiki.py <slug> [--dir <obsidian>] --agent
   <agent folder> [--agent …] --about "<one line>"` (a team: one vault, every agent folder
   linked). Do it after the agents' CLAUDE.md files exist, and tell the user the vault path.
   New vaults copy the theme and Style Settings of the vault remembered as `look_from` in
   `~/.hivey/wiki.json` (set it once with `--look-from <their main vault>`); on first open
   the user must click "Turn on community plugins" for the folder colors to show.

## Slack

```bash
hivey slack connect [--force]     # once per machine, in a terminal: Slack app + bot token (hidden)
hivey slack status                # connected? workspace, bot, missing scopes
hivey swarm launch … --slack      # new swarm/agent with its own channel #<slug> + Slack relay
hivey slack add <slug>            # channel for one that's already running
hivey home setup --slack          # #hivey for the hivey agent
```
New channels invite the user (relay config `invite`: their Slack member id, set by `hivey slack
connect` or worked out from shared channels), so they appear in their Slack at once.
**Before creating a channel, check Slack is connected:** `hivey slack status`. Not connected →
ask the user to run `hivey slack connect` first (once; the token stays saved, and hivey, its
relays and the /swarm skill all use it), don't try and fail.
**Rule for every agent and swarm:** a Slack channel you create must be visible to the user
right away. Create channels only through hivey (`--slack`, `hivey slack add`, `hivey home
setup --slack`), which invites them; if one was made another way (e.g. the /swarm skill's
`swarm_slack.py create`, which also invites), make sure the user is in it (`hivey slack add
<slug>`) before saying it's done.
When creating a swarm or agent, ask the user whether it should get a Slack channel. Never ask
for the Slack token in chat: the user types it into `hivey slack connect`.

## Desktop pet (macOS)

```bash
hivey pet                         # chosen pet, running or not, the pets to choose from
hivey pet use hivey-h|hivey-dot|hivey-prompt   # switch (built the first time, ~1 min)
hivey pet off                     # no pet
hivey pet choose                  # interactive picker (also ⌥P, and hivey menu → pets)
```
Clicking the pet opens a chat box: it sends `hivey msg send hivey/master "…"` (from `human`);
the hivey agent's `hivey msg send human` reply appears in the pet's bubble, spoken.
The pet shows while a hivey window is open (it appears with the first window and quits
~6s after the last one closes). It watches hivey (swarm starts, working, finished, needs you, messages incl. Slack) and
says the important ones aloud; right-click it for Switch pet / Turn off pet / Speak aloud.
Sources: `pets/<id>/` + `pets/shared/HiveyWatch.swift` in the hivey repo. Only switch or turn
it off when the user asks.

## The hivey agent (`hivey home`)

An always-on Claude in `~/.hivey/agent` (solo agent `hivey`, ⬢ mauve, first space): the user's
main way into hivey, also from Slack `#hivey` (relay addon). It uses this skill to launch and
check swarms and agents. While enabled, the server of its session restarts it when it's gone.
```bash
hivey home                       # status: running?, session, model, Slack channel
hivey home setup [--slack] [--model M] [--force]   # create/refresh it in THIS session
hivey home start | enable | disable
hivey msg send hivey/master "…"  # talk to it from anywhere
```
Settings: `~/.hivey/config.json`; its brief: `~/.hivey/agent/CLAUDE.md` (kept on setup unless
`--force`); hivey's config: `~/.hivey/config` → `~/.config/hivey`.

## Solo agents

A solo agent is a swarm with one member that is its own master: own space, ★ teal row in the
sidebar, messages, schedules and heartbeats like a master. Create one from the folder it will
live in: `mkdir -p <folder> && cd <folder> && hivey swarm new --agent "<task>"` (the agent
creator chosen in settings → plugins, else the built-in `hivey.agent-creator`). The designer
pane stays; close it once the agent runs. Lower level: `hivey swarm launch <folder> --slug S --solo
[--model M] [--kind codex] [--description … --skills … --tools …]`. Relaunch with
`hivey swarm relaunch <slug>` (new space if its pane is gone).

**Talking across swarms/agents:** only masters (and solo agents) may message another swarm,
`hivey msg send <slug>/master "…"`. Agents check `hivey swarm directory` first, **ask the user
before sending any work**, and don't disturb entries marked busy.

An addon whose process exits (e.g. `q` in the dashboard) is reopened with
`hivey swarm addon <slug> <id>` or `hivey swarm relaunch <slug> --addons-only`. Plugin code
changes apply when its pane is reopened.

## Create a new plugin

A plugin is a folder with `herdr-plugin.toml` + any program. Two kinds matter for swarms:

**1. Addon (a pane in each swarm's space):** relays, dashboards, monitors.
```toml
id = "me.my-addon"
name = "My addon"
version = "0.1.0"
min_herdr_version = "0.9.0"
platforms = ["linux", "macos"]

[[panes]]
id = "relay"            # default entrypoint; others are opened as me.my-addon:<pane>
title = "my addon"
placement = "split"
command = ["sh", "-c", "exec python3 \"$HERDR_PLUGIN_ROOT/main.py\""]
```
It runs in the swarm root with `HIVEY_SWARM_ROOT`, `HIVEY_SWARM_SLUG`, `HIVEY_SWARM_CHANNEL`,
`HERDR_BIN_PATH`, `HERDR_PLUGIN_CONFIG_DIR`, `HERDR_PLUGIN_STATE_DIR`.
- Read the roster: `<root>/.swarm/agents.json`; live states: `hivey swarm list --json`.
- Outside → agents: `"$HERDR_BIN_PATH" msg send <to> "text" --swarm "$HIVEY_SWARM_SLUG" --from human`.
- Agents → outside: tail `<root>/.swarm/bus.jsonl` (`"ev":"msg"` lines; skip `copy: true`).
- Keep state on disk, never exit on network errors. Full contract: `plugins/README.md`.

**2. Setup provider (designs a swarm for `hivey swarm new`):** a pane with `id = "setup"`.
It reads `HIVEY_SETUP_TASK` / `HIVEY_SETUP_CWD`, then: design the team (confirm with the user) →
write `<root>/<agent>/CLAUDE.md` → `hivey swarm launch <root> --slug … <agents>` from its own pane
→ `hivey swarm accept-trust --pane "$HERDR_PANE_ID" &` → `exec claude …` (it becomes the master).
Walkthrough: `docs/hivey-providers.md`; template: `plugins/team-template/`.

Workflow for a new plugin:
1. Copy the closest example (`plugins/dashboard`, `plugins/slack-relay`, `plugins/team-template`)
   to a new folder; change `id`, `name`, the pane `command`.
2. Unit-test the pure parts (`python3 -m unittest`), like the examples' `test_*.py`.
3. `hivey plugin link <dir>`, then try it on a small swarm (Haiku models, short budget):
   `hivey swarm addon <slug> <id>` or `hivey swarm new --provider <id> "<task>"`.
4. Check `hivey plugin log <id>` if the pane dies.

## Troubleshooting

- **A feature is missing in one session** (no click-to-master, no swarm summary row/hover info,
  "config.toml has unknown keys" banner): that session's server predates the binary. Compare
  `ps -o lstart= -p <pid>` with `ls -l ~/.local/bin/hivey`; find the pid with
  `lsof -U | grep sessions/<name>/herdr.sock`. Fix, after asking: a live handoff keeps agents
  running: `python3 <this skill>/scripts/handoff.py <name>` (or stop + `hivey swarm relaunch`).
- **`ctrl+b q` / `⌥Q` don't quit:** check `[keys] detach` in config.toml; it should be
  `["alt+q", "prefix+q"]` (rerun `hivey swarm setup`), then `hivey server reload-config`.
- **`⌥Q` types a symbol (œ) / hivey doesn't close:** the terminal doesn't send Option as Alt.
  Tell the user: `ctrl+b` then `q` always works, or close the terminal tab (agents keep
  running); to fix ⌥ keys set "Use Option as Meta" (Terminal.app Profiles → Keyboard; iTerm2
  Left Option = Esc+; Ghostty `macos-option-as-alt = true`).
- **The desktop pet doesn't leave:** a hivey window is still open. `ls ~/.hivey/windows` lists
  them (one file per window, named by pid; `ps -o tty=,command= -p <pid>` shows the terminal).
  The pet quits ~6s after the last one closes and comes back with the next `hivey`.
- **Agent shows blocked:** it's waiting on a dialog: `hivey pane read <pane> --source visible`.
- **Messages not delivered:** `hivey swarm list` (✉ queued), is the swarm paused?
  `hivey msg log --swarm <slug>`.
- **Update this skill:** `hivey skill install` (it ships inside the hivey binary; source
  `skills/hivey/` in the repo).
- **Install / update hivey:** first install from a clone: `./install.sh`. Update: `hivey update`
  (latest hivey, installed, running sessions handed off live; prints old → new version).
  Maintainer: `hivey update --check` (merges herdr, tests, asks before push + install).
