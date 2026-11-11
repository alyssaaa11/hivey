"""The agent office: what every live agent is doing now, drawn as a 3D office of desks.

One desk per agent you talk to in hivey: solo agents, extra sessions in a space, and each swarm's
coordinator (a swarm's workers and critics are folded into their coordinator's desk as its team).
For each desk it reads two things:

  terminal      `hivey pane read <pane>`: the bottom of its screen (what it is doing right now,
                a permission prompt, a question for you), for any CLI (Claude Code, Codex, Grok, agy)
  session log   the Claude Code transcript of its session (~/.claude/projects/*/<session>.jsonl):
                context size (input + cache tokens of the last reply), model, your last prompt, the
                session's recap (away_summary) and its last replies

Haiku (`claude -p --model haiku`, no tools, no session saved) turns them into a short summary per
desk: the current job, a speech-bubble line, what it achieved, what it waits for and whether to keep,
/compact or /clear the conversation. Only desks whose terminal or log changed since the last run are
sent again, in batches, so 100 agents cost a few calls per refresh. Without a model (--no-llm, or the
call fails) the summary comes from the recap and the last prompt.

Output: ~/.hivey/agent-map/reports/office/office.json, read by office.html (Three.js, vendored), served
by serve.py, which reruns this every 5 minutes while the page is open. Python standard library only.

  office.py run [--no-llm] [--quick] [--model haiku]    one refresh (--quick: the 1-minute live pass)
  office.py open                               start the server, refresh if stale, open the page
"""
from __future__ import annotations

import argparse
import base64
import concurrent.futures as cf
import datetime as dt
import glob
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPTS))
import context_window  # noqa: E402

HOME = Path.home()
STATE = HOME / ".hivey" / "agent-map"
OUT = STATE / "reports" / "office"
CACHE = STATE / "office-cache.json"
LOCK = STATE / "office.lock"
RUN_DIR = STATE / "office-run"  # neutral cwd for the model calls (no CLAUDE.md, no project)

STANDARD_WINDOW = 200_000
LARGE_WINDOW = 1_000_000
SCREEN_LINES = 70
SCREEN_CHARS = 3500
BATCH = 10
STALE_SECONDS = 240  # `open` refreshes first when the data is older than this
ANSI = re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*(\x07|\x1b\\)")
BOX = re.compile(r"[─━│┃┌┐└┘├┤┬┴┼╭╮╯╰═║╔╗╚╝╠╣╦╩╬╌╍┄┅]+")
CLIS = {"claude": "Claude Code", "codex": "Codex", "grok": "Grok", "agy": "Antigravity", "gemini": "Antigravity"}


def hivey_bin() -> str | None:
    exe = os.environ.get("HIVEY_BIN")
    return exe if exe and os.path.isfile(exe) else shutil.which("hivey")


INHERITED = ("HERDR_SOCKET_PATH", "HERDR_CLIENT_SOCKET_PATH", "HERDR_SESSION", "HERDR_PANE_ID", "HERDR_TAB_ID",
             "HERDR_WORKSPACE_ID", "HERDR_PANE_RUNTIME_ID")


def hv_env(sock: str | None) -> dict | None:
    """The environment that points a hivey command at one session's server (None: inherit ours)."""
    if not sock:
        return None
    env = {k: v for k, v in os.environ.items() if k not in INHERITED}
    env["HERDR_SOCKET_PATH"] = sock
    return env


def sh(cmd: list[str], timeout: int = 15, sock: str | None = None) -> str:
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=hv_env(sock)).stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def short(path: str | None) -> str:
    return (path or "").replace(str(HOME), "~", 1)


def now_iso() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds")


# ---------------------------------------------------------------- who is in the office

def hv_sessions(hv: str) -> list[dict]:
    """Every named hivey session: [{name, running, default, socket}] (the default one first)."""
    try:
        got = json.loads(sh([hv, "session", "list", "--json"]) or "{}").get("sessions") or []
    except (ValueError, AttributeError):
        got = []
    out = [{"name": s.get("name") or "default", "running": bool(s.get("running")), "default": bool(s.get("default")),
            "socket": s.get("socket_path"), "dir": s.get("session_dir")} for s in got if isinstance(s, dict)]
    if not out:  # an older hivey without sessions: the server we are attached to
        out = [{"name": "default", "running": True, "default": True, "socket": None}]
    out.sort(key=lambda s: (not s["default"], not s["running"], s["name"]))
    return out


def spaces(hv: str, sock: str | None = None) -> dict:
    """workspace id → {label, emoji, name, about}."""
    out = {}
    try:
        data = json.loads(sh([hv, "workspace", "list"], sock=sock) or "{}")
    except ValueError:
        return out
    for w in data.get("result", {}).get("workspaces", []):
        label = w.get("label") or w.get("workspace_id", "")
        m = re.match(r"^(\W+?)\s+(.+)$", label)
        tok = w.get("tokens") or {}
        about = " ".join(v.split("  ", 1)[-1].strip() for k, v in sorted(tok.items())
                         if k.startswith("info") and v and (v.startswith("about") or v[:1].islower() and "about" not in v
                                                            and not re.match(r"^(★|channel|running|folder|addons)\b", v)))
        out[w.get("workspace_id")] = {"label": label, "emoji": m.group(1).strip() if m else "🐝",
                                      "name": m.group(2) if m else label, "about": about.strip()[:200]}
    return out


def desks(hv: str, hs: dict | None = None) -> list[dict]:
    """One entry per agent you talk to; swarm workers/critics/scripts fold into their coordinator.
    hs: the hivey session to read (default: the one we are attached to)."""
    hs = hs or {"name": "default", "default": True, "socket": None}
    sock = hs.get("socket")
    try:
        data = json.loads(sh([hv, "agent", "list"], sock=sock) or "{}")
    except ValueError:
        return []
    agents = data.get("result", {}).get("agents", [])
    rooms = spaces(hv, sock)
    team: dict[str, list] = {}
    for a in agents:
        tok = a.get("tokens") or {}
        if tok.get("swarm") and tok.get("role") not in (None, "master"):
            team.setdefault(tok["swarm"], []).append({"name": a.get("name") or tok.get("role"),
                                                      "role": tok.get("role"), "status": a.get("agent_status")})
    out, per_space = [], {}
    for a in agents:
        tok = a.get("tokens") or {}
        if tok.get("swarm") and tok.get("role") not in (None, "master"):
            continue
        ws = a.get("workspace_id", "")
        room = rooms.get(ws, {"label": ws, "emoji": "🐝", "name": ws, "about": ""})
        per_space[ws] = per_space.get(ws, 0) + 1
        master = tok.get("role") == "master"
        name = a.get("name") or tok.get("swarm") or room["name"]
        if not master:
            name = f"{room['name']} · session {per_space[ws]}"
        members = team.get(tok.get("swarm"), []) if master else []
        sess = a.get("agent_session") or {}
        title = re.sub(r"^\W+\s*", "", a.get("title") or "")
        out.append({
            "id": a.get("pane_id") if hs.get("default") else f"{hs['name']}/{a.get('pane_id')}",
            "pane": a.get("pane_id"), "hv_session": hs["name"], "sock": sock, "workspace": ws, "space": room["name"],
            "space_label": room["label"], "emoji": room["emoji"], "about": room["about"] if master else "",
            "name": name, "role": "coordinator" if members else ("agent" if master else "session"),
            "cli": CLIS.get(a.get("agent"), a.get("agent") or "?"), "cli_id": a.get("agent"),
            "status": a.get("agent_status") or "unknown", "cwd": short(a.get("foreground_cwd") or a.get("cwd")),
            "folder": a.get("foreground_cwd") or a.get("cwd"), "session": sess.get("value"),
            "model_hint": title.rsplit("·", 1)[-1].strip() if "·" in title else "",
            "team": members, "team_working": sum(1 for m in members if m["status"] == "working"),
            "focused": bool(a.get("focused")), "queued": bool(tok.get("queued")),
        })
    for d in out:  # the only desk in a space is just the space
        if d["role"] == "session" and per_space.get(d["workspace"]) == 1:
            d["name"] = d["space"]
    return out


def asleep_desks(hs: dict) -> list[dict]:
    """The agents of a stopped hivey session, from the layout it saved (session.json): they sit powered down
    at their desks; their context and last work still come from their session logs."""
    try:
        path = Path(hs.get("dir") or "") / "session.json"
        saved = json.loads(path.read_text())
        stopped_at = dt.datetime.fromtimestamp(path.stat().st_mtime, dt.timezone.utc).isoformat(timespec="seconds")
    except (OSError, ValueError, TypeError):
        return []
    out = []
    for w in saved.get("workspaces") or []:
        if not isinstance(w, dict):
            continue
        ws, label = w.get("id", ""), w.get("custom_name") or w.get("id", "")
        m = re.match(r"^(\W+?)\s+(.+)$", label)
        emoji, space = (m.group(1).strip(), m.group(2)) if m else ("🐝", label)
        panes = [(pid, p) for t in w.get("tabs") or [] for pid, p in (t.get("panes") or {}).items()
                 if isinstance(p, dict) and (p.get("agent_session") or {}).get("value")]
        leads = [x for x in panes if not x[1].get("managed_agent_kind")] or panes[:1]
        crew = [x for x in panes if x not in leads]
        for n, (pid, p) in enumerate(leads, 1):
            sess = p["agent_session"]
            members = [{"name": c.get("agent_name") or f"pane {cid}", "role": (c.get("agent_name") or "").rsplit("-", 1)[-1] or "worker",
                        "status": "idle"} for cid, c in crew] if n == 1 else []
            name = p.get("agent_name") or (space if len(leads) == 1 else f"{space} · session {n}")
            pane = f"{ws}:p{pid}"
            out.append({
                "id": f"{hs['name']}/{pane}", "pane": pane, "hv_session": hs["name"], "sock": None, "asleep": True,
                "stopped_at": stopped_at, "workspace": f"{hs['name']}/{ws}", "space": space, "space_label": label, "emoji": emoji, "about": "",
                "name": name, "role": "coordinator" if members else "agent",
                "cli": CLIS.get(sess.get("agent"), sess.get("agent") or "?"), "cli_id": sess.get("agent"),
                "status": "idle", "cwd": short(p.get("cwd")), "folder": p.get("cwd"), "session": sess.get("value"),
                "model_hint": "", "team": members, "team_working": 0, "focused": False, "queued": False,
            })
    return out


# ---------------------------------------------------------------- its wiki: newest pages and log entries

WIKI_SKIP = {"raw", ".obsidian", ".trash", ".git", "node_modules", "assets", "attachments", "templates", "_templates"}
WIKI_META = {"log.md", "index.md", "claude.md", "agents.md", "ingest-ledger.md", "readme.md"}


def obsidian_vaults() -> list[str]:
    """Folders Obsidian knows as vaults (pages inside them open with obsidian://open)."""
    try:
        got = json.loads((HOME / "Library/Application Support/obsidian/obsidian.json").read_text())
        return [os.path.realpath(v["path"]) for v in (got.get("vaults") or {}).values() if v.get("path")]
    except (OSError, ValueError, AttributeError, TypeError):
        return []


def wiki_dir(folder: str | None, space: str = "") -> Path | None:
    """The agent's or swarm's own wiki: a `<name>-wiki/` folder in its folder (or the folder itself); when its
    terminal sits one level up (~/swarms), the folder named after its space."""
    bases = [Path(folder)] if folder and os.path.isdir(folder) else []
    if space and re.fullmatch(r"[\w.-]{1,64}", space):
        bases += [b / space for b in bases] + [HOME / "agents" / space, HOME / "swarms" / space]
    for base in bases:
        if not base.is_dir():
            continue
        for cand in [base] + sorted(base.glob("*-wiki")):
            if cand.is_dir() and cand.name.endswith("-wiki") and any((cand / n).exists() for n in ("index.md", "log.md", "wiki")):
                return cand
    return None


def wiki_info(folder: str | None, vaults: list[str], space: str = "") -> dict | None:
    """The newest pages and reports the agent wrote in its wiki and its latest log entries."""
    root = wiki_dir(folder, space)
    if not root:
        return None
    pages, seen = [], 0
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d.lower() not in WIKI_SKIP and not d.startswith(".")]
        for f in filenames:
            if not f.endswith(".md") or f.lower() in WIKI_META or f.endswith(".excalidraw.md"):
                continue
            seen += 1
            p = os.path.join(dirpath, f)
            try:
                pages.append((os.path.getmtime(p), p))
            except OSError:
                pass
        if seen > 20000:
            break
    recent = []
    for mtime, p in sorted(pages, reverse=True)[:8]:
        title = Path(p).stem.replace("-", " ")
        try:
            with open(p, encoding="utf-8", errors="replace") as fh:
                for line in fh.read(4000).splitlines():
                    if line.startswith("# "):
                        title = line[2:].strip()
                        break
        except OSError:
            pass
        rel = os.path.relpath(p, root)
        recent.append({"path": rel, "title": title[:120], "folder": os.path.dirname(rel),
                       "at": dt.datetime.fromtimestamp(mtime, dt.timezone.utc).isoformat(timespec="seconds")})
    log = []
    for lp in (root / "log.md", root / "wiki" / "log.md"):
        try:
            text = lp.read_text(errors="replace")[-20000:]
        except OSError:
            continue
        for m in re.finditer(r"^## \[([^\]]+)\]\s*(.+)$", text, re.M):
            log.append({"date": m.group(1).strip(), "what": m.group(2).strip()[:140]})
        log = log[-5:][::-1]
        break
    real = os.path.realpath(root)
    vault = next((v for v in vaults if real == v or real.startswith(v + os.sep)), None)
    return {"dir": str(root), "short": short(str(root)), "vault": bool(vault), "pages": len(pages),
            "recent": recent, "log": log}


# ---------------------------------------------------------------- what each one shows

def screen(hv: str, pane: str, sock: str | None = None) -> str:
    """The bottom of the pane's terminal, cleaned for reading (no colors, no box lines, no blank runs)."""
    raw = sh([hv, "pane", "read", pane, "--source", "recent", "--lines", str(SCREEN_LINES)], timeout=10, sock=sock)
    lines = []
    for line in ANSI.sub("", raw).splitlines():
        line = BOX.sub(" ", line).rstrip()
        line = re.sub(r"\s{3,}", "  ", line)
        if line.strip() or (lines and lines[-1]):
            lines.append(line)
    text = "\n".join(lines).strip()
    return text[-SCREEN_CHARS:]


def transcript_of(session: str | None, cli: str | None) -> Path | None:
    if not session or cli != "claude" or not re.fullmatch(r"[\w-]{8,80}", session):
        return None
    hits = glob.glob(str(HOME / ".claude/projects" / "*" / f"{session}.jsonl"))
    return Path(max(hits, key=os.path.getmtime)) if hits else None


def text_of(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(c.get("text", "") for c in content if isinstance(c, dict) and c.get("type") == "text")
    return ""


def tool_line(c: dict) -> str:
    inp = c.get("input") or {}
    arg = inp.get("description") or inp.get("command") or inp.get("file_path") or inp.get("pattern") \
        or inp.get("prompt") or inp.get("url") or ""
    return f"{c.get('name')}: {str(arg)[:110]}"


def session_log(path: Path | None) -> dict:
    """Context size, model, last prompt, recap, last replies and tools from the tail of a transcript."""
    if not path:
        return {}
    try:
        size = path.stat().st_size
        with open(path, "rb") as fh:
            fh.seek(max(0, size - 1_500_000))
            tail = fh.read().decode("utf-8", "replace").splitlines()[1:]
    except OSError:
        return {}
    info = {"log": short(str(path)), "context": 0, "model": "", "last_prompt": "", "recap": "", "replies": [],
            "tools": [], "prompts": [], "last_activity": None, "started": None, "uuid": ""}
    for line in tail:
        try:
            d = json.loads(line)
        except ValueError:
            continue
        kind = d.get("type")
        ts = d.get("timestamp")
        if kind == "last-prompt":
            info["last_prompt"] = (d.get("lastPrompt") or "")[:400]
            continue
        if kind == "system" and d.get("subtype") == "compact_boundary":
            # /compact (or auto-compact) shrank the context; until the next reply this is its size
            meta = d.get("compactMetadata") if isinstance(d.get("compactMetadata"), dict) else {}
            if meta.get("postTokens"):
                info["context"] = int(meta["postTokens"])
            info["compacted_at"] = d.get("timestamp")
            continue
        if kind == "system" and d.get("subtype") == "away_summary":
            info["recap"] = re.sub(r"\n+DONE:.*$", "", d.get("content") or "", flags=re.S).strip()[:600]
            continue
        msg = d.get("message") or {}
        if kind == "assistant":
            u = msg.get("usage") or {}
            if u and msg.get("model") != "<synthetic>":
                info["context"] = (u.get("input_tokens", 0) or 0) + (u.get("cache_read_input_tokens", 0) or 0) \
                    + (u.get("cache_creation_input_tokens", 0) or 0)
                info["model"] = msg.get("model", "") or info["model"]
            for c in msg.get("content") or []:
                if not isinstance(c, dict):
                    continue
                if c.get("type") == "text" and c.get("text", "").strip():
                    info["replies"].append(c["text"].strip()[:700])
                elif c.get("type") == "tool_use":
                    info["tools"].append(tool_line(c))
        elif kind == "user" and not d.get("isMeta"):
            t = text_of(msg.get("content")).strip()
            if t and not t.startswith("<") and "tool_result" not in t:
                info["prompts"].append(t[:300])
        if kind in ("assistant", "user") and ts:
            info["last_activity"] = ts
            info["started"] = info["started"] or ts
            info["uuid"] = d.get("uuid", "")
    info["replies"] = info["replies"][-4:]
    info["tools"] = info["tools"][-8:]
    info["prompts"] = info["prompts"][-3:]
    return info


def window_for(desk: dict, ctx: int) -> int:
    folder = Path(desk["folder"]) if desk.get("folder") and os.path.isdir(desk["folder"]) else None
    w = LARGE_WINDOW if context_window.window(folder) == context_window.LARGE else STANDARD_WINDOW
    model = (desk.get("model") or "").lower()
    if "sonnet" in model or "haiku" in model:
        w = STANDARD_WINDOW if ctx <= STANDARD_WINDOW else LARGE_WINDOW
    return LARGE_WINDOW if ctx > w else w


# ---------------------------------------------------------------- the summaries

PROMPT = """You are the receptionist of an office of AI coding agents. For each agent below you get its
status, its terminal screen (bottom of the screen = now) and, for Claude Code agents, facts from its session
log. Write what a busy manager needs at a glance. Plain words, no jargon, no markdown.

Return ONLY a JSON array, one object per agent, same ids, with these keys:
  "id": the agent id as given
  "job": its current job in 3-9 words (what it is working on now, or the last thing if idle)
  "bubble": one sentence it would say, first person, max 90 characters, may start with one fitting emoji
  "done": up to 3 short things it achieved in this session (max 60 characters each), newest first
  "next": what happens next or what it waits for, max 80 characters ("" if nothing)
  "needs_you": if it waits for the human (a permission prompt, a question, a choice): what to answer, max 80
               characters; otherwise ""
  "advice": "keep", "compact" or "clear" for its conversation. compact = the context is large (over ~60%)
            and the work continues; clear = the task is finished and the next task is unrelated or the
            context is large and stale; keep otherwise. Never clear while it is working or waiting for you.
  "advice_why": max 70 characters

Agents:
"""


def fingerprint(desk: dict) -> str:
    key = json.dumps([desk["status"], desk.get("screen", "")[-1500:], desk.get("log_uuid", "")])
    return hashlib.sha1(key.encode()).hexdigest()[:16]


def brief(desk: dict) -> str:
    parts = [f"### id={desk['id']}  name={desk['name']}  cli={desk['cli']}  status={desk['status']}"
             f"  context={desk['context_pct']}% of {desk['window'] // 1000}k"]
    if desk.get("about"):
        parts.append(f"about: {desk['about']}")
    if desk["team"]:
        parts.append("swarm team: " + ", ".join(f"{m['name']} ({m['status']})" for m in desk["team"]))
    log = desk.get("_log") or {}
    if log.get("recap"):
        parts.append(f"session recap: {log['recap']}")
    if log.get("last_prompt"):
        parts.append(f"human's last prompt: {log['last_prompt']}")
    for r in log.get("replies", [])[-2:]:
        parts.append(f"agent said: {r[:400]}")
    if log.get("tools"):
        parts.append("recent tools: " + " | ".join(log["tools"][-5:]))
    parts.append("terminal (bottom = now):\n" + (desk.get("screen") or "(empty)")[-2200:])
    return "\n".join(parts)


def claude_token() -> str | None:
    try:
        import report  # the nightly's long-lived login, when one is set
        RUN_DIR.mkdir(parents=True, exist_ok=True)
        return report.claude_token(RUN_DIR)
    except Exception:  # noqa: BLE001 - the normal login is fine
        return None


def ask_model(batch: list[dict], model: str, token: str | None) -> list[dict]:
    claude = shutil.which("claude") or str(HOME / ".local/bin/claude")
    prompt = PROMPT + "\n\n".join(brief(d) for d in batch)
    env = dict(os.environ)
    for k in list(env):
        if k.startswith(("HERDR_", "CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT")):
            env.pop(k)
    if token:
        env["CLAUDE_CODE_OAUTH_TOKEN"] = token
    RUN_DIR.mkdir(parents=True, exist_ok=True)
    p = subprocess.run([claude, "-p", "--model", model, "--tools", "", "--setting-sources", "",
                        "--strict-mcp-config", "--no-session-persistence", "--output-format", "text"],
                       input=prompt, cwd=RUN_DIR, capture_output=True, text=True, timeout=240, env=env)
    m = re.search(r"\[.*\]", p.stdout, re.S)
    if p.returncode != 0 or not m:
        raise RuntimeError((p.stderr or p.stdout).strip()[-300:] or f"exit {p.returncode}")
    return [x for x in json.loads(m.group(0)) if isinstance(x, dict) and x.get("id")]


def first_sentence(text: str, limit: int) -> str:
    text = re.sub(r"\s+", " ", text or "").strip()
    m = re.match(r"(.+?[.!?])(\s|$)", text)
    s = m.group(1) if m else text
    return s if len(s) <= limit else s[: limit - 1].rstrip() + "…"


def fallback(desk: dict) -> dict:
    """A summary without a model: the recap, the last prompt and the last reply."""
    log = desk.get("_log") or {}
    scr = desk.get("screen", "")
    needs = ""
    if desk["status"] == "blocked":
        q = re.findall(r"(Do you want[^\n]*|[^\n]*\?\s*$)", scr, re.M)
        needs = first_sentence(q[-1], 80) if q else "it is waiting for you in its terminal"
    reply = log.get("replies", [""])[-1] if log.get("replies") else ""
    lines = [ln.strip() for ln in scr.splitlines() if len(ln.strip()) > 20]
    return {
        "job": first_sentence(log.get("last_prompt") or (lines[-1] if lines else desk["name"]), 60),
        "bubble": first_sentence(log.get("recap") or reply or (lines[-1] if lines else "…"), 90),
        "done": [], "next": "", "needs_you": needs,
        "advice": "compact" if desk["context_pct"] >= 70 and desk["status"] not in ("working", "blocked") else "keep",
        "advice_why": f"context at {desk['context_pct']}%" if desk["context_pct"] >= 70 else "",
        "by": "fallback",
    }


def clean_summary(s: dict) -> dict:
    out = {k: str(s.get(k) or "")[:limit] for k, limit in
           (("job", 80), ("bubble", 110), ("next", 100), ("needs_you", 100), ("advice_why", 90))}
    out["done"] = [str(x)[:80] for x in (s.get("done") or []) if x][:3]
    out["advice"] = s.get("advice") if s.get("advice") in ("keep", "compact", "clear") else "keep"
    return out


# ---------------------------------------------------------------- one refresh

def collect(hv: str, sessions: list[dict] | None = None) -> list[dict]:
    """Every agent of every running hivey session (one spaceship per session on the page)."""
    people = [d for hs in (sessions or hv_sessions(hv))
              for d in (desks(hv, hs) if hs["running"] else asleep_desks(hs))]
    with cf.ThreadPoolExecutor(max_workers=8) as pool:
        screens = list(pool.map(lambda d: "" if d.get("asleep") else screen(hv, d["pane"], d.get("sock")), people))
    vaults = obsidian_vaults()
    for d, scr in zip(people, screens):
        d["screen"] = scr
        d["wiki"] = wiki_info(d.get("folder"), vaults, d.get("space", ""))
        log = session_log(transcript_of(d["session"], d["cli_id"]))
        d["_log"] = log
        d["log_uuid"] = log.get("uuid", "")
        d["model"] = log.get("model") or d["model_hint"]
        d["context"] = log.get("context", 0)
        d["window"] = window_for(d, d["context"])
        d["context_pct"] = round(100 * d["context"] / d["window"]) if d["context"] else 0
        d["last_activity"] = log.get("last_activity")
        d["session_started"] = log.get("started")
        d["log"] = log.get("log", "")
        d["last_prompt"] = log.get("last_prompt", "")
        d["recap"] = log.get("recap", "")
        d["compacted_at"] = log.get("compacted_at")
    return people


def run(model: str = "haiku", use_llm: bool = True, quick: bool = False) -> int:
    hv = hivey_bin()
    if not hv:
        print("office: hivey not found", file=sys.stderr)
        return 1
    STATE.mkdir(parents=True, exist_ok=True)
    try:
        fd = os.open(LOCK, os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        os.write(fd, str(os.getpid()).encode())
        os.close(fd)
    except FileExistsError:
        if time.time() - LOCK.stat().st_mtime < 600:
            print("office: a refresh is already running")
            return 0
        LOCK.unlink(missing_ok=True)
        return run(model, use_llm, quick)
    try:
        return _run(hv, model, use_llm, quick)
    finally:
        LOCK.unlink(missing_ok=True)


def _run(hv: str, model: str, use_llm: bool, quick: bool = False) -> int:
    """quick: the 1-minute live pass. Status, context and screen are fresh; a desk whose terminal moved
    but whose status did not keeps its last summary (re-summarized at the next full pass), and only
    desks whose status changed (blocked -> working, working -> done...) are sent to the model now."""
    start = time.time()
    sessions = hv_sessions(hv)
    people = collect(hv, sessions)
    try:
        cache = json.loads(CACHE.read_text())
    except (OSError, ValueError):
        cache = {}
    todo, error, stale = [], "", {}
    for d in people:
        d["fp"] = fingerprint(d)
        hit = cache.get(d["id"])
        if d.get("asleep") and not d["log_uuid"]:  # stopped, and its session log is gone: nothing to summarize
            day = dt.datetime.fromisoformat(d["stopped_at"]).astimezone().strftime("%b %-d")
            crew = f" with {len(d['team'])} teammates" if d["team"] else ""
            d["summary"] = {"job": f"Session {d['hv_session']} stopped", "bubble": f"💤 Asleep since {day}{crew}. Click to start session {d['hv_session']}.",
                            "next": f"Start session {d['hv_session']} to wake it", "needs_you": "", "done": [],
                            "advice": "keep", "advice_why": "its session is stopped", "by": "stopped"}
            d["summarized_at"] = d["stopped_at"]
        elif hit and hit.get("fp") == d["fp"] and (hit.get("by") != "fallback" or not use_llm):
            d["summary"] = hit["summary"]
            d["summarized_at"] = hit.get("at")
        elif quick and hit and hit.get("status", d["status"]) == d["status"] and hit.get("by") != "fallback":
            d["summary"] = hit["summary"]
            d["summarized_at"] = hit.get("at")
            stale[d["id"]] = hit  # keep its old fingerprint so the next full pass re-summarizes it
        else:
            todo.append(d)
    if todo and use_llm:
        token = claude_token()
        batches = [todo[i:i + BATCH] for i in range(0, len(todo), BATCH)]
        with cf.ThreadPoolExecutor(max_workers=4) as pool:
            futures = {pool.submit(ask_model, b, model, token): b for b in batches}
            for fut in cf.as_completed(futures):
                try:
                    got = {s["id"]: s for s in fut.result()}
                except Exception as e:  # noqa: BLE001 - keep the page alive with the fallback
                    error, got = str(e)[:300], {}
                for d in futures[fut]:
                    if d["id"] in got:
                        d["summary"] = {**clean_summary(got[d["id"]]), "by": model}
                        d["summarized_at"] = now_iso()
    for d in people:
        if "summary" not in d:
            d["summary"] = fallback(d)
            d["summarized_at"] = now_iso()
        s = d["summary"]
        if d["context_pct"] >= 85 and s.get("advice") == "keep":  # a model can be too gentle
            s["advice"], s["advice_why"] = "compact", f"context at {d['context_pct']}%"
        if d["context_pct"] < 40 and s.get("advice") == "compact":  # already compacted since that summary
            s["advice"], s["advice_why"] = "keep", f"context at {d['context_pct']}% after compacting" \
                if d.get("compacted_at") else f"context at {d['context_pct']}%"
        if d["status"] in ("working", "blocked") and s.get("advice") == "clear":
            s["advice"] = "keep"
    CACHE.write_text(json.dumps({d["id"]: stale.get(d["id"]) or {
        "fp": d["fp"], "summary": d["summary"], "at": d["summarized_at"], "by": d["summary"].get("by"),
        "status": d["status"]} for d in people}))
    for d in people:
        log = d.pop("_log", {})
        d["tools"] = log.get("tools", [])[-5:]
        d["screen"] = "\n".join(d["screen"].splitlines()[-30:])
        for k in ("folder", "model_hint", "log_uuid", "fp"):
            d.pop(k, None)
    order = {"blocked": 0, "working": 1, "done": 2, "idle": 3}
    people.sort(key=lambda d: (order.get(d["status"], 4), -d["context_pct"], d["name"]))
    counts = {k: sum(1 for d in people if d["status"] == k and not d.get("asleep"))
              for k in ("working", "blocked", "done", "idle")}
    counts["asleep"] = sum(1 for d in people if d.get("asleep"))
    full_at = now_iso()
    if quick:  # a live pass: the last full summary pass stays the one already on the page
        try:
            full_at = json.loads((OUT / "office.json").read_text()).get("full_at") or full_at
        except (OSError, ValueError):
            pass
    data = {"generated": now_iso(), "kind": "quick" if quick else "full", "full_at": full_at, "seconds": round(time.time() - start, 1), "model": model if use_llm else None,
            "summarized": len(todo) if use_llm else 0, "error": error, "refresh_every": 60,
            "counts": counts, "agents": people,
            "sessions": [{"name": hs["name"], "running": hs["running"], "default": hs["default"],
                          "agents": sum(1 for d in people if d["hv_session"] == hs["name"])} for hs in sessions],
            "advice": {k: sum(1 for d in people if d["summary"].get("advice") == k and not d.get("asleep"))
                       for k in ("compact", "clear")}}
    write(data)
    print(f"office: {len(people)} desks, {len(todo) if use_llm else 0} summarized"
          f"{' (' + error + ')' if error else ''} in {data['seconds']}s → {OUT / 'index.html'}")
    return 0


def write(data: dict) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    assets = SCRIPTS / "office"  # the page and its modules (Three.js, post-processing, the ship)
    for src in assets.rglob("*"):
        if not src.is_file() or src.name.startswith("."):
            continue
        rel = src.relative_to(assets)
        blob = src.read_bytes()
        if rel.suffix == ".b64":  # binary models travel base64 (skills install text files)
            rel, blob = rel.with_suffix(""), base64.b64decode(blob)
        dst = OUT / ("index.html" if rel.as_posix() == "office.html" else rel)
        if not dst.is_file() or dst.read_bytes() != blob:
            dst.parent.mkdir(parents=True, exist_ok=True)
            dst.write_bytes(blob)
    tmp = OUT / "office.json.tmp"
    tmp.write_text(json.dumps(data, ensure_ascii=False))
    os.replace(tmp, OUT / "office.json")


def open_page(model: str) -> int:
    import serve
    data = OUT / "office.json"
    fresh = data.is_file() and time.time() - data.stat().st_mtime < STALE_SECONDS
    if not fresh:
        run(model, use_llm=False)  # seconds: desks and terminal lines now, summaries follow
    port = serve.ensure()
    if not port:
        print("office: the local page server did not start (see ~/.hivey/agent-map/server.log)", file=sys.stderr)
        return 1
    if not fresh:
        serve.request_office_refresh(port)
    url = f"http://127.0.0.1:{port}/office/"
    subprocess.run(["open", url], capture_output=True)
    print(f"agent fleet: {url}")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="office.py", description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd")
    r = sub.add_parser("run")
    r.add_argument("--no-llm", action="store_true")
    r.add_argument("--quick", action="store_true", help="live pass: summarize only desks whose status changed")
    r.add_argument("--model", default="haiku")
    o = sub.add_parser("open")
    o.add_argument("--model", default="haiku")
    a = ap.parse_args(argv)
    if a.cmd == "run":
        return run(a.model, not a.no_llm, a.quick)
    if a.cmd == "open":
        return open_page(a.model)
    ap.print_help()
    return 0


if __name__ == "__main__":
    sys.exit(main())
