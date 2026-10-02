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


if __name__ == "__main__":
    unittest.main()
