#!/usr/bin/env python3
"""hivey.slack-relay socket hub: ONE Slack Socket Mode connection for every swarm's relay.

Polling conversations.history per channel doesn't scale (each relay asked every 10 s, and
Slack answers HTTP 429). With Socket Mode, Slack pushes each new message over one WebSocket;
this hub appends it to ~/.hivey/slack-events/<channel>.jsonl and each relay reads its own
channel's file. Relays start the hub when it isn't running (one per machine: a lock file),
and fall back to slow polling while it's down. After every (re)connect the hub bumps
`connected_at` in hub.json, so relays fetch whatever they missed once.

The app-level token (xapp-…, scope connections:write) comes from config.json's
app_token_command (`hivey slack connect --socket` sets it up). Standard library only: a small
WebSocket client (RFC 6455, text frames, ping/pong, close) is below.
"""
import base64
import fcntl
import json
import os
import socket
import ssl
import struct
import sys
import time
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import relay  # noqa: E402  (same folder)

BEAT_EVERY = 10  # seconds between hub.json heartbeats
STALE_AFTER = 45  # a heartbeat older than this means the hub is down
ROTATE_BYTES = 5 * 1024 * 1024  # an events file is rotated to .old past this size
# Message events relays care about: new top-level posts (history returns the same ones).
KEPT_SUBTYPES = {None, "file_share", "thread_broadcast", "me_message", "bot_message"}


def events_dir():
    return relay.hivey_dir() / "slack-events"


def events_path(channel):
    return events_dir() / f"{channel}.jsonl"


def hub_state():
    """{pid, connected_at, beat} or {} when the hub never ran."""
    try:
        return json.loads((events_dir() / "hub.json").read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def hub_alive(state=None, now=None):
    state = hub_state() if state is None else state
    now = time.time() if now is None else now
    return bool(state.get("connected_at")) and now - state.get("beat", 0) < STALE_AFTER


def resolve_app_token(config):
    if os.environ.get("SLACK_APP_TOKEN"):
        return os.environ["SLACK_APP_TOKEN"]
    command = config.get("app_token_command")
    if not command:
        return None
    import subprocess
    out = subprocess.run(command, shell=True, capture_output=True, text=True)
    return out.stdout.strip() or None


def keep(event):
    """True for a message event a relay forwards: a channel post, not an edit/delete, not a
    reply inside a thread (relays read the channel, like conversations.history does)."""
    if event.get("type") != "message" or not event.get("channel"):
        return False
    if event.get("subtype") not in KEPT_SUBTYPES:
        return False
    thread = event.get("thread_ts")
    return not thread or thread == event.get("ts") or event.get("subtype") == "thread_broadcast"


def record(event):
    """Appends one message event to its channel's file (rotating a big file first)."""
    path = events_path(event["channel"])
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        if path.stat().st_size > ROTATE_BYTES:
            path.replace(path.with_suffix(".old"))
    except FileNotFoundError:
        pass
    fields = ("ts", "user", "bot_id", "text", "subtype", "thread_ts", "channel")
    line = json.dumps({key: event[key] for key in fields if key in event})
    with open(path, "a") as handle:
        handle.write(line + "\n")


def read_lines(path, offset):
    """(JSON records appended since offset, new offset); tolerates a torn last line."""
    try:
        with open(path, "rb") as handle:
            handle.seek(offset)
            data = handle.read()
    except FileNotFoundError:
        return [], offset
    end = data.rfind(b"\n") + 1  # only complete lines
    records = []
    for line in data[:end].splitlines():
        try:
            records.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return records, offset + end


def end_position(channel):
    """Where a reader starts to see only events recorded from now on."""
    try:
        stat = events_path(channel).stat()
    except FileNotFoundError:
        return {"ino": 0, "off": 0}
    return {"ino": stat.st_ino, "off": stat.st_size}


def read_events(channel, position):
    """(events recorded since position, new position). A position is {ino, off}: the file's
    inode tells a rotation apart (the old file is finished first, then the new one)."""
    path = events_path(channel)
    try:
        ino = path.stat().st_ino
    except FileNotFoundError:
        return [], {"ino": 0, "off": 0}
    events, offset = [], position.get("off", 0)
    if position.get("ino") not in (0, ino):  # rotated since the last read
        old = path.with_suffix(".old")
        try:
            if old.stat().st_ino == position.get("ino"):
                events, _ = read_lines(old, offset)
        except FileNotFoundError:
            pass
        offset = 0
    elif position.get("ino") == 0:
        offset = 0
    new, offset = read_lines(path, offset)
    return events + new, {"ino": ino, "off": offset}


# ---------------------------------------------------------------------------
# WebSocket (client side of RFC 6455; just what Socket Mode needs)
# ---------------------------------------------------------------------------

class WebSocket:
    def __init__(self, url, timeout=BEAT_EVERY):
        parts = urllib.parse.urlsplit(url)
        host, port = parts.hostname, parts.port or 443
        raw = socket.create_connection((host, port), timeout=30)
        self.sock = ssl.create_default_context().wrap_socket(raw, server_hostname=host)
        key = base64.b64encode(os.urandom(16)).decode()
        path = parts.path + ("?" + parts.query if parts.query else "")
        self.sock.sendall((f"GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\n"
                           f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                           "Sec-WebSocket-Version: 13\r\n\r\n").encode())
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise ConnectionError("closed during the handshake")
            head += chunk
        head, self.buffer = head.split(b"\r\n\r\n", 1)
        if b" 101 " not in head.split(b"\r\n", 1)[0]:
            raise ConnectionError(head.split(b"\r\n", 1)[0].decode(errors="replace"))
        self.sock.settimeout(timeout)

    def _read(self, count):
        while len(self.buffer) < count:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError("connection closed")
            self.buffer += chunk
        data, self.buffer = self.buffer[:count], self.buffer[count:]
        return data

    def send(self, opcode, payload=b""):
        if isinstance(payload, str):
            payload = payload.encode()
        mask = os.urandom(4)
        length = len(payload)
        if length < 126:
            header = struct.pack("!BB", 0x80 | opcode, 0x80 | length)
        elif length < 1 << 16:
            header = struct.pack("!BBH", 0x80 | opcode, 0x80 | 126, length)
        else:
            header = struct.pack("!BBQ", 0x80 | opcode, 0x80 | 127, length)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.sock.sendall(header + mask + masked)

    def receive(self):
        """The next text message (str), or None on a read timeout. Answers pings; raises
        ConnectionError when the server closes."""
        message = b""
        while True:
            try:
                first, second = self._read(2)
            except (socket.timeout, TimeoutError):
                if message:
                    continue
                return None
            opcode, length = first & 0x0F, second & 0x7F
            if length == 126:
                length = struct.unpack("!H", self._read(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self._read(8))[0]
            mask = self._read(4) if second & 0x80 else None
            payload = self._read(length)
            if mask:
                payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
            if opcode == 0x9:  # ping
                self.send(0xA, payload)
                continue
            if opcode == 0xA:  # pong
                continue
            if opcode == 0x8:
                raise ConnectionError("server closed the connection")
            message += payload
            if first & 0x80:  # final fragment
                return message.decode(errors="replace")

    def close(self):
        try:
            self.send(0x8)
            self.sock.close()
        except OSError:
            pass


# ---------------------------------------------------------------------------
# The hub
# ---------------------------------------------------------------------------

def open_url(app_token):
    req = urllib.request.Request("https://slack.com/api/apps.connections.open", data=b"",
                                 headers={"Authorization": f"Bearer {app_token}"})
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode())
    if not data.get("ok"):
        raise RuntimeError(f"apps.connections.open: {data.get('error')}")
    return data["url"]


def beat(connected_at):
    path = events_dir() / "hub.json"
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps({"pid": os.getpid(), "connected_at": connected_at,
                               "beat": time.time()}))
    tmp.replace(path)


def handle(envelope, ws):
    """Acks one Socket Mode envelope and records its message event. False: reconnect."""
    if envelope.get("envelope_id"):
        ws.send(0x1, json.dumps({"envelope_id": envelope["envelope_id"]}))
    kind = envelope.get("type")
    if kind == "disconnect":
        return False
    if kind == "events_api":
        event = envelope.get("payload", {}).get("event", {})
        if keep(event):
            record(event)
    return True


def serve(app_token):
    ws = WebSocket(open_url(app_token))
    connected_at = time.time()
    relay.log("connected to Slack (Socket Mode)")
    beat(connected_at)
    last_beat = time.time()
    try:
        while True:
            text = ws.receive()
            if text is not None:
                try:
                    envelope = json.loads(text)
                except json.JSONDecodeError:
                    continue
                if not handle(envelope, ws):
                    relay.log("Slack asked to reconnect")
                    return
            if time.time() - last_beat >= BEAT_EVERY:
                beat(connected_at)
                last_beat = time.time()
    finally:
        ws.close()


def main():
    events_dir().mkdir(parents=True, exist_ok=True)
    lock = open(events_dir() / "hub.lock", "w")
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        return 0  # another hub is running
    os.environ.setdefault("HERDR_PLUGIN_CONFIG_DIR", str(
        Path.home() / ".config" / "hivey" / "plugins" / "config" / "hivey.slack-relay"))
    app_token = resolve_app_token(relay.load_config())
    if not app_token:
        relay.log("no app-level token (hivey slack connect --socket); exiting")
        return 1
    failures = 0
    while True:
        try:
            serve(app_token)
            failures = 0
        except Exception as err:  # network, Slack errors: back off and reconnect
            failures += 1
            relay.log(f"error: {err}")
            if "invalid_auth" in str(err) or "not_allowed_token_type" in str(err):
                return 1
        time.sleep(min(60, 2 ** min(failures, 6)))


if __name__ == "__main__":
    sys.exit(main())
