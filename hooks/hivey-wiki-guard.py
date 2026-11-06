#!/usr/bin/env python3
"""hivey wiki guard: a Claude Code PreToolUse hook that keeps agents' wiki memory safe.

A wiki vault (a folder with `CLAUDE.md`, `raw/` and `wiki/`, as `new_wiki.py` makes it) is
memory that every agent of an agent or swarm reads and that is often synced or shared. Two
checks run on writes into one (exit 2 blocks the write; the reason goes back to the agent):

- **Secrets.** Write, Edit, MultiEdit and NotebookEdit content, and shell lines that name a
  vault path, are scanned with the patterns of `hivey-secret-scan.py` (installed next to this
  file). A key in a wiki page is read by every agent and leaves with every share. Follows the
  safety switch: `hivey safety off` or `HIVEY_SAFETY=off` skips it.
- **Page locks.** Before a session writes a file in a vault it takes a short lease on that
  file (`<vault>/.locks/`, which Obsidian ignores). Another session that tries to write the
  same file within LEASE seconds of the holder's last write is told to wait, re-read the page
  and then apply its change, so two agents never overwrite each other's edits. Leases expire by
  themselves; there is nothing to release. `HIVEY_WIKI_LOCK=off` skips it for one session.

Nothing happens for paths outside a vault. Standard library only.
"""
import importlib.util
import json
import os
import re
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
HOME = str(Path.home())
LEASE = 120  # seconds a page stays locked after its holder's last write
WRITE_TOOLS = ("Write", "Edit", "MultiEdit", "NotebookEdit")
PATH_LIKE = re.compile(r"[~\w.$/{}-]*/[~\w.$/{}-]*")
OFF = ("off", "0", "false")


def load_scanner():
    """hivey-secret-scan.py as a module (its patterns), or None when it isn't installed."""
    path = HERE / "hivey-secret-scan.py"
    if not path.is_file():
        return None
    try:
        spec = importlib.util.spec_from_file_location("hivey_secret_scan", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    except Exception:
        return None


def safety_on():
    if os.environ.get("HIVEY_SAFETY", "").lower() in OFF:
        return False
    try:
        state = json.loads((Path(HOME) / ".hivey" / "safety.json").read_text())
    except (OSError, ValueError):
        return True
    return state.get("enabled") is not False


def expand(word, cwd):
    word = word.replace("${HOME}", HOME).replace("$HOME", HOME)
    if word.startswith("~"):
        word = HOME + word[1:]
    if not os.path.isabs(word):
        word = os.path.join(cwd, word)
    return os.path.realpath(os.path.normpath(word))


def is_vault(folder):
    return (
        os.path.isfile(os.path.join(folder, "CLAUDE.md"))
        and os.path.isdir(os.path.join(folder, "raw"))
        and os.path.isdir(os.path.join(folder, "wiki"))
    )


def find_vault(path):
    """The vault holding `path` (a real path), or None."""
    folder = path if os.path.isdir(path) else os.path.dirname(path)
    while folder and folder not in ("/", HOME):
        if is_vault(folder):
            return folder
        parent = os.path.dirname(folder)
        if parent == folder:
            break
        folder = parent
    return None


def written_text(tool, data):
    """The text a file tool is about to write."""
    if tool == "Write":
        return data.get("content") or ""
    if tool == "Edit":
        return data.get("new_string") or ""
    if tool == "MultiEdit":
        return "\n".join(e.get("new_string") or "" for e in data.get("edits") or [])
    if tool == "NotebookEdit":
        return data.get("new_source") or ""
    return ""


def find_secrets(scanner, text):
    """Labels of the secret patterns found in `text`."""
    found = []
    for line in text.splitlines():
        if scanner.FALSE_POSITIVE_MARKERS.search(line):
            continue
        for label, pattern in scanner.SECRET_PATTERNS:
            match = pattern.search(line)
            if match and not scanner.is_code_identifier_value(line, match):
                found.append(label)
                break
    return found


def bash_vault(command, cwd):
    """The vault a shell line names (or runs in), or None."""
    if "wiki" not in command and "obsidian" not in command:
        return find_vault(os.path.realpath(cwd))
    for word in PATH_LIKE.findall(command):
        word = word.strip("'\"")
        if word.strip("/."):
            vault = find_vault(expand(word, cwd))
            if vault:
                return vault
    return find_vault(os.path.realpath(cwd))


def lock_file(vault, path):
    relative = os.path.relpath(path, vault).replace(os.sep, "__")
    return os.path.join(vault, ".locks", relative + ".lock")


def take_lease(vault, path, session, agent, now=None):
    """Takes or refreshes this session's lease on `path`. Returns why it can't, or None."""
    now = time.time() if now is None else now
    lock = lock_file(vault, path)
    try:
        with open(lock, encoding="utf-8") as handle:
            holder = json.load(handle)
    except (OSError, ValueError):
        holder = None
    if holder and holder.get("session") != session:
        age = now - float(holder.get("time", 0))
        if 0 <= age < LEASE:
            who = holder.get("agent") or "another agent"
            return (
                f"{os.path.relpath(path, vault)} in the wiki is being edited by {who} "
                f"(last write {int(age)}s ago). Wait about {int(LEASE - age) + 1}s, re-read the "
                "page so you keep their changes, then apply yours"
            )
    try:
        os.makedirs(os.path.dirname(lock), exist_ok=True)
        temp = f"{lock}.{os.getpid()}.tmp"
        with open(temp, "w", encoding="utf-8") as handle:
            json.dump({"session": session, "agent": agent, "time": now}, handle)
        os.replace(temp, lock)
    except OSError:
        pass  # a read-only vault: the write itself will say so
    return None


def check(event):
    tool = event.get("tool_name", "")
    data = event.get("tool_input") or {}
    cwd = event.get("cwd") or os.getcwd()
    if tool in WRITE_TOOLS:
        target = data.get("file_path") or data.get("notebook_path")
        if not target:
            return None
        path = expand(target, cwd)
        vault = find_vault(path)
        if not vault:
            return None
        text = written_text(tool, data)
    elif tool == "Bash":
        text = data.get("command") or ""
        vault = bash_vault(text, cwd)
        if not vault:
            return None
        path = None
    else:
        return None
    if safety_on():
        scanner = load_scanner()
        found = find_secrets(scanner, text) if scanner else []
        if found:
            return (
                f"this would put a secret ({', '.join(sorted(set(found)))}) into the wiki at "
                f"{vault}, which every agent reads and which leaves with every share or sync. "
                "Store the secret with envsave and write only its envsave ID or the variable "
                "name. If it is a placeholder, ask the user before going around this check"
            )
    session = event.get("session_id")
    if path and session and os.environ.get("HIVEY_WIKI_LOCK", "").lower() not in OFF:
        agent = os.path.basename(os.environ.get("CLAUDE_PROJECT_DIR") or cwd)
        return take_lease(vault, path, session, agent)
    return None


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    try:
        reason = check(event)
    except Exception:
        return 0  # never block on a bug in the guard itself
    if not reason:
        return 0
    print(f"hivey wiki guard blocked this: {reason}.", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
