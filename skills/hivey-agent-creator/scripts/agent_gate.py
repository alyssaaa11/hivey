#!/usr/bin/env python3
"""Stop-hook "done gate" for a hivey agent (Claude Code), from hivey-agent-creator.

Registered in the agent's own .claude/settings.json by setup_agent.py --gate. While a run is
in progress (the agent created .agent/state/run_active at the start of it), every attempt to
finish runs the agent's check command from .agent/config.json. If the check fails, the stop is
blocked and the agent gets the check output as its next instruction. After MAX_ATTEMPTS failed
attempts the gate gives up (so it can never loop forever), logs it, and lets the agent stop.
With no run in progress, it does nothing: normal conversation is never blocked.
"""
import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

MAX_ATTEMPTS = 3
TIMEOUT_S = 300


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
    check = json.loads(cfg_path.read_text()).get("check")
    if not check:
        return

    attempts_file = state / "gate_attempts"
    try:
        r = subprocess.run(check, shell=True, cwd=root, capture_output=True, text=True, timeout=TIMEOUT_S)
        code, output = r.returncode, (r.stdout + r.stderr).strip()
    except subprocess.TimeoutExpired:
        code, output = 124, f"check timed out after {TIMEOUT_S}s"

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
