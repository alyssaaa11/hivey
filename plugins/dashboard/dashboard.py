#!/usr/bin/env python3
"""hivey.dashboard: a live terminal dashboard for one hivey swarm.

Shows, refreshed every few seconds:
  - budget: elapsed vs. budget_minutes, as a bar
  - agents: role, state (and for how long), model, queued messages, tokens as a bar
  - token rate: a sparkline of the swarm's tokens per interval
  - tasks: counts per status from .swarm/tasks.json (the /swarm skill's task board)
  - recent messages from .swarm/bus.jsonl

Tokens are summed from each agent's Claude Code transcripts
(~/.claude/projects/*/<session>.jsonl and its subagents), deduplicated by message id,
read incrementally and cached in <root>/.swarm/dashboard_usage.json. Tokens only, no $.

Keys: q quit · r refresh now. Environment: HIVEY_SWARM_ROOT (required), HERDR_BIN_PATH.
"""
import curses
import glob
import json
import os
import subprocess
import sys
import time
from pathlib import Path

PROJECTS = Path.home() / ".claude" / "projects"
CODEX_SESSIONS = Path.home() / ".codex" / "sessions"
FIELDS = ("input_tokens", "output_tokens", "cache_creation_input_tokens", "cache_read_input_tokens")
REFRESH_S = 3
TOKENS_EVERY_S = 10
SPARK = "▁▂▃▄▅▆▇█"
GLYPH = {"master": "◆", "worker": "●", "critic": "✎", "script": "▷"}
TASK_ORDER = ("open", "in-progress", "review", "approved", "blocked")


# ---------------------------------------------------------------------------
# Pure helpers (unit-tested in test_dashboard.py)
# ---------------------------------------------------------------------------

def human(n):
    return f"{n / 1e6:.1f}M" if n >= 1e6 else f"{n / 1e3:.0f}k" if n >= 1e3 else str(int(n))


def bar(value, maximum, width):
    if width <= 0:
        return ""
    filled = 0 if maximum <= 0 else round(width * min(value, maximum) / maximum)
    return "█" * filled + "░" * (width - filled)


def sparkline(values, width):
    values = list(values)[-width:]
    if not values:
        return ""
    top = max(values) or 1
    return "".join(SPARK[min(len(SPARK) - 1, int(v / top * (len(SPARK) - 1)))] for v in values)


def duration(seconds):
    seconds = int(max(0, seconds))
    if seconds < 60:
        return f"{seconds}s"
    if seconds < 3600:
        return f"{seconds // 60}m"
    return f"{seconds // 3600}h{seconds % 3600 // 60:02d}"


def task_counts(tasks):
    counts = {status: 0 for status in TASK_ORDER}
    for task in tasks:
        status = task.get("status", "open")
        counts[status] = counts.get(status, 0) + 1
    return counts


def scan_transcript(path, state):
    """Add usage from lines appended to `path` since the last scan; returns total tokens."""
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            f.seek(state.get("offset", 0))
            while True:
                line = f.readline()
                if not line.endswith("\n"):
                    break
                state["offset"] = f.tell()
                if '"usage"' not in line:
                    continue
                try:
                    msg = json.loads(line).get("message") or {}
                except json.JSONDecodeError:
                    continue
                if not isinstance(msg, dict) or not msg.get("usage") or msg.get("id") == state.get("last_id"):
                    continue
                state["last_id"] = msg.get("id")
                state["total"] = state.get("total", 0) + sum(int(msg["usage"].get(k) or 0) for k in FIELDS)
    except FileNotFoundError:
        pass
    return state.get("total", 0)


def scan_codex_rollout(path, state):
    """Codex rollouts carry a running total in `token_count` events; keep the latest."""
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            f.seek(state.get("offset", 0))
            while True:
                line = f.readline()
                if not line.endswith("\n"):
                    break
                state["offset"] = f.tell()
                if '"token_count"' not in line:
                    continue
                try:
                    payload = json.loads(line).get("payload") or {}
                except json.JSONDecodeError:
                    continue
                usage = ((payload.get("info") or {}).get("total_token_usage") or {})
                if usage.get("total_tokens") is not None:
                    state["total"] = int(usage["total_tokens"])
    except FileNotFoundError:
        pass
    return state.get("total", 0)


def session_files(session):
    """Usage files for one recorded session: "codex:<id>" or a Claude session id."""
    if session.startswith("codex:"):
        sid = session[len("codex:"):]
        return [(p, scan_codex_rollout) for p in glob.glob(str(CODEX_SESSIONS / "*" / "*" / "*" / f"*{sid}.jsonl"))]
    files = glob.glob(str(PROJECTS / "*" / f"{session}.jsonl"))
    files += glob.glob(str(PROJECTS / "*" / session / "subagents" / "*.jsonl"))
    return [(p, scan_transcript) for p in files]


def recent_messages(bus_path, limit):
    try:
        with open(bus_path, "rb") as f:
            f.seek(0, os.SEEK_END)
            f.seek(max(0, f.tell() - 64 * 1024))
            lines = f.read().splitlines()
    except FileNotFoundError:
        return []
    out = []
    for line in lines:
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("ev") == "msg" and not record.get("copy"):
            out.append(record)
    return out[-limit:]


# ---------------------------------------------------------------------------
# Data collection
# ---------------------------------------------------------------------------

class Collector:
    def __init__(self, root, binary):
        self.root, self.binary = Path(root), binary
        self.cache_path = self.root / ".swarm" / "dashboard_usage.json"
        try:
            self.cache = json.loads(self.cache_path.read_text())
        except (OSError, json.JSONDecodeError):
            self.cache = {"sessions": {}, "files": {}}
        self.since = {}  # agent key → (status, first seen at)
        self.rate = []  # swarm tokens gained per token refresh
        self.last_total = None
        self.last_tokens_at = 0
        self.tokens = {}

    def hivey(self, *args):
        out = subprocess.run([self.binary, *args], capture_output=True, text=True)
        try:
            return json.loads(out.stdout)
        except json.JSONDecodeError:
            return {}

    def swarm(self, slug):
        listing = self.hivey("swarm", "list", "--json")
        return next((s for s in listing.get("swarms", []) if s.get("slug") == slug), None)

    def refresh_tokens(self, swarm):
        """Map agents to their Claude Code / Codex sessions (by pane), then sum usage."""
        agents = self.hivey("agent", "list").get("result", {}).get("agents", [])
        session_by_pane = {}
        for a in agents:
            session = a.get("agent_session") or {}
            if session.get("value"):
                prefix = "codex:" if session.get("source") == "herdr:codex" else ""
                session_by_pane[a["pane_id"]] = prefix + session["value"]
        for agent in swarm.get("agents", []):
            sid = session_by_pane.get(agent.get("pane_id"))
            known = self.cache["sessions"].setdefault(agent["key"], [])
            if sid and sid not in known:
                known.append(sid)
        tokens = {}
        for key, sessions in self.cache["sessions"].items():
            total = 0
            for sid in sessions:
                for path, scan in session_files(sid):
                    total += scan(path, self.cache["files"].setdefault(path, {}))
            tokens[key] = total
        tmp = self.cache_path.with_suffix(".tmp")
        tmp.write_text(json.dumps(self.cache))
        tmp.replace(self.cache_path)
        swarm_total = sum(tokens.values())
        if self.last_total is not None:
            self.rate.append(max(0, swarm_total - self.last_total))
            self.rate = self.rate[-120:]
        self.last_total = swarm_total
        self.tokens = tokens

    def track_active(self, launched_at, messages, working, now):
        """Seconds the swarm has actually worked: the clock runs only while an agent is working.

        Kept in the usage cache so it survives restarts. A dashboard started on an existing
        swarm seeds it with launch → last message."""
        if not launched_at:
            return None
        active = self.cache.get("active")
        if active is None:
            last = max((m.get("ts", 0) / 1000 for m in messages), default=launched_at)
            active = self.cache["active"] = {"seconds": max(0, last - launched_at), "at": now}
        if working:
            # Count the time since the last refresh, but not a long gap (dashboard was closed).
            active["seconds"] += min(max(0, now - active["at"]), REFRESH_S * 3)
        active["at"] = now
        return active["seconds"]

    def snapshot(self, slug, force_tokens=False):
        swarm = self.swarm(slug)
        if swarm is None:
            return None
        now = time.time()
        if force_tokens or now - self.last_tokens_at >= TOKENS_EVERY_S:
            self.refresh_tokens(swarm)
            self.last_tokens_at = now
        for agent in swarm.get("agents", []):
            status = agent.get("status")
            if self.since.get(agent["key"], (None,))[0] != status:
                self.since[agent["key"]] = (status, now)
        manifest = {}
        try:
            manifest = json.loads((self.root / ".swarm" / "agents.json").read_text())
        except (OSError, json.JSONDecodeError):
            pass
        tasks = []
        try:
            tasks = json.loads((self.root / ".swarm" / "tasks.json").read_text()).get("tasks", [])
        except (OSError, json.JSONDecodeError):
            pass
        messages = recent_messages(self.root / ".swarm" / "bus.jsonl", 12)
        working = any(a.get("status") == "working" for a in swarm.get("agents", [])
                      if a.get("role") != "script")
        active = self.track_active(manifest.get("launched_at"), messages, working, now)
        return {
            "swarm": swarm,
            "manifest": manifest,
            "tasks": tasks,
            "messages": messages,
            "active": active,
            "working": working,
            "done": bool(tasks) and all(t.get("status") == "approved" for t in tasks),
            "tokens": dict(self.tokens),
            "rate": list(self.rate),
            "since": dict(self.since),
            "now": now,
        }


# ---------------------------------------------------------------------------
# Rendering
# ---------------------------------------------------------------------------

STATE_COLOR = {"working": 3, "idle": 7, "done": 2, "blocked": 1, "gone": 8, "unknown": 7, "script": 8}
ROLE_COLOR = {"master": 4, "worker": 5, "critic": 6, "script": 8}


def init_colors():
    curses.start_color()
    curses.use_default_colors()
    pairs = {1: curses.COLOR_RED, 2: curses.COLOR_GREEN, 3: curses.COLOR_CYAN, 4: curses.COLOR_YELLOW,
             5: curses.COLOR_BLUE, 6: curses.COLOR_MAGENTA, 7: curses.COLOR_WHITE, 8: curses.COLOR_BLACK}
    for pair, color in pairs.items():
        curses.init_pair(pair, color, -1)


def put(win, y, x, text, attr=0):
    height, width = win.getmaxyx()
    if y >= height or x >= width:
        return x
    text = text[: max(0, width - x - 1)]
    try:
        win.addstr(y, x, text, attr)
    except curses.error:
        pass
    return x + len(text)


def color(pair, bold=False):
    attr = curses.color_pair(pair)
    if pair == 8:
        attr |= curses.A_DIM
    return attr | (curses.A_BOLD if bold else 0)


def draw(win, snap, slug):
    win.erase()
    height, width = win.getmaxyx()
    if snap is None:
        put(win, 0, 1, f"swarm {slug!r} not found (hivey swarm list)", color(1))
        win.refresh()
        return
    swarm, manifest, tokens = snap["swarm"], snap["manifest"], snap["tokens"]
    agents = swarm.get("agents", [])
    total = sum(tokens.values())
    if height < len([a for a in agents if a.get("role") != "script"]) + 4:
        draw_compact(win, snap, slug)
        return
    y = 0

    # Header: swarm, agent count, budget bar, total tokens.
    x = put(win, y, 1, f"◆ {slug}", color(4, True))
    x = put(win, y, x, f"  {sum(1 for a in agents if a.get('role') != 'script')} agents", color(7))
    timing = clock(snap)
    if timing:
        label, active, pct = timing
        if pct is None:
            x = put(win, y, x, f"  {label}", color(7))
        else:
            budget = manifest["budget_minutes"]
            x = put(win, y, x, f"  {label}/{budget}m ", color(7))
            x = put(win, y, x, bar(active, budget * 60, 12), color(budget_pair(pct)))
            x = put(win, y, x, f" {int(pct * 100)}%", color(7))
    put(win, y, max(x + 2, width - 18), f"tokens {human(total):>7}", color(3, True))
    y += 2

    # Agents table with token bars.
    put(win, y, 1, "AGENT               STATE            MODEL    ✉  TOKENS", color(8, True))
    y += 1
    top = max(tokens.values(), default=0)
    bar_width = max(3, width - 51 - 9)  # leave room for the number after the bar
    for agent in agents:
        if y >= height - 1:
            break
        if agent.get("role") == "script":
            continue  # addons have their own panes and use no tokens
        key, role = agent.get("key", "?"), agent.get("role", "worker")
        status = agent.get("status", "?")
        seen = snap["since"].get(key, (status, snap["now"]))[1]
        x = put(win, y, 1, f"{GLYPH.get(role, '●')} ", color(ROLE_COLOR.get(role, 7), True))
        put(win, y, x, f"{key[:16]:<16}", color(ROLE_COLOR.get(role, 7), role == "master"))
        state_text = status if role == "script" else f"{status} {duration(snap['now'] - seen)}"
        put(win, y, 21, f"{state_text:<16}", color(STATE_COLOR.get(status, 7), status == "blocked"))
        put(win, y, 38, f"{(agent.get('model') or agent.get('kind') or '')[:8]:<8}", color(7))
        queued = agent.get("queued", 0)
        put(win, y, 47, f"{queued if queued else '':>2}", color(4, True))
        if role != "script":
            used = tokens.get(key, 0)
            x = put(win, y, 51, bar(used, top, bar_width), color(ROLE_COLOR.get(role, 7)))
            put(win, y, x + 1, human(used), color(7))
        y += 1

    # Token rate sparkline.
    y += 1
    put(win, y, 1, "TOKENS / 10s ", color(8, True))
    put(win, y, 14, sparkline(snap["rate"], max(10, width - 30)), color(3))
    if snap["rate"]:
        put(win, y, width - 14, f"last {human(snap['rate'][-1]):>6}", color(7))
    y += 2

    # Task board summary.
    if snap["tasks"]:
        counts = task_counts(snap["tasks"])
        done = counts.get("approved", 0)
        x = put(win, y, 1, "TASKS ", color(8, True))
        x = put(win, y, x, bar(done, len(snap["tasks"]), 16), color(2))
        x = put(win, y, x, f" {done}/{len(snap['tasks'])} approved   ", color(7))
        for status in TASK_ORDER[:-1] + ("blocked",):
            if status == "approved":
                continue
            n = counts.get(status, 0)
            x = put(win, y, x, f"{status} {n}  ", color(1 if status == "blocked" and n else 7))
        y += 2

    # Recent messages.
    put(win, y, 1, "MESSAGES", color(8, True))
    y += 1
    for msg in snap["messages"][-(height - y - 1):]:
        if y >= height - 1:
            break
        when = time.strftime("%H:%M", time.localtime(msg.get("ts", 0) / 1000))
        frm = msg.get("from", "?").split("/")[-1]
        kind = {"fyi": " fyi", "urgent": " URGENT"}.get(msg.get("kind"), "")
        x = put(win, y, 1, f"{when} ", color(8))
        x = put(win, y, x, f"{frm} → {msg.get('to', '?')}{kind}: ", color(4 if kind == " URGENT" else 3))
        put(win, y, x, msg.get("text", "").replace("\n", " "), color(7))
        y += 1
    put(win, height - 1, 1, "q quit · r refresh", color(8))
    win.refresh()


def clock(snap):
    """(label, active seconds, budget fraction or None): time counts only while agents work."""
    active, budget = snap.get("active"), snap["manifest"].get("budget_minutes")
    if active is None:
        return None
    if snap.get("done") and not snap.get("working"):
        label = f"done in {duration(active)}"
    elif snap.get("working"):
        label = f"active {duration(active)}"
    else:
        label = f"idle · active {duration(active)}"
    return label, active, (active / (budget * 60) if budget else None)


def budget_pair(pct):
    return 1 if pct >= 1 else 4 if pct >= 0.8 else 2


def compact_parts(snap, slug):
    """Short-pane summary as (text, color pair, bold) parts: swarm, budget, tokens per agent, total."""
    manifest, tokens = snap["manifest"], snap["tokens"]
    parts = [(f"◆ {slug}", 4, True)]
    timing = clock(snap)
    if timing:
        label, _, pct = timing
        if pct is None:
            parts.append((f"  {label}", 7, False))
        else:
            parts.append((f"  {label}/{manifest['budget_minutes']}m {int(pct * 100)}%",
                          7 if snap.get("done") else budget_pair(pct), False))
    parts.append(("  │", 8, False))
    for agent in snap["swarm"].get("agents", []):
        role = agent.get("role", "worker")
        if role == "script":
            continue
        parts.append((f" {GLYPH.get(role, '●')} {agent.get('key', '?')} ", ROLE_COLOR.get(role, 7), role == "master"))
        parts.append((human(tokens.get(agent.get("key"), 0)), 7, False))
    parts.append(("  │ tokens ", 8, False))
    parts.append((human(sum(tokens.values())), 3, True))
    return parts


def draw_compact(win, snap, slug):
    """One or two rows: everything that matters, tokens per agent included."""
    height, width = win.getmaxyx()
    y, x = 0, 1
    for text, pair, bold in compact_parts(snap, slug):
        if x + len(text) >= width - 1 and y + 1 < height and x > 1:
            y, x = y + 1, 3  # wrap onto the next row when there is one
        x = put(win, y, x, text, color(pair, bold))
    win.refresh()


def run(stdscr, collector, slug):
    curses.curs_set(0)
    init_colors()
    stdscr.timeout(REFRESH_S * 1000)
    force = True
    while True:
        try:
            snap = collector.snapshot(slug, force_tokens=force)
        except Exception as err:  # keep the dashboard alive through transient errors
            snap = None
            put(stdscr, 0, 1, f"error: {err}")
        force = False
        draw(stdscr, snap, slug)
        key = stdscr.getch()
        if key in (ord("q"), ord("Q")):
            return
        if key in (ord("r"), ord("R")):
            force = True


def main():
    root = os.environ.get("HIVEY_SWARM_ROOT") or (sys.argv[1] if len(sys.argv) > 1 else "")
    if not root:
        sys.exit("HIVEY_SWARM_ROOT is not set (open this through `hivey swarm addon <swarm> hivey.dashboard`)")
    manifest = json.loads((Path(root) / ".swarm" / "agents.json").read_text())
    slug = os.environ.get("HIVEY_SWARM_SLUG") or manifest["slug"]
    collector = Collector(root, os.environ.get("HERDR_BIN_PATH", "hivey"))
    curses.wrapper(run, collector, slug)


if __name__ == "__main__":
    main()
