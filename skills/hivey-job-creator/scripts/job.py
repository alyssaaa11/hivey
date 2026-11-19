#!/usr/bin/env python3
"""hivey job: the coordinator's bookkeeping for a job split into GitHub issues, each fixed by
its own agent (a Claude Code cloud session via `hivey cloud`, or a local agent) in a pull
request into the job branch `job/<name>`. One job file per job: ~/.hivey/jobs/<name>.json.

  job.py init <name> --test-cmd "CMD" --tests GLOB[,GLOB…] [--base main] [--label job:<name>]
                                 the job file; run inside the repo (job branch made later)
  job.py add <name> <issue> [--model M] [--effort E] [--where cloud|local] [--deps 3,4]
         [--tests GLOB,…] [--title T]
                                 record an issue (the plan) in the job file
  job.py set <name> <issue> key=value …   change an issue's fields (status, notes, …)
  job.py show <name> [--json]    the plan and where every issue stands
  job.py ready <name>            issues that can start now: planned, deps merged
  job.py check <name> <pr>       guard + tests for one pull request: it must target the job
                                 branch, carry no bundle files, leave the job's tests alone,
                                 then the test command runs on it in a clean worktree
  job.py merge <name> <pr>       squash-merge a checked pull request into the job branch
  job.py wait <name> [--every 120] [--timeout 3600]
                                 block until a job agent's state or pull request changes
                                 (run it in the background); prints what changed
  job.py finish <name> [--draft] tests on the job branch, then one pull request job/<name> ->
                                 base listing every issue (Closes #n); closes the job's cloud
                                 agents (hivey cloud close) — the coordinator stays for the next job
  job.py board [--port 42830]    the live job board (tasks as nodes, deps as arrows, which agent
                                 works on what), served locally for every job and registered on the
                                 agent fleet page's Dashboards tab as "Jobs"
  job.py coordinator [--model opus] [--kickoff "<job>"]
                                 register and start the job coordinator: a long-lived hivey agent
                                 (slug job-coordinator, folder ~/.hivey/jobs) that runs one job
                                 after another with this skill

Statuses: planned, running, pushed, pr-open, failing, passed, merged, blocked, local.
"""

from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse
import urllib.request
from pathlib import Path

JOBS = Path.home() / ".hivey" / "jobs"
BUNDLE_PATHS = (".hivey-cloud/", ".claude/rules/hivey-cloud-global.md")
TAIL = 120
HERE = Path(__file__).resolve().parent
BOARD_PORT = 42830
BOARD_STATE = JOBS / "board-server.json"
COORDINATOR = "job-coordinator"


def die(msg: str, code: int = 1) -> None:
    print(f"job: {msg}", file=sys.stderr)
    sys.exit(code)


def sh(cmd: list[str], cwd: Path | None = None, check: bool = True, timeout: int | None = None):
    proc = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True, timeout=timeout)
    if check and proc.returncode != 0:
        die(f"{' '.join(cmd[:4])} failed: {(proc.stderr or proc.stdout).strip()[:400]}")
    return proc


def path_of(name: str) -> Path:
    return JOBS / f"{name}.json"


def load(name: str) -> dict:
    p = path_of(name)
    if not p.exists():
        die(f"no job {name!r} (job.py init {name} …)")
    return json.loads(p.read_text())


def save(job: dict) -> None:
    JOBS.mkdir(parents=True, exist_ok=True)
    p = path_of(job["name"])
    tmp = p.with_suffix(".tmp")
    tmp.write_text(json.dumps(job, indent=2) + "\n")
    os.replace(tmp, p)


def now() -> str:
    return dt.datetime.now().isoformat(timespec="seconds")


def globs(text: str | None) -> list[str]:
    return [g.strip() for g in (text or "").split(",") if g.strip()]


def remote(repo: Path) -> str:
    return (sh(["git", "remote"], repo, check=False).stdout.split() or ["origin"])[0]


def gh_json(repo: Path, *args: str):
    return json.loads(sh(["gh", *args], repo).stdout or "null")


# ---------------------------------------------------------------- plan

def cmd_init(a) -> int:
    repo = Path(sh(["git", "rev-parse", "--show-toplevel"]).stdout.strip())
    if path_of(a.name).exists() and not a.force:
        die(f"job {a.name} exists (--force to start over)")
    job = {"name": a.name, "repo": str(repo), "base": a.base, "branch": f"job/{a.name}",
           "label": a.label or f"job:{a.name}", "test_cmd": a.test_cmd, "tests": globs(a.tests),
           "max_attempts": a.max_attempts, "created": now(), "issues": {},
           "slug": sh(["gh", "repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"],
                      repo, check=False).stdout.strip()}
    save(job)
    print(f"job {a.name}: {path_of(a.name)}  branch {job['branch']} from {a.base}")
    return 0


def cmd_add(a) -> int:
    job = load(a.name)
    entry = job["issues"].setdefault(str(a.issue), {"status": "planned", "attempts": 0})
    for key in ("model", "effort", "where", "title"):
        if getattr(a, key):
            entry[key] = getattr(a, key)
    entry.setdefault("where", "cloud")
    if a.deps is not None:
        entry["deps"] = [int(d) for d in a.deps.split(",") if d.strip()]
    if a.tests:
        entry["tests"] = globs(a.tests)
    save(job)
    print(f"#{a.issue}: {json.dumps(entry)}")
    return 0


def cmd_set(a) -> int:
    job = load(a.name)
    entry = job["issues"].setdefault(str(a.issue), {"status": "planned", "attempts": 0})
    for pair in a.pairs:
        key, _, value = pair.partition("=")
        if not key:
            die(f"not key=value: {pair}")
        entry[key] = int(value) if value.isdigit() and key in ("attempts", "pr") else value
    entry["updated"] = now()
    save(job)
    print(f"#{a.issue}: {json.dumps(entry)}")
    return 0


def ready(job: dict) -> list[str]:
    merged = {n for n, e in job["issues"].items() if e.get("status") == "merged"}
    return [n for n, e in sorted(job["issues"].items(), key=lambda kv: int(kv[0]))
            if e.get("status") == "planned" and all(str(d) in merged for d in e.get("deps") or [])]


def cmd_ready(a) -> int:
    print(" ".join(ready(load(a.name))))
    return 0


def cloud_rows(name: str) -> dict[str, dict]:
    proc = sh(["hivey", "cloud", "status", "--job", name, "--json"], check=False)
    try:
        rows = json.loads(proc.stdout) if proc.returncode == 0 else []
    except ValueError:
        rows = []
    out: dict[str, dict] = {}
    for r in rows:  # the newest session per issue wins
        out[str(r.get("issue"))] = r
    return out


def cmd_show(a) -> int:
    job = load(a.name)
    rows = cloud_rows(a.name)
    if a.json:
        print(json.dumps({**job, "cloud": rows}, indent=2))
        return 0
    done = sum(e.get("status") == "merged" for e in job["issues"].values())
    print(f"job {job['name']}  {job['branch']} -> {job['base']}  {done}/{len(job['issues'])} merged")
    print(f"tests: {job['test_cmd']}   protected: {', '.join(job['tests']) or '-'}")
    for n, e in sorted(job["issues"].items(), key=lambda kv: int(kv[0])):
        r = rows.get(n) or {}
        pr = (r.get("pr") or {}).get("url") or (f"#{e['pr']}" if e.get("pr") else "")
        deps = ",".join(map(str, e.get("deps") or [])) or "-"
        print(f"  #{n:<5} {e.get('status', '?'):9} {e.get('where', 'cloud'):5} "
              f"{(e.get('model') or '-'):>8} {(e.get('effort') or '-'):>6}  deps {deps:<6} "
              f"tries {e.get('attempts', 0)}  {r.get('state', '')} {pr}  {e.get('title', '')[:50]}")
    return 0


# ---------------------------------------------------------------- review one PR

def touched_protected(files: list[str], protected: list[str]) -> list[str]:
    return [f for f in files if any(fnmatch.fnmatch(f, g) for g in protected)]


def issue_of(job: dict, pr: dict) -> str | None:
    marker = f"hivey-job:{job['name']}#"
    body = pr.get("body") or ""
    if marker in body:
        return body.split(marker, 1)[1].split()[0].strip("`.,)")
    return None


def cmd_check(a) -> int:
    job = load(a.name)
    repo = Path(job["repo"])
    pr = gh_json(repo, "pr", "view", str(a.pr), "--json",
                 "number,body,baseRefName,headRefName,headRefOid,files,state,url")
    problems = []
    if pr["baseRefName"] != job["branch"]:
        problems.append(f"targets {pr['baseRefName']}, not {job['branch']} "
                        f"(gh pr edit {a.pr} --base {job['branch']})")
    files = [f["path"] for f in pr.get("files") or []]
    bundle = [f for f in files if f.startswith(BUNDLE_PATHS)]
    if bundle:
        problems.append(f"carries hivey cloud bundle files: {', '.join(bundle[:5])}")
    issue = issue_of(job, pr)
    protected = list(job["tests"]) + list((job["issues"].get(issue or "", {}) or {}).get("tests") or [])
    touched = touched_protected(files, protected)
    if touched:
        problems.append(f"changes the coordinator's tests: {', '.join(touched[:8])}")
    result = {"pr": a.pr, "issue": issue, "head": pr["headRefOid"], "files": len(files),
              "problems": problems, "tests": None, "passed": False}
    if not problems:
        result["tests"] = run_tests(job, repo, a.pr, a.timeout)
        result["passed"] = result["tests"]["code"] == 0
    if issue and issue in job["issues"]:
        e = job["issues"][issue]
        e["pr"] = a.pr
        e["attempts"] = e.get("attempts", 0) + 1
        e["status"] = "passed" if result["passed"] else "failing"
        e["checked_head"] = pr["headRefOid"]
        e["updated"] = now()
        save(job)
        if not result["passed"] and e["attempts"] >= job.get("max_attempts", 3):
            result["escalate"] = (f"#{issue} failed {e['attempts']} checks: stop sending fixes, "
                                  "tell the user")
    print(json.dumps(result, indent=2))
    return 0 if result["passed"] else 1


def run_tests(job: dict, repo: Path, pr: int, timeout: int) -> dict:
    """The job's test command on the pull request's head merged onto the job branch, in a
    throwaway worktree (the user's checkout is never touched)."""
    rem = remote(repo)
    sh(["git", "fetch", "--quiet", rem, f"+refs/pull/{pr}/head:refs/hivey-job/pr-{pr}",
        f"+refs/heads/{job['branch']}:refs/remotes/{rem}/{job['branch']}"], repo)
    wt = Path(tempfile.mkdtemp(prefix=f"{job['name']}-pr{pr}-", dir=_wt_dir()))
    try:
        sh(["git", "worktree", "add", "--quiet", "--detach", str(wt), f"{rem}/{job['branch']}"], repo)
        merge = sh(["git", "-c", "user.name=hivey-job", "-c", "user.email=hivey-job@localhost",
                    "merge", "--no-edit", "--quiet", f"refs/hivey-job/pr-{pr}"], wt, check=False)
        if merge.returncode != 0:
            return {"code": 1, "cmd": "git merge", "tail": "merge conflict with the job branch: ask "
                    "the agent to rebase onto origin/" + job["branch"] + "\n" + merge.stdout[-2000:]}
        t0 = time.time()
        try:
            proc = subprocess.run(job["test_cmd"], shell=True, cwd=wt, text=True,
                                  capture_output=True, timeout=timeout)
            code, out = proc.returncode, (proc.stdout + proc.stderr)
        except subprocess.TimeoutExpired as e:
            code, out = 124, f"timed out after {timeout}s\n{e.stdout or ''}"
        lines = out.splitlines()
        return {"code": code, "cmd": job["test_cmd"], "seconds": round(time.time() - t0),
                "tail": "\n".join(lines[-TAIL:])}
    finally:
        sh(["git", "worktree", "remove", "--force", str(wt)], repo, check=False)
        shutil.rmtree(wt, ignore_errors=True)


def _wt_dir() -> Path:
    d = JOBS / "worktrees"
    d.mkdir(parents=True, exist_ok=True)
    return d


def cmd_merge(a) -> int:
    job = load(a.name)
    repo = Path(job["repo"])
    pr = gh_json(repo, "pr", "view", str(a.pr), "--json", "headRefOid,baseRefName,body")
    issue = issue_of(job, pr)
    e = job["issues"].get(issue or "", {})
    if not a.force and (e.get("status") != "passed" or e.get("checked_head") != pr["headRefOid"]):
        die(f"#{a.pr} has no passing check on its current head: job.py check {job['name']} {a.pr}")
    if pr["baseRefName"] != job["branch"]:
        die(f"#{a.pr} targets {pr['baseRefName']}, not {job['branch']}")
    sh(["gh", "pr", "merge", str(a.pr), "--squash", "--delete-branch",
        "--match-head-commit", pr["headRefOid"]], repo)
    if issue in job["issues"]:
        job["issues"][issue].update(status="merged", merged=now())
        save(job)
    print(f"merged #{a.pr} into {job['branch']}" + (f" (issue #{issue})" if issue else ""))
    nxt = ready(job)
    if nxt:
        print(f"ready to start now: {' '.join('#' + n for n in nxt)}")
    return 0


# ---------------------------------------------------------------- watch and finish

def snapshot(name: str) -> dict[str, tuple]:
    return {n: (r.get("state"), (r.get("pr") or {}).get("headRefOid"))
            for n, r in cloud_rows(name).items()}


def cmd_wait(a) -> int:
    before = snapshot(a.name)
    deadline = time.time() + a.timeout
    while time.time() < deadline:
        time.sleep(a.every)
        after = snapshot(a.name)
        changed = {n: s for n, s in after.items() if before.get(n) != s}
        if changed:
            for n, (state, head) in sorted(changed.items()):
                old = before.get(n, (None, None))
                why = "new commits" if state == old[0] and head != old[1] else f"{old[0]} -> {state}"
                print(f"#{n}: {why}")
            return 0
    print("no change")
    return 2


def cmd_finish(a) -> int:
    job = load(a.name)
    repo = Path(job["repo"])
    open_ = [n for n, e in job["issues"].items() if e.get("status") != "merged"]
    if open_ and not a.force:
        die(f"not merged yet: {' '.join('#' + n for n in open_)} (--force to open the PR anyway)")
    rem = remote(repo)
    sh(["git", "fetch", "--quiet", rem, job["branch"]], repo)
    wt = Path(tempfile.mkdtemp(prefix=f"{job['name']}-final-", dir=_wt_dir()))
    try:
        sh(["git", "worktree", "add", "--quiet", "--detach", str(wt), f"{rem}/{job['branch']}"], repo)
        proc = subprocess.run(job["test_cmd"], shell=True, cwd=wt, text=True, capture_output=True,
                              timeout=a.timeout)
    finally:
        sh(["git", "worktree", "remove", "--force", str(wt)], repo, check=False)
        shutil.rmtree(wt, ignore_errors=True)
    tail = "\n".join((proc.stdout + proc.stderr).splitlines()[-40:])
    if proc.returncode != 0:
        print(tail)
        die(f"the job branch fails its tests ({job['test_cmd']}): fix before the final PR")
    merged = sorted((int(n) for n, e in job["issues"].items() if e.get("status") == "merged"))
    body = "\n".join([
        f"Job **{job['name']}**: {len(merged)} issues, each fixed by its own agent, checked by the "
        "coordinator (tests written first, run on every pull request) and merged into "
        f"`{job['branch']}`.", "",
        *[f"- Closes #{n} — {job['issues'][str(n)].get('title', '')}" for n in merged], "",
        f"Tests: `{job['test_cmd']}` passed on `{job['branch']}`.", "",
        "<details><summary>test output (last lines)</summary>", "", "```", tail, "```", "</details>",
    ])
    cmd = ["gh", "pr", "create", "--base", job["base"], "--head", job["branch"],
           "--title", a.title or f"{job['name']}: {len(merged)} fixes", "--body", body]
    if a.draft:
        cmd.append("--draft")
    print(sh(cmd, repo).stdout.strip())
    job["final_pr"] = now()
    job["finished"] = now()
    save(job)
    # The job's cloud agents are done: hivey forgets them and deletes their bundle branches.
    closed = sh(["hivey", "cloud", "close", "--job", job["name"]], repo, check=False)
    print((closed.stdout or closed.stderr).strip())
    return 0


# ---------------------------------------------------------------- board

def board_data(name: str, rows: dict[str, dict] | None = None) -> dict:
    """One job for the board: every task with its status, agent and links."""
    job = load(name)
    rows = cloud_rows(name) if rows is None else rows
    slug = job.get("slug") or ""
    tasks = {}
    for n, e in job["issues"].items():
        r = rows.get(n) or {}
        status = e.get("status", "planned")
        if status == "running" and r.get("state") in ("pushed", "pr-open"):
            status = r["state"]
        if e.get("where") == "local" and status == "planned":
            status = "local"
        pr = (r.get("pr") or {}).get("url") or (
            f"https://github.com/{slug}/pull/{e['pr']}" if slug and e.get("pr") else "")
        tasks[n] = {"title": e.get("title", ""), "status": status, "model": e.get("model"),
                    "effort": e.get("effort"), "where": e.get("where", "cloud"),
                    "deps": e.get("deps") or [], "attempts": e.get("attempts", 0),
                    "issue_url": f"https://github.com/{slug}/issues/{n}" if slug else "",
                    "session_url": r.get("url") if r.get("session") else "", "pr_url": pr}
    return {"name": job["name"], "branch": job["branch"], "base": job["base"], "tasks": tasks,
            "final_pr": job.get("final_pr"), "finished": job.get("finished"), "updated": now()}


def job_summaries() -> list[dict]:
    out = []
    for p in sorted(JOBS.glob("*.json"), key=lambda p: p.stat().st_mtime, reverse=True):
        try:
            job = json.loads(p.read_text())
        except ValueError:
            continue
        if "issues" not in job:
            continue
        issues = job["issues"].values()
        out.append({"name": job["name"], "total": len(job["issues"]),
                    "merged": sum(e.get("status") == "merged" for e in issues),
                    "finished": bool(job.get("finished")),
                    "active": not job.get("finished") and any(
                        e.get("status") not in ("planned", "merged") for e in issues)})
    return out


def cmd_serve(a) -> int:
    """The board server (started by `job.py board`): the page and its JSON, 127.0.0.1 only."""
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    cache: dict[str, tuple[float, dict]] = {}

    class Board(BaseHTTPRequestHandler):
        def log_message(self, *args) -> None:
            pass

        def send(self, code: int, body: bytes, kind: str) -> None:
            self.send_response(code)
            self.send_header("Content-Type", kind)
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self) -> None:
            path = self.path.split("?")[0]
            try:
                if path == "/ping":
                    return self.send(200, b"hivey-job-board", "text/plain")
                if path == "/api/jobs":
                    return self.send(200, json.dumps(job_summaries()).encode(), "application/json")
                if path.startswith("/api/job/"):
                    name = urllib.parse.unquote(path[len("/api/job/"):])
                    if not path_of(name).exists() or "/" in name:
                        return self.send(404, b"{}", "application/json")
                    hit = cache.get(name)
                    if not hit or time.time() - hit[0] > 20:  # gh and git calls: at most every 20 s
                        hit = cache[name] = (time.time(), board_data(name))
                    return self.send(200, json.dumps(hit[1]).encode(), "application/json")
                return self.send(200, (HERE / "board.html").read_bytes(), "text/html; charset=utf-8")
            except SystemExit:
                return self.send(500, b"{}", "application/json")

    server = ThreadingHTTPServer(("127.0.0.1", a.port), Board)
    JOBS.mkdir(parents=True, exist_ok=True)
    BOARD_STATE.write_text(json.dumps({"port": a.port, "pid": os.getpid(), "started": now()}))
    server.serve_forever()
    return 0


def board_running(port: int) -> bool:
    try:
        with urllib.request.urlopen(f"http://127.0.0.1:{port}/ping", timeout=2) as r:
            return r.read() == b"hivey-job-board"
    except OSError:
        return False


def cmd_board(a) -> int:
    port = a.port
    if not board_running(port):
        JOBS.mkdir(parents=True, exist_ok=True)
        log = open(JOBS / "board-server.log", "a")
        subprocess.Popen([sys.executable, str(Path(__file__).resolve()), "serve", "--port", str(port)],
                         stdout=log, stderr=log, stdin=subprocess.DEVNULL, start_new_session=True)
        for _ in range(20):
            time.sleep(0.25)
            if board_running(port):
                break
        else:
            die(f"the board server did not start on port {port} (see {JOBS / 'board-server.log'})")
    url = f"http://127.0.0.1:{port}/"
    listed = sh(["hivey", "agent-map", "dashboard", "list", "--json"], check=False).stdout
    if url not in listed:
        sh(["hivey", "agent-map", "dashboard", "add", "Jobs", url, "--agent", COORDINATOR,
            "--note", "hivey jobs: tasks, dependencies and which agent works on what"], check=False)
    print(f"job board: {url}  (agent fleet page → 🖥 Dashboards → Jobs)")
    return 0


# ---------------------------------------------------------------- the coordinator agent

def registered(slug: str) -> bool:
    out = sh(["hivey", "swarm", "directory", "--json"], check=False).stdout
    try:
        data = json.loads(out)
    except ValueError:
        return slug in out
    items = data if isinstance(data, list) else data.get("entries") or data.get("swarms") or []
    return any((i.get("slug") or i.get("name")) == slug for i in items if isinstance(i, dict))


def cmd_coordinator(a) -> int:
    JOBS.mkdir(parents=True, exist_ok=True)
    brief = JOBS / "CLAUDE.md"
    if not brief.exists() or a.rewrite:
        brief.write_text((HERE.parent / "references" / "coordinator-brief.md").read_text())
    if registered(COORDINATOR):
        print(f"{COORDINATOR} is already a hivey agent. Give it a job: "
              f"hivey msg send {COORDINATOR}/master \"<the job, the repo path>\"")
        if a.kickoff:
            sh(["hivey", "msg", "send", f"{COORDINATOR}/master", a.kickoff], check=False)
        return 0
    cmd = ["hivey", "swarm", "launch", str(JOBS), "--slug", COORDINATOR, "--icon", "🧭", "--solo",
           "--kind", "claude", "--model", a.model,
           "--description", "Runs your jobs: splits each into GitHub issues with tests, one cloud "
           "agent per issue, checks and merges their PRs into job/<name>, one final PR for you",
           "--skills", "hivey-job-creator,model-picker,hivey-codex,hivey-grok,hivey-agy",
           "--tools", "gh, hivey cloud, git"]
    if a.kickoff:
        cmd += ["--kickoff", a.kickoff]
    proc = sh(cmd, check=False)
    print((proc.stdout + proc.stderr).strip())
    if proc.returncode == 0:
        sh(["hivey", "swarm", "schedule", "add", COORDINATOR, "--every", "20m", "--id", "watch",
            "Watch run: python3 ~/.claude/skills/hivey-job-creator/scripts/job.py show for every "
            "active job; handle new pushes, pull requests and finished checks (skill steps 5-7); "
            "if nothing is active, reply IDLE and do nothing."], check=False)
        cmd_board(argparse.Namespace(port=BOARD_PORT))
    return proc.returncode


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(prog="job.py", description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("init"); p.add_argument("name"); p.add_argument("--test-cmd", required=True)
    p.add_argument("--tests", default=""); p.add_argument("--base", default="main")
    p.add_argument("--label"); p.add_argument("--max-attempts", type=int, default=3)
    p.add_argument("--force", action="store_true")
    p = sub.add_parser("add"); p.add_argument("name"); p.add_argument("issue", type=int)
    for f in ("--model", "--effort", "--title", "--deps", "--tests"):
        p.add_argument(f)
    p.add_argument("--where", choices=["cloud", "local"])
    p = sub.add_parser("set"); p.add_argument("name"); p.add_argument("issue", type=int)
    p.add_argument("pairs", nargs="+")
    p = sub.add_parser("show"); p.add_argument("name"); p.add_argument("--json", action="store_true")
    p = sub.add_parser("ready"); p.add_argument("name")
    p = sub.add_parser("check"); p.add_argument("name"); p.add_argument("pr", type=int)
    p.add_argument("--timeout", type=int, default=1800)
    p = sub.add_parser("merge"); p.add_argument("name"); p.add_argument("pr", type=int)
    p.add_argument("--force", action="store_true")
    p = sub.add_parser("wait"); p.add_argument("name"); p.add_argument("--every", type=int, default=120)
    p.add_argument("--timeout", type=int, default=3600)
    p = sub.add_parser("finish"); p.add_argument("name"); p.add_argument("--draft", action="store_true")
    p.add_argument("--title"); p.add_argument("--force", action="store_true")
    p.add_argument("--timeout", type=int, default=1800)
    p = sub.add_parser("board"); p.add_argument("--port", type=int, default=BOARD_PORT)
    p = sub.add_parser("serve"); p.add_argument("--port", type=int, default=BOARD_PORT)
    p = sub.add_parser("coordinator"); p.add_argument("--model", default="opus")
    p.add_argument("--kickoff"); p.add_argument("--rewrite", action="store_true")
    a = ap.parse_args(argv)
    return {"board": cmd_board, "serve": cmd_serve, "coordinator": cmd_coordinator,
            "init": cmd_init, "add": cmd_add, "set": cmd_set, "show": cmd_show, "ready": cmd_ready,
            "check": cmd_check, "merge": cmd_merge, "wait": cmd_wait, "finish": cmd_finish}[a.cmd](a)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
