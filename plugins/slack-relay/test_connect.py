"""Tests for connect.py (no network: Slack calls are stubbed)."""
import io
import json
import os
import stat
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from unittest import mock

import connect


class ConnectTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.env = mock.patch.dict(os.environ, {"HERDR_PLUGIN_CONFIG_DIR": self.dir.name})
        self.env.start()
        os.environ.pop("SLACK_TOKEN", None)

    def tearDown(self):
        self.env.stop()
        self.dir.cleanup()

    def test_save_token_is_private_and_keeps_other_settings(self):
        config = Path(self.dir.name) / "config.json"
        config.write_text(json.dumps({"mirror": "all", "token_command": "old"}))
        token_file = connect.save_token("xoxb-secret")
        self.assertEqual(stat.S_IMODE(token_file.stat().st_mode), 0o600)
        self.assertEqual(token_file.read_text().strip(), "xoxb-secret")
        saved = json.loads(config.read_text())
        self.assertEqual(saved["mirror"], "all")
        self.assertEqual(saved["token_command"], f"cat '{token_file}'")
        self.assertNotIn("xoxb-secret", config.read_text())

    def test_not_connected_without_a_token(self):
        state = connect.current()
        self.assertFalse(state["connected"])
        self.assertIn("not connected", connect.describe(state))

    def test_connected_reports_missing_scopes(self):
        connect.save_token("xoxb-ok")
        reply = ({"ok": True, "team": "Acme", "user": "hiver"}, {"chat:write"})
        with mock.patch.object(connect, "auth_test", return_value=reply):
            state = connect.current()
        self.assertTrue(state["connected"])
        self.assertIn("channels:manage", state["missing_scopes"])
        self.assertIn("missing scopes", connect.describe(state))

    def test_refused_token_is_not_connected(self):
        connect.save_token("xoxb-revoked")
        with mock.patch.object(connect, "auth_test", side_effect=RuntimeError("token_revoked")):
            state = connect.current()
        self.assertFalse(state["connected"])
        self.assertIn("token_revoked", state["error"])

    def test_connect_needs_a_terminal(self):
        err = io.StringIO()
        with mock.patch("sys.stdin", io.StringIO("")), redirect_stderr(err):
            self.assertEqual(connect.connect(force=False), 1)
        self.assertIn("never paste it into a chat", err.getvalue())


if __name__ == "__main__":
    unittest.main()
