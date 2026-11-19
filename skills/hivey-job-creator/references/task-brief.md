# Task brief template (`~/.hivey/jobs/<name>/tasks/<k>-<slug>.md` = the issue body)

The agent starts with an empty context in a fresh clone of the job branch. This brief is all it
knows about the job, so write what a strong engineer new to the codebase needs to do the task
right the first time. Be concrete: real paths, real function names, the exact test command.
Aim for one screen to two screens; longer means the task should be split.

```markdown
## Goal
<What to build or fix, and why it matters to the job, in two or three sentences.>

## Context
- Read first: `src/auth/email.ts` (`normalizeEmail`, line ~40), `src/api/login.ts` (where it is called).
- How it works today: <one paragraph>.
- Conventions: <test framework, error type to use, logging, naming> — follow the surrounding code.

## Approach
<The approach you'd take and why, in a few bullets. The agent may choose differently if it finds
a better way, but must say why in its PR.>

## Edge cases
- <empty input> → <expected>
- <unicode / very long / concurrent / missing config> → <expected>

## Acceptance tests (written by the coordinator — make them pass, never edit, skip or weaken them)
- `tests/job_<name>/test_email_case.py` — <what it checks>
- Run just these: `<command>`
- Full suite before pushing: `<test command>`

## Scope
- Change: `<paths>`.
- Don't touch: `<paths or areas>` (other agents are working there / out of scope).
- No unrelated refactors, renames or formatting changes.

## Done when
- [ ] The acceptance tests above pass, and the full suite passes.
- [ ] New code has its own unit tests where the acceptance tests don't reach.
- [ ] The PR (into `job/<name>`) says what changed, why, and the test output, and carries the
      line `hivey-job:<name>#<issue>`.

Depends on: #<n> (merged into the job branch before this starts) — or: nothing.
Part of #<parent> (job plan).
```
