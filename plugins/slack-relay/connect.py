#!/usr/bin/env python3
"""Connect hiver to the user's Slack workspace: ask for a bot token, check it, store it.

usage: connect.py [--force]     interactive: create the Slack app, paste its token
       connect.py --status      is Slack connected? (exit 0 yes, 1 no) [--json]

Run by `hiver slack connect` / `hiver slack status` and by install.sh. The token is typed
hidden, checked with Slack (auth.test), saved to <config dir>/slack-token (mode 600) and
read through config.json's token_command, so the relay and create_channel.py use it. An
existing working token_command (e.g. a secrets manager) is kept unless --force.
"""
import argparse
import getpass
import json
import os
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import relay  # noqa: E402  (same folder)

HERE = Path(__file__).resolve().parent
APP_MANIFEST = HERE / "slack-app-manifest.json"
NEW_APP_URL = "https://api.slack.com/apps?new_app=1"
NEEDED_SCOPES = {"channels:history", "channels:join", "channels:manage", "channels:read",
                 "chat:write"}


def config_dir():
    path = os.environ.get("HERDR_PLUGIN_CONFIG_DIR") or str(
        Path.home() / ".config" / "hiver" / "plugins" / "config" / "hiver.slack-relay")
    return Path(path)


def auth_test(token):
    """(auth.test reply, granted scopes) for a token; raises RuntimeError when Slack refuses it."""
    req = urllib.request.Request("https://slack.com/api/auth.test", data=b"",
                                 headers={"Authorization": f"Bearer {token}"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode())
        scopes = {s.strip() for s in (resp.headers.get("x-oauth-scopes") or "").split(",")}
    if not data.get("ok"):
        raise RuntimeError(data.get("error") or "unknown error")
    return data, scopes - {""}


def current():
    """{connected, team, user, missing_scopes, error} for the configured token."""
    os.environ["HERDR_PLUGIN_CONFIG_DIR"] = str(config_dir())
    token = relay.resolve_token(relay.load_config())
    if not token:
        return {"connected": False, "error": "no Slack token set"}
    try:
        data, scopes = auth_test(token)
    except (RuntimeError, OSError) as err:
        return {"connected": False, "error": f"Slack refused the token: {err}"}
    return {"connected": True, "team": data.get("team"), "user": data.get("user"),
            "missing_scopes": sorted(NEEDED_SCOPES - scopes) if scopes else []}


def describe(state):
    if not state["connected"]:
        return f"Slack: not connected ({state['error']})"
    line = f"Slack: connected to {state['team']} as @{state['user']}"
    if state["missing_scopes"]:
        line += ("\n  missing scopes: " + ", ".join(state["missing_scopes"]) +
                 " (add them under OAuth & Permissions, reinstall the app, then "
                 "hiver slack connect --force)")
    return line


def save_token(token):
    directory = config_dir()
    directory.mkdir(parents=True, exist_ok=True)
    token_file = directory / "slack-token"
    fd = os.open(token_file, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as handle:
        handle.write(token + "\n")
    os.chmod(token_file, 0o600)
    config_path = directory / "config.json"
    config = relay.load_config() if config_path.exists() else {}
    config["token_command"] = f"cat '{token_file}'"
    config.setdefault("mirror", "masters")
    config.setdefault("interval", 10)
    config_path.write_text(json.dumps(config, indent=2) + "\n")
    return token_file


def remember_you(token):
    """Who hiver invites to the channels it creates (config "invite"), so they show up in your
    Slack: the person the bot already shares channels with, else your member id, asked once."""
    import create_channel  # same folder
    config = relay.load_config()
    if config.get("invite"):
        return
    try:
        found = create_channel.shared_people(relay.Slack(token))[:1]
    except (RuntimeError, OSError):
        found = []
    if not found:
        print("\nhiver invites you to the channels it creates, so they appear in your Slack.")
        try:
            answer = input("Your Slack member ID (your profile → ⋯ → Copy member ID; "
                           "Enter to skip): ").strip()
        except (EOFError, KeyboardInterrupt):
            answer = ""
        found = [answer] if answer.startswith(("U", "W")) else []
    if found:
        config["invite"] = found
        (config_dir() / "config.json").write_text(json.dumps(config, indent=2) + "\n")
        print(f"new channels will invite: {', '.join(found)}")
    else:
        print("skipped: new channels won't invite anyone (set \"invite\" in "
              f"{config_dir() / 'config.json'})")


def walk_through():
    print(f"""
Connect hiver to Slack (about 2 minutes):
  1. Open {NEW_APP_URL}
     → "From a manifest" → pick your workspace → paste the manifest below → Create.
  2. Click "Install to Workspace" → Allow.
  3. Copy the "Bot User OAuth Token" (starts with xoxb-) from OAuth & Permissions.

Manifest ({APP_MANIFEST}):
{APP_MANIFEST.read_text()}""")
    if sys.platform == "darwin" and shutil.which("pbcopy"):
        subprocess.run(["pbcopy"], input=APP_MANIFEST.read_bytes(), check=False)
        print("(the manifest is on your clipboard)")
    if sys.platform == "darwin" and shutil.which("open"):
        subprocess.run(["open", NEW_APP_URL], check=False)
    elif shutil.which("xdg-open"):
        subprocess.run(["xdg-open", NEW_APP_URL], check=False,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def connect(force):
    state = current()
    if state["connected"] and not force:
        print(describe(state))
        print("(hiver slack connect --force to use another token)")
        return 0
    if not sys.stdin.isatty():
        print("hiver slack connect needs a terminal to type the token in "
              "(never paste it into a chat)", file=sys.stderr)
        return 1
    walk_through()
    for _ in range(3):
        try:
            token = getpass.getpass("\nPaste the Bot User OAuth Token (hidden, empty to cancel): ")
        except (EOFError, KeyboardInterrupt):
            print()
            return 1
        token = token.strip()
        if not token:
            print("cancelled: Slack not connected (later: hiver slack connect)")
            return 1
        if not token.startswith(("xoxb-", "xoxp-")):
            print("that is not a Slack token (it starts with xoxb-); try again")
            continue
        try:
            data, scopes = auth_test(token)
        except (RuntimeError, OSError) as err:
            print(f"Slack refused it ({err}); try again")
            continue
        token_file = save_token(token)
        print(f"saved (only you can read it): {token_file}")
        missing = sorted(NEEDED_SCOPES - scopes) if scopes else []
        print(describe({"connected": True, "team": data.get("team"), "user": data.get("user"),
                        "missing_scopes": missing}))
        remember_you(token)
        return 0
    print("Slack not connected (later: hiver slack connect)")
    return 1


def main():
    parser = argparse.ArgumentParser(description="connect hiver to Slack")
    parser.add_argument("--status", action="store_true")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--force", action="store_true")
    args = parser.parse_args()
    if args.status:
        state = current()
        print(json.dumps(state) if args.json else describe(state))
        return 0 if state["connected"] else 1
    return connect(args.force)


if __name__ == "__main__":
    sys.exit(main())
