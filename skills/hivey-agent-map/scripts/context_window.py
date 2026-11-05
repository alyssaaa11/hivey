"""Context window per agent: the standard window (Claude Code compacts the session before it gets
large) or 1M tokens (Opus's default in Claude Code, where every reply re-reads the whole session).

Set per agent folder in <folder>/.claude/settings.json → "env": {"CLAUDE_CODE_DISABLE_1M_CONTEXT": "1"},
so it only affects that agent and survives hivey updates. A running session keeps its window until it
restarts. Python standard library only.
"""
from __future__ import annotations

import json
import os
import shutil
import time
from pathlib import Path

HOME = Path.home()
KEY = "CLAUDE_CODE_DISABLE_1M_CONTEXT"
STANDARD = "standard"  # 200k tokens
LARGE = "1M"


def folder_of(agent: str) -> Path | None:
    """The working folder of an agent name as the spend log names it (see hivey_spend.agent_of)."""
    try:
        aliases = json.loads((HOME / ".hivey/spend/aliases.json").read_text())
    except (OSError, ValueError):
        aliases = {}
    for prefix, name in aliases.items():
        if name == agent:
            return Path(os.path.expanduser(prefix))
    if agent == "hivey":
        cand = HOME / ".hivey/agent"
    elif agent.startswith("swarm:"):
        cand = HOME / "swarms" / agent.split(":", 1)[1]
    elif agent.startswith("project:"):
        cand = HOME / "projects" / agent.split(":", 1)[1]
    elif agent.startswith("~/"):
        cand = HOME / agent[2:]
    elif "/" not in agent and ":" not in agent and " " not in agent:
        cand = HOME / "agents" / agent
    else:
        return None
    return cand if cand.is_dir() else None


def _env(path: Path) -> dict:
    try:
        return json.loads(path.read_text()).get("env") or {}
    except (OSError, ValueError, AttributeError):
        return {}


def window(folder: Path | None) -> str:
    """STANDARD or LARGE for sessions started in this folder (folder settings win over the user's)."""
    layers = [HOME / ".claude/settings.json"]
    if folder:
        layers += [folder / ".claude/settings.json", folder / ".claude/settings.local.json"]
    value = None
    for path in layers:
        if KEY in _env(path):
            value = str(_env(path)[KEY])
    return STANDARD if value in ("1", "true", "True") else LARGE


def set_window(folder: Path, size: str) -> str:
    """Write the agent's choice into <folder>/.claude/settings.json (a copy of the old file goes to
    ~/.hivey/old/). Returns what the folder now uses."""
    path = folder / ".claude/settings.json"
    try:
        data = json.loads(path.read_text()) if path.is_file() else {}
    except ValueError as e:
        raise SystemExit(f"{path} is not valid JSON ({e}); fix it first")
    if path.is_file():
        old = HOME / ".hivey/old"
        old.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, old / f"{folder.name}-settings-{time.strftime('%Y%m%d-%H%M%S')}.json")
    env = dict(data.get("env") or {})
    if size == STANDARD:
        env[KEY] = "1"
    else:
        env.pop(KEY, None)
        if str(_env(HOME / ".claude/settings.json").get(KEY)) in ("1", "true", "True"):
            env[KEY] = "0"  # the user's settings turn 1M off everywhere: turn it back on here
    data = {"env": env, **{k: v for k, v in data.items() if k != "env"}} if env else \
        {k: v for k, v in data.items() if k != "env"}
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")
    return window(folder)
