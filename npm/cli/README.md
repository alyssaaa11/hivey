# Hivey

**All your AI swarms. One terminal.**

Hivey is a terminal workspace and lead agent for teams of AI coding agents (Claude Code and
Codex). Give Hivey one goal: it designs a team, gives it a coordinator, opens every agent in its
own pane, and keeps their messages, status and memory in one place. You can talk to the team in
the terminal or from Slack, and come back later to see what it did.

Website and docs: **[hivey.dev](https://hivey.dev)** · Source: [github.com/alyssaaa11/hivey](https://github.com/alyssaaa11/hivey) · Apache-2.0

> This npm package ships a prebuilt binary for **Apple Silicon Macs** (M1 and later). On Linux
> or an Intel Mac, [build from source](#build-from-source-linux-intel-mac).

## Contents

- [Requirements](#requirements)
- [Install](#install)
- [Set up (once)](#set-up-once)
- [Start Hivey](#start-hivey)
- [Your first swarm](#your-first-swarm)
- [Talk to your agents](#talk-to-your-agents)
- [Find your way around](#find-your-way-around)
- [Manage swarms](#manage-swarms)
- [Scheduled checks and quiet mode](#scheduled-checks-and-quiet-mode)
- [Slack](#slack)
- [Update](#update)
- [Uninstall](#uninstall)
- [Troubleshooting](#troubleshooting)

## Requirements

- An Apple Silicon Mac
- **Node.js** 18 or later (for npm)
- **Python 3** (the setup and the bundled plugins use it)
- **Claude Code** for the Hivey lead agent and the team designer. Codex is optional: agents can
  run on either. You sign in to those tools yourself; Hivey uses your existing logins.

## Install

```bash
npm install -g @hivey/cli
hivey --version
```

This installs the `hivey` command: a small Node launcher plus the prebuilt binary, the bundled
plugins and the agent skills. Nothing is compiled on your machine.

## Set up (once)

```bash
hivey setup
```

`hivey setup` prepares your machine and lists everything it changed at the end:

1. **Coding agents.** It looks for Claude Code, Codex, Antigravity (Gemini) and Grok, offers
   to install the ones you don't have (Claude Code by default), and links each one found to
   Hivey so their panes show *working*, *idle* or *blocked*.
2. **Skills, hooks and commands.** It installs the Hivey skills into Claude Code
   (`~/.claude/skills`) and Codex (`~/.agents/skills`). They teach your agents to run Hivey for
   you (sessions, swarms, plugins).
3. **Bundled plugins.** It links the dashboard, Slack relay, GitHub publisher, team template,
   swarm creator, agent creator, skills library, swarm watcher and voice plugins.
4. **Optional extras.** It asks about Slack (to talk to your agents from any device), online
   skill search, a desktop pet (a small character that reacts to your agents and can speak
   updates), and the Hivey lead agent in `~/.hivey/agent`. It also offers two companion tools,
   both off unless you say yes: **skylls** (a library to find, save and share skills, agents
   and swarms; needs a GitHub login) and **envsave** (keeps API keys out of your code; needs
   an envsave license).

`hivey setup --no-setup` skips the questions and does only the non-interactive part. Run
`hivey setup` again any time; it only changes what's missing or out of date.

## Start Hivey

Open a terminal in the folder where the work lives and run:

```bash
cd ~/projects/my-project
hivey                          # the default session
hivey --session my-project     # or one Hivey per project
```

Hivey is a full-screen terminal workspace: a sidebar of swarms on the left, agent panes on the
right. Detach with `⌥Q` (or `ctrl+b` then `q`). Everything keeps running, and `hivey` reattaches.
Your agents keep working while the Mac stays awake and connected. Hivey is not a hosted
service, so a sleeping Mac pauses them.

## Your first swarm

Ask the Hivey lead agent in plain words ("build a team to research and prototype my idea"), or
launch one from the project folder:

```bash
hivey swarm new "Research and build my next idea"
```

A **setup provider** designs the team. The bundled one opens a coordinator that plans the swarm
with you: which agents, which models, what each one delivers, and whether it gets a Slack
channel or a memory wiki. Then it launches the swarm in its own space:

- the **coordinator** (master) in the first pane, the **agents** beside it, each in its own
  folder
- a **dashboard** with each agent's state, token use, budget, tasks and recent messages
- a **message bus** so agents can hand work to each other

```bash
hivey swarm providers          # the installed team designers
```

## Talk to your agents

```bash
hivey msg send my-swarm/coordinator "How is it going?"   # the coordinator
hivey msg send my-swarm/scout "Share your sources"       # one agent
hivey msg send @all --swarm my-swarm "Wrap up by 6pm"    # everyone
hivey msg send my-swarm/scout --fyi "FYI: API key rotated"      # never wakes it
hivey msg send my-swarm/scout --urgent "Stop, wrong repo"       # now, even mid-task
hivey msg log --swarm my-swarm                            # history
```

Normal messages are delivered when the agent is idle, never in the middle of a task. Several
waiting messages arrive together as one prompt. If an agent is stuck on a question, its
messages wait and the coordinator is told once.

## Find your way around

| Keys | What |
| --- | --- |
| `⌥S` | Pick a swarm and jump to its coordinator |
| `⌥M` | Jump to this swarm's coordinator |
| `⌥A` | Send a message (an agent, `@all`, a role; normal, FYI or urgent) |
| `⌥I` | Swarm info: agents, Slack channel, memory wiki, plugins, budget, tasks |
| `⌥L` | This swarm's message log |
| `⌥F` | Zoom the focused pane, and back |
| `⌥P` | Choose your desktop pet, or none |
| `⌥Q` | Detach (everything keeps running) |

Your terminal must send Option as Alt: iTerm2 → Profiles → Keys → Left Option = Esc+;
Terminal.app → Profiles → Keyboard → "Use Option as Meta key"; Ghostty:
`macos-option-as-alt = true`.

With the mouse: **click a swarm** in the sidebar to open it and focus its coordinator,
**hover** it for its agents and status, **double-click a pane's title** to make it full screen,
and **right-click an agent** to open a second session of it beside the first.

## Manage swarms

```bash
hivey swarm list                      # swarms, agents, states, waiting messages
hivey swarm info my-swarm             # what it uses: agents, Slack, wiki, plugins, budget
hivey swarm pause my-swarm            # hold deliveries; messages still queue
hivey swarm resume my-swarm
hivey swarm relaunch my-swarm         # restart stopped agents, continuing their conversations
hivey swarm relaunch my-swarm scout   # one agent (--fresh starts a new conversation)
hivey swarm addon my-swarm hivey.dashboard   # reopen the dashboard
```

After restarting the Mac, run `hivey`, then `hivey swarm relaunch <swarm>` for any agent that
came back as a plain shell.

## Scheduled checks and quiet mode

Wake the coordinator (or one agent) on a timer. The check is delivered only when that agent is
idle, so it never interrupts work, and each one costs one turn of that agent's model.

```bash
hivey swarm schedule add my-swarm --every 2h "run the test suite and file failures"
hivey swarm schedule add my-swarm --at 09:00 "summarize yesterday's progress"
hivey swarm schedule list
```

Let swarms work through the night without waking you:

```bash
hivey quiet on                     # no sounds or speech; Slack posts held for a catch-up
hivey quiet schedule 22:30-6:45    # every day
hivey quiet off
```

## Slack

```bash
hivey slack connect          # once: walks you through creating the Slack app
hivey home setup --slack     # #hivey: talk to the Hivey lead agent from anywhere
```

Each swarm can get its own channel. In it, `@scout …` reaches that agent, `@all …` everyone,
and a message without a mention goes to the coordinator.

## Update

```bash
hivey update           # runs npm install -g @hivey/cli@latest
hivey update --check   # only compare your version with the latest on npm
```

Running sessions keep the old version until they restart (`hivey server stop`, then `hivey`).

## Uninstall

```bash
hivey server stop
npm uninstall -g @hivey/cli
```

Your settings stay in `~/.config/hivey` and `~/.hivey`, and the skills in `~/.claude/skills`
and `~/.agents/skills`. Delete those folders too for a clean removal.

## Build from source (Linux, Intel Mac)

To use Hivey on Linux or an Intel Mac, or to change Hivey itself, install from a checkout. You
need Git, Rust, Zig 0.16.0, Python 3 and Node.js:

```bash
git clone https://github.com/alyssaaa11/hivey.git ~/hivey
cd ~/hivey && ./install.sh
```

`hivey update` then pulls and rebuilds that checkout.

## Troubleshooting

| Symptom | Fix |
| --- | --- |
| `EBADPLATFORM` when installing | This package is for Apple Silicon Macs only. Build from source instead. |
| `hivey setup` says python3 not found | Install Python 3 (`xcode-select --install` or python.org), then run it again. |
| `⌥` keys type symbols (œ, ƒ) | Set your terminal to send Option as Alt (see [Find your way around](#find-your-way-around)). Meanwhile, `ctrl+b` then `q` detaches. |
| A message never arrives | `hivey swarm list`: the agent may be *working* (it gets the message when idle), *blocked* (the coordinator was told), or the swarm is paused. |
| An agent shows *gone* | `hivey swarm relaunch <swarm>` |
| The dashboard closed | `hivey swarm addon <swarm> hivey.dashboard` |

More: the [docs on hivey.dev](https://hivey.dev/docs/) and the
[full guide](https://github.com/alyssaaa11/hivey/blob/main/docs/hivey-guide.md).

## License

Apache-2.0. Hivey is built on the [herdr](https://github.com/herdrdev/herdr) terminal runtime.
