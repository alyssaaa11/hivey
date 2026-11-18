"""Tests for the Socket Mode hub and the relay reading from it (no network):
python3 -m unittest plugins/slack-relay/test_socket_hub.py"""
import json
import os
import socket
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
os.environ["HIVEY_HOME"] = tempfile.mkdtemp()  # never the real ~/.hivey
import relay  # noqa: E402
import socket_hub  # noqa: E402
from test_relay import MANIFEST, FakeHivey, FakeSlack  # noqa: E402


class Events(unittest.TestCase):
    def test_keeps_channel_posts_only(self):
        post = {"type": "message", "channel": "C1", "ts": "1.0", "text": "hi"}
        self.assertTrue(socket_hub.keep(post))
        self.assertFalse(socket_hub.keep({**post, "subtype": "message_changed"}))
        self.assertFalse(socket_hub.keep({**post, "thread_ts": "0.5"}), "a reply in a thread")
        self.assertTrue(socket_hub.keep({**post, "thread_ts": "1.0"}), "a thread's parent")
        self.assertTrue(socket_hub.keep({**post, "thread_ts": "0.5", "subtype": "thread_broadcast"}))
        self.assertFalse(socket_hub.keep({"type": "reaction_added", "channel": "C1"}))

    def test_records_and_reads_across_a_rotation(self):
        channel = "CROT"
        socket_hub.record({"type": "message", "channel": channel, "ts": "1.0", "text": "a"})
        events, position = socket_hub.read_events(channel, {"ino": 0, "off": 0})
        self.assertEqual([e["text"] for e in events], ["a"])
        socket_hub.record({"type": "message", "channel": channel, "ts": "2.0", "text": "b"})
        path = socket_hub.events_path(channel)
        path.replace(path.with_suffix(".old"))  # what record() does past ROTATE_BYTES
        socket_hub.record({"type": "message", "channel": channel, "ts": "3.0", "text": "c"})
        events, position = socket_hub.read_events(channel, position)
        self.assertEqual([e["text"] for e in events], ["b", "c"])
        self.assertEqual(socket_hub.read_events(channel, position), ([], position))
        socket_hub.record({"type": "message", "channel": channel, "ts": "4.0", "text": "d"})
        self.assertEqual(socket_hub.read_events(channel, socket_hub.end_position(channel))[0], [])

    def test_a_stale_heartbeat_means_the_hub_is_down(self):
        self.assertFalse(socket_hub.hub_alive({}, now=100))
        state = {"connected_at": 1, "beat": 100}
        self.assertTrue(socket_hub.hub_alive(state, now=110))
        self.assertFalse(socket_hub.hub_alive(state, now=100 + socket_hub.STALE_AFTER))


class Frames(unittest.TestCase):
    """The WebSocket client against a fake server on a socket pair (no TLS)."""

    def setUp(self):
        self.client_sock, self.server = socket.socketpair()
        self.ws = socket_hub.WebSocket.__new__(socket_hub.WebSocket)
        self.ws.sock, self.ws.buffer = self.client_sock, b""
        self.client_sock.settimeout(0.2)

    def tearDown(self):
        self.client_sock.close()
        self.server.close()

    def server_frame(self, opcode, payload, fin=True):
        payload = payload.encode() if isinstance(payload, str) else payload
        head = bytes([(0x80 if fin else 0) | opcode])
        if len(payload) < 126:
            head += bytes([len(payload)])
        else:
            head += bytes([126]) + len(payload).to_bytes(2, "big")
        self.server.sendall(head + payload)

    def read_client_frame(self):
        first, second = self.server.recv(2)
        length = second & 0x7F
        if length == 126:
            length = int.from_bytes(self.server.recv(2), "big")
        mask = self.server.recv(4)
        data = b""
        while len(data) < length:
            data += self.server.recv(length - len(data))
        return first & 0x0F, bytes(b ^ mask[i % 4] for i, b in enumerate(data))

    def test_reads_text_answers_pings_and_joins_fragments(self):
        self.server_frame(0x9, "p")
        self.server_frame(0x1, "hel", fin=False)
        self.server_frame(0x0, "lo")
        self.assertEqual(self.ws.receive(), "hello")
        self.assertEqual(self.read_client_frame(), (0xA, b"p"), "the ping was answered")
        long = "x" * 300
        self.server_frame(0x1, long)
        self.assertEqual(self.ws.receive(), long)
        self.assertIsNone(self.ws.receive(), "a quiet socket times out")

    def test_close_raises(self):
        self.server_frame(0x8, b"")
        with self.assertRaises(ConnectionError):
            self.ws.receive()

    def test_acks_envelopes_and_records_messages(self):
        envelope = {"envelope_id": "e1", "type": "events_api", "payload": {"event": {
            "type": "message", "channel": "CACK", "ts": "5.0", "text": "yo", "user": "U1"}}}
        self.assertTrue(socket_hub.handle(envelope, self.ws))
        opcode, ack = self.read_client_frame()
        self.assertEqual((opcode, json.loads(ack)), (0x1, {"envelope_id": "e1"}))
        events, _ = socket_hub.read_events("CACK", {})
        self.assertEqual(events[0]["text"], "yo")
        self.assertFalse(socket_hub.handle({"type": "disconnect"}, self.ws))


class FakeHub:
    def __init__(self):
        self.live, self.connected_at, self.events, self.starts = True, 1.0, [], 0

    def state(self):
        return {"connected_at": self.connected_at} if self.live else {}

    def alive(self, state):
        return self.live

    def end(self, channel):
        return 0

    def read(self, channel, offset):
        return self.events[offset:], len(self.events)

    def start(self):
        self.starts += 1


class RelayWithHub(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = Path(self.tmp.name)
        (root / ".swarm").mkdir()
        (root / ".swarm" / "agents.json").write_text(json.dumps(MANIFEST))
        bus = root / ".swarm" / "bus.jsonl"
        bus.write_text("")
        self.slack, self.hivey, self.hub = FakeSlack(), FakeHivey(bus), FakeHub()
        self.now = 1000.0
        self.relay = relay.Relay(root, self.slack, self.hivey, "C1", "human",
                                 quiet=lambda: False, clock=lambda: self.now, hub=self.hub)
        self.relay.state["last_ts"] = "0"
        self.history_calls = 0
        history = self.slack.history

        def counted(channel, oldest):
            self.history_calls += 1
            return history(channel, oldest)
        self.slack.history = counted

    def tearDown(self):
        self.tmp.cleanup()

    def test_pushed_messages_need_no_polling(self):
        self.slack.inbox.append({"ts": "1.000000", "text": "missed while down"})
        self.relay.step()
        self.assertEqual(self.history_calls, 1, "one catch-up per hub connection")
        self.hub.events.append({"ts": "2.000000", "text": "hello"})
        self.slack.inbox.append({"ts": "2.000000", "text": "hello"})
        for _ in range(5):
            self.relay.step()
        self.assertEqual(self.history_calls, 1, "no polling while the hub delivers")
        self.assertEqual([s[1] for s in self.hivey.sent], ["missed while down", "hello"])
        self.assertEqual(self.relay.wait_seconds(10), relay.HUB_ROUND)
        self.hub.connected_at = 2.0  # reconnected: catch up once more, nothing doubled
        self.relay.step()
        self.assertEqual(self.history_calls, 2)
        self.assertEqual(len(self.hivey.sent), 2)

    def test_polls_slowly_and_starts_the_hub_while_it_is_down(self):
        self.hub.live = False
        self.relay.step()
        self.relay.step()
        self.assertEqual(self.history_calls, 1)
        self.assertEqual(self.hub.starts, 2)
        self.now += relay.POLL_FALLBACK
        self.relay.step()
        self.assertEqual(self.history_calls, 2)


if __name__ == "__main__":
    unittest.main()
