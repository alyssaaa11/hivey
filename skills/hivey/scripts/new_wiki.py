#!/usr/bin/env python3
"""Create an Obsidian wiki vault as the memory of a hivey agent or swarm (LLM-wiki pattern).

usage: new_wiki.py <name> [--root FOLDER] [--look-from VAULT] [--agent FOLDER]... [--about TEXT]

The vault lives inside the swarm's or agent's own folder, so everything travels together:
creates <FOLDER>/obsidian/ (raw/ for sources, wiki/ for maintained pages, index.md, log.md,
CLAUDE.md with the rules, .obsidian/ so Obsidian opens it as a vault) and adds a "Memory"
section to each --agent FOLDER's CLAUDE.md pointing at it with a relative path. FOLDER is the
swarm root or the solo agent's folder (default: the current folder). An existing vault is
reused, never overwritten.
Obsidian keeps the look per vault, so a new vault copies the theme, CSS snippets and Style
Settings of the vault remembered as --look-from (kept in ~/.hivey/wiki.json); without one it
opens in stock Obsidian.
Prints JSON: {"ok": true, "path": "...", "existing": false, "look": true, "agents": [...]}.
"""
import argparse
import json
import os
import shutil
import sys
import time
from pathlib import Path

SETTINGS = Path.home() / ".hivey" / "wiki.json"
MARKER = "<!-- hivey: wiki memory -->"
VAULT = "obsidian"

RULES = """# {name} wiki: rules for the agents that keep it

This vault is the long-term memory of **{name}**{about}. Agents read it before working and
write what they learn, so knowledge survives restarts and new sessions.

## Layout
- `raw/`: sources as they came in (articles, notes, transcripts, exports). Never edited.
- `wiki/`: maintained pages, one topic per page, linked with [[wikilinks]].
- `index.md`: every wiki page with a one-line summary. Keep it current.
- `log.md`: newest first, one line per change: `YYYY-MM-DD what changed (pages)`.

## Working with it
1. Before a task: read `index.md`, then the pages that matter.
2. New source: save it under `raw/`, then update or create the `wiki/` pages it informs.
3. New finding or decision: add it to the right page (create one if none fits), add the page
   to `index.md`, and a line to `log.md`.
4. Prefer updating a page over creating a near-duplicate. Cite the `raw/` file a fact came from.
"""

MEMORY = """
{marker}
## Memory (Obsidian wiki)

Your long-term memory is the Obsidian vault `{path}` (relative to this file's folder). Read
its `CLAUDE.md` (the rules) and `index.md` before working; save sources in `raw/`, keep
`wiki/` pages, `index.md` and `log.md` up to date as you learn. Don't keep knowledge only in
this conversation.
"""


def setting(key, given):
    """The given path (then remembered in ~/.hivey/wiki.json), else the remembered one."""
    try:
        saved = json.loads(SETTINGS.read_text())
    except (OSError, ValueError):
        saved = {}
    if given:
        saved[key] = str(Path(given).expanduser())
        SETTINGS.parent.mkdir(parents=True, exist_ok=True)
        SETTINGS.write_text(json.dumps(saved) + "\n")
    value = saved.get(key)
    return Path(value).expanduser() if value else None


def copy_look(source, dest):
    """Copies the source vault's theme, snippets and Style Settings into dest/.obsidian."""
    src, dst = source / ".obsidian", dest / ".obsidian"
    if not (src / "appearance.json").is_file():
        return False
    shutil.copy2(src / "appearance.json", dst / "appearance.json")
    for folder in ("themes", "snippets"):
        if (src / folder).is_dir():
            shutil.copytree(src / folder, dst / folder)
    style = src / "plugins" / "obsidian-style-settings"
    if style.is_dir():
        shutil.copytree(style, dst / "plugins" / style.name)
        (dst / "community-plugins.json").write_text(json.dumps([style.name]) + "\n")
    return True


def create(path, name, about):
    (path / ".obsidian").mkdir(parents=True)
    (path / "raw").mkdir()
    (path / "wiki").mkdir()
    (path / "CLAUDE.md").write_text(RULES.format(name=name, about=f" ({about})" if about else ""))
    (path / "index.md").write_text(f"# {name} wiki: index\n\n(no pages yet)\n")
    (path / "log.md").write_text(f"# {name} wiki: log\n\n{time.strftime('%Y-%m-%d')} vault created\n")


def link(agent_dir, path):
    """Adds the Memory section to the agent's CLAUDE.md once."""
    brief = Path(agent_dir).expanduser() / "CLAUDE.md"
    text = brief.read_text() if brief.exists() else ""
    if MARKER in text:
        return False
    brief.parent.mkdir(parents=True, exist_ok=True)
    relative = os.path.relpath(path, brief.parent.resolve())
    brief.write_text(text.rstrip("\n") + "\n" + MEMORY.format(marker=MARKER, path=relative))
    return True


def main():
    parser = argparse.ArgumentParser(description="Obsidian wiki memory for a hivey agent or swarm")
    parser.add_argument("name")
    parser.add_argument("--root", default=".", help="swarm or agent folder; the vault is <root>/obsidian")
    parser.add_argument("--look-from", help="vault whose theme new vaults copy; remembered")
    parser.add_argument("--agent", action="append", default=[], help="agent folder to link")
    parser.add_argument("--about", default="", help="one line: what the agent or swarm does")
    args = parser.parse_args()
    look_from = setting("look_from", args.look_from)
    path = Path(args.root).expanduser().resolve() / VAULT
    existing = path.exists()
    look = False
    if not existing:
        create(path, args.name, args.about)
        look = bool(look_from) and copy_look(look_from, path)
    linked = [str(Path(a).expanduser()) for a in args.agent if link(a, path)]
    print(json.dumps({"ok": True, "path": str(path), "existing": existing, "look": look, "agents": linked}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
