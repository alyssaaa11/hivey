"""Tests for the hiver Slack relay: python3 -m unittest plugins/slack-relay/test_relay.py"""
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import relay  # noqa: E402

MANIFEST = {
    "slug": "ideas",
    "coordinator": "ideas-coordinator",
    "agents": {"scout": {"herdr_name": "ideas-scout"}, "critic": {"herdr_name": "ideas-critic"}},
}


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


class FakeHiver:
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
            slack, hiver = FakeSlack(), FakeHiver(bus)
            r = relay.Relay(root, slack, hiver, "C1", "masters")
            r.state["last_ts"] = "0"
            slack.inbox.append({"ts": "1.000000", "text": "@scout find 3 competitors"})
            r.step()
            self.assertEqual(hiver.sent, [("scout", "@scout find 3 competitors", "human")])
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
            self.assertEqual(len(hiver.sent), 1)
            # State survives a restart of the relay.
            again = relay.Relay(root, slack, hiver, "C1", "masters")
            again.step()
            self.assertEqual(len(slack.posted), 1)
            self.assertEqual(len(hiver.sent), 1)


if __name__ == "__main__":
    unittest.main()
