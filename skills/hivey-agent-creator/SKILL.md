---
name: hivey-agent-creator
description: Design, scaffold and launch ONE specialized, long-lived hivey agent (a solo agent, its own master) for a particular task, on Claude Code, Codex, Grok or Antigravity (agy). It gets its own folder, a detailed CLAUDE.md brief (mission, deliverables with runnable checks, method, rules, resumable dated Status), hand-picked skills, project-scoped MCP servers only when needed, tool permissions, an optional done gate, optional Obsidian wiki memory, its Slack channel #<name> and an optional schedule, then it is launched with `hivey swarm launch --solo`. Use whenever the user says "create an agent", "new agent", "make an agent that does X", "an agent that does X every day", "build me an agent for <task>", "/agent-creator", "/hivey-agent-creator", or when it runs as the setup pane of `hivey swarm new --agent`. For a team of cooperating agents use `hivey swarm new` instead.
argument-hint: <task description or name>
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey agent creator

You design, scaffold and launch **one** specialized agent in hivey. You don't become that
agent: you are the designer. The user wants something that runs with little supervision, so
most of the value is in the design and in the agent's `CLAUDE.md`, the only brief it will ever
get.

You run either as the setup pane of `hivey swarm new --agent "<task>"` (then `HIVEY_SETUP_TASK`
holds the task and `HIVEY_SETUP_CWD` the folder the user ran it from) or because the user typed
`/hivey-agent-creator`. The `/hivey` skill knows every hivey command; `hivey swarm <command>
--help` shows usage without running anything.

Scripts live in `~/.claude/skills/hivey-agent-creator/scripts/` (`$A` below). References are in
`~/.claude/skills/hivey-agent-creator/references/`.

## 1. Name and folder

**Ask for the name first** (one question). Suggest one from the task or the folder's name,
normalized to `[a-z][a-z0-9-]{0,31}`; skip the question if the task already says "Name: …".
That one name is used everywhere: the slug (`--slug`), the Slack channel `#<name>` and the
wiki `<agent folder>/<name>-wiki`. Never prefix it (`agent-` or anything else).
In the same question suggest an **emoji** for it (e.g. 📺 for a YouTube agent, 📝 for a
writer) unless the task already says "Icon: …"; the sidebar shows its space as
`<emoji> <name>` (`--icon`, step 8). The user can pick another or none.

**The agent folder** (`$DIR` below):
- `$HIVEY_SETUP_CWD` when you're a setup pane, or your current folder, if it is empty or holds
  only what the user put there for this agent.
- Otherwise `~/agents/<name>` (create it).
- Stop and ask if the folder is `$HOME`, a project with unrelated code, or already has a
  `CLAUDE.md`/`AGENTS.md` (offer **Resume**, another folder, or replace).

## 2. Understand the task

From the request, work out:
- **mission**: what the agent owns, and what it must not touch.
- **deliverables**: exact output paths, a "done when" for each and, where possible, a **check
  command**: a shell command that exits 0 only when the deliverable really is done (`pytest -q`,
  `test -s reports/latest.md && grep -q '## Sources' reports/latest.md`, a schema validator).
  Runnable checks catch "claimed done but isn't", which prose criteria miss. Keep outputs out
  of `dist/`, `build/`, `target/`, `.next/`, `.venv/`, `node_modules/`; use `reports/`,
  `output/`, `data/` or similar.
- **the run check**: the deliverables' checks combined into one command that says "this run is
  done", usually `bash scripts/check.sh` in the agent folder (written in step 6). It can back a
  **done gate** (step 4). Make it strict about what matters, but able to pass on a legitimately
  quiet run (e.g. a "nothing new" marker). For a builder it reads `tasks.json`.
- **one-shot or ongoing**: a job with an end, or a standing role (daily scans, a queue). Ongoing
  agents need a cadence and a place to log each run.
- **coding agent and model** (step 3 asks):

  | Kind (`--kind`) | Suggest it for | Model |
  |---|---|---|
  | `claude`, Claude Code (default) | most work: building, research, writing, ops; browser control | `opus` for code and deep reasoning; `sonnet` for research, writing, routine ops; `haiku` for trivial jobs |
  | `codex`, Codex | code review, a second opinion from another model family | leave out to use the user's configured model |
  | `grok`, Grok | X/Twitter and web research | `grok models` lists them |
  | `agy`, Antigravity (Gemini) | fast, cheap tasks; image generation | see the `hivey-agy` skill's model table |

  All four read the brief: you write `CLAUDE.md`, and launch links `AGENTS.md` (and `GEMINI.md`
  for agy) to it. Only Claude Code gets hivey's safety hooks, the done gate and the reviewer
  subagent; say so when you suggest another kind.
- **schedule** (ongoing agents): on demand only, an interval (`--heartbeat 6h`), or fixed times
  (`hivey swarm schedule add <name> --at 08:57 …`). Draft the exact prompt each run gets.

If the request is too vague to fill these in, ask one short round of questions. Don't guess the
core job.

## 3. Pick capabilities

**Skills.** Run `hivey skills guide` and follow it: skylls first (the user's and friends'
published skills), skills.sh only with the user's OK, and skills land in the agent's own
`$DIR/.claude/skills/`, never globally. Pick only skills the agent will use (usually 2 to 6):
each one costs context in every session. List **skill gaps** (needs no skill covers) as
suggestions; don't build them now. A non-Claude agent doesn't load `.claude/skills/` by itself:
its brief names each skill with its path (`.claude/skills/<skill>/SKILL.md`) and says when to
read it. Before designing from scratch, `skylls --json agents find <words> --limit 5` may show
the user or a friend already has this agent.

**MCP servers.** Only the ones the task really needs, and only with the user's OK. They go
**project-scoped** in `$DIR/.mcp.json`, so only this agent loads them; hivey installs none.
Library docs need none: the `hivey-context7` skill covers them. Prefer a CLI when one does the
job. Note which servers need an API key.

**Tools.**
- CLIs the job needs (`gh`, `ffmpeg`, `uv`, `yt-dlp`…): check each with `command -v` and list
  the missing ones with an install command. Don't install system software unasked.
- Secrets: only from the user's secret store, fetched at the moment they're needed. The brief
  names the key (or the command that fetches it), never the value. Never write a secret into
  a file, prompt, message or commit, and never ask for one in chat.
- Permissions: hivey launches every agent with permissions pre-approved (Claude Code
  `--dangerously-skip-permissions`, Codex without approvals or sandbox, Grok `--always-approve`,
  agy `--dangerously-skip-permissions`). For Claude Code agents, hivey's **safety hooks** guard
  that: check they're on with `hivey safety` (on by default; if off, tell the user, and don't
  turn them on or off yourself). Add `deny` rules the job calls for (`Bash(git push:*)` is
  always added: agents commit, they never push). If the agent touches money, production or
  other people's accounts, say so in the plan and make its brief require asking first.
- Extra folders it may read or write: list them (`--dirs`); Grok can't take extra folders.

**Directory profile.** A one-line **description**, its **skills** and its **tools** (CLIs and
`mcp:<name>`). They go into `hivey swarm directory`, where other agents read them to decide
whether to ask this agent for help.

## 4. Confirm the plan

Show the plan and wait for a yes before creating anything:

```
Agent: <emoji> <name> — <one-line role>    folder: <DIR>
Kind: one-shot | ongoing (<cadence>)       runs on: Claude Code | Codex | Grok | agy, model <m> (<why>)
| # | Deliverable | Path | Done when / check |
Skills: <a> (why), <b> (why)               gaps: <gap> → suggestion
MCPs:   none | <x> (why, needs key: <name>)    CLIs: <ok…>  missing: <tool> → <install cmd>
Permissions: pre-approved + hivey safety on | (non-Claude: no safety hooks)   deny: git push, <…>
Extra dirs: <paths>
Run check: <command> · done gate: yes | no · reviewer: yes | no   (Claude Code only)
Memory:   <name>-wiki | none
Slack:    #<name> | none (Slack not connected)
Schedule: on demand | --heartbeat <6h> | at <HH:MM>, … → "<run prompt>"
Profile:  "<description>" · skills <a,b> · tools <x,y>
```

In the same message ask what you can't infer, one question at a time if you have no question
tool (AskUserQuestion in Claude Code; otherwise plain questions):
1. **Coding agent and model**, with your suggestion first and why (step 2's table). If the user
   already named one, don't ask.
2. **Memory**: *Obsidian wiki* (`<DIR>/<name>-wiki/`, a Karpathy-style LLM wiki the agent
   maintains, inside its own folder so the agent and its knowledge move and share as one
   folder; best for research, domain and content agents whose knowledge compounds) or *none*
   (the Status section is its only memory; fine for one-shot jobs). Never ask where the user's
   Obsidian is.
3. **Schedule** (ongoing agents only): your recommendation first. Runs happen only while hivey
   runs; each wake-up arrives as a `[hivey]` message when the agent is idle, never piles up and
   never expires.
4. **Done gate and reviewer** (Claude Code only): propose the reviewer when the output can be
   checked against sources or specs and a second read is worth the tokens; leave it out for
   routine ops.

**Slack is not a question.** Run `hivey slack status`. Connected: launch creates (or joins)
`#<name>`, invites the user and starts the relay. Not connected: ask the user to run `hivey
slack connect` in a terminal (never ask for the token in chat), or go on without Slack if
they'd rather not.

Adjust the plan if the user tweaks it; continue once they approve.

## 5. Scaffold the folder

```bash
python3 $A/setup_agent.py "$DIR" --name <name> --kind <claude|codex|grok|agy> \
    [--dirs /extra/path,…] [--allow 'Bash(gh:*)'] [--deny 'Bash(rm:*)'] \
    [--mcp '<server>={"command":"npx","args":["-y","<pkg>"]}'] \
    [--check-cmd 'bash scripts/check.sh'] [--gate] [--reviewer] [--git]
```

It is idempotent (re-running only adds) and prints JSON. It writes:
- a `CLAUDE.md` skeleton if there's none (sections and the Status markers; step 6 fills it);
- `.claude/settings.json` in the agent folder: `additionalDirectories` (the folder plus
  `--dirs`; launch turns them into `--add-dir`), `allow`/`deny` rules (`Bash(git push:*)` always
  denied) and, for MCP servers, `enabledMcpjsonServers` so no approval dialog blocks startup;
- `.mcp.json` with each `--mcp` server (`--enable-mcps x,y` pre-approves servers already in
  it). For a Codex agent the same servers are copied into `.codex/config.toml`. Grok and agy:
  set up MCP servers with their own CLI if really needed, or prefer a CLI tool;
- `.agent/config.json` with the run check; with `--gate` (Claude Code) the **done gate** and
  **resume context** hooks are registered in the agent's own `.claude/settings.json`, never the
  global one; `--reviewer` installs the read-only `reviewer` subagent in `.claude/agents/`;
- `.gitignore` (keeps `.mcp.json`, `.codex/`, `.agent/state/`, `logs/` out of git) and, with
  `--git`, a git repo in the folder if it isn't in one.

Then install the skills as `hivey skills guide` says, from `$DIR`, and check `ls
$DIR/.claude/skills`. For keys an MCP server needs, the user puts them in `.mcp.json` (or the
environment) from their secret store; tell them which servers hold a key (the file is
git-ignored).

The harness, when chosen:

| Piece | What it does |
|---|---|
| Done gate | Stop hook `agent_gate.py`: while `.agent/state/run_active` exists, runs the check when the agent tries to finish and blocks it (at most 3 times), logging to `.agent/state/gate.log` |
| Resume context | SessionStart hook `agent_context.py`: on startup, resume and compaction injects run-in-progress, gate log, `tasks.json` progress, recent commits and the newest progress-log entries |
| Reviewer | `.claude/agents/reviewer.md`: read-only subagent (Sonnet) that checks artifacts against The ask and their sources |

The hooks point at this skill's scripts, so improving them upgrades every agent. Codex, Grok
and agy agents run the check themselves (the template says how).

## 6. Write the CLAUDE.md

Read `references/agent-claude-md.md` now: it has the template and guidance per agent type.
Write `$DIR/CLAUDE.md` (replace the skeleton):
1. For agents that write code, start with the general guidelines block: copy
   `references/global-guidelines.md` verbatim (it carries its own `<!-- BEGIN GUIDELINES -->`
   and `<!-- END GUIDELINES -->` markers).
2. Then the agent's own brief from the template: mission, deliverables each with a runnable
   check, method, rules (secrets, safety, docs with `hivey-context7` first, asking before
   anything destructive or outward-facing), memory, and the dated **Status** section it rewrites
   so a fresh session can resume.

Make it detailed and specific: quote the user's request verbatim, use absolute paths, name each
skill and MCP with when to use it, and include the runnable checks. The agent reads this cold.
A vague brief produces a vague agent.

**But keep it short.** Long briefs get partly ignored.
- Keep the agent's own part to about **150 lines**: `awk '/<!-- END GUIDELINES -->/{f=1;next} f'
  CLAUDE.md | wc -l` (or `wc -l CLAUDE.md` without the block).
- Keep Method to the essential steps; put the full method, examples and edge cases in
  `$DIR/METHOD.md` and have the brief say "read METHOD.md before every run".
- Put hard rules in settings, the check command and the gate, not only in prose.

Also write, in this step:
- `scripts/check.sh`: the run check. Executable, exit 0 only when a run is really done. Run it
  once now: it should fail cleanly before the first run.
- `tasks.json` for builder agents (format in the template).

Keep the template's **Schedule** section only if there's a schedule and its **Slack** section
only if Slack is connected; otherwise delete them and their checklist lines.

## 7. Memory (if the user chose a wiki)

After `CLAUDE.md` exists, invoke the `hivey-create-wiki` skill with args `<name> --in "$DIR"`.
It creates `$DIR/<name>-wiki/`, tailors it to the brief and patches the brief with a wiki
section. Answer its questions yourself from the plan. Then check that the template's Memory
section and the patched section agree (keep one). Tell the user the vault path.

## 8. Launch

```bash
hivey swarm launch "$DIR" --slug <name> --icon <emoji> --solo --kind <claude|codex|grok|agy> [--model <m>] \
    --description "<one line>" --skills a,b --tools "gh, mcp:x" \
    [--heartbeat 6h --heartbeat-task "Start a run (Run protocol): <run prompt>"] \
    [--kickoff "<first concrete task>"]
```

The agent opens in a **new space `<emoji> <name>`** with a ★ row in the sidebar, as its own master.
Your pane stays where it is. With Slack connected, launch creates `#<name>` and its relay; add
`--slack` to make a missing connection an error, `--no-slack` to skip it.

Fixed-time runs, one entry per time (`--at` is daily):
```bash
hivey swarm schedule add <name> --at 08:57 --id morning "Start a run (Run protocol): <run prompt>"
hivey swarm schedule add <name> --every 30m --id check "<task>"
hivey swarm schedule list <name>
```
No cron, `/loop` or CronCreate: hivey wakes the agent.

**Verify it started.** `hivey swarm info <name>`, then `hivey pane read <pane> --source visible`
(pane from `hivey swarm list`): it should have read its brief and restated its task correctly.
If it waits on a dialog, answer it or tell the user. If it misread the task, correct it now
with `hivey msg send <name>/master "…"`: a wrong assumption fixed in minute one saves an hour.

## 9. Summary

```
Agent "<name>" created
Runs on:  Claude Code | Codex | Grok | agy (<model>)
Folder:   <DIR>                  CLAUDE.md: <n> agent lines (+ METHOD.md <n> lines)
Skills:   <list>  (gaps: …)
MCPs:     <list> | none          (keys in .mcp.json for: …)
Memory:   <DIR>/<name>-wiki | none
Slack:    #<name> | none
Schedule: on demand | heartbeat <6h> | at <times>
Harness:  check `<cmd>` · done gate · resume context · reviewer | none
Space:    <emoji> <name> (★ in the sidebar)
Talk to it:  hivey msg send <name>/master "…"   (or in #<name>)
Relaunch:    hivey swarm relaunch <name>
Profile:     hivey swarm profile <name> --description … --skills … --tools …
Saved:       skylls agents push (v<n>) | not saved
```

Tell the user they can close this designer tab.

## 10. Save and share (skylls)

If `skylls --version` works, offer to save the agent so the user can reuse and share it. Only
with their yes: `skylls agents push "$DIR" -m "<what it does>"` (it takes the agent's memory
along), plus `skylls push <skill folder>` for any new skill you wrote. Push scans for secrets
and personal data and refuses if it finds any: remove them, never pass `--skip-scan`. Share
(`skylls agents share <name> @user`) only when the user asks.

## Resume and change

If the folder already has a `CLAUDE.md` and the user wants that agent back, don't redesign it:
read the brief (including Status) and run `hivey swarm relaunch <name>` (it continues the
conversation; `hivey swarm register "$DIR"` first if it was unregistered). To change it, edit
the relevant brief sections and rerun `setup_agent.py` for new permissions, MCPs or the
harness (idempotent), then update its directory entry with `hivey swarm profile`. To add a
schedule later, use `hivey swarm schedule add` and add the template's Schedule section. To add
Slack later, `hivey slack add <name>`.

## Rules for you, the designer

- Confirm the plan before creating anything; ask before anything destructive or outward-facing.
- Only touch the agent folder (and the extra folders the plan names).
- Never edit the global `~/.claude/settings.json`, global skills or other agents' folders.
- If a safety hook blocks you, ask the user; never work around it or turn safety off.
- Look up any library, API or CLI docs you need with the `hivey-context7` skill first.
- End your final message with `DONE: <3 words max>` on its own line.
