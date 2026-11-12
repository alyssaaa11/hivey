#!/usr/bin/env python3
"""Stop-hook "done gate" for a hivey agent (Claude Code), from hivey-agent-creator.

Registered in the agent's own .claude/settings.json by setup_agent.py --gate. While a run is
in progress (the agent created .agent/state/run_active at the start of it), every attempt to
finish runs the agent's check command from .agent/config.json. If the check fails, the stop is
blocked and the agent gets the check output as its next instruction. After MAX_ATTEMPTS failed
attempts the gate gives up (so it can never loop forever), logs it, and lets the agent stop.
With no run in progress, it does nothing: normal conversation is never blocked.

After the check passes, two more gates (verifier audit 2026-11-05: stale Status files and self-graded
"done" made agents' status untrustworthy):
  - Status: status_check.py --today must pass (Status rewritten for this run, ≤ 40 lines, dated today,
    no unreviewed work under "done"). Off with "status_check": false in .agent/config.json.
  - Review: when .agent/config.json has review.critics > 0, a fresh review written during this run must
    say APPROVED, or the run must have used all review.rounds (leftovers noted). The agent re-reading its
    own fixes doesn't count. A run labelled in run_active (e.g. `echo evening > .agent/state/run_active`)
    skips this only if the label is listed in review.optional_for.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

MAX_ATTEMPTS = 3
TIMEOUT_S = 300
HERE = Path(__file__).resolve().parent
VERDICT_RE = re.compile(r"^\*\*Verdict:\*\*\s*(\w+)", re.M)


def review_gate(root, cfg, active):
    """None if the run's review requirement is met, else the reason it isn't."""
    review = cfg.get("review") or {}
    if int(review.get("critics") or 0) <= 0:
        return None
    label = active.read_text().strip() if active.exists() else ""
    if label and label in (review.get("optional_for") or []):
        return None
    start = active.stat().st_mtime
    files = []
    for d in (root / ".swarm" / "reviews", root / ".agent" / "reviews"):
        if d.is_dir():
            files += [f for f in d.glob("*.md") if f.stat().st_mtime >= start]
    if not files:
        return ("no fresh review was run during this run. Run the fresh review your brief names "
                "(fresh_review.py run …) and fix what it finds; your own re-read doesn't count.")
    files.sort(key=lambda f: f.stat().st_mtime)
    verdicts = [(VERDICT_RE.search(f.read_text(errors="ignore")) or [None, "?"])[1] for f in files]
    if verdicts[-1] == "APPROVED":
        return None
    rounds = int(review.get("rounds") or 3)
    if len(files) >= rounds:
        return None  # max rounds used: allowed to finish, leftovers must be noted in Status
    return (f"the latest fresh review says {verdicts[-1]} ({files[-1].name}). Fixing its defects yourself "
            f"isn't a review: run round {len(files) + 1} (--round {len(files) + 1} --previous {files[-1]}) "
            f"until APPROVED, or until {rounds} rounds are used and the leftovers are noted in Status.")


def status_gate(root, cfg):
    if cfg.get("status_check") is False or not (HERE / "status_check.py").exists():
        return None
    r = subprocess.run([sys.executable, str(HERE / "status_check.py"), str(root), "--today"],
                       capture_output=True, text=True, timeout=60)
    return None if r.returncode == 0 else (r.stdout + r.stderr).strip()


def log(state, line):
    with (state / "gate.log").open("a") as f:
        f.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {line}\n")


def main():
    argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
                            epilog="Reads the Stop hook's JSON event on stdin.").parse_args()
    try:
        event = json.load(sys.stdin)
    except json.JSONDecodeError:
        event = {}
    root = Path(os.environ.get("CLAUDE_PROJECT_DIR") or event.get("cwd") or ".")
    cfg_path = root / ".agent" / "config.json"
    state = root / ".agent" / "state"
    active = state / "run_active"
    if not cfg_path.exists() or not active.exists():
        return
    cfg = json.loads(cfg_path.read_text())
    check = cfg.get("check")
    if not check:
        return

    attempts_file = state / "gate_attempts"
    try:
        r = subprocess.run(check, shell=True, cwd=root, capture_output=True, text=True, timeout=TIMEOUT_S)
        code, output = r.returncode, (r.stdout + r.stderr).strip()
    except subprocess.TimeoutExpired:
        code, output = 124, f"check timed out after {TIMEOUT_S}s"

    if code == 0:
        extra = [x for x in (status_gate(root, cfg), review_gate(root, cfg, active)) if x]
        if extra:
            code, output = 1, "The check passed, but:\n- " + "\n- ".join(extra)
    if code == 0:
        active.unlink(missing_ok=True)
        attempts_file.unlink(missing_ok=True)
        log(state, f"PASS {check}")
        return

    attempts = int(attempts_file.read_text() or 0) + 1 if attempts_file.exists() else 1
    if attempts > MAX_ATTEMPTS:
        active.unlink(missing_ok=True)
        attempts_file.unlink(missing_ok=True)
        log(state, f"GAVE UP after {MAX_ATTEMPTS} failed attempts: {check} → {output[-300:]}")
        print(json.dumps({"systemMessage": f"Done gate gave up: `{check}` still fails after "
                                           f"{MAX_ATTEMPTS} attempts. See .agent/state/gate.log."}))
        return
    attempts_file.write_text(str(attempts))
    log(state, f"BLOCK {attempts}/{MAX_ATTEMPTS} exit={code}: {check}")
    print(json.dumps({
        "decision": "block",
        "reason": (f"Done gate: your run isn't finished. `{check}` exited {code} "
                   f"(attempt {attempts}/{MAX_ATTEMPTS}). Output:\n{output[-1500:]}\n\n"
                   "Fix what it reports, run the check yourself until it passes, update Status, "
                   "then finish. If it can't pass, explain why in Status and in your reply."),
    }))


if __name__ == "__main__":
    main()
