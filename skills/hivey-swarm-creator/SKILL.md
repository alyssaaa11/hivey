---
name: hivey-swarm-creator
description: Turn a task into a coordinated swarm of coding agents (Claude Code, Codex, Grok, Antigravity/Gemini) in hivey. This session becomes the coordinator (the swarm's master) - it designs the smallest team with the user, always adds a critic, writes a detailed CLAUDE.md brief per agent with deliverables and runnable checks, sets up the task board, git worktrees, the swarm's wiki, skills and Slack, launches it with `hivey swarm launch`, and then coordinates it. Use whenever the user says "swarm", "/swarm", "/hivey-swarm-creator", "spin up a team of agents", "launch multiple agents for this", "split this across agents", "create agents to build/research X in parallel", or otherwise wants several agents working together on one project, even if they don't say the word swarm. Also use to pause, resume, restart or continue an existing swarm ("pause the swarm", "stop the agents and save progress", "resume the swarm").
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey swarm creator

You are the **coordinator**. You don't become one of the workers: you design the team with the
user, set it up, launch it, and then stay on as its master (pane 1 of the swarm's space). The
user wants a team that runs with little supervision, so most of the value is in the design and
the per-agent CLAUDE.md briefs.

You run either as the setup pane of `hivey swarm new "<task>"` (`HIVEY_SETUP_TASK` holds the
task, `HIVEY_SETUP_CWD` the folder it was started from) or because the user typed
`/hivey-swarm-creator` in a hivey pane. The `hivey` skill knows every hivey command;
`hivey swarm <command> --help` shows usage without running anything.

Scripts live in `~/.claude/skills/hivey-swarm-creator/scripts/` (or
`~/.codex/skills/hivey-swarm-creator/scripts/`), called `$S` below. Standard library only.

**House rules** (`references/shared-rules.md`) apply to you and every agent: end every final
response with `DONE: <3 words max>` on its own line (hivey's Stop hook speaks it for masters
and solo agents; workers stay silent), look up docs with `hivey-context7` first, respect the
safety hooks, keep secrets out of files, style Obsidian notes with callouts.

## Preconditions

- `test "$HIVEY_ENV" = 1`: you must be inside hivey. If not, say so and stop.
- **Folder** = `HIVEY_SETUP_CWD`, else the current directory, unless the user named another.
  The **swarm root** is `<Folder>/<name>` (step 1). Agent homes go in `<root>/<agent>/`; the
  shared work product goes in `<root>/` or a subfolder (e.g. `<root>/app/`). Every agent gets
  access to the whole root.
- **Resuming?** If `<root>/.swarm/agents.json` already exists (or the current folder is a swarm
  root), this swarm has run before. Follow **Resume a swarm** instead of designing a new one,
  unless the user asks for a new or changed team.

## 1. Name (ask first)

Ask the user for the swarm's **name** (suggest one from the task; `[a-z][a-z0-9-]{0,18}`),
unless the task already says "Name: …". That one name is used everywhere: the slug
(`--slug`), the Slack channel `#<name>`, the root `<Folder>/<name>` and the wiki
`<root>/<name>-wiki`. No `swarm-` prefix anywhere. Below, `<name>` and `<slug>` are the same.
In the same question suggest an **emoji** for it (e.g. 📈 for trading, 📡 for a news radar)
unless the task already says "Icon: …"; the sidebar shows the space as `<emoji> <name>`
(`--icon`, step 7). The user can pick another or none.

## 2. Design the team

Work out the **smallest team** that covers the task, usually 2–5 workers. Each one costs
attention and coordination, so give each a **distinct deliverable**. If one agent needs
another's output, make that hand-off explicit instead of having two agents share a job.

**Every swarm includes a `critic`**, on top of the workers. It produces nothing itself: it
reviews each deliverable posted as DONE, sends concrete critique to the owner, and approves the
work once it's good, so the team improves its drafts instead of shipping them. Tailor its
criteria to the project (code: correctness, tests, security; research: sources, gaps, logic;
copy: clarity, audience fit) and put them in its CLAUDE.md.

For each agent decide:
- **key**: short kebab-case role matching `[a-z][a-z0-9_-]{0,31}` (`researcher`, `builder`,
  `designer`). It's the folder name and the address for `hivey msg send <key>`.
- **mission and deliverables**: exact output paths and an objective **"done when"** for each.
- **dependencies**: who it needs input from and who consumes its output.
- **coding agent** (`--kinds`): which CLI runs it. Claude Code is the default. Suggest per
  role: **Claude Code** for most work and the critic; **Codex** for code review or a second
  opinion; **Grok** for X/Twitter and web research; **agy** (Antigravity, Gemini) for fast,
  cheap tasks and image generation. All four read the CLAUDE.md you write: launch links
  `AGENTS.md` (and `GEMINI.md` for agy) to it.
- **model** (`--models`): for Claude Code, `opus` for builders, the critic and deep reasoning;
  `sonnet` for research, writing and data gathering, at a fraction of the cost. A swarm costs
  many times what one agent does, so spend the strong model where it pays off. For Codex, Grok
  and agy leave it to the CLI's default unless the user names one.
- **skills**: run `hivey skills guide` and follow it for every agent (skylls first, skills.sh
  only with the user's OK). Skills go in the plan per agent and land in each agent's own
  `.claude/skills/`, never globally. Capabilities no skill covers are **skill gaps**: list them
  as suggestions (`/skill-creator`), don't build them now.

Then break the work into a **task list** for the task board. Each task has an ID, title, owner,
dependencies, deliverable path and, where possible, a **`check` command**: a shell command that
exits 0 only when the deliverable is really done (`npm test`, `pytest -q`,
`test -s report.md && grep -q '## Sources' report.md`, a link checker, a schema validator).
Runnable checks catch "claimed done but isn't", which prose criteria miss. Most failed
multi-agent runs come from vague specs, agents out of sync or missing verification; the board,
contracts and checks target exactly those.

**Contracts first** when agents build things that must fit together (code, data, APIs, file
formats): task T1 is "Write `<root>/shared/CONTRACTS.md`" (interfaces, data shapes, formats,
naming, layout), with a check, and every build task depends on it, so no builder starts before
the critic approves the contract.

**Code swarms: one git worktree per builder.** When two or more agents write code in the same
repo, each gets its own worktree and branch (step 6) so they never overwrite each other. Name an
**integrator** (the critic, or a dedicated agent for bigger builds) who merges the branches into
the main checkout and runs the full test suite on the combined result, as its own task with a
check.

**Mobile apps (iPhone, iOS, Android): build them with Expo**: Expo SDK (latest stable) + Expo
Router + TypeScript. Don't plan native Swift/Kotlin projects, even if a spec says "native". The
app runs through Expo tooling (`npx expo start`, `npx expo run:ios`, a dev-client build via
`expo prebuild`). Shared logic goes in plain TS packages; anything that must be native
(widgets, Live Activities, extensions) goes in as Expo modules or targets through config
plugins, not a hand-maintained Xcode project. Give the mobile builder Expo skills (the
`expo:*` plugin skills if installed, or via `hivey skills guide`) and make its check an Expo
command (`npx tsc --noEmit`, the tests, a prebuild + simulator build). Anything that needs the
user's EAS or App Store account stays with the user.

**Budget**: agree a time budget with the user (e.g. 120 min). The watcher warns you at 80% and
100%.

## 3. Confirm with the user

Show the plan and wait for approval before creating anything:

```
Swarm: <emoji> <name>   root: <Folder>/<name>   channel: #<name>   budget: <N> min
| Agent | CLI | Model | Role / mission | Deliverables (path) | Skills | Depends on |
Tasks:
| ID | Task | Owner | Deps | Check |
Skill gaps: <gap> → suggestion …
Code: <repo path>, worktrees for <builders>, integrator: <agent>   (code swarms only)
(critic always included)
Wiki: <root>/<name>-wiki   (the swarm's memory; say no to skip it)
Slack: #<name> (connected) | not connected: run `hivey slack connect` first, or go without
Schedules: monitoring pass every <N> min · <timed task> … (or none)
Addons: hivey.dashboard, hivey.slack-relay, hivey.swarm-watcher
Publish: product repo <name>, swarm repo <name>-swarm, private (only if it builds something)
```

In the same message:
- **Confirm each agent's CLI and model.** Show them per agent with a one-line reason
  ("builder: Claude Code opus, writes the core code"; "researcher: Grok, X and web research";
  "reviewer: Codex, second opinion on the code") and ask whether to keep or change them. The
  user pays for every agent, so this is their call, not a default to apply silently.
- **Ask whether you should run scheduled checks**, and how:
  - a **monitoring pass** every N minutes (who is idle, blocked or stuck; answer questions;
    review finished work). Suggest 15 min, shorter for fast swarms, longer for long research.
  - **timed tasks** with their own instructions ("every day at 09:00 summarize progress to the
    user", "every 2 h run the test suite and file failures").
  - or **none** (you only react to messages and alerts).
  Each wake-up costs a turn of your model.
- **Slack**: run `hivey slack status` before showing the plan. Connected: `hivey swarm launch`
  creates (or joins) `#<name>`, invites the user and starts the relay, nothing else to do. Not
  connected: ask the user to run `hivey slack connect` in a terminal (never ask for the token in
  chat), or go on without Slack if they prefer.
- **GitHub**: if the swarm builds something, ask whether to publish (product and swarm are
  separate repos; see step 7).

The wiki always lives **inside the swarm folder**, at `<root>/<name>-wiki`, so the swarm, its
agents and its knowledge can be moved or shared as one folder. Never ask where the user's
Obsidian is. In anything an agent reads, write it relative to the agent's folder:
`../<name>-wiki`. Adjust the plan until the user says yes.

## 4. Scaffold the swarm

```bash
mkdir -p <root>/shared
python3 $S/shared_rules.py <root>          # house rules into <root>/CLAUDE.md (marked block)
python3 $S/setup_agent.py <root> <agent> [--worktree <root>/app]   # once per agent, critic too
```

`setup_agent.py` creates `<root>/<agent>/` and its `.claude/settings.json`, which grants access
to the swarm root (`permissions.additionalDirectories`, plus `--vault` if the vault lives
elsewhere) and sets `AGENTS_TTS=0` so workers stay silent. **Code swarms:** make sure the repo
(e.g. `<root>/app`) exists with at least one commit, then pass `--worktree <repo>` for each
builder: it gets `<root>/<agent>/repo` on branch `swarm/<agent>`. Builders commit on their
branch; the integrator merges into the main checkout and runs the checks there.

Then install each agent's skills as `hivey skills guide` says (step 4 of the guide), and check
`ls <root>/<agent>/.claude/skills`.

**Task board.** Load the approved task list. Always pass `--root <root>`, since your working
directory may not be under the root:

```bash
T="python3 $S/swarm_tasks.py --root <root>"
$T add T1 "Write shared/CONTRACTS.md" --owner builder --path shared/CONTRACTS.md \
   --check "test -s shared/CONTRACTS.md"
$T add T2 "Build the API" --owner builder --deps T1 --path app/ --check "cd app && npm test"
```

This writes `.swarm/tasks.json` (the `hivey.dashboard` reads it), a readable `.swarm/TASKS.md`,
and `.swarm/events.jsonl`, the swarm's timeline. Rules the board enforces: agents **claim**
before starting, only their own (or unowned) tasks, and only once every dependency is
`approved`. Only `critic` or `coordinator` can set `approved`, and only when the task's check
passes (checks run outside the board lock, so a slow suite doesn't block anyone). Statuses:
`open → in-progress → review → approved` (`blocked` any time).

## 5. Write the briefs

Read `references/agent-claude-md.md` now: it has the template and role guidance. The briefs are
the heart of the swarm. Each agent works only from its own file, so it must understand the
whole team, not just its own job.

1. **Write "The swarm" section once**: setup (root, the swarm's space, you as master, Slack
   mirror, wiki), the **full roster** (every agent including you and the critic: key, CLI, role,
   what it owns, what to ask it for), a "who to talk to" list, the task board, the hand-off
   chain, critique loop, messaging, contracts and code, safety, secrets and docs. Save it as
   `<root>/.swarm/README.md`.
2. **Write `<root>/<agent>/CLAUDE.md` for each agent**: its own sections (mission, scope,
   deliverables with paths and checks, skills, method, definition of done, Status) plus that
   swarm section pasted in **verbatim**, so every agent sees the same roster and rules.

Make each brief detailed and specific: quote the user's original request, use exact paths, and
name the real agents in the commands. A vague brief leads to an agent that idles, duplicates a
teammate or asks the wrong agent.

## 6. Create the wiki (memory)

Unless the user said no, create the swarm's Obsidian wiki with the **`hivey-create-wiki`**
skill (Skill tool, args `<name> --in <root> --agent <root>/<agent> [--agent …]`, every agent
including the critic) now that the briefs exist. It reads the briefs, tailors the vault and
patches every agent's CLAUDE.md. Answer any question it has from the plan instead of asking the
user again. If the vault already exists (a resumed swarm), reuse it. Shape it for a swarm:
- `raw/reports/<agent>/` for each worker and the critic: agents write their reports there, and
  only there.
- `wiki/` sub-folders: `agents/`, `findings/`, `decisions/`, `deliverables/`, `lessons/`.
- In the vault's CLAUDE.md add the swarm rule: *only the coordinator writes `wiki/`; it ingests
  agent reports from `raw/reports/` without waiting to discuss them, because the swarm runs
  unattended.* Agents writing `index.md`/`log.md` in parallel would clobber each other.

Then initialize the memory: `wiki/swarm-overview.md` (the user's request, goal, roster, Slack
channel, root, hand-off chain), `wiki/agents/<agent>.md` per agent (role, owns, deliverables,
report folder, status "not started"), `wiki/decisions/plan.md` (the approved plan and the
user's tweaks), linked from `index.md`, with a `bootstrap` entry in `log.md`.

## 7. Launch

```bash
hivey swarm launch <root> --slug <name> --icon <emoji> <agent>... critic \
    --models builder=opus,critic=opus,writer=sonnet --kinds reviewer=codex,scout=grok \
    --budget-min 120 --heartbeat 15m \
    --heartbeat-task "Monitoring pass: follow the hivey-swarm-creator skill's monitoring pass" \
    --addon hivey.dashboard --addon hivey.swarm-watcher \
    --kickoff "Read your CLAUDE.md, then send START to coordinator as it says."
```

- This pane becomes the master `<name>-coordinator` and moves into a new space `<name>`; the
  agents open beside it, each as `<name>-<agent>` in its own folder, run unattended.
- Slack: when connected, launch creates (or joins) `#<name>`, invites the user and starts
  `hivey.slack-relay`. Post a short kickoff there (goal, roster); `hivey swarm info <name>`
  shows the channel.
- Addons: `hivey.dashboard` (states, tokens per agent, active time vs budget, tasks, messages),
  `hivey.slack-relay` (Slack ⇄ bus) and `hivey.swarm-watcher` (stalled, blocked, idle and budget
  alerts to you, plus a live Gantt dashboard in Chrome; see `references/dashboard.md`). Missing
  addons are reported and skipped; check `hivey plugin list`. Reopen one later with
  `hivey swarm addon <name> <id>`.
- Leave out `--heartbeat` if the user chose no monitoring pass. Add timed tasks with
  `hivey swarm schedule add <name> --at 09:00 [--id ID] "<instructions>"` or `--every 2h`
  (`--to <agent>` wakes an agent instead of you); `hivey swarm schedule list|remove|run <name>
  [<id>]`. Each wake-up arrives as a `[hivey]` message with the task and a status snapshot, only
  when you're idle; one still waiting is never duplicated, and paused swarms are skipped.
- **Directory**: register what the swarm does so other swarms and agents can judge whether to ask
  it for help: `hivey swarm profile <name> --description "<one line>" --skills a,b --tools "gh, …"`.
  Only you (the master) may message another swarm (`hivey msg send <slug>/master "…"`), and only
  after the user says yes; don't disturb one marked busy.
- **GitHub**: if the user wants the product published, keep it in its own folder (default
  `<root>/app`, or `"product_dir"` in `.swarm/agents.json`) with its own git repo, and run
  `hivey swarm addon <name> hivey.github`: it shows the plan, the user confirms in that pane,
  and secrets are scanned before each first push. Never create or push repos without that.

Tell the user what's running: the space, agents, CLIs, Slack channel, wiki path, dashboards,
schedules and skill gaps.

### Save and share (skylls)

If `skylls --version` works, offer to save the swarm once it's running, so the user can reuse
and share it. Only with their yes: `skylls swarms push <root> -m "<what it does>"`, plus
`skylls push <skill folder>` for any new skill you wrote. Push scans for secrets and personal
data and refuses if it finds any: remove them, never pass `--skip-scan`. Share
(`skylls swarms share <name> @user`) only when the user asks. Before designing a new swarm,
`skylls --json swarms find <words> --limit 5` may show one that already fits.

## 8. Coordinate

**The wiki is your source of truth too.** Read `wiki/index.md`, `swarm-overview.md`, the agent
pages and recent `findings/`/`decisions/` whenever you start or resume coordinating, and before
any decision that depends on what the swarm knows. If an agent's claim or your memory conflicts
with the wiki, check the source, then correct the wiki or the agent, so there is one truth.

Watch the agents, not just their messages:

```bash
hivey swarm list                                   # states (working/idle/blocked) + queued messages
hivey agent read <name>-<agent> --source recent-unwrapped --lines 120   # what it is doing/said
hivey msg inbox                                    # messages for you
hivey msg log --swarm <name> --limit 40            # the team's traffic
hivey swarm info <name>                            # channel, vault, addons, budget, tasks
```

You reach agents with `hivey msg send <agent> "…"` (`@all`, `@role:worker`, `--fyi` never wakes,
`--urgent` interrupts). Delivery waits until the recipient is idle, and you're told when one is
blocked. Messages and alerts arrive in your pane as `[hivey]` prompts; handle them as
coordinator work.

**Handling an alert or message** (one agent):
- **It asked a question**: answer it if it's within the plan; if it's the user's call, ask the
  user and relay the answer.
- **It says it's done**: check the deliverable exists, meets its "done when" and its check
  passes, that it sent DONE to the critic and wrote its report to the wiki. If anything is
  missing, tell it exactly what.
- **It's waiting on a teammate**: check that teammate's state and nudge whichever side holds
  things up.
- **It stopped mid-task** with no question and no DONE: tell it to continue from its own Next
  steps.
- **Blocked on a dialog**: read its pane first; answer only dialogs you understand, ask the user
  about anything else.
- **A safety block** (`hivey safety` hooks): decide whether the operation is really needed. If it
  is, ask the user. Never tell an agent to work around a block, and never turn safety off.
- **Gone** (pane closed or agent exited): `hivey swarm relaunch <name> <agent>`; it continues its
  conversation and its Status section.

**Check every START message.** Each agent's first message restates its task and lists its
assumptions. Compare them with the plan as they arrive: a wrong assumption corrected in minute
two saves an hour.

**A monitoring pass** (each heartbeat, and whenever the user asks for status):
1. `hivey swarm list`, then `hivey agent read` for any agent whose state changed since the last
   pass (all of them every third pass). Read `hivey msg log`, `.swarm/TASKS.md` and the tail of
   `.swarm/events.jsonl`.
2. Glance at token use on the dashboard. An agent burning tokens while its tasks don't move is
   probably looping: read its pane and redirect it. Compare each agent's work with its tasks;
   flag anything missing, out of scope or duplicated, unclaimed tasks whose deps are approved
   (assign them), and `review` tasks nobody is reviewing.
3. Answer open questions, nudge idle agents that still have work, resolve stalled critique.
4. Ingest new reports into the wiki (`references/wiki-memory.md`), and update **Swarm status**
   at the bottom of `.swarm/README.md` and in `wiki/swarm-overview.md` (date, each agent's state
   and deliverables, critique rounds, open decisions, what to do first on resume).
5. Tell the user only what changed or needs their decision; otherwise keep the pass quiet.

**Critique loop.** Every deliverable goes owner → DONE → critic → CRITIQUE r<N> → REVISED →
APPROVED. If owner and critic disagree, or a deliverable reaches round 3 without APPROVED, you
decide: accept, re-scope, or give a specific fix, and tell both.

**Roster changes** (agents added or re-scoped): update `.swarm/README.md` and the swarm section
in every agent's CLAUDE.md, then `hivey msg send @all` what changed. A new agent: scaffold it,
write its brief, and add it with `hivey swarm relaunch <name> <agent>` once it's in the manifest
(or ask the user to relaunch the swarm).

Every agent keeps its own Status section; if one looks stale against its pane output, tell the
agent to update it. Don't close the swarm's space or panes unless the user asks.

**Finishing.** A worker only counts as finished once every deliverable is APPROVED and its final
report is ingested into the wiki. When all workers and the critic have finished, run the
connection pass and write `wiki/swarm-final-report.md` (`references/wiki-memory.md`), check the
deliverables against the user's original request, and give the user the **final report**:
outcome, where each deliverable is, open risks, next steps and the wiki path. If the `archify`
skill is installed, add the visual report (`references/archify-report.md`). Then write the
retrospective (`references/retrospective.md`).

## Pause a swarm

When the user says "pause/stop the swarm (for now)" or "save progress for next run":
1. Tell every agent: `hivey msg send @all --urgent "Pause: stop at a safe point, save your
   in-progress work to files, and rewrite the Status section of your CLAUDE.md (keep the STATUS
   markers). Then reply PAUSED --fyi and wait."`
2. Wait until every agent is idle in `hivey swarm list` and its CLAUDE.md changed after the
   pause began (`ls -l <root>/*/CLAUDE.md`). For any that didn't save, read its pane and ask
   again, or write what you can see into `.swarm/README.md`. Never invent its status.
3. `hivey swarm pause <name>`: holds message delivery; schedules skip a paused swarm.
4. Record half-done work in **Swarm status** (`.swarm/README.md` and `wiki/swarm-overview.md`):
   what's approved, what's open, who does what first on resume, and the resume command. Log it:
   `python3 $S/swarm_tasks.py --root <root> log pause --as coordinator --note "<reason>"`.
5. Tell the user it's paused, what's saved where, and how to resume. Panes and sessions stay;
   stopping the hivey session would stop the agents, so only do that if the user asks.

## Resume a swarm

When `<root>/.swarm/agents.json` exists, the team, briefs, skills and channel already exist, and
each agent's memory is its Status section.
1. Read the wiki first (`index.md`, `swarm-overview.md`, `swarm-final-report.md` if present,
   agent pages, recent findings and decisions), then `.swarm/README.md` (Swarm status),
   `.swarm/TASKS.md` and each agent's Status. Summarize for the user where each agent left off
   and what's next, and ask how much time remains.
2. `hivey swarm resume <name>` releases message delivery. If agents exited or their panes are
   gone (e.g. after a restart), `hivey swarm relaunch <name> [<agent>...]` starts them again,
   continuing their conversations, and reopens dead addons (`--addons-only` for just those).
3. `hivey msg send @all "Resumed: read the wiki and your Status, then continue from Next steps
   and send RESUMED to coordinator."` Check each RESUMED reply against the Swarm status notes.
4. Coordinate as in step 8. For roster changes, update every brief first. Use
   `hivey swarm launch` again only for a brand-new team or when the manifest is missing.
