<p align="center">
  <img src="assets/logo.svg" alt="Hivey H logo" width="100" />
</p>

# Hivey

## All your AI swarms. One terminal.

**Give Hivey the goal. Let it get the team together.**

Hivey is a terminal workspace and coordinating agent for swarms of AI agents. Talk to Hivey to create and manage your swarms. Each swarm has its own workspace and a dedicated coordinator that directs its agents, keeps the work moving, and reports back to you.

Run teams with **Claude Code and Codex**, exchange findings across swarms, and stay in touch through **Slack**. Keep reusable knowledge in **Obsidian**, extend the workspace with plugins, and share useful setups through **Skylls**.

[Website](https://hivey.dev) · [Install](#install) · [User guide](docs/hivey-guide.md) · [Watch the introduction](#meet-hivey)

### Meet Hivey

https://github.com/user-attachments/assets/2091568c-07b5-41f2-810c-9c0834372812

*Meet the little H behind your team. This is a stylized character introduction, not a recording of the terminal.*

## How it works

1. **Tell Hivey what you want to do.** Hivey creates a swarm or works with one you already have.
2. **Let the coordinator lead.** Each swarm's coordinator assigns work to its agents in a separate terminal space.
3. **Keep teams connected.** Agents message teammates; Hivey and coordinators carry findings and requests across swarms.
4. **Stay close or step away.** Talk in the terminal or Slack. Detach and return later while agents keep running on your awake, connected machine.

You do not need to prompt every swarm yourself. When you want more detail, talk directly to a swarm's coordinator.

## Your tools, working together

| Tool or capability | What it adds |
| --- | --- |
| Claude Code + Codex | Agent support for teams with different roles. |
| Swarm messaging | Address agents, roles, or a whole swarm, with queued delivery, inboxes, and message history. Coordinators handle communication across swarms. |
| Slack | Once connected, talk to Hivey in its channel and to each swarm's coordinator in that swarm's channel. |
| Scheduled check-ins | Wake an idle coordinator at configured times. |
| Obsidian memory | Optionally give a swarm a local Markdown wiki for sources, findings, decisions, and reusable knowledge. Open its vault in Obsidian. |
| Skills + plugins | Extend swarm creation, agent setup, dashboards, integrations, and repeatable workflows. |
| EnvSave | Configure integrations such as the Slack relay to retrieve credentials with an external `envsave` command. |
| Skylls | With the separate Skylls tool installed and configured, share and import skills, agents, and whole swarm setups. |
| Hivey pet + voice | An optional macOS companion that reacts to agent activity and can speak updates aloud. |
| herdr runtime | The terminal foundation for panes, sessions, and agent workspaces. |

### Memory that outlives a chat

Swarms can maintain a wiki you can open in Obsidian: original sources, linked knowledge, decisions, and lessons that inform the next task. The approach is inspired by [Karpathy's LLM Wiki as agent memory](https://aaif.io/blog/karpathys-llm-wiki-as-agent-memory). Memory lives in files you and your agents can inspect and maintain.

### Share a team with Skylls

Hivey integrates with **[Skylls](https://skylls.dev/)**, a separate website and tool for sharing skills, agents, and complete swarm setups. Share a setup with friends, let their agents reuse it, or import a team someone has shared with you and adapt it to your own work. Recipients configure their own credentials and agent access.

### On the roadmap

**Native per-swarm whiteboards** are planned: a shared view for tasks, notes, and decisions. They are not included as a shipped feature here.

## Install

Hivey installs from source on **macOS or Linux**.

Before starting, install **Git, Rust/Cargo, Zig 0.16.0, and Python 3**. Install **Node.js/npx** for the installer’s skill-library setup. The Hivey lead-agent setup uses Claude Code; configure your chosen agents and their access separately. See [`install.sh`](install.sh) for the current requirements and setup options.

```bash
git clone https://github.com/alyssaaa11/hivey.git ~/hivey
cd ~/hivey
./install.sh
```

The installer builds Hivey, links bundled plugins, installs its skill, and walks you through the lead agent and optional integrations. To skip the interactive setup, use `./install.sh --no-setup`.

The default install location is `~/.local/bin/hivey`. If your shell cannot find `hivey`, add `~/.local/bin` to your PATH as the installer explains.

Start Hivey where your work lives:

```bash
hivey
```

Talk to Hivey to get a team started, or create a swarm from the command line:

```bash
hivey swarm new "Research and build my next idea"
```

`⌥Q` (or `ctrl+b`, then `q`) detaches. Run `hivey` to reattach. Detaching does not stop the agents, but your machine must stay awake and connected; Hivey is not a hosted service that continues after your computer sleeps.

Continue with the **[user guide](docs/hivey-guide.md)** for setup and everyday use.

## Explore the project

- [User guide](docs/hivey-guide.md)
- [Architecture](docs/diagrams/hivey-architecture.html)
- [New swarm flow](docs/diagrams/hivey-new-swarm.html)
- [Write a provider](docs/hivey-providers.md)
- [Agent skill](skills/hivey/SKILL.md) — install with `hivey skill install`
- [Design](docs/hivey-design.md)

## Development

```bash
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
python3 scripts/third_party_licenses.py   # regenerate THIRD_PARTY_LICENSES.md after dependency changes
```

If you are an AI agent working in this repository, read [`HIVEY.md`](HIVEY.md) and [`AGENTS.md`](AGENTS.md) first. Hivey's fork-specific guidance is in `HIVEY.md`.

## Credits and license

Hivey is based on [herdr](https://github.com/herdrdev/herdr), Copyright the herdr authors, licensed under the Apache License 2.0. Hivey is not affiliated with or endorsed by the herdr project; “herdr” describes where this code comes from. See [`NOTICE`](NOTICE).

Hivey is licensed under the [Apache License 2.0](LICENSE). Third-party components and their licenses are listed in [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).
