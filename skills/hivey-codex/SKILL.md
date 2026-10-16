---
name: hivey-codex
description: >
  Run a task with OpenAI's Codex CLI (`codex`) headless from the shell with `codex exec`: coding
  work in a folder, code review, research, quick answers or structured JSON extraction, with the
  right model, sandbox and output options, then check the result. Also resumes or forks earlier
  Codex sessions. Use whenever the user wants something done "with Codex", "ask Codex", "codex
  exec", wants a Codex / GPT second opinion or review, or hands a task to Codex, even if they
  don't say "CLI". Not for the ChatGPT app or for calling the OpenAI API from code.
allowed-tools:
  - Bash
  - Read
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey codex: run Codex headless

`codex exec` runs one Codex task without the interactive UI and exits when it's done: the
final message goes to stdout (progress to stderr). Check `codex --version`; install with
`bun install -g @openai/codex` (or npm), sign in once with `codex login`.

## Run a task

```bash
codex exec -C <folder> -s workspace-write -o /tmp/codex-answer.md "<prompt>"
```

- `-C, --cd <DIR>`: the folder Codex works in (its root). `--add-dir <DIR>` lets it write more.
- `-s, --sandbox`: `read-only` (answers, reviews, research), `workspace-write` (edits in the
  folder; the usual choice for coding), `danger-full-access` (only when asked).
- `--approve-for-me`: approvals reviewed automatically, with `workspace-write`.
  `--dangerously-bypass-approvals-and-sandbox` only in a sandboxed environment the user set up.
- `-m, --model <MODEL>`: leave it out to use the user's configured model; set it when they
  ask for one. Effort: `-c model_reasoning_effort="low|medium|high|xhigh"` (low for quick
  answers, high for hard coding or review).
- `-o, --output-last-message <FILE>`: also write the final message to a file; read it after.
- `--json`: one JSON event per line on stdout (to follow tool calls and the session id).
- `--output-schema <FILE>`: the final answer must match this JSON Schema (extraction).
- `-i, --image <FILE>`: attach images. `--skip-git-repo-check`: run outside a git repo.
- `--ephemeral`: don't keep the session. `-p, --profile <name>`: a config profile.
- The prompt can come on stdin: `cat brief.md | codex exec -`.

## Review, resume, fork

```bash
codex exec review                          # review the current repository's changes
codex exec resume --last "<follow-up>"     # continue the latest session in this folder
codex exec resume <SESSION_ID> "<follow-up>"
codex exec fork <SESSION_ID> "<prompt>"     # a new session from an earlier one
```

## Writing the prompt

Say the goal, the files or folder, constraints (don't touch X, keep the API), and what "done"
means (tests pass, a file exists). Ask for a short summary of what changed at the end. For
long briefs, write them to a file and pipe it in.

## Check the result

Never pass Codex's answer on unchecked: read the `-o` file, look at `git diff` (or the files it
should have made), run the tests or the check the task named, and report what Codex did, what
you verified and anything that failed. On a non-zero exit, show the error (stderr) and the
likely cause (not signed in, sandbox refused a write, no git repo).
