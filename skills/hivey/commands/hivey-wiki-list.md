---
name: hivey-wiki-list
description: >
  List the wiki vault of the agent/swarm you are in (its `<name>-wiki/` folder in the same directory) and, with
  `--all`, every `<name>-wiki/` vault under ~/swarms and ~/agents plus the legacy ~/Obsidian/*-wiki/ vaults, each with
  purpose, page count and last log entry.
  Usage: /hivey-wiki-list [--all]
allowed-tools:
  - Bash
---

# Wiki List

Default: report only the vault of the folder you are in (nearest `<name>-wiki/`, or older `obsidian/`, with a
`CLAUDE.md`, walking up).
With `--all`: also every other vault.

```bash
bash -c '
shopt -s nullglob
show() { d="$1"; name="$2"
  purpose=$(grep -m1 -v "^#" "$d/CLAUDE.md" 2>/dev/null | grep -v "^$" | head -c 140)
  pages=$(find "$d/wiki" -name "*.md" 2>/dev/null | wc -l | tr -d " ")
  last=$( (grep -m1 -E "^(## \[)?[0-9]{4}-[0-9]{2}-[0-9]{2}" "$d/log.md" || grep -E "^## \[[0-9]{4}" "$d/wiki/log.md" | tail -1) 2>/dev/null | head -c 100)
  printf "• %s  (%s)\n  %s pages | last: %s\n  %s\n\n" "$name" "$d" "$pages" "${last:-never}" "${purpose:-(no purpose)}"; }
here=""; p="$PWD"
while [ "$p" != "/" ]; do
  here=$(find "$p" -mindepth 2 -maxdepth 2 -name CLAUDE.md \( -path "$p/*-wiki/CLAUDE.md" -o -path "$p/obsidian/CLAUDE.md" \) 2>/dev/null | head -1)
  [ -n "$here" ] && { here=$(dirname "$here"); break; }; p=$(dirname "$p")
done
[ -n "$here" ] && show "$here" "this folder" || echo "No <name>-wiki/ vault in this folder or above."
if [ "$1" = "--all" ]; then
  for d in ~/swarms/*/*-wiki ~/swarms/*/obsidian ~/agents/*/*-wiki ~/agents/*/obsidian ~/Obsidian/*-wiki; do
    [ -f "$d/CLAUDE.md" ] && [ "$(cd "$d" && pwd -P)" != "$(cd "${here:-/}" && pwd -P)" ] && show "$d" "$(basename "$d")"
  done
fi' _ $ARGUMENTS
```

If no vault exists, suggest `/hivey-create-wiki`.
