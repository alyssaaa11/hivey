# hivey — status (2026-09-30)

Design: [`hivey-design.md`](hivey-design.md) · Fork rules: [`../HIVEY.md`](../HIVEY.md)
Repo: `github.com/jcsancho/hiver` (private) · Local: `~/projects/swarmAgents/hivey` · Binary: `~/.local/bin/hivey`

## Done (all on `main`, 3,734 Rust tests + addon tests passing)

| Commit | What |
|---|---|
| `9265f795` | **Fork.** `hivey` binary, own config/socket (`~/.config/hivey`), runs side by side with herdr, no self-update. |
| `2c16ccac` | **Message bus + swarm engine.** Delivery when idle, `--fyi` never wakes, `--urgent` now (never into a dialog), per-agent queue rebuilt after restart, one digest per wake-up, blocked/gone targets escalated to the master, `@all` / `@role:` / `@masters` / `<swarm>/<agent>` / `human`, full log in `.swarm/bus.jsonl`. |
| `64212901` | **Live-test fixes.** Stops agent↔agent ping-pong (footer asks for replies only when needed; a pair may wake each other 6×/5 min, further messages become FYI). The master is told once per blocked episode. |
| `a3bcc86b` | **Swarm tree** in the Agents panel. Master first; a collapsed swarm shows its master plus agents that need attention; counters `●working/total ⚠attention ✉queued`. Clicking a header focuses that swarm's master and shows its panes. Client-only, no protocol change. |
| `bf6f4204` | **Role-colored pane titles** (`◆ coordinator · app-ideas · opus` in gold, workers blue, critic purple). **Keys:** `prefix+m` jump to master, `prefix+shift+m` pick a swarm, `prefix+a` send a message, `prefix+i` message log (`hivey swarm install-keys`). |
| `2e683c95` | **`hivey swarm launch`.** Same arguments and manifest as `launch_swarm.py`; the calling pane (master) moves into a new space named after the swarm as pane 1, workers are tiled beside it, the swarm is registered. Pane identity survives the move. |
| `e5a4d5d2` | **One hivey per project:** swarm registry per named session (`hivey --session <name>`). |
| `8f353757` | **Addons.** `launch --addon <plugin>` (alias `--relay`) opens plugin panes in the swarm's space before the agents; **`hivey.slack-relay`** (Slack ↔ bus, masters-only mirror, no echoes). Contract in `plugins/README.md`. |
| `eafbcb43` | **`hivey.dashboard`** addon (states, token bars + sparkline, budget, tasks, messages) and `hivey swarm addon <swarm> <plugin>` for running swarms. |
| `4faef301` | **`hivey.github`** addon: product and swarm workspace as two repos (`<slug>`, `<slug>-swarm`), confirm-first, gitleaks gate, "push both". |
| `384ad950` | **Addon map + runtime-driver proposal** (`docs/hivey-addons.md`): herdr as one driver among tmux/zellij/… |
| `c6572ed0` | **`hivey swarm relaunch`**: restarts agents that aren't running in their own pane, continuing their Claude conversation; reopens dead addons. |
| `4038879b` | **Pause/resume**: `hivey swarm pause|resume` (and the skill's `swarm_ctl.py`) hold/release all message delivery; `⏸` in titles and the tree. |
| `3eb2796f` | **Resume keeps launch flags**: after a restart herdr resumed agents with only `claude --resume <id>` (no permission mode, model or add-dirs); the engine now reports the full resume command. Relaunch fixes (`--addons-only`, no double start, addon anchor, failed addons retried). |
| skill | **`/swarm` switch-over** (only when `HIVEY_ENV=1`; backup in `~/.claude/skill-backups/`): `launch_swarm.py` → `hivey swarm launch` with the relay, dashboard and the skill's own `swarm.watcher` addon; agent briefs get a hivey messaging variant; SKILL.md "Running inside hivey" (incl. two GitHub repos). |

Verified live with real Claude (Haiku) agents via `scripts/live_smoke.sh`, and a hivey client read through a herdr pane:
- delivery timing, digests, blocked hold + escalation
- ping-pong fix
- tree rendering + clicks
- all four keys
- launch into its own space

## Known limitations
- After a restart, agents that never had a conversation come back as shells: run `hivey swarm relaunch <swarm>`.
- Swarms imported from herdr (not launched by hivey) have no recorded per-agent args, so their resume flags are only fixed after one `hivey swarm relaunch`.
- Scripts (the Slack relay) aren't agents, so they don't appear in the tree; their pane title still shows `▷`.
- The tree only renders in the single-machine sidebar (the multi-SSH-machine sidebar still shows the flat list).
- The onboarding dialog and some help texts still say "herdr".
- `~/SKILLS/swarm` (backup copy) isn't synced yet: run `/save-skill swarm` once the other session editing the skill is done.

## Next steps
1. From `docs/hivey-addons.md`: event hooks → agent adapters (mixed Claude/Codex swarms) → `Runtime` trait + standalone daemon → tmux driver.
2. Weekly upstream rebase job (`scripts/hivey_hooks.py` + nextest).

## Try it
```bash
hivey                                   # start (separate from herdr)
hivey swarm install-keys                # prefix+m / prefix+shift+m / prefix+a / prefix+i
hivey swarm import ~/swarms/<name>      # adopt an existing /swarm folder
hivey msg send <swarm>/coordinator "…"  # talk to a master
hivey plugin link ~/projects/swarmAgents/hivey/plugins/{slack-relay,dashboard,github}
hivey swarm addon <swarm> hivey.dashboard   # live dashboard for a running swarm
```
In a hivey pane, `/swarm` launches straight into its own space with the relay, dashboard and watcher.
