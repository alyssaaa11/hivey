---
name: hivey-context7
description: >
  Look up current documentation for any library, framework, SDK, API, CLI or cloud service in
  seconds with Context7 (context7.com), straight from the shell: find the library, then fetch
  the docs and code snippets for the exact question. Works in every agent hivey runs (Claude
  Code, Codex, Grok, Antigravity) with no MCP setup. Use it FIRST whenever you need API syntax,
  configuration, setup steps, version changes or library-specific debugging, even for
  well-known libraries (React, Next.js, Expo, Prisma, Tailwind, Django…): your training data
  may be out of date. Not for general programming concepts or the user's own code.
allowed-tools:
  - Bash
---

> Local additions: if `EXTRA_SKILL.md` exists in this skill's folder, read it too and follow it.
> It holds this machine's own additions; hivey updates never touch it, so put local changes
> there, never here.

# hivey context7: fast, current docs

Context7 serves up-to-date documentation and code snippets for thousands of libraries, picked
for a question. It is faster and more precise than a web search for library docs, so use it
before searching the web or guessing from memory.

## 1. Find the library ID

```bash
curl -s "https://context7.com/api/v2/libs/search?libraryName=<library>&query=<what you need>" \
  ${CONTEXT7_API_KEY:+-H "Authorization: Bearer $CONTEXT7_API_KEY"}
```

The JSON `results` list each library's `id` (e.g. `/vercel/next.js`), `title`, `description`,
`totalSnippets` and `trustScore`. Pick the official one (highest trust and snippets, matching
title). Skip this step when you already know the ID, or when the user gave one
(`/org/project`, optionally `/org/project/<version>`).

## 2. Fetch the docs for the question

```bash
curl -s "https://context7.com/api/v2/context?libraryId=<id>&query=<specific question>&type=txt" \
  ${CONTEXT7_API_KEY:+-H "Authorization: Bearer $CONTEXT7_API_KEY"}
```

Plain text: titled snippets with their source URL and code. URL-encode the query (spaces as
`%20`). Make the query specific ("app router root layout", not "next.js"): the snippets are
chosen for it. Ask again with another query for another part of the answer. `type=json`
returns `codeSnippets` and `infoSnippets` instead.

## Rules

- Answer from what Context7 returns and cite the source URLs it gives. If it has nothing
  useful, say so, then fall back to the library's official docs on the web.
- Without an API key the rate limit is low: on HTTP `429`, wait for `Retry-After` seconds. A
  key (free at context7.com) goes in `CONTEXT7_API_KEY` (from the user's secret store, never
  written into a file or prompt).
- Never send private code, secrets or personal data in a query: it goes to context7.com.
