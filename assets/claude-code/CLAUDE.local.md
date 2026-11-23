<!-- hivey: generated from assets/claude-code by scripts/claude-code-setup.sh; hivey update rewrites it. Put local changes in EXTRA_AGENTS.md. -->

# hivey (built on herdr)

Digest of `AGENTS.md` (herdr's upstream instructions, excluded from context through
`claudeMdExcludes` in `.claude/settings.local.json`). Herdr's maintainer workflow, release channels,
Windows VM, external-contributor rules and Can's machine setup don't apply here. For any of them,
read that section of `AGENTS.md` on demand.

This machine's own additions (git-ignored, never touched by updates): @EXTRA_AGENTS.md

hivey only adds on top of herdr: never rename, remove or change herdr's own pieces. Put new work in
hivey's files (`src/swarm/`, `src/cli/swarm/`, `src/hivey.rs`, `skills/`, `plugins/`, `hooks/`) or as
small `// hivey:` additions in shared files.

## Principles
- `AppState` is pure data, testable without PTYs or async. `PaneState` is separate from `PaneRuntime`.
- Render is pure: `compute_view()` does geometry and mutations, `render(&AppState)` only draws.
- No god objects. `app/` stays split into state, actions and input.
- OS code lives in `src/platform/<os>.rs`. Core modules have no `#[cfg(target_os)]`.
- The detector reads a screen snapshot, never the parser or viewport state.
- Mouse-first TUI: reuse existing modal/screen patterns and close actions, no one-off screens.

## Hot paths (view computation, render, resize, PTY parsing, detection, client fanout)
- Work there multiplies per byte/event/render × panes/tabs/workspaces × clients. Know the cardinality.
- In pane-scaled loops: narrow accessors only, no aggregate state, snapshots, process trees, file I/O
  or needless allocation. Keep terminal-core locks short. Keep hidden-pane and retained-render early exits.
- If you widen such a loop, profile 1 vs 15+ panes (`just bench-render-scale`) and report the delta.

## Runtime/client boundary
- Shared runtime facts (pane/agent metadata, process/terminal state, events) go in server state and
  the JSON API. TUI presentation (layout, colors, selection, modals, mouse) stays client-side.
- No new shared behavior only reachable through the private TUI socket. Use neutral API names
  (not sidebar/row/card/widget).

## Stable client endpoint contract (generation 1)
- Named codecs are immutable: never add, remove, reorder or reinterpret fields or enum variants.
  Add a new codec name instead. Treat every enum reachable from a frozen codec as closed.
- New JSON fields are optional or defaulted. New enum values need an `Unknown` fallback.
- Add features as advertised methods/optional data. A missing feature disables only that action.
- Never change an advertised method's meaning or parameter shape. Use a new method or capability.
- Never update frozen fixtures, bincode digests, wire-tag tests or
  `tests/fixtures/endpoint-method-shapes-v1.json` to bless a wire change.
- Wire protocol changes: see `PROTOCOL_VERSION` in `src/protocol/wire.rs` and "Code Conventions" in AGENTS.md.

## Testing
- Use `just`: `just test`; `just check` (fmt + nextest + maintenance tests) before committing.
- Unit tests live next to the code. New state behavior must be testable with `AppState::test_new()`
  and `Workspace::test_new()`.
- Broad refactors (identity, persisted state, protocol IDs, restore/handoff, detection authority):
  read AGENTS.md "Testing" first for the invariant helpers.
- Test a new build from inside herdr/hivey:
  `env -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH cargo run -- <command>`
- Windows cross-compile setup: AGENTS.md "Testing".

## Code conventions
- No `unwrap()` in production code. Log with `tracing`. `#[allow]` only with a comment saying why.
- Compile-gate platform code (`#[cfg(windows)]`, `#[cfg(unix)]`). `cfg!()` only for pure constants.
- No new dependencies without a reason.
- Integration asset versions bump once per release, relative to the latest released tag.

## Commits
- Lowercase conventional commits, no emojis, no AI co-author lines. Propose the message first.
- Related issue: body line `refs #<n>`, never `fixes`/`closes`/`resolves`.

## On-demand guidance
- Agent detection manifests (`src/detect/manifests/`): skill `hivey-agent-detection`.
- `vendor/` and docs trees: path-scoped rules in `.claude/rules/hivey-*.md` load when you touch those files.
