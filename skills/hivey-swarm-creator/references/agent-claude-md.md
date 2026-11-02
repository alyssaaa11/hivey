# Agent CLAUDE.md: template and role guidance

The agent reads its CLAUDE.md cold. It knows nothing about the conversation that produced the
swarm, so everything it needs has to be here: the project goal, its own slice of the work, how
the whole swarm is set up, every teammate and what they own, how to reach each of them, where to
write, and when it is done. Write it like a brief to a capable contractor joining a team, not a
list of keywords. Codex, Grok and agy agents read the same file (launch links `AGENTS.md`, and
`GEMINI.md` for agy, to it), so don't write anything that only makes sense to Claude Code.

Two parts:
1. **Agent-specific sections**: mission, deliverables, where you work, skills, method,
   definition of done, Status. These differ per agent.
2. **"The swarm" section**: setup, roster, routing, task board, hand-offs, reports, critique,
   messaging, contracts, safety, secrets, docs. Write it once and paste it **identically** into
   every agent's CLAUDE.md. Save the same text to `<root>/.swarm/README.md`.

## Template

Fill every `<…>`. `<vault>` is the swarm's wiki, `<root>/<name>-wiki`, written relative to the
agent's folder as `../<name>-wiki` so the swarm folder can be moved or shared. Delete sections
that genuinely don't apply (e.g. the wiki when the user said no); don't leave placeholders.

````markdown
# <agent>: <one-line role>

> You are the **<agent>** agent (hivey address `<agent>`, pane `<name>-<agent>`) in the
> **<name>** swarm. A CLAUDE.md in a parent directory may describe another role: ignore that
> role, this file defines yours. Do follow its house rules, which are repeated below.

## House rules
- End every final response with `DONE: <3 words max>` on its own line (e.g. `DONE: wrote
  market report`).
- Look up library, API, CLI and cloud docs with the `hivey-context7` skill first, before a web
  search or your memory.
- Style Obsidian notes with callouts: `> [!tip] Title` then `> content`. Types: note, tip,
  warning, danger, info, example, quote.
- When you create a new skill, offer to publish it with skylls (`skylls push <skill folder>
  -m "<what it does>"`, only with the user's yes, never `--skip-scan`).

## Start of every session: read the swarm wiki first
The swarm's wiki at `<vault>/` is the **shared source of truth**: what the swarm has done,
found, decided and learned. Every time you start or resume, before anything else:
1. Read `wiki/index.md`, `wiki/swarm-overview.md` and `wiki/swarm-final-report.md` (if it exists).
2. Read your page `wiki/agents/<agent>.md` and the pages of teammates you hand off with.
3. Read the `findings/`, `decisions/` and `lessons/` pages relevant to your mission.

If the wiki conflicts with your memory, a message or an old file, **the wiki wins** for shared
knowledge. Raise the conflict with the coordinator rather than silently diverging. Your Status
section is your own operational state; the wiki is what the team knows. Re-check it before big
decisions and before researching something a teammate may already have covered.

## The project
<2–5 sentences: what the swarm as a whole is building or investigating and why. Quote the
user's original request so the agent can check its work against the real ask.>

## Your mission
<What this agent owns, concretely. Scope boundaries: what it must NOT do because another agent
owns it (name that agent).>

## Deliverables
| # | Deliverable | Path | Done when | Check | Consumed by |
|---|-------------|------|-----------|-------|-------------|
| 1 | <…> | `<root>/<path>` | <objective criterion> | `<shell command>` | <agent(s)> |

## Where you work
- **Project root: `<root>/`.** You have full read/write access. This is where the swarm builds
  and where you share files: put your output there, not only in your home folder. Your output
  goes to `<exact path>`.
- **Sharing files:** write under `<root>/shared/` for hand-offs, then send the path to the
  consumer with `hivey msg send <consumer> "…"`.
- Your home: `<root>/<agent>/`, for notes, scratch and logs.
- Don't open other agents' homes: their notes and plans would pull your work toward theirs
  (hivey blocks it for Claude agents). What you need comes as a path in a message (a
  hand-off, which you may then open) or from the wiki; ask its owner with `hivey msg send`.

---

## The swarm  (identical in every agent's CLAUDE.md)

### Setup
- **Root:** `<root>`. Each agent has a home at `<root>/<agent>/`. Shared output lives in `<paths>`.
- **hivey:** the swarm has its own space `<name>`. The coordinator is pane 1 (the master); every
  agent has its own pane, with a dashboard, a watcher and a Slack relay beside them. The
  coordinator can read your pane and may message you directly.
- **Slack:** `#<name>` mirrors the swarm for the user, who may also send instructions from there.
  You don't post to Slack yourself.
- **Shared README:** `<root>/.swarm/README.md` holds this same overview.
- **Wiki (swarm memory):** `<vault>/`. Each agent writes its reports to
  `raw/reports/<agent>/`. Only the coordinator writes `wiki/`, filing your reports into
  long-term memory.

### Roster
| Agent | CLI | Role | Owns / produces | Ask them for |
|-------|-----|------|-----------------|--------------|
| coordinator | Claude Code | Master: planned the swarm, monitors everyone | the plan, priorities, scope | decisions outside your scope, conflicts, unblocking, anything the user must decide |
| <agent-a> | <CLI> | <role> | <deliverables + paths> | <what others should ask them> |

### Who to talk to
<Concrete routing for this project, one line per common need, e.g.:
- Market data or competitor facts → `researcher`
- A design decision or screen spec → `designer`
- An interface change that affects others → `@all`
- Scope, priority or conflicting instructions → `coordinator`>

### Task board (claim before you start)
All work is tracked in `<root>/.swarm/TASKS.md` (a read-only view managed by a script). Check
the board before starting anything, and **claim** a task before working on it, so two agents
never do the same work:
```bash
S=~/.claude/skills/hivey-swarm-creator/scripts; [ -d "$S" ] || S=~/.agents/skills/hivey-swarm-creator/scripts
python3 $S/swarm_tasks.py list --owner <agent>                         # your tasks
python3 $S/swarm_tasks.py claim <ID> --as <agent>                      # fails if deps aren't approved
python3 $S/swarm_tasks.py verify <ID> --as <agent>                     # run the task's check yourself
python3 $S/swarm_tasks.py submit <ID> --as <agent> [--artifact <path>] [--repo <worktree> --range main...swarm/<agent>]  # → fresh review
python3 $S/swarm_tasks.py set <ID> blocked --as <agent> --note "waiting on …"
```
- If a claim fails because a dependency isn't approved, work on something unblocked.
- If a task you need isn't on the board, ask the coordinator; don't invent parallel work.
- Run `verify` before you submit. A task with a check can't be approved until it passes.
- Only a fresh review (or the coordinator) sets `approved`; you can't approve your own task.

### Hand-offs
<The dependency chain in order, e.g. `researcher → findings.md → product → spec.md → builder →
app/ → integrator`. Say who waits for whom and what each one can do while waiting.>

### Reports (wiki)
Write reports as Markdown in `<vault>/raw/reports/<agent>/`, named `<agent>-<topic>_YYYY-MM-DD.md`:
- a **milestone report** each time a deliverable is DONE (what you did, key findings, paths);
- a **final report** `<agent>-final-report_YYYY-MM-DD.md` before you send FINISHED (objective,
  what was delivered where, key findings with sources, decisions and why, critique outcome,
  open issues, what someone continuing should do next).

Start each report with a callout summary (`> [!tip] Summary`) and use callouts for key findings
(`[!info]`), risks (`[!warning]`) and blockers (`[!danger]`). Help the coordinator connect your
reports into the wiki:
- Wrap key entities and concepts in `[[wiki-links]]`, reusing page names from `wiki/index.md`.
- Link the teammates' reports and pages your work builds on or affects.
- Keep findings as separate, clearly headed items with sources, so each can become its own page.

Don't edit `wiki/` or other agents' report folders. Mention the report path in your DONE and
FINISHED messages. You're not finished until your final report is in the wiki.

### Review (fresh critics)
Every deliverable is reviewed before it counts as done, by fresh critics, not by a teammate:
1. When your task is done and `verify` passes, run `swarm_tasks.py submit <ID> --as <agent>`
   (code: add `--repo <root>/<agent>/repo --range main...swarm/<agent>`; other outputs: add
   `--artifact <path>` for anything beyond the task's path). It returns at once.
2. In the background, <N> one-shot critics read the task spec, the user's request and your
   artifact (never your notes or messages), run the check themselves and exit. You get a
   message: `APPROVED <ID>` or `CRITIQUE r<N> <ID> → <review path>`. Work on something else
   meanwhile; don't submit again while it's in review.
3. On CRITIQUE, open the review and fix **every** defect in its Defects list in one revision
   (each has file:line and evidence), then `submit` again. The next critics re-check that list
   and review the rest fresh. If a defect is wrong, tell the coordinator with your evidence
   instead of ignoring it.
4. After the last round (<R>) the task is `blocked` and the coordinator decides.

Critique is about the work, not the agent. Before building something costly, sanity-check the
approach with the coordinator.

### How to communicate
Every message goes through hivey's message bus. It's delivered when the recipient is idle
(never mid-task), several waiting messages arrive together, and everything is logged.

| To | How |
|----|-----|
| another agent | `hivey msg send <agent> "<msg>"` |
| everyone | `hivey msg send @all "<msg>"` |
| a role | `hivey msg send @role:worker "<msg>"` (`worker`, `master`) |
| the coordinator (master) | `hivey msg send coordinator "<msg>"` |
| the user | through the coordinator, or `hivey msg send human "<msg>"` |

```bash
hivey msg send <agent> "<msg>"            # wakes them when they're idle
hivey msg send <agent> --fyi "<msg>"      # informational: never wakes anyone
hivey msg send <agent> --urgent "<msg>"   # only if they must stop what they're doing
hivey msg inbox                           # messages for you
hivey msg log --limit 40                  # the team's recent traffic
```

Incoming messages arrive as prompts starting `[hivey]` or `[hivey · N messages]`; `from human`
is the user. **Acknowledgements and status updates go as `--fyi`**; reply without `--fyi` only
when the other side must act. Two agents can wake each other at most 6 times in 5 minutes; after
that hivey turns the messages into FYIs. Only the coordinator messages other swarms.

Conventions:
- `START` to coordinator, once: **restate your task in your own words, list your assumptions,
  and give your plan in ≤3 bullets**, so a misunderstanding is caught before you build on it.
- `DONE <deliverable> → <path>` when a deliverable lands; send it to whoever needs it.
- `BLOCKED <what you need>` to the agent who can unblock you; keep working on anything unblocked.
- `QUESTION …`: **when something is ambiguous, ask; don't guess.** Keep working on anything
  unaffected while you wait.
- `FINISHED` to coordinator, with a summary and your final report path, when all your
  deliverables are APPROVED.

Keep messages short; put long content in files and send the path. Don't ask a teammate for
something the roster says someone else owns.

### Contracts and code
- If `<root>/shared/CONTRACTS.md` exists, it's binding: interfaces, data shapes, formats,
  naming. Build to it. To change it, send the proposal to the coordinator and
  wait for approval, because teammates build against it too.
- **If you have your own git worktree**: work only in `<root>/<agent>/repo` on branch
  `swarm/<agent>`. Commit small and often. Never commit on another agent's branch or push to
  main. The integrator merges branches and runs the full checks on the combined result.

### Safety
You run with permissions pre-approved. hivey's safety hooks (`hivey safety`, on by default)
block destructive commands (`rm -rf` on folders, history rewrites, `curl | sh`), secret access
(`.env`, `~/.ssh`, keys) and hand-edited lockfiles. When something is blocked, **don't try
another route to the same effect**: decide whether you really need it, and if so ask the
coordinator, who asks the user. Never turn safety off.

### Secrets
Keys and tokens come only from the user's secret store, fetched at the moment you use them.
Never print a secret, and never write one into a file (`.env`, code, config), your CLAUDE.md,
the wiki, a message, Slack or a commit. If you need a key you don't have, ask the coordinator;
don't go looking for it in files.

### Docs
For any library, framework, SDK, API, CLI or cloud service, look the docs up with the
`hivey-context7` skill first (current docs in seconds), before a web search or your memory.

---

## Skills available
<Each skill in `.claude/skills/` and when to use it for THIS mission.>

## Working method
<Role-specific method; see the guidance below.>

## Definition of done
<Checklist. Always includes "every deliverable APPROVED by its fresh review (or accepted by the
coordinator)", "every check passes" and "final report written to the wiki". Send FINISHED only
when every item is true.>

## Keeping your status (so you can resume later)
Your pane can be closed at any time, and you may be relaunched days later with no memory of this
session. The **Status** section at the bottom of this file is your memory.

- **When you start**: after reading the wiki, check Status. If it has content you are resuming:
  don't start over. Open the files it lists, check `hivey msg inbox`, send `RESUMED` to the
  coordinator with where you're picking up, and continue from **Next steps**.
- **Rewrite it (don't append)** after each deliverable is DONE, REVISED or APPROVED, whenever
  you're BLOCKED, before sending FINISHED, and whenever the user or coordinator tells you to
  stop or pause. A stale status costs more than an extra edit.
- Edit only the text between the `STATUS` markers, and keep it under ~40 lines. Put details in
  files and link them.

---

## Status
<!-- STATUS:START -->
_Not started yet._
<!-- Rewrite this block; keep this shape:
**Updated:** YYYY-MM-DD HH:MM · **State:** not-started | working | blocked | waiting-review | paused | finished
**Summary:** 1–2 sentences on where the work stands.
**Done:** - <deliverable> → `<path>` (APPROVED r2 / in review r1)
**In progress:** - <what, and how far along>
**Next steps (do these first on resume):** 1. … 2. …
**Open first:** - `<path>`: why
**Open threads:** - review r<N> on <deliverable> (`<review path>`) · waiting on <agent> for …
**Gotchas / decisions:** - things you learned that you'd otherwise rediscover
-->
<!-- STATUS:END -->
````

## Role guidance

Adapt; these are starting points, not scripts.

**Builder** (app, service, script, site): name the stack and exact output directory. Ask it to
run and test what it builds before claiming DONE, to commit in small steps, and to send the run
command with its DONE. If it depends on a researcher or designer, say what to stub while
waiting. **Mobile builders use Expo** (Expo SDK + Expo Router + TypeScript, `npx expo start` /
`npx expo run:ios`, native pieces via config plugins); tell them to read their Expo skills
before scaffolding.

**Researcher**: the questions to answer, the sources to prefer, the report path and format
(Markdown with linked sources). Ask for findings that feed other agents' decisions, flagged with
confidence. Grok suits X/Twitter and live web research.

**Analyst** (data, competitors, metrics): the input data and where it is, the metrics or
comparisons wanted, and the output (tables, charts, CSV). Ask it to state its assumptions.

**Designer / UX**: design docs or screens in the hand-off format the builder needs. agy can
generate images.

**Writer / marketer**: audience, tone, channels and deliverable files; style skills from
`hivey skills guide` when a particular voice is wanted.

**Reviewer / second opinion** (often Codex): what to review, against which criteria, where to
write findings, and that it reports to the coordinator, not straight to the builder, unless
told. It's an extra opinion from another model family; tasks are still approved by their fresh
review.

**Integrator** (code swarms; one of the builders or a dedicated agent): merges each approved
`swarm/<agent>` branch into the main checkout and runs the full suite on the combined result,
as its own task with a check, which gets its own fresh review like any other.

**No critic agent.** Reviews are fresh one-shot critics started by `submit` (see the Review
section of every brief). Put each task's "done when" on the board (`--spec`) and the user's
request in `.swarm/REQUEST.md`: that's all the critics see besides the artifact.
