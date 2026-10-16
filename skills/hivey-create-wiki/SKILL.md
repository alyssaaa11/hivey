---
name: hivey-create-wiki
description: >
  Create the Obsidian wiki of a hivey agent or swarm (LLM Wiki pattern, Karpathy): a
  <name>-wiki/ vault inside the agent's or swarm's own folder with raw/ + wiki/, a CLAUDE.md
  schema tailored to the agent's domain, index.md and log.md, and a section in the agent's
  CLAUDE.md (each swarm agent's) pointing at it. Only for agents and swarms. Use whenever the
  user wants a wiki, a knowledge base or long-term memory for an agent or a swarm ("create
  wiki", "new wiki", "wiki for <agent>", "/hivey-create-wiki"), and whenever an agent or swarm
  creator sets up its memory.
argument-hint: <agent-or-swarm-name> [--in <folder>] [--agent <dir>]...
allowed-tools:
  - Bash
  - Read
  - Write
  - Edit
  - Glob
---

# hivey create wiki

Scaffold the Obsidian wiki vault of a hivey agent or swarm, following Andrej
Karpathy's [LLM Wiki pattern](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f).

The wiki is a **persistent, compounding knowledge artifact** that an LLM maintains over time,
separate from the agent's operational state. Sources go into `raw/` (read-only), the LLM writes
synthesised pages into `wiki/`, and `index.md` + `log.md` track the catalog and history.

## Arguments

`$ARGUMENTS`: the **agent or swarm name** (e.g. `btc`, `gap-finder`), then optional flags:

- `--in <folder>`: the agent's or swarm's folder; the vault is created at
  `<folder>/<slug>-wiki/`, next to the agents that use it, so the agent or swarm and its
  knowledge travel together and can be shared as one folder. **Default when the current
  folder (or one given by the caller) has a `CLAUDE.md`:** `--in .`. Agent and swarm creators
  always pass it; never ask the user where their Obsidian is.
- `--agent <dir>` (repeatable): agent folders whose `CLAUDE.md` gets the wiki section (a
  swarm's agents). With `--in` and no `--agent`, the folder's own `CLAUDE.md` is patched when
  it exists.
This skill only makes wikis for agents and swarms. With no `--in` and no `CLAUDE.md` in the
current folder, ask which agent or swarm folder the wiki is for (it must have a `CLAUDE.md`,
or be a swarm root with agent folders); don't create a wiki that belongs to no agent.

If no name is given, use the folder's name; the hivey agent's folder `~/.hivey/agent` is named
`hivey` (its wiki is `~/.hivey/agent/hivey-wiki`).

Why `<slug>-wiki`: Obsidian lists vaults by folder name, so each wiki gets its own name in the
vault switcher. Older hivey vaults are called `obsidian/`; `hivey migrate` renames them. The
wiki commands (`/hivey-wiki-ingest`, `/hivey-wiki-query`, `/hivey-wiki-lint`,
`/hivey-wiki-log`, `/hivey-wiki-list`) find the nearest `<slug>-wiki/` with a `CLAUDE.md`,
walking up from the current folder.

## Solo agent or swarm (detected, never asked)

- **Solo agent**: the folder has a `CLAUDE.md` and no `--agent` folders: one brief to read
  and patch, the vault at `<folder>/<slug>-wiki/`.
- **Swarm**: `--agent` folders were given (or the folder's sub-folders have their own
  `CLAUDE.md`): read the root's and every agent's brief, one shared vault at
  `<root>/<slug>-wiki/`, every agent's brief patched with `../<slug>-wiki/…` paths.

## Step 1: Validate

1. Slugify the name: lowercase, hyphenated, no surrounding whitespace. Reject a path
   separator, a leading dot, or an empty result. Drop a trailing `-wiki` (it is added).
2. If `<folder>/<slug>-wiki/CLAUDE.md` (or an older `<folder>/obsidian/CLAUDE.md`) already
   exists, say the wiki exists and stop. Never overwrite: it protects accumulated knowledge.

## Step 2: Gather context

Read the folder's `CLAUDE.md` and each `--agent` folder's `CLAUDE.md` and extract:

- **Purpose**: what the agent or swarm does, in one paragraph.
- **Key concepts / domain entities** it reasons about repeatedly.
- **Recurring source types**: where its inputs come from (digests, reports, emails, articles,
  papers, transcripts).
- **What the wiki must NOT duplicate**: notebooks or logs it already keeps (a progress file,
  a strategies list, task boards).

Infer 4-6 sub-folders for `wiki/` and 2-4 for `raw/`:

| Agent kind | wiki/ sub-folders | raw/ sub-folders |
|---|---|---|
| trading bot | strategies, indicators, regimes, signals, lessons | digests, reflects, research, market |
| email reviewer | senders, threads, topics, actions, lessons | inbox-snapshots, rules, research |
| research agent | sources, topics, entities, syntheses, lessons | papers, articles, transcripts |
| content creator | formats, audiences, voice, ideas, lessons | drafts, refs, examples |

If the domain fits none of these, infer reasonable names from the briefs and go on without
asking; the schema says this is an initial structure.

## Step 3: Create the vault

Use hivey's script (in the hivey skill: `~/.claude/skills/hivey/scripts/new_wiki.py`, or
`~/.codex/skills/hivey/scripts/new_wiki.py`). It creates `<folder>/<slug>-wiki/` with `raw/`,
`wiki/`, `index.md`, `log.md`, a starter `CLAUDE.md` and `.obsidian/` (so Obsidian opens it as
a vault), copies the user's Obsidian look (theme, snippets, Style Settings) from the vault
remembered in `~/.hivey/wiki.json`, and adds a short "Memory" section to each `--agent`
folder's `CLAUDE.md`:

```bash
python3 ~/.claude/skills/hivey/scripts/new_wiki.py <slug> --root <folder> \
  --agent <folder or each swarm agent folder> ... --about "<one line: what it does>"
```

It prints JSON with the vault `path`.

If the user has a main vault whose look new vaults should copy and none is remembered yet,
pass `--look-from <that vault>` once (it is remembered).

Then add the sub-folders from Step 2:

```bash
mkdir -p <vault>/raw/<sub1> <vault>/raw/<sub2> ... <vault>/wiki/<sub1> <vault>/wiki/<sub2> ...
```

## Step 4: Write the wiki's CLAUDE.md

Replace the starter `<vault>/CLAUDE.md` with the "Wiki CLAUDE.md template" below, filled with
the context from Step 2. Keep every section (the wiki LLM reads only this file to learn its
contract): Purpose (concrete, not template wording), Scope rules, Folder structure (the actual
sub-folders), Ingest workflow, Page format, Citation rules (adapted to the source types),
Question answering, Lint workflow, Log format, Hard rules.

## Step 5: Seed index.md and log.md

`<vault>/index.md`: a section header per `wiki/` sub-folder, each empty with a note that pages
are filed on first ingest. `<vault>/log.md`: a one-line preamble, the log format, and one
`bootstrap` entry dated today (`date -u +%Y-%m-%d`).

## Step 6: Patch the agents' CLAUDE.md

The script added a short "Memory" section. Below it, add the "Agent CLAUDE.md section
template" to the folder's `CLAUDE.md` and each `--agent` folder's `CLAUDE.md`. Write vault
paths **relative to that file's folder**: `<slug>-wiki/…` for the agent's own folder,
`../<slug>-wiki/…` for a swarm agent one level down. Never absolute, so the folder still works
after it's moved or shared.

If `~/Obsidian/` exists and has no `<slug>-wiki` yet, also link it there so the user finds it
next to their other vaults: `ln -s <vault> ~/Obsidian/<slug>-wiki`.

## Step 7: Report

A short summary: vault path, solo agent or swarm, sub-folders, which `CLAUDE.md` files were patched, how to
open it ("Obsidian → Open another vault → Open folder as vault → `<vault>`"; on first open,
"Turn on community plugins" so Style Settings loads, when a look was copied), and how to test:
`/hivey-wiki-ingest <path-to-source>` from the agent's folder.

End with `DONE: created <slug> wiki`.

---

## Wiki CLAUDE.md template

Replace the `{{...}}` placeholders with values from Step 2. Keep all section headings.

```markdown
# LLM Wiki: {{Domain Title}}

A persistent, compounding knowledge base for {{the agent / the swarm}}.
Maintained by its agents following Andrej Karpathy's LLM Wiki pattern.

## Purpose

{{One paragraph on what this wiki accumulates, from the briefs.}}

The wiki accumulates everything learned over time about:

{{- bullet list of 4-8 areas, from the key concepts in Step 2}}

The wiki is **separate from** {{any notebook/log the agent keeps, named}} and **separate
from** {{any raw daily input, named}}. Those are sources; the wiki is the synthesis on top.

## Scope rules

- The wiki is **knowledge / synthesis only**: never live config, runnable code, or current
  operational state. Link to operational state, do not duplicate it.
- {{Any agent-specific boundary.}}

## Folder structure

\```
raw/                       -- immutable sources (never modify)
  {{sub-folders chosen in Step 2}}
wiki/                      -- pages maintained by the agents
  {{sub-folders chosen in Step 2}}
index.md                   -- catalog of all pages
log.md                     -- append-only operations log
\```

Sub-folders are an initial guess. Flatten when a folder has fewer than ~3 pages, split when
it exceeds ~15. Discuss reorganisations with the user.

## Ingest workflow

When a new source arrives in `raw/` (or the user asks to ingest something):

1. Read the full source.
2. Discuss key takeaways with the user before writing when they are around; confirm what's
   worth filing.
3. Create a summary page in the right `wiki/` sub-folder, named after the source
   (lowercase-hyphenated).
4. Update or create concept pages for each major idea, entity, or topic the source touches.
   **A single source typically touches 10-15 wiki pages; that is normal.**
5. Add wiki-links (`[[page-name]]`) so every page has inbound and outbound links.
6. Update `index.md` with new pages and one-line descriptions.
7. Append an entry to `log.md` (see Log format).

## Page format

\```markdown
# Page Title

**Summary**: One to two sentences describing this page.

**Sources**: List of raw source files this page draws from.

**Last updated**: YYYY-MM-DD

---

Main content. Clear headings, short paragraphs. Link related concepts with [[wiki-links]].

## Related pages

- [[related-concept-1]]
- [[related-concept-2]]
\```

Use Obsidian callouts when warranted (`> [!warning] Title`); types: `note`, `tip`, `warning`,
`danger`, `info`, `example`, `quote`.

## Citation rules

- Every factual / quantitative claim references its source: `(source: filename.pdf p.42)`,
  `(source: 2026-05-11 — title.md)`, or any unambiguous pointer to a file in `raw/`.
- If two sources disagree, note it in a `> [!warning]` callout and track the open question on
  a `lessons/` (or equivalent) page.
- A claim without a source is marked `> [!info] Unverified` and flagged at the next lint.

## Question answering

1. Read `index.md` first to find relevant pages.
2. Read those pages and synthesize an answer with wiki-page citations.
3. {{If applicable: also read fresh operational data; never invent numbers.}}
4. If the answer is not in the wiki, say so.
5. If the answer is valuable, **offer to file it back as a new wiki page**.

## Lint workflow

When asked to lint or audit, look for: contradictions between pages, stale claims newer
sources superseded, orphan pages (no inbound links), concepts in 3+ pages without their own
page, cross-reference gaps, format compliance (Summary / Sources / Last updated)
{{, source-coverage gaps if applicable}}. Report a numbered list with suggested fixes; do not
auto-fix, discuss first.

## Log format

Every entry in `log.md` starts with a parseable prefix:

\```
## [2026-05-12] ingest | <source name>
- Created: wiki/<folder>/<page>.md
- Updated: wiki/<folder>/<page>.md, index.md
- Notes: <one-line context>

## [2026-05-12] query | "<question>"
- Read: wiki/<folder>/<page>.md, ...
- Filed answer back as: wiki/<folder>/<new-page>.md   (if applicable)

## [2026-05-12] lint
- Findings: N contradictions, M orphan pages, K stale points
\```

`grep "^## \[" log.md | tail -10` gives the last 10 operations.

## Hard rules

- **Never** modify anything in `raw/`.
- **Never** put live config, runnable code, or current operational state in the wiki.
- **Always** update `index.md` and `log.md` after any wiki write.
- Page filenames: lowercase-hyphenated.
- {{If applicable: measured data wins over wiki narrative; when they disagree, update the
  wiki.}}
```

---

## Agent CLAUDE.md section template

Paths relative to the patched file's folder (`{{vault}}` = `<slug>-wiki` or `../<slug>-wiki`).

```markdown
# {{Slug Title}} wiki: long-term knowledge base

You own the Obsidian wiki `{{vault}}/` (an independent vault). Its schema, page format,
citation rules and workflows are in `{{vault}}/CLAUDE.md`: **always** read it on your first
wiki touch in a session and follow it exactly.

- `{{vault}}/raw/`: immutable sources ({{sub-folders}}). Read-only.
- `{{vault}}/wiki/`: pages you write ({{sub-folders}}).
- `{{vault}}/index.md`: table of contents. Read it first when answering.
- `{{vault}}/log.md`: append-only operations log.

Scope: knowledge / synthesis ({{domain examples}}), never live config, runnable code or
current operational state. Do not duplicate {{named notebook}} or {{named raw input}}.

**This wiki is the default place for anything substantive you learn**: research findings,
summaries of what you read, decisions and their reasons, lessons from successes and failures,
useful references. Before writing a standalone note or report, ask whether it belongs in the
wiki instead; durable knowledge almost always does. Exceptions: live state, today's progress,
ephemeral status.

When to touch it:
1. After every task that produces a finding: create or update the page, update `index.md`,
   add a `log.md` entry. Filing is the default.
2. When the user adds a source: the Ingest workflow.
3. When the user asks a question: `index.md` first, answer from the pages, cite them, offer
   to file a valuable answer back.
4. When asked to lint or audit: the Lint workflow.
5. When you'd otherwise write a report: write it as a wiki page instead.

Hard rules: never modify `{{vault}}/raw/`; no live config or operational state in the wiki;
always update `index.md` and `log.md` after a wiki write; filenames lowercase-hyphenated.
```
