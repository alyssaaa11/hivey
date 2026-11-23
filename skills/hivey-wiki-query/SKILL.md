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
  - Agent
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.
>
> Arguments: Claude Code puts what the user wrote after the skill name in the arguments
> placeholder below; in Codex (or any other agent), take it from the user's request.

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
3. Pick the relevant pages from the index (for a large vault and a broad question, sweep it with
   parallel subagents first, as in "Broad search with parallel subagents" below), read them, synthesize with citations to the pages used
   (`(see wiki/<page>.md)`). If no page is relevant, say so; do not invent.
4. If the synthesis is worth keeping, **offer** to file it as a new page (propose path and one-line summary);
   write only after confirmation, then update the index and append a `query` line to the log in the vault's format.

## Broad search with parallel subagents

Use this on top of the steps above when the wiki is large and the question is broad. It trades
tokens for breadth: several cheap subagents each sweep one slice of the wiki and report back, so
you see findings on pages you would never have opened.

Measured once (superintel wiki, 322 pages, ~400K words, 2026-11-23, one question): three Sonnet
subagents touched ~40 pages against 11 solo, returned about twice the quantified findings, and
flagged ~10 contradictions or wiki defects, in the same wall time (~30 s), for ~200K Sonnet
tokens. 7 of 7 spot-checked numbers were correct. Depth was not better: they mostly read excerpts.

### When to fan out

Fan out when the vault has more than about 100 pages **and** at least one of these holds:
- the question is cross-cutting ("everything on X", "what do we know about X", compare, list all);
- the index has no single hub page for the topic, or the hub page looks stale;
- the user asks for a broad, deep or exhaustive answer, or for contradictions.

Answer solo (the steps above) for a lookup, a question one hub page clearly covers, or a small vault.
If you cannot spawn subagents in this harness, do the same sweep yourself, slice by slice.

### How

1. Do steps 1–2 as usual, and read the hub page for the topic yourself if there is one.
2. Count pages and words per folder (`ls | wc -l`, `cat *.md | wc -w`). Split the vault into
   2–4 slices of similar size; split a big folder by filename range. Every page belongs to
   exactly one slice.
3. Launch one subagent per slice, **all in one message** so they run in parallel: read-only,
   fresh context, a cheap model (Sonnet in Claude Code). Give each the prompt below.
4. While they run, do not re-read their slices. Wait for all of them.
5. Merge: drop duplicates, keep the page path on every claim, and keep the vault's own claim
   labels. Before answering, open the pages for at least 3 numbers you did not read yourself and
   check them. A claim only one subagent reports, with no page path, is left out.
6. Answer with citations as in step 3 above. List separately the contradictions and wiki
   defects the subagents found (unreconciled numbers, uneven labels, missing sources) and offer
   to fix them; do not fix them silently.

### Subagent prompt

```
Read-only research task on the wiki at <vault>. Do not edit any file.

Question: <the question, with its synonyms and related terms spelled out>

Your slice: ONLY <folders or filename range>. Other agents cover the rest.
Grep for the terms and their synonyms first, then open and read the pages that matter; read the
5 most relevant pages in full, not just the matching lines. Go beyond pages that use the literal
phrase.

Report in this shape, under 700 words:
1. PAGES READ: paths, saying which you read in full and which only as excerpts.
2. FINDINGS: up to 15 bullets, each one claim with its number, its source id if the page gives
   one, the page's claim label, and the page path. Quote numbers exactly as the page states
   them; never fill in from your own memory.
3. CONTRADICTIONS or tensions between pages, or "none found".
4. GAPS: what the question needs that your slice does not contain.
Mark anything you are unsure of as unsure.
```
