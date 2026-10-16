#!/usr/bin/env python3
"""hivey safety: a Claude Code PreToolUse hook that stops the most damaging mistakes.

Agents in hivey often run with every permission pre-approved, so this is their guardrail. It
blocks (exit 2, the reason goes back to the agent):

- destructive commands: rm -r/-f on /, the home folder, a top-level folder of either, or
  everything (*, ., ..); wiping disks (dd to a device, mkfs, diskutil erase); chmod/chown -R
  on / or the home folder; fork bombs; sudo rm
- git commands that destroy work or history unless the user asked for them: push --force (not
  --force-with-lease), reset --hard, clean -f, checkout/restore of everything, branch -D,
  stash drop/clear
- piping a download into a shell (curl … | sh)
- reading or writing secrets: ~/.ssh keys, .env files (not .env.example and friends), cloud
  and tool credentials, private keys and certificates
- editing lockfiles by hand (the package manager owns them)

It looks at the words of each command, never at arbitrary substrings, so code such as
`json.dumps` or a variable named `key` is not mistaken for a file. `HIVEY_SAFETY=off` in the
environment turns it off for one session; `hivey safety off` for good. Standard library only.
"""
import json
import os
import re
import shlex
import sys
from pathlib import Path

HOME = Path.home()

# Allowed even though they look like secrets: templates that are meant to be committed.
SECRET_TEMPLATES = (".example", ".sample", ".template", ".dist", ".defaults")

SECRET_DIRS = [
    HOME / ".ssh",
    HOME / ".gnupg",
    HOME / ".aws",
    HOME / ".azure",
    HOME / ".kube",
    HOME / ".docker",
    HOME / ".config" / "gcloud",
    HOME / ".config" / "gh",
    HOME / ".password-store",
]
SECRET_FILES = [
    HOME / ".netrc",
    HOME / ".npmrc",
    HOME / ".pypirc",
    HOME / ".git-credentials",
    HOME / ".hivey" / "slack.json",
]
SECRET_NAMES = re.compile(
    r"^(id_(rsa|dsa|ecdsa|ed25519)(_sk)?|credentials(\.json)?|service[-_]account.*\.json|"
    r".*\.(pem|p12|pfx|jks|keystore|asc|ppk))$"
)
LOCKFILES = {
    "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb", "bun.lock",
    "Cargo.lock", "poetry.lock", "uv.lock", "Pipfile.lock", "Gemfile.lock",
    "composer.lock", "go.sum", "Podfile.lock", "pubspec.lock", "flake.lock",
}
SYSTEM_DIRS = {
    "/", "/bin", "/sbin", "/usr", "/etc", "/var", "/opt", "/private", "/System", "/Library",
    "/Applications", "/Users", "/home", "/root", "/boot", "/dev", "/lib", "/lib64", "/proc",
    "/sys", "/Volumes",
}
# Commands that only look at a path (exists? list it) without reading the contents.
LOOK_ONLY = {"ls", "test", "[", "stat", "file", "du", "wc", "touch", "mkdir", "chmod", "which"}


def expand(word, cwd):
    """A command word as an absolute path (~, $HOME, ${HOME} and relative paths expanded)."""
    word = word.replace("${HOME}", str(HOME)).replace("$HOME", str(HOME))
    if word.startswith("~"):
        word = str(HOME) + word[1:]
    path = Path(word)
    if not path.is_absolute():
        path = Path(cwd) / path
    return Path(os.path.normpath(path))


def is_secret(path):
    """Whether a path is a secret (a key, credentials, a .env file)."""
    name = path.name
    if any(marker in name for marker in SECRET_TEMPLATES):
        return False
    if name == ".env" or name.startswith(".env.") or name.endswith(".env"):
        return True
    if SECRET_NAMES.match(name) and not name.endswith(".pub"):
        return True
    if path in SECRET_FILES:
        return True
    for folder in SECRET_DIRS:
        if path == folder or folder in path.parents:
            # Public keys and known_hosts are fine to read.
            return not (name.endswith(".pub") or name in ("known_hosts", "config.pub"))
    return False


def too_broad(path):
    """Whether deleting this path recursively would wipe the system or a whole home area."""
    text = str(path)
    if text in SYSTEM_DIRS or path == HOME or HOME.is_relative_to(path):
        return True
    if path.parent in (Path("/"), HOME):  # /tmp-level and ~/projects-level folders
        return text != "/tmp"
    if path.parent == HOME / ".config" or path == HOME / "Library":
        return True
    return False


def split_commands(command):
    """The simple commands of a shell line: [[word, ...], ...] (split on ; && || | & and
    newlines; $(…) and backticks are scanned as their own commands)."""
    inner = re.findall(r"\$\(([^()]*)\)|`([^`]*)`", command)
    parts = [command] + [a or b for a, b in inner]
    commands = []
    for part in parts:
        try:
            lexer = shlex.shlex(part, posix=True, punctuation_chars=";&|")
            lexer.whitespace_split = True
            lexer.commenters = ""
            words = list(lexer)
        except ValueError:
            words = part.split()
        current = []
        for word in words:
            if word and set(word) <= set(";&|"):
                if current:
                    commands.append(current)
                current = []
            else:
                current.append(word)
        if current:
            commands.append(current)
    return commands


def strip_prefixes(words):
    """The command without sudo / env / nohup / time prefixes and VAR=value assignments."""
    while words:
        head = words[0]
        if head in ("sudo", "env", "nohup", "time", "command", "builtin", "exec", "nice"):
            words = words[1:]
            while words and words[0].startswith("-"):
                words = words[1:]
        elif re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", head):
            words = words[1:]
        else:
            break
    return words


def check_rm(words, cwd, sudo):
    flags = "".join(w[1:] for w in words[1:] if w.startswith("-") and not w.startswith("--"))
    long_flags = [w for w in words[1:] if w.startswith("--")]
    recursive = "r" in flags.lower() or "--recursive" in long_flags
    force = "f" in flags or "--force" in long_flags
    if "--no-preserve-root" in long_flags:
        return "rm --no-preserve-root"
    if sudo and (recursive or force):
        return "sudo rm -r/-f"
    if not (recursive or force):
        return None
    for word in words[1:]:
        if word.startswith("-"):
            continue
        if word in ("*", ".", "..", "./*", "../*", "~/*", "/*") or word.endswith("/*") and expand(word[:-2], cwd) in (HOME, Path("/")):
            return f"rm -r/-f of everything in a folder ({word})"
        target = expand(word, cwd)
        if too_broad(target):
            return f"rm -r/-f of {target}, a whole system or home folder"
        if target.name == ".git":
            return "rm -r/-f of a .git folder (the repo's history)"
    return None


def check_git(words):
    args = words[1:]
    while args and args[0].startswith("-"):  # git -C dir, git -c k=v
        args = args[2:] if args[0] in ("-C", "-c") else args[1:]
    if not args:
        return None
    sub, rest = args[0], args[1:]
    if sub == "push" and any(w in ("--force", "-f") or re.match(r"^-[a-zA-Z]*f", w) or w.startswith("+") for w in rest):
        if not any(w.startswith("--force-with-lease") for w in rest):
            return "git push --force (rewrites shared history; use --force-with-lease, and ask first)"
    if sub == "reset" and "--hard" in rest:
        return "git reset --hard (throws away uncommitted work)"
    if sub == "clean" and any(re.match(r"^-[a-zA-Z]*f", w) or w == "--force" for w in rest):
        return "git clean -f (deletes untracked files for good)"
    if sub in ("checkout", "restore") and ("." in rest or "--" in rest and rest[-1] in (".", "*")):
        return f"git {sub} of everything (throws away uncommitted changes)"
    if sub == "branch" and any(w in ("-D", "--delete --force") for w in rest):
        return "git branch -D (deletes an unmerged branch)"
    if sub == "stash" and rest[:1] in (["drop"], ["clear"]):
        return f"git stash {rest[0]} (deletes stashed work)"
    return None


def code_only(command):
    """The command without heredoc bodies and quoted text, which are data, not code."""
    command = re.sub(r"<<-?\s*(['\"]?)(\w+)\1[^\n]*\n.*?\n\s*\2\s*(\n|$)", "\n", command, flags=re.S)
    return re.sub(r"'[^']*'|\"(\\.|[^\"\\])*\"", "''", command)


def check_bash(command, cwd):
    code = code_only(command)
    if re.search(r":\(\)\s*\{.*:\s*\|\s*:.*\}", code):
        return "a fork bomb"
    if re.search(r"\b(curl|wget)\b[^|]*\|\s*(sudo\s+)?(ba|z|da|k)?sh\b", code):
        return "piping a download into a shell (download it, read it, then run it)"
    for words in split_commands(command):
        sudo = bool(words) and words[0] == "sudo"
        words = strip_prefixes(words)
        if not words:
            continue
        name = os.path.basename(words[0])
        if name == "rm":
            reason = check_rm(words, cwd, sudo)
        elif name == "git":
            reason = check_git(words)
        elif name == "dd" and any(w.startswith("of=/dev/") for w in words):
            reason = "dd onto a device (wipes a disk)"
        elif name.startswith("mkfs") or name == "diskutil" and any(w.startswith("erase") for w in words):
            reason = "formatting or erasing a disk"
        elif name in ("chmod", "chown") and any(w in ("-R", "-r", "--recursive") for w in words):
            targets = [expand(w, cwd) for w in words[1:] if not w.startswith("-")]
            reason = next((f"{name} -R on {t}" for t in targets if too_broad(t)), None)
        else:
            reason = None
        if reason:
            return reason
        if name not in LOOK_ONLY:
            for word in words[1:]:
                if word.startswith("-") or "=" in word and not word.startswith(("/", "~", ".")):
                    continue
                if is_secret(expand(word, cwd)):
                    return f"reading or copying a secret ({word})"
    return None


def check_file(tool, path, cwd):
    if not path:
        return None
    target = expand(path, cwd)
    if is_secret(target):
        return f"{tool} of a secret ({target})"
    if tool != "Read" and target.name in LOCKFILES:
        return f"editing the lockfile {target.name} by hand (run the package manager instead)"
    return None


def main():
    if os.environ.get("HIVEY_SAFETY", "").lower() in ("off", "0", "false"):
        return 0
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    tool = event.get("tool_name", "")
    data = event.get("tool_input") or {}
    cwd = event.get("cwd") or os.getcwd()
    if tool == "Bash":
        reason = check_bash(data.get("command", ""), cwd)
    elif tool in ("Edit", "MultiEdit", "Write", "NotebookEdit", "Read"):
        reason = check_file(tool, data.get("file_path") or data.get("notebook_path"), cwd)
    else:
        reason = None
    if not reason:
        return 0
    print(
        f"hivey safety blocked this: {reason}. If it is really needed, ask the user to do it "
        "themselves or to confirm it explicitly; never work around this check.",
        file=sys.stderr,
    )
    return 2


if __name__ == "__main__":
    sys.exit(main())
