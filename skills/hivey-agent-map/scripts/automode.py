#!/usr/bin/env python3
"""Auto Mode: an agent keeps working toward a goal for a set time (1, 2, 4 or 10 hours) with a supervisor.

Turning it on (fleet page: 🤖 Auto on the agent, or `hivey agent-map auto start <desk> --hours 2`) opens an
"autopilot" tab in the agent's space running Claude (Fable, your subscription) in its own folder
~/.hivey/auto/<slug>/, with a brief (CLAUDE.md, from autopilot_brief.md) that makes it the agent's
supervisor until the time is up: it sets the goal and a task list, gets the agent a fresh start on Fable,
hands work to the agent or to workers (a Claude in the cloud, preferred, or a local worker pane in its own
folder, like a mini swarm) on the model the task needs, answers their questions within the guardrails,
reviews every finished turn and gives the next piece of work. When the goal is reached or the time is up it
writes what was achieved and what is missing to the agent's wiki and to its own MEMORY.md (read next time).

A watcher process (`auto watch <id>`, started by `start`) follows the agent's and the workers' panes and
types an event into the supervisor when one finishes a turn or asks something, checks in when nothing
happened for a while, says when the time is up and closes the tab after the wrap-up.

State: ~/.hivey/agent-map/auto/<id>.json. Standard library only.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time
from pathlib import Path

HOME = Path.home()
STATE_DIR = HOME / ".hivey/agent-map/auto"
AUTO_HOME = HOME / ".hivey/auto"  # the supervisors' own folders: ~/.hivey/auto/<slug>/
FLEET_JSON = HOME / ".hivey/agent-map/reports/fleet/fleet.json"
BRIEF = Path(__file__).with_name("autopilot_brief.md")
FABLE = "claude-fable-5-2"  # the latest Fable; interactive Claude runs on the user's subscription
HOURS = (1, 2, 4, 10)
POLL = 10           # seconds between the watcher's looks
CHECK_IN = 25 * 60  # a check-in for the supervisor after this long without an event
WRAP_WAIT = 30 * 60  # how long the wrap-up may take before the tab is closed anyway
ACTIVE = ("running", "wrapping")
NAME = re.compile(r"^[a-z0-9][a-z0-9-]{0,40}$")


# ---------------------------------------------------------------- helpers

def hivey_bin() -> str:
    return os.environ.get("HIVEY_BIN") or shutil.which("hivey") or str(HOME / ".local/bin/hivey")


def hv(*args: str, sock: str | None = None, timeout: int = 15) -> subprocess.CompletedProcess:
    env = None
    if sock:
        env = {k: v for k, v in os.environ.items() if not k.startswith("HERDR_")}
        env["HERDR_SOCKET_PATH"] = sock
    return subprocess.run([hivey_bin(), *args], capture_output=True, text=True, timeout=timeout, env=env)


def hv_json(*args: str, sock: str | None = None) -> dict:
    try:
        return json.loads(hv(*args, sock=sock).stdout or "{}").get("result") or {}
    except (ValueError, OSError, subprocess.SubprocessError):
        return {}


def now() -> float:
    return time.time()


def iso(t: float) -> str:
    return dt.datetime.fromtimestamp(t).astimezone().isoformat(timespec="minutes")


def slugify(text: str) -> str:
    return re.sub(r"-+", "-", re.sub(r"[^a-z0-9]+", "-", text.lower())).strip("-")[:40] or "agent"


def state_path(sid: str) -> Path:
    return STATE_DIR / f"{sid}.json"


def load(sid: str) -> dict | None:
    try:
        return json.loads(state_path(sid).read_text())
    except (OSError, ValueError):
        return None


def save(st: dict) -> None:
    STATE_DIR.mkdir(parents=True, exist_ok=True)
    tmp = state_path(st["id"]).with_suffix(".tmp")
    tmp.write_text(json.dumps(st, indent=2))
    os.replace(tmp, state_path(st["id"]))


def all_states() -> list[dict]:
    out = []
    for f in sorted(STATE_DIR.glob("*.json")):
        try:
            out.append(json.loads(f.read_text()))
        except (OSError, ValueError):
            continue
    return out


def active_for(desk_id: str) -> dict | None:
    return next((s for s in all_states() if s.get("desk_id") == desk_id and s.get("status") in ACTIVE), None)


def summary(desk_id: str) -> dict | None:
    """What the fleet page shows on an agent's card (None when Auto Mode is off)."""
    st = active_for(desk_id)
    if not st:
        return None
    return {"id": st["id"], "status": st["status"], "ends": iso(st["ends"]), "left": max(0, int(st["ends"] - now())),
            "hours": st["hours"], "goal": st.get("goal", ""), "sheet": st.get("sheet", ""),
            "workers": [{"name": w["name"], "kind": w["kind"], "model": w.get("model", "")} for w in st.get("workers", [])]}


def auto_panes() -> set[str]:
    """Panes Auto Mode runs (supervisors, local workers): the fleet page folds them into their agent."""
    out = set()
    for st in all_states():
        if st.get("status") in ACTIVE:
            out.add(st.get("sup_pane", ""))
            out.update(w.get("pane", "") for w in st.get("workers", []) if w.get("kind") == "local")
    return {p for p in out if p}


def desk(desk_id: str) -> dict | None:
    try:
        agents = json.loads(FLEET_JSON.read_text()).get("agents", [])
    except (OSError, ValueError):
        return None
    return next((a for a in agents if a.get("id") == desk_id), None)


def trust(folder: Path) -> None:
    """Mark a folder trusted in ~/.claude.json, as starting Claude Code there and saying yes does, so the
    supervisor and workers never stop at the "do you trust this folder?" question."""
    f = HOME / ".claude.json"
    try:
        data = json.loads(f.read_text())
    except (OSError, ValueError):
        return
    projects = data.setdefault("projects", {})
    key = str(folder)
    if projects.get(key, {}).get("hasTrustDialogAccepted"):
        return
    projects.setdefault(key, {})["hasTrustDialogAccepted"] = True
    tmp = f.with_name(".claude.json.hivey-auto.tmp")
    tmp.write_text(json.dumps(data, indent=2))
    os.replace(tmp, f)


def pane_status(pane: str, sock: str | None) -> str:
    got = hv_json("pane", "get", pane, sock=sock).get("pane") or {}
    return got.get("agent_status") or ("gone" if not got else "unknown")


def type_into(pane: str, text: str, sock: str | None) -> bool:
    """Type a message into a pane's prompt and press Enter."""
    if hv("pane", "send-text", pane, text, sock=sock).returncode != 0:
        return False
    time.sleep(0.4)
    return hv("pane", "send-keys", pane, "Enter", sock=sock).returncode == 0


def notify(title: str, body: str) -> None:
    hv("notification", "show", "--title", title, "--body", body[:240])


# ---------------------------------------------------------------- start / stop

def start(desk_id: str, hours: float, goal: str = "") -> tuple[bool, str]:
    d = desk(desk_id)
    if not d:
        return False, "unknown agent (refresh the fleet page)"
    if d.get("outside") or d.get("asleep"):
        return False, "Auto Mode needs an agent running in hivey (start its session first)"
    if d.get("cli_id") != "claude":
        return False, "Auto Mode works with Claude Code agents"
    if hours not in HOURS:
        return False, f"hours: one of {', '.join(map(str, HOURS))}"
    if active_for(desk_id):
        return False, "it is already in Auto Mode"
    sock, pane = d.get("sock"), d["pane"]
    if d.get("hibernated"):
        hv("hibernate", "wake", pane, sock=sock, timeout=30)
    name = d["name"].split(" · ")[0].replace(" (outside hivey)", "")
    slug = slugify(name)
    sid = f"{slug}-{dt.datetime.now().strftime('%Y%m%d-%H%M')}"
    sup = AUTO_HOME / slug
    for sub in ("reports", "workers"):
        (sup / sub).mkdir(parents=True, exist_ok=True)
    folder = os.path.expanduser(d.get("folder") or d.get("cwd") or "")  # fleet.json keeps the short cwd
    if not folder or not os.path.isdir(folder) or Path(folder) == HOME:
        return False, "could not tell which folder the agent works in"
    wiki = (d.get("wiki") or {}).get("dir") or ""
    st = {"id": sid, "desk_id": desk_id, "name": name, "slug": slug, "pane": pane, "sock": sock,
          "workspace": d.get("workspace"), "folder": folder, "wiki": wiki, "sup_folder": str(sup),
          "hours": hours, "goal": goal.strip()[:2000], "started": now(), "ends": now() + hours * 3600,
          "status": "running", "workers": [], "sheet": "", "sup_pane": "", "sup_tab": ""}
    (sup / "CLAUDE.md").write_text(brief(st))
    trust(sup)
    made = hv_json("tab", "create", "--workspace", d["workspace"], "--cwd", str(sup), "--label", "🤖 autopilot",
                   "--no-focus", sock=sock)
    sup_pane = (made.get("root_pane") or {}).get("pane_id")
    if not sup_pane:
        return False, "could not open the autopilot tab"
    st["sup_pane"], st["sup_tab"] = sup_pane, (made.get("tab") or {}).get("tab_id", "")
    save(st)
    cmd = (f"claude --model {FABLE} --dangerously-skip-permissions --add-dir {shlex.quote(folder)}"
           + (f" --add-dir {shlex.quote(wiki)}" if wiki else "")
           + f" -- {shlex.quote('Auto Mode is on for ' + name + '. Start with step 1 of your CLAUDE.md.')}")  # after --: --add-dir takes several
    hv("pane", "run", sup_pane, cmd, sock=sock)
    log = STATE_DIR / f"{sid}.log"
    subprocess.Popen([sys.executable, "-I", __file__, "watch", sid], stdin=subprocess.DEVNULL,
                     stdout=log.open("a"), stderr=subprocess.STDOUT, start_new_session=True)
    return True, f"Auto Mode on for {hours:g} h: its supervisor is in the 🤖 autopilot tab of {name}'s space"


def update(sid: str, **fields) -> dict | None:
    """Change some fields on a fresh read, so writers (watcher, supervisor tools) don't undo each other."""
    st = load(sid)
    if st:
        st.update(fields)
        save(st)
    return st


def stop(sid: str, reason: str = "user") -> tuple[bool, str]:
    st = load(sid)
    if not st or st.get("status") not in ACTIVE:
        return False, "Auto Mode is not on"
    with (STATE_DIR / f"{sid}.inbox").open("a") as f:  # the watcher passes it on to the supervisor
        f.write(json.dumps({"stop": reason}) + "\n")
    return True, "Auto Mode is stopping: its supervisor writes what it achieved, then the tab closes"


def done(sid: str, goal_reached: bool = False, note: str = "") -> tuple[bool, str]:
    st = load(sid)
    if not st:
        return False, "unknown Auto Mode run"
    st = update(sid, status="finished", finished=now(), goal_reached=goal_reached, note=note[:500]) or st
    notify(f"🤖 {st['name']}: Auto Mode finished", ("Goal reached. " if goal_reached else "") + (note or "See its report."))
    return True, "finished"


# ---------------------------------------------------------------- the supervisor's tools

def fresh(sid: str) -> tuple[bool, str]:
    """The agent's fresh start: /clear its conversation and switch it to Fable (it must be idle)."""
    st = load(sid)
    if not st:
        return False, "unknown Auto Mode run"
    status = pane_status(st["pane"], st["sock"])
    if status in ("working", "blocked"):
        return False, f"the agent is {status}; wait until it is idle"
    type_into(st["pane"], "/clear", st["sock"])
    time.sleep(3)
    type_into(st["pane"], f"/model {FABLE}", st["sock"])
    return True, "cleared and switched to Fable"


def target(st: dict, who: str) -> str | None:
    if who in ("", "agent"):
        return st["pane"]
    w = next((w for w in st.get("workers", []) if w["name"] == who and w["kind"] == "local"), None)
    return w["pane"] if w else None


def say(sid: str, text: str, who: str = "agent") -> tuple[bool, str]:
    st = load(sid)
    pane = target(st, who) if st else None
    if not pane:
        return False, "unknown agent or worker"
    return (True, "sent") if type_into(pane, text, st["sock"]) else (False, "could not type into it")


def key(sid: str, keyname: str, who: str = "agent") -> tuple[bool, str]:
    st = load(sid)
    pane = target(st, who) if st else None
    if not pane or keyname not in ("Enter", "Escape", "Up", "Down", "Tab", "1", "2", "3"):
        return False, "unknown agent/worker or key"
    ok = hv("pane", "send-keys", pane, keyname, sock=st["sock"]).returncode == 0
    return (True, "pressed") if ok else (False, "could not press it")


def read(sid: str, who: str = "agent", lines: int = 80) -> tuple[bool, str]:
    st = load(sid)
    pane = target(st, who) if st else None
    if not pane:
        return False, "unknown agent or worker"
    p = hv("pane", "read", pane, "--source", "recent", "--lines", str(lines), sock=st["sock"])
    return p.returncode == 0, p.stdout


def worker(sid: str, name: str, task: str, model: str, cloud: bool) -> tuple[bool, str]:
    st = load(sid)
    if not st or st.get("status") not in ACTIVE:
        return False, "Auto Mode is not on"
    if not NAME.match(name) or any(w["name"] == name for w in st.get("workers", [])):
        return False, "name: lowercase letters, digits and dashes, not used yet in this run"
    if cloud:
        p = subprocess.run([hivey_bin(), "cloud", "run", task, "--name", name, "--model", model],
                           cwd=st["folder"], capture_output=True, text=True, timeout=600)
        if p.returncode != 0:
            return False, (p.stderr or p.stdout).strip()[-600:] or "hivey cloud run failed"
        st["workers"].append({"name": name, "kind": "cloud", "model": model, "task": task[:300], "started": now()})
        save(st)
        return True, p.stdout.strip()[-800:]
    folder = Path(st["sup_folder"]) / "workers" / name
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "TASK.md").write_text(
        f"# {name}\n\nYou are a worker in {st['name']}'s Auto Mode. Your supervisor gave you this task:\n\n{task}\n\n"
        f"Work in this folder; the agent's folder is {st['folder']} (you may read and change it as the task says).\n"
        "Check your work with a real check (run it, test it). Commit only on a branch, never push to main, never\n"
        "publish, post, deploy, delete outside this folder or message anyone: say so and stop instead.\n"
        "End your final answer with what you did, how you checked it, and `DONE: <3 words>` on its own line.\n")
    trust(folder)
    made = hv_json("pane", "split", st["sup_pane"], "--direction", "right", sock=st["sock"])
    pane = (made.get("pane") or made.get("new_pane") or {}).get("pane_id") or made.get("pane_id")
    if not pane:
        return False, "could not open a pane for the worker"
    cmd = (f"cd {shlex.quote(str(folder))} && claude --model {shlex.quote(model)} --dangerously-skip-permissions "
           f"--add-dir {shlex.quote(st['folder'])} -- {shlex.quote('Read TASK.md and do it.')}")
    hv("pane", "run", pane, cmd, sock=st["sock"])
    st["workers"].append({"name": name, "kind": "local", "pane": pane, "model": model, "task": task[:300], "started": now()})
    save(st)
    return True, f"worker {name} started in pane {pane} ({model})"


def set_fields(sid: str, sheet: str = "", goal: str = "") -> tuple[bool, str]:
    st = load(sid)
    if not st:
        return False, "unknown Auto Mode run"
    if sheet:
        st["sheet"] = sheet.strip()[:500]
    if goal:
        st["goal"] = goal.strip()[:2000]
    save(st)
    return True, "saved"


# ---------------------------------------------------------------- the watcher

WRAP_UP = ("Do the wrap-up in your CLAUDE.md (report, wiki, MEMORY.md, task list), stop your local workers, "
           "then run: hivey agent-map auto done {sid}")


def watch(sid: str) -> int:
    seen: dict[str, str] = {}
    events: list[str] = []
    last_event = now()
    inbox = STATE_DIR / f"{sid}.inbox"
    while True:
        st = load(sid)
        if not st or st.get("status") not in ACTIVE:
            break
        sock = st["sock"]
        if pane_status(st["sup_pane"], sock) == "gone":  # the autopilot tab was closed by hand
            done(sid, note="its autopilot tab was closed")
            break
        if inbox.is_file():  # stop requests (the fleet page's Stop, `auto stop`)
            for line in inbox.read_text().splitlines():
                try:
                    reason = json.loads(line).get("stop")
                except ValueError:
                    continue
                if reason and st["status"] == "running":
                    st = update(sid, status="wrapping", wrap_since=now()) or st
                    events.append(f"[auto] STOPPED ({reason}): Auto Mode ends now. " + WRAP_UP.format(sid=sid))
            inbox.unlink(missing_ok=True)
        panes = [("the agent", "agent", st["pane"])] + [
            (f"worker {w['name']}", w["name"], w["pane"]) for w in st.get("workers", []) if w.get("kind") == "local"]
        for label, who, pane in panes:
            cur, prev = pane_status(pane, sock), seen.get(pane)
            seen[pane] = cur
            if prev is None or cur == prev:
                continue
            read_cmd = f"hivey agent-map auto read {sid}" + ("" if who == "agent" else f" --who {who}")
            if cur == "blocked":
                events.append(f"[auto] {label} is waiting for an answer. Read it: {read_cmd}")
            elif prev == "working" and cur in ("idle", "done"):
                events.append(f"[auto] {label} finished its turn. Read it: {read_cmd}")
            elif cur == "gone":
                events.append(f"[auto] {label}'s pane is gone.")
        if st["status"] == "running" and now() >= st["ends"]:
            st = update(sid, status="wrapping", wrap_since=now()) or st
            events.append("[auto] TIME UP: Auto Mode ends now. " + WRAP_UP.format(sid=sid))
        if pane_status(st["sup_pane"], sock) in ("idle", "done", "unknown"):
            if not events and st["status"] == "running" and now() - last_event > CHECK_IN:
                left = int((st["ends"] - now()) / 60)
                events.append(f"[auto] check-in: {left} min left. Look at the cloud workers (hivey cloud status), "
                              "update the task list and keep the work moving toward the goal.")
            if events and type_into(st["sup_pane"], "\n".join(events[:6]), sock):
                del events[:6]
                last_event = now()
        if st["status"] == "wrapping" and now() - st.get("wrap_since", now()) > WRAP_WAIT:
            done(sid, note="the wrap-up took too long; the tab was closed")
            break
        time.sleep(POLL)
    st = load(sid) or {}
    if st.get("status") == "finished":  # leave a minute to read it, then close the tab and the local workers
        time.sleep(60)
        for w in st.get("workers", []):
            if w.get("kind") == "local" and w.get("pane"):
                hv("pane", "close", w["pane"], sock=st.get("sock"))
        if st.get("sup_tab"):
            hv("tab", "close", st["sup_tab"], sock=st.get("sock"))
    return 0


# ---------------------------------------------------------------- the brief

def brief(st: dict) -> str:
    text = BRIEF.read_text()
    memory = Path(st["sup_folder"]) / "MEMORY.md"
    fields = {
        "NAME": st["name"], "ID": st["id"], "FOLDER": st["folder"], "WIKI": st["wiki"] or "(none)",
        "SUP": st["sup_folder"], "HOURS": str(st["hours"]), "ENDS": iso(st["ends"]), "FABLE": FABLE,
        "GOAL": st["goal"] or "(none given: propose one in step 2)",
        "MEMORY": memory.read_text()[-6000:] if memory.is_file() else "(first Auto Mode run for this agent)",
        "DATE": dt.date.today().isoformat(),
    }
    for k, v in fields.items():
        text = text.replace("{{" + k + "}}", v)
    return text


# ---------------------------------------------------------------- command line

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="hivey agent-map auto", description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("start", help="turn Auto Mode on for an agent (its fleet id, e.g. wG:p1)")
    s.add_argument("desk"); s.add_argument("--hours", type=float, required=True); s.add_argument("--goal", default="")
    s = sub.add_parser("stop", help="end Auto Mode now (its supervisor wraps up first)")
    s.add_argument("id"); s.add_argument("--reason", default="user")
    s = sub.add_parser("done", help="the supervisor: the wrap-up is written, close Auto Mode")
    s.add_argument("id"); s.add_argument("--goal", action="store_true", help="the goal was reached"); s.add_argument("--note", default="")
    sub.add_parser("status", help="Auto Mode runs (active first)").add_argument("--json", action="store_true")
    s = sub.add_parser("fresh", help="the supervisor: /clear the agent and switch it to Fable")
    s.add_argument("id")
    s = sub.add_parser("say", help="the supervisor: type a message into the agent (or a local worker) and press Enter")
    s.add_argument("id"); s.add_argument("text"); s.add_argument("--who", default="agent")
    s = sub.add_parser("key", help="the supervisor: press a key in the agent (Enter = first option, Escape = no)")
    s.add_argument("id"); s.add_argument("key"); s.add_argument("--who", default="agent")
    s = sub.add_parser("read", help="the supervisor: the bottom of the agent's (or a worker's) terminal")
    s.add_argument("id"); s.add_argument("--who", default="agent"); s.add_argument("--lines", type=int, default=80)
    s = sub.add_parser("worker", help="the supervisor: start a worker (local pane in its own folder, or --cloud)")
    s.add_argument("id"); s.add_argument("name"); s.add_argument("task")
    s.add_argument("--model", default="sonnet"); s.add_argument("--cloud", action="store_true")
    s = sub.add_parser("set", help="the supervisor: save the task list's link or the goal")
    s.add_argument("id"); s.add_argument("--sheet", default=""); s.add_argument("--goal", default="")
    s = sub.add_parser("watch", help=argparse.SUPPRESS)
    s.add_argument("id")
    a = ap.parse_args(argv)
    if a.cmd == "watch":
        return watch(a.id)
    if a.cmd == "status":
        runs = sorted(all_states(), key=lambda s: (s.get("status") not in ACTIVE, -s.get("started", 0)))
        if a.json:
            print(json.dumps(runs, indent=2))
        for r in runs[:20] if not a.json else []:
            left = f"{max(0, int((r['ends'] - now()) / 60))} min left" if r.get("status") in ACTIVE else r.get("status")
            print(f"{r['id']:40} {r['name']:22} {r['hours']:>4g} h  {left}  {r.get('goal', '')[:60]}")
        return 0
    ok, msg = {
        "start": lambda: start(a.desk, a.hours, a.goal),
        "stop": lambda: stop(a.id, a.reason),
        "done": lambda: done(a.id, a.goal, a.note),
        "fresh": lambda: fresh(a.id),
        "say": lambda: say(a.id, a.text, a.who),
        "key": lambda: key(a.id, a.key, a.who),
        "read": lambda: read(a.id, a.who, a.lines),
        "worker": lambda: worker(a.id, a.name, a.task, a.model, a.cloud),
        "set": lambda: set_fields(a.id, a.sheet, a.goal),
    }[a.cmd]()
    print(msg)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
