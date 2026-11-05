"""Tests for create_channel.py's invites (no network: a fake Slack answers)."""
import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock

import create_channel


class FakeSlack:
    """Bot B shares #a and #b with the user U1, and #b with U2."""

    def __init__(self, in_channel=()):
        self.invited = []
        self.in_channel = set(in_channel)

    def call(self, method, http="POST", **params):
        if method == "auth.test":
            return {"user_id": "B"}
        if method == "conversations.list":
            return {"channels": [{"id": "Ca", "is_member": True}, {"id": "Cb", "is_member": True},
                                 {"id": "Cc", "is_member": False}]}
        if method == "conversations.members":
            return {"members": {"Ca": ["B", "U1"], "Cb": ["B", "U1", "U2"]}[params["channel"]]}
        if method == "conversations.invite":
            if params["users"] in self.in_channel:
                raise RuntimeError("slack conversations.invite: already_in_channel")
            self.invited.append(params["users"])
            return {"ok": True}
        raise AssertionError(method)


class InviteTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.env = mock.patch.dict(os.environ, {"HERDR_PLUGIN_CONFIG_DIR": self.dir.name})
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.dir.cleanup()

    def test_the_person_sharing_most_channels_is_invited_and_remembered(self):
        config = {"token_command": "x"}
        self.assertEqual(create_channel.invitees(FakeSlack(), config), ["U1"])
        saved = json.loads((Path(self.dir.name) / "config.json").read_text())
        self.assertEqual(saved["invite"], ["U1"])

    def test_configured_people_win(self):
        self.assertEqual(create_channel.invitees(FakeSlack(), {"invite": "U9"}), ["U9"])

    def test_invite_skips_people_already_there(self):
        slack = FakeSlack(in_channel={"U1"})
        self.assertEqual(create_channel.invite(slack, "Cnew", ["U1", "U2"]), ["U2"])
        self.assertEqual(slack.invited, ["U2"])


class CreateSlack:
    """conversations.create answers with `create_error`, else creates; #taken exists."""

    def __init__(self, create_error=None, taken=None):
        self.create_error, self.taken, self.calls = create_error, taken, []

    def call(self, method, http="POST", **params):
        self.calls.append((method, params))
        if method == "conversations.create":
            if self.create_error:
                raise RuntimeError(f"slack conversations.create: {self.create_error}")
            return {"channel": {"id": "Cnew", "is_private": params["is_private"]}}
        if method == "conversations.list":
            self.listed_types = params["types"]
            return {"channels": [self.taken] if self.taken else []}
        if method == "conversations.join":
            return {"ok": True}
        raise AssertionError(method)


class CreateTest(unittest.TestCase):
    def test_new_channels_are_private(self):
        slack = CreateSlack()
        channel, existing = create_channel.create(slack, "hivey")
        self.assertEqual(slack.calls[0], ("conversations.create", {"name": "hivey", "is_private": True}))
        self.assertTrue(channel["is_private"])
        self.assertFalse(existing)

    def test_missing_scope_says_how_to_add_it(self):
        with self.assertRaisesRegex(RuntimeError, "groups:write.*Reinstall"):
            create_channel.create(CreateSlack(create_error="missing_scope"), "hivey")

    def test_existing_private_channel_is_found_without_joining(self):
        slack = CreateSlack("name_taken", {"id": "Cp", "name": "hivey", "is_private": True, "is_member": True})
        channel, existing = create_channel.create(slack, "hivey")
        self.assertEqual((channel["id"], existing), ("Cp", True))
        self.assertIn("private_channel", slack.listed_types)
        self.assertNotIn("conversations.join", [m for m, _ in slack.calls])

    def test_existing_public_channel_is_joined(self):
        slack = CreateSlack("name_taken", {"id": "Cq", "name": "hivey", "is_private": False, "is_member": False})
        channel, existing = create_channel.create(slack, "hivey")
        self.assertIn(("conversations.join", {"channel": "Cq"}), slack.calls)

    def test_private_channel_the_bot_is_not_in_asks_for_an_invite(self):
        slack = CreateSlack("name_taken", {"id": "Cr", "name": "hivey", "is_private": True, "is_member": False})
        with self.assertRaisesRegex(RuntimeError, "invite @hivey"):
            create_channel.create(slack, "hivey")


if __name__ == "__main__":
    unittest.main()
