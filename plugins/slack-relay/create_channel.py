#!/usr/bin/env python3
"""Create (or find) a public Slack channel for a hiver swarm or agent; prints its id as JSON.

usage: create_channel.py <name> [--purpose TEXT]

Uses the relay's token (config.json token_command, or $SLACK_TOKEN; `hiver slack connect` sets
it); needs channels:manage, plus channels:read and channels:join to find and join an existing
channel. Run by `hiver slack add`, `hiver swarm launch --slack` and `hiver home setup --slack`.
Output: {"ok": true, "channel_id": "C…", "name": "hiver", "existing": false}
"""
import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import relay  # noqa: E402  (same folder)


def find(slack, name):
    cursor = ""
    while True:
        data = slack.call("conversations.list", http="GET", types="public_channel",
                          exclude_archived="true", limit=1000, cursor=cursor)
        for channel in data.get("channels", []):
            if channel.get("name") == name:
                return channel
        cursor = data.get("response_metadata", {}).get("next_cursor", "")
        if not cursor:
            return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("name")
    parser.add_argument("--purpose")
    args = parser.parse_args()
    if not os.environ.get("HERDR_PLUGIN_CONFIG_DIR"):
        # Run outside a plugin pane: use the installed relay's config.
        os.environ["HERDR_PLUGIN_CONFIG_DIR"] = str(
            Path.home() / ".config" / "hiver" / "plugins" / "config" / "hiver.slack-relay")
    token = relay.resolve_token(relay.load_config())
    if not token:
        print(json.dumps({"ok": False, "error": "Slack is not connected: run hiver slack connect "
                          "in a terminal"}))
        return 1
    slack = relay.Slack(token)
    existing = False
    try:
        channel = slack.call("conversations.create", name=args.name, is_private=False)["channel"]
    except RuntimeError as err:
        if "name_taken" not in str(err):
            print(json.dumps({"ok": False, "error": str(err)}))
            return 1
        channel, existing = find(slack, args.name), True
        if channel is None:
            print(json.dumps({"ok": False, "error": "name_taken (archived or private channel)"}))
            return 1
        if not channel.get("is_member"):
            # The relay reads the channel's history: the bot has to be in it.
            try:
                slack.call("conversations.join", channel=channel["id"])
            except RuntimeError as err:
                print(json.dumps({"ok": False, "error": f"cannot join #{args.name}: {err}"}))
                return 1
    if args.purpose and not existing:
        try:
            slack.call("conversations.setPurpose", channel=channel["id"], purpose=args.purpose[:250])
        except RuntimeError:
            pass
    print(json.dumps({"ok": True, "channel_id": channel["id"], "name": args.name,
                      "existing": existing}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
