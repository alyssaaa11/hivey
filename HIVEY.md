# hivey

Terminal workspace for swarms of AI coding agents. A private fork of
[herdr](https://github.com/herdrdev/herdr) (Apache-2.0).

- Design: `docs/hivey-design.md` · Status and next steps: `docs/hivey-status.md`
- Remotes: `origin` = `alyssaaa11/hivey` (private), `upstream` = `herdrdev/herdr` (fetch only, push disabled)
- `AGENTS.md` is herdr's upstream guide. Leave it unmodified so rebases stay clean; this file replaces it for hivey.

## hivey fork rules

### Where code goes
- All swarm code lives in `src/swarm/` (engine, bus, roster) and `src/cli/swarm.rs` (CLI).
  Process setup lives in `src/hivey.rs`.
- Changes to herdr's own files are **hooks**: as small as possible, delegating to `src/swarm/`,
  and marked with a `// hivey:` comment so they're easy to find and re-port.
- Every hook must also exist in `scripts/hivey_hooks.py` (idempotent). A core-file edit that
  isn't in the script will be lost on the next rebase.
- Prefer one new API method with an `op` field (`Method::Swarm`) over many new variants.
  Reach app state through the internal API (`crate::api::dispatch_internal`), not by
  editing `AppState`.

### Identity and coexistence
- Binary `hivey`; config, sockets and sessions in `~/.config/hivey` (`hivey-dev` for debug builds).
- Panes keep herdr's `HERDR_*` env names (so existing tools keep working) and add `HIVEY_ENV=1`.
  Never rename the `HERDR_*` variables.
- hivey never self-updates or checks herdr.dev for releases. Update from source only.

### Upstream sync
1. `git fetch upstream && git rebase upstream/master`
2. On conflicts in core files, prefer upstream's version, then **rerun `python3 scripts/hivey_hooks.py`**
   to reapply the hooks. If an anchor no longer matches, update the script, not just the file.
3. Regenerate the API schema (it includes hivey's `swarm` method):
   `HERDR_UPDATE_API_SCHEMA=1 cargo nextest run generated_protocol_schema_artifact_is_current`
4. `cargo build --release` and `just test` (nextest) must be green before pushing to `origin`.

### Build requirements
- Zig **0.16.0** (`brew install zig`) for the vendored libghostty-vt.
- `cargo-nextest` (`brew install cargo-nextest`). Plain `cargo test` dies with SIGPIPE in this
  suite; always use `just test` or `cargo nextest run`.

### Commits
- Lowercase conventional commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`), no emojis.
  Descriptive subject; a body when the why isn't obvious.
- **No AI co-author lines** (no `Co-Authored-By: Claude …`).
- Commit hivey work on `main` in `alyssaaa11/hivey`. Never push, open issues, or open PRs
  against `herdrdev/herdr`.

## Engineering rules (from herdr)

### Principles
- **State is separated from runtime.** `AppState` is pure data, testable without PTYs or async. `PaneState` is separate from `PaneRuntime`. Workspace logic doesn't need real terminals.
- **Render is pure.** `compute_view()` handles geometry and mutations. `render()` takes `&AppState` and only draws. Never mutate state during render.
- **No god objects.** If a module is doing too many things, split it.
- **Platform code is isolated.** OS-specific behavior lives in `src/platform/<os>.rs`; only shared traits, types, wrappers and testable contracts go in `src/platform/mod.rs`. Core modules don't have `#[cfg(target_os)]`.
- **Detection is decoupled.** The detector reads a screen snapshot, never touches the parser or viewport state.
- **Screen detection is evidence-based.** When changing `src/detect/manifests/`, capture the bottom-buffer state with `hivey agent read <pane> --source detection --format text` (and `--format ansi` when styling matters). Encode invariant controls as explicit AND/OR gates. Don't match whole-pane incidental text or the user-visible viewport.
- **UI patterns should be reused.** Mouse-first TUI: new dialogs, popups and panels (swarm tree, send-message popup, whiteboard) follow the existing modal/screen structure and affordances.

### Multiplicative performance paths
Work reachable from view computation, rendering, background-pane resizing, PTY parsing, detection and client frame fanout is multiplicative. Before adding work there, find its frequency and cardinality: per byte, event or render × panes, tabs or workspaces × attached clients.

Inside pane-scaled render and layout loops:
- Use narrow terminal-state accessors. No aggregate input-state collection, snapshot formatting, process-tree inspection, filesystem I/O, or allocation when one scalar fact is enough.
- Keep terminal-core lock duration minimal.
- Preserve hidden-source and retained-render early exits.
- When a change widens work in these loops, profile with 1 and ≥15 populated panes (`just bench-render-scale`) and report the delta.

This applies to the swarm engine too. It runs off the render path on its own thread, with one `agent.list` per tick, and stays idle when no swarm is registered. Keep it that way: no per-pane work in render for swarm features. The sidebar tree must render from data the client already has.

Prefer deterministic behavioral tests to wall-clock limits.

### Runtime/client boundary
New work must not deepen server/TUI coupling. Before adding state, API fields, events, commands or socket messages, classify the feature:
- **Shared runtime fact** (swarms, roles, messages, queues, agent state): server state, exposed through the JSON API/event path.
- **TUI presentation** (tree layout, glyphs, colors, selection, popups): client only.

Don't add shared behavior that only works through the private TUI client socket. Use neutral server/API names (`swarm`, `msg`), not UI names like sidebar, row or widget.

### Stable client endpoint contract
Endpoint generation 1 is the compatibility floor and must stay available.
- Named core codecs are immutable: don't add, remove, reorder or reinterpret fields or enum variants in a published codec. Add a new codec name and keep the old one as a fallback.
- New JSON fields must be optional or have field-specific defaults; new enum values need an `Unknown` fallback.
- Add features through advertised API methods and optional snapshot data. A missing optional feature disables only that action.
- Don't change the meaning or parameter shape of an advertised method; add a new method or capability instead.
- Frozen endpoint fixtures, bincode digests, wire-tag tests and `tests/fixtures/endpoint-method-shapes-v1.json` are contracts. Never update a generation-1 expectation to bless a wire change.
- Treat every enum reachable from a frozen codec as append-closed.

This matters most for the swarm tree sidebar: swarm data must reach the client as an optional addition, not a changed codec.

## Testing
```bash
just test     # cargo nextest + maintenance script tests (default)
just check    # formatting + tests + windows lint (the windows stage needs `just setup-windows-cross`)
```
- Unit tests live next to the code (`#[cfg(test)] mod tests`). New `AppState`/`Workspace` behavior should be testable with `AppState::test_new()` / `Workspace::test_new()`.
- Swarm logic stays testable without a server: pure functions in `bus.rs`/`model.rs`, and engine state tests in `engine.rs` using temp swarm folders.
- Risky refactors (identity, persisted state, protocol/API IDs, restore/handoff, detection authority): name or add characterization tests first; use `AppState::assert_invariants_for_test()` / `Workspace::assert_invariants_for_test()` with the adversarial test states.
- Testing a dev build from inside a hivey or herdr session: debug builds use `~/.config/hivey-dev`. Clear inherited socket overrides so the debug binary talks to the debug server:
  ```bash
  env -u HERDR_SOCKET_PATH -u HERDR_CLIENT_SOCKET_PATH cargo run -- <command>
  ```

## Agent detection
Use the manifest hot-reload loop: bundled manifest in `src/detect/manifests/<agent>.toml`, a temporary override in `~/.config/hivey/agent-detection/<agent>.toml`, then `hivey server reload-agent-manifests`. Check for an existing override first; never overwrite or remove one without asking, and restore it when done. Unit-test the detection engine with synthetic manifests, not captured CLI screens. Validate agent behavior with live smoke tests.

## Vendored libghostty-vt
`vendor/libghostty-vt.vendor.json` records the vendored upstream commit. Local patches are listed in `vendor/libghostty-vt.patches.md` with patch files under `vendor/patches/libghostty-vt/` (why, base commit, touched files, verification, removal condition). `just check` verifies they're indexed and apply cleanly.

## Code conventions
- Rust: no `unwrap()` in production code. Use `tracing` for logging. `#[allow]` only with a comment explaining why.
- Platform-specific code is compile-gated (`#[cfg(windows)]`, `#[cfg(unix)]` on imports, fields, fns, impls, match arms). `cfg!(...)` only for pure policy constants that compile on every target.
- Don't add dependencies without a reason; check existing ones first.
- When changing the server/client wire protocol, bump `src/protocol/wire.rs::PROTOCOL_VERSION` if the current protocol has already shipped in a hivey build you use, and update the protocol fixtures in tests.
- Local planning notes go in `.local/prd/` (ignored).
