#!/usr/bin/env python3
"""Check that a hivey agent's Status is short, current and honest ("rewrite, don't append").

Finds the Status block between <!-- STATUS:START --> and <!-- STATUS:END --> in CLAUDE.md, else
STATUS.md. Fails when:
  - it has more than --max-lines non-empty lines (default 40) or --max-chars (default 5000): old
    lines belong in the wiki/log;
  - its first lines carry no YYYY-MM-DD date;
  - that date is older than today (--today, used by the done gate at the end of a run) or older
    than the folder's last git commit (stale: work happened after the Status was written);
  - a line counts unreviewed work as done ("✅ … not re-reviewed", "done (self-reviewed)", or an
    "unreviewed" line under a Done/Shipped heading).
Why: the verifier audit of 2026-11-05 found 24-34 KB Status files, 92% stale, that named wrong rows and
called posted posts "queued", and fixes graded by the agent itself filed under "done".

    status_check.py <folder> [--today] [--max-lines 40]      exit 0 = OK, 1 = problems (printed)
    status_check.py --all ~/agents                           report for every agent folder
"""
import argparse
import datetime as dt
import re
import subprocess
import sys
from pathlib import Path

DATE_RE = re.compile(r"\b(20\d\d-\d\d-\d\d)\b")
UNREVIEWED_RE = re.compile(r"not (re-?)?reviewed|unreviewed|self-?reviewed|self-?graded|no (re-?)?review", re.I)
DONE_RE = re.compile(r"✅|\bdone\b|\bcomplete[d]?\b|\bshipped\b|built and reviewed|\bfixed\b", re.I)


def find_status(folder):
    claude = folder / "CLAUDE.md"
    if claude.exists():
        text = claude.read_text(errors="ignore")
        m = re.search(r"<!--\s*STATUS:START\s*-->(.*?)<!--\s*STATUS:END\s*-->", text, re.S)
        if m:
            return "CLAUDE.md Status", m.group(1)
    status = folder / "STATUS.md"
    if status.exists():
        return "STATUS.md", status.read_text(errors="ignore")
    return None, None


def last_commit_date(folder):
    try:
        r = subprocess.run(["git", "-C", str(folder), "log", "-1", "--format=%cs", "--", "."],
                           capture_output=True, text=True, timeout=20)
        return dt.date.fromisoformat(r.stdout.strip()) if r.returncode == 0 and r.stdout.strip() else None
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def check(folder, today=False, max_lines=40, max_chars=5000):
    folder = Path(folder).expanduser()
    where, body = find_status(folder)
    if body is None:
        return None, []
    lines = [l for l in body.splitlines() if l.strip()]
    problems = []
    if len(lines) > max_lines:
        problems.append(f"{where} has {len(lines)} lines (max {max_lines}): rewrite it, don't append. "
                        "Move history and finished items into your wiki or log, keep only what's current.")
    size = sum(len(l) for l in lines)
    if size > max_chars:
        problems.append(f"{where} is {size} characters (max {max_chars}): it's re-read every turn. Keep "
                        "it to the current state; move history into your wiki or log.")
    date = None
    for l in lines[:5]:
        m = DATE_RE.search(l)
        if m:
            date = dt.date.fromisoformat(m.group(1))
            break
    if not date:
        problems.append(f"{where} has no YYYY-MM-DD date in its first lines (add **Updated:** YYYY-MM-DD HH:MM).")
    else:
        if today and date < dt.date.today():
            problems.append(f"{where} is dated {date}, not today: rewrite it for this run.")
        commit = last_commit_date(folder)
        if commit and date < commit:
            problems.append(f"{where} is dated {date} but this folder has commits from {commit}: it's stale.")
    heading = ""
    for l in lines:
        if l.lstrip().startswith("#") or re.match(r"^\*\*[^*]+:\*\*\s*$", l.strip()):
            heading = l
        if UNREVIEWED_RE.search(l) and (DONE_RE.search(l) or DONE_RE.search(heading)):
            problems.append(f"{where} counts unreviewed work as done: {l.strip()[:160]!r}. Either get it "
                            "reviewed (fresh review) or list it under open/pending, not done.")
    return where, problems


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("folder", nargs="?", default=".")
    ap.add_argument("--today", action="store_true", help="the Status must be dated today (end of a run)")
    ap.add_argument("--max-lines", type=int, default=40)
    ap.add_argument("--max-chars", type=int, default=5000)
    ap.add_argument("--all", metavar="DIR", help="report on every sub-folder of DIR that has a Status")
    a = ap.parse_args()
    if a.all:
        bad = 0
        for f in sorted(Path(a.all).expanduser().iterdir()):
            if not f.is_dir():
                continue
            where, problems = check(f, False, a.max_lines, a.max_chars)
            if where is None:
                continue
            bad += bool(problems)
            print(f"{'FAIL' if problems else 'OK  '} {f.name}: " + (" | ".join(p[:110] for p in problems) or where))
        sys.exit(1 if bad else 0)
    where, problems = check(a.folder, a.today, a.max_lines, a.max_chars)
    if where is None:
        print("STATUS OK (no Status block or STATUS.md here)")
        return
    if problems:
        print("STATUS FAIL:\n- " + "\n- ".join(problems))
        sys.exit(1)
    print(f"STATUS OK ({where})")


if __name__ == "__main__":
    main()
