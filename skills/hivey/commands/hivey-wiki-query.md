---
name: hivey-wiki-query
description: >
  Answer a question from the wiki of the agent/swarm you are in (its own `<name>-wiki/` folder in the same
  directory): read index.md, synthesize from the relevant pages, cite them, offer to file the answer back.
  Usage: /hivey-wiki-query <question>
argument-hint: <question>
allowed-tools:
  - Bash
  - Read
  - Write
  - Glob
  - Grep
---

# Wiki Query

Arguments: `$ARGUMENTS` — the question.

## Finding the vault (do this first)
The wiki is the `<name>-wiki/` folder of the agent or swarm you are working in (older vaults: `obsidian/`), the same
as that agent's own memory: walk up from the current directory and take the first such folder that contains a
`CLAUDE.md` (call it `<vault>`).
Only when there is none, and the user gave a slug, fall back to `~/Obsidian/<slug>-wiki/`. Never pick another
agent's vault by guessing. An optional first word that is not a path and not part of the request is a slug only for
that fallback. Where the vault's own `CLAUDE.md` puts `index.md` and `log.md` (vault root or `wiki/`) and what
format its log uses, follow it; do not impose another layout.

## Steps

1. Resolve `<vault>`. If no question was given, ask for it.
2. Read `<vault>/index.md` first (or `wiki/index.md` per the vault's CLAUDE.md), then `<vault>/CLAUDE.md` for its
   question-answering rules.
3. Pick the relevant pages from the index, read them, synthesize with citations to the pages used
   (`(see wiki/<page>.md)`). If no page is relevant, say so; do not invent.
4. If the synthesis is worth keeping, **offer** to file it as a new page (propose path and one-line summary);
   write only after confirmation, then update the index and append a `query` line to the log in the vault's format.
