#!/usr/bin/env python3
"""Shared task board + event log for a swarm.

Tasks live in <root>/.swarm/tasks.json (locked on every write) and are rendered to
<root>/.swarm/TASKS.md. Every change is appended to <root>/.swarm/events.jsonl, the
swarm's timeline (addons such as the watcher may log there too). hivey's dashboard reads
tasks.json, so keep its format.

Statuses: open → in-progress → review (deliverable submitted, fresh review running) → approved
          (+ blocked at any point)
Rules: a task can be claimed only by its owner (or anyone if unowned) and only once all
its deps are approved. If a task has a `check` command, `approved` requires it to pass.
Checks run outside the board lock, so a long test suite doesn't stall other agents.

Review: `submit` starts a fresh review in the background (the hivey skill's
fresh_review.py): N one-shot critics (config `<root>/.swarm/review.json`, default 2 × sonnet),
each in an empty context with no saved session, read the task spec, `<root>/.swarm/REQUEST.md`
(the user's request) and the artifact, run the check and exit. All PASS and the check exits 0
→ the task is `approved` (by `fresh-review`); otherwise it goes back to `in-progress` with the
merged review, and the owner gets a message either way. After `rounds` failed rounds (default
3) it's `blocked` and the coordinator is told. Besides that, only the coordinator approves.
With `critics: 0` there's no fresh review: `submit` sets `review` for the coordinator.

Usage (root is found by walking up from cwd to the folder containing .swarm/):
  swarm_tasks.py add ID "title" [--owner A] [--deps T1,T2] [--path P] [--check "cmd"]
                   [--spec "done when …"] [--as coordinator]
  swarm_tasks.py list [--owner A] [--status S]
  swarm_tasks.py claim ID --as A
  swarm_tasks.py set ID STATUS --as A [--note TEXT]
  swarm_tasks.py verify ID --as A          # run the task's check command
  swarm_tasks.py submit ID --as A [--artifact P]... [--repo R --range main...swarm/A]
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
APPROVERS = {"coordinator", "critic"}  # "critic": swarms made before fresh review
REVIEWER = "fresh-review"  # the actor of approvals that fresh_review.py earned


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



def fresh_review_script():
    """$HIVEY_FRESH_REVIEW, else the installed hivey skill, else the repo copy next to this skill."""
    candidates = [os.environ.get("HIVEY_FRESH_REVIEW")] + [
        str(d / "fresh_review.py") for d in (Path.home() / ".claude/skills/hivey/scripts",
                                             Path.home() / ".agents/skills/hivey/scripts",
                                             Path(__file__).resolve().parents[2] / "hivey/scripts")]
    for f in candidates:
        if f and Path(f).is_file():
            return Path(f)
    sys.exit("fresh_review.py not found: install the hivey skill (hivey skill install)")


def review_config(root):
    try:
        cfg = json.loads((Path(root) / ".swarm" / "review.json").read_text())
    except (OSError, ValueError):
        cfg = {}
    return {"critics": int(cfg.get("critics", 2)), "rounds": int(cfg.get("rounds", 3))}


def submit(root, args):
    """Mark the task in review and start its fresh review in a detached process."""
    cfg = review_config(root)
    with board(root) as data:
        t = get(data, args.id)
        if t.get("owner") and t["owner"] != args.actor:
            sys.exit(f"{args.id} is owned by {t['owner']}: only the owner submits it")
        if t["status"] == "review":
            sys.exit(f"{args.id} is already in review; wait for the verdict message")
        if t["status"] == "approved":
            sys.exit(f"{args.id} is already approved")
        t["status"] = "review"
        rnd = t.get("review_round", 0) + 1
        if cfg["critics"] > 0:
            t["review_round"] = rnd
        log_event(root, "review_submitted", args.actor, task=args.id, round=rnd, critics=cfg["critics"])
        task = dict(t)
    if cfg["critics"] <= 0:
        print(f"{args.id} is in review. Fresh review is off for this swarm (critics: 0): "
              f"send DONE to the coordinator, who approves it.")
        return
    worker = [sys.executable, str(Path(__file__).resolve()), "--root", str(root), "_review", args.id,
              "--as", args.actor, "--round", str(rnd)]
    for a in args.artifact:
        worker += ["--artifact", a]
    if args.repo:
        worker += ["--repo", args.repo, "--range", args.range or "HEAD~1..HEAD"]
    reviews = Path(root) / ".swarm" / "reviews"
    reviews.mkdir(parents=True, exist_ok=True)
    log = open(reviews / f"{args.id}-r{rnd}.log", "w")
    subprocess.Popen(worker, stdout=log, stderr=subprocess.STDOUT, start_new_session=True, cwd=str(root))
    print(f"{args.id} round {rnd}: {cfg['critics']} fresh critics are reviewing it in the background "
          f"(review: .swarm/reviews/{args.id}-r{rnd}.md). You'll get a message with the verdict; "
          f"work on something else meanwhile.")


def review_worker(root, args):
    """Detached: run fresh_review.py for one task, then record the verdict on the board."""
    cfg = review_config(root)
    t = get(load(root), args.id)
    out = Path(root) / ".swarm" / "reviews" / f"{args.id}-r{args.round}.md"
    spec = (f"Task {t['id']}: {t['title']}\nOwner: {t.get('owner') or '-'}\n"
            f"Deliverable: {t.get('path') or '-'}\nDone when: {t.get('spec') or '(see the title and the check)'}")
    cmd = [sys.executable, str(fresh_review_script()), "run", "--folder", str(root), "--label", t["id"],
           "--round", str(args.round), "--spec", spec, "--out", str(out), "--check-cwd", str(root)]
    request = Path(root) / ".swarm" / "REQUEST.md"
    if request.exists():
        cmd += ["--spec-file", str(request)]
    if t.get("check"):
        cmd += ["--check", t["check"]]
    if t.get("last_review"):
        cmd += ["--previous", t["last_review"]]
    artifacts = list(args.artifact) + ([str(Path(root) / t["path"])] if t.get("path") else [])
    for a in artifacts:
        if Path(a).expanduser().exists():
            cmd += ["--artifact", a]
    if args.repo:
        cmd += ["--repo", args.repo, "--range", args.range]
    code = subprocess.run(cmd).returncode
    verdict = {0: "APPROVED", 1: "CHANGES"}.get(code, "ERROR")
    owner = t.get("owner") or args.actor
    escalate = verdict == "CHANGES" and args.round >= cfg["rounds"]
    with board(root) as data:
        t = get(data, args.id)
        t["last_review"] = str(out)
        if verdict == "APPROVED":
            t["status"], actor = "approved", REVIEWER
        elif escalate:
            t["status"], actor = "blocked", REVIEWER
        else:
            t["status"], actor = ("in-progress" if verdict == "CHANGES" else "review"), REVIEWER
        t["notes"].append(f"{time.strftime('%Y-%m-%d %H:%M')} {REVIEWER}: r{args.round} {verdict} → {out}")
        log_event(root, "review_" + verdict.lower(), actor, task=args.id, round=args.round, review=str(out))
    rel = out.relative_to(root)
    msgs = {"APPROVED": f"APPROVED {args.id} (fresh review r{args.round}, every critic PASS, check passed) → {rel}",
            "CHANGES": f"CRITIQUE r{args.round} {args.id} → {rel}: fix every defect in its Defects list in one "
                       f"revision, then submit again",
            "ERROR": f"Fresh review r{args.round} of {args.id} failed to run (see {rel} and its .log); "
                     f"tell the coordinator"}
    if escalate:
        msgs["CHANGES"] = (f"CRITIQUE r{args.round} {args.id} → {rel}: that was the last review round; "
                           f"the task is blocked and the coordinator decides")
    send = ["hivey", "msg", "send"]
    subprocess.run(send + [owner, msgs[verdict]], capture_output=True)
    if escalate or verdict == "ERROR":
        subprocess.run(send + ["coordinator", f"{args.id} needs you: fresh review r{args.round} "
                               f"{verdict} → {rel}"], capture_output=True)

def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--root")
    sub = p.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("add"); a.add_argument("id"); a.add_argument("title")
    a.add_argument("--owner"); a.add_argument("--deps", default=""); a.add_argument("--path")
    a.add_argument("--check"); a.add_argument("--spec", help="the deliverable's 'done when', for the critics")
    a.add_argument("--as", dest="actor", default="coordinator")
    ls = sub.add_parser("list"); ls.add_argument("--owner"); ls.add_argument("--status")
    c = sub.add_parser("claim"); c.add_argument("id"); c.add_argument("--as", dest="actor", required=True)
    s = sub.add_parser("set"); s.add_argument("id"); s.add_argument("status", choices=STATUSES)
    s.add_argument("--as", dest="actor", required=True); s.add_argument("--note", default="")
    v = sub.add_parser("verify"); v.add_argument("id"); v.add_argument("--as", dest="actor", required=True)
    sb = sub.add_parser("submit"); sb.add_argument("id"); sb.add_argument("--as", dest="actor", required=True)
    sb.add_argument("--artifact", action="append", default=[]); sb.add_argument("--repo"); sb.add_argument("--range")
    rw = sub.add_parser("_review"); rw.add_argument("id"); rw.add_argument("--as", dest="actor", required=True)
    rw.add_argument("--round", type=int, required=True); rw.add_argument("--artifact", action="append", default=[])
    rw.add_argument("--repo"); rw.add_argument("--range")
    lg = sub.add_parser("log"); lg.add_argument("type"); lg.add_argument("--as", dest="actor", required=True)
    lg.add_argument("--note", default="")
    args = p.parse_args()
    root = Path(args.root).resolve() if args.root else find_root()

    if args.cmd == "list":
        ts = [t for t in load(root)["tasks"] if (not args.owner or t.get("owner") == args.owner)
              and (not args.status or t["status"] == args.status)]
        print(json.dumps(ts, indent=2))
        return
    if args.cmd == "submit":
        submit(root, args)
        return
    if args.cmd == "_review":
        review_worker(root, args)
        return
    if args.cmd == "log":
        log_event(root, args.type, args.actor, note=args.note)
        print(json.dumps({"ok": True}))
        return
    if args.cmd == "set" and args.status == "approved" and args.actor not in APPROVERS:
        sys.exit(f"only a fresh review or the coordinator can approve {args.id}: run "
                 f"`swarm_tasks.py submit {args.id} --as {args.actor}`")
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
                 "spec": args.spec, "notes": []}
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
