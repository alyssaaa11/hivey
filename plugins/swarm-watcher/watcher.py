#!/usr/bin/env python3
"""hivey.swarm-watcher: watches a swarm's workers and tells its master when one needs it.

Every --interval seconds it reads the swarm's manifest (<root>/.swarm/agents.json) and the
agents' states (`hivey agent list`), and sends the master a message (`hivey msg send`,
delivered when the master is idle) when a worker
  - is blocked (a dialog or question waits): at once, once per episode;
  - stopped (idle/done) for --idle-after seconds after working: it may be done, waiting on
    someone, or have asked something in its output (the tail of its screen is included);
  - kept working --stall-after seconds with no sign of progress (no task-board or event-log
    change mentioning it): possibly looping or lost;
  - is gone (its pane closed or the agent exited);
and when the swarm's time budget (manifest budget_minutes) reaches 80% and 100%.
It also rebuilds the swarm's Gantt dashboard (dashboard.py) every minute; --open shows it in
the browser once. Everything it reports goes to <root>/.swarm/events.jsonl too. A paused
swarm (`hivey swarm pause`) is left alone. Standard library only.

Usage: watcher.py <swarm_root> [--interval 10] [--idle-after 60] [--stall-after 2700] [--open]
"""
import argparse
import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
PREFIX = "[hivey.swarm-watcher]"


def hivey(*args):
    """Runs hivey with args; its stdout, or "" when it failed."""
    try:
        done = subprocess.run(["hivey", *args], capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return ""
    return done.stdout if done.returncode == 0 else ""


def agent_states():
    """{hivey agent name: status}, or None when hivey could not be asked."""
    text = hivey("agent", "list")
    try:
        agents = json.loads(text)["result"]["agents"]
    except (ValueError, KeyError, TypeError):
        return None
    return {a.get("name"): a.get("agent_status", "unknown") for a in agents}


def screen_tail(name, lines=20):
    text = hivey("agent", "read", name, "--source", "recent-unwrapped", "--lines", str(lines))
    try:
        text = json.loads(text)["result"]["read"]["text"]
    except (ValueError, KeyError, TypeError):
        pass
    return "\n".join(line for line in text.splitlines() if line.strip())[-1500:]


def log_event(root, **event):
    event.setdefault("ts", time.time())
    event.setdefault("type", "watcher")
    try:
        with open(root / ".swarm" / "events.jsonl", "a") as log:
            log.write(json.dumps(event) + "\n")
    except OSError:
        pass


def tell_master(root, manifest, text):
    slug = manifest.get("slug", "")
    message = f"{PREFIX} {text}"
    sent = hivey("msg", "send", "@role:master", message, "--swarm", slug)
    log_event(root, what=text, delivered=bool(sent))


def activity_mark(root, key):
    """Something that changes when the agent makes visible progress: the task board's and the
    event log's size and mtime, counted only for lines that mention the agent."""
    mark = []
    for name in ("events.jsonl", "tasks.json"):
        path = root / ".swarm" / name
        try:
            text = path.read_text(errors="ignore")
        except OSError:
            continue
        mark.append(sum(1 for line in text.splitlines() if key in line))
    return tuple(mark)


def watch_once(root, manifest, seen, args):
    states = agent_states()
    if states is None:  # hivey didn't answer: skip this pass rather than report everyone gone
        return
    now = time.time()
    for key, info in (manifest.get("agents") or {}).items():
        name = info.get("herdr_name") or f"{manifest.get('slug')}-{key}"
        if name == manifest.get("coordinator"):
            continue
        status = states.get(name, "gone")
        mark = activity_mark(root, key)
        last = seen.setdefault(name, {"status": None, "since": now, "alerted": None,
                                      "mark": mark, "progress": now, "worked": False})
        if mark != last["mark"]:
            last["mark"], last["progress"] = mark, now
        if status != last["status"]:
            last.update(status=status, since=now, alerted=None)
            if status == "working":
                last["worked"] = True
        if status != "gone":
            last["alive"] = True
        held = now - last["since"]
        if status == "blocked" and last["alerted"] != "blocked":
            tell_master(root, manifest, f"{key} is blocked (a dialog or question is waiting). "
                        f"Read it: hivey agent read {name} --source visible\n{screen_tail(name, 15)}")
            last["alerted"] = "blocked"
        elif status in ("idle", "done") and last["worked"] and held >= args.idle_after \
                and last["alerted"] != "stopped":
            tell_master(root, manifest, f"{key} stopped ({status}) {int(held)}s ago: it may be done, "
                        f"waiting on someone, or have asked something. Its last output:\n"
                        f"{screen_tail(name)}")
            last["alerted"] = "stopped"
        elif status == "working" and now - last["progress"] >= args.stall_after \
                and last["alerted"] != "stalled":
            minutes = int((now - last["progress"]) / 60)
            tell_master(root, manifest, f"{key} has been working {minutes} min with no task-board "
                        f"or event-log progress: it may be looping or lost. "
                        f"Read it: hivey agent read {name} --source recent-unwrapped --lines 120")
            last["alerted"] = "stalled"
            last["progress"] = now  # remind again after another stall-after
        # Only an agent seen alive can be "gone" (not a swarm that isn't running, nor one starting).
        elif status == "gone" and last.get("alive") and last["alerted"] != "gone":
            tell_master(root, manifest, f"{key} is gone (its pane closed or the agent exited). "
                        f"Restart it with: hivey swarm relaunch {manifest.get('slug')} {key}")
            last["alerted"] = "gone"


def budget_check(root, manifest, sent):
    minutes = manifest.get("budget_minutes")
    start = manifest.get("first_launched_at") or manifest.get("launched_at")
    if not minutes or not start:
        return
    used = (time.time() - float(start)) / 60 / float(minutes)
    for mark in (0.8, 1.0):
        if used >= mark and mark not in sent:
            sent.add(mark)
            tell_master(root, manifest, f"time budget {int(mark * 100)}% used "
                        f"({int(used * float(minutes))} of {minutes} min). "
                        + ("Wrap up: get final reports, then the final summary." if mark >= 1
                           else "Plan the remaining work to fit."))


def rebuild_dashboard(root, open_it):
    command = [sys.executable, str(HERE / "dashboard.py"), str(root)]
    if open_it:
        command.append("--open")
    try:
        subprocess.run(command, capture_output=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("root", help="the swarm's folder (with .swarm/agents.json)")
    parser.add_argument("--interval", type=float, default=10)
    parser.add_argument("--idle-after", type=float, default=60)
    parser.add_argument("--stall-after", type=float, default=2700)
    parser.add_argument("--open", action="store_true", help="open the dashboard once at start")
    parser.add_argument("--once", action="store_true", help="one pass, then exit (tests)")
    args = parser.parse_args()
    root = Path(args.root).expanduser().resolve()
    seen, budget_sent, last_dashboard, first = {}, set(), 0.0, True
    print(f"hivey.swarm-watcher: watching {root} (every {args.interval:g}s)", flush=True)
    while True:
        try:
            manifest = json.loads((root / ".swarm" / "agents.json").read_text())
        except (OSError, ValueError):
            manifest = None
        if manifest and manifest.get("state") != "paused":
            watch_once(root, manifest, seen, args)
            budget_check(root, manifest, budget_sent)
            if time.time() - last_dashboard >= 60:
                rebuild_dashboard(root, args.open and first)
                last_dashboard, first = time.time(), False
        if args.once:
            return 0
        time.sleep(args.interval)


if __name__ == "__main__":
    sys.exit(main())
