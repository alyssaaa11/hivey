#!/usr/bin/env python3
"""Shared task board + event log for a swarm.

Tasks live in <root>/.swarm/tasks.json (locked on every write) and are rendered to
<root>/.swarm/TASKS.md. Every change is appended to <root>/.swarm/events.jsonl, the
swarm's timeline (addons such as the watcher may log there too). hivey's dashboard reads
tasks.json, so keep its format.

Statuses: open → in-progress → review (deliverable DONE, waiting on critic) → approved
          (+ blocked at any point)
Rules: a task can be claimed only by its owner (or anyone if unowned) and only once all
its deps are approved. If a task has a `check` command, `approved` requires it to pass.
Only the critic or the coordinator can set a task to `approved`. Checks run outside the
board lock, so a long test suite doesn't stall other agents.

Usage (root is found by walking up from cwd to the folder containing .swarm/):
  swarm_tasks.py add ID "title" [--owner A] [--deps T1,T2] [--path P] [--check "cmd"] [--as coordinator]
  swarm_tasks.py list [--owner A] [--status S]
  swarm_tasks.py claim ID --as A
  swarm_tasks.py set ID STATUS --as A [--note TEXT]
  swarm_tasks.py verify ID --as A          # run the task's check command
  swarm_tasks.py log TYPE --as A [--note TEXT]   # append a free-form event
"""
import argparse
import fcntl
import json
import os
import subprocess
import sys
import time
from contextlib import contextmanager
from pathlib import Path

STATUSES = ["open", "in-progress", "blocked", "review", "approved"]
APPROVERS = {"critic", "coordinator"}


def find_root(start=None):
    p = Path(start or Path.cwd()).resolve()
    for d in [p, *p.parents]:
        if (d / ".swarm").is_dir():
            return d
    sys.exit("no .swarm/ folder found above the current directory — pass --root")


def log_event(root, type_, actor, **fields):
    rec = {"ts": time.strftime("%Y-%m-%dT%H:%M:%S"), "type": type_, "actor": actor, **fields}
    with open(Path(root) / ".swarm" / "events.jsonl", "a") as f:
        f.write(json.dumps(rec) + "\n")


def write_atomic(path, text):
    """Write via a temp file + rename, so readers never see a half-written file."""
    tmp = Path(path).with_name(Path(path).name + ".tmp")
    tmp.write_text(text)
    os.replace(tmp, path)


def load(root):
    path = Path(root) / ".swarm" / "tasks.json"
    return json.loads(path.read_text()) if path.exists() else {"tasks": []}


@contextmanager
def board(root):
    swarm = Path(root) / ".swarm"
    with open(swarm / "tasks.lock", "w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        data = load(root)
        yield data
        write_atomic(swarm / "tasks.json", json.dumps(data, indent=2) + "\n")
        render(root, data)


def render(root, data):
    rows = ["# Swarm task board", "", "_Generated from tasks.json by swarm_tasks.py; don't edit._", "",
            "| ID | Task | Owner | Status | Deps | Deliverable | Check |", "|---|---|---|---|---|---|---|"]
    for t in data["tasks"]:
        rows.append(f"| {t['id']} | {t['title']} | {t.get('owner') or '—'} | **{t['status']}** | "
                    f"{','.join(t['deps']) or '—'} | {t.get('path') or '—'} | "
                    f"{'`' + t['check'] + '`' if t.get('check') else '—'} |")
    write_atomic(Path(root) / ".swarm" / "TASKS.md", "\n".join(rows) + "\n")


def get(data, tid):
    for t in data["tasks"]:
        if t["id"] == tid:
            return t
    sys.exit(f"no task {tid}")


def run_check(root, t):
    try:
        r = subprocess.run(t["check"], shell=True, cwd=root, capture_output=True, text=True, timeout=900)
        return r.returncode == 0, (r.stdout + r.stderr)[-1500:]
    except subprocess.TimeoutExpired:
        return False, "check timed out after 900s"


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--root")
    sub = p.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("add"); a.add_argument("id"); a.add_argument("title")
    a.add_argument("--owner"); a.add_argument("--deps", default=""); a.add_argument("--path")
    a.add_argument("--check"); a.add_argument("--as", dest="actor", default="coordinator")
    ls = sub.add_parser("list"); ls.add_argument("--owner"); ls.add_argument("--status")
    c = sub.add_parser("claim"); c.add_argument("id"); c.add_argument("--as", dest="actor", required=True)
    s = sub.add_parser("set"); s.add_argument("id"); s.add_argument("status", choices=STATUSES)
    s.add_argument("--as", dest="actor", required=True); s.add_argument("--note", default="")
    v = sub.add_parser("verify"); v.add_argument("id"); v.add_argument("--as", dest="actor", required=True)
    lg = sub.add_parser("log"); lg.add_argument("type"); lg.add_argument("--as", dest="actor", required=True)
    lg.add_argument("--note", default="")
    args = p.parse_args()
    root = Path(args.root).resolve() if args.root else find_root()

    if args.cmd == "list":
        ts = [t for t in load(root)["tasks"] if (not args.owner or t.get("owner") == args.owner)
              and (not args.status or t["status"] == args.status)]
        print(json.dumps(ts, indent=2))
        return
    if args.cmd == "log":
        log_event(root, args.type, args.actor, note=args.note)
        print(json.dumps({"ok": True}))
        return
    if args.cmd == "set" and args.status == "approved" and args.actor not in APPROVERS:
        sys.exit(f"only the critic or coordinator can approve {args.id} — set it to review and tell the critic")
    if args.cmd == "verify" or (args.cmd == "set" and args.status == "approved"):
        t = get(load(root), args.id)
        if t.get("check"):  # run outside the board lock: a check can take minutes
            ok, out = run_check(root, t)
            log_event(root, "check_pass" if ok else "check_fail", args.actor, task=args.id, output=out[-300:])
            if not ok:
                print(out)
                sys.exit(f"check FAILED for {args.id}" + (" — not approved" if args.cmd == "set" else ""))
            if args.cmd == "verify":
                print(out)
                print(f"check passed for {args.id}")
                return
        elif args.cmd == "verify":
            sys.exit(f"{args.id} has no check command")

    with board(root) as data:
        if args.cmd == "add":
            if any(t["id"] == args.id for t in data["tasks"]):
                sys.exit(f"task {args.id} already exists")
            t = {"id": args.id, "title": args.title, "owner": args.owner, "status": "open",
                 "deps": [d for d in args.deps.split(",") if d], "path": args.path, "check": args.check,
                 "notes": []}
            data["tasks"].append(t)
            log_event(root, "task_added", args.actor, task=args.id, title=args.title, owner=args.owner)
        elif args.cmd == "claim":
            t = get(data, args.id)
            if t.get("owner") and t["owner"] != args.actor:
                sys.exit(f"{args.id} is owned by {t['owner']} — don't duplicate it; ask them or the coordinator")
            waiting = [d for d in t["deps"] if get(data, d)["status"] != "approved"]
            if waiting:
                sys.exit(f"{args.id} waits on unapproved deps: {', '.join(waiting)} — work on something unblocked")
            t["owner"], t["status"] = args.actor, "in-progress"
            log_event(root, "task_claimed", args.actor, task=args.id)
        elif args.cmd == "set":
            t = get(data, args.id)
            t["status"] = args.status
            if args.note:
                t["notes"].append(f"{time.strftime('%Y-%m-%d %H:%M')} {args.actor}: {args.note}")
            log_event(root, "task_status", args.actor, task=args.id, status=args.status, note=args.note)
        print(json.dumps(t, indent=2))


if __name__ == "__main__":
    main()
