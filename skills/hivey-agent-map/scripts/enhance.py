"""hivey's layer on top of the archify pages, added after they are finalized:

- a navbar (🚀 Fleet · Stats · Map · Logs) on every page, so each page reaches the others;
- on the map: each job/agent box coloured by its spend in the window (green: none, red: the most),
  with a legend; a hover explanation on each column header; and, in the box's info card, what the
  job or agent does, its tokens, cost, calls, models and last run.

Map boxes are tied to the snapshot by `map-meta.json` in the report folder
({"nodes": {"<map node id>": {"refs": ["agent:x-content", "cron:x-ai-agents-trends"],
"description": "…"}}, "stages": {"<stage label>": "…"}}), written by the nightly model; a box
without refs is matched by its label. Python standard library only.
"""
from __future__ import annotations

import html
import json
import math
import re
from pathlib import Path

STAGE_HELP = {
    "scheduled jobs": "Jobs that start on their own at set times (cron or launchd), without anyone asking.",
    "raw data": "What the jobs write: transcripts, Baserow tables, files. Other jobs and agents read them.",
    "pipelines": "Jobs that read the raw data and turn it into something: wiki pages, reports, summaries.",
    "knowledge & agents": "Wikis and vaults the pipelines keep up to date, and the agents that feed others.",
    "knowledge": "Wikis and vaults the pipelines keep up to date.",
    "agents": "hivey agents you talk to: they read the knowledge and data on the left and do the work.",
    "outputs": "What leaves the system: posts, messages, published files.",
}

# The tab icon: hivey's logo (assets/logo.svg), embedded so the pages need no other file.
FAVICON = ('<link rel="icon" type="image/svg+xml" href="data:image/svg+xml,'
           "%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 40 40'%3E%3Crect width='40' height='40' rx='9' "
           "fill='%239befc8'/%3E%3Cpath d='M10 9h7v9h6V9h7v23h-7V22h-6v10h-7z' fill='%23122319'/%3E%3C/svg%3E\">")

NAV_CSS = """
.hv-nav{position:relative;z-index:50;display:flex;flex-wrap:wrap;gap:4px;align-items:center;justify-content:flex-start;
  padding:6px 16px;font:600 13px/1.2 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;
  border-bottom:1px solid rgba(127,127,127,.25);background:rgba(127,127,127,.06)}
.hv-nav a{color:inherit;opacity:.65;text-decoration:none;padding:4px 12px;border-radius:999px}
.hv-nav a:hover{opacity:1;background:rgba(127,127,127,.14)}
.hv-nav a.hv-fleet{opacity:1;font-weight:600;margin-right:8px;border:1px solid rgba(127,127,127,.3)}
.hv-nav a.on{opacity:1;background:rgba(47,111,236,.16);color:#2f6fec}
.hv-nav .hv-span{display:inline-flex;margin-left:12px;border:1px solid rgba(127,127,127,.3);border-radius:999px;padding:2px}
.hv-nav .hv-span a{padding:2px 10px;font-weight:600;font-size:12px}
.hv-nav .hv-when{margin-left:12px;opacity:.55;font-weight:400;font-size:12px;white-space:nowrap}
.hv-nav .hv-refresh{margin-left:8px;font:600 12px/1.2 inherit;font-family:inherit;color:#2f6fec;cursor:pointer;
  background:rgba(47,111,236,.10);border:1px solid rgba(47,111,236,.35);border-radius:999px;padding:4px 11px}
.hv-nav .hv-refresh:hover{background:rgba(47,111,236,.18)} .hv-nav .hv-refresh:disabled{opacity:.6;cursor:wait}
"""

MAP_CSS = """
.hv-legend{position:fixed;left:14px;bottom:14px;z-index:40;padding:8px 10px;border-radius:10px;
  font:12px/1.3 -apple-system,BlinkMacSystemFont,sans-serif;background:rgba(255,255,255,.92);color:#1d2330;
  border:1px solid rgba(127,127,127,.3);box-shadow:0 2px 10px rgba(0,0,0,.08)}
html[data-theme="dark"] .hv-legend{background:rgba(22,27,34,.92);color:#e6edf3}
@media (prefers-color-scheme:dark){html:not([data-theme="light"]) .hv-legend{background:rgba(22,27,34,.92);color:#e6edf3}}
.hv-legend .bar{width:180px;height:9px;border-radius:5px;margin:5px 0 3px;
  background:linear-gradient(90deg,hsl(120,65%,45%),hsl(60,75%,48%),hsl(0,70%,50%))}
.hv-legend .ends{display:flex;justify-content:space-between;opacity:.75}
.hv-info{margin-top:.7rem;padding-top:.6rem;border-top:1px solid rgba(127,127,127,.25);font-size:.78rem;line-height:1.45}
.hv-info .hv-desc{margin:0 0 .5rem;font-size:.85rem}
.hv-info dl{display:grid;grid-template-columns:auto 1fr;gap:.18rem .7rem;margin:0}
.hv-alert > rect:not(.c-mask){stroke-dasharray:5 3;stroke-width:2.2px}
.hv-info a{color:inherit;text-decoration:underline}
.hv-info dt{opacity:.6}.hv-info dd{margin:0;font-variant-numeric:tabular-nums}
.hv-info .hv-models{display:flex;flex-wrap:wrap;gap:.25rem}
.hv-info .hv-models span{padding:.05rem .4rem;border:1px solid rgba(127,127,127,.35);border-radius:999px;font-size:.7rem}
svg text.hv-stage{cursor:help;text-decoration:underline dotted;text-underline-offset:2px}
"""


def span(days: int) -> str:
    """The window in words: "24 hours" or "7 days"."""
    return "24 hours" if days == 1 else f"{days} days"


DAY_DIR = "1d"  # each report's 24-hour pages live in <report>/1d/, the 7-day ones in <report>/


def nav(active: str, generated: str, days: int = 7) -> str:
    links = [("agent-stats.html", "Stats"), ("agent-map.html", "Map"), ("agent-logs.html", "Logs"),
             ("agent-sessions.html", "Sessions")]
    items = "".join(f'<a href="{href}"{" class=on" if href == active else ""}>{label}</a>' for href, label in links)
    when = html.escape(generated[:16].replace("T", " "))
    day = days == 1  # the toggle opens the same page in the other window
    toggle = (f'<span class="hv-span"><a href="{active if day else DAY_DIR + "/" + active}"{" class=on" if day else ""}>24 h</a>'
              f'<a href="{"../" + active if day else active}"{"" if day else " class=on"}>7 days</a></span>')
    # back to the fleet page (it shows these pages in its Jobs tab, where the link hides itself)
    fleet = ('<a class="hv-fleet" href="/fleet/?view=fleet" target="_top" title="The hivey fleet: every live agent now">'
             '🚀 Fleet</a><script>if(window.top!==window)document.currentScript.previousElementSibling.remove()</script>')
    return (f'<nav class="hv-nav">{fleet}{items}{toggle}<span class="hv-when">scan {when}</span>'
            '<button class="hv-refresh" type="button" title="Scan again now and reload (free: no model run)" '
            'onclick="hvRefresh(this)">↻ Refresh</button></nav>' + REFRESH_JS)


# The Refresh button: the pages are served by `hivey agent-map open` (serve.py) on 127.0.0.1,
# which reruns the scan on POST /hv-refresh. Opened as plain files, it says how to get there.
REFRESH_JS = """<script>async function hvRefresh(b){
if(location.protocol==='file:'){b.textContent='open from the hivey menu → agent jobs to refresh';return;}
b.disabled=true;const t0=Date.now();const tick=setInterval(()=>{b.textContent='↻ Scanning… '+Math.round((Date.now()-t0)/1000)+'s';},500);
try{const r=await fetch('/hv-refresh',{method:'POST'});const j=await r.json();clearInterval(tick);
if(j.ok){location.reload();}else{b.disabled=false;b.textContent='refresh failed';b.title=j.error||'';}}
catch(e){clearInterval(tick);b.disabled=false;b.textContent='refresh failed: server not running (reopen from the hivey menu)';}}</script>"""


def inject(page: str, css: str, top: str, script: str = "") -> str:
    """Adds hivey's pieces, each wrapped in <!--hv--> markers so a rerun replaces them."""
    page = re.sub(r"<!--hv-->.*?<!--/hv-->", "", page, flags=re.S)
    page = re.sub(r"<link\b[^>]*\brel=[\"']?(?:shortcut )?icon\b[^>]*>", "", page, flags=re.I)  # hivey's icon only
    page = page.replace("</head>", f"<!--hv-->{FAVICON}<style>{css}</style><!--/hv--></head>", 1)
    m = re.search(r"<body[^>]*>", page)
    if m:
        page = page[: m.end()] + f"<!--hv-->{top}<!--/hv-->" + page[m.end():]
    if script:
        block = f"<!--hv--><script>{script}</script><!--/hv-->"
        page = page.replace("</body>", block + "</body>", 1) if "</body>" in page else page + block
    return page


def money(v: float) -> str:
    return f"${v:,.2f}"


def mtok(n: int) -> str:
    return f"{n / 1e6:,.1f}M" if n >= 1e6 else f"{n / 1e3:,.0f}k" if n >= 1e3 else str(n)


def model_name(m: str) -> str:
    m = m.rsplit("/", 1)[-1]
    k = re.match(r"claude-(opus|sonnet|haiku)-(\d+)(?:-(\d{1,2}))?(?!\d)", m)
    return (f"{k.group(1).title()} {k.group(2)}" + (f".{k.group(3)}" if k.group(3) else "")) if k else m


def job_anchor(job_id: str) -> str:
    """The logs page anchor of a job (same as report.job_anchor)."""
    return "job-" + re.sub(r"[^a-z0-9]+", "-", job_id.lower()).strip("-")


def node_data(snap: dict, spec: dict, meta: dict, job_state) -> dict:
    usage = {a["agent"]: a for a in snap["usage"]["agents"]}
    try:  # what the nightly model (or you) wrote since the scan wins
        overrides = json.loads((Path.home() / ".hivey/agent-map/descriptions.json").read_text())
    except (OSError, ValueError):
        overrides = {}
    jobs = {j["id"]: j for j in snap["jobs"]}
    jobs_by_name = {j["name"]: j for j in snap["jobs"]}
    snodes = {n["id"]: n for n in snap["nodes"]}
    snodes_by_label = {n["label"]: n for n in snap["nodes"]}
    live = {}
    for a in snap["live_agents"]:
        live.setdefault(a["agent"], a)
    out = {}
    for n in spec.get("nodes", []):
        m = (meta.get("nodes") or {}).get(n["id"], {})
        refs = list(m.get("refs") or [])
        if not refs:
            label = n["label"]
            for cand in (label, re.sub(r"\s+(vault|wiki|jobs?|agent)$", "", label)):
                if cand in snodes_by_label:
                    refs.append(snodes_by_label[cand]["id"])
                    break
                if cand in jobs_by_name:
                    refs.append(jobs_by_name[cand]["id"])
                    break
        names, desc, states, code_models, alerts = [], m.get("description", ""), [], [], []
        for ref in refs:
            if overrides.get(ref) and not desc:
                desc = overrides[ref]
            j = jobs.get(ref)
            if j:
                # its own spend only: a job inside an agent's folder must not take the agent's cost
                agent = j.get("agent") or ""
                names += [j["name"]] + ([agent, agent.removeprefix("skill:")] if agent.startswith("skill:") else [])
                desc = desc or j.get("description", "")
                emo, words = job_state(j)
                states.append(f"{emo} {words} · {(j.get('last_run') or j.get('log_modified') or '?').replace('T', ' ')}")
                if emo in ("❌", "⚠️"):  # failed, missed or errors in its log: flag the box
                    alerts.append({"name": j["name"], "words": words, "anchor": job_anchor(j["id"])})
            sn = snodes.get(ref)
            if sn:
                code_models += [m for m in (sn.get("llm_api") or {}).get("models", []) if m not in code_models]
            if sn:
                names.append(sn["label"])
                desc = desc or sn.get("description", "")
                if sn["type"] == "agent" and sn["label"] in live:
                    states.append(f"{live[sn['label']]['status']} in hivey")
        seen, tot, models = set(), {"cost": 0.0, "tokens": 0, "out": 0, "calls": 0}, []
        for name in names:
            if name and name in usage and name not in seen:
                seen.add(name)
                a = usage[name]
                tot["cost"] += a["cost_usd"]
                tot["tokens"] += a["input"] + a["output"] + a["cache_read"] + a["cache_write"]
                tot["out"] += a["output"]
                tot["calls"] += a["calls"]
                models += [model_name(x) for x in a["models"] if model_name(x) not in models]
        # models seen in its code (also local ones that log no tokens), then the ones with recorded tokens
        for mname in models:
            if not any(mname.split()[0].lower() in c.lower() for c in code_models):
                code_models.append(mname)
        out[n["id"]] = {"refs": refs, "desc": desc, "cost": round(tot["cost"], 2), "tokens": tot["tokens"],
                        "out": tot["out"], "calls": tot["calls"], "models": code_models[:5], "states": states[:3], "alerts": alerts if n.get("type") != "database" else [],
                        "subject": bool(refs) and n.get("type") != "database"}
    spend = [d["cost"] for d in out.values() if d["subject"]]
    top = max(spend, default=0.0)
    for d in out.values():
        if d["subject"]:
            t = math.log1p(d["cost"]) / math.log1p(top) if top > 0 else 0.0
            d["hue"] = round(120 * (1 - t))
    return out, top


MAP_JS = r"""
(function(){
const D=__DATA__, STAGES=__STAGES__, SPAN=__SPAN__;
const fmt=v=>'$'+v.toLocaleString(undefined,{minimumFractionDigits:2,maximumFractionDigits:2});
const tok=n=>n>=1e6?(n/1e6).toFixed(1)+'M':n>=1e3?Math.round(n/1e3)+'k':String(n);
const esc=s=>String(s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
document.querySelectorAll('g[data-node-id]').forEach(g=>{
  const d=D[g.dataset.nodeId]; if(!d) return;
  if(d.hue!=null){g.querySelectorAll(':scope > rect:not(.c-mask)').forEach(r=>{
    r.style.fill='hsla('+d.hue+',75%,50%,.18)'; r.style.stroke='hsl('+d.hue+',68%,42%)';});}
  if(d.alerts&&d.alerts.length){
    const lab=g.querySelector(':scope > text[data-node-label]');
    if(lab&&!lab.querySelector('.hv-warn')){const w=document.createElementNS('http://www.w3.org/2000/svg','tspan');
      w.setAttribute('class','hv-warn'); w.textContent=' ⚠️'; lab.appendChild(w);}
    g.classList.add('hv-alert');
  }
  const t=g.querySelector(':scope > title');
  if(t){t.textContent=[g.dataset.nodeLabel, d.desc, d.subject?(fmt(d.cost)+' in '+SPAN+' · '+tok(d.tokens)+' tokens'+(d.models.length?' · '+d.models.join(', '):'')):''].filter(Boolean).concat((d.alerts||[]).map(a=>'⚠️ '+a.name+': '+a.words)).join('\n');}
});
document.querySelectorAll('svg text').forEach(t=>{
  const m=/^\s*\d+\s*\/\s*(.+?)\s*$/.exec(t.textContent||''); if(!m) return;
  const help=STAGES[m[1].toLowerCase()]; if(!help) return;
  t.classList.add('hv-stage');
  const ti=document.createElementNS('http://www.w3.org/2000/svg','title'); ti.textContent=m[1]+': '+help; t.appendChild(ti);
  t.style.pointerEvents='auto';
});
let current=null;
const refill=()=>document.querySelectorAll('.semantic-passport-meta').forEach(fill);
const pick=e=>{const g=e.target.closest&&e.target.closest('g[data-node-id]'); if(g){current=g.dataset.nodeId; setTimeout(refill,0); requestAnimationFrame(refill);}};
document.addEventListener('pointerdown',pick,true); document.addEventListener('focusin',pick,true);
function fill(meta){
  const box=meta.parentElement; if(!box) return;
  const old=box.querySelector('.hv-info');
  if(old&&old.dataset.node===current) return;  // the card is reused for every box: refill on a new pick
  if(old) old.remove();
  const d=D[current]; if(!d) return;
  const rows=[];
  (d.alerts||[]).forEach(a=>rows.push(['⚠️ Problem', esc(a.name)+': '+esc(a.words)+' · <a href="agent-logs.html#'+a.anchor+'">open its log</a>']));
  if(d.subject){
    rows.push(['Spend', fmt(d.cost)+' in '+SPAN+', automated runs only (API-equivalent)']);
    rows.push(['Tokens', tok(d.tokens)+' ('+tok(d.out)+' output) · '+d.calls.toLocaleString()+' calls']);
  }
  rows.push(['Models', d.models.length?'<span class="hv-models">'+d.models.map(m=>'<span>'+esc(m)+'</span>').join('')+'</span>':'none found']);
  d.states.forEach(s=>rows.push(['Last run', esc(s)]));
  const div=document.createElement('div'); div.className='hv-info'; div.dataset.node=current;
  div.innerHTML=(d.desc?'<p class="hv-desc">'+esc(d.desc)+'</p>':'')+(rows.length?'<dl>'+rows.map(r=>'<dt>'+r[0]+'</dt><dd>'+r[1]+'</dd>').join('')+'</dl>':'');
  meta.insertAdjacentElement('afterend',div);
}
new MutationObserver(refill).observe(document.body,{childList:true,subtree:true,characterData:true});
})();
"""


def enhance_folder(folder: Path, snap: dict, job_state) -> None:
    generated, days = snap.get("generated", ""), snap.get("window_days", 7)
    stats = folder / "agent-stats.html"
    if stats.is_file():
        stats.write_text(inject(stats.read_text(), NAV_CSS, nav("agent-stats.html", generated, days)))
    page, spec_path = folder / "agent-map.html", folder / "map.json"
    if page.is_file() and spec_path.is_file():
        spec = json.loads(spec_path.read_text())
        try:
            meta = json.loads((folder / "map-meta.json").read_text())
        except (OSError, ValueError):
            meta = {}
        data, top = node_data(snap, spec, meta, job_state)
        stages = dict(STAGE_HELP)
        stages.update({k.lower(): v for k, v in (meta.get("stages") or {}).items()})
        script = (MAP_JS.replace("__DATA__", json.dumps(data)).replace("__STAGES__", json.dumps(stages))
                  .replace("__SPAN__", json.dumps(span(days))))
        legend = (f'<div class="hv-legend"><b>Spend, last {span(days)}</b><div class="bar"></div>'
                  f'<div class="ends"><span>$0</span><span>{money(top)}</span></div>'
                  f'<div style="opacity:.65;margin-top:3px">click a box for what it does, tokens and models</div>'
                  f'<div style="opacity:.65">⚠️ its last run failed, was missed or logged errors</div></div>')
        page.write_text(inject(page.read_text(), NAV_CSS + MAP_CSS, nav("agent-map.html", generated, days) + legend, script))
