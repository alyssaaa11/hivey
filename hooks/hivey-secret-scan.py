#!/usr/bin/env python3
"""
PreToolUse hook: scan `git commit` invocations for secrets before allowing them.

Blocks the commit (exit 2) if it would stage:
  - A real .env file (not .env.example / .env.sample / .env.template)
  - Lines matching common secret patterns (AWS, GitHub, OpenAI, Anthropic,
    Slack, Google, generic key=value pairs, private keys)

Reads PreToolUse JSON from stdin. Only acts on Bash tool calls whose command
contains `git commit` (and not `git commit --help` / `git commit-tree` etc.).
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import sys

# High-signal secret regexes. Each tuple: (label, pattern).
SECRET_PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    ("AWS access key", re.compile(r"AKIA[0-9A-Z]{16}")),
    ("AWS secret/access key assignment",
     re.compile(r"(aws_)?(secret|access)_?key([_-]?id)?[\"'\s]*[:=][\"'\s]*[A-Za-z0-9/+=]{16,}", re.I)),
    ("GitHub PAT (classic)", re.compile(r"ghp_[A-Za-z0-9]{30,}")),
    ("GitHub OAuth token", re.compile(r"gho_[A-Za-z0-9]{30,}")),
    ("GitHub server token", re.compile(r"ghs_[A-Za-z0-9]{30,}")),
    ("GitHub fine-grained PAT", re.compile(r"github_pat_[A-Za-z0-9_]{30,}")),
    ("OpenAI key", re.compile(r"sk-[A-Za-z0-9_-]{20,}")),
    ("Anthropic key", re.compile(r"sk-ant-[A-Za-z0-9_-]{20,}")),
    ("Slack token", re.compile(r"xox[abprs]-[A-Za-z0-9-]{10,}")),
    ("Google API key", re.compile(r"AIza[0-9A-Za-z_-]{30,}")),
    ("Generic credential assignment",
     re.compile(r"\b(api[_-]?key|apikey|secret|token|password|passwd|pwd)[\"'\s]*[:=][\"'\s]*(?P<value>[A-Za-z0-9_!@#$%^&*\-]{12,})", re.I)),
    ("Private key block",
     re.compile(r"-----BEGIN (RSA |EC |DSA |OPENSSH |PGP )?PRIVATE KEY-----")),
]

# Lines containing any of these tokens are considered placeholders, not real secrets.
FALSE_POSITIVE_MARKERS = re.compile(
    r"(es_RQ_|envsave\b|YOUR_|EXAMPLE|PLACEHOLDER|\$\{|\$\(|process\.env|os\.environ|<your-|xxxx+|\*\*\*+)",
    re.I,
)

ENV_FILE_RE = re.compile(r"(^|/)\.env(\.|$)")
ENV_FILE_ALLOW_SUFFIX = re.compile(r"\.(example|sample|template)$")


def is_code_identifier_value(line: str, match: re.Match[str]) -> bool:
    """True if the matched value is a bare variable reference, not a literal.

    Catches lines like
        apikey: SUPABASE_SERVICE_ROLE_KEY,
        'xi-api-key': ELEVENLABS_API_KEY,
        next_page_token: nextPageToken,
    where the right-hand side is unquoted code (ALL_CAPS env-var name or
    camelCase identifier), not a hardcoded secret.
    """
    groups = match.groupdict() or {}
    value = groups.get("value")
    if not value:
        return False
    # Quoted values are treated as literals — real secrets usually live in
    # string form.
    start = match.start("value")
    prev = line[start - 1] if start > 0 else ""
    if prev in ('"', "'", "`"):
        return False
    # ALL_CAPS_SNAKE with at least one underscore — env-var convention.
    if re.fullmatch(r"[A-Z][A-Z0-9_]*", value) and "_" in value:
        return True
    # camelCase / PascalCase identifier with no digits — JS/TS variable.
    if re.fullmatch(r"[a-zA-Z_][a-zA-Z0-9_]*", value) and not re.search(r"\d", value):
        return True
    return False


def is_git_commit_command(cmd: str) -> bool:
    """True if the command actually invokes `git commit` (not a sibling like git commit-tree)."""
    # Strip simple comments
    cmd_no_comments = re.sub(r"#.*$", "", cmd, flags=re.MULTILINE)
    # Look for `git commit` not followed by a dash (catches `commit-tree`)
    if not re.search(r"\bgit\s+commit(?!\S)", cmd_no_comments):
        return False
    # Skip read-only / help variants
    if re.search(r"\bgit\s+commit\s+(--help|-h)\b", cmd_no_comments):
        return False
    return True


def run(cmd: list[str], cwd: str | None = None) -> tuple[int, str]:
    try:
        proc = subprocess.run(
            cmd, cwd=cwd, capture_output=True, text=True, timeout=15
        )
        return proc.returncode, proc.stdout
    except Exception as e:
        return 1, f"<error running {' '.join(cmd)}: {e}>"


def scan_staged(cwd: str) -> list[str]:
    """Return a list of human-readable findings; empty list means clean."""
    findings: list[str] = []

    # 1. .env files newly staged
    rc, names = run(["git", "diff", "--cached", "--name-only", "--diff-filter=A"], cwd=cwd)
    if rc == 0:
        for path in names.splitlines():
            path = path.strip()
            if not path:
                continue
            if ENV_FILE_RE.search(path) and not ENV_FILE_ALLOW_SUFFIX.search(path):
                findings.append(f"  .env file staged: {path}")

    # 2. Secret patterns in added lines
    rc, diff = run(["git", "diff", "--cached", "-U0"], cwd=cwd)
    if rc == 0:
        current_file = "?"
        for raw_line in diff.splitlines():
            if raw_line.startswith("+++ "):
                current_file = raw_line[4:].lstrip("b/").strip() or "?"
                continue
            if not raw_line.startswith("+") or raw_line.startswith("+++"):
                continue
            line = raw_line[1:]  # strip leading '+'
            if FALSE_POSITIVE_MARKERS.search(line):
                continue
            for label, pat in SECRET_PATTERNS:
                m = pat.search(line)
                if not m:
                    continue
                if is_code_identifier_value(line, m):
                    continue
                snippet = line.strip()
                if len(snippet) > 120:
                    snippet = snippet[:117] + "..."
                findings.append(f"  [{label}] {current_file}: {snippet}")
                break  # one finding per line is enough
    return findings


def main() -> int:
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return 0  # never block on malformed input

    if payload.get("tool_name") != "Bash":
        return 0

    cmd = (payload.get("tool_input") or {}).get("command", "")
    if not cmd or not is_git_commit_command(cmd):
        return 0

    cwd = payload.get("cwd") or os.getcwd()

    # If not inside a git repo, let the command fail naturally.
    rc, _ = run(["git", "rev-parse", "--is-inside-work-tree"], cwd=cwd)
    if rc != 0:
        return 0

    findings = scan_staged(cwd)
    if not findings:
        return 0

    msg = [
        "Commit BLOCKED by secret scanner — potential secrets in staged diff:",
        "",
        *findings,
        "",
        "Options:",
        "  1. Unstage the offending file/line and store the secret via envsave.",
        "  2. Add the file to .gitignore and unstage it.",
        "  3. If this is a confirmed false positive, ask the user before bypassing.",
        "",
        "Do NOT use --no-verify or any bypass without explicit user approval.",
    ]
    print("\n".join(msg), file=sys.stderr)
    return 2  # exit code 2 → block tool call and feed stderr back to Claude


if __name__ == "__main__":
    sys.exit(main())
