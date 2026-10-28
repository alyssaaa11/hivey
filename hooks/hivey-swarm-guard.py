#!/usr/bin/env python3
"""hivey swarm guard: a Claude Code PreToolUse hook that keeps swarm workers out of each
other's folders.

Agents that read their teammates' plans and drafts anchor on them: the swarm converges and
loses the diversity it was built for (SwarmResearch: workers that see only their own lineage
converge less; Agora: herding on a shared memory). So a worker in `<root>/<agent>/` may not
read, edit or search (exit 2, the reason goes back to the agent):

- another worker's home `<root>/<other>/` (the critic and every other non-master agent);
- the coordinator's private notes `<root>/.swarm/coordinator/`;
- the raw message log `<root>/.swarm/bus.jsonl` and other agents' overflow messages in
  `<root>/.swarm/inbox/` (its own messages come through `hivey msg inbox`);
- a search (Grep, Glob, grep -r, rg, find, tree…) rooted at the swarm root or above, which
  would reach those folders.

Shared parts stay open: the swarm root's own files, the task board in `.swarm/`, the wiki and
the agent's own home. A path another agent or the human sent to this agent in a message (a
hand-off, a deliverable to review) is allowed, file or whole folder.

Nothing happens outside swarms, for the master (it works in the root), for solo agents, or when
the manifest says `"isolation": false`. `HIVEY_ISOLATION=off` turns it off for one session.
The agent is found from `CLAUDE_PROJECT_DIR` (where it was started), so `cd ..` doesn't escape
it. Standard library only.
"""
import glob
import json
import os
import re
import shlex
import sys
from pathlib import Path

HOME = str(Path.home())
NOT_WORKERS = {"master", "coordinator", "script"}
# Commands that walk a whole folder tree when given one.
WALKERS = {"rg", "find", "fd", "tree", "ag", "ack", "tar", "zip", "rsync"}
# Commands that walk a tree only with -r / -R / --recursive.
MAYBE_WALKERS = {"grep", "egrep", "fgrep", "ls", "cp", "du", "chmod", "chown"}
# Commands whose words are text, not paths to open.
TALKERS = {"hivey", "herdr", "echo", "printf"}
PATH_LIKE = re.compile(r"[~\w.$/{}*?-]*/[~\w.$/{}*?-]*")


def real(path):
    return os.path.realpath(path)


def inside(path, folder):
    """Whether `path` is `folder` or below it (both real paths)."""
    return path == folder or path.startswith(folder.rstrip("/") + "/")


def find_root(start):
    """The swarm root above `start` (the folder holding .swarm/agents.json), or None."""
    path = Path(start)
    for folder in [path, *path.parents]:
        if (folder / ".swarm" / "agents.json").is_file():
            return folder
    return None


def role(key, entry):
    value = entry.get("role") if isinstance(entry, dict) else None
    if value:
        return value
    if key in ("coordinator", "master"):
        return "master"
    return "script" if isinstance(entry, dict) and "command" in entry else "worker"


class Swarm:
    def __init__(self, root, manifest, me):
        self.root = real(root)
        self.slug = manifest.get("slug") or os.path.basename(self.root)
        self.me = me
        agents = manifest.get("agents") or {}
        self.others = {
            key: real(os.path.join(self.root, key))
            for key, entry in agents.items()
            if key != me and role(key, entry) not in NOT_WORKERS
        }
        self.home = real(os.path.join(self.root, me))
        self.swarm_dir = os.path.join(self.root, ".swarm")
        self.handoffs = []

    def load_handoffs(self):
        """Paths named in messages to this agent: hand-offs it may open."""
        try:
            with open(os.path.join(self.swarm_dir, "bus.jsonl"), encoding="utf-8") as log:
                lines = log.read().splitlines()
        except OSError:
            return
        for line in lines:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if record.get("ev") != "msg" or record.get("to") != self.me:
                continue
            if record.get("swarm") not in (None, self.slug):
                continue
            for word in PATH_LIKE.findall(record.get("text", "")):
                word = word.rstrip(".,;:)")
                if not word.strip("/."):
                    continue
                path = expand(word, self.root)
                # Only something inside a folder: naming a whole home (or the root) in a
                # message, as a kickoff roster does, must not open it.
                if any(inside(folder, path) for folder in [self.root, *self.others.values()]):
                    continue
                self.handoffs.append(path)

    def blocked(self, path):
        """Why this agent may not touch `path` (a real path), or None."""
        if any(inside(path, handoff) for handoff in self.handoffs):
            return None
        if inside(path, self.home):
            return None
        for key, home in self.others.items():
            if inside(path, home):
                return f"{path} is in {key}'s folder"
        if inside(path, os.path.join(self.swarm_dir, "coordinator")):
            return f"{path} holds the coordinator's private notes"
        if path == os.path.join(self.swarm_dir, "bus.jsonl"):
            return f"{path} is every agent's message log (read yours with hivey msg inbox)"
        inbox = os.path.join(self.swarm_dir, "inbox")
        if inside(path, inbox) and path != inbox:
            name = os.path.basename(path)
            if not re.fullmatch(re.escape(self.me) + r"-\d+\.md", name):
                return f"{path} is another agent's message"
        return None

    def walk_blocked(self, base):
        """Why searching the tree under `base` (a real path) is not allowed, or None."""
        reason = self.blocked(base)
        if reason:
            return reason
        reached = [key for key, home in self.others.items() if inside(home, base)]
        if inside(os.path.join(self.swarm_dir, "coordinator"), base):
            reached.append("the coordinator's notes")
        if reached:
            return (
                f"searching {base} would reach {', '.join(sorted(reached))}; search a narrower "
                "folder instead (your home, the shared outputs, .swarm/TASKS.md or the wiki)"
            )
        return None


def expand(word, cwd):
    """A path word as a real absolute path (~, $HOME, ${HOME} and relative paths expanded)."""
    word = word.replace("${HOME}", HOME).replace("$HOME", HOME)
    if word.startswith("~"):
        word = HOME + word[1:]
    if not os.path.isabs(word):
        word = os.path.join(cwd, word)
    return real(os.path.normpath(word))


def expand_all(word, cwd):
    """`expand`, plus every match when the word is a glob."""
    path = expand(word, cwd) if not any(c in word for c in "*?[") else None
    if path:
        return [path]
    pattern = word.replace("${HOME}", HOME).replace("$HOME", HOME)
    if pattern.startswith("~"):
        pattern = HOME + pattern[1:]
    if not os.path.isabs(pattern):
        pattern = os.path.join(cwd, pattern)
    matches = [real(match) for match in glob.glob(pattern)]
    # The fixed part before the first wildcard, so an unmatched glob still names its folder.
    fixed = re.split(r"[*?\[]", pattern, maxsplit=1)[0]
    fixed = fixed if fixed.endswith("/") else os.path.dirname(fixed)
    return matches + [real(os.path.normpath(fixed or "/"))]


def words_of(command):
    """Every word of a shell line, and the simple commands it is made of."""
    try:
        lexer = shlex.shlex(command, posix=True, punctuation_chars=";&|()<>")
        lexer.whitespace_split = True
        lexer.commenters = ""
        words = list(lexer)
    except ValueError:
        words = command.split()
    commands, current = [], []
    for word in words:
        if word and set(word) <= set(";&|()<>"):
            if current:
                commands.append(current)
            current = []
        else:
            current.append(word)
    if current:
        commands.append(current)
    return words, commands


def program(words):
    """The command name and its arguments, without env/sudo prefixes and VAR=value words."""
    while words and (
        words[0] in ("sudo", "env", "nohup", "time", "command", "exec", "nice", "xargs")
        or re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", words[0])
    ):
        words = words[1:]
    if not words:
        return "", []
    return os.path.basename(words[0]), words[1:]


def path_words(word, cwd):
    """The paths a shell word may name: the word itself when it looks like a path (or names
    something that exists), plus path-like pieces inside it (quoted code, --opt=path)."""
    if word.startswith("-"):
        word = word.split("=", 1)[1] if "=" in word else ""
    pieces = PATH_LIKE.findall(word)
    if "/" in word or word.startswith("~") or word in (".", ".."):
        pieces.insert(0, word)
    elif word and os.path.lexists(os.path.join(cwd, word)):
        pieces.insert(0, word)
    paths = []
    for piece in pieces:
        if piece.strip("/.") or piece in (".", ".."):
            paths.extend(expand_all(piece, cwd))
    return paths


def check_bash(swarm, command, cwd):
    _, commands = words_of(command)
    for words in commands:
        name, args = program(words)
        # Messages and printed text name agents and paths without opening them.
        if name in TALKERS:
            continue
        for word in words:
            for path in path_words(word, cwd):
                reason = swarm.blocked(path)
                if reason:
                    return reason
        flags = "".join(a[1:] for a in args if a.startswith("-") and not a.startswith("--"))
        recursive = (
            name in WALKERS
            or name in MAYBE_WALKERS
            and ("R" in flags or name != "ls" and "r" in flags or "--recursive" in args or name == "du")
        )
        if not recursive:
            continue
        folders = [
            path
            for arg in args
            if not arg.startswith("-")
            for path in expand_all(arg, cwd)
            if os.path.isdir(path)
        ]
        for folder in folders or [real(cwd)]:
            reason = swarm.walk_blocked(folder)
            if reason:
                return reason
    return None


def check(swarm, tool, data, cwd):
    if tool == "Bash":
        return check_bash(swarm, data.get("command", ""), cwd)
    if tool in ("Read", "Edit", "MultiEdit", "Write", "NotebookEdit"):
        path = data.get("file_path") or data.get("notebook_path")
        return swarm.blocked(expand(path, cwd)) if path else None
    if tool in ("Grep", "Glob"):
        base = expand(data.get("path") or cwd, cwd)
        pattern = data.get("pattern", "") if tool == "Glob" else ""
        if pattern and not pattern.startswith("**"):
            # A pattern such as ../critic/*.md moves the search base.
            fixed = re.split(r"[*?\[{]", pattern, maxsplit=1)[0]
            fixed = fixed if fixed.endswith("/") else os.path.dirname(fixed)
            if fixed:
                base = expand(os.path.join(base, fixed), base)
        if os.path.isdir(base):
            return swarm.walk_blocked(base)
        return swarm.blocked(base)
    return None


def find_swarm(start_dirs):
    """The Swarm the agent started in `start_dirs` works for, if it is a guarded worker."""
    for start in start_dirs:
        if not start:
            continue
        root = find_root(real(start))
        if root is None:
            continue
        try:
            manifest = json.loads((root / ".swarm" / "agents.json").read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return None
        if manifest.get("solo") or manifest.get("isolation") is False:
            return None
        relative = os.path.relpath(real(start), real(root))
        if relative == "." or relative.startswith(".."):
            return None  # the master (or a session) in the root itself
        me = relative.split(os.sep, 1)[0]
        agents = manifest.get("agents") or {}
        if me not in agents or role(me, agents[me]) in NOT_WORKERS:
            return None
        return Swarm(root, manifest, me)
    return None


def main():
    if os.environ.get("HIVEY_ISOLATION", "").lower() in ("off", "0", "false"):
        return 0
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    cwd = event.get("cwd") or os.getcwd()
    swarm = find_swarm([os.environ.get("CLAUDE_PROJECT_DIR"), cwd])
    if swarm is None:
        return 0
    swarm.load_handoffs()
    reason = check(swarm, event.get("tool_name", ""), event.get("tool_input") or {}, cwd)
    if not reason:
        return 0
    print(
        f"hivey isolation blocked this: {reason}. In swarm {swarm.slug} each agent works only in "
        f"its own folder ({swarm.home}) so it doesn't anchor on its teammates' plans and drafts. "
        "If you need something from another agent, ask it with `hivey msg send <agent> \"…\"`; "
        "a path sent to you in a message is allowed. Never work around this check.",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
