---
name: hivey-agy
description: Run a task with Google's Antigravity CLI (`agy`, the replacement for Gemini CLI). Picks the right model and effort for the task (Gemini 3.8 Flash low/medium/high, Gemini 3.1 Pro, or Claude / gpt-oss through agy), writes an effective Gemini-style prompt, runs agy headless through a bundled uv Python runner, and checks the result, including any files the task should create. Use it whenever the user wants something done "with agy", "with Antigravity", "with Gemini", "ask Gemini", wants a Gemini second opinion, or wants to hand a coding, research, summarising, extraction or image-generation task to agy. Also use it when they ask which agy model to use or how to prompt Gemini, even if they don't say "agy". Not for driving the Antigravity IDE app, and not for calling the Gemini API from code.
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# agy: run a task with the right model and prompt

The user gives a task. Your job is to (1) choose the agy model, (2) write the prompt agy gets,
(3) run it headless, (4) check the result and report. agy is a full agent: it can read and
write files in its workspace, run shell commands, search the web, browse, and generate images.

Runner: `uv run -q --script ~/.claude/skills/hivey-agy/scripts/agy_run.py` (called `RUN` below).
It's stdlib-only and wraps `~/.local/bin/agy`. Call agy by full path or through `RUN`: some
shells still carry an old `alias agy="agy . …"` that breaks bare `agy`.

## 1. Understand the task

Work out four things. Usually you can infer them, so don't interrogate the user:
- **Kind**: quick answer · structured extraction · summary/writing · research (needs web) ·
  coding/file work in a folder · image · multi-turn conversation.
- **Workspace**: which folder agy should work in (`--cwd`). For pure Q&A, use a scratch dir
  like `/tmp/agy-<slug>` so agy doesn't wander around in your current folder.
- **Done-when**: what has to be true at the end (a file exists, tests pass, a JSON shape, an answer).
- **Model override**: if the user named a model or tier, use it.

Ask the user only when the missing piece would change what you run, such as which repo or what
the output file should be. Otherwise assume, and mention the assumption in your report.

## 2. Pick the model

Gemini slugs already encode effort (`-low/-medium/-high`), so don't add `--effort` for them.
Check what's currently available with `RUN --models`. The list changes as Google ships models.

| Task | Model | Why |
|---|---|---|
| One-line answers, reformatting, classification, `--schema` extraction | `gemini-3.8-flash-low` | Fast and cheap; reasoning isn't the bottleneck |
| Default: everyday coding, summaries, writing, explanations | `gemini-3.8-flash-medium` | Best speed/quality balance |
| Hard debugging, multi-file refactors, long agent runs, web research, planning | `gemini-3.8-flash-high` | 3.8 Flash high matches or beats 3.1 Pro on most coding/agent benchmarks |
| Very long documents (≫100k tokens), deep reasoning where flash-high already failed, a second opinion | `gemini-3.1-pro-high` | Different model family; use it as the escalation tier |
| User wants Claude through agy, or careful prose / code review | `claude-sonnet-4-6`, or `claude-opus-4-6-thinking` for the hardest | Available in agy at no extra setup |
| Cheap open-model second opinion | `gpt-oss-120b-medium` | Independent check |

Older 3.7/3.6 Flash slugs exist only for comparison. Don't pick them by default.
If a run fails or gives a shallow result, escalate one step (low → medium → high → pro-high)
instead of rewriting the prompt into a monster.

## 3. Write the prompt

Read `references/gemini-prompting.md` the first time you use this skill in a session. In short:
- **Direct, compact, structured.** Role, constraints and output format go at the top, in one
  style (XML tags *or* Markdown headers). No hype, no "think step by step", no threats. The
  `-high` tier already does the reasoning.
- **Long pasted data goes in the middle, and the question at the end**, starting with "Based on
  the information above…".
- **Spell out output size and shape.** Gemini defaults to terse, so ask for length, sections
  or a full file if you want them.
- **Give it a done-when and a report line** for agentic work. Headless agy can't ask questions,
  so add "if unclear, assume reasonably and state the assumption".
- **Phrase rules positively.** Absolute bans ("never infer") make Gemini over-literal.
- **Use the user's own words for the goal**, and add the context they didn't think to give
  (paths, versions, the repo's test command).

Use the templates in the reference: generic, long-context, agentic coding, and the task-type
patterns (research, extraction, writing, debugging, image).

## 4. Run it

```bash
RUN=~/.claude/skills/hivey-agy/scripts/agy_run.py
uv run -q --script $RUN -m <model> [flags] "<prompt>"          # or: printf '%s' "$P" | … -
```

Flags to choose:
- `--cwd <dir>` is the workspace agy reads and writes. Always set it for file work.
- `--schema '<json schema>'` returns parsed JSON for extraction. Prefer it over describing
  JSON in prose.
- `--yolo` (= `--dangerously-skip-permissions`) is needed whenever agy must **run shell
  commands, read web pages (research), or create images**. Headless agy soft-denies those, and
  the run can still "succeed" with an empty or half-done answer. The runner prints
  `[DENIED] …` when that happens. Use `--yolo` only in the task's own folder (a scratch dir for
  research). For untrusted code, add `--sandbox`.
- `--expect <file>` (repeatable) makes the run exit 3 if the file wasn't created. Use it for
  every task that should produce a file. agy can report SUCCESS after a tool failed upstream.
- `--timeout 20m` for long agent runs (the default is 10m).
- `--json` returns the full envelope (`conversation_id`, `usage`). Keep the id when a
  follow-up is likely, then continue with `--conversation <id>` (or `-c` for the last one).
- `--chat` is an interactive multi-turn session. Use it only when the user wants to talk to
  agy themselves.

For long prompts, write the prompt to a temp file and pipe it in (`… - < /tmp/agy-prompt.md`)
so shell quoting can't mangle it.

Runs can take minutes. For anything you expect to exceed ~2 minutes, run it in the background
and tell the user it's running.

## 5. Check and report

- **Exit codes:** 0 = success. 1 = agy error (read stderr: auth, unknown model, timeout).
  3 = an expected file is missing. 4 = agy finished with an empty answer, usually because an
  action was denied. Rerun with `--yolo`.
- **Look at the output yourself:** open the created file and run the tests it claimed to pass.
  For images, use Read on the image file to look at it. Don't relay "done" from agy unchecked.
- **Image fallback:** if agy's image generation keeps failing (it returned upstream 500s on
  2026-10-11), offer the user the `gemini-image-chrome` skill (Gemini in Chrome) as a fallback.
- **Report briefly:** which model and why (one line), the prompt you sent (collapsed or short
  if it's long), the result or file paths, anything you verified, and any assumption you made.

## Example

User: "with agy, summarise ~/Downloads/report.pdf for my team in 5 bullets"
- Kind: summary. Model: `gemini-3.8-flash-medium`. Workspace: `~/Downloads`.
- Prompt:
  ```
  <role>You summarise business documents for a busy engineering team.</role>
  <task>Read report.pdf in the current folder and summarise it.</task>
  <output_format>Exactly 5 Markdown bullets, each ≤25 words, keep key numbers and decisions.
  Then one line "Open questions:" listing anything the report leaves unresolved.</output_format>
  ```
- Run: `uv run -q --script $RUN -m gemini-3.8-flash-medium --cwd ~/Downloads "<prompt>"`
