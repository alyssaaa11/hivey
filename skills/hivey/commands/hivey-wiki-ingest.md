---
name: hivey-wiki-ingest
description: >
  Ingest a source into the wiki of the agent/swarm you are in (its own `<name>-wiki/` folder in the same directory),
  following that vault's CLAUDE.md Ingest workflow. Copies the source to raw/, creates or updates wiki pages,
  refreshes index.md, appends to log.md.
  Usage: /hivey-wiki-ingest <source-path-or-description>
argument-hint: <source-path-or-description>
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
  - Glob
---

# Wiki Ingest

Arguments: `$ARGUMENTS` — a file path to ingest, or a free-form description of what to file.

## Finding the vault (do this first)
The wiki is the `<name>-wiki/` folder of the agent or swarm you are working in (older vaults: `obsidian/`), the same
as that agent's own memory: walk up from the current directory and take the first such folder that contains a
`CLAUDE.md` (call it `<vault>`).
Only when there is none, and the user gave a slug, fall back to `~/Obsidian/<slug>-wiki/`. Never pick another
agent's vault by guessing. An optional first word that is not a path and not part of the request is a slug only for
that fallback. Where the vault's own `CLAUDE.md` puts `index.md` and `log.md` (vault root or `wiki/`) and what
format its log uses, follow it; do not impose another layout.

## Steps

1. Resolve `<vault>` as above. If none is found and no slug was given, say so and offer `/hivey-create-wiki`.
2. **Read `<vault>/CLAUDE.md` in full**: it is the source of truth for this wiki's ingest workflow, page format,
   citation rules and folders. Follow it exactly.
3. If the source is a file path, copy it into the right `<vault>/raw/` sub-folder (keep the filename; create
   `raw/` if missing). If it is a description, find the underlying material first and ask when unclear.
4. Execute the vault's Ingest workflow (summary page, update related pages, links, refresh `index.md`, append to `log.md`).
5. End with a short summary: pages created, pages updated, and the new `log.md` line.
