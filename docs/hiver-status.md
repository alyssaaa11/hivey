# hiver — status (2026-09-30)

Design: [`hiver-design.md`](hiver-design.md) · Fork rules: [`../HIVER.md`](../HIVER.md)
Repo: `github.com/jcsancho/hiver` (private) · Local: `~/projects/swarmAgents/hiver` · Binary: `~/.local/bin/hiver`

## Done (all on `main`, 3,729 tests passing)

| Commit | What |
|---|---|
| `9265f795` | **Fork.** `hiver` binary, own config/socket (`~/.config/hiver`), runs side by side with herdr, no self-update. |
| `2c16ccac` | **Message bus + swarm engine.** Delivery when idle, `--fyi` never wakes, `--urgent` now (never into a dialog), per-agent queue rebuilt after restart, one digest per wake-up, blocked/gone targets escalated to the master, `@all` / `@role:` / `@masters` / `<swarm>/<agent>` / `human`, full log in `.swarm/bus.jsonl`. |
| `64212901` | **Live-test fixes.** Stops agent↔agent ping-pong (footer asks for replies only when needed; a pair may wake each other 6×/5 min, further messages become FYI). The master is told once per blocked episode. |
| `a3bcc86b` | **Swarm tree** in the Agents panel. Master first; a collapsed swarm shows its master plus agents that need attention; counters `●working/total ⚠attention ✉queued`. Clicking a header focuses that swarm's master and shows its panes. Client-only, no protocol change. |
| `bf6f4204` | **Role-colored pane titles** (`◆ coordinator · app-ideas · opus` in gold, workers blue, critic purple). **Keys:** `prefix+m` jump to master, `prefix+shift+m` pick a swarm, `prefix+a` send a message, `prefix+i` message log (`hiver swarm install-keys`). |
| `2e683c95` | **`hiver swarm launch`.** Same arguments and manifest as `launch_swarm.py`; the calling pane (master) moves into a new space named after the swarm as pane 1, workers are tiled beside it, the swarm is registered. Pane identity survives the move. |

Verified live with real Claude (Haiku) agents via `scripts/live_smoke.sh`, and a hiver client read through a herdr pane:
- delivery timing, digests, blocked hold + escalation
- ping-pong fix
- tree rendering + clicks
- all four keys
- launch into its own space

## Known limitations
- **Restart:** after a hiver server restart, agents that never received a prompt come back as plain shells (herdr only resumes Claude sessions that exist). Needs `hiver swarm relaunch`.
- Scripts (the Slack relay) aren't agents, so they don't appear in the tree; their pane title still shows `▷`.
- The tree only renders in the single-machine sidebar (the multi-SSH-machine sidebar still shows the flat list).
- The onboarding dialog and some help texts still say "herdr".
- `launch` doesn't start the Slack relay or dashboard; the skill still does.

## Next steps
1. **`/swarm` skill switch-over (needs your OK: it edits `~/.claude/skills/swarm`).** When `HIVER_ENV=1`:
   - `launch_swarm.py` calls `hiver swarm launch`
   - agent CLAUDE.md templates say `hiver msg send` instead of Slack for agent↔agent
   - the relay only bridges Slack
2. **`hiver swarm relaunch [<agent>]`:** restart gone agents in their panes with their briefs (fixes the restart limitation).
3. **Slack bridge** as the only Slack path: mirror the bus to `#swarm-<slug>` (masters-only by default), Slack `@agent` → bus.
4. **Whiteboard** (`hiver task …` compatible with `swarm_tasks.py`, kanban panel), then the built-in supervisor (idle/stall/budget) to retire `swarm_relay.py`.
5. Weekly upstream rebase job (`scripts/hiver_hooks.py` + nextest).

## Try it
```bash
hiver                                   # start (separate from herdr)
hiver swarm install-keys                # prefix+m / prefix+shift+m / prefix+a / prefix+i
hiver swarm import ~/swarms/<name>      # adopt an existing /swarm folder
hiver msg send <swarm>/coordinator "…"  # talk to a master
```
