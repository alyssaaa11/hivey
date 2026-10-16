# What hivey installs on a computer

Everything `./install.sh` puts on a computer the first time, and what `hivey update` keeps
current. On any machine, `hivey installed` prints the same list from the real files.

Rule: everything hivey installs for Claude Code and Codex is named `hivey` or `hivey-<name>`
(skills, commands, hooks, agents) and every plugin is `hivey.<name>`, flat in the usual
folders (Claude Code finds skills only one level deep, so nothing is nested). The user's own
files are never touched. `hivey migrate` (run by `install.sh` and `hivey update`) moves copies
an older version left behind to `~/.hivey/old/`, never deleting them.

Status: **now** = in hivey today; **next** = the port of the swarm and agent creators, not
shipped yet.

## Program

| What | Where |
|---|---|
| `hivey` | `~/.local/bin/hivey` (`HIVEY_BIN_DIR` to change) |

`hivey --agent [claude args…]` starts Claude Code in the hivey agent's folder (`~/.hivey/agent`).

## Coding agents (offered, then linked to hivey)

`install.sh` checks each one. A missing one is offered (Claude Code by default yes, the others
no); each one found gets its hivey integration (`hivey integration install <target>`), a hook
that tells hivey whether its panes are working, idle or blocked.

| Agent | Command | Installed with | hivey integration |
|---|---|---|---|
| Claude Code | `claude` | `bun`/`npm install -g @anthropic-ai/claude-code`, else `curl -fsSL https://claude.ai/install.sh \| bash` | `~/.claude/hooks/herdr-agent-state.sh` |
| Codex | `codex` | `bun`/`npm install -g @openai/codex` | `~/.codex/herdr-agent-state.sh` |
| Antigravity (Gemini) | `agy` | `curl -fsSL https://antigravity.google/cli/install.sh \| bash` | `~/.gemini/config/hooks/herdr-agent-state.sh` |
| Grok | `grok` | `curl -fsSL https://x.ai/cli/install.sh \| bash` | `~/.grok/hooks/herdr-agent-state.sh` |

These come from herdr's agent integrations and keep herdr's name (`herdr-agent-state.*`), so
they stay in step with herdr's updates.

## Skills

In `~/.claude/skills/`, and `~/.codex/skills/` when Codex is set up (`hivey skill install`).

| Skill | What it does | Status |
|---|---|---|
| `hivey` | operate hivey: sessions, swarms, plugins, messages, troubleshooting; scripts `new_wiki.py`, `handoff.py` | now |
| `hivey-create-wiki` | create the `<name>-wiki` Obsidian vault of an agent or a swarm | now |
| `hivey-context7` | current docs for any library, API or CLI in seconds (Context7 web API; every agent's first stop for docs) | now |
| `hivey-codex` | run a task with Codex headless (`codex exec`): models, sandbox, review, resume | now |
| `hivey-agy` | run a task with Antigravity / Gemini (`agy`) headless; runner `scripts/agy_run.py` | now |
| `hivey-grok` | run a task with Grok (`grok`) headless; runner `scripts/grok_run.py` | now |
| `hivey-swarm-creator` | design a swarm with the user (each agent on Claude Code, Codex, Grok or agy), write the briefs, make the wiki, launch it; scripts: task board, house rules, agent setup | now |
| `hivey-agent-creator` | design one long-lived agent with the user (any of the four CLIs), write its brief, make its wiki, launch it; scripts: agent setup, done gate, resume context | now |

Also installed globally: `find-skills` (skills.sh), which creators use to find skills an
agent needs.

## Commands (Claude Code)

In `~/.claude/commands/`.

| Command | What it does | Status |
|---|---|---|
| `/hivey-wiki-ingest` | add a source to the wiki of the agent or swarm you are in | now |
| `/hivey-wiki-query` | answer a question from that wiki | now |
| `/hivey-wiki-lint` | health check of that wiki | now |
| `/hivey-wiki-log` | its latest log entries | now |
| `/hivey-wiki-list` | that wiki, or every wiki with `--all` | now |

`/hivey-create-wiki` is the skill above.

## Hooks

Registered in `~/.claude/settings.json` (Codex: `~/.codex/hooks.json`).

| Hook | What it does | Status |
|---|---|---|
| `~/.claude/hooks/herdr-agent-state.sh`, `~/.codex/herdr-agent-state.sh` | tell hivey whether each agent is working, idle or blocked (from `hivey integration install` or the first-run prompt) | now (herdr's name, kept on purpose) |
| `~/.claude/hooks/hivey-agent-stop.py` (Stop) | speak an agent's `DONE:` line with `hivey voice` (workers stay silent) | now |
| `~/.claude/hooks/hivey-safety.py` (Bash, Edit, Write, Read) | hivey's own safety check: blocks destructive commands, history rewrites, `curl \| sh`, secret access and hand-edited lockfiles; on by default, settings → safety / `hivey safety on\|off` | now |
| `~/.claude/hooks/hivey-secret-scan.py` (Bash) | block a `git commit` that contains secrets; same safety switch | now |

Installing or updating replaces older hooks they cover (a damage-control folder,
`secret_scan.py`, `agents_stop.py`): unregistered, files moved to `~/.hivey/old/`. The
user's other hooks stay as they are. Source: `hooks/` in the repo.

## Plugins

Linked from the hivey checkout's `plugins/` (`hivey plugin list`).

| Plugin | What it does | Status |
|---|---|---|
| `hivey.slack-relay` | Slack ⇄ the agents' message bus | now |
| `hivey.dashboard` | agents, states, tokens, tasks, messages | now |
| `hivey.github` | product and swarm repos, asks before creating anything | now |
| `hivey.team-template` | example creator: a builder and a critic | now |
| `hivey.skills` | how creators pick and install each agent's skills | now |
| `hivey.swarm-creator` | the swarm creator: runs the `hivey-swarm-creator` skill | now |
| `hivey.agent-creator` | the agent creator: runs the `hivey-agent-creator` skill | now |
| `hivey.swarm-watcher` | tells the master when a worker is blocked, stopped, stalled or gone and when the time budget runs out; Gantt dashboard page | now |
| `hivey.tts` (macOS) | optional local voices: its pane runs `hivey voice install`, which installs the `tts` tool ([hivey-tts](https://github.com/jcsancho/hivey-tts), Kokoro voices, ~3 GB of models) only when you open it, e.g. by picking a Kokoro voice in settings → voice | now |

Plugins hivey replaced (`swarm.creator`, `agent.creator`, `swarm.watcher`, from the old /swarm and
/agent-creator skills) are unlinked by `hivey migrate`; a creator chosen in `swarm-setup.json` is
switched to hivey's. `hivey update` also links bundled plugins that are new.

## MCP servers

None. hivey installs no MCP server, globally or per agent, and needs none: docs come from the
`hivey-context7` skill (a plain web API every agent can call). An agent creator may give one
agent its own project MCP servers in `<agent folder>/.mcp.json`, only those its task needs and
only with the user's OK. MCP servers the user set up (Claude Code plugins, claude.ai
connectors, their own) are not hivey's and are never touched.

## Data and settings

| Folder | What's in it |
|---|---|
| `~/.config/hivey/` | `config.toml` (sidebar, keys), plugins, sessions, logs |
| `~/.hivey/` | the hivey agent (`agent/CLAUDE.md`, its wiki `agent/hivey-wiki/`), settings, Slack token, voice, wiki look (`wiki.json`), `old/` |
| `~/.local/state/hivey/` | which version is installed |
| `<agent or swarm folder>/<name>-wiki/` | each agent's or swarm's wiki, made when it is created |

## Machine-specific additions (never touched by updates)

| File | Read by | For |
|---|---|---|
| `EXTRA_AGENTS.md` in the hivey repo (git-ignored, created empty by `install.sh`) | every agent working on hivey, via `AGENTS.md` / `CLAUDE.md` | this machine's own instructions |
| `EXTRA_SKILL.md` in a hivey skill's folder | that skill, via its `SKILL.md` | this machine's own additions to the skill |

hivey updates rewrite `AGENTS.md`, the skills and the commands; put local changes in these
files so they survive and never block `hivey update`.

The hivey agent's brief (`~/.hivey/agent/CLAUDE.md`) is written once and then belongs to the
user: updates only add sections hivey introduced later (marked `<!-- hivey section: … -->`)
that it doesn't have yet, before `## Notes`, and never change the rest.

## Never installed

XFlow, the old `agents_*` XFlow hooks, `agents_soul_loader`, archify.
