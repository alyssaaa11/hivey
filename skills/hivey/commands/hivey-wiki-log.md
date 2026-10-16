---
name: hivey-wiki-log
description: >
  Show the most recent entries of the log.md of the wiki of the agent/swarm you are in (its `<name>-wiki/` folder in
  the same directory).
  Usage: /hivey-wiki-log [count]
argument-hint: [count]
allowed-tools:
  - Bash
---

# Wiki Log

Arguments: `$ARGUMENTS` — optional number of entries (default 10).

## Finding the vault (do this first)
The wiki is the `<name>-wiki/` folder of the agent or swarm you are working in (older vaults: `obsidian/`), the same
as that agent's own memory: walk up from the current directory and take the first such folder that contains a
`CLAUDE.md` (call it `<vault>`).
Only when there is none, and the user gave a slug, fall back to `~/Obsidian/<slug>-wiki/`. Never pick another
agent's vault by guessing. An optional first word that is not a path and not part of the request is a slug only for
that fallback. Where the vault's own `CLAUDE.md` puts `index.md` and `log.md` (vault root or `wiki/`) and what
format its log uses, follow it; do not impose another layout.

```bash
count="${ARGUMENTS:-10}"; p="$PWD"; v=""
while [ "$p" != "/" ]; do
  v=$(find "$p" -mindepth 2 -maxdepth 2 -name CLAUDE.md \( -path "$p/*-wiki/CLAUDE.md" -o -path "$p/obsidian/CLAUDE.md" \) 2>/dev/null | head -1)
  [ -n "$v" ] && { v=$(dirname "$v"); break; }; p=$(dirname "$p")
done
[ -n "$v" ] || { echo "No <name>-wiki/ vault in this folder or above"; exit 1; }
log="$v/log.md"; [ -f "$log" ] || log="$v/wiki/log.md"
[ -f "$log" ] || { echo "No log.md in $v"; exit 1; }
grep -nE "^(## \[)?[0-9]{4}-[0-9]{2}-[0-9]{2}" "$log" | head -"$count"
```

Note: some vaults list newest first (head), others append (use tail); check the vault's CLAUDE.md. Offer to dig
into noteworthy entries.
