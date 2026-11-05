"""hivey agent map reports: the nightly run, opening the latest report, and its launchd schedule.

Used by agent_map.py (`nightly`, `open`, `schedule`). A report is a folder
~/.hivey/agent-map/reports/<YYYY-MM-DD-HHMM>/ with:
  agent-stats.html   one-page stats (archify explainer), generated here without an LLM
  agent-map.html     the jobs → data → agents map (archify dataflow), updated by a cheap model
                     (Claude Haiku by default) from the previous night's map.json
  snapshot.json, agents.csv, daily.csv, stats.archify.md, map.json, nightly.log
`reports/latest` points at the newest complete report. Python standard library only.
"""
from __future__ import annotations

import csv
import datetime as dt
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
from pathlib import Path

HOME = Path.home()
STATE = HOME / ".hivey" / "agent-map"
REPORTS = STATE / "reports"
LATEST = REPORTS / "latest"
SCRIPTS = Path(__file__).resolve().parent
LABEL = "com.hivey.agent-map"
PLIST = HOME / "Library" / "LaunchAgents" / f"{LABEL}.plist"
KEEP_REPORTS = 30
LAUNCH_PATH = ":".join(str(p) for p in (
    HOME / ".local/bin", HOME / ".bun/bin", HOME / ".nvm/current/bin", Path("/opt/homebrew/bin"),
    Path("/usr/local/bin"), Path("/usr/bin"), Path("/bin")))


def archify_bin() -> Path | None:
    for p in (HOME / ".claude/skills/archify/bin/archify.mjs", HOME / ".agents/skills/archify/bin/archify.mjs"):
        if p.is_file():
            return p
    return None


def log(folder: Path, msg: str) -> None:
    line = f"[{dt.datetime.now():%Y-%m-%d %H:%M:%S}] {msg}"
    print(line, flush=True)
    with open(folder / "nightly.log", "a") as fh:
        fh.write(line + "\n")


# ---------------------------------------------------------------- stats page (no LLM)

def money(v: float) -> str:
    return f"${v:,.2f}"


def mtok(n: int) -> str:
    return f"{n / 1e6:,.0f}M" if n >= 1e6 else f"{n / 1e3:,.0f}k"


def cell(text) -> str:
    """Table-cell / prose-safe text: names with paths or symbols go in backticks."""
    s = str(text).replace("`", "'").replace("|", "/").strip()
    return f"`{s}`" if re.search(r"[<>*_~/\\\[\]{}#@]", s) else s


def node_label(node_id: str) -> str:
    return re.sub(r"^(cron:|launchd:(com\.jcsancho\.|com\.hivey\.)?|agent:)", "", node_id)


def model_name(m: str) -> str:
    m = m.rsplit("/", 1)[-1]
    k = re.match(r"claude-(opus|sonnet|haiku)-(\d+)(?:-(\d{1,2}))?(?!\d)", m)
    if not k:
        return m
    return f"{k.group(1).title()} {k.group(2)}" + (f".{k.group(3)}" if k.group(3) else "")


def write_stats(snap: dict, folder: Path) -> None:
    from hivey_spend import price_for  # one price table

    u = snap["usage"]
    days = snap["window_days"]
    total = u["total_cost_usd"] or 0.0
    agents = u["agents"]
    (folder / "snapshot.json").write_text(json.dumps(snap, indent=1, default=str))
    with open(folder / "daily.csv", "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["day", "cost_usd"])
        for day, v in u["daily_cost_by_provider"].items():
            w.writerow([day[5:], round(sum(v.values()), 2)])
    with open(folder / "agents.csv", "w", newline="") as fh:
        w = csv.writer(fh)
        w.writerow(["agent", "cost_usd"])
        for a in agents[:10]:
            w.writerow([a["agent"], round(a["cost_usd"], 2)])

    kinds = u["kinds"]
    interactive = sum(v for k, v in kinds.items() if "interactive" in k)
    headless = sum(v for k, v in kinds.items() if "headless" in k)
    api = total - interactive - headless
    models = sorted(u["models"].items(), key=lambda kv: -kv[1]["cost_usd"])
    jobs = snap["jobs"]
    failing = [j for j in jobs if job_state(j)[0] == "❌"]
    n_cron = sum(1 for j in jobs if j["kind"].startswith("cron"))
    gaps = [n for n in snap["nodes"] if (n.get("llm_api") or {}).get("unlogged")]
    top = agents[0] if agents else None
    pct = lambda v: f"{100 * v / total:.0f}%" if total else "0%"

    L = []
    L.append(f"# What ran on this Mac in the last {days} days, and what did it cost? {{#agent-stats}}\n")
    if top:
        L.append(f"Your agents used about **{money(total)} of API-equivalent tokens** in {days} days; "
                 f"**{cell(top['agent'])} used {money(top['cost_usd'])}** of it ({pct(top['cost_usd'])}).\n")
    else:
        L.append(f"No token use was recorded in the last {days} days.\n")
    L.append(f"- {pct(interactive)} of the cost is interactive Claude Code ({money(interactive)}); "
             f"headless runs cost {money(headless)} and direct API calls {money(api)}.")
    if models:
        L.append("- " + "; ".join(f"{cell(model_name(m))} {money(v['cost_usd'])}" for m, v in models[:3]) + ".")
    states = [job_state(j)[0] for j in jobs]
    tally = ", ".join(f"{emo} {states.count(emo)}" for emo in ("✅", "⚠️", "❌", "⏳", "❔") if states.count(emo))
    L.append(f"- {len(jobs)} scheduled jobs ({n_cron} cron, {len(jobs) - n_cron} launchd): {tally}.")
    L.append(f"- {len(snap['links_confirmed'])} confirmed links between jobs and agents; "
             f"{len(gaps)} with LLM calls not logged yet.\n")
    L.append(f'@answer scope="last {days} days to {snap["generated"][:10]} · costs are list-price estimates · '
             f'Claude Code (Max) and Codex (Plus) are not billed per token"')
    L.append("@meta locale=en")
    L.append('@source scan title="hivey agent map snapshot (agent_map.py scan)" path=snapshot.json\n')

    if agents:
        L.append("## Which agents spend the most? {#by-agent}\n")
        L.append(f'@chart type=bar data=agents.csv x=agent y=cost_usd unit=USD title="Cost by agent, '
                 f'last {days} days (API-equivalent USD, estimated)"\n')
        L.append("| Agent | Cost | Calls | Tokens | Main model |")
        L.append("| --- | --- | --- | --- | --- |")
        for a in agents[:8]:
            tok = a["input"] + a["output"] + a["cache_read"] + a["cache_write"]
            badge = " {bad:" + pct(a["cost_usd"]) + "}" if a is top and total and a["cost_usd"] / total > 0.3 else ""
            main = model_name(a["models"][0]) if a["models"] else "?"
            L.append(f"| {cell(a['agent'])} | {money(a['cost_usd'])}{badge} | {a['calls']:,} | {mtok(tok)} | {cell(main)} |")
        L.append("")

        pi, po, pr, pw = price_for(top["models"][0] if top["models"] else "")
        parts = [("Cache reads", top["cache_read"], pr), ("Cache writes", top["cache_write"], pw),
                 ("Output", top["output"], po), ("Fresh input", top["input"], pi)]
        L.append(f"## Where does {cell(top['agent'])}'s cost come from? {{#top-spender}}\n")
        L.append("@facts")
        for name, n, p in parts:
            L.append(f"| {name} | {mtok(n)} tokens → {money(n * p / 1e6)} |")
        L.append("")
        biggest = max(parts, key=lambda x: x[1] * x[2])[0]
        if biggest.startswith("Cache"):
            L.append("Most of it is context: each turn re-reads the session, so a long session costs more per turn "
                     "as it grows. Clearing or compacting more often, or a cheaper model for routine runs, cuts it.\n")
        else:
            L.append("Most of it is output tokens: long answers, reports or code written by the agent.\n")

    if len(u["daily_cost_by_provider"]) > 1:
        L.append("## How does the cost move day by day? {#daily}\n")
        L.append('@chart type=line data=daily.csv x=day y=cost_usd unit=USD title="Daily cost, all providers '
                 '(API-equivalent USD, estimated; today is partial)"\n')
    per_day = u.get("daily_by_agent", {})
    if per_day:
        top5 = [a["agent"] for a in agents[:5]]
        cols = [re.sub(r"[^\w-]", "_", a) for a in top5]
        with open(folder / "daily_agents.csv", "w", newline="") as fh:
            w = csv.writer(fh)
            w.writerow(["day"] + cols)
            for day, rows in per_day.items():
                by = {r["agent"]: r["cost_usd"] for r in rows}
                w.writerow([day[5:]] + [round(by.get(a, 0.0), 2) for a in top5])
        L.append("## What ran each day? {#per-day}\n")
        L.append(f'@chart type=bar data=daily_agents.csv x=day y={",".join(cols)} unit=USD '
                 f'title="Cost per day of the top {len(top5)} agents (API-equivalent USD, estimated)"\n')
        L.append("| Day | Cost | Tokens | Agents that day (top 3) |")
        L.append("| --- | --- | --- | --- |")
        days_cost = {d: sum(r["cost_usd"] for r in rows) for d, rows in per_day.items()}
        peak = max(days_cost, key=days_cost.get)
        for day, rows in per_day.items():
            cost_d, tok_d = days_cost[day], sum(r["tokens"] for r in rows)
            badge = " {bad:peak}" if day == peak and len(per_day) > 2 else ""
            tops = ", ".join(f"{cell(r['agent'])} {money(r['cost_usd'])}" for r in rows[:3])
            L.append(f"| {dt.date.fromisoformat(day):%a %d %b} | {money(cost_d)}{badge} | {mtok(tok_d)} | "
                     f"{tops} (+{max(0, len(rows) - 3)} more) |")
        L.append("")

    L.append("## How do the agents feed each other? {#map}\n")
    L.append("The full map is `agent-map.html` in this folder.\n")
    links = snap["links_confirmed"]
    if links:
        for l in links[:12]:
            L.append(f"- {cell(node_label(l['from']))} → {cell(node_label(l['to']))}: {cell(l.get('via', ''))}")
        if len(links) > 12:
            L.append(f"- and {len(links) - 12} more links")
    else:
        L.append("- No links confirmed yet: the nightly run confirms them from the shared tables and folders.")
    L.append("")

    L.append("## Which jobs need attention? {#jobs}\n")
    attention = [j for j in jobs if job_state(j)[0] in ("❌", "⚠️", "❔")]
    attention.sort(key=lambda j: ORDER.get(job_state(j)[0], 9))
    if attention:
        L.append("| Job | Schedule | Issue |")
        L.append("| --- | --- | --- |")
        for j in attention[:14]:
            emo, words = job_state(j)
            badge = " {bad:failing}" if emo == "❌" else " {caution:check}"
            err = (j.get("last_error") or words)[:90]
            L.append(f"| {emo} {cell(j['name'])} | {cell(j['kind'].split()[0] + ' ' + j['schedule'])} | {cell(err)}{badge} |")
        L.append("")
    else:
        L.append(f"✅ All {len(jobs)} jobs ran cleanly.\n")
    L.append("Every job and agent, with its status, issues and the last lines of its log: `agent-logs.html` "
             "in this folder (hivey menu → agent map opens it too).\n")

    L.append("## What is not counted yet? {#gaps}\n")
    L.append('@callout level=warning title="Spend the map cannot see"')
    for n in gaps:
        L.append(f"{cell(n['label'])} calls {', '.join(n['llm_api']['unlogged'])} without logging it.")
    for s in u.get("sources_without_tokens", []):
        L.append(cell(s.split(":")[0]) + " keeps no token data on disk.")
    L.append("")
    todo = [f"- [ ] Fix {cell(j['name'])} ({cell((j.get('last_error') or j['status'])[:60])})" for j in failing[:4]]
    todo += [f"- [ ] Log {cell(n['label'])}'s API calls with `hivey_spend.record(...)`" for n in gaps[:4]]
    if todo:
        L.extend(todo)
        L.append("")
    (folder / "stats.archify.md").write_text("\n".join(L))


# ---------------------------------------------------------------- run log (no LLM)

def job_state(j: dict) -> tuple[str, str]:
    """(emoji, words) for one scheduled job's latest run."""
    st = j.get("status", "?")
    if st in ("running", "RUNNING"):
        return "⏳", "running now"
    if st.startswith("exit") or st == "not loaded":
        return "❌", st if st != "not loaded" else "not loaded in launchd"
    if st == "MISSED" or j.get("overdue"):
        return "⚠️", "missed its last run"
    if j.get("errors"):
        return "⚠️", "ran, errors in its log"
    if st == "?" and not j.get("log"):
        return "❔", "no log to check"
    return "✅", "ran ok"


AGENT_STATE = {"working": ("🟢", "working"), "running": ("🟢", "running"), "blocked": ("🔴", "blocked, needs you"),
               "idle": ("⚪", "idle"), "done": ("✅", "done"), "gone": ("⚫", "not running")}
ORDER = {"❌": 0, "⚠️": 1, "🔴": 1, "⏳": 2, "🟢": 2, "❔": 3, "⚪": 4, "✅": 4, "⚫": 5}


def write_logs_page(snap: dict, folder: Path) -> None:
    import html
    e = html.escape
    jobs = sorted(snap["jobs"], key=lambda j: (ORDER.get(job_state(j)[0], 9), j["name"]))
    usage = {a["agent"]: a for a in snap["usage"]["agents"]}
    logs = snap.get("agent_logs", {})
    live: dict[str, dict] = {}
    for a in snap["live_agents"]:
        cur = live.get(a["agent"])
        if not cur or ORDER.get(AGENT_STATE.get(a["status"], ("⚪",))[0], 9) < ORDER.get(AGENT_STATE.get(cur["status"], ("⚪",))[0], 9):
            live[a["agent"]] = a
    names = sorted(set(live) | {n["label"] for n in snap["nodes"] if n["type"] == "agent"}
                   | {a for a in usage if usage[a]["cost_usd"] >= 0.5})

    def details(body: list[str]) -> str:
        return "".join(body)

    def log_block(info: dict) -> list[str]:
        out = []
        if info.get("log"):
            out.append(f"<p class=meta>log <code>{e(info['log'])}</code> · last written {e(info.get('log_modified', '?'))}</p>")
        if info.get("errors"):
            out.append("<p class=meta>issues found in its log:</p><ul class=errs>"
                       + "".join(f"<li><code>{e(x)}</code></li>" for x in info["errors"]) + "</ul>")
        if info.get("tail"):
            out.append(f"<pre>{e(chr(10).join(info['tail']))}</pre>")
        return out

    counts: dict[str, int] = {}
    rows = []
    for j in jobs:
        emo, words = job_state(j)
        counts[emo] = counts.get(emo, 0) + 1
        issue = (j.get("errors") or [""])[-1]
        last = (j.get("last_run") or j.get("log_modified") or "?").replace("T", " ")
        body = [f"<p class=meta>{e(j['kind'])} · {e(j['schedule'])} · agent {e(j.get('agent') or '—')}</p>",
                f"<p class=meta>command <code>{e(j['command'][:400])}</code></p>"] + log_block(j)
        if not j.get("log"):
            body.append("<p class=meta>this job writes no log file hivey can find.</p>")
        rows.append(f"<details class=row data-state='{emo}'><summary><span class=emo>{emo}</span>"
                    f"<span class=name><b>{e(j['name'])}</b><small class=desc>{e(j.get('description') or '')}</small></span>"
                    f"<span class=sub>{e(words)}</span><span class=when>{e(last)}</span>"
                    f"<span class=issue>{e(issue[:110])}</span></summary>{details(body)}</details>")
    arows = []
    dormant = [n for n in names if n not in live and n not in usage and not logs.get(n)]
    names = [n for n in names if n not in dormant]
    for name in sorted(names, key=lambda n: (ORDER.get(AGENT_STATE.get(live.get(n, {}).get("status", "gone"), ("⚫",))[0], 9), n)):
        a = live.get(name)
        emo, words = AGENT_STATE.get(a["status"], ("⚪", a["status"])) if a else ("⚫", "not running")
        info = logs.get(name, {})
        if info.get("errors") and emo in ("⚪", "✅", "⚫"):
            emo, words = "⚠️", words + ", errors in its log"
        u = usage.get(name, {})
        cost = f"${u['cost_usd']:,.2f} in {snap['window_days']} days" if u else "no token use"
        body = [f"<p class=meta>{e(a['where'] if a else 'not open in hivey')} · {e(a.get('cwd', '') if a else '')}"
                f" · last active {e(u.get('last_active') or '?')} · {e(cost)}"
                + (f" · {e(', '.join(u.get('models', [])[:2]))}" if u else "") + "</p>"]
        body += log_block(info) if info else ["<p class=meta>no <code>logs/</code> folder in this agent.</p>"]
        adesc = next((n.get("description", "") for n in snap["nodes"] if n["type"] == "agent" and n["label"] == name), "")
        arows.append(f"<details class=row><summary><span class=emo>{emo}</span>"
                     f"<span class=name><b>{e(name)}</b><small class=desc>{e(adesc)}</small></span>"
                     f"<span class=sub>{e(words)}</span><span class=when>{e((u.get('last_active') or '')[:16].replace('T', ' '))}</span>"
                     f"<span class=issue>{e(cost)}</span></summary>{details(body)}</details>")
    if dormant:
        arows.append(f"<details class=row><summary><span class=emo>⚫</span><b>{len(dormant)} dormant agents</b>"
                     f"<span class=sub>not running, no token use, no logs</span><span class=when></span>"
                     f"<span class=issue>{e(', '.join(dormant[:8]))}…</span></summary>"
                     f"<p class=meta>{e(', '.join(dormant))}</p></details>")
    summary = " · ".join(f"{k} {v}" for k, v in sorted(counts.items(), key=lambda kv: ORDER.get(kv[0], 9)))
    page = f"""<!doctype html><html lang=en><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>Agent logs · {e(snap['generated'][:16])}</title>
<style>
:root{{--bg:#fff;--fg:#1d2330;--mute:#667085;--line:#e4e7ec;--card:#f8f9fb;--accent:#2f6fec;--pre:#f2f4f7}}
@media (prefers-color-scheme:dark){{:root{{--bg:#0d1117;--fg:#e6edf3;--mute:#8b949e;--line:#30363d;--card:#161b22;--accent:#79a8ff;--pre:#0b0f14}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}}
main{{max-width:1180px;margin:0 auto;padding:28px 24px 60px}}
nav a{{color:var(--accent);margin-right:18px;text-decoration:none;font-weight:600}} nav a.on{{color:var(--fg)}}
h1{{font-size:26px;margin:18px 0 4px}} h2{{font-size:19px;margin:34px 0 10px}}
.lead{{color:var(--mute);margin:0 0 6px}} .counts{{font-size:17px;margin:10px 0 0}}
.row{{border:1px solid var(--line);background:var(--card);border-radius:10px;margin:7px 0}}
summary{{display:grid;grid-template-columns:30px minmax(220px,2fr) minmax(120px,.8fr) 130px minmax(0,1.6fr);gap:12px;align-items:center;padding:10px 14px;cursor:pointer;list-style:none}}
summary::-webkit-details-marker{{display:none}}
.emo{{font-size:18px}} .name{{min-width:0}} .name b{{display:block}}
.desc{{display:block;color:var(--mute);font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}} .sub,.when{{color:var(--mute);font-size:13px}} .issue{{font:12.5px ui-monospace,Menlo,monospace;color:var(--mute);white-space:nowrap;overflow:hidden;text-overflow:ellipsis}}
details[open] summary{{border-bottom:1px solid var(--line)}} details>*:not(summary){{margin:10px 16px}}
.meta{{color:var(--mute);font-size:13px}} code{{font:12.5px ui-monospace,Menlo,monospace;word-break:break-all}}
.errs{{margin-top:0}} .errs li{{color:#d1242f}} @media (prefers-color-scheme:dark){{.errs li{{color:#ff7b72}}}}
pre{{background:var(--pre);border:1px solid var(--line);border-radius:8px;padding:12px;max-height:420px;overflow:auto;font:12px/1.45 ui-monospace,Menlo,monospace;white-space:pre-wrap;word-break:break-all}}
.legend{{color:var(--mute);font-size:13px;margin-top:40px}}
@media (max-width:760px){{summary{{grid-template-columns:28px 1fr}} .sub,.when,.issue{{grid-column:2}}}}
</style>
<main>
<nav class=hv-top><a href="agent-stats.html">Stats</a><a href="agent-map.html">Map</a><a class=on href="agent-logs.html">Logs</a></nav>
<h1>Did every job and agent run?</h1>
<p class=lead>Scan of {e(snap['generated'][:16].replace('T', ' '))}. Click a row for its issues and the last lines of its log.</p>
<p class=counts>Scheduled jobs: {summary}</p>
<h2>Scheduled jobs ({len(jobs)})</h2>
{''.join(rows)}
<h2>Agents ({len(names)} active{f", {len(dormant)} dormant" if dormant else ""})</h2>
{''.join(arows)}
<p class=legend>✅ ran ok · ⚠️ errors in its log or a missed run · ❌ failed (non-zero exit) or not loaded · ⏳ running ·
❔ no log · 🟢 working · 🔴 blocked, needs you · ⚪ idle · ⚫ not running. "Issues" are the error lines among the
last 40 lines of each log, so an old error can show until the log moves on.</p>
</main></html>"""
    (folder / "agent-logs.html").write_text(page)


# ---------------------------------------------------------------- archify

def finalize(folder: Path, kind: str, src: str, out: str) -> tuple[bool, str]:
    node, archify = shutil.which("node"), archify_bin()
    if not node or not archify:
        return False, "archify or node not found"
    n = 1
    while (folder / f"review-{kind}-{n}").exists():
        n += 1
    cmd = [node, str(archify), "finalize", kind, src, out, "--json", "--out-dir", f"review-{kind}-{n}"]
    if kind != "explainer":
        cmd[6:6] = ["--quality", "showcase"]
    try:
        p = subprocess.run(cmd, cwd=folder, capture_output=True, text=True, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        return False, str(e)
    try:
        r = json.loads(p.stdout)
        msgs = "; ".join(d.get("message", "")[:160] for d in r.get("diagnostics", [])[:3])
        return r.get("status") == "pass", msgs or r.get("status", "")
    except ValueError:
        return False, (p.stdout + p.stderr)[-300:]


MAP_PROMPT = """You are the nightly run of the hivey agent map. Your working folder is {folder} (your cwd);
use absolute paths and no `cd`. Read ~/.claude/skills/hivey-agent-map/SKILL.md first (Workflow step 2 and
"Drawing with archify" → the map), then archify's SKILL.md for the dataflow rules. The stats page is
already built: do not touch stats.archify.md or agent-stats.html.

1. Links: in snapshot.json compare links_candidates.shared_stores with links_confirmed. For each store not
   covered yet, read only the evidence lines you need. When it is clear who writes and who reads, save it:
   python3 -I {scripts}/agent_map.py link <from-id> <to-id> --via "<what flows>" --evidence "<file:line>"
   Skip anything unclear or coincidental. Then rerun: python3 -I {scripts}/agent_map.py scan --out snapshot.json
2. Map: {map_task}
   Costs and status go in each node's sublabel (from usage.agents and jobs in snapshot.json); keep at most
   25 nodes on 5 stages x rows 0-4; refresh the numbers in the cards.
3. Descriptions: every node in snapshot.json has a "description" (what it does). For each job and agent
   whose description is empty or does not say what it does (an instruction to the agent, "You are X"),
   read its brief or script (first lines only) and write ONE plain sentence of at most 18 words into
   ~/.hivey/agent-map/descriptions.json ({{"<node id>": "<sentence>"}}; keep the entries already there).
   Then write map-meta.json in this folder: {{"nodes": {{"<map node id>": {{"refs": [<snapshot node ids
   this box stands for>], "description": "<one sentence>"}}}}, "stages": {{"<stage label>": "<what this
   column holds, one sentence>"}}}}. A box that folds several jobs lists all of them in refs.
4. Run: node {archify} finalize dataflow map.json agent-map.html --quality showcase --json --out-dir review-map-llm-1
   If it fails, repair map.json and rerun with review-map-llm-2, -3 ... (at most 4 runs).
Stop there and reply with one line: what changed."""


CONFIG = STATE / "config.json"  # {"claude_token_command": "envsave get es_…"}: never the token itself


def claude_token(folder: Path) -> str | None:
    """A long-lived Claude login (`claude setup-token`) for unattended runs, read at run time by the
    configured command so it is never written to a file. None: Claude Code's normal login."""
    try:
        command = json.loads(CONFIG.read_text()).get("claude_token_command")
    except (OSError, ValueError):
        return None
    if not command:
        return None
    try:
        p = subprocess.run(["/bin/bash", "-c", command], capture_output=True, text=True, timeout=60,
                           env={**os.environ, "PATH": LAUNCH_PATH})
    except (OSError, subprocess.SubprocessError) as e:
        log(folder, f"claude token command failed: {e}; using the normal login")
        return None
    token = p.stdout.strip()
    if p.returncode != 0 or not token:
        log(folder, f"claude token command failed (exit {p.returncode}); using the normal login")
        return None
    return token


def set_token_command(command: str) -> int:
    STATE.mkdir(parents=True, exist_ok=True)
    try:
        cfg = json.loads(CONFIG.read_text())
    except (OSError, ValueError):
        cfg = {}
    if command:
        cfg["claude_token_command"] = command
    else:
        cfg.pop("claude_token_command", None)
    CONFIG.write_text(json.dumps(cfg, indent=1))
    print(f"nightly Claude login: {'token from: ' + command if command else 'the normal claude login'} ({CONFIG})")
    return 0


def run_llm(folder: Path, model: str, has_map: bool) -> bool:
    claude = shutil.which("claude")
    archify = archify_bin()
    if not claude or not archify:
        log(folder, "skipping the map update: claude or archify not found")
        return False
    task = ("map.json is last night's map: update it, adding nodes and flows for newly confirmed links."
            if has_map else "there is no map.json yet: write it following the skill's map rules.")
    prompt = MAP_PROMPT.format(folder=folder, scripts=SCRIPTS, archify=archify, map_task=task)
    tools = ("Read,Write,Edit,Glob,Grep,Bash(python3:*),Bash(node:*),Bash(ls:*),Bash(cat:*),"
             "Bash(head:*),Bash(tail:*),Bash(grep:*),Bash(sed -n:*),Bash(wc:*)")
    cmd = [claude, "-p", prompt, "--model", model, "--allowedTools", tools, "--output-format", "text",
           "--add-dir", str(STATE)]  # descriptions.json and links.json live there
    log(folder, f"updating the map with {model}")
    env = {**os.environ, "HIVEY_AGENT": "hivey-agent-map"}
    token = claude_token(folder)
    if token:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = token  # this process only; never logged
    try:
        p = subprocess.run(cmd, cwd=folder, capture_output=True, text=True, timeout=1500, env=env)
        (folder / "llm.log").write_text(p.stdout + "\n--- stderr ---\n" + p.stderr)
        # the model's own words stay in llm.log: they would read as errors here ("… failed …")
        log(folder, f"{model}: exit {p.returncode} (its reply: llm.log)")
        return p.returncode == 0
    except (OSError, subprocess.SubprocessError) as e:
        log(folder, f"{model} run failed: {e}")
        return False


def html_current(folder: Path, html: str, src: str) -> bool:
    h, s = folder / html, folder / src
    return h.is_file() and s.is_file() and h.stat().st_mtime >= s.stat().st_mtime


def nightly(scan, days: int, model: str, use_llm: bool) -> int:
    REPORTS.mkdir(parents=True, exist_ok=True)
    now = dt.datetime.now()
    folder = REPORTS / now.strftime("%Y-%m-%d-%H%M")
    if folder.exists():  # a second run in the same minute gets its own folder
        folder = REPORTS / now.strftime("%Y-%m-%d-%H%M%S")
    folder.mkdir(exist_ok=True)
    log(folder, f"scan, last {days} days")
    snap = scan(days)
    (STATE / "snapshot.json").write_text(json.dumps(snap, indent=1, default=str))
    write_stats(snap, folder)
    write_logs_page(snap, folder)
    ok, msg = finalize(folder, "explainer", "stats.archify.md", "agent-stats.html")
    log(folder, f"stats page: {'ok' if ok else 'FAILED ' + msg}")

    prev = LATEST.resolve() / "map.json" if LATEST.exists() else None
    if prev and prev.is_file() and prev.resolve() != (folder / "map.json").resolve():
        shutil.copy2(prev, folder / "map.json")
    has_map = (folder / "map.json").is_file()
    if use_llm:
        run_llm(folder, model, has_map)
    if (folder / "map.json").is_file() and not html_current(folder, "agent-map.html", "map.json"):
        good, why = finalize(folder, "dataflow", "map.json", "agent-map.html")
        if not good and prev and prev.is_file():  # the model broke it: last night's map, unchanged
            log(folder, f"map failed ({why}); using last night's map")
            shutil.copy2(prev, folder / "map.json")
            good, why = finalize(folder, "dataflow", "map.json", "agent-map.html")
        log(folder, f"map: {'ok' if good else 'FAILED ' + why}")
    else:
        log(folder, "map: ok" if (folder / "agent-map.html").is_file() else "map: none yet")

    try:
        import enhance
        enhance.enhance_folder(folder, json.loads((folder / "snapshot.json").read_text()), job_state)
    except Exception as e:  # the pages still work without hivey's layer
        log(folder, f"enhance failed: {e}")
    if ok:
        tmp = REPORTS / ".latest-new"
        if tmp.is_symlink() or tmp.exists():
            tmp.unlink()
        tmp.symlink_to(folder.name)
        os.replace(tmp, LATEST)  # atomic: `open` never sees a missing link
        for old in sorted(p for p in REPORTS.iterdir() if p.is_dir() and not p.is_symlink()
                          and re.match(r"\d{4}-\d\d-\d\d-\d{4}(\d\d)?$", p.name))[:-KEEP_REPORTS]:
            shutil.rmtree(old, ignore_errors=True)
    log(folder, f"done: {folder}")
    return 0 if ok else 1


# ---------------------------------------------------------------- open

def latest_report() -> Path | None:
    if LATEST.exists() and (LATEST / "agent-stats.html").is_file():
        return LATEST.resolve()
    folders = sorted(p for p in REPORTS.glob("*-*-*-*") if (p / "agent-stats.html").is_file()) if REPORTS.is_dir() else []
    return folders[-1] if folders else None


def notify(text: str) -> None:
    if sys.platform == "darwin" and not (HOME / ".hivey" / "quiet").exists():
        subprocess.run(["osascript", "-e", f'display notification {json.dumps(text)} with title "hivey agent map"'],
                       capture_output=True)


def open_latest(model: str) -> int:
    folder = latest_report()
    if folder:
        for page in ("agent-stats.html", "agent-map.html", "agent-logs.html"):
            if (folder / page).is_file():
                subprocess.run(["open", str(folder / page)], capture_output=True)
        print(f"opened {folder}")
        return 0
    # No report yet: build the first one in the background and open it when it is ready.
    REPORTS.mkdir(parents=True, exist_ok=True)
    with open(STATE / "first-run.log", "a") as fh:
        subprocess.Popen([sys.executable, "-I", str(SCRIPTS / "agent_map.py"), "nightly", "--model", model, "--open"],
                         stdout=fh, stderr=fh, stdin=subprocess.DEVNULL, start_new_session=True)
    notify("Building your first agent map (a few minutes); it opens when ready.")
    print("no report yet: building the first one in the background (log: ~/.hivey/agent-map/first-run.log)")
    return 0


# ---------------------------------------------------------------- schedule (launchd)

OFFER = STATE / "offer.json"


def offer(hour: int, minute: int, model: str) -> int:
    """Asked once, by install.sh and `hivey update`: set up the nightly run? The answer is kept in
    offer.json so it is never asked again; with no terminal nothing is asked (asked next time)."""
    if sys.platform != "darwin" or PLIST.is_file() or OFFER.is_file() or not sys.stdin.isatty():
        return 0  # not a person at a terminal (remote or scripted update): asked next time
    try:
        tty = open("/dev/tty", "r+")
    except OSError:
        return 0
    with tty:
        tty.write(
            "\nhivey can watch this computer every night: scheduled jobs (cron, launchd), agents and\n"
            "swarms, token use and estimated cost per agent and provider, failing jobs, and how\n"
            "agents feed each other. The report opens from the hivey menu → agent map.\n"
            f"It runs at {hour:02}:{minute:02} with Claude {model} (a few cents a night).\n"
            "Set up the nightly agent monitoring? [Y/n] ")
        tty.flush()
        answer = tty.readline().strip().lower()
    yes = not answer.startswith("n")
    STATE.mkdir(parents=True, exist_ok=True)
    OFFER.write_text(json.dumps({"asked": dt.datetime.now().isoformat(timespec="seconds"), "answer": "yes" if yes else "no"}))
    if not yes:
        print("skipped (later: hivey agent-map schedule install)")
        return 0
    return schedule("install", hour, minute, model)


def schedule(action: str, hour: int, minute: int, model: str) -> int:
    if action == "offer":
        return offer(hour, minute, model)
    uid = os.getuid()
    if action == "status":
        if not PLIST.is_file():
            print("not scheduled (hivey agent-map schedule install)")
            return 1
        with open(PLIST, "rb") as fh:
            d = plistlib.load(fh)
        cal = d.get("StartCalendarInterval", {})
        loaded = subprocess.run(["launchctl", "print", f"gui/{uid}/{LABEL}"], capture_output=True).returncode == 0
        print(f"nightly at {cal.get('Hour', 0):02}:{cal.get('Minute', 0):02} with "
              f"{d['ProgramArguments'][-1]} · {'loaded' if loaded else 'not loaded'} · {PLIST}")
        return 0
    subprocess.run(["launchctl", "bootout", f"gui/{uid}/{LABEL}"], capture_output=True)
    if action == "remove":
        if PLIST.exists():
            PLIST.unlink()
        print("nightly run removed")
        return 0
    installed = HOME / ".claude/skills/hivey-agent-map/scripts/agent_map.py"
    script = installed if installed.is_file() else SCRIPTS / "agent_map.py"  # refreshed by hivey update
    STATE.mkdir(parents=True, exist_ok=True)
    plist = {
        "Label": LABEL,
        # launchd, not cron: `claude -p` needs the login keychain, which cron cannot reach.
        "ProgramArguments": ["/usr/bin/env", "python3", "-I", str(script), "nightly", "--model", model],
        "StartCalendarInterval": {"Hour": hour, "Minute": minute},  # a missed run happens on wake
        "EnvironmentVariables": {"PATH": LAUNCH_PATH, "HOME": str(HOME)},
        "WorkingDirectory": str(STATE),
        "StandardOutPath": str(STATE / "nightly.log"),
        "StandardErrorPath": str(STATE / "nightly.log"),
        "ProcessType": "Background",
        "LowPriorityIO": True,
    }
    PLIST.parent.mkdir(parents=True, exist_ok=True)
    with open(PLIST, "wb") as fh:
        plistlib.dump(plist, fh)
    r = subprocess.run(["launchctl", "bootstrap", f"gui/{uid}", str(PLIST)], capture_output=True, text=True)
    if r.returncode != 0:
        print(f"wrote {PLIST} but launchctl bootstrap failed: {r.stderr.strip()}")
        return 1
    print(f"nightly at {hour:02}:{minute:02} with {model}: {PLIST} (log {STATE / 'nightly.log'})")
    return 0
