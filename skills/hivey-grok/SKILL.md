---
name: hivey-grok
description: Run tasks with xAI's Grok Build CLI (`grok`, installed at ~/.grok/bin/grok) headless through a bundled uv Python runner — research questions (web search, "what's X/Twitter saying about…", trend scans with source links), quick answers, structured JSON extraction, or coding work in a folder — then check the result and report cost. Also manages grok itself: models, sessions, resuming, login, usage limits, updates, config and docs. Use whenever the user says "grok", "ask Grok", "with Grok", "grok -p", wants a Grok/xAI second opinion, or wants a Grok-powered scan of X/Twitter discussion, even if they don't say "CLI". Not for the grok.com web app in Chrome (use grok-research / deep-research-duo) and not for calling the xAI API from code.
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# grok: run tasks with the Grok Build CLI

`grok` is xAI's agentic CLI (like Claude Code): it can search/fetch the web, read and edit files,
run shell commands and spawn subagents. Headless mode (`-p`) runs one prompt and exits.

Runner: `uv run -q --script ~/.claude/skills/hivey-grok/scripts/grok_run.py` (called `RUN` below).
Stdlib-only; wraps `~/.grok/bin/grok -p … --output-format json`, prints the answer to stdout
and a one-line `[grok] stop=… turns=… session=… cost=$…` summary to stderr.

## Why the runner and not bare `grok -p`

Two things bit us when we ran grok headless directly:
- **Tool calls that need approval are silently cancelled.** The JSON comes back with
  `stopReason: "cancelled"` and a half-finished answer. The first `web_fetch` is enough to
  trigger it. The runner turns this into exit code 3 and prints a hint.
- **Unrestricted, grok explores the machine.** For an "search X for…" prompt it read
  `~/.claude/skills/*`, looked for local CLIs, and tried to drive Chrome through agent-browser.
  That burned 17 turns (~$0.54) and hit the free-tier limit. `--research` stops this: it allows
  WebSearch/WebFetch, denies Bash/Edit/Write/MCP, and adds a rule to cite URLs.
  `--tools` would be the obvious fix, but it needs internal tool IDs and ignores names it doesn't know.

The runner also runs in a scratch `--cwd` (`/tmp/grok-runs`) by default, so grok doesn't index
whatever repo you happen to be in.

## 1. Pick the mode

| Task | Command |
|---|---|
| Research / news / "what's X saying" / trends with links | `RUN --research "<prompt>"` |
| Quick answer, explanation, rewrite | `RUN "<prompt>"` (add `--max-turns 3` to keep it cheap) |
| Structured extraction | `RUN --schema '<json schema or file.json>' "<prompt>"` → prints JSON |
| Code/file work in a project | `RUN --code --cwd <project> "<task>"` (auto-approves edits and shell; only in a folder the user named) |
| Follow-up in the same conversation | `RUN -c --cwd <same dir> "<follow-up>"` or `RUN -r <session-id> "<follow-up>"` |
| Long prompt | write it to a file and use `cat prompt.md \| RUN --research -` |

Useful extras: `--save out.md` (write the answer to a file), `--json` (full envelope with usage),
`-m <model>`, `-e low|medium|high|xhigh|max` (reasoning effort), `--max-turns N` (cost cap),
`--allow '<Rule>'` / `--deny '<Rule>'` (e.g. `--allow 'Bash(git *)'`), `--timeout 900`.

Research runs take 1–4 minutes. Run them with a long Bash timeout (600000 ms) or in the background.

## 2. Write the prompt

Grok does what the prompt literally says, so be specific about:
- **Scope and time window**: "last 7 days", "since 2026-10-08". Grok knows today's date.
- **Sources**: "posts on X (x.com)", "GitHub repos", "Hacker News threads".
- **Output shape**: sections, a table, or "top 10 with one line each".
- **Evidence**: "include the original post URL for every item; skip items you can't link".

Example (the user's own):
```bash
RUN --research --save ~/Desktop/agent-frameworks.md \
  "Search X for the most discussed AI agent frameworks over the last 7 days. Identify emerging trends, recurring complaints, and promising new projects. Include links to the original posts."
```

**About X/Twitter data**: on this account the CLI has no native X-search connector. It sees X
through web search and fetch, and x.com pages often need a login. Expect fewer and less fresh
X links than grok.com's in-app search gives. If the answer comes back thin on actual post URLs,
tell the user, and offer the Chrome-based `grok-research` / `deep-research-duo` skills, which use
grok.com directly.

## 3. Check and report

- Exit 0: show the answer, or summarise it if it's long and was saved with `--save`. Include
  the cost/turns line, and mention the session id when a follow-up is likely.
- Spot-check that links are real `x.com/<user>/status/<id>` (or the asked-for source) URLs, not
  invented or generic search URLs. Mention any that look made up.
- Exit codes: `1` error (message printed) · `2` **usage limit**: the free Grok Build quota is
  exhausted, so tell the user to wait or upgrade to SuperGrok; don't retry in a loop · `3` cancelled
  by a permission prompt, so rerun with the right mode or `--allow` · `4` empty answer ·
  `124` timeout.

## 4. Managing grok itself

These don't need the runner. They're plain `grok` subcommands:

| Need | Command |
|---|---|
| Version / update | `grok --version` · `grok update` |
| Auth status / login | `grok models` (first line shows login) · `grok login` (browser) · `grok login --device-auth` · or `XAI_API_KEY` env |
| Models | `RUN --models` |
| Sessions | `RUN --sessions` (per cwd) · `grok sessions search <q>` · `grok export <id>` (transcript as Markdown, good for seeing which tools it used) |
| Token/cost of a session | `grok usage <session-id>` |
| Config in effect | `grok inspect` · file: `~/.grok/config.toml` |
| MCP servers / plugins / memory | `grok mcp …` · `grok plugin …` · `grok memory …` |
| Health check | `grok doctor` · `grok du` (disk use of ~/.grok) |
| Interactive TUI | `grok` or `grok "<initial prompt>"`. Can't run inside Claude Code's Bash; tell the user to run it with `! grok` or in their own terminal |

Full docs ship with the CLI in `~/.grok/docs/user-guide/`. Read only the file you need:
`14-headless-mode.md` (flags, JSON format, exit codes), `22-permissions-and-safety.md`
(allow/deny rule syntax), `17-sessions.md`, `07-mcp-servers.md`, `26-config-reference.md`,
`02-authentication.md`.
