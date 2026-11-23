---
name: hivey-agent-detection
description: How to change agent screen detection in hivey/herdr (src/detect/manifests/*.toml, distribution/agent-detection/) - capture evidence, hot-reload a local override, test the engine not the CLI, live smoke test. Use whenever you add or fix detection of an agent's idle/working/blocked state or edit a detection manifest.
---

<!-- hivey: generated from assets/claude-code by scripts/claude-code-setup.sh; hivey update rewrites it. Put local changes in EXTRA_AGENTS.md. -->

# Agent detection updates

Source: AGENTS.md "Principles" (screen detection) and "Agent Detection Updates".

## Evidence first
- Capture the bottom-buffer state: `herdr agent read <pane> --source detection --format text`
  (add `--format ansi` when styling or alternate screen matters).
- Decide which visible controls are invariant and which are alternatives. Encode them as explicit
  AND/OR gates. Don't match incidental whole-pane text, and never use the user-visible viewport
  (users scroll it).

## Hot-reload loop
1. Drive the real agent UI into the target state in a disposable named session, using the
   `herdr-throwaway-repro` skill at `.agents/skills/herdr-throwaway-repro/SKILL.md`.
2. Inspect matching: `herdr agent explain <pane> --json`.
3. Edit the bundled manifest `src/detect/manifests/<agent>.toml`.
4. Before writing `~/.config/herdr/agent-detection/<agent>.toml`, check whether an override already
   exists. Never overwrite or remove a pre-existing one without asking. Copy the manifest there.
5. `herdr server reload-agent-manifests` against the session under test. Repeat until correct.
6. Remove the temporary override, or restore the previous one exactly. The bundled manifest stays
   the source of truth.

## Tests
- Unit-test the engine with synthetic manifests and minimal strings: parsing, regions, AND/OR/NOT
  gates, rule priority, skip-state, source precedence, cache reload, update flow.
- Keep bundled-manifest schema validation, process identification, and hook/protocol tests.
- Don't add tests that classify captured or invented CLI screens against bundled rules, or that freeze
  rule IDs/priorities.

## Live smoke test
Exercise the changed state and nearby transitions (idle, working, blocked, background work where
supported), including relevant OSC settings. Record the CLI version, signals seen and outcomes.

## Published catalog
`distribution/agent-detection/` is what released clients fetch. Keep already released agents aligned
with their bundled manifests, unless the validator records an exact compatibility exception. A newly
bundled agent may stay unpublished behind an exact version-and-digest exception until the first stable
release that ships it (`just release-docs-check` enforces this).
