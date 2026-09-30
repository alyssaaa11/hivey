# hiver swarm addons

An **addon** is an ordinary hiver (herdr) plugin that `hiver swarm launch` opens in a swarm's
own space before the agents start. Relays are the main use: bridging the swarm's message bus
to Slack, Discord, Telegram, email, or anything else. Anyone can write one.

```bash
hiver plugin link ./plugins/slack-relay                # or: hiver plugin install owner/repo/path
hiver swarm launch <root> --slug app --channel C0123 scout critic \
    --addon hiver.slack-relay                          # alias: --relay; repeatable
hiver swarm launch … --addon me.discord:bridge         # a pane entrypoint other than "relay"
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
2. **Environment.** hiver sets:
   | Variable | Value |
   |---|---|
   | `HIVER_SWARM_ROOT` | the swarm folder (`<root>/.swarm/agents.json` is the roster) |
   | `HIVER_SWARM_SLUG` | the swarm's slug |
   | `HIVER_SWARM_CHANNEL` | `--channel` (or the manifest's `channel_id`), may be empty |
   | `HERDR_BIN_PATH` | the hiver binary to call |
   | `HERDR_PLUGIN_CONFIG_DIR` / `HERDR_PLUGIN_STATE_DIR` | your config (tokens, options) / state |
3. **Inbound (outside → agents).** Send through the bus, so delivery timing, queueing, digests
   and the log all apply:
   ```bash
   "$HERDR_BIN_PATH" msg send <agent|@all|master> "text" --swarm "$HIVER_SWARM_SLUG" --from human --json
   ```
   Use `--from <slug>/<agent>` when the outside message was written on an agent's behalf.
4. **Outbound (agents → outside).** Tail `<root>/.swarm/bus.jsonl`. Every line is JSON; messages
   have `"ev": "msg"` with `id`, `ts` (ms), `from` (`<slug>/<agent>`, `human`, `hiver`), `swarm`,
   `to`, `kind` (`normal`/`fyi`/`urgent`), `text`, and `copy: true` on a sender-side copy of a
   cross-swarm message (skip those). Receipts (`delivered`, `read`, `held`) can be ignored.
   Remember the ids you injected, so you don't echo them back.
5. **Robustness.** Keep your own state (e.g. under `<root>/.swarm/`) so a restart resumes where
   it stopped, and never exit on a network error.

hiver records opened addons in the manifest (`"addons"`) and shows them as `▷` script panes;
they never receive messages themselves.

## Included

- [`slack-relay`](slack-relay/) (`hiver.slack-relay`): Slack ↔ bus. Configure the token in
  `$(hiver plugin config-dir hiver.slack-relay)/config.json`:
  `{"token_command": "envsave get <id>", "mirror": "masters"}` (or set `SLACK_TOKEN`).
  `mirror` is `masters` (default), `all`, or `off`. Tests: `python3 -m unittest plugins/slack-relay/test_relay.py`.
