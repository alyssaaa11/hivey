#!/usr/bin/env python3
"""Stop-hook "done gate" for a hivey swarm agent (Claude Code), from hivey-swarm-creator.

Registered in the agent's own .claude/settings.json by setup_agent.py. When the agent tries
to finish a turn while it still owns an `in-progress` task on the swarm's board
(<root>/.swarm/tasks.json), the stop is blocked once with a reminder: submit the task for
review, mark it blocked with a note, or say in one line why it stays open (waiting for a reply
or for background work). One reminder per task per in-progress episode, so it can never loop,
and nothing happens when the agent owns no open task. Standard library only; any problem just
lets the agent stop.

Usage: registered as `python3 swarm_gate.py` (reads the Stop hook's JSON on stdin).
"""
import json
import os
import sys
import time
from pathlib import Path

HERE = Path(__file__).absolute().parent
TASKS = HERE / "swarm_tasks.py"


def find_agent(start):
    """(swarm root, agent key): the folder holding .swarm/ and the first folder below it."""
    p = Path(start).resolve()
    for d in [p, *p.parents]:
        if (d / ".swarm").is_dir():
            if d == p:
                return None
            return d, p.relative_to(d).parts[0]
    return None


def log_event(root, actor, **fields):
    rec = {"ts": time.strftime("%Y-%m-%dT%H:%M:%S"), "type": "gate", "actor": actor, **fields}
    try:
        with open(root / ".swarm" / "events.jsonl", "a") as f:
            f.write(json.dumps(rec) + "\n")
    except OSError:
        pass


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        event = {}
    found = find_agent(event.get("cwd") or os.environ.get("CLAUDE_PROJECT_DIR") or ".")
    if not found:
        return
    root, agent = found
    try:
        tasks = json.loads((root / ".swarm" / "tasks.json").read_text()).get("tasks", [])
    except (OSError, ValueError):
        return
    open_ids = sorted(t["id"] for t in tasks
                      if t.get("owner") == agent and t.get("status") == "in-progress")

    state_path = root / ".swarm" / "gate" / f"{agent}.json"
    try:
        reminded = set(json.loads(state_path.read_text()).get("reminded", []))
    except (OSError, ValueError):
        reminded = set()
    # A task that left in-progress starts a new episode next time.
    reminded &= set(open_ids)
    fresh = [tid for tid in open_ids if tid not in reminded]
    if fresh:
        reminded |= set(fresh)
    try:
        state_path.parent.mkdir(parents=True, exist_ok=True)
        state_path.write_text(json.dumps({"reminded": sorted(reminded)}) + "\n")
    except OSError:
        return
    if not fresh:
        return

    ids = ", ".join(fresh)
    first = fresh[0]
    log_event(root, agent, tasks=fresh)
    print(json.dumps({
        "decision": "block",
        "reason": (
            f"Swarm done gate: you still own {ids} (in-progress) on the board. Before you stop:\n"
            f"- finished: `python3 {TASKS} submit {first} --as {agent} --artifact <path>` "
            "(add --repo/--range for code), so the fresh critics review it;\n"
            f"- stuck or waiting on someone: `python3 {TASKS} set {first} blocked --as {agent} "
            "--note '<why>'` and tell your master;\n"
            "- only pausing (waiting for a reply or background work): say so in one line and stop.\n"
            "This reminder comes once per task."
        ),
    }))


if __name__ == "__main__":
    main()
