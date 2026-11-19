---
name: hivey-job-creator
description: Turn a job (a feature, a bug list, a refactor, a to-do list) into small independent tasks, each a GitHub issue with acceptance tests the coordinator writes first, and fix them in parallel with one agent per issue - Claude Code cloud sessions via hivey cloud (model and effort picked per task with model-picker), or local hivey agents for tasks that need this Mac. This session is the coordinator - it plans with the user, writes the tests, launches the agents in waves, checks every pull request (tests run on it in a clean worktree, the agent may not touch the tests), sends failures back to the agent until they pass, merges passing work into a job branch, and opens one final pull request for the user to review. Use whenever the user says "hivey job", "/hivey-job-creator", "split this into issues and fix them in the cloud", "one agent per issue", "cloud agents for my to-do list", "fix these issues in parallel and send me a PR", or wants a task list worked by several agents that hand in pull requests, even without saying cloud. For a long-lived local team that talks to each other use hivey-swarm-creator instead.
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey job creator

You are the **coordinator**. The user gives you a job; you split it into small tasks, write a
GitHub issue and failing acceptance tests for each, start one agent per task, check every pull
request they hand in, send failures back, merge what passes into `job/<name>`, and at the end open
**one** pull request `job/<name>` → base for the user to review. Agents never merge anything and
never touch the base branch.

The workers are **Claude Code cloud sessions** (`hivey cloud`, Claude only), each on its own
branch with the user's skills, settings and secrets. Tasks that need this Mac (local database,
devices, private network, the user's apps) are **local tasks**: you run them here (below).

**Who coordinates.** The user's standing coordinator is the hivey agent `job-coordinator`
(folder `~/.hivey/jobs`, a ★ row in hivey's sidebar). It runs one job after another and stays
when a job is done; only the job's cloud agents are closed. If you are not that agent and the
user wants a job run, set it up once and hand the job over instead of coordinating here:

```bash
python3 ~/.claude/skills/hivey-job-creator/scripts/job.py coordinator \
  --kickoff "New job in <repo path>: <the job, in the user's words>"
```

It registers and starts the agent (Claude Code, Opus, with this skill), wakes it every 20
minutes for a watch run, starts the board, and sends it the job (or, if it already exists, just
sends the job). Coordinate in this session only when the user asks for that.

**The board.** `python3 $J board` serves the live job board on this Mac and registers it on the
agent fleet page's 🖥 Dashboards tab as **Jobs**: every task as a node, dependencies as arrows,
the final PR at the end; colour = status, a pulsing dot = an agent is working on it, with its
model, check count and links to the issue, the cloud session and the PR. Run it when a job
starts and tell the user where to look.

Script: `J=~/.claude/skills/hivey-job-creator/scripts/job.py` (`python3 $J --help`). It keeps the
job file `~/.hivey/jobs/<name>.json`, so a restarted coordinator picks up where it stopped:
start every session on an existing job with `python3 $J show <name>`.

## 0. Preconditions (check, then tell the user what is missing)

- The repo is on GitHub, `gh auth status` is fine here, and the working tree is clean.
- The **Claude GitHub App** is installed on the repo (claude.ai/code): without it cloud agents
  cannot push. Check with `hivey cloud list` / a past run, or ask.
- `.hivey/cloud.toml` exists (`hivey cloud init`). For agents to open pull requests themselves
  they need `GH_TOKEN` (a fine-grained token for this repo: Contents + Pull requests read/write) as
  an `encrypted` secret there, which needs `hivey cloud setup` once. Without it, agents push a
  branch and **you** open the pull request (step 6) — that works too; don't block on it.
- A test command that runs headless and finishes in minutes (`npm test`, `cargo nextest run`,
  `pytest -q`, …). Find it in the repo (package.json, justfile, Makefile, CI config) and confirm.

## 1. Plan with the user (nothing is created before they agree)

Read the code the job touches. Split the job into tasks that are:

- **small**: one agent, one sitting, one reviewable diff (roughly under 300 changed lines);
- **independent**: tasks that edit the same files or depend on each other's code get an explicit
  dependency (`deps`) and run in a later wave — conflicting parallel PRs are the main way this
  fails;
- **testable**: each has acceptance criteria you can turn into an automated test. A task with no
  testable outcome (docs, copy) still gets a check you can run (a lint, a build, a grep).
- **placed**: `cloud` unless it needs this Mac → `local`.

Pick **model and effort per task** with the `model-picker` skill (load it; follow its house rules).
These are worker agents with a reliable machine check (your tests), so start low: Sonnet or
Haiku at low/medium for mechanical, well-specified tasks; Opus medium/high for tasks that need
design judgement or touch tricky code; Fable at most medium, only for the hardest. Escalate a task
one step when it fails twice.

**Write the plan down** as `~/.hivey/jobs/<name>/plan.md` from
[references/plan-template.md](references/plan-template.md): the goal, what you found in the code,
the task table (task, files, deps, where, model/effort and why), the waves as a small diagram,
the test strategy, the risks and what is out of scope. Then write **one brief per task** in
`~/.hivey/jobs/<name>/tasks/<k>-<slug>.md` from
[references/task-brief.md](references/task-brief.md). The brief is everything that agent will
know — it starts with an empty context in a fresh clone — so make it specific: the files and
functions to read first, the approach you'd take and why, the edge cases, the exact tests it must
pass, what not to touch, and a done checklist. A vague brief is the most common reason an agent
hands in the wrong thing; spend your effort here.

Show the user the plan (summarize it in chat and give the file path; offer it as a page if they
want to share it): the wave order, the max agents at once (default 3: each cloud agent uses the
user's Claude plan; check usage if the user is near a limit), the test command and the max fix
rounds per PR (default 3, then it comes back to the user). Adjust plan and briefs until the user
says go.

## 2. Job file, issues, job branch

```bash
python3 $J init <name> --test-cmd "<cmd>" --tests "tests/job_<name>/**" --base main
gh label create "job:<name>" --color 5319e7 2>/dev/null
```

One parent issue for the whole job (its body: plan.md), then one issue per task whose body **is
that task's brief** (`gh issue create --body-file ~/.hivey/jobs/<name>/tasks/<k>-<slug>.md`),
labelled `job:<name>`, plus `model:<id>`, `effort:<level>`, and `where:local` when it is local
(create labels as needed). `hivey cloud run --issue` hands the agent the issue body, so the brief
is exactly what the agent reads. Fill in the issue numbers in plan.md and the briefs' "Part of" /
"Depends on" lines (`gh issue edit <n> --body-file …`) once they exist.

Record each one: `python3 $J add <name> <issue> --model <m> --effort <e> --where cloud --deps 3,4 --title "<t>"`.

## 3. Tests first, on the job branch

Make the job branch from the base, write the acceptance tests for **every** task, commit, push:

```bash
git switch -c job/<name> origin/main
# write the tests; they must fail now for the right reason (feature missing), not crash on setup
<run them: they fail>   # and the rest of the suite still passes
git add <tests> && git commit -m "test(<name>): acceptance tests for the job" && git push -u origin job/<name>
```

Put them where the job's protected glob (`--tests`) covers them. If the project marks expected
failures (`xfail`, `#[ignore]`, `it.todo`), don't: the agent should see them fail. If failing
tests would break the project's CI on the job branch, that's fine — the job branch is not the base.

## 4. Launch a wave

The tasks that can start: `python3 $J ready <name>`. For each (respecting the max at once):

```bash
hivey cloud run --job <name> --issue <n> --base job/<name> --pr-base job/<name> \
  --model <m> --effort <e>
python3 $J set <name> <n> status=running
```

`--issue` puts the issue text and the house rules in the agent's prompt (agents get: make the
listed tests pass, never edit them, stay in scope, run the full suite, mark the PR with
`hivey-job:<name>#<n>`). `--base job/<name>` gives the agent your tests. Model and effort are
pinned in the agent's own branch settings. Tested on 2026-11-19: the **model** applies in the
cloud; the **effort** is passed but the cloud session showed its default (medium), so choose the
model per task and treat effort as best effort. Add task-specific guidance after `run` as the task text
only when the issue is not enough (then also pass the issue number).

Shortcut for a whole wave with the models in the labels: `hivey cloud issues --label job:<name>
--job <name> --base job/<name> --pr-base job/<name> --max 3` (skips issues that already have an
agent and `where:local` ones; mind deps — it doesn't know them, so only use it when the open
issues in the label have no unmet deps).

**Local tasks**: do them here in a git worktree on `job/<name>-<n>` made from `origin/job/<name>`
— yourself, a subagent pinned to the picked model, or the user's other CLIs through the
`hivey-codex`, `hivey-grok` or `hivey-agy` skills (headless, in that worktree) — then push and
open the PR into `job/<name>` with the marker line, exactly like a cloud agent would.

## 5. Watch

Run `python3 $J wait <name>` **in the background** (Bash `run_in_background`); you are woken when an
agent's state changes (pushed, PR opened, new commits). Meanwhile `hivey cloud status --job <name>`
and `python3 $J show <name>` show everything; the sessions are on claude.ai/code and in the Claude
app (`hivey cloud pull <session>` continues one here). Restart `wait` after handling each change.
If nothing changes for a long time (an hour+), look at the session and nudge it with
`hivey cloud send <session> "<question>"`.

## 6. Review each pull request

- **Pushed but no PR** (agent had no token): open it — `gh pr create --base job/<name> --head
  <branch> --title "<task> (#<n>)" --body "hivey-job:<name>#<n>\n\nRefs #<n>"`.
- **PR open or new commits**: `python3 $J check <name> <pr>`. It refuses PRs that target the wrong
  branch, carry hivey bundle files or change the protected tests; otherwise it merges the PR head
  onto the latest job branch in a throwaway worktree and runs the test command. Then read the diff
  yourself (`gh pr diff <pr>`): scope, quality, no test weakening hidden elsewhere (skips,
  mocks of the thing under test, loosened assertions). For anything non-trivial, have a fresh
  critic review it (`fresh_review.py` from the hivey skill, if installed) — cheap insurance.
- **Fails** (check or your review): send the agent exactly what to fix —
  `hivey cloud send <session> "<failing test names + the relevant tail of the output + your review
  points>. Push the fix to the same branch."` Keep it specific. After `max_attempts` failed
  checks (`check` says `escalate`), stop and bring it to the user with what you tried; or relaunch
  the task one model/effort step up.
- **Passes**: `python3 $J merge <name> <pr>` (squash into `job/<name>`, only if the passing check
  is on the current head). It prints the tasks that are now unblocked: launch them (step 4). Other
  open PRs may now conflict; `check` reports a merge conflict — tell that agent to rebase onto
  `origin/job/<name>`.

Never merge into the base branch, never force-push the job branch, never edit an agent's PR
branch yourself (send the fix instead; local tasks excepted).

## 7. Finish

When every task is merged: `python3 $J finish <name>` runs the test command on the job branch and
opens the final pull request `job/<name>` → base with `Closes #n` for each issue and the test
output, then closes the job's cloud agents (`hivey cloud close --job <name>`: hivey forgets
them and deletes their `hivey-cloud/*` bundle branches; their PR branches stay). Close the parent
issue's checklist, update plan.md's log and your Status, and report to the user: the PR link, per task the model/effort and fix rounds,
anything you escalated or left out, and what to look at first when reviewing. Then wait for the
next job: the coordinator stays.

## Rules

- The user reviews and merges the final PR; you merge only into `job/<name>`.
- Your tests are the definition of done. If a test turns out to be wrong, fix it yourself on the
  job branch (commit `test(<name>): …`), tell the agent, and note it in the final report.
- Every agent works on exactly one issue; scope creep goes into a new issue, not the PR.
- Secrets reach cloud agents only through `.hivey/cloud.toml` (EnvSave opaque keys); never paste a
  secret into an issue, a prompt or `hivey cloud send`.
- Keep the job file current (`set … status=…`): it is how you, or the next coordinator session,
  know where things stand.
