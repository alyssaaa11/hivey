# hivey swarm addons

An **addon** is an ordinary hivey (herdr) plugin that `hivey swarm launch` opens in a swarm's
own space before the agents start. Relays are the main use: bridging the swarm's message bus
to Slack, Discord, Telegram, email, or anything else. Anyone can write one.

```bash
hivey plugin link ./plugins/slack-relay                # or: hivey plugin install owner/repo/path
hivey swarm launch <root> --slug app --channel C0123 scout critic \
    --addon hivey.slack-relay                          # alias: --relay; repeatable
hivey swarm launch … --addon me.discord:bridge         # a pane entrypoint other than "relay"
```

## Contract

1. **Manifest.** A `herdr-plugin.toml` with a `[[panes]]` entrypoint, `relay` by default:
   ```toml
   id = "me.discord-relay"
   name = "Discord relay"
   version = "0.1.0"
   min_herdr_version = "0.9.0"
   platforms = ["linux", "macos"]

   [[panes]]
   id = "relay"
   title = "discord relay"
   placement = "split"
   command = ["sh", "-c", "exec python3 \"$HERDR_PLUGIN_ROOT/relay.py\""]
   ```
   The pane runs with the **swarm root as its working directory**, so reach your own files
   through `$HERDR_PLUGIN_ROOT`.
2. **Environment.** hivey sets:
   | Variable | Value |
   |---|---|
   | `HIVEY_SWARM_ROOT` | the swarm folder (`<root>/.swarm/agents.json` is the roster) |
   | `HIVEY_SWARM_SLUG` | the swarm's slug |
   | `HIVEY_SWARM_CHANNEL` | `--channel` (or the manifest's `channel_id`), may be empty |
   | `HERDR_BIN_PATH` | the hivey binary to call |
   | `HERDR_PLUGIN_CONFIG_DIR` / `HERDR_PLUGIN_STATE_DIR` | your config (tokens, options) / state |
3. **Inbound (outside → agents).** Send through the bus, so delivery timing, queueing, digests
   and the log all apply:
   ```bash
   "$HERDR_BIN_PATH" msg send <agent|@all|master> "text" --swarm "$HIVEY_SWARM_SLUG" --from human --json
   ```
   Use `--from <slug>/<agent>` when the outside message was written on an agent's behalf.
4. **Outbound (agents → outside).** Tail `<root>/.swarm/bus.jsonl`. Every line is JSON; messages
   have `"ev": "msg"` with `id`, `ts` (ms), `from` (`<slug>/<agent>`, `human`, `hivey`), `swarm`,
   `to`, `kind` (`normal`/`fyi`/`urgent`), `text`, and `copy: true` on a sender-side copy of a
   cross-swarm message (skip those). Receipts (`delivered`, `read`, `held`) can be ignored.
   Remember the ids you injected, so you don't echo them back.
5. **Robustness.** Keep your own state (e.g. under `<root>/.swarm/`) so a restart resumes where
   it stopped, and never exit on a network error.

hivey records opened addons in the manifest (`"addons"`) and shows them as `▷` script panes;
they never receive messages themselves.

## Setup providers

A **setup provider** designs a swarm (team, roles, briefs, memory, channels) and hands it to
hivey. hivey doesn't hardcode one: any plugin with a pane entrypoint `setup` is a provider.

```bash
hivey swarm providers                          # installed providers (* = default)
hivey swarm new "<task>"                       # the default (or only) provider
hivey swarm new --provider my.crew --default "<task>"
```

`hivey swarm new` opens the provider's `setup` pane as a new tab in the current folder (creating
a space for it if none is open), with `HIVEY_SETUP_TASK` and `HIVEY_SETUP_CWD` set. The provider
then:

1. writes each agent's brief into `<root>/<agent>/CLAUDE.md` (Codex agents read `AGENTS.md`;
   hivey links it to `CLAUDE.md` when only that exists), and
2. runs `hivey swarm launch <root> --slug … <agents…>` with whatever it chose: `--models`,
   `--kinds a=codex`, `--channel`, `--addon …`, `--heartbeat 15m --heartbeat-task "…"`,
   `--kickoff "…"`. Launch moves the provider's pane into the swarm's space as the master.

The swarm manifest `<root>/.swarm/agents.json` is hivey's; a provider may add:

| Field | Shown / used by hivey |
|---|---|
| `info` | `{"Slack": "#swarm-x (C0…)", "vault": "…"}` or `[[label, value], …]`: shown in the hover card and `hivey swarm info` |
| `product_dir` | the product folder `hivey.github` publishes (default `app`) |
| `state` | `"paused"` holds all deliveries (what `hivey swarm pause` sets) |
| `.swarm/tasks.json` | `{"tasks": [{"status": "open\|in-progress\|review\|approved\|blocked"}]}`: task counts in info and the dashboard |

Step-by-step guide and an example to copy: [`../docs/hivey-providers.md`](../docs/hivey-providers.md)
(`hivey.team-template`, a fixed builder + critic team).

Included: **`swarm.creator`** (`~/.claude/skills/swarm/hivey-setup`) opens a Claude coordinator
running the `/swarm` skill. It writes its own kickoff and monitoring texts, and `info` with the
swarm's Slack channel and Obsidian vault.

## Included

- [`slack-relay`](slack-relay/) (`hivey.slack-relay`): Slack ↔ bus. Configure the token in
  `$(hivey plugin config-dir hivey.slack-relay)/config.json`:
  `{"token_command": "envsave get <id>", "mirror": "masters"}` (or set `SLACK_TOKEN`).
  `mirror` is `masters` (default), `all`, or `off`. Tests: `python3 -m unittest plugins/slack-relay/test_relay.py`.
- [`dashboard`](dashboard/) (`hivey.dashboard`): live pane with agent states, token bars and a
  token-rate sparkline, budget, task counts and recent messages. Tests:
  `python3 -m unittest plugins/dashboard/test_dashboard.py`.
- [`github`](github/) (`hivey.github`): publishes the product (e.g. `app/`) and the swarm
  workspace as two repos, `<slug>` and `<slug>-swarm`. It shows the plan and creates nothing
  until you confirm; gitleaks scans what is committed before each first push. Then "push both"
  on demand. Try it safely with `HIVEY_GITHUB_DRY_RUN=1`. Tests:
  `python3 -m unittest plugins/github/test_github.py`.
