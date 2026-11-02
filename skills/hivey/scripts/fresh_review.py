#!/usr/bin/env python3
"""Fresh review: N one-shot critics in parallel, each starting from an empty context.

hivey's review step for swarms and solo agents (from arXiv 2610.34577: a separate critic in a
fresh context, given the spec and the diff and able to run its own checks, beats self-review
and extra review rounds, and parallel critics are cheaper than more rounds). Every critic is a
`claude -p` process with `--no-session-persistence`, started in an empty temporary folder (so
no CLAUDE.md, notes or Status of the builder are loaded), with only the spec, the artifact
paths, the git range and the check. It runs read-only (no Edit/Write tools; told never to
change files), prints its verdict and exits: no living agent, no saved session, nothing to
resume. The script merges the verdicts into one review: a defect counts if any critic reports
it. APPROVED needs every critic to PASS and the check to exit 0.

Config (per swarm or per solo agent; `config` writes it):
  swarm   <root>/.swarm/review.json
  agent   <folder>/.agent/config.json, key "review"
  keys    critics (default 2, 0 = no fresh review), model (sonnet), rounds (3, max review
          rounds before escalating), timeout_min (15, per critic)

Usage:
  fresh_review.py config <folder> [--critics N] [--model M] [--rounds R] [--timeout-min T]
  fresh_review.py run [--folder F] [--spec TEXT] [--spec-file F]... [--artifact P]...
                      [--repo R --range A..B] [--check CMD] [--check-cwd D]
                      [--critics N] [--model M] [--round N] [--previous REVIEW.md]
                      [--label L] [--out REVIEW.md] [--notify ADDR] [--background] [--json]
                      [--dry-run]
  run exit codes: 0 approved · 1 changes needed · 2 a critic or the setup failed

For a solo agent `run --folder <agent>` takes the spec from its CLAUDE.md (The ask and the
Deliverables table) and the check from .agent/config.json. Standard library only.
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).absolute().parent
BRIEF = HERE / "critic-brief.md"
DEFAULTS = {"critics": 2, "model": "sonnet", "rounds": 3, "timeout_min": 15}
MAX_CRITICS = 8
READ_TOOLS = ["Read", "Grep", "Glob", "Bash", "WebFetch", "WebSearch"]
WRITE_TOOLS = ["Edit", "Write", "NotebookEdit", "MultiEdit"]
# A critic must not report to (or be taken for) the pane that launched it.
STRIP_ENV = re.compile(r"^(HERDR_(PANE|TAB|WORKSPACE)_ID|CLAUDECODE|CLAUDE_CODE_(SESSION|CHILD|"
                       r"MESSAGING|ENTRYPOINT)\w*|CLAUDE_PID|CLAUDE_PROJECT_DIR)$")


# ---- config -------------------------------------------------------------------------------

def config_target(folder):
    folder = Path(folder).expanduser().resolve()
    if (folder / ".swarm").is_dir():
        return folder / ".swarm" / "review.json", None
    return folder / ".agent" / "config.json", "review"


def read_json(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return {}


def load_config(folder):
    path, key = config_target(folder)
    data = read_json(path)
    cfg = dict(DEFAULTS)
    cfg.update({k: v for k, v in (data.get(key, {}) if key else data).items() if k in DEFAULTS})
    return cfg


def save_config(folder, updates):
    path, key = config_target(folder)
    data = read_json(path)
    section = data.setdefault(key, {}) if key else data
    section.update(updates)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")
    return path, load_config(folder)


def cmd_config(args):
    updates = {k: v for k, v in [("critics", args.critics), ("model", args.model),
                                 ("rounds", args.rounds), ("timeout_min", args.timeout_min)]
               if v is not None}
    if "critics" in updates and not 0 <= updates["critics"] <= MAX_CRITICS:
        sys.exit(f"--critics must be 0..{MAX_CRITICS}")
    if updates:
        path, cfg = save_config(args.folder, updates)
    else:
        path, cfg = config_target(args.folder)[0], load_config(args.folder)
    print(json.dumps({"config": str(path), **cfg}, indent=2))
    return 0


# ---- spec ---------------------------------------------------------------------------------

def brief_sections(claude_md, names=("The ask", "Deliverables")):
    """The spec parts of a solo agent's CLAUDE.md, never its Status or notes."""
    try:
        text = Path(claude_md).read_text()
    except OSError:
        return ""
    out = []
    for name in names:
        m = re.search(rf"^## {re.escape(name)}[^\n]*\n(.*?)(?=^## |\Z)", text, re.M | re.S)
        if m:
            out.append(f"### {name}\n{m.group(1).strip()}")
    return "\n\n".join(out)


def previous_defects(review_md):
    try:
        text = Path(review_md).read_text()
    except OSError:
        return ""
    m = re.search(r"^## Defects\n(.*?)(?=^## |\Z)", text, re.M | re.S)
    return m.group(1).strip() if m else ""


# ---- critics ------------------------------------------------------------------------------

def build_request(a, spec, k, n):
    parts = [f"# Review request: {a.label}, round {a.round} (critic {k} of {n}, independent)",
             "## Spec", spec or "(no spec given: judge against the artifact's own stated purpose)"]
    if a.artifact:
        parts += ["## Artifacts", "\n".join(f"- {p}" for p in a.artifact)]
    if a.repo:
        rng = a.range or "HEAD~1..HEAD"
        parts += ["## Code", f"Repo: {a.repo}\nDiff: `git -C {a.repo} diff {rng}` "
                  f"(and `git -C {a.repo} status` for uncommitted work)"]
    if a.check:
        parts += ["## Check (must exit 0)", f"`{a.check}`, run in `{a.check_cwd}`"]
    prev = previous_defects(a.previous) if a.previous else ""
    if prev:
        parts += ["## Defects reported last round",
                  "Verify each one is fixed, then review everything else fresh as well:\n" + prev]
    parts.append("Reply with the verdict in the format of your instructions.")
    return "\n\n".join(parts)


def forget_project(workdir):
    """Claude Code makes ~/.claude/projects/<cwd>/ (an empty memory/ folder) even with
    --no-session-persistence. Remove it if nothing was saved, so no trace of a critic stays."""
    key = re.sub(r"[^A-Za-z0-9]", "-", str(Path(workdir).resolve()))
    for d in {key, re.sub(r"[^A-Za-z0-9]", "-", workdir)}:
        proj = Path.home() / ".claude" / "projects" / d
        if proj.is_dir() and not any(f.is_file() for f in proj.rglob("*")):
            shutil.rmtree(proj, ignore_errors=True)


def critic_env():
    env = {k: v for k, v in os.environ.items() if not STRIP_ENV.match(k)}
    env["HIVEY_CRITIC"] = "1"
    return env


def run_critic(a, spec, k, n, cfg, dirs):
    claude = shutil.which("claude")
    if not claude:
        return {"critic": k, "ok": False, "error": "claude CLI not found"}
    workdir = tempfile.mkdtemp(prefix="hivey-critic-")  # empty: no CLAUDE.md, no history
    cmd = [claude, "-p", "--model", cfg["model"], "--no-session-persistence",
           "--output-format", "json", "--append-system-prompt", BRIEF.read_text(),
           "--allowedTools", *READ_TOOLS, "--disallowedTools", *WRITE_TOOLS]
    for d in dirs:
        cmd += ["--add-dir", d]
    started = time.time()
    try:
        r = subprocess.run(cmd, input=build_request(a, spec, k, n), capture_output=True,
                           text=True, cwd=workdir, env=critic_env(),
                           timeout=cfg["timeout_min"] * 60)
    except subprocess.TimeoutExpired:
        return {"critic": k, "ok": False, "error": f"timed out after {cfg['timeout_min']} min"}
    finally:
        shutil.rmtree(workdir, ignore_errors=True)
        forget_project(workdir)
    try:
        out = json.loads(r.stdout)
        text, cost = out.get("result", ""), out.get("total_cost_usd")
    except ValueError:
        text, cost = r.stdout, None
    m = re.search(r"VERDICT:\s*(PASS|FAIL)", text or "")
    if r.returncode != 0 or not m:
        return {"critic": k, "ok": False, "text": text,
                "error": f"exit {r.returncode}, no verdict: {(r.stderr or '').strip()[-300:]}"}
    return {"critic": k, "ok": True, "verdict": m.group(1), "text": text.strip(),
            "defects": parse_defects(text), "cost_usd": cost,
            "seconds": round(time.time() - started)}


def parse_defects(text):
    m = re.search(r"^Defects:\s*\n(.*?)(?=^Verified OK:|^```|\Z)", text, re.M | re.S)
    if not m:
        return []
    items, cur = [], None
    for line in m.group(1).splitlines():
        if re.match(r"^\s*\d+\.\s+", line):
            cur = re.sub(r"^\s*\d+\.\s+", "", line).strip()
            items.append(cur)
        elif cur is not None and line.strip():
            items[-1] += " " + line.strip()
    return [i for i in items if i and i.lower() not in ("none", "none.", "-")]


def merge(results):
    """Union of defects; the same location from several critics is listed once."""
    merged = {}
    for r in results:
        for d in r.get("defects", []):
            loc = d.split(" — ")[0].split(" - ")[0].strip().lower()
            entry = merged.setdefault(loc, {"text": d, "critics": []})
            entry["critics"].append(r["critic"])
            if len(d) > len(entry["text"]):
                entry["text"] = d
    return list(merged.values())


# ---- run ----------------------------------------------------------------------------------

def run_check(cmd, cwd):
    if not cmd:
        return None
    try:
        r = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True, timeout=1800)
        return {"cmd": cmd, "exit": r.returncode, "tail": (r.stdout + r.stderr)[-1500:]}
    except subprocess.TimeoutExpired:
        return {"cmd": cmd, "exit": 124, "tail": "timed out after 30 min"}


def write_review(path, a, cfg, check, results, defects, verdict):
    lines = [f"# Fresh review: {a.label}, round {a.round}", "",
             f"**Verdict:** {verdict} · **Critics:** {len(results)} × {cfg['model']} "
             f"(one-shot, fresh context, no saved session) · **Date:** "
             f"{time.strftime('%Y-%m-%d %H:%M')}", ""]
    if check:
        lines += [f"**Check:** `{check['cmd']}` → exit {check['exit']}", ""]
    lines.append("| Critic | Verdict | Defects | Time | Cost |")
    lines.append("|---|---|---|---|---|")
    for r in results:
        cost = f"${r['cost_usd']:.2f}" if r.get("cost_usd") is not None else "-"
        lines.append(f"| {r['critic']} | {r.get('verdict', 'ERROR')} | {len(r.get('defects', []))} "
                     f"| {r.get('seconds', '-')}s | {cost} |")
    lines += ["", "## Defects"]
    lines += [f"{i}. {d['text']} (critic {', '.join(map(str, d['critics']))})"
              for i, d in enumerate(defects, 1)] or ["None."]
    if check and check["exit"] != 0:
        lines += ["", "## Check output (tail)", "```", check["tail"], "```"]
    lines += ["", "## Each critic's verdict"]
    for r in results:
        lines += ["", f"### Critic {r['critic']}", ""]
        lines += [r.get("text") or f"ERROR: {r.get('error')}"]
        if r.get("error") and r.get("text"):
            lines += ["", f"ERROR: {r['error']}"]
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    Path(path).write_text("\n".join(lines) + "\n")


def default_out(folder, label, rnd):
    folder = Path(folder)
    base = folder / ".swarm" / "reviews" if (folder / ".swarm").is_dir() else folder / ".agent" / "reviews"
    safe = re.sub(r"[^A-Za-z0-9_.-]+", "-", label).strip("-") or "review"
    return base / f"{safe}-r{rnd}-{time.strftime('%Y%m%d-%H%M%S')}.md"


def notify(addr, text):
    if addr and shutil.which("hivey"):
        subprocess.run(["hivey", "msg", "send", addr, text], capture_output=True)


def cmd_run(a):
    folder = Path(a.folder).expanduser().resolve()
    cfg = load_config(folder)
    for k in ("critics", "model", "timeout_min"):
        if getattr(a, k) is not None:
            cfg[k] = getattr(a, k)
    if not 1 <= cfg["critics"] <= MAX_CRITICS:
        sys.exit(f"critics is {cfg['critics']}: fresh review is off for {folder} "
                 f"(fresh_review.py config {folder} --critics 2 turns it on)")
    agent_cfg = read_json(folder / ".agent" / "config.json")
    a.check = a.check or agent_cfg.get("check")
    a.check_cwd = str(Path(a.check_cwd).expanduser().resolve()) if a.check_cwd else str(folder)
    a.label = a.label or folder.name
    a.artifact = [str(Path(p).expanduser().resolve()) for p in a.artifact]
    a.repo = str(Path(a.repo).expanduser().resolve()) if a.repo else None
    spec_parts = list(a.spec)
    for f in a.spec_file:
        spec_parts.append(Path(f).expanduser().read_text())
    if not spec_parts and (folder / "CLAUDE.md").exists():
        spec_parts.append(brief_sections(folder / "CLAUDE.md"))
    spec = "\n\n".join(p for p in spec_parts if p.strip())
    out = Path(a.out).expanduser() if a.out else default_out(folder, a.label, a.round)

    if a.background:
        argv = [arg for arg in sys.argv[1:] if arg != "--background"]
        if not a.out:
            argv += ["--out", str(out)]
        log = out.with_suffix(".log")
        log.parent.mkdir(parents=True, exist_ok=True)
        p = subprocess.Popen([sys.executable, __file__, *argv], stdout=open(log, "w"),
                             stderr=subprocess.STDOUT, start_new_session=True, cwd=str(folder))
        print(json.dumps({"started": True, "pid": p.pid, "critics": cfg["critics"],
                          "review": str(out), "log": str(log), "notify": a.notify}))
        return 0

    dirs = sorted({str(Path(p) if Path(p).is_dir() else Path(p).parent) for p in a.artifact}
                  | ({a.repo} if a.repo else set()) | {a.check_cwd})
    if a.dry_run:
        print(build_request(a, spec, 1, cfg["critics"]))
        print(json.dumps({"critics": cfg["critics"], "model": cfg["model"], "dirs": dirs,
                          "review": str(out)}, indent=2))
        return 0

    check = run_check(a.check, a.check_cwd)
    with ThreadPoolExecutor(max_workers=cfg["critics"]) as pool:
        results = list(pool.map(lambda k: run_critic(a, spec, k, cfg["critics"], cfg, dirs),
                                range(1, cfg["critics"] + 1)))
    defects = merge(results)
    failed = [r for r in results if not r["ok"]]
    if failed:
        verdict, code = "ERROR", 2
    elif all(r["verdict"] == "PASS" for r in results) and not defects and (not check or check["exit"] == 0):
        verdict, code = "APPROVED", 0
    else:
        verdict, code = "CHANGES", 1
    write_review(out, a, cfg, check, results, defects, verdict)
    summary = {"verdict": verdict, "label": a.label, "round": a.round, "review": str(out),
               "critics": [{k: r.get(k) for k in ("critic", "verdict", "error", "seconds", "cost_usd")}
                           for r in results],
               "defects": len(defects), "check_exit": check["exit"] if check else None}
    check_note = f", check exit {check['exit']}" if check else ""
    print(json.dumps(summary, indent=2) if a.json else
          f"{verdict}: {a.label} r{a.round}, {len(defects)} defect(s) from {len(results)} fresh "
          f"critic(s){check_note} → {out}")
    notify(a.notify, f"FRESH REVIEW {verdict} {a.label} r{a.round}: {len(defects)} defect(s) "
                     f"from {len(results)} critic(s) → {out}")
    return code


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("config", help="show or set the review config of a swarm root or agent folder")
    c.add_argument("folder")
    c.add_argument("--critics", type=int)
    c.add_argument("--model")
    c.add_argument("--rounds", type=int)
    c.add_argument("--timeout-min", type=int)
    r = sub.add_parser("run", help="run one fresh review")
    r.add_argument("--folder", default=".", help="swarm root or agent folder (config, default out)")
    r.add_argument("--spec", action="append", default=[], help="spec text (repeatable)")
    r.add_argument("--spec-file", action="append", default=[], help="file with spec text (repeatable)")
    r.add_argument("--artifact", action="append", default=[], help="path to review (repeatable)")
    r.add_argument("--repo", help="git repo or worktree of the change")
    r.add_argument("--range", help="git range to diff, e.g. main...swarm/builder (default HEAD~1..HEAD)")
    r.add_argument("--check", help="command that must exit 0 (default: .agent/config.json check)")
    r.add_argument("--check-cwd", help="where to run the check (default: --folder)")
    r.add_argument("--critics", type=int)
    r.add_argument("--model")
    r.add_argument("--timeout-min", type=int)
    r.add_argument("--round", type=int, default=1)
    r.add_argument("--previous", help="last round's review: its defects are re-checked")
    r.add_argument("--label", help="what is reviewed, e.g. T2 or the run name")
    r.add_argument("--out", help="review file (default .swarm/reviews/ or .agent/reviews/)")
    r.add_argument("--notify", help="hivey address to message with the verdict")
    r.add_argument("--background", action="store_true", help="return at once; --notify gets the verdict")
    r.add_argument("--json", action="store_true")
    r.add_argument("--dry-run", action="store_true", help="print the request and settings, run nothing")
    a = p.parse_args()
    sys.exit(cmd_config(a) if a.cmd == "config" else cmd_run(a))


if __name__ == "__main__":
    main()
