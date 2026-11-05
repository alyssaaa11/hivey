---
name: hivey-agent-map
description: "Map everything that runs on this Mac: scheduled jobs (cron, cron-catchup, launchd), hivey agents and swarms, headless claude/codex/grok/agy runs, with tokens and estimated cost per agent, model and provider (Claude Code, DeepSeek/OpenRouter, Codex, Grok, Anthropic API, Gemini), daily cost trends, failing jobs, what is running now, and how the agents feed each other (which job writes the Baserow table or folder another agent reads); drawn as an archify graph plus a one-page stats explainer. Also the spend log: hivey_spend.py records the tokens and cost of LLM API calls that scripts make directly. Use whenever the user asks what runs on their machine, which agents or jobs exist, what is running now, how much each agent or provider costs or how many tokens it used, cost or token trends, why a scheduled job failed, how agents are connected or feed each other, wants an agent map, architecture graph or dependency diagram of their automations, or wants to track or log the spend of their scripts and agents, even if they don't say hivey."
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey agent map

One scan collects the facts; you confirm the links and draw them.

```bash
M=~/.claude/skills/hivey-agent-map/scripts
python3 -I $M/agent_map.py stats --days 7     # summary in the terminal + snapshot JSON
python3 -I $M/agent_map.py scan --days 30     # snapshot only: ~/.hivey/agent-map/snapshot.json
```

It reads, never changes: the crontab, `~/.config/cron-catchup/jobs.tsv` (and `cron-catchup.py list`
status), `~/Library/LaunchAgents` (non-Apple; `launchctl list` exit codes), each job's log (last
error line), `hivey agent list` plus stray claude/codex/grok/agy processes, and token use from
Claude Code transcripts (`~/.claude/projects`, which also hold the DeepSeek/OpenRouter and local Muse
agents hivey runs through Claude Code), Codex sessions, Grok `usage.json`, the claude-api-guard
ledger and the hivey spend log. It runs in about 2 s.

## Snapshot fields

- `jobs[]`: `name, kind (cron | cron (cron-catchup) | launchd), schedule, command, agent, status,
  last_run, log, log_modified, last_error`. `agent` is the agent folder the job runs in, if any.
- `live_agents[]`: `agent, cli, status (working/idle/blocked/done/running), where (hivey pane or pid,
  headless), cwd, role`.
- `usage`: `agents[]` (tokens, `cost_usd`, calls, models, providers; sorted by cost),
  `providers{}`, `models{}`, `kinds{}` (interactive vs headless vs api), `daily_cost_by_provider{}`,
  totals, `sources_without_tokens`.
- `nodes[]`: every job and agent folder (`~/agents/*`, `~/swarms/*`), with `llm_api`
  (`providers` it calls, the `unlogged` ones, `files`).
- `links_candidates.belongs_to[]`: jobs that run inside an agent's folder.
- `links_candidates.shared_stores[]`: a Baserow table (`baserow:table/<id>`) or data folder
  (`dir:~/X/posts`) named by two or more nodes, its `owner` (the node whose folder holds it) and the
  file that names it for each node (`evidence`).
- `links_confirmed[]`: links already confirmed (`~/.hivey/agent-map/links.json`).

Agent names come from folders: `~/agents/<n>` → `n`, `~/swarms/<n>` → `swarm:n`, `~/.hivey/agent` →
`hivey`, `~/projects/<n>` → `project:n`, `~/.claude/skills/<n>` → `skill:n`. Rename or group with
`~/.hivey/agent-map/agents.json` (`{"/path/prefix": "name"}`).

## Workflow

1. **Scan.** Run `stats` (default 7 days; Claude Code transcripts are purged after 15 days, so a
   longer window undercounts Claude Code). Tell the user the headline: total estimated cost, top
   agents, failing jobs, what runs now.
2. **Confirm the links.** Candidates are heuristic: a shared table or folder only shows that two
   nodes name it. For each `shared_stores` entry, read the evidence files (just enough lines) to
   decide who **writes** and who **reads** it, then save each real edge:
   `python3 -I $M/agent_map.py link <from-id> <to-id> --via "Baserow Tweets table" --evidence "<file:line>"`
   (node ids as in `nodes[]`, e.g. `cron:x-ai-agents-trends` → `agent:x-content`). Drop
   coincidences (a backup swarm, a path in a comment). Also add obvious edges the heuristic cannot
   see when the evidence is clear: a job that runs a skill (`claude -p "/github-trending"`), an
   agent brief that says it reads another agent's output. Ask the user only when the direction is
   really unclear. Confirmed links are reused next time, so later maps are fast.
3. **Draw** (archify, below).
4. **Offer the spend gaps.** For each node with `llm_api.unlogged`, offer to add one
   `hivey_spend.record(...)` line after its API call (see Spend log). These are the user's own
   scripts: show the change and get a yes per script; never touch them unasked.

## Drawing with archify

Needs the `archify` skill (`~/.claude/skills/archify`). Put both artifacts in
`~/.hivey/agent-map/reports/<YYYY-MM-DD-HHMM>/`, following archify's own SKILL.md (read it first:
schemas, `finalize`, repair rules).

- **The map: a `dataflow` diagram** (`map.json` → `agent-map.html`). The grid is at most 5 stages ×
  rows 0–4, so **25 nodes at most**. A layout that works: stages *Scheduled jobs* → *Raw data* (the
  stores jobs write: transcripts, Baserow tables) → *Pipelines* (jobs that read them) → *Knowledge &
  agents* (vaults/wikis, upstream agents) → *Agents* (the consumers), one row per domain (YouTube/wikis,
  X content, radars, trading, research swarms). Draw only confirmed links, labelled with what flows
  (≤ 17 characters: "top-20 tweets", "transcripts"). Fold the rest: a job that belongs to an agent
  merges into it ("+ nightly 03:15" in its sublabel), many similar jobs become one node ("12 trading
  jobs"), and unlinked housekeeping (git auto-commits, upgrades, purges) goes in a "Not drawn" card.
  **Node tags do not show at default zoom: put cost and status in the sublabel** ("$110 · posts → n8n",
  "launchd 04:30 · exit 1"). Put an agent that shares no data at a free cell where no line crosses it.
  Providers and totals go in cards (spend by provider and model, needs attention, spend gaps, not
  drawn), not as edges: agent → provider edges cross everything.
- **The stats: an `explainer` page** (`stats.archify.md` → `agent-stats.html`; conclusion first): total
  cost and tokens with the API-equivalent note in scope; cost by agent (`@chart type=bar` over an
  `agents.csv` you write from the snapshot, plus a table with calls, tokens and model); why the top
  spender costs what it does (`@facts`: cache reads, cache writes, output, fresh input, from its
  `usage.agents[]` entry and the prices); daily cost (`@chart type=line` over `daily.csv`); job health
  table (failing jobs and their last error); spend gaps (`@callout`) and a `- [ ]` checklist. Copy
  `snapshot.json` into the folder and cite it as `@source scan … path=snapshot.json`. **Do not embed
  the map** with `@diagram`: a map this dense fails the page's 10px readability check. Instead,
  describe the main links in a list and name `agent-map.html` (it sits in the same folder).
- Finalize each one: `finalize dataflow map.json agent-map.html --quality showcase --json` and
  `finalize explainer stats.archify.md agent-stats.html --json`. After a run that reached
  browser-check, rerun with a fresh `--out-dir review-<n>`. Recompute every number you quote from the
  snapshot (sums, shares, job counts by kind).

Lead your reply with links to both files and two or three findings (biggest spender, a failing job, a
surprising link). If archify is missing, give the stats as Markdown tables and the map as a Mermaid
`flowchart LR`, and say archify would render it properly.

## Nightly report, menu and schedule

```bash
hivey agent-map open                      # the latest report; builds the first one when none exists
hivey agent-map nightly [--model haiku]   # a full report now (also: --no-llm, --open)
hivey agent-map schedule install [--at 06:30] [--model haiku]   # | remove | status
```

- **hivey menu → agent map** runs `hivey agent-map open`: it opens `agent-stats.html`,
  `agent-map.html` and `agent-logs.html` from `~/.hivey/agent-map/reports/latest`.
- **hivey's layer on the archify pages** (`enhance.py`, after finalize; `agent_map.py enhance [folder]`
  re-applies it): a Stats · Map · Logs navbar on every page; map boxes coloured by spend (green: none →
  red: the most, log scale, with a legend); a hover explanation on each column header; and in each box's
  info card what it does, spend, tokens, calls, models and last run. Boxes are tied to snapshot nodes by
  `map-meta.json` (`{"nodes": {"<box id>": {"refs": [...], "description": "…"}}, "stages": {...}}`,
  written by the nightly model), else by label. A job counts only its own spend, not its agent's.
- **Descriptions** ("what it does"): from a brief's mission, a skill's description or a script's module
  docstring; the nightly model writes the missing or weak ones to `~/.hivey/agent-map/descriptions.json`
  (kept, edit freely), which wins over the extracted ones. They show in the map's info cards and the run log.
- **Automated vs interactive:** the map, the stats page and map colours count AUTOMATED work only
  (scheduled jobs, scripts, headless `claude -p` / `codex exec`, scripts' API calls; snapshot `usage`).
  People chatting with agents (Claude Code, Codex, Grok in a terminal; snapshot `interactive`, with one
  entry per session) go to **`agent-sessions.html`**: cost by agent and by day, the costliest sessions with
  their peak context (⚠️ over 300k tokens), length and first message, and tips to spend less.
- **Models per job:** the scan reads each job's code for the models it uses, local ones included
  (`nodes[].llm_api.models`: JEV-9B local or Jev on OpenRouter, Claude Opus/Sonnet/Haiku, Grok, DeepSeek,
  Gemini, OpenAI/Codex, Muse, Whisper, Parakeet); the map's info card lists them next to recorded spend.
- **Per day:** the stats page charts the top agents' cost per day and lists each day's cost, tokens and
  top agents (`usage.daily_by_agent` in the snapshot).
- **`agent-logs.html`** (run log, written by code): every scheduled job with ✅ ran ok · ⚠️ errors in
  its log or a missed run (a launchd log older than its schedule allows) · ❌ non-zero exit or not
  loaded · ⏳ running · ❔ no log, its last run, the issue, and on click the error lines and last 40
  log lines; then every agent with 🟢 working · 🔴 blocked · ⚪ idle · ✅ done · ⚫ not running, last
  active, cost, and its newest `logs/` file. Dormant agents fold into one row. When the user asks
  why a job or agent is not working, read its entry in `snapshot.json` (`jobs[].errors`, `tail`,
  `agent_logs`) or open this page, then the full log it names.
- **`nightly`** (`report.py`): scan → stats page written and finalized by code (no LLM) → last night's
  `map.json` copied in → one `claude -p --model haiku` run (in the report folder, limited tools) that
  confirms new links and updates the map → if the model fails or breaks the map, last night's map is
  rendered unchanged. `reports/latest` moves only when the stats page passed; the 30 newest reports
  are kept. About 2 minutes and $0.03 with Haiku. Log: `nightly.log` and `llm.log` in the report.
- **`schedule install`** writes the LaunchAgent `com.hivey.agent-map` (launchd, not cron: `claude -p`
  needs the login keychain; a run missed during sleep happens on wake). Its log is
  `~/.hivey/agent-map/nightly.log`. Unattended runs need a long-lived Claude login: the user runs
  `claude setup-token`, stores the token in their secret store (`envsave set CLAUDE_CODE_OAUTH_TOKEN`)
  and points the run at it: `hivey agent-map schedule token --command "envsave get es_…"`. Only the
  command is kept (`config.json`); the token is read each night and passed to `claude -p` as
  `CLAUDE_CODE_OAUTH_TOKEN`, never written to a file. Without it, the normal login is used.
- **`schedule offer`** asks once, at the first install (`install.sh`) or the first `hivey update` that
  has it, whether to set the schedule up; the answer is kept in `~/.hivey/agent-map/offer.json`, so it
  is never asked again. Without a person at a terminal it asks nothing.
- When you are the nightly model: follow the prompt you were given, change only `map.json`,
  `agent-map.html` and `links.json` (through `link`), and stop after the map passes.

## Spend log (`hivey_spend.py`)

Calls made through Claude Code, Codex and Grok CLI are already counted from their own transcripts, and
claude-api-guard logs the Anthropic API calls that go through it. Everything else (scripts calling
OpenAI, OpenRouter/DeepSeek, xAI/Grok, Gemini, Anthropic without the guard, a local server) logs one
line per call to `~/.hivey/spend/ledger.jsonl` with the core function:

```python
import os, sys; sys.path.insert(0, os.path.expanduser("~/.claude/skills/hivey-agent-map/scripts"))
import hivey_spend
resp = client.chat.completions.create(...)      # an SDK object, a dict, or a requests/httpx response
hivey_spend.record(resp, agent="x-ai-agents-trends")
```

It reads the usage block of Anthropic Messages, OpenAI Chat Completions and Responses (also
OpenRouter, xAI, DeepSeek and other compatible APIs, with their reported `cost` when present) and
Gemini `usageMetadata`, estimates the cost when none is reported, and never raises. Other entry
points: `hivey_spend.record_usage(agent=, provider=, model=, input=, output=, cache_read=, cache_write=,
cost_usd=)`, a shell pipe that echoes the response unchanged
(`curl … | python3 $M/hivey_spend.py --agent my-job`), and
`python3 -I $M/agent_map.py log --agent N --provider P --model M --in N --out N [--cost USD]`.
`agent` defaults to `$HIVEY_AGENT`, else the agent of the current folder. For a streamed call, pass
the final message (or the last chunk carrying usage).

## Costs and limits

- Costs are list-price estimates (`hivey_spend.PRICES`; override in `~/.hivey/agent-map/prices.json`
  as `{"model-substring": [in, out, cache_read, cache_write]}` per million tokens). Claude Code on a
  Max plan and Codex on Plus are not billed per token: call their cost "API-equivalent". Grok CLI and
  OpenRouter report real cost, used as is.
- Antigravity (`agy`) keeps no token data on disk: it shows as running, not as spend.
- Codex sessions are zstd-compressed; without `zstd` on PATH they are skipped.
- Read-only: the scan never edits jobs, plists or scripts.
