#!/usr/bin/env python3
"""Merge the swarm's house rules into a CLAUDE.md (default: <root>/CLAUDE.md, the master's).

The rules live in ../references/shared-rules.md between hivey-swarm:shared-rules markers.
Every agent loads <root>/CLAUDE.md through its parent folder, so they apply to the whole swarm.
- File missing → created with the rules.
- Marker block present → replaced with the current rules (keeps them up to date).
- No markers but every rule heading already present (e.g. a hand-written CLAUDE.md) → left alone.
- Otherwise → appended at the end. Existing content is never removed.

Usage: shared_rules.py <swarm_root | path/to/CLAUDE.md>
"""
import argparse
import json
import re
from pathlib import Path

RULES_FILE = Path(__file__).resolve().parent.parent / "references" / "shared-rules.md"
BLOCK = re.compile(r"<!-- hivey-swarm:shared-rules:start -->.*?<!-- hivey-swarm:shared-rules:end -->", re.S)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("target", help="swarm root folder, or a CLAUDE.md path")
    args = p.parse_args()

    rules = RULES_FILE.read_text().strip()
    headings = [line for line in rules.splitlines() if line.startswith("# ")]
    target = Path(args.target).expanduser().resolve()
    if target.is_dir():
        target = target / "CLAUDE.md"
    text = target.read_text() if target.exists() else ""

    if BLOCK.search(text):
        new, action = BLOCK.sub(lambda _: rules, text), "updated"
    elif text and headings and all(h in text for h in headings):
        new, action = text, "already present (hand-written), left alone"
    else:
        new = (text.rstrip() + "\n\n" if text else "") + rules + "\n"
        action = "appended" if text else "created"
    if new != text:
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(new)
    elif action == "updated":
        action = "unchanged"
    print(json.dumps({"file": str(target), "result": action}))


if __name__ == "__main__":
    main()
