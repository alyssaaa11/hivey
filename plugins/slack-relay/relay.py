#!/usr/bin/env python3
"""hivey.slack-relay: bridge a hivey swarm's message bus and its Slack channel.

Slack is for the human and the agents only: agents talk to each other on hivey's local
message bus (`hivey msg send`), never through Slack.
Slack → bus   Human messages in the channel become bus messages from `human`:
              `@scout` → scout, `@all`/`@here`/`@channel`/`@everyone` → @all,
              `@coordinator`/`@master` → the master, no mention → the master.
              Posts agents make in the channel ("*[agent]* text") are for the human and
              are never forwarded to other agents.
bus → Slack   Messages to or from the human (and hivey's own notices) are mirrored into
              the channel; agent↔agent messages never are. The swarm's own `comms.slack`
              (`hivey swarm comms <slug> --slack human|off`, read live), else config
              "mirror", can turn it "off". Messages that came from Slack are never echoed.
Quiet mode    While ~/.hivey/quiet exists (hivey menu → quiet mode, `hivey quiet on`), bus →
              Slack posts are held; when it ends they go out as one catch-up message, with
              posts agents held through swarm_slack.py (~/.hivey/quiet-held/<channel>.jsonl).
              Slack → bus keeps working. While the human is talking in the channel (a message
              in the last TALKING_MIN minutes) quiet mode doesn't apply to it: replies go out
              at once, and so does whatever was held.

Environment (set by `hivey swarm launch --addon hivey.slack-relay`):
  HIVEY_SWARM_ROOT     swarm folder (required)   HIVEY_SWARM_SLUG   swarm slug
  HIVEY_SWARM_CHANNEL  Slack channel id (else the manifest's channel_id)
  HERDR_BIN_PATH       hivey binary (set by hivey for plugin panes)
Config: $HERDR_PLUGIN_CONFIG_DIR/config.json
  {"token_command": "envsave get <id>", "mirror": "human", "interval": 10}
  The token comes from $SLACK_TOKEN if set, else from token_command's output.
  Scopes needed: channels:history, chat:write (groups:history for private channels).
Failures      Each direction fails on its own (a read error doesn't block posts). After 3 failed
              rounds in a row the relay alerts once (desktop notification + a post in the channel,
              when posting works), backs off up to 6× the interval, and says when it recovers.
Socket Mode   With an app-level token (`hivey slack connect --socket`), Slack pushes messages
              over one connection (socket_hub.py, started by the first relay that needs it) and
              relays read them from ~/.hivey/slack-events/<channel>.jsonl every HUB_ROUND s;
              while the hub is down they poll conversations.history every POLL_FALLBACK s.
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
# The relay's own posts: a mirror ("👤 *human → coordinator*: …") or a quiet-mode catch-up.
# Slack returns emoji as :shortcodes:. Agent posts ("*[scout]* …") don't match.
RELAY_POST_RE = re.compile(
    r"^(?::[a-z0-9_+-]+:|[^\x00-\x7F]+)\s+\*(?:[^*\n]+ → [^*\n]+|Quiet mode ended[^*\n]*)\*")
BROADCAST = {"all", "everyone", "here", "channel"}
MASTER_ALIASES = {"coordinator", "master"}
ROLE_EMOJI = {"master": "🧭", "critic": "🧐", "script": "⚙️"}
FIXED_EMOJI = {"human": "👤", "hivey": "⚙️"}
PALETTE = ["🦊", "🐙", "🦉", "🐝", "🦄", "🐬", "🦖", "🐢", "🦜", "🐳", "🦋", "🐧"]
KEEP_GUARDS = 500  # echo-guard ids/timestamps remembered
KEEP_HELD = 500  # posts held in quiet mode
TALKING_MIN = 30  # minutes after the human's last message that quiet mode skips the channel
CATCH_UP_LINES = 40  # lines in the catch-up message; the rest are counted
POLL_FALLBACK = 60  # seconds between history polls while the socket hub is down
HUB_ROUND = 2  # seconds between rounds while the socket hub delivers (local file reads only)
ALERT_AFTER = 3  # failed rounds in a row before the relay alerts the user (then it backs off)
MAX_BACKOFF = 6  # at most interval × this between rounds while failing


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
    if sender != "human":
        targets = []  # agents talk to each other on the bus, not through Slack
    elif not targets:
        targets.append(master_key(roles))
    return sender, body.strip(), targets


def is_relay_post(text):
    """True for text this relay posted. Its ts guard can miss one (a second relay on the
    channel, a restart between post and save); injecting it again starts an echo loop."""
    return bool(RELAY_POST_RE.match(text or ""))


MIRROR_SCOPES = ("human", "off")
# Older scopes that also mirrored agent↔agent messages; they now mean "human".
LEGACY_SCOPES = ("masters", "all")


def normal_scope(scope):
    return "human" if scope in LEGACY_SCOPES else scope


def mirror_scope(manifest, default):
    """The swarm's own Slack scope (manifest comms.slack) wins over the relay's config."""
    comms = manifest.get("comms") if isinstance(manifest.get("comms"), dict) else {}
    scope = normal_scope(comms.get("slack"))
    return scope if scope in MIRROR_SCOPES else (normal_scope(default) or "human")


def involves_human(message):
    """A message the human sent or gets (hivey's own notices count as the human's)."""
    return message.get("from") in FIXED_EMOJI or message.get("to") == "human"


def should_mirror(message, scope, roles, injected):
    if scope == "off" or message.get("copy") or message.get("id") in injected:
        return False
    return involves_human(message)


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


def format_catch_up(lines, ended=True):
    """The one message that replaces the posts held while quiet mode was on (`ended` False:
    it's still on, but the human is talking in the channel)."""
    shown = [line if len(line) <= 600 else line[:600] + "…" for line in lines[-CATCH_UP_LINES:]]
    count = f"{len(lines)} update{'s' if len(lines) != 1 else ''}"
    head = (f"🌙 *Quiet mode ended: {count} while it was on*" if ended
            else f"🌙 *{count} held by quiet mode*")
    if len(lines) > len(shown):
        head += f" _(last {len(shown)}; all of them: `hivey msg log`)_"
    return "\n".join([head] + shown)


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


class SocketHub:
    """The relay's side of socket_hub.py (one Socket Mode connection for every relay)."""

    def __init__(self, config):
        self.config, self.last_start = config, 0.0

    @staticmethod
    def _module():
        import socket_hub  # same folder; imported late (it imports this module)
        return socket_hub

    def state(self):
        return self._module().hub_state()

    def alive(self, state):
        return self._module().hub_alive(state)

    def end(self, channel):
        return self._module().end_position(channel)

    def read(self, channel, offset):
        return self._module().read_events(channel, offset)

    def start(self):
        """Starts the hub (detached; it exits at once when another one runs), at most once a
        minute, and only once an app-level token is set up."""
        if not self.config.get("app_token_command") or time.time() - self.last_start < 60:
            return
        self.last_start = time.time()
        hub = self._module()
        hub.events_dir().mkdir(parents=True, exist_ok=True)
        with open(hub.events_dir() / "hub.log", "a") as out:
            subprocess.Popen([sys.executable, str(Path(__file__).resolve().parent / "socket_hub.py")],
                             stdin=subprocess.DEVNULL, stdout=out, stderr=out,
                             start_new_session=True, env=os.environ.copy())


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


def hivey_dir():
    return Path(os.environ.get("HIVEY_HOME") or Path.home() / ".hivey")


def quiet_mode_on():
    return (hivey_dir() / "quiet").exists()


def take_held_posts(channel):
    """Posts agents held through swarm_slack.py in quiet mode for this channel (claimed by
    renaming the file, so a concurrent writer starts a new one)."""
    path = hivey_dir() / "quiet-held" / f"{channel}.jsonl"
    claimed = path.with_suffix(f".{os.getpid()}.claimed")
    try:
        path.replace(claimed)
    except FileNotFoundError:
        return []
    texts = []
    for line in claimed.read_text().splitlines():
        try:
            texts.append(json.loads(line)["text"])
        except (json.JSONDecodeError, KeyError, TypeError):
            continue
    claimed.unlink()
    return texts


def desktop_notify(text):
    """Best-effort macOS notification; silent elsewhere or if osascript is missing."""
    if sys.platform != "darwin":
        return
    plain = re.sub(r"[*_`]", "", text).replace('"', "'")[:230]
    try:
        subprocess.run(["osascript", "-e", f'display notification "{plain}" with title "hivey relay"'],
                       capture_output=True, timeout=10)
    except (OSError, subprocess.SubprocessError):
        pass


def log(line):
    print(f"{time.strftime('%H:%M:%S')} {line}", flush=True)


class Relay:
    def __init__(self, root, slack, hivey, channel, scope, quiet=quiet_mode_on,
                 held_posts=take_held_posts, notify=None, clock=time.time, hub=None):
        self.root, self.slack, self.hivey = Path(root), slack, hivey
        self.notify = notify or desktop_notify
        self.failures, self.alerted, self.last_error = 0, False, ""
        self.channel, self.scope = channel, scope
        self.quiet, self.held_posts, self.clock = quiet, held_posts, clock
        # The Socket Mode hub (socket_hub.py); None: poll conversations.history every round.
        self.hub, self.hub_live, self.last_poll = hub, False, 0.0
        self.state_path = self.root / ".swarm" / "slack_relay_state.json"
        self.bus_path = self.root / ".swarm" / "bus.jsonl"
        try:
            self.state = json.loads(self.state_path.read_text())
        except (OSError, json.JSONDecodeError):
            # First run: start from now; don't replay history either way.
            self.state = {"last_ts": f"{time.time():.6f}",
                          "bus_offset": self.bus_path.stat().st_size if self.bus_path.exists() else 0,
                          "injected": [], "posted": []}
        self.state.setdefault("held", [])

    def manifest(self):
        return json.loads((self.root / ".swarm" / "agents.json").read_text())

    def save(self):
        for key in ("injected", "posted"):
            self.state[key] = self.state[key][-KEEP_GUARDS:]
        self.state["held"] = self.state["held"][-KEEP_HELD:]
        tmp = self.state_path.with_suffix(".tmp")
        tmp.write_text(json.dumps(self.state))
        tmp.replace(self.state_path)

    def incoming(self):
        """New channel messages: pushed by the socket hub when it runs (after one history
        catch-up per hub connection), else polled (every POLL_FALLBACK s with a hub)."""
        if self.hub is None:
            return self.slack.history(self.channel, self.state["last_ts"])
        hub = self.hub.state()
        self.hub_live = self.hub.alive(hub)
        if not self.hub_live:
            self.hub.start()
            if self.clock() - self.last_poll < POLL_FALLBACK:
                return []
            self.last_poll = self.clock()
            return self.slack.history(self.channel, self.state["last_ts"])
        messages = []
        if "events_at" not in self.state:  # first sight of the hub: don't replay its file
            self.state["events_at"] = self.hub.end(self.channel)
        if hub.get("connected_at") != self.state.get("hub_connected_at"):
            messages = self.slack.history(self.channel, self.state["last_ts"])
            self.state["hub_connected_at"] = hub.get("connected_at")
        events, self.state["events_at"] = self.hub.read(self.channel, self.state["events_at"])
        return sorted(messages + events, key=lambda m: float(m["ts"]))

    def slack_to_bus(self, manifest):
        for msg in self.incoming():
            if float(msg["ts"]) <= float(self.state["last_ts"]):
                continue  # seen (history and the hub overlap after a reconnect)
            self.state["last_ts"] = msg["ts"]
            if msg["ts"] in self.state["posted"] or msg.get("subtype") in ("channel_join", "bot_add"):
                continue
            if is_relay_post(msg.get("text", "")):
                continue
            sender, body, targets = parse_slack(msg.get("text", ""), manifest)
            if sender == "human" and targets:
                self.state["human_at"] = self.clock()
            for target in targets:
                try:
                    ids = self.hivey.send(target, body, sender)
                    self.state["injected"] += ids
                    log(f"slack → bus  {sender} → {target}: {body[:70]!r}")
                except RuntimeError as err:
                    log(f"slack → bus  failed for {target}: {err}")

    def human_talking(self):
        """The human wrote in the channel lately: they're here, so replies shouldn't wait."""
        at = self.state.get("human_at")
        return at is not None and self.clock() - at < TALKING_MIN * 60

    def bus_to_slack(self, manifest):
        quiet_mode = self.quiet()
        quiet = quiet_mode and not self.human_talking()
        if not quiet:
            self.catch_up(ended=not quiet_mode)  # before reading the bus: if it fails, nothing new is lost
        messages, self.state["bus_offset"] = new_bus_messages(self.bus_path, self.state["bus_offset"])
        roles = roster(manifest)
        scope = mirror_scope(manifest, self.scope)
        for message in messages:
            if not should_mirror(message, scope, roles, set(self.state["injected"])):
                continue
            text = format_mirror(message, roles, manifest.get("slug", ""))
            if quiet:
                self.state["held"].append(text)
                log(f"bus → held   {message.get('from')} → {message.get('to')} (quiet mode)")
                continue
            ts = self.slack.post(self.channel, text)
            self.state["posted"].append(ts)
            log(f"bus → slack  {message.get('from')} → {message.get('to')}")

    def catch_up(self, ended=True):
        """After quiet mode: everything held goes out as one message."""
        held = self.state["held"] + self.held_posts(self.channel)
        if not held:
            return
        # Saved until posted, so a failed post (or a restart) retries it
        self.state["held"] = held
        self.save()
        ts = self.slack.post(self.channel, format_catch_up(held, ended))
        self.state["posted"].append(ts)
        self.state["held"] = []
        log(f"bus → slack  catch-up of {len(held)} held post(s)")

    def step(self):
        """One round. The two directions fail independently: a read error (e.g. a missing
        groups:history scope) must not stop agents' posts from reaching Slack."""
        manifest = self.manifest()
        errors = []
        for part in (self.slack_to_bus, self.bus_to_slack):
            try:
                part(manifest)
            except Exception as err:  # network blips, Slack errors: keep the other direction going
                errors.append(f"{part.__name__.replace('_', ' ')}: {err}")
        self.save()
        self.track(errors)
        return errors

    def track(self, errors):
        """Silent retry loops hid a broken relay for 30 minutes (verifier audit 2026-11-05):
        after ALERT_AFTER failed rounds in a row, tell the user once; say when it recovers."""
        if not errors:
            if self.alerted:
                self.alert(f"✅ *hivey relay → you*: #{self.slug()} works again after "
                           f"{self.failures} failed rounds.")
            self.failures, self.alerted, self.last_error = 0, False, ""
            return
        self.failures += 1
        self.last_error = errors[0]
        for e in errors:
            log(f"error: {e}")
        if self.failures >= ALERT_AFTER and not self.alerted:
            self.alerted = True
            self.alert(f"⚠️ *hivey relay → you*: #{self.slug()} is failing ({self.failures} rounds "
                       f"in a row): {errors[0][:200]}. Messages may not reach agents or Slack until "
                       f"it's fixed. Log: the relay pane of this swarm.")

    def slug(self):
        try:
            return self.manifest().get("slug", "?")
        except (OSError, json.JSONDecodeError):
            return "?"

    def alert(self, text):
        log(f"ALERT {text}")
        self.notify(text)
        try:  # posting may still work when reading doesn't
            self.state["posted"].append(self.slack.post(self.channel, text))
            self.save()
        except Exception as err:
            log(f"alert post failed too: {err}")

    def wait_seconds(self, interval):
        base = HUB_ROUND if self.hub_live else interval
        return base * min(MAX_BACKOFF, max(1, self.failures))


def claim(pid_path):
    """Make this process the swarm's relay. Returns a check that is False once a newer relay
    has claimed it: closing a pane doesn't always stop the relay in it, and two relays on one
    channel double every post and poll (HTTP 429) and can echo each other's posts."""
    pid_path.write_text(str(os.getpid()))

    def still_ours():
        try:
            return pid_path.read_text().strip() == str(os.getpid())
        except OSError:
            return True  # unreadable for a moment: keep going, the next check decides
    return still_ours


def main():
    root = os.environ.get("HIVEY_SWARM_ROOT") or (sys.argv[1] if len(sys.argv) > 1 else "")
    if not root:
        sys.exit("HIVEY_SWARM_ROOT is not set (open this through `hivey swarm launch --addon`)")
    config = load_config()
    manifest = json.loads((Path(root) / ".swarm" / "agents.json").read_text())
    slug = os.environ.get("HIVEY_SWARM_SLUG") or manifest["slug"]
    channel = os.environ.get("HIVEY_SWARM_CHANNEL") or manifest.get("channel_id")
    scope = normal_scope(config.get("mirror", "human"))
    interval = float(config.get("interval", 10))
    binary = os.environ.get("HERDR_BIN_PATH", "hivey")
    log(f"hivey slack relay for swarm {slug!r}, channel {channel}, "
        f"mirror={mirror_scope(manifest, scope)}")
    if not channel:
        log("no Slack channel (manifest channel_id / HIVEY_SWARM_CHANNEL); idle")
    token = None
    while not (channel and token):
        token = token or resolve_token(config)
        if channel and not token:
            log(f"no Slack token: set SLACK_TOKEN or token_command in "
                f"{os.environ.get('HERDR_PLUGIN_CONFIG_DIR', '?')}/config.json; retrying in 60s")
        time.sleep(60 if not (channel and token) else 0)
    relay = Relay(root, Slack(token), Hivey(binary, slug), channel, scope, hub=SocketHub(config))
    still_ours = claim(Path(root) / ".swarm" / "slack_relay.pid")
    while True:
        if not still_ours():
            log("a newer relay took over this swarm; exiting")
            return
        try:
            relay.step()
        except Exception as err:  # a bad manifest read: keep relaying
            log(f"error: {err}")
            relay.track([f"step: {err}"])
        time.sleep(relay.wait_seconds(interval))


if __name__ == "__main__":
    main()
