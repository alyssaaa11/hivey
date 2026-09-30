# hiver — status (2026-09-30)

Design: [`hiver-design.md`](hiver-design.md) · Fork rules: [`../HIVER.md`](../HIVER.md)
Repo: `github.com/jcsancho/hiver` (private) · Local: `~/projects/swarmAgents/hiver` · Binary: `~/.local/bin/hiver`

## Done (all on `main`, 3,730 Rust tests + addon tests passing)

| Commit | What |
|---|---|
| `9265f795` | **Fork.** `hiver` binary, own config/socket (`~/.config/hiver`), runs side by side with herdr, no self-update. |
| `2c16ccac` | **Message bus + swarm engine.** Delivery when idle, `--fyi` never wakes, `--urgent` now (never into a dialog), per-agent queue rebuilt after restart, one digest per wake-up, blocked/gone targets escalated to the master, `@all` / `@role:` / `@masters` / `<swarm>/<agent>` / `human`, full log in `.swarm/bus.jsonl`. |
| `64212901` | **Live-test fixes.** Stops agent↔agent ping-pong (footer asks for replies only when needed; a pair may wake each other 6×/5 min, further messages become FYI). The master is told once per blocked episode. |
| `a3bcc86b` | **Swarm tree** in the Agents panel. Master first; a collapsed swarm shows its master plus agents that need attention; counters `●working/total ⚠attention ✉queued`. Clicking a header focuses that swarm's master and shows its panes. Client-only, no protocol change. |
| `bf6f4204` | **Role-colored pane titles** (`◆ coordinator · app-ideas · opus` in gold, workers blue, critic purple). **Keys:** `prefix+m` jump to master, `prefix+shift+m` pick a swarm, `prefix+a` send a message, `prefix+i` message log (`hiver swarm install-keys`). |
| `2e683c95` | **`hiver swarm launch`.** Same arguments and manifest as `launch_swarm.py`; the calling pane (master) moves into a new space named after the swarm as pane 1, workers are tiled beside it, the swarm is registered. Pane identity survives the move. |
| `e5a4d5d2` | **One hiver per project:** swarm registry per named session (`hiver --session <name>`). |
| `8f353757` | **Addons.** `launch --addon <plugin>` (alias `--relay`) opens plugin panes in the swarm's space before the agents; **`hiver.slack-relay`** (Slack ↔ bus, masters-only mirror, no echoes). Contract in `plugins/README.md`. |
| `eafbcb43` | **`hiver.dashboard`** addon (states, token bars + sparkline, budget, tasks, messages) and `hiver swarm addon <swarm> <plugin>` for running swarms. |
| `4faef301` | **`hiver.github`** addon: product and swarm workspace as two repos (`<slug>`, `<slug>-swarm`), confirm-first, gitleaks gate, "push both". |
| `384ad950` | **Addon map + runtime-driver proposal** (`docs/hiver-addons.md`): herdr as one driver among tmux/zellij/… |
| skill | **`/swarm` switch-over** (only when `HIVER_ENV=1`; backup in `~/.claude/skill-backups/`): `launch_swarm.py` → `hiver swarm launch` with the relay, dashboard and the skill's own `swarm.watcher` addon; agent briefs get a hiver messaging variant; SKILL.md "Running inside hiver" (incl. two GitHub repos). |

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
- The skill's `swarm_ctl.py` (pause/resume) still wakes agents through Slack; in hiver it should use the bus.
- `~/SKILLS/swarm` (backup copy) isn't synced yet: run `/save-skill swarm` once the other session editing the skill is done.

## Next steps
1. `hiver swarm relaunch [<agent>]`: restart agents that came back as shells after a restart.
2. `swarm_ctl.py` pause/resume via the bus inside hiver.
3. From `docs/hiver-addons.md`: event hooks → agent adapters (mixed Claude/Codex swarms) → `Runtime` trait + standalone daemon → tmux driver.
4. Weekly upstream rebase job (`scripts/hiver_hooks.py` + nextest).

## Try it
```bash
hiver                                   # start (separate from herdr)
hiver swarm install-keys                # prefix+m / prefix+shift+m / prefix+a / prefix+i
hiver swarm import ~/swarms/<name>      # adopt an existing /swarm folder
hiver msg send <swarm>/coordinator "…"  # talk to a master
hiver plugin link ~/projects/swarmAgents/hiver/plugins/{slack-relay,dashboard,github}
hiver swarm addon <swarm> hiver.dashboard   # live dashboard for a running swarm
```
In a hiver pane, `/swarm` launches straight into its own space with the relay, dashboard and watcher.
