#!/usr/bin/env python3
"""SessionStart hook for a hivey agent (from hivey-agent-creator): prints resume context that Claude Code adds
to the session (on startup, resume, clear and after compaction).

It summarizes what the agent would otherwise have to rediscover: whether a run was left in
progress, recent done-gate results, task-list progress (tasks.json), recent commits touching
the agent folder, and the newest entries of its memory progress log (.agent/config.json
"progress_log"). The Status section itself lives in CLAUDE.md, which is always loaded.
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path


def tail(path, n):
    try:
        return [l for l in path.read_text().splitlines() if l.strip()][-n:]
    except OSError:
        return []


def main():
    argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter,
                            epilog="Reads the SessionStart hook's JSON event on stdin.").parse_args()
    try:
        event = json.load(sys.stdin)
    except json.JSONDecodeError:
        event = {}
    root = Path(os.environ.get("CLAUDE_PROJECT_DIR") or event.get("cwd") or ".")
    cfg_path = root / ".agent" / "config.json"
    cfg = json.loads(cfg_path.read_text()) if cfg_path.exists() else {}
    state = root / ".agent" / "state"
    out = [f"## Resume context (auto-injected by hivey-agent-creator, session source: {event.get('source', '?')})"]

    if (state / "run_active").exists():
        out.append("- ⚠ A run was left IN PROGRESS (.agent/state/run_active exists). Finish it or, "
                   "if it's stale, delete that file. The done gate will check it when you stop.")
    gate = tail(state / "gate.log", 3)
    if gate:
        out.append("- Recent done-gate results:\n" + "\n".join(f"  - {l}" for l in gate))

    tasks_path = root / "tasks.json"
    if tasks_path.exists():
        try:
            tasks = json.loads(tasks_path.read_text()).get("tasks", [])
            done = sum(1 for t in tasks if t.get("passes"))
            nxt = next((t for t in tasks if not t.get("passes")), None)
            out.append(f"- tasks.json: {done}/{len(tasks)} passing"
                       + (f"; next: {nxt.get('id')} — {nxt.get('title', '')}" if nxt else "; all passing"))
        except (json.JSONDecodeError, AttributeError):
            out.append("- tasks.json exists but isn't valid JSON: fix it first.")

    r = subprocess.run(["git", "-C", str(root), "log", "-5", "--format=%h %ad %s", "--date=short", "--", "."],
                       capture_output=True, text=True)
    if r.returncode == 0 and r.stdout.strip():
        out.append("- Recent commits for this folder:\n" + "\n".join(f"  - {l}" for l in r.stdout.splitlines()))

    progress = cfg.get("progress_log")
    if progress:
        try:  # newest-first log: the first entries are the latest
            lines = [l for l in Path(progress).expanduser().read_text().splitlines()
                     if l.lstrip().startswith("- ")]
        except OSError:
            lines = []
        if lines:
            out.append(f"- Newest entries in {progress}:\n" + "\n".join(f"  {l.strip()}" for l in lines[:5]))

    if len(out) > 1:
        print("\n".join(out))


if __name__ == "__main__":
    main()
