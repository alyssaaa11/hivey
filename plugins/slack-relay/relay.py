#!/usr/bin/env python3
"""hivey.slack-relay: bridge a hivey swarm's message bus and its Slack channel.

Slack → bus   Human messages in the channel become bus messages from `human`:
              `@scout` → scout, `@all`/`@here`/`@channel`/`@everyone` → @all,
              `@coordinator`/`@master` → the master, no mention → the master.
              Posts made by agents with the /swarm skill's swarm_slack.py
              ("*[agent]* text") keep that agent as the sender.
bus → Slack   Bus messages are mirrored into the channel. Scope (config "mirror"):
              "masters" (default: only messages to/from a master or the human),
              "all", or "off". Messages that came from Slack are never echoed.

Environment (set by `hivey swarm launch --addon hivey.slack-relay`):
  HIVEY_SWARM_ROOT     swarm folder (required)   HIVEY_SWARM_SLUG   swarm slug
  HIVEY_SWARM_CHANNEL  Slack channel id (else the manifest's channel_id)
  HERDR_BIN_PATH       hivey binary (set by hivey for plugin panes)
Config: $HERDR_PLUGIN_CONFIG_DIR/config.json
  {"token_command": "envsave get <id>", "mirror": "masters", "interval": 10}
  The token comes from $SLACK_TOKEN if set, else from token_command's output.
  Scopes needed: channels:history, chat:write (groups:history for private channels).
State: <root>/.swarm/slack_relay_state.json (last Slack ts, bus offset, echo guards).
"""
import json
import os
import re
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

SENDER_RE = re.compile(r"^(?:(?::[a-z0-9_+-]+:|[^\x00-\x7F]+)\s+)?\*\[([^\]]+)\]\*\s*")
MENTION_RE = re.compile(r"@([a-z][a-z0-9_-]*)")
BROADCAST = {"all", "everyone", "here", "channel"}
MASTER_ALIASES = {"coordinator", "master"}
ROLE_EMOJI = {"master": "🧭", "critic": "🧐", "script": "⚙️"}
FIXED_EMOJI = {"human": "👤", "hivey": "⚙️"}
PALETTE = ["🦊", "🐙", "🦉", "🐝", "🦄", "🐬", "🦖", "🐢", "🦜", "🐳", "🦋", "🐧"]
KEEP_GUARDS = 500  # echo-guard ids/timestamps remembered


# ---------------------------------------------------------------------------
# Pure logic (unit-tested in test_relay.py)
# ---------------------------------------------------------------------------

def roster(manifest):
    """{agent key: role} from a /swarm or hivey manifest (master included)."""
    roles = {}
    for key, entry in (manifest.get("agents") or {}).items():
        role = entry.get("role") or ("critic" if key == "critic" else "worker")
        roles[key] = "master" if role in MASTER_ALIASES else role
    if not any(role == "master" for role in roles.values()):
        roles["coordinator"] = "master"
    return roles


def master_key(roles):
    return next((key for key, role in roles.items() if role == "master"), "coordinator")


def parse_slack(text, manifest):
    """(sender key or 'human', body, [targets]) for one Slack message.

    Targets are bus addresses inside the swarm: agent keys, '@all' or the master key.
    """
    roles = roster(manifest)
    slug = manifest.get("slug", "")
    match = SENDER_RE.match(text)
    sender = match.group(1) if match else "human"
    body = text[match.end():] if match else text
    body = re.sub(r"<!(channel|here|everyone)[^>]*>", r"@\1", body)
    by_name = {key: key for key in roles}
    by_name.update({f"{slug}-{key}": key for key in roles})
    targets = []
    for word in MENTION_RE.findall(body.lower()):
        if word in BROADCAST:
            target = "@all"
        elif word in MASTER_ALIASES:
            target = master_key(roles)
        else:
            target = by_name.get(word)
        if target and target not in targets and target != sender:
            targets.append(target)
    if not targets and sender == "human":
        targets.append(master_key(roles))
    return sender, body.strip(), targets


def involves_master(message, roles):
    def is_master(label):
        return label.split("/")[-1] in {k for k, r in roles.items() if r == "master"}
    return (
        message.get("from") in FIXED_EMOJI
        or message.get("to") == "human"
        or is_master(message.get("from", ""))
        or is_master(message.get("to", ""))
    )


def should_mirror(message, scope, roles, injected):
    if scope == "off" or message.get("copy") or message.get("id") in injected:
        return False
    return scope == "all" or involves_master(message, roles)


def emoji(label, roles):
    if label in FIXED_EMOJI:
        return FIXED_EMOJI[label]
    key = label.split("/")[-1]
    if roles.get(key) in ROLE_EMOJI:
        return ROLE_EMOJI[roles[key]]
    return PALETTE[sum(map(ord, key)) % len(PALETTE)]


def format_mirror(message, roles, slug):
    def short(label):
        return label[len(slug) + 1:] if label.startswith(f"{slug}/") else label
    target = message.get("to", "?")
    if message.get("swarm") and message["swarm"] != slug:
        target = f"{message['swarm']}/{target}"
    kind = {"fyi": " _(fyi)_", "urgent": " *(urgent)*"}.get(message.get("kind"), "")
    return (f"{emoji(message.get('from', '?'), roles)} *{short(message.get('from', '?'))} → "
            f"{short(target)}*{kind}: {message.get('text', '')}")


def new_bus_messages(path, offset):
    """(messages appended since offset, new offset); tolerates a torn last line."""
    try:
        with open(path, "rb") as f:
            f.seek(offset)
            data = f.read()
    except FileNotFoundError:
        return [], offset
    end = data.rfind(b"\n") + 1  # only complete lines
    messages = []
    for line in data[:end].splitlines():
        try:
            record = json.loads(line)
        except json.JSONDecodeError:
            continue
        if record.get("ev") == "msg":
            messages.append(record)
    return messages, offset + end


# ---------------------------------------------------------------------------
# I/O
# ---------------------------------------------------------------------------

class Slack:
    def __init__(self, token):
        self.token = token

    def call(self, method, http="POST", **params):
        url = f"https://slack.com/api/{method}"
        headers = {"Authorization": f"Bearer {self.token}"}
        if http == "GET":
            req = urllib.request.Request(url + "?" + urllib.parse.urlencode(params), headers=headers)
        else:
            headers["Content-Type"] = "application/json; charset=utf-8"
            req = urllib.request.Request(url, data=json.dumps(params).encode(), headers=headers)
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read().decode())
        if not data.get("ok"):
            raise RuntimeError(f"slack {method}: {data.get('error')}")
        return data

    def history(self, channel, oldest):
        messages, cursor = [], ""
        while True:
            data = self.call("conversations.history", http="GET", channel=channel,
                             oldest=oldest, limit=200, cursor=cursor)
            messages += data.get("messages", [])
            cursor = data.get("response_metadata", {}).get("next_cursor", "")
            if not data.get("has_more") or not cursor:
                return sorted(messages, key=lambda m: float(m["ts"]))

    def post(self, channel, text):
        return self.call("chat.postMessage", channel=channel, text=text)["ts"]


class Hivey:
    def __init__(self, binary, slug):
        self.binary, self.slug = binary, slug

    def send(self, target, text, sender):
        frm = "human" if sender == "human" else f"{self.slug}/{sender}"
        cmd = [self.binary, "msg", "send", target, text, "--swarm", self.slug, "--from", frm, "--json"]
        out = subprocess.run(cmd, capture_output=True, text=True)
        if out.returncode != 0:
            raise RuntimeError((out.stderr or out.stdout).strip())
        return [sent["id"] for sent in json.loads(out.stdout).get("sent", [])]


def load_config():
    path = Path(os.environ.get("HERDR_PLUGIN_CONFIG_DIR", ".")) / "config.json"
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def resolve_token(config):
    if os.environ.get("SLACK_TOKEN"):
        return os.environ["SLACK_TOKEN"]
    command = config.get("token_command")
    if not command:
        return None
    out = subprocess.run(command, shell=True, capture_output=True, text=True)
    return out.stdout.strip() or None


def log(line):
    print(f"{time.strftime('%H:%M:%S')} {line}", flush=True)


class Relay:
    def __init__(self, root, slack, hivey, channel, scope):
        self.root, self.slack, self.hivey = Path(root), slack, hivey
        self.channel, self.scope = channel, scope
        self.state_path = self.root / ".swarm" / "slack_relay_state.json"
        self.bus_path = self.root / ".swarm" / "bus.jsonl"
        try:
            self.state = json.loads(self.state_path.read_text())
        except (OSError, json.JSONDecodeError):
            # First run: start from now; don't replay history either way.
            self.state = {"last_ts": f"{time.time():.6f}",
                          "bus_offset": self.bus_path.stat().st_size if self.bus_path.exists() else 0,
                          "injected": [], "posted": []}

    def manifest(self):
        return json.loads((self.root / ".swarm" / "agents.json").read_text())

    def save(self):
        for key in ("injected", "posted"):
            self.state[key] = self.state[key][-KEEP_GUARDS:]
        tmp = self.state_path.with_suffix(".tmp")
        tmp.write_text(json.dumps(self.state))
        tmp.replace(self.state_path)

    def slack_to_bus(self, manifest):
        for msg in self.slack.history(self.channel, self.state["last_ts"]):
            self.state["last_ts"] = msg["ts"]
            if msg["ts"] in self.state["posted"] or msg.get("subtype") in ("channel_join", "bot_add"):
                continue
            sender, body, targets = parse_slack(msg.get("text", ""), manifest)
            for target in targets:
                try:
                    ids = self.hivey.send(target, body, sender)
                    self.state["injected"] += ids
                    log(f"slack → bus  {sender} → {target}: {body[:70]!r}")
                except RuntimeError as err:
                    log(f"slack → bus  failed for {target}: {err}")

    def bus_to_slack(self, manifest):
        messages, self.state["bus_offset"] = new_bus_messages(self.bus_path, self.state["bus_offset"])
        roles = roster(manifest)
        for message in messages:
            if not should_mirror(message, self.scope, roles, set(self.state["injected"])):
                continue
            ts = self.slack.post(self.channel, format_mirror(message, roles, manifest.get("slug", "")))
            self.state["posted"].append(ts)
            log(f"bus → slack  {message.get('from')} → {message.get('to')}")

    def step(self):
        manifest = self.manifest()
        self.slack_to_bus(manifest)
        self.bus_to_slack(manifest)
        self.save()


def main():
    root = os.environ.get("HIVEY_SWARM_ROOT") or (sys.argv[1] if len(sys.argv) > 1 else "")
    if not root:
        sys.exit("HIVEY_SWARM_ROOT is not set (open this through `hivey swarm launch --addon`)")
    config = load_config()
    manifest = json.loads((Path(root) / ".swarm" / "agents.json").read_text())
    slug = os.environ.get("HIVEY_SWARM_SLUG") or manifest["slug"]
    channel = os.environ.get("HIVEY_SWARM_CHANNEL") or manifest.get("channel_id")
    scope = config.get("mirror", "masters")
    interval = float(config.get("interval", 10))
    binary = os.environ.get("HERDR_BIN_PATH", "hivey")
    log(f"hivey slack relay for swarm {slug!r}, channel {channel}, mirror={scope}")
    if not channel:
        log("no Slack channel (manifest channel_id / HIVEY_SWARM_CHANNEL); idle")
    token = None
    while not (channel and token):
        token = token or resolve_token(config)
        if channel and not token:
            log(f"no Slack token: set SLACK_TOKEN or token_command in "
                f"{os.environ.get('HERDR_PLUGIN_CONFIG_DIR', '?')}/config.json; retrying in 60s")
        time.sleep(60 if not (channel and token) else 0)
    relay = Relay(root, Slack(token), Hivey(binary, slug), channel, scope)
    while True:
        try:
            relay.step()
        except Exception as err:  # network blips, a bad manifest read: keep relaying
            log(f"error: {err}")
        time.sleep(interval)


if __name__ == "__main__":
    main()
