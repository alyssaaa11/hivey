---
name: hivey-wiki-lint
description: >
  Run a health check on the wiki of the agent/swarm you are in (its own `<name>-wiki/` folder in the same directory):
  contradictions, stale claims, orphan pages, missing cross-references, format compliance, source-coverage gaps.
  Reports findings as a numbered list, does not auto-fix.
  Usage: /hivey-wiki-lint
allowed-tools:
  - Bash
  - Read
  - Glob
  - Grep
---

# Wiki Lint

## Finding the vault (do this first)
The wiki is the `<name>-wiki/` folder of the agent or swarm you are working in (older vaults: `obsidian/`), the same
as that agent's own memory: walk up from the current directory and take the first such folder that contains a
`CLAUDE.md` (call it `<vault>`).
Only when there is none, and the user gave a slug, fall back to `~/Obsidian/<slug>-wiki/`. Never pick another
agent's vault by guessing. An optional first word that is not a path and not part of the request is a slug only for
that fallback. Where the vault's own `CLAUDE.md` puts `index.md` and `log.md` (vault root or `wiki/`) and what
format its log uses, follow it; do not impose another layout.

## Steps

1. Resolve `<vault>`. If none is found, stop with a clear error.
2. **Read `<vault>/CLAUDE.md`**: it defines the lint workflow (checks, what counts as stale, coverage gaps). Follow it.
3. Walk every file under `<vault>/wiki/` (and `index.md`). Use Grep for the wiki-link inventory
   (`\[\[([^\]]+)\]\]`) and for the page-format fields the vault requires.
4. Report a numbered list: the issue, the affected page(s), a suggested fix. Do **not** auto-fix.
5. Append a `lint` line to the vault's `log.md` in its own format (`Findings: N contradictions, M orphans, ...`).
   Offer to file the findings as a page; do not assume.
