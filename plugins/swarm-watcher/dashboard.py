#!/usr/bin/env python3
"""Live, animated Gantt dashboard for a swarm → a self-contained HTML page.

Animated with CSS + a small inline script (works from file://, no deps): running bars
and the "now" line grow in real time between reloads, active tasks show moving stripes,
working agents pulse, blocked items glow, and the intro sweep plays only on first open
(the 60 s reload keeps scroll position and doesn't replay it).

Built from .swarm/agents.json (roster, launch time, budget), .swarm/tasks.json (task board)
and .swarm/events.jsonl (claims, status changes, agent states, alerts). No LLM, no deps —
the hivey.swarm-watcher addon rebuilds it every minute. Output: <launch_dir>/diagrams/dashboard-swarm-<slug>.html
(launch_dir from the manifest, else the root), or --out PATH.

Usage: dashboard.py <swarm_root> [--out PATH] [--open]
"""
import argparse
import html
import json
import subprocess
import time
from datetime import datetime
from pathlib import Path

TASK_COLORS = {"open": "#6b7280", "in-progress": "#3b82f6", "review": "#f59e0b",
               "blocked": "#ef4444", "approved": "#22c55e"}
STATE_COLORS = {"working": "#3b82f6", "idle": "#4b5563", "done": "#4b5563",
                "blocked": "#ef4444", "gone": "#1f2937", "unknown": "#374151"}


def ts(s):
    return datetime.strptime(s, "%Y-%m-%dT%H:%M:%S").timestamp()


def load(root):
    sw = Path(root) / ".swarm"
    m = json.loads((sw / "agents.json").read_text())
    tasks = json.loads((sw / "tasks.json").read_text())["tasks"] if (sw / "tasks.json").exists() else []
    events = []
    if (sw / "events.jsonl").exists():
        for line in (sw / "events.jsonl").read_text().splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return m, tasks, events


def segments(changes, start, now):
    """[(t, value)] sorted → [(t0, t1, value, live)]; the last one runs to now (live=True)."""
    out = []
    for i, (t, v) in enumerate(changes):
        live = i + 1 == len(changes)
        end = now if live else changes[i + 1][0]
        if end > t and t >= start - 1:
            out.append((t, end, v, live))
    return out


def dur(sec):
    mins = round(sec / 60)
    return f"{mins // 60}h {mins % 60:02d}m" if mins >= 60 else f"{mins}m"


def build(root, out=None):
    m, tasks, events = load(root)
    now = time.time()
    start = m.get("launched_at") or (ts(events[0]["ts"]) if events else now - 60)
    budget = m.get("budget_minutes")
    end = max(now, start + budget * 60 if budget else now) + 60
    span = max(end - start, 60)

    # task status timelines + agent state timelines from the event log
    task_changes, agent_changes = {}, {}
    for e in events:
        t = ts(e["ts"])
        if e["type"] == "task_claimed":
            task_changes.setdefault(e["task"], []).append((t, "in-progress"))
        elif e["type"] == "task_status":
            task_changes.setdefault(e["task"], []).append((t, e["status"]))
        elif e["type"] == "agent_state":
            agent_changes.setdefault(e["agent"], []).append((t, e["status"]))

    # finished = every task approved; the swarm's run time ends at the last approval
    approvals = [ts(e["ts"]) for e in events if e["type"] == "task_status" and e.get("status") == "approved"]
    finished_at = max(approvals) if approvals and tasks and all(t["status"] == "approved" for t in tasks) else None
    first = m.get("first_launched_at") or start
    until = finished_at or now
    # agent time = time each agent spent `working` (all launches, not just the latest)
    work = {a: sum(t1 - t0 for t0, t1, st, _ in segments(sorted(agent_changes.get(a, [])), 0, until)
                   if st == "working") for a in m["agents"]}
    total_work = sum(work.values())

    last_act = {}
    for e in events:
        if e["type"] == "delivered":
            continue
        for who in {e.get("actor"), e.get("agent")} & set(m["agents"]):
            last_act[who] = max(last_act.get(who, 0), ts(e["ts"]))
    up = Path(root) / ".swarm" / "usage.json"
    usage = json.loads(up.read_text()) if up.exists() else {}
    per_tok = usage.get("per_agent", {})
    hum = lambda n: f"{n / 1e6:.1f}M" if n >= 1e6 else f"{n / 1e3:.0f}k" if n >= 1e3 else str(n)
    tok = lambda a: hum(per_tok[a]["total"]) + " tok" if a in per_tok else "— tok"
    ago = lambda a: (f'<tspan class="ago" data-t="{last_act[a]:.0f}">active '
                     f'{int((now - last_act[a]) // 60)} m ago</tspan>' if a in last_act else "no activity yet")

    W, LABEL, ROW, BAR = 1100, 190, 26, 16
    x = lambda t: LABEL + (t - start) / span * (W - LABEL - 20)
    rows, y, n = [], 40, 0

    def delay():  # stagger the intro sweep
        nonlocal n
        n += 1
        return f' style="--d:{n * 45}ms"'

    for agent in m["agents"]:
        info = m["agents"][agent]
        rows.append(f'<text x="{LABEL - 8}" y="{y + 17}" class="tok" text-anchor="end">{tok(agent)}'
                    f'<title>{agent}: {per_tok.get(agent, {}).get("output", 0):,} output tokens</title></text>')
        rows.append(f'<text x="8" y="{y + 17}" class="agent">{html.escape(agent)}</text>'
                    f'<text x="8" y="{y + 30}" class="sub">{html.escape(str(info.get("model") or ""))} · '
                    f'⏱ {dur(work[agent])} · {ago(agent)}</text>')
        for t0, t1, st, live in segments(sorted(agent_changes.get(agent, [])), start, now):
            lv = f' data-live="1" data-x0="{x(t0):.1f}"' if live else ""
            rows.append(f'<rect class="strip st-{st}"{lv}{delay()} x="{x(t0):.1f}" y="{y + 4}" '
                        f'width="{max(x(t1) - x(t0), 1):.1f}" height="8" rx="2" '
                        f'fill="{STATE_COLORS.get(st, "#374151")}"><title>{agent}: {st}</title></rect>')
        y += ROW + 10
        for task in [t for t in tasks if t.get("owner") == agent]:
            segs = segments(sorted(task_changes.get(task["id"], [])), start, now)
            rows.append(f'<text x="24" y="{y + 13}" class="task">{html.escape(task["id"])} · '
                        f'{html.escape(task["title"][:22])}</text>')
            for t0, t1, st, live in segs:
                if st == "approved":
                    rows.append(f'<path class="diamond" d="M{x(t0):.1f},{y + 1} l7,7 l-7,7 l-7,-7z" '
                                f'fill="{TASK_COLORS["approved"]}"><title>{task["id"]} approved</title></path>')
                    break
                lv = f' data-live="1" data-x0="{x(t0):.1f}"' if live else ""
                geo = f'x="{x(t0):.1f}" y="{y}" width="{max(x(t1) - x(t0), 2):.1f}" height="{BAR}" rx="3"'
                rows.append(f'<rect class="bar st-{st}"{lv}{delay()} {geo} fill="{TASK_COLORS.get(st, "#6b7280")}">'
                            f'<title>{task["id"]}: {st}</title></rect>')
                if live and st in ("in-progress", "review"):  # moving stripes on active work
                    rows.append(f'<rect class="bar"{lv}{delay()} {geo} fill="url(#stripes)" pointer-events="none"/>')
            if not segs:
                rows.append(f'<text x="{LABEL}" y="{y + 13}" class="sub">not started'
                            f'{" · waits on " + ",".join(task["deps"]) if task["deps"] else ""}</text>')
            y += ROW
        y += 8
    unowned = [t for t in tasks if not t.get("owner") or t["owner"] not in m["agents"]]
    for task in unowned:
        rows.append(f'<text x="24" y="{y + 13}" class="task">{html.escape(task["id"])} · unassigned · '
                    f'{html.escape(task["title"][:40])}</text>')
        y += ROW

    # axis: ticks every 10/30/60 min depending on span
    step = 600 if span <= 3 * 3600 else 1800 if span <= 8 * 3600 else 3600
    ticks = "".join(f'<line x1="{x(t):.1f}" y1="28" x2="{x(t):.1f}" y2="{y}" class="grid"/>'
                    f'<text x="{x(t):.1f}" y="22" class="tick">{time.strftime("%H:%M", time.localtime(t))}</text>'
                    for t in range(int(start // step + 1) * step, int(end), step))
    marks = f'<line id="nowline" x1="{x(now):.1f}" y1="28" x2="{x(now):.1f}" y2="{y}" class="now"/>' \
            f'<text id="nowtext" x="{x(now) + 4:.1f}" y="36" class="nowt">now</text>'
    if budget:
        bx = x(start + budget * 60)
        marks += f'<line x1="{bx:.1f}" y1="28" x2="{bx:.1f}" y2="{y}" class="budget"/>' \
                 f'<text x="{bx - 4:.1f}" y="48" class="budt" text-anchor="end">budget</text>'
    defs = ('<defs><pattern id="stripes" width="12" height="12" patternUnits="userSpaceOnUse" '
            'patternTransform="rotate(45)"><rect width="6" height="12" fill="rgba(255,255,255,.18)"/>'
            '<animateTransform attributeName="patternTransform" type="translate" from="0 0" to="12 0" '
            'additive="sum" dur="0.8s" repeatCount="indefinite"/></pattern></defs>')
    svg = f'<svg viewBox="0 0 {W} {y + 10}" width="100%">{defs}{ticks}{marks}{"".join(rows)}</svg>'

    # header stats, agent cards, recent events
    counts = {s: sum(t["status"] == s for t in tasks) for s in TASK_COLORS}
    used = (until - start) / 60
    fmt = lambda t: time.strftime("%b %d %H:%M", time.localtime(t))
    done_banner = (f'<div class="done">✅ Swarm finished in <b>{dur(finished_at - first)}</b> '
                   f'({fmt(first)} → last approval {fmt(finished_at)}) · agents worked '
                   f'<b>{dur(total_work)}</b> in total</div>' if finished_at else "")
    last_state = {a: (sorted(c)[-1][1] if c else "unknown") for a, c in agent_changes.items()}
    cards = "".join(
        f'<div class="card"><b>{html.escape(a)}</b><span class="pill st-{last_state.get(a, "unknown")}" style="background:'
        f'{STATE_COLORS.get(last_state.get(a, "unknown"), "#374151")}">{last_state.get(a, "unknown")}</span>'
        f'<div class="sub">{html.escape(str(v.get("model") or ""))} · '
        f'{sum(t.get("owner") == a and t["status"] == "approved" for t in tasks)}/'
        f'{sum(t.get("owner") == a for t in tasks)} tasks approved · '
        f'<b class="tokc">{tok(a)}</b> (out {hum(per_tok.get(a, {}).get("output", 0))}) · '
        f'⏱ <b>{dur(work[a])}</b> working · '
        f'{ago(a).replace("tspan", "span")}</div>'
        f'<div class="sub">now: {html.escape(next((t["id"] + " " + t["title"][:30] for t in tasks if t.get("owner") == a and t["status"] in ("in-progress", "blocked", "review")), "—"))}</div></div>'
        for a, v in m["agents"].items())
    def describe(e):
        k = e["type"]
        if k == "task_status":
            return f'{e["task"]} → {e["status"]}' + (f' ({e["note"]})' if e.get("note") else "")
        if k == "task_claimed":
            return f'claimed {e["task"]}'
        if k == "agent_state":
            return f'is {e["status"]}'
        if k == "slack":
            return f'Slack {e.get("kind", "")}: {e.get("text", "")}'
        if k in ("check_pass", "check_fail"):
            return f'{k.replace("_", " ")} {e["task"]}'
        return f'{k} {e.get("why") or e.get("task") or ""}'
    feed = "".join(
        f'<li style="--i:{i}"><span class="sub">{e["ts"][11:16]}</span> <b>{html.escape(str(e.get("actor", "")))}</b> '
        f'{html.escape(describe(e)[:110])}</li>'
        for i, e in enumerate(reversed([e for e in events if e["type"] != "delivered"][-18:])))
    parts = [("output", "#f472b6"), ("input", "#a78bfa"), ("cache_write", "#60a5fa"), ("cache_read", "#334155")]
    who_list = sorted(per_tok, key=lambda w: -per_tok[w]["total"])
    top = max([per_tok[w]["total"] for w in who_list] or [1]) or 1
    bars = []
    for i, w in enumerate(who_list):
        u, xx, yy = per_tok[w], 150.0, 8 + i * 28
        bars.append(f'<text x="8" y="{yy + 13}" class="agent">{html.escape(w)}</text>')
        for k, c in parts:
            wdt = u.get(k, 0) / top * 760
            if wdt >= 0.5:
                bars.append(f'<rect class="bar" style="--d:{i * 80}ms" x="{xx:.1f}" y="{yy}" width="{wdt:.1f}" '
                            f'height="16" rx="2" fill="{c}"><title>{w} {k}: {u.get(k, 0):,}</title></rect>')
            xx += wdt
        bars.append(f'<text x="{xx + 8:.1f}" y="{yy + 13}" class="task">{hum(u["total"])} · out {hum(u["output"])}'
                    f' · {html.escape(", ".join(x.replace("claude-", "") for x in u.get("models", [])))}</text>')
    tok_legend = "".join(f'<span class="pill" style="background:{c}">{k.replace("_", " ")}</span>' for k, c in parts)
    swarm_tot = usage.get("swarm_total", {})
    tok_panel = (f'<div class="panel"><div class="sub" style="margin-bottom:6px">🔥 Tokens by agent · swarm total '
                 f'<b>{hum(swarm_tot.get("total", 0))}</b> (output {hum(swarm_tot.get("output", 0))}) · '
                 f'updated with the dashboard · {tok_legend}</div>'
                 f'<svg viewBox="0 0 1100 {len(who_list) * 28 + 12}" width="100%">{"".join(bars)}</svg></div>'
                 if who_list else "")
    diagrams = sorted((Path(m.get("launch_dir") or root) / "diagrams").glob(f"*swarm-{m['slug']}*/*.html"),
                      key=lambda p: p.stat().st_mtime)
    latest = f'<a href="file://{diagrams[-1]}">latest Archify diagram ↗</a>' if diagrams else "no Archify diagram yet"
    legend = "".join(f'<span class="pill" style="background:{c}">{s}</span>' for s, c in TASK_COLORS.items())

    page = f"""<!doctype html><html><head><meta charset="utf-8">
<title>swarm-{html.escape(m['slug'])} · dashboard</title><style>
body{{background:#0b1020;color:#e5e7eb;font:14px -apple-system,system-ui,sans-serif;margin:24px}}
h1{{font-size:20px;margin:0 0 4px}} .sub{{color:#9ca3af;font-size:12px}} .stats span{{margin-right:14px}}
.cards{{display:flex;flex-wrap:wrap;gap:10px;margin:14px 0}} .card{{background:#111827;border:1px solid #1f2937;border-radius:10px;padding:10px 12px;min-width:210px}}
.pill{{display:inline-block;border-radius:9px;padding:1px 8px;margin-left:6px;font-size:11px;color:#fff}}
.panel{{background:#111827;border:1px solid #1f2937;border-radius:10px;padding:12px;margin-top:12px}}
svg text{{fill:#e5e7eb}} .agent{{font-weight:700;font-size:13px}} .task{{font-size:11px;fill:#cbd5e1}} svg .sub{{fill:#9ca3af;font-size:10px}}
.tick{{font-size:10px;fill:#9ca3af;text-anchor:middle}} .grid{{stroke:#1f2937}} .now{{stroke:#f472b6;stroke-width:2}}
.nowt{{fill:#f472b6;font-size:10px}} .budget{{stroke:#fbbf24;stroke-dasharray:4 3}} .budt{{fill:#fbbf24;font-size:10px}}
ul{{list-style:none;padding:0;margin:0}}
.done{{margin-top:10px;padding:8px 12px;border-radius:10px;background:rgba(34,197,94,.12);border:1px solid #22c55e;color:#bbf7d0}}
.tok{{font-size:11px;fill:#f9a8d4;font-weight:700}} .tokc{{color:#f9a8d4}}
.fresh{{border-radius:9px;padding:1px 8px;color:#fff;background:#15803d}} .fresh.warn{{background:#b45309}} .fresh.stale{{background:#b91c1c;animation:pulse .8s infinite}}
.ago.warn{{fill:#fbbf24;color:#fbbf24}} .ago.stale{{fill:#f87171;color:#f87171;font-weight:700}}
/* animation */
.bar,.strip{{transform-box:fill-box;transform-origin:left center}}
.intro .bar,.intro .strip{{animation:grow .9s cubic-bezier(.2,.8,.2,1) both;animation-delay:var(--d,0ms)}}
.diamond{{transform-box:fill-box;transform-origin:center}} .intro .diamond{{animation:pop .5s 1s both}}
@keyframes grow{{from{{transform:scaleX(0)}}}}
@keyframes pop{{0%{{transform:scale(0)}}70%{{transform:scale(1.5)}}100%{{transform:scale(1)}}}}
.st-blocked{{animation:glow 1.4s ease-in-out infinite}}
.intro .st-blocked{{animation:grow .9s cubic-bezier(.2,.8,.2,1) both,glow 1.4s ease-in-out 1s infinite}}
@keyframes glow{{50%{{filter:drop-shadow(0 0 6px #ef4444);opacity:.75}}}}
.pill.st-working{{animation:pulse 1.6s ease-in-out infinite}} .pill.st-blocked{{animation:pulse 0.8s ease-in-out infinite}}
@keyframes pulse{{50%{{box-shadow:0 0 0 5px rgba(59,130,246,0);opacity:.6}}}}
.now{{animation:nowp 2s ease-in-out infinite}} @keyframes nowp{{50%{{opacity:.45}}}}
.intro li{{animation:slide .45s ease-out both;animation-delay:calc(var(--i) * 45ms + 300ms)}}
@keyframes slide{{from{{opacity:0;transform:translateX(-12px)}}}}
@media (prefers-reduced-motion:reduce){{*{{animation:none!important}}}} li{{padding:2px 0;font-size:12px}} a{{color:#93c5fd}}
</style></head><body>
<h1>🐝 swarm-{html.escape(m['slug'])}</h1>
<div class="sub"><span id="fresh" class="fresh" data-built="{now:.0f}">updated {time.strftime("%H:%M:%S")}</span>
 · data rebuilt every 60 s by the relay · {latest}</div>
{done_banner}
<div class="stats" style="margin-top:10px"><span>⏱ <b id="elapsed">{round(used)}</b> min{f" / {budget} min budget ({int(used / budget * 100)}%)" if budget else ""}</span>
<span>🤖 agent time: <b>{dur(total_work)}</b></span>
{"".join(f"<span>{s}: <b>{n}</b></span>" for s, n in counts.items())}
<span>🔥 tokens: <b>{hum(usage.get("swarm_total", {}).get("total", 0))}</b>{f" (master {hum(per_tok['coordinator']['total'])})" if "coordinator" in per_tok else ""}</span></div>
<div class="cards">{cards}</div>
<div class="panel"><div class="sub" style="margin-bottom:6px">Gantt · top strip per agent = agent state (working / idle / blocked) · bars = tasks by status · ◆ approved &nbsp; {legend}</div>{svg}</div>
{tok_panel}
<div class="panel"><div class="sub" style="margin-bottom:6px">Recent events</div><ul>{feed}</ul></div>
<script>
(function(){{
  const START={start}, SPAN={span}, W={W}, LABEL={LABEL}, FINISHED={"true" if finished_at else "false"};
  const X=t=>LABEL+(t-START)/SPAN*(W-LABEL-20);
  const KEY="swarm-{html.escape(m['slug'])}";
  const headless=/HeadlessChrome/.test(navigator.userAgent);  // screenshots: always the final state
  if(!headless && location.hash!=="#nointro" && !sessionStorage.getItem(KEY+":seen")){{
    document.body.classList.add("intro"); sessionStorage.setItem(KEY+":seen","1");
    setTimeout(()=>document.body.classList.remove("intro"),2500);  // never leave bars mid-animation
  }}
  const y=sessionStorage.getItem(KEY+":scroll"); if(y) window.scrollTo(0,+y);
  addEventListener("beforeunload",()=>sessionStorage.setItem(KEY+":scroll",scrollY));
  function tick(){{  // grow running bars + move "now" in real time between rebuilds
    const now=Date.now()/1000, xn=X(now);
    document.querySelectorAll("[data-live]").forEach(r=>r.setAttribute("width",Math.max(xn-parseFloat(r.dataset.x0),2).toFixed(1)));
    const l=document.getElementById("nowline"); if(l){{l.setAttribute("x1",xn);l.setAttribute("x2",xn);}}
    const tx=document.getElementById("nowtext"); if(tx) tx.setAttribute("x",xn+4);
    const el=document.getElementById("elapsed"); if(el && !FINISHED) el.textContent=Math.floor((now-START)/60);
    const f=document.getElementById("fresh");  // how old is this data?
    if(f){{const s=Math.max(0,Math.round(now-f.dataset.built));
      f.textContent=s<90?`updated ${{s}} s ago`:`updated ${{Math.floor(s/60)}} m ago`+(s>=300?" — stale, is the relay running?":"");
      f.className="fresh"+(s>=300?" stale":s>=120?" warn":"");}}
    document.querySelectorAll(".ago[data-t]").forEach(a=>{{const mnt=Math.floor((now-a.dataset.t)/60);
      a.textContent=`active ${{mnt<1?"<1":mnt}} m ago`; a.classList.toggle("warn",mnt>=10&&mnt<30); a.classList.toggle("stale",mnt>=30);}});
  }}
  setInterval(tick,1000); tick();
  setTimeout(()=>location.reload(),60000);  // pick up the relay's rebuild
}})();
</script>
</body></html>"""
    out = Path(out) if out else Path(m.get("launch_dir") or root) / "diagrams" / f"dashboard-swarm-{m['slug']}.html"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(page)
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("root")
    p.add_argument("--out")
    p.add_argument("--open", action="store_true", help="open in Google Chrome")
    args = p.parse_args()
    out = build(Path(args.root).expanduser().resolve(), args.out)
    print(out)
    if args.open:
        import shutil
        if shutil.which("open"):  # macOS: Chrome when installed, else the default browser
            r = subprocess.run(["open", "-a", "Google Chrome", str(out)])
        else:
            r = subprocess.run(["xdg-open", str(out)])
        if r.returncode and shutil.which("open"):
            subprocess.run(["open", str(out)])


if __name__ == "__main__":
    main()
