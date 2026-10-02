---
name: hiver
description: Operate hiver, the terminal workspace for swarms of AI agents (a herdr fork). Use when the user says hiver (or "hyver"/"hybrid" meaning hiver) and wants to start, attach, detach, close or stop hiver sessions; create, launch, inspect, pause, relaunch or message swarms; schedule master check-ins; list, install, link, enable or configure plugins/addons (Slack relay, dashboard, GitHub, watcher); or write a new hiver plugin (an addon pane or a swarm setup provider). Also for hiver troubleshooting (keys, click-to-master, missing features after an upgrade).
---

# hiver

hiver runs swarms of Claude Code / Codex agents: **one space per swarm, pane 1 = the master
(coordinator)**, a message bus between agents (`hiver msg`), addons (plugins) in the swarm's space,
and a "swarms" sidebar. It's a fork of herdr; every `herdr` command works as `hiver`.

- Binary: `hiver` (`~/.local/bin/hiver`). Repo: the clone hiver was installed from (`hiver update` uses it; `HIVER_REPO` overrides)
  (guide `docs/hiver-guide.md`, plugins `plugins/`, provider guide `docs/hiver-providers.md`).
- Config: `~/.config/hiver/config.toml`. Plugin configs: `hiver plugin config-dir <id>`.
- The original `herdr` (separate binary and server) may also be running: never touch it.

## Rules

- **Stopping a session stops its agents.** Detaching doesn't. Ask before `session stop`,
  `server stop`, closing panes, or `swarm forget` on a swarm that is working.
- Prefer detach + attach. A stopped swarm comes back with `hiver swarm relaunch <slug>`
  (agents continue their conversations).
- Confirm before anything outward-facing (Slack posts, GitHub repos); the GitHub addon already asks.
- Never run `hiver integration install` (it rewrites shared agent hooks). `hiver update` is safe
  (builds from the user's hiver clone, hands sessions off live); `--check` pushes, so ask first.
- Secrets: never write tokens into files or prompts; plugins read them via `token_command`
  (e.g. envsave). Key *names* are fine.

## Sessions: start, attach, detach, close

Each project can have its own session (server + its swarms).

| Want | Do |
|---|---|
| open hiver (default session) | `hiver` |
| per-project session | `hiver --session <name>` (start or attach) |
| list sessions | `hiver session list` |
| **detach** (leave, everything keeps running) | `⌥Q` or `ctrl+b` then `q` in the hiver terminal |
| re-attach | `hiver session attach <name>` / `hiver` |
| **stop** a session (agents stop too, ask first) | `hiver session stop <name>`; default: `hiver server stop` |
| delete a stopped session | `hiver session delete <name>` |

Target a session from outside it: `hiver --session <name> <command>` (e.g.
`hiver --session test-project swarm list`). Inside a hiver pane, commands go to that session.

Keys (after `hiver swarm setup`): `⌥S` swarms picker, `⌥M` master, `⌥A` all agents, `⌥I` info,
`⌥L` message log, `⌥F` / double-click a pane title = zoom, `⌥Q` quit. The `ctrl+b` prefix keys
still work. If `⌥` types symbols, the terminal must send Option as Meta (Terminal.app: Profiles →
Keyboard → "Use Option as Meta key"; iTerm2: Profiles → Keys → Left Option = Esc+).
Mouse: click a swarm in the sidebar → focuses its master; hover → info card.

## Swarms

```bash
hiver swarm new "<task>"                      # a setup provider designs + launches it (asks first)
hiver swarm new --provider <id> --default "<task>"
hiver swarm providers                         # installed setup providers
hiver swarm list [--json]                     # swarms, agents, roles, states, queued messages
hiver swarm info [<slug>]                     # agents, Slack channel, vault, addons, budget, tasks…
hiver swarm master [<slug>] [--focus]         # the master pane (focus it)
hiver swarm pause|resume <slug>               # hold / release message delivery
hiver swarm relaunch <slug> [<agent>...]      # restart agents (resume conversations) + dead addons
hiver swarm relaunch <slug> --addons-only     # only reopen addons; --fresh: new conversations
hiver swarm register <root> | unregister <slug>  # (alias import / forget; files kept)
hiver swarm directory [--json]                # every swarm + solo agent: what it does, skills,
                                              # tools, busy/idle, address (<slug>/master)
hiver swarm profile <slug> --description "…" --skills a,b --tools x,y   # set its entry
```

Lower level (what providers call): `hiver swarm launch <root> --slug S <agent>... [--models a=sonnet]
[--kinds b=codex] [--addon ID]... [--channel C0…] [--heartbeat 15m --heartbeat-task "…"]
[--budget-min N] [--kickoff "…"]`. Run it from the pane that should become the master.

Scheduled check-ins (wake the master, or `--to AGENT`, with a task):
```bash
hiver swarm schedule add <slug> --every 30m "Check progress; unblock anyone stuck"
hiver swarm schedule add <slug> --at 09:00 --id morning "Morning status report to the user"
hiver swarm schedule list|remove|run [<slug>] [<id>]
```

Messages (delivered when the target is idle; `--fyi` never wakes; `--urgent` now):
```bash
hiver msg send <agent|@all|@role:worker|@masters|<swarm>/<agent>|human> "text" [--fyi|--urgent] [--swarm S]
hiver msg inbox [--agent S/A]      hiver msg log [--swarm S] [--limit N]      hiver msg compose
```

Panes/agents (herdr layer): `hiver pane list`, `hiver pane read <pane> --source visible`,
`hiver pane send-text <pane> "…"` + `hiver pane send-keys <pane> enter`, `hiver agent list`.

## Plugins (addons)

```bash
hiver plugin list                               # installed, enabled, source, config dir
hiver plugin link <dir>                         # a local plugin folder (edits apply on reopen)
hiver plugin install <owner/repo[/subdir]> [--ref R]
hiver plugin enable|disable|unlink|uninstall <id>
hiver plugin config-dir <id>                    # where its config.json lives
hiver plugin log <id>                           # its command logs
hiver swarm addon <slug> <id>[:<pane>]...       # open addons in a running swarm
hiver swarm launch … --addon <id>               # open them at launch
```

Bundled in the repo's `plugins/` (linked by `install.sh`): the four `hiver.*` ones. The others
come with the /swarm and /agent-creator skills and are only there when those are installed;
check `hiver plugin list` / `hiver swarm providers` before relying on them.

| id | what |
|---|---|
| `hiver.slack-relay` | Slack ⇄ bus bridge (config `token_command`, channel per swarm) |
| `hiver.dashboard` | agents, states, tokens per agent, active time vs budget, tasks, messages |
| `hiver.github` | product + swarm repos, asks before creating anything |
| `hiver.team-template` | example setup provider (builder + critic); copy it |
| `swarm.watcher` | /swarm skill addon: stalled-agent and budget alerts to the master |
| `swarm.skill` | /swarm skill as a setup provider |
| `agent.creator` | /agent-creator skill: one solo agent in the current folder |

**Creating a swarm or agent: always ask the user two things first** (one question each, then
act on the answers):
1. **Slack channel?** (see below)
2. **Obsidian wiki memory?** A vault in the user's Obsidian where it keeps what it learns. If
   yes and you don't know where their Obsidian is, ask once (default `~/Obsidian`; it's then
   remembered in `~/.hiver/wiki.json`). Create it with the `agents-create-wiki` skill if it's
   installed, else `python3 <this skill>/scripts/new_wiki.py <slug> [--dir <obsidian>] --agent
   <agent folder> [--agent …] --about "<one line>"` (a team: one vault, every agent folder
   linked). Do it after the agents' CLAUDE.md files exist, and tell the user the vault path.

## Slack

```bash
hiver slack connect [--force]     # once per machine, in a terminal: Slack app + bot token (hidden)
hiver slack status                # connected? workspace, bot, missing scopes
hiver swarm launch … --slack      # new swarm/agent with its own channel #<slug> + Slack relay
hiver slack add <slug>            # channel for one that's already running
hiver home setup --slack          # #hiver for the hiver agent
```
New channels invite the user (relay config `invite`: their Slack member id, set by `hiver slack
connect` or worked out from shared channels), so they appear in their Slack at once.
**Before creating a channel, check Slack is connected:** `hiver slack status`. Not connected →
ask the user to run `hiver slack connect` first (once; the token stays saved, and hiver, its
relays and the /swarm skill all use it), don't try and fail.
**Rule for every agent and swarm:** a Slack channel you create must be visible to the user
right away. Create channels only through hiver (`--slack`, `hiver slack add`, `hiver home
setup --slack`), which invites them; if one was made another way (e.g. the /swarm skill's
`swarm_slack.py create`, which also invites), make sure the user is in it (`hiver slack add
<slug>`) before saying it's done.
When creating a swarm or agent, ask the user whether it should get a Slack channel. Never ask
for the Slack token in chat: the user types it into `hiver slack connect`.

## Desktop pet (macOS)

```bash
hiver pet                         # chosen pet, running or not, the pets to choose from
hiver pet use hiver-h|hiver-dot|hiver-prompt   # switch (built the first time, ~1 min)
hiver pet off                     # no pet
hiver pet choose                  # interactive picker (also ⌥P, and hiver menu → pets)
```
Clicking the pet opens a chat box: it sends `hiver msg send hiver/master "…"` (from `human`);
the hiver agent's `hiver msg send human` reply appears in the pet's bubble, spoken.
The pet shows while a hiver window is open (it appears with the first window and quits
~6s after the last one closes). It watches hiver (swarm starts, working, finished, needs you, messages incl. Slack) and
says the important ones aloud; right-click it for Switch pet / Turn off pet / Speak aloud.
Sources: `pets/<id>/` + `pets/shared/HiverWatch.swift` in the hiver repo. Only switch or turn
it off when the user asks.

## The hiver agent (`hiver home`)

An always-on Claude in `~/.hiver/agent` (solo agent `hiver`, ⬢ mauve, first space): the user's
main way into hiver, also from Slack `#hiver` (relay addon). It uses this skill to launch and
check swarms and agents. While enabled, the server of its session restarts it when it's gone.
```bash
hiver home                       # status: running?, session, model, Slack channel
hiver home setup [--slack] [--model M] [--force]   # create/refresh it in THIS session
hiver home start | enable | disable
hiver msg send hiver/master "…"  # talk to it from anywhere
```
Settings: `~/.hiver/config.json`; its brief: `~/.hiver/agent/CLAUDE.md` (kept on setup unless
`--force`); hiver's config: `~/.hiver/config` → `~/.config/hiver`.

## Solo agents

A solo agent is a swarm with one member that is its own master: own space, ★ teal row in the
sidebar, messages, schedules and heartbeats like a master. Create one from the folder it will
live in: `cd <folder> && hiver swarm new --provider agent.creator "<task>"` (the designer pane
stays; close it once the agent runs). Lower level: `hiver swarm launch <folder> --slug S --solo
[--model M] [--kind codex] [--description … --skills … --tools …]`. Relaunch with
`hiver swarm relaunch <slug>` (new space if its pane is gone).

**Talking across swarms/agents:** only masters (and solo agents) may message another swarm,
`hiver msg send <slug>/master "…"`. Agents check `hiver swarm directory` first, **ask the user
before sending any work**, and don't disturb entries marked busy.

An addon whose process exits (e.g. `q` in the dashboard) is reopened with
`hiver swarm addon <slug> <id>` or `hiver swarm relaunch <slug> --addons-only`. Plugin code
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
It runs in the swarm root with `HIVER_SWARM_ROOT`, `HIVER_SWARM_SLUG`, `HIVER_SWARM_CHANNEL`,
`HERDR_BIN_PATH`, `HERDR_PLUGIN_CONFIG_DIR`, `HERDR_PLUGIN_STATE_DIR`.
- Read the roster: `<root>/.swarm/agents.json`; live states: `hiver swarm list --json`.
- Outside → agents: `"$HERDR_BIN_PATH" msg send <to> "text" --swarm "$HIVER_SWARM_SLUG" --from human`.
- Agents → outside: tail `<root>/.swarm/bus.jsonl` (`"ev":"msg"` lines; skip `copy: true`).
- Keep state on disk, never exit on network errors. Full contract: `plugins/README.md`.

**2. Setup provider (designs a swarm for `hiver swarm new`):** a pane with `id = "setup"`.
It reads `HIVER_SETUP_TASK` / `HIVER_SETUP_CWD`, then: design the team (confirm with the user) →
write `<root>/<agent>/CLAUDE.md` → `hiver swarm launch <root> --slug … <agents>` from its own pane
→ `hiver swarm accept-trust --pane "$HERDR_PANE_ID" &` → `exec claude …` (it becomes the master).
Walkthrough: `docs/hiver-providers.md`; template: `plugins/team-template/`.

Workflow for a new plugin:
1. Copy the closest example (`plugins/dashboard`, `plugins/slack-relay`, `plugins/team-template`)
   to a new folder; change `id`, `name`, the pane `command`.
2. Unit-test the pure parts (`python3 -m unittest`), like the examples' `test_*.py`.
3. `hiver plugin link <dir>`, then try it on a small swarm (Haiku models, short budget):
   `hiver swarm addon <slug> <id>` or `hiver swarm new --provider <id> "<task>"`.
4. Check `hiver plugin log <id>` if the pane dies.

## Troubleshooting

- **A feature is missing in one session** (no click-to-master, no swarm summary row/hover info,
  "config.toml has unknown keys" banner): that session's server predates the binary. Compare
  `ps -o lstart= -p <pid>` with `ls -l ~/.local/bin/hiver`; find the pid with
  `lsof -U | grep sessions/<name>/herdr.sock`. Fix, after asking: a live handoff keeps agents
  running: `python3 <this skill>/scripts/handoff.py <name>` (or stop + `hiver swarm relaunch`).
- **`ctrl+b q` / `⌥Q` don't quit:** check `[keys] detach` in config.toml; it should be
  `["alt+q", "prefix+q"]` (rerun `hiver swarm setup`), then `hiver server reload-config`.
- **`⌥Q` types a symbol (œ) / hiver doesn't close:** the terminal doesn't send Option as Alt.
  Tell the user: `ctrl+b` then `q` always works, or close the terminal tab (agents keep
  running); to fix ⌥ keys set "Use Option as Meta" (Terminal.app Profiles → Keyboard; iTerm2
  Left Option = Esc+; Ghostty `macos-option-as-alt = true`).
- **The desktop pet doesn't leave:** a hiver window is still open. `ls ~/.hiver/windows` lists
  them (one file per window, named by pid; `ps -o tty=,command= -p <pid>` shows the terminal).
  The pet quits ~6s after the last one closes and comes back with the next `hiver`.
- **Agent shows blocked:** it's waiting on a dialog: `hiver pane read <pane> --source visible`.
- **Messages not delivered:** `hiver swarm list` (✉ queued), is the swarm paused?
  `hiver msg log --swarm <slug>`.
- **Update this skill:** `hiver skill install` (it ships inside the hiver binary; source
  `skills/hiver/` in the repo).
- **Install / update hiver:** first install from a clone: `./install.sh`. Update: `hiver update`
  (latest hiver, installed, running sessions handed off live; prints old → new version).
  Maintainer: `hiver update --check` (merges herdr, tests, asks before push + install).
