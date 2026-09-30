# hiver — status (2026-09-30)

Design: [`hiver-design.md`](hiver-design.md). Repo: `github.com/jcsancho/hiver` (private). Local: `~/projects/swarmAgents/hiver`.
Remotes: `origin` = hiver, `upstream` = herdrdev/herdr (push disabled).

## Done

### P0 — fork (committed and pushed: `9265f795`)
- Binary `hiver`, own config/socket/session dirs (`~/.config/hiver`), so it runs **side by side with herdr**.
- Panes get `HIVER_ENV=1` and keep `HERDR_*` vars, so `herdr agent prompt`, the swarm scripts and Claude hooks still work inside hiver panes.
- hiver started from a herdr pane drops the inherited `HERDR_*` vars (verified both ways).
- Self-update and herdr.dev release checks disabled.
- NOTICE, README header, design doc.
- **All 3,699 upstream tests pass** (`cargo nextest run`).
- Build needs Zig 0.16.0 (`brew install zig`) and cargo-nextest (`brew install cargo-nextest`); both installed.

### Message bus + swarm engine (written, builds, **NOT committed**)
Covers the four gaps in herdr's `agent prompt`:

| Gap | What's implemented |
|---|---|
| Timing | Normal messages are held until the target has been idle for 3 s. `--urgent` delivers right away but never types into a blocked dialog. `--fyi` never wakes anyone. |
| Queue / inbox | Per-agent queues are rebuilt from `.swarm/bus.jsonl` after a restart. Blocked or gone targets: messages are held and the master is told once. Queued messages go out as **one digest prompt**. `hiver msg inbox` pulls messages and cancels their pending delivery. |
| Addressing | `scout`, `app-ideas-scout`, `master`, `@all`, `@role:worker\|critic\|master`, `@masters`, `<swarm>/<agent>`, `human`. Only masters and the human can message another swarm. Scripts are excluded. Rate limit: 30 messages/min per sender. |
| Record | Every message plus delivered/read/held receipts goes to `<root>/.swarm/bus.jsonl`. Cross-swarm messages are logged in both swarms. `hiver msg log`. |

Also:
- Pane titles/borders: `◆ coordinator · app-ideas · opus · ✉2`, plus pane tokens `role`, `swarm`, `queued` for sidebar rules.
- The engine sits idle when no swarm is registered.

Files:
- `src/swarm/{mod,model,bus,engine}.rs` — roster, bus logic, the server thread (an internal API client: `agent.list`, `agent.prompt`, `pane.report_metadata`).
- `src/cli/swarm.rs` — `hiver swarm import|list|master [--focus]|forget` and `hiver msg send|inbox|log`.
- Core hooks: one `Method::Swarm` variant, handling in `api/server.rs`, engine start, `api::dispatch_internal`, CLI dispatch. All of them are reapplied by `scripts/hiver_hooks.py` (idempotent, for upstream rebases).

Tests: 18 of 19 new swarm tests pass. Failing: `swarm::bus::tests::role_addressing_selects_by_role`. The test data is the likely cause, not the code: `serde_json::Map` sorts keys, so recipients come back as `analyst, scout`, not the expected `scout, analyst`.

## Missing

1. **Fix that test** (compare sorted), run the full suite, commit and push the engine.
2. **Live test with real Claude panes.** Import `~/swarms/apps_ideas`, then check:
   - multi-line digests paste and submit correctly through `agent.prompt`
   - the 3 s idle settle is long enough
   - escalation messages reach the master
3. **Tree sidebar** (P1 UI). Nothing written yet. It's client-side (`src/client/shell/sidebar.rs`, `endpoint_sidebar.rs`); follow CLAUDE.md's endpoint-contract rules and add *optional* fields/new codecs only.
   - Interim option without code: `[ui.sidebar.agents]` rules on `$role`/`$queued`, plus `agent.view.set` filtered to `role=master` or blocked/done.
4. **Role-colored pane borders** (`src/ui/panes.rs`; today only the focus accent is used).
5. **Keys:**
   - `prefix m`: jump to master (`hiver swarm master --focus` already works from the CLI)
   - `prefix 1..9`: jump to the master of swarm N
   - `prefix s`: send-message popup
   - `prefix w`: whiteboard
6. **`hiver swarm launch`** (P2): one space per swarm, master in pane 1, create agent folders, start the agents; replaces `launch_swarm.py`.
7. **Slack bridge plugin** (P3 remainder): mirror the bus to `#swarm-<slug>` (default scope: masters only) and turn Slack `@agent` messages into bus sends.
8. **Whiteboard** (P4): `hiver task …` compatible with `swarm_tasks.py`, notes, kanban panel.
9. **Supervisor** (P5): idle/stall/budget alerts and token usage built in; retire `swarm_relay.py`.
10. **`/swarm` skill** (P6): use `hiver` when `HIVER_ENV=1`, and tell agents to talk via `hiver msg send` instead of Slack.
11. Weekly upstream rebase job (CI) running `scripts/hiver_hooks.py` + nextest.

## Next steps (in order)
1. Fix the test → `cargo nextest run` → commit and push the engine.
2. Install the binary (`cp target/release/hiver ~/.local/bin/`), start `hiver` in a new terminal, run `hiver swarm import ~/swarms/apps_ideas`, and test `hiver msg send` against real agents.
3. Interim sidebar config (masters-only Agent view + role colors), then the native tree sidebar.
4. `hiver swarm launch` and the `/swarm` skill switch-over.
