# Job plan template (`~/.hivey/jobs/<name>/plan.md`)

Write it for the user first (they approve it) and for yourself later (a restarted coordinator
reads it). Plain words, short sections, tables where there is a list. Keep it current: when a
task is split, escalated or dropped, update the table and say why in the log.

```markdown
# Job: <name> — <one-line goal>

**Repo:** <owner/repo> · **Base:** `main` · **Job branch:** `job/<name>` · **Parent issue:** #<n>
**Tests:** `<test command>` · **Protected tests:** `tests/job_<name>/**`
**Agents at once:** 3 · **Fix rounds per PR:** 3 · **Final review:** one PR `job/<name>` → `main`

## Goal
What the user asked for and why, in two to four sentences. What "done" means for the whole job.

## What I found in the code
The parts of the codebase this touches, how they fit together, conventions the agents must follow
(test framework, style, error handling), anything surprising. Links as `path/file.ext:line`.

## Tasks

| # | Task | Issue | Files | Deps | Where | Model · effort | Why this model |
|---|------|-------|-------|------|-------|----------------|----------------|
| 1 | Parse uppercase emails | #12 | `src/auth/email.ts` | – | cloud | Sonnet 5.5 · low | small, fully tested |
| 2 | Session refresh on login | #13 | `src/auth/session.ts`, `src/api/login.ts` | 1 | cloud | Opus 5.5 · medium | touches token flow |
| 3 | Seed test DB fixture | #14 | `scripts/seed.ts` | – | local | Sonnet 5.5 · medium | needs the local Postgres |

## Waves
```text
wave 1:  #1  #3          (no deps, in parallel)
wave 2:  #2              (after #1 is merged)
final:   PR job/<name> -> main
```

## Test strategy
What the acceptance tests check per task, how to run one task's tests alone, what the full suite
covers, and anything the tests can't check (that you will review by hand).

## Risks
- Shared files between tasks and how the deps avoid conflicts.
- Tasks most likely to need a stronger model or the user's input.

## Out of scope
What this job deliberately does not do (new issues later, if wanted).

## Log
- <date time> plan approved by the user
- <date time> #12 merged after 1 fix round; #13 escalated to Opus high after 2 failures
```
