---
name: hiver
description: Operate hiver, the terminal workspace for swarms of AI agents (a herdr fork). Use when the user says hiver (or "hyver"/"hybrid" meaning hiver) and wants to start, attach, detach, close or stop hiver sessions; create, launch, inspect, pause, relaunch or message swarms; schedule master check-ins; list, install, link, enable or configure plugins/addons (Slack relay, dashboard, GitHub, watcher); or write a new hiver plugin (an addon pane or a swarm setup provider). Also for hiver troubleshooting (keys, click-to-master, missing features after an upgrade).
---

# hiver

hiver runs swarms of Claude Code / Codex agents: **one space per swarm, pane 1 = the master
(coordinator)**, a message bus between agents (`hiver msg`), addons (plugins) in the swarm's space,
and a "swarms" sidebar. It's a fork of herdr; every `herdr` command works as `hiver`.

- Binary: `hiver` (`~/.local/bin/hiver`). Repo: `~/projects/swarmAgents/hiver`
  (guide `docs/hiver-guide.md`, plugins `plugins/`, provider guide `docs/hiver-providers.md`).
- Config: `~/.config/hiver/config.toml`. Plugin configs: `hiver plugin config-dir <id>`.
- The original `herdr` (separate binary and server) may also be running: never touch it.

## Rules

- **Stopping a session stops its agents.** Detaching doesn't. Ask before `session stop`,
  `server stop`, closing panes, or `swarm forget` on a swarm that is working.
- Prefer detach + attach. A stopped swarm comes back with `hiver swarm relaunch <slug>`
  (agents continue their conversations).
- Confirm before anything outward-facing (Slack posts, GitHub repos); the GitHub addon already asks.
- Never run `hiver integration install` (it rewrites shared agent hooks) or `hiver update`
  (disabled in the fork; rebuild from the repo instead).
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
hiver swarm import <root> | forget <slug>     # register / unregister (files kept)
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

Included (in `~/projects/swarmAgents/hiver/plugins/`):

| id | what |
|---|---|
| `hiver.slack-relay` | Slack ⇄ bus bridge (config `token_command`, channel per swarm) |
| `hiver.dashboard` | agents, states, tokens per agent, active time vs budget, tasks, messages |
| `hiver.github` | product + swarm repos, asks before creating anything |
| `hiver.team-template` | example setup provider (builder + critic); copy it |
| `swarm.watcher` | /swarm skill addon: stalled-agent and budget alerts to the master |
| `swarm.skill` | /swarm skill as a setup provider |

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
- **Agent shows blocked:** it's waiting on a dialog: `hiver pane read <pane> --source visible`.
- **Messages not delivered:** `hiver swarm list` (✉ queued), is the swarm paused?
  `hiver msg log --swarm <slug>`.
- **Update this skill:** `hiver skill install` (it ships inside the hiver binary; source
  `skills/hiver/` in the repo).
- **After rebuilding hiver:** `cargo build --release` in the repo, copy to `~/.local/bin/hiver`;
  running sessions keep the old code until handoff/restart.
