#!/usr/bin/env python3
"""Create (or find) a private Slack channel for a hivey swarm or agent; prints its id as JSON.

usage: create_channel.py <name> [--purpose TEXT]

Uses the relay's token (config.json token_command, or $SLACK_TOKEN; `hivey slack connect` sets
it); needs groups:write to create the channel private and invite people (only the people invited
see it, not everyone who joins the workspace), groups:read and channels:read to find an existing
one, and channels:join to join an existing public one. Run by `hivey slack add`, `hivey swarm launch --slack` and `hivey home setup --slack`.
The user is invited, so the channel shows up in their Slack right away: config.json "invite"
(Slack member ids; `hivey slack connect` sets it, else it's worked out once from the people in
the channels the bot already shares with them).
Output: {"ok": true, "channel_id": "C…", "name": "hivey", "existing": false, "private": true,
         "invited": ["U…"]}
An existing public channel of that name is reused as it is ("private": false, with a note to change
it to private in Slack: a bot cannot convert it).
"""
import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import relay  # noqa: E402  (same folder)

CHANNEL_TYPES = "public_channel,private_channel"  # private: only the ones the bot is in
MISSING_WRITE = ("Slack needs the groups:write scope to create private channels: add it to the hivey "
                 "app under OAuth & Permissions → Bot Token Scopes, click Reinstall to Workspace, then "
                 "run hivey slack connect --force")


def find(slack, name):
    cursor = ""
    while True:
        data = slack.call("conversations.list", http="GET", types=CHANNEL_TYPES,
                          exclude_archived="true", limit=1000, cursor=cursor)
        for channel in data.get("channels", []):
            if channel.get("name") == name:
                return channel
        cursor = data.get("response_metadata", {}).get("next_cursor", "")
        if not cursor:
            return None


def config_path():
    return Path(os.environ["HERDR_PLUGIN_CONFIG_DIR"]) / "config.json"


def shared_people(slack):
    """Humans in the channels the bot is in, most shared first (the bot itself excluded)."""
    bot = slack.call("auth.test").get("user_id")
    counts = {}
    data = slack.call("conversations.list", http="GET", types=CHANNEL_TYPES,
                      exclude_archived="true", limit=1000)
    for channel in data.get("channels", []):
        if not channel.get("is_member"):
            continue
        members = slack.call("conversations.members", http="GET", channel=channel["id"])
        for member in members.get("members", []):
            if member != bot:
                counts[member] = counts.get(member, 0) + 1
    return sorted(counts, key=lambda member: -counts[member])


def invitees(slack, config):
    """Who to invite: config "invite", else the person the bot shares the most channels with
    (then remembered in config.json)."""
    people = config.get("invite")
    if isinstance(people, str):
        people = [people]
    if people:
        return people
    try:
        found = shared_people(slack)[:1]
    except RuntimeError:
        return []
    if found:
        config["invite"] = found
        config_path().write_text(json.dumps(config, indent=2) + "\n")
    return found


def invite(slack, channel_id, people):
    """Invites the people not in the channel yet; returns the ones invited now."""
    invited = []
    for person in people:
        try:
            slack.call("conversations.invite", channel=channel_id, users=person)
            invited.append(person)
        except RuntimeError as err:
            if "already_in_channel" not in str(err):
                print(f"cannot invite {person}: {err}", file=sys.stderr)
    return invited


def create(slack, name):
    """(channel, existing) for #name: created private, else the existing one (joined when public).
    Raises RuntimeError with a message for the user."""
    try:
        return slack.call("conversations.create", name=name, is_private=True)["channel"], False
    except RuntimeError as err:
        if "missing_scope" in str(err):
            raise RuntimeError(MISSING_WRITE) from err
        if "name_taken" not in str(err):
            raise
    channel = find(slack, name)
    if channel is None:
        raise RuntimeError("name_taken (an archived channel, or a private one the bot is not in)")
    if not channel.get("is_member"):
        if channel.get("is_private"):
            raise RuntimeError(f"#{name} is private and the bot is not in it: invite @hivey there")
        # The relay reads the channel's history: the bot has to be in it.
        try:
            slack.call("conversations.join", channel=channel["id"])
        except RuntimeError as err:
            raise RuntimeError(f"cannot join #{name}: {err}") from err
    return channel, True


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("name")
    parser.add_argument("--purpose")
    args = parser.parse_args()
    if not os.environ.get("HERDR_PLUGIN_CONFIG_DIR"):
        # Run outside a plugin pane: use the installed relay's config.
        os.environ["HERDR_PLUGIN_CONFIG_DIR"] = str(
            Path.home() / ".config" / "hivey" / "plugins" / "config" / "hivey.slack-relay")
    config = relay.load_config()
    token = relay.resolve_token(config)
    if not token:
        print(json.dumps({"ok": False, "error": "Slack is not connected: run hivey slack connect "
                          "in a terminal"}))
        return 1
    slack = relay.Slack(token)
    try:
        channel, existing = create(slack, args.name)
    except RuntimeError as err:
        print(json.dumps({"ok": False, "error": str(err)}))
        return 1
    private = bool(channel.get("is_private", not existing))
    if args.purpose and not existing:
        try:
            slack.call("conversations.setPurpose", channel=channel["id"], purpose=args.purpose[:250])
        except RuntimeError:
            pass
    invited = invite(slack, channel["id"], invitees(slack, config))
    print(json.dumps({"ok": True, "channel_id": channel["id"], "name": args.name,
                      "existing": existing, "private": private, "invited": invited}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
