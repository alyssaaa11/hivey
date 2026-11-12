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
import time
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
    span = span_of(snap)
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
    headless = sum(v for k, v in kinds.items() if "headless" in k)
    api = total - headless
    models = sorted(u["models"].items(), key=lambda kv: -kv[1]["cost_usd"])
    jobs = snap["jobs"]
    failing = [j for j in jobs if job_state(j)[0] == "❌"]
    n_cron = sum(1 for j in jobs if j["kind"].startswith("cron"))
    gaps = [n for n in snap["nodes"] if (n.get("llm_api") or {}).get("unlogged")]
    top = agents[0] if agents else None
    pct = lambda v: f"{100 * v / total:.0f}%" if total else "0%"

    L = []
    inter = snap.get("interactive", {})
    L.append(f"# What ran on its own on this Mac in the last {span}, and what did it cost? {{#agent-stats}}\n")
    if top:
        L.append(f"Your scheduled jobs, scripts and headless agents used about **{money(total)} of tokens** in "
                 f"{span}; **{cell(top['agent'])} used {money(top['cost_usd'])}** of it ({pct(top['cost_usd'])}).\n")
    else:
        L.append(f"No automated token use was recorded in the last {span}.\n")
    L.append(f"- Headless agent runs cost {money(headless)} and scripts' direct API calls {money(api)}; local "
             f"models (JEV, Whisper) cost nothing.")
    if inter.get("total_cost_usd"):
        L.append(f"- Your interactive sessions are not counted here: {money(inter['total_cost_usd'])} in "
                 f"{(inter.get('sessions') or [{}])[0].get('count_total', 0)} sessions, on the Sessions page (`agent-sessions.html`).")
    if models:
        L.append("- " + "; ".join(f"{cell(model_name(m))} {money(v['cost_usd'])}" for m, v in models[:3]) + ".")
    states = [job_state(j)[0] for j in jobs]
    tally = ", ".join(f"{emo} {states.count(emo)}" for emo in ("✅", "⚠️", "❌", "⏳", "❔") if states.count(emo))
    L.append(f"- {len(jobs)} scheduled jobs ({n_cron} cron, {len(jobs) - n_cron} launchd): {tally}; "
             f"{len(snap['links_confirmed'])} links between them; {len(gaps)} with LLM calls not logged yet.\n")
    L.append(f'@answer scope="last {span} to {snap["generated"][:10]} · costs are list-price estimates · '
             f'Claude Code (Max) and Codex (Plus) are not billed per token"')
    L.append("@meta locale=en")
    L.append('@source scan title="hivey agent map snapshot (agent_map.py scan)" path=snapshot.json\n')

    if agents:
        L.append("## Which agents spend the most? {#by-agent}\n")
        L.append(f'@chart type=bar data=agents.csv x=agent y=cost_usd unit=USD title="Cost by agent, '
                 f'last {span} (API-equivalent USD, estimated)"\n')
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
             "in this folder (hivey menu → agent jobs opens it too).\n")

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
def job_anchor(job_id: str) -> str:
    """The logs page anchor of a job (the map links to it)."""
    return "job-" + re.sub(r"[^a-z0-9]+", "-", job_id.lower()).strip("-")


ORDER = {"❌": 0, "⚠️": 1, "🔴": 1, "⏳": 2, "🟢": 2, "❔": 3, "⚪": 4, "✅": 4, "⚫": 5}


def write_logs_page(snap: dict, folder: Path) -> None:
    import html
    e = html.escape
    jobs = sorted(snap["jobs"], key=lambda j: (ORDER.get(job_state(j)[0], 9), j["name"]))
    usage = {a["agent"]: a for a in snap["usage"]["agents"]}
    inter_usage = {a["agent"]: a for a in (snap.get("interactive") or {}).get("agents", [])}
    logs = snap.get("agent_logs", {})
    live: dict[str, dict] = {}
    for a in snap["live_agents"]:
        cur = live.get(a["agent"])
        if not cur or ORDER.get(AGENT_STATE.get(a["status"], ("⚪",))[0], 9) < ORDER.get(AGENT_STATE.get(cur["status"], ("⚪",))[0], 9):
            live[a["agent"]] = a
    names = sorted(set(live) | {n["label"] for n in snap["nodes"] if n["type"] == "agent"}
                   | {a for a in usage if usage[a]["cost_usd"] >= 0.5}
                   | {a for a in inter_usage if inter_usage[a]["cost_usd"] >= 0.5})

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
        rows.append(f"<details class=row id='{job_anchor(j['id'])}' data-state='{emo}'><summary><span class=emo>{emo}</span>"
                    f"<span class=name><b>{e(j['name'])}</b><small class=desc>{e(j.get('description') or '')}</small></span>"
                    f"<span class=sub>{e(words)}</span><span class=when>{e(last)}</span>"
                    f"<span class=issue>{e(issue[:110])}</span></summary>{details(body)}</details>")
    arows = []
    dormant = [n for n in names if n not in live and n not in usage and n not in inter_usage and not logs.get(n)]
    names = [n for n in names if n not in dormant]
    for name in sorted(names, key=lambda n: (ORDER.get(AGENT_STATE.get(live.get(n, {}).get("status", "gone"), ("⚫",))[0], 9), n)):
        a = live.get(name)
        emo, words = AGENT_STATE.get(a["status"], ("⚪", a["status"])) if a else ("⚫", "not running")
        info = logs.get(name, {})
        if info.get("errors") and emo in ("⚪", "✅", "⚫"):
            emo, words = "⚠️", words + ", errors in its log"
        u = usage.get(name, {})
        iu = inter_usage.get(name, {})
        parts = ([f"automated ${u['cost_usd']:,.2f}"] if u else []) + ([f"chats ${iu['cost_usd']:,.2f}"] if iu else [])
        cost = (" · ".join(parts) + f" in {span_of(snap)}") if parts else "no token use"
        body = [f"<p class=meta>{e(a['where'] if a else 'not open in hivey')} · {e(a.get('cwd', '') if a else '')}"
                f" · last active {e((u or iu).get('last_active') or '?')} · {e(cost)}"
                + (f" · {e(', '.join((u or iu).get('models', [])[:2]))}" if (u or iu) else "") + "</p>"]
        body += log_block(info) if info else ["<p class=meta>no <code>logs/</code> folder in this agent.</p>"]
        adesc = next((n.get("description", "") for n in snap["nodes"] if n["type"] == "agent" and n["label"] == name), "")
        arows.append(f"<details class=row><summary><span class=emo>{emo}</span>"
                     f"<span class=name><b>{e(name)}</b><small class=desc>{e(adesc)}</small></span>"
                     f"<span class=sub>{e(words)}</span><span class=when>{e(((u or iu).get('last_active') or '')[:16].replace('T', ' '))}</span>"
                     f"<span class=issue>{e(cost)}</span></summary>{details(body)}</details>")
    if dormant:
        arows.append(f"<details class=row><summary><span class=emo>⚫</span><b>{len(dormant)} dormant agents</b>"
                     f"<span class=sub>not running, no token use, no logs</span><span class=when></span>"
                     f"<span class=issue>{e(', '.join(dormant[:8]))}…</span></summary>"
                     f"<p class=meta>{e(', '.join(dormant))}</p></details>")
    summary = " · ".join(f"{k} {v}" for k, v in sorted(counts.items(), key=lambda kv: ORDER.get(kv[0], 9)))
    import enhance
    NAV, NAV_CSS = enhance.nav("agent-logs.html", snap['generated'], snap['window_days']), enhance.NAV_CSS
    page = f"""<!doctype html><html lang=en><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
{enhance.FAVICON}
<title>Agent logs · {e(snap['generated'][:16])}</title>
<style>
:root{{--bg:#fff;--fg:#1d2330;--mute:#667085;--line:#e4e7ec;--card:#f8f9fb;--accent:#2f6fec;--pre:#f2f4f7}}
@media (prefers-color-scheme:dark){{:root{{--bg:#0d1117;--fg:#e6edf3;--mute:#8b949e;--line:#30363d;--card:#161b22;--accent:#79a8ff;--pre:#0b0f14}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}}
main{{max-width:1180px;margin:0 auto;padding:28px 24px 60px}}
{NAV_CSS}
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
{NAV}
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
</main>
<script>(function(){{const open=()=>{{const d=location.hash&&document.getElementById(location.hash.slice(1));
if(d&&d.tagName==='DETAILS'){{d.open=true;d.scrollIntoView({{block:'start'}});}}}};open();addEventListener('hashchange',open);}})();</script>
</html>"""
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
   The map shows AUTOMATED work only: scheduled jobs, scripts and headless agents, the data they pass
   to each other, and the models each one uses (any model: JEV, Claude, DeepSeek, Grok, Codex, Whisper;
   nodes[].llm_api.models in snapshot.json). People chatting with agents (interactive sessions) are on
   another page: never show their spend; drop an agent that has no automated runs and no data links,
   and keep a consumer agent (one a job feeds) without its interactive cost. Each node's sublabel holds
   its schedule with its start time (a box folding several jobs shows the earliest: "cron from 00:30")
   or its role, its models and its automated cost when it has one ("cron 04:15 · Grok + Sonnet ·
   $0.40"); spend is usage.agents (automated only). Keep at most 25 nodes on 5 stages x rows 0-4;
   refresh the numbers in the cards (automated spend, models in use, needs attention).
3. Descriptions: every node in snapshot.json has a "description" (what it does). snapshot.json
   "descriptions_to_write" lists the ids whose description is empty, cut off, copied from a brief
   ("You are X", "Own the…") or shared with another job. For each, read its brief, script or command
   (first lines only) and write ONE plain sentence into ~/.hivey/agent-map/descriptions.json
   ({{"<node id>": "<sentence>"}}; keep the entries already there). Rules: 8 to 18 words; start with a
   verb ("Ranks…", "Downloads…", "Paper-trades…"); say what it does and what it produces or where;
   no "you", no dates, no ids, no trailing "…"; different jobs get different sentences (say what
   sets each apart, e.g. "places the orders" vs "writes the report").
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


def span_of(snap: dict) -> str:
    import enhance
    return enhance.span(snap.get("window_days", 7))


def write_day_view(scan, folder: Path) -> tuple[bool, str]:
    """The same pages for the last 24 hours, in <folder>/1d/ (the navbar's "24 h" toggle). The map
    keeps the boxes the night drew; its colours, info cards and legend use the 24-hour numbers."""
    import enhance
    import sessions_page
    sub = folder / enhance.DAY_DIR
    sub.mkdir(exist_ok=True)
    snap = scan(1)
    write_stats(snap, sub)
    write_logs_page(snap, sub)
    sessions_page.write_sessions_page(snap, sub)
    ok, msg = finalize(sub, "explainer", "stats.archify.md", "agent-stats.html")
    for name in ("map.json", "map-meta.json", "agent-map.html"):
        if (sub / name).exists():
            (sub / name).unlink()
    if (folder / "map-meta.json").is_file():
        shutil.copy2(folder / "map-meta.json", sub / "map-meta.json")
    if (folder / "map.json").is_file():
        (sub / "map.json").write_text(json.dumps(day_spec(json.loads((folder / "map.json").read_text())), indent=1))
        good, why = finalize(sub, "dataflow", "map.json", "agent-map.html")
        if not good and (folder / "agent-map.html").is_file():  # the 7-day drawing still beats no map
            log(folder, f"24-hour map failed ({why}); using the 7-day drawing")
            shutil.copy2(folder / "agent-map.html", sub / "agent-map.html")
    enhance.enhance_folder(sub, snap, job_state)
    return ok, msg


MONEY = r"\$[\d,.]+k?"


def day_spec(spec: dict) -> dict:
    """The night's map for the 24-hour pages: same boxes and flows, without the 7-day dollar amounts
    the model wrote into them (the colours and info cards carry the 24-hour numbers)."""
    meta = dict(spec.get("meta") or {})
    title = meta.get("title", "")
    meta["title"] = re.sub(r"\(last \d+ days\)", "(last 24 hours)", title) if re.search(r"\(last \d+ days\)", title) \
        else (title + " (last 24 hours)").strip()
    nodes = []
    for n in spec.get("nodes") or []:
        n = dict(n)
        if "sublabel" in n:
            n["sublabel"] = re.sub(r"\s*·\s*" + MONEY + r"(?=\s*(·|$))", "", n["sublabel"]).strip(" ·")
        if re.fullmatch(r"\s*" + MONEY + r"\s*", str(n.get("tag", ""))):
            n.pop("tag")
        nodes.append(n)
    cards = [c for c in spec.get("cards") or [] if not re.search(r"last \d+ days|spend", str(c.get("title", "")), re.I)]
    return {**spec, "meta": meta, "nodes": nodes, "cards": cards}


DAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]


def job_start(schedule: str) -> tuple[str, str] | None:
    """(HH:MM, the schedule's words for one job) from a cron line or a launchd description, or None
    when the job has no fixed start time (hourly, every N s, always on, at login, on demand)."""
    s = (schedule or "").strip()
    f = s.split()
    if len(f) == 5:  # cron: minute hour day-of-month month day-of-week
        mm, hh = (re.split(r"[,-]", x)[0] for x in f[:2])
        if not (mm.isdigit() and hh.isdigit()):
            return None
        t = f"{int(hh):02d}:{int(mm):02d}"
        if f[4].isdigit():
            return t, f"{DAYS[int(f[4]) % 7]} {t}"
        if f[2].isdigit():
            return t, f"day {f[2]} {t}"
        return t, t
    m = re.search(r"(\d\d):(\d\d)", s)
    if not m:
        return None
    return m.group(0), re.sub(r"^daily\s+", "", s)


def start_time_labels(spec: dict, meta: dict, snap: dict) -> dict[str, list[str]]:
    """Every scheduled box on the map shows when it starts. The model writes the sublabels and leaves
    the time out of folded boxes ("11 trading jobs") and of agents run by jobs; the scan knows each
    job's schedule, so a box without a time gets its job's time, or "from <earliest>" when it stands
    for several jobs. Data stores get none. Returns {box id: sublabels to try, longest first}: when a
    box is too narrow, the model names go before the cost and the errors."""
    jobs = {j["id"]: j for j in snap.get("jobs") or [] if j.get("id")}
    owned: dict[str, list[str]] = {}
    for b in (snap.get("links_candidates") or {}).get("belongs_to") or []:
        owned.setdefault(b.get("agent", ""), []).append(b.get("job", ""))
    out = {}
    for n in spec.get("nodes") or []:
        sub = str(n.get("sublabel", ""))
        if n.get("type") == "database" or re.search(r"\b\d\d:\d\d\b", sub):
            continue
        refs = ((meta.get("nodes") or {}).get(n.get("id")) or {}).get("refs") or []
        ids = [r for r in refs if r in jobs] or [j for r in refs for j in owned.get(r, []) if j in jobs]
        starts = [s for s in (job_start(jobs[j].get("schedule", "")) for j in dict.fromkeys(ids)) if s]
        if not starts:
            continue
        when = starts[0][1] if len(starts) == 1 else "from " + min(t for t, _ in starts)
        m = re.match(r"(cron|launchd|nightly|daily|weekly|\d+ jobs)\b", sub)
        first = (sub[:m.end()] + " " + when + sub[m.end():]) if m else " · ".join(x for x in (when, sub) if x)
        parts = [x.strip() for x in first.split("·")]
        tries = [first]
        while len(parts) > 1:
            keep = lambda x: re.search(r"\$|error|exit|fail", x, re.I)
            drop = next((i for i in range(len(parts) - 1, 0, -1) if not keep(parts[i])), len(parts) - 1)
            parts.pop(drop)
            tries.append(" · ".join(parts))
        out[str(n.get("id"))] = tries
    return out


def place_start_times(folder: Path) -> None:
    """Put the start times on the map (start_time_labels) and redraw it. A box the drawing rejects as
    too narrow tries its next, shorter sublabel; when nothing fits, the night's map stays as it was."""
    spec_path, meta_path, snap_path = folder / "map.json", folder / "map-meta.json", folder / "snapshot.json"
    try:
        original = spec_path.read_text()
        spec, meta, snap = json.loads(original), json.loads(meta_path.read_text()), json.loads(snap_path.read_text())
    except (OSError, ValueError):
        return
    tries = start_time_labels(spec, meta, snap)
    if not tries:
        return
    step = dict.fromkeys(tries, 0)
    for _ in range(8):
        for n in spec.get("nodes") or []:
            if n.get("id") in step:
                n["sublabel"] = tries[n["id"]][step[n["id"]]]
        spec_path.write_text(json.dumps(spec, indent=1))
        good, why = finalize(folder, "dataflow", "map.json", "agent-map.html")
        if good:
            log(folder, f"map start times: {len(step)} boxes")
            return
        bad = [i for i in re.findall(r'node "([^"]+)"', why) if i in step]
        if not bad or any(step[i] + 1 >= len(tries[i]) for i in bad):
            break
        for i in bad:
            step[i] += 1
    log(folder, f"map start times did not fit ({why}); map unchanged")
    spec_path.write_text(original)
    finalize(folder, "dataflow", "map.json", "agent-map.html")


def day_view(scan, folder: Path) -> None:
    try:
        ok, msg = write_day_view(scan, folder)
        log(folder, f"24-hour pages: {'ok' if ok else 'stats page FAILED ' + msg}")
    except Exception as e:  # the 7-day pages stand on their own
        log(folder, f"24-hour pages failed: {e}")


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
    import sessions_page
    sessions_page.write_sessions_page(snap, folder)
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
    if (folder / "agent-map.html").is_file():
        place_start_times(folder)

    try:
        import enhance
        enhance.enhance_folder(folder, json.loads((folder / "snapshot.json").read_text()), job_state)
    except Exception as e:  # the pages still work without hivey's layer
        log(folder, f"enhance failed: {e}")
    day_view(scan, folder)
    if ok or (folder / "agent-logs.html").is_file():  # a failed stats page still publishes logs and sessions
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


def refresh(scan, days: int, model: str) -> int:
    """The Refresh button: a new scan rewrites the latest report's pages in place (stats, run log,
    sessions, the map's colours and info cards). No model run, so it is free; the map's boxes stay
    as the night drew them. With no report yet it builds one (also without a model)."""
    folder = latest_report()
    if not folder:
        return nightly(scan, days, model, False)
    log(folder, f"refresh: scan, last {days} days")
    snap = scan(days)
    (STATE / "snapshot.json").write_text(json.dumps(snap, indent=1, default=str))
    write_stats(snap, folder)
    write_logs_page(snap, folder)
    import sessions_page
    sessions_page.write_sessions_page(snap, folder)
    ok, msg = finalize(folder, "explainer", "stats.archify.md", "agent-stats.html")
    place_start_times(folder)
    try:
        import enhance
        enhance.enhance_folder(folder, json.loads((folder / "snapshot.json").read_text()), job_state)
    except Exception as e:
        log(folder, f"enhance failed: {e}")
    day_view(scan, folder)
    log(folder, f"refresh: {'ok' if ok else 'stats page FAILED ' + msg}")
    return 0 if ok else 1


# ---------------------------------------------------------------- open

def latest_report() -> Path | None:
    def has_pages(p: Path) -> bool:
        return (p / "agent-stats.html").is_file() or (p / "agent-logs.html").is_file()
    if LATEST.exists() and has_pages(LATEST):
        return LATEST.resolve()
    folders = sorted(p for p in REPORTS.glob("*-*-*-*") if has_pages(p)) if REPORTS.is_dir() else []
    return folders[-1] if folders else None


def notify(text: str) -> None:
    if sys.platform == "darwin" and not (HOME / ".hivey" / "quiet").exists():
        subprocess.run(["osascript", "-e", f'display notification {json.dumps(text)} with title "hivey agent map"'],
                       capture_output=True)


PAGES = ("agent-map.html", "agent-stats.html", "agent-logs.html", "agent-sessions.html")  # the first one found opens
FIRST_RUN = STATE / "first-run.json"


def open_page(folder: Path) -> bool:
    """Open the map (else the first page there is) from the local server, so Refresh works; the
    navbar reaches the other pages. Without the server, the file itself."""
    page = next((p for p in PAGES if (folder / p).is_file()), None)
    if not page:
        return False
    import serve
    port = serve.ensure()
    rel = "latest" if LATEST.exists() and LATEST.resolve() == folder.resolve() else folder.name
    target = f"http://127.0.0.1:{port}/{rel}/{page}" if port else str(folder / page)
    p = subprocess.run(["open", target], capture_output=True, text=True)
    if p.returncode:
        notify(f"Could not open the agent map: {(p.stderr or '').strip()[:120]}")
        return False
    print(f"opened {target}")
    return True


def first_run_busy() -> bool:
    try:
        info = json.loads(FIRST_RUN.read_text())
        os.kill(int(info["pid"]), 0)
        return time.time() - info.get("started", 0) < 1800
    except (OSError, ValueError, KeyError, TypeError):
        return False


def open_latest(model: str, show: bool = True) -> int:
    folder = latest_report()
    if folder:
        return 0 if not show or open_page(folder) else 1
    # No report yet: the pages without a model first (seconds), open them, then the map in the background.
    if first_run_busy():  # a second click while the first report is being built
        notify("Your first agent map is still being built; it opens when ready.")
        return 0
    REPORTS.mkdir(parents=True, exist_ok=True)
    FIRST_RUN.write_text(json.dumps({"pid": os.getpid(), "started": time.time()}))
    notify("Building your first agent map (a few seconds)…")
    with open(STATE / "first-run.log", "a") as fh:
        subprocess.run([sys.executable, "-I", str(SCRIPTS / "agent_map.py"), "refresh"], stdout=fh, stderr=fh,
                       stdin=subprocess.DEVNULL, timeout=600)
        folder = latest_report()
        if folder and show:
            open_page(folder)
        # the map needs the model: drawn in the background; the page's Refresh (or reopening) shows it
        bg = subprocess.Popen([sys.executable, "-I", str(SCRIPTS / "agent_map.py"), "nightly", "--model", model],
                              stdout=fh, stderr=fh, stdin=subprocess.DEVNULL, start_new_session=True)
    FIRST_RUN.write_text(json.dumps({"pid": bg.pid, "started": time.time()}))
    if folder and show:
        notify("Stats, logs and sessions are open; the map is being drawn (a few minutes).")
    print("first report: pages built; the map is being drawn in the background (log: ~/.hivey/agent-map/first-run.log)")
    return 0


# ---------------------------------------------------------------- schedule (launchd)

OFFER = STATE / "offer.json"


def offer(hour: int, minute: int, model: str) -> int:
    """Asked once, by install.sh and `hivey update`: set up the nightly run? The answer is kept in
    offer.json so it is never asked again; with no terminal nothing is asked (asked next time)."""
    if sys.platform == "darwin" and PLIST.is_file():  # nights set up with Haiku move to Sonnet, same time:
        try:                                             # Haiku did not follow the map rules well
            with open(PLIST, "rb") as fh:
                d = plistlib.load(fh)
            if d.get("ProgramArguments", [])[-2:] == ["--model", "haiku"]:
                cal = d.get("StartCalendarInterval", {})
                return schedule("install", cal.get("Hour", hour), cal.get("Minute", minute), "sonnet")
        except (OSError, ValueError, plistlib.InvalidFileException):
            pass
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
            "agents feed each other. The report opens from the hivey menu → agent jobs.\n"
            f"It runs at {hour:02}:{minute:02} with Claude {model} (well under a dollar a night).\n"
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
