#!/usr/bin/env python3
"""Scaffold one hivey swarm agent's folder.

Creates <root>/<agent>/ (and <root>/<agent>/.claude/skills/) and writes
<root>/<agent>/.claude/settings.json with:
  - permissions.additionalDirectories: the swarm root (and --vault, if given), so the agent
    can read and write the whole swarm folder;
  - env.AGENTS_TTS = "0": workers stay silent; only the master speaks its summaries;
  - hooks.Stop: the swarm done gate (swarm_gate.py, Claude Code only): an agent that tries
    to stop while it still owns an in-progress task is reminded once to submit it, mark it
    blocked, or say why it stays open. --no-gate leaves it out.
Existing settings are kept and merged. The coordinator writes the agent's CLAUDE.md; skills
are installed separately, as `hivey skills guide` says.

--worktree REPO gives a coding agent its own git worktree of REPO at <root>/<agent>/repo on
branch swarm/<agent>, so parallel builders never overwrite each other. REPO needs at least one
commit. The integrator (the critic or a dedicated agent) merges the branches. Gitignored files
the worktree needs (`.env`, local config) are copied in when REPO has a `.worktreeinclude`
(gitignore syntax, the same file Claude Code's own worktrees read).

Usage: setup_agent.py <swarm_root> <agent> [--vault PATH] [--worktree REPO] [--no-gate]
"""
import argparse
import json
import re
import shutil
import subprocess
from pathlib import Path

NAME_RE = re.compile(r"^[a-z][a-z0-9_-]{0,31}$")  # hivey agent-name rule
GATE = Path(__file__).absolute().parent / "swarm_gate.py"  # keep the installed path


def copy_included(repo, worktree):
    """Copy REPO's untracked files that match its .worktreeinclude into the new worktree."""
    include = repo / ".worktreeinclude"
    if not include.is_file():
        return []
    r = subprocess.run(["git", "-C", str(repo), "ls-files", "--others", "--ignored",
                        f"--exclude-from={include}"], capture_output=True, text=True)
    copied = []
    for rel in r.stdout.splitlines():
        src, dst = repo / rel, worktree / rel
        if not src.is_file() or dst.exists():
            continue
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst)
        copied.append(rel)
    return copied


def add_gate(settings):
    """Register the swarm done gate as a Stop hook (idempotent). True when added."""
    cmd = f"python3 {GATE}"
    entries = settings.setdefault("hooks", {}).setdefault("Stop", [])
    if any(h.get("command") == cmd for m in entries for h in m.get("hooks", [])):
        return False
    entries.append({"hooks": [{"type": "command", "command": cmd, "timeout": 30}]})
    return True


def add_worktree(repo, worktree, agent):
    if worktree.exists():
        return
    branch = f"swarm/{agent}"
    exists = subprocess.run(["git", "-C", str(repo), "rev-parse", "--verify", "--quiet", branch],
                            capture_output=True).returncode == 0
    cmd = ["git", "-C", str(repo), "worktree", "add", str(worktree)] + ([branch] if exists else ["-b", branch])
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit(f"git worktree failed (does {repo} have a commit?): {r.stderr.strip()}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("root", help="the swarm root folder")
    p.add_argument("agent", help="agent key, e.g. builder (matches [a-z][a-z0-9_-]{0,31})")
    p.add_argument("--vault", help="the swarm's wiki, if it lives outside the root; the agent gets access")
    p.add_argument("--worktree", metavar="REPO", help="git repo to give this agent its own worktree of")
    p.add_argument("--no-gate", action="store_true",
                   help="don't register the done gate (the Stop hook that reminds about open tasks)")
    args = p.parse_args()

    if not NAME_RE.match(args.agent):
        raise SystemExit(f"invalid agent name {args.agent!r}: must match {NAME_RE.pattern}")

    root = Path(args.root).expanduser().resolve()
    agent_dir = root / args.agent
    (agent_dir / ".claude" / "skills").mkdir(parents=True, exist_ok=True)

    worktree, copied = None, []
    if args.worktree:
        worktree = agent_dir / "repo"
        repo = Path(args.worktree).expanduser().resolve()
        add_worktree(repo, worktree, args.agent)
        copied = copy_included(repo, worktree)

    settings_path = agent_dir / ".claude" / "settings.json"
    try:
        settings = json.loads(settings_path.read_text()) if settings_path.exists() else {}
    except json.JSONDecodeError as e:
        raise SystemExit(f"{settings_path} is not valid JSON ({e}); fix it and run again")
    dirs = settings.setdefault("permissions", {}).setdefault("additionalDirectories", [])
    for d in [root] + ([Path(args.vault).expanduser().resolve()] if args.vault else []):
        if str(d) not in dirs:
            dirs.append(str(d))
    settings.setdefault("env", {})["AGENTS_TTS"] = "0"  # workers stay silent; only the master speaks
    gate = False if args.no_gate else add_gate(settings)
    settings_path.write_text(json.dumps(settings, indent=2) + "\n")

    print(json.dumps({"agent": args.agent, "dir": str(agent_dir), "settings": str(settings_path),
                      "worktree": str(worktree) if worktree else None, "copied": copied,
                      "gate_added": gate}, indent=2))


if __name__ == "__main__":
    main()
