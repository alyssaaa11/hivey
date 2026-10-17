"""Tests for the hivey Slack relay: python3 -m unittest plugins/slack-relay/test_relay.py"""
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import relay  # noqa: E402

# Never the real ~/.hivey (its quiet mode, its held posts)
os.environ["HIVEY_HOME"] = tempfile.mkdtemp()

MANIFEST = {
    "slug": "ideas",
    "coordinator": "ideas-coordinator",
    "agents": {"scout": {"herdr_name": "ideas-scout"}, "critic": {"herdr_name": "ideas-critic"}},
}


class EchoGuards(unittest.TestCase):
    def test_recognizes_the_relays_own_posts(self):
        roles = relay.roster(MANIFEST)
        mirror = relay.format_mirror({"from": "human", "to": "coordinator", "text": "hi"}, roles, "ideas")
        self.assertTrue(relay.is_relay_post(mirror))
        self.assertTrue(relay.is_relay_post(":bust_in_silhouette: *human → coordinator*: hi"))
        self.assertTrue(relay.is_relay_post(relay.format_catch_up(["a", "b"])))
        self.assertTrue(relay.is_relay_post(":crescent_moon: *Quiet mode ended: 13 updates while it was on*"))
        # Agent posts and people's messages still reach the bus.
        self.assertFalse(relay.is_relay_post(":fox_face: *[scout]* found 3"))
        self.assertFalse(relay.is_relay_post("@scout find 3 competitors"))
        self.assertFalse(relay.is_relay_post("*important* → read this"))

    def test_own_post_is_not_reinjected_when_its_ts_guard_is_lost(self):
        # The 2026-10-17 loop: the catch-up came back from Slack as a "human" message, was
        # mirrored, came back again, and nested ("human → coordinator: human → …").
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".swarm").mkdir()
            (root / ".swarm" / "agents.json").write_text(json.dumps(MANIFEST))
            bus = root / ".swarm" / "bus.jsonl"
            bus.write_text("")
            slack, hivey = FakeSlack(), FakeHivey(bus)
            r = relay.Relay(root, slack, hivey, "C1", "masters")
            r.state["last_ts"] = "0"
            slack.inbox.append({"ts": "1.000000", "bot_id": "B1",
                                "text": ":crescent_moon: *Quiet mode ended: 3 updates while it was on*"})
            slack.inbox.append({"ts": "2.000000", "bot_id": "B1",
                                "text": ":bust_in_silhouette: *human → coordinator*: back"})
            r.step()
            self.assertEqual(hivey.sent, [])
            self.assertEqual(slack.posted, [])

    def test_a_newer_relay_takes_over(self):
        with tempfile.TemporaryDirectory() as tmp:
            pid_path = Path(tmp) / "slack_relay.pid"
            still_ours = relay.claim(pid_path)
            self.assertTrue(still_ours())
            pid_path.write_text("999999")  # another relay started for this swarm
            self.assertFalse(still_ours())


class ParseSlack(unittest.TestCase):
    def test_unaddressed_human_message_goes_to_the_master(self):
        self.assertEqual(relay.parse_slack("how is it going?", MANIFEST),
                         ("human", "how is it going?", ["coordinator"]))

    def test_mentions_broadcast_and_master_aliases(self):
        _, _, targets = relay.parse_slack("@scout and @ideas-critic please, @master fyi", MANIFEST)
        self.assertEqual(targets, ["scout", "critic", "coordinator"])
        _, _, targets = relay.parse_slack("<!here> stop", MANIFEST)
        self.assertEqual(targets, ["@all"])

    def test_agent_posts_keep_their_sender_and_never_target_themselves(self):
        sender, body, targets = relay.parse_slack("🔭 *[scout]* done @scout @critic", MANIFEST)
        self.assertEqual((sender, body, targets), ("scout", "done @scout @critic", ["critic"]))
        # An agent's unaddressed chatter isn't forwarded to anyone.
        self.assertEqual(relay.parse_slack("*[scout]* thinking", MANIFEST)[2], [])


class Mirror(unittest.TestCase):
    roles = relay.roster(MANIFEST)

    def msg(self, frm, to, **extra):
        return {"ev": "msg", "id": "m1", "from": frm, "to": to, "swarm": "ideas", "text": "t", **extra}

    def test_masters_scope_keeps_master_and_human_traffic_only(self):
        keep = relay.should_mirror
        self.assertTrue(keep(self.msg("ideas/scout", "coordinator"), "masters", self.roles, set()))
        self.assertTrue(keep(self.msg("ideas/coordinator", "scout"), "masters", self.roles, set()))
        self.assertTrue(keep(self.msg("ideas/scout", "human"), "masters", self.roles, set()))
        self.assertFalse(keep(self.msg("ideas/scout", "critic"), "masters", self.roles, set()))
        self.assertTrue(keep(self.msg("ideas/scout", "critic"), "all", self.roles, set()))
        self.assertFalse(keep(self.msg("ideas/scout", "coordinator"), "off", self.roles, set()))

    def test_never_echoes_slack_messages_or_sender_copies(self):
        self.assertFalse(relay.should_mirror(self.msg("human", "coordinator"), "all", self.roles, {"m1"}))
        self.assertFalse(relay.should_mirror(self.msg("ideas/coordinator", "coordinator", copy=True),
                                             "all", self.roles, set()))

    def test_format(self):
        text = relay.format_mirror(self.msg("ideas/scout", "coordinator", kind="urgent"), self.roles, "ideas")
        self.assertTrue(text.endswith("*scout → coordinator* *(urgent)*: t"), text)
        other = relay.format_mirror(self.msg("ideas/coordinator", "coordinator", swarm="tonight"),
                                    self.roles, "ideas")
        self.assertIn("🧭 *coordinator → tonight/coordinator*", other)


class FakeSlack:
    def __init__(self):
        self.inbox, self.posted, self.next_ts = [], [], 100.0

    def history(self, channel, oldest):
        return [m for m in self.inbox if float(m["ts"]) > float(oldest)]

    def post(self, channel, text):
        self.next_ts += 1
        ts = f"{self.next_ts:.6f}"
        self.posted.append(text)
        self.inbox.append({"ts": ts, "text": text})  # our own post shows up in history
        return ts


class FakeHivey:
    def __init__(self, bus):
        self.bus, self.sent, self.n = bus, [], 0

    def send(self, target, text, sender):
        self.n += 1
        mid = f"m{self.n}"
        self.sent.append((target, text, sender))
        frm = "human" if sender == "human" else f"ideas/{sender}"
        with open(self.bus, "a") as f:
            f.write(json.dumps({"ev": "msg", "id": mid, "from": frm, "to": target, "swarm": "ideas",
                                "text": text}) + "\n")
        return [mid]


class RoundTrip(unittest.TestCase):
    def test_slack_to_bus_to_slack_without_echo(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / ".swarm").mkdir()
            (root / ".swarm" / "agents.json").write_text(json.dumps(MANIFEST))
            bus = root / ".swarm" / "bus.jsonl"
            bus.write_text("")
            slack, hivey = FakeSlack(), FakeHivey(bus)
            r = relay.Relay(root, slack, hivey, "C1", "masters")
            r.state["last_ts"] = "0"
            slack.inbox.append({"ts": "1.000000", "text": "@scout find 3 competitors"})
            r.step()
            self.assertEqual(hivey.sent, [("scout", "@scout find 3 competitors", "human")])
            self.assertEqual(slack.posted, [], "a message from Slack is not echoed back")
            # An agent answers the master on the bus: mirrored once, and the mirror post
            # coming back through history is not re-injected.
            with open(bus, "a") as f:
                f.write(json.dumps({"ev": "msg", "id": "a1", "from": "ideas/scout",
                                    "to": "coordinator", "swarm": "ideas", "text": "found 3"}) + "\n")
                f.write('{"ev": "msg", "id": "torn"')  # half-written line is left for later
            r.step()
            r.step()
            self.assertEqual(len(slack.posted), 1)
            self.assertIn("scout → coordinator", slack.posted[0])
            self.assertEqual(len(hivey.sent), 1)
            # State survives a restart of the relay.
            again = relay.Relay(root, slack, hivey, "C1", "masters")
            again.step()
            self.assertEqual(len(slack.posted), 1)
            self.assertEqual(len(hivey.sent), 1)


class QuietMode(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / ".swarm").mkdir()
        (self.root / ".swarm" / "agents.json").write_text(json.dumps(MANIFEST))
        self.bus = self.root / ".swarm" / "bus.jsonl"
        self.bus.write_text("")
        self.slack, self.hivey = FakeSlack(), FakeHivey(self.bus)
        self.quiet = True
        self.direct = []  # posts agents held through swarm_slack.py

        def held_posts(channel):
            taken, self.direct[:] = list(self.direct), []
            return taken

        self.relay = relay.Relay(self.root, self.slack, self.hivey, "C1", "masters",
                                 quiet=lambda: self.quiet, held_posts=held_posts)
        self.relay.state["last_ts"] = "0"

    def tearDown(self):
        self.tmp.cleanup()

    def agent_reports(self, text):
        with open(self.bus, "a") as f:
            f.write(json.dumps({"ev": "msg", "id": text, "from": "ideas/scout",
                                "to": "coordinator", "swarm": "ideas", "text": text}) + "\n")

    def test_holds_posts_then_sends_one_catch_up(self):
        self.slack.inbox.append({"ts": "1.000000", "text": "@scout keep going overnight"})
        self.agent_reports("found 3")
        self.relay.step()
        self.agent_reports("found 5")
        self.direct.append("*[scout]* posted directly")
        self.relay.step()
        self.assertEqual(self.slack.posted, [], "nothing reaches Slack in quiet mode")
        self.assertEqual(len(self.hivey.sent), 1, "Slack → bus keeps working")
        # Held posts survive a relay restart
        self.relay = relay.Relay(self.root, self.slack, self.hivey, "C1", "masters",
                                 quiet=lambda: self.quiet, held_posts=lambda _channel: [])
        self.quiet = False
        self.agent_reports("found 8")
        self.relay.step()
        self.assertEqual(len(self.slack.posted), 2)
        catch_up, after = self.slack.posted
        self.assertTrue(catch_up.startswith("🌙 *Quiet mode ended: 2 updates"))
        self.assertIn("found 3", catch_up)
        self.assertIn("found 5", catch_up)
        self.assertIn("found 8", after)
        self.relay.step()
        self.assertEqual(len(self.slack.posted), 2, "the catch-up is sent once")
        self.assertEqual(len(self.hivey.sent), 1, "our catch-up post is not injected back")

    def test_catch_up_includes_direct_posts(self):
        self.direct.append("*[scout]* posted directly")
        self.quiet = False
        self.relay.step()
        self.assertEqual(len(self.slack.posted), 1)
        self.assertIn("1 update while", self.slack.posted[0])
        self.assertIn("posted directly", self.slack.posted[0])

    def test_catch_up_is_bounded(self):
        message = relay.format_catch_up([f"line {n}" for n in range(100)])
        self.assertIn("100 updates", message)
        self.assertIn("last 40", message)
        self.assertNotIn("line 59\n", message)
        self.assertTrue(message.endswith("line 99"))


class HeldPostFiles(unittest.TestCase):
    def test_take_held_posts_claims_the_channel_file_once(self):
        held = relay.hivey_dir() / "quiet-held"
        held.mkdir(parents=True, exist_ok=True)
        (held / "C9.jsonl").write_text(json.dumps({"text": "*[a]* one"}) + "\nnot json\n"
                                       + json.dumps({"text": "*[b]* two"}) + "\n")
        self.assertEqual(relay.take_held_posts("C9"), ["*[a]* one", "*[b]* two"])
        self.assertEqual(relay.take_held_posts("C9"), [])
        self.assertEqual(list(held.iterdir()), [])


if __name__ == "__main__":
    unittest.main()
