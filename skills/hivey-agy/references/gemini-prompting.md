# Prompting Gemini 3.x (what works, with sources)

Researched 2026-10-11. Sources at the bottom. Applies to every Gemini slug in `agy models`
(3.6/3.7/3.8 Flash, 3.1 Pro). Claude and gpt-oss slugs also do fine with these prompts.

## Contents
1. Core rules
2. Structure templates
3. Task-type patterns
4. Things that backfire
5. Sources

## 1. Core rules

- **Direct and short beats clever.** Gemini 3 "responds best to direct, clear instructions" and
  "may over-analyze verbose or overly complex prompt engineering techniques used for older
  models". Drop persuasion, threats, "you are the world's best…", and hand-written
  chain-of-thought scaffolds. Reasoning depth comes from the model tier and effort
  (the `-low/-medium/-high` slug), not from "think step by step".
- **Say how much output you want.** By default it's terse and direct. To get a long report, a
  friendly tone, or a full file, ask for it explicitly (length, sections, tone).
- **Role and constraints go first, the question goes last.** Put the role, hard rules and output
  format at the top. With big context (pasted docs, logs, code), put the data in the middle and
  the actual question at the **end**, opening with "Based on the information above…".
- **One consistent structure.** Use either XML-style tags or Markdown headers, not both. Define
  any ambiguous term ("short" = under 120 words; "recent" = after 2026-01-01).
- **State the done-condition.** For agentic work, say exactly what counts as finished: the file
  exists, the tests pass, the answer is a JSON object. agy keeps going until it thinks the task
  is done, so a vague done-condition gives you a vague stop.
- **Persistence for agentic tasks:** "Keep working until the task is completely done. If a tool
  or command fails, read the error and try a different approach." This is useful with
  `--dangerously-skip-permissions` runs.
- **Ambiguity:** "If something is unclear, make a reasonable assumption, state it in one line,
  and continue." Headless runs can't ask the user, and an `ask_question` call just stalls or
  gets soft-denied.
- **Leave temperature alone.** (agy doesn't expose it anyway.) Google recommends the default 1.0
  for Gemini 3, because low temperatures cause looping.

## 2. Structure templates

### Generic task (XML)
```
<role>You are a {senior X} helping with {domain}.</role>

<task>{one-paragraph statement of the goal, in the user's terms}</task>

<context>
{facts, file paths, constraints the user gave, pasted data}
</context>

<constraints>
- {hard rule 1}
- {hard rule 2}
- Verbosity: {low|medium|high}. Tone: {…}.
- If something is unclear, assume reasonably and state the assumption.
</constraints>

<output_format>
{exact shape: "a Markdown table with columns A,B,C", "only the code, one block", "≤5 bullets"}
</output_format>
```

### Long context (data first, question last)
```
<role>…</role>
<constraints>…</constraints>

<document>
{the long thing}
</document>

Based on the information above, {question}. {output format}.
```

### Agentic coding / file work (run with --cwd and usually --yolo)
```
# Goal
{what must exist or change when you're done}

# Context
- Repo: {path}. Stack: {…}. Relevant files: {…}
- Conventions: {match existing style, no new deps, …}

# Steps you may take
Read files, run {test/lint commands}, edit files inside this folder only.

# Done when
- {tests command} passes
- {file} exists and contains {…}

# Report
End with: files changed, commands run and their result, anything left undone.
```

## 3. Task-type patterns

- **Research / web questions:** "Search the web. Decompose into sub-questions, answer each from
  sources, cite every claim with its URL, and flag anything you couldn't verify." Use
  flash-high. Ask for dates, because models can mix up versions.
- **Extraction / classification:** Use `--schema` (agy's `--json-schema`) instead of describing
  JSON in prose. The prompt can then be one line. Use flash-low.
- **Summaries:** Give the audience and the length ("for a busy CTO, 5 bullets, each ≤20 words")
  and say what to keep (numbers, names, decisions).
- **Writing / creative:** Give the audience, goal, voice, banned phrases, and one short example
  of the tone. Ask for a self-review pass: "before answering, check for corporate jargon and
  rewrite it".
- **Code review / debugging:** Give the symptom, the exact error, the repro command, and what
  was already tried. Ask for the root cause first and the fix second.
- **Image generation (generate_image tool):** Describe the subject, style, composition,
  lighting, aspect ratio and any text to render, in quotes. Then give the save path:
  "save it as ./out/name.png and confirm with ls". It can fail upstream while the run still
  reports SUCCESS, so always check the file.
- **Planning-before-doing (big tasks):** "First write a short numbered plan, then execute it,
  ticking items off. At the end, verify each Done-when item." This is cheap, and it helps flash.

## 4. Things that backfire

- **Absolute negatives** like "do not infer" or "never guess". Gemini 3 can follow them too
  literally and refuse basic arithmetic or refuse to connect facts across a document. Say what
  you want instead: "only use facts from the document; if a fact is missing, say 'not in
  document'".
- **Asking about something it can't access.** Split it: "First check whether X is available;
  if it isn't, say so and stop."
- **Over-long prompts on flash-low.** Low effort plus a huge rubric gives shallow compliance.
  Either shorten the prompt or raise the effort.
- **Mixing several unrelated jobs in one `-p`.** Run them as separate calls, or as a `--chat`
  session.

## 5. Sources
- Google, Gemini 3 developer guide: https://ai.google.dev/gemini-api/docs/gemini-3
- Google Cloud, Gemini 3 prompting guide: https://docs.cloud.google.com/vertex-ai/generative-ai/docs/start/gemini-3-prompting-guide
- Phil Schmid, Gemini 3 prompting best practices: https://www.philschmid.de/gemini-3-prompt-practices
- Antigravity CLI headless docs: https://antigravity.google/docs/cli/headless/
- Flash vs Pro: https://benchlm.ai/compare/gemini-3-1-pro-vs-gemini-3-8-flash , https://myclaw.ai/blog/gemini-3-8-flash-vs-3-1-pro
