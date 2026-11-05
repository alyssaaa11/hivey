"""The Sessions page (agent-sessions.html): what your interactive sessions with agents cost.

Interactive = a person chatting with an agent in a terminal (Claude Code, Codex, Grok). These are
kept out of the map and the stats page, which show automated work only. Written from the snapshot's
`interactive` summary, without an LLM. Python standard library only.
"""
from __future__ import annotations

import collections
import datetime as dt
import html
import re
from pathlib import Path

e = html.escape
LONG_CONTEXT = 300_000  # tokens: above this every reply re-reads a very large session


def money(v: float) -> str:
    return f"${v:,.2f}"


def mtok(n: int) -> str:
    return f"{n / 1e6:,.1f}M" if n >= 1e6 else f"{n / 1e3:,.0f}k" if n >= 1e3 else str(n)


def model_name(m: str) -> str:
    m = m.rsplit("/", 1)[-1]
    k = re.match(r"claude-(opus|sonnet|haiku)-(\d+)(?:-(\d{1,2}))?(?!\d)", m)
    return (f"{k.group(1).title()} {k.group(2)}" + (f".{k.group(3)}" if k.group(3) else "")) if k else m


def bar(value: float, top: float) -> str:
    pct = 0 if top <= 0 else max(1.5, 100 * value / top)
    return f'<span class=bar><i style="width:{pct:.1f}%"></i></span>'


def write_sessions_page(snap: dict, folder: Path) -> None:
    inter = snap.get("interactive") or {}
    sessions = inter.get("sessions") or []
    days = snap.get("window_days", 7)
    total = inter.get("total_cost_usd", 0.0)
    agents = inter.get("agents") or []
    per_agent = collections.defaultdict(lambda: {"n": 0, "peak": 0, "long": 0})
    for s in sessions:
        for name in s.get("agents") or [s["agent"]]:
            pa = per_agent[name]
            pa["n"] += 1
            pa["peak"] = max(pa["peak"], s["max_context"])
            pa["long"] += s["max_context"] > LONG_CONTEXT
    long_cost = sum(s["cost_usd"] for s in sessions if s["max_context"] > LONG_CONTEXT)
    long_n = sum(1 for s in sessions if s["max_context"] > LONG_CONTEXT)
    day_long = sum(1 for s in sessions if s["hours"] >= 12)
    models = sorted((inter.get("models") or {}).items(), key=lambda kv: -kv[1]["cost_usd"])

    top_agent = max((a["cost_usd"] for a in agents), default=0.0)
    arows = []
    for a in agents[:25]:
        pa = per_agent[a["agent"]]
        tok = a["input"] + a["output"] + a["cache_read"] + a["cache_write"]
        warn = " ⚠️" if pa["long"] else ""
        arows.append(
            f"<tr><td><b>{e(a['agent'])}</b></td><td class=num>{pa['n']}</td>"
            f"<td class=cost>{bar(a['cost_usd'], top_agent)}<span>{money(a['cost_usd'])}</span></td>"
            f"<td class=num>{mtok(tok)}</td><td class=num>{mtok(pa['peak'])}{warn}</td>"
            f"<td>{e(', '.join(model_name(m) for m in a['models'][:2]))}</td>"
            f"<td class=dim>{e((a.get('last_active') or '')[:16].replace('T', ' '))}</td></tr>")

    per_day = inter.get("daily_by_agent") or {}
    day_cost = {d: sum(r["cost_usd"] for r in rows) for d, rows in per_day.items()}
    top_day = max(day_cost.values(), default=0.0)
    drows = []
    for d, rows in per_day.items():
        tops = ", ".join(f"{e(r['agent'])} {money(r['cost_usd'])}" for r in rows[:3])
        drows.append(
            f"<tr><td><b>{dt.date.fromisoformat(d):%a %d %b}</b></td>"
            f"<td class=cost>{bar(day_cost[d], top_day)}<span>{money(day_cost[d])}</span></td>"
            f"<td class=num>{mtok(sum(r['tokens'] for r in rows))}</td><td>{tops}</td></tr>")

    srows = []
    for s in sessions[:20]:
        ctx_warn = " ⚠️" if s["max_context"] > LONG_CONTEXT else ""
        srows.append(
            f"<tr><td class=num><b>{money(s['cost_usd'])}</b></td><td>{e(s['agent'])}</td>"
            f"<td class=dim>{e(s['start'][5:16].replace('T', ' '))} → {e(s['end'][5:16].replace('T', ' '))}"
            f" · {s['hours']}h</td><td class=num>{s['calls']:,}</td>"
            f"<td class=num>{mtok(s['max_context'])}{ctx_warn}</td>"
            f"<td>{e(', '.join(model_name(m) for m in s['models'][:2]))}</td>"
            f"<td class=prompt title=\"{e(s.get('prompt', ''))}\">{e(s.get('prompt', '') or '—')}</td></tr>")

    tips = []
    if long_n:
        tips.append(f"<li><b>{long_n} sessions grew past {LONG_CONTEXT // 1000}k tokens of context</b> and cost "
                    f"{money(long_cost)} ({100 * long_cost / total:.0f}% of the total): every reply re-reads the whole "
                    f"session. Start a new session per task (<code>/clear</code>) and <code>/compact</code> a long one.</li>")
    if day_long:
        tips.append(f"<li><b>{day_long} sessions stayed open 12 hours or more.</b> After a pause the cached context "
                    "expires and is written again in full, which costs more than reading it.</li>")
    if models and "opus" in models[0][0]:
        tips.append(f"<li><b>{e(model_name(models[0][0]))} is {money(models[0][1]['cost_usd'])}</b> of it: drafting, "
                    "reviews and diagrams cost half on Sonnet (<code>/model sonnet</code>).</li>")
    n_total = sessions[0]["count_total"] if sessions else 0

    page = f"""<!doctype html><html lang=en><meta charset=utf-8>
<meta name=viewport content="width=device-width,initial-scale=1">
<title>Interactive sessions · {e(snap.get('generated', '')[:16])}</title>
<style>
:root{{--bg:#fff;--fg:#1d2330;--mute:#667085;--line:#e4e7ec;--card:#f8f9fb;--accent:#2f6fec}}
@media (prefers-color-scheme:dark){{:root{{--bg:#0d1117;--fg:#e6edf3;--mute:#8b949e;--line:#30363d;--card:#161b22;--accent:#79a8ff}}}}
body{{margin:0;background:var(--bg);color:var(--fg);font:15px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}}
main{{max-width:1180px;margin:0 auto;padding:28px 24px 60px}}
nav a{{color:var(--accent);margin-right:18px;text-decoration:none;font-weight:600}} nav a.on{{color:var(--fg)}}
h1{{font-size:26px;margin:18px 0 4px}} h2{{font-size:19px;margin:34px 0 10px}}
.lead{{color:var(--mute);margin:0 0 14px}}
.kpis{{display:flex;gap:12px;flex-wrap:wrap;margin:14px 0}}
.kpi{{border:1px solid var(--line);background:var(--card);border-radius:10px;padding:10px 16px;min-width:150px}}
.kpi b{{display:block;font-size:22px}} .kpi span{{color:var(--mute);font-size:13px}}
table{{width:100%;border-collapse:collapse;font-size:14px}}
th{{text-align:left;color:var(--mute);font-weight:600;font-size:12.5px;border-bottom:1px solid var(--line);padding:6px 8px}}
td{{border-bottom:1px solid var(--line);padding:7px 8px;vertical-align:middle}}
.num{{text-align:right;font-variant-numeric:tabular-nums;white-space:nowrap}} .dim{{color:var(--mute);font-size:13px;white-space:nowrap}}
.cost{{display:flex;align-items:center;gap:8px;min-width:200px}} .cost span{{font-variant-numeric:tabular-nums}}
.bar{{flex:1;height:8px;border-radius:4px;background:color-mix(in srgb,var(--line) 70%,transparent);overflow:hidden;max-width:160px}}
.bar i{{display:block;height:100%;background:linear-gradient(90deg,hsl(40,85%,52%),hsl(0,72%,52%))}}
.prompt{{max-width:330px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;color:var(--mute);font-size:13px}}
.tips{{border:1px solid var(--line);background:var(--card);border-radius:10px;padding:6px 18px}}
.tips li{{margin:8px 0}} code{{font:12.5px ui-monospace,Menlo,monospace}}
.note{{color:var(--mute);font-size:13px;margin-top:30px}}
</style>
<main>
<nav><a href="agent-stats.html">Stats</a><a href="agent-map.html">Map</a><a href="agent-logs.html">Logs</a><a class=on href="agent-sessions.html">Sessions</a></nav>
<h1>Your interactive sessions, last {days} days</h1>
<p class=lead>Chats with agents in a terminal (Claude Code, Codex, Grok), kept apart from the automated jobs on the map.
Costs are API-equivalent: a Max plan is not billed per token, but these use up its limits.</p>
<div class=kpis>
<div class=kpi><b>{money(total)}</b><span>API-equivalent cost</span></div>
<div class=kpi><b>{n_total}</b><span>sessions</span></div>
<div class=kpi><b>{mtok(inter.get('total_tokens', 0))}</b><span>tokens (mostly re-read context)</span></div>
<div class=kpi><b>{e(', '.join(model_name(m) for m, _ in models[:2]) or '—')}</b><span>main models</span></div>
</div>
{f'<h2>How to spend less</h2><ul class=tips>{"".join(tips)}</ul>' if tips else ''}
<h2>By agent</h2>
<table><tr><th>Agent</th><th class=num>Sessions</th><th>Cost</th><th class=num>Tokens</th><th class=num>Peak context</th><th>Models</th><th>Last active</th></tr>
{''.join(arows) or '<tr><td colspan=7>No interactive sessions in this window.</td></tr>'}</table>
<h2>By day</h2>
<table><tr><th>Day</th><th>Cost</th><th class=num>Tokens</th><th>Agents that day (top 3)</th></tr>{''.join(drows)}</table>
<h2>Costliest sessions</h2>
<table><tr><th class=num>Cost</th><th>Agent</th><th>When</th><th class=num>Calls</th><th class=num>Peak context</th><th>Models</th><th>First message</th></tr>
{''.join(srows)}</table>
<p class=note>⚠️ peak context over {LONG_CONTEXT // 1000}k tokens. From the Claude Code, Codex and Grok session files on this
Mac; Claude Code keeps 15 days of them. Scan {e(snap.get('generated', '')[:16].replace('T', ' '))}.</p>
</main></html>"""
    (folder / "agent-sessions.html").write_text(page)
