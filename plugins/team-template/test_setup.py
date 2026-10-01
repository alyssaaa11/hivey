"""Tests for the example provider: python3 -m unittest plugins/team-template/test_setup.py"""
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import setup  # noqa: E402


class TemplateTests(unittest.TestCase):
    def test_slug_is_short_and_safe(self):
        self.assertEqual(setup.slugify("Build a TODO app, quickly!"), "build-a-todo-app-q")
        self.assertEqual(setup.slugify("¿?"), "swarm")
        self.assertLessEqual(len(setup.slugify("x" * 50)), 18)

    def test_config_overrides_defaults(self):
        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, "config.json").write_text(json.dumps({"model": "haiku", "heartbeat": ""}))
            os.environ["HERDR_PLUGIN_CONFIG_DIR"] = tmp
            try:
                cfg = setup.config()
            finally:
                del os.environ["HERDR_PLUGIN_CONFIG_DIR"]
        self.assertEqual(cfg["model"], "haiku")
        self.assertEqual(cfg["heartbeat"], "")
        self.assertEqual(cfg["master_model"], "opus")

    def test_briefs_name_the_team_and_hiver_messaging(self):
        brief = setup.BRIEF.format(agent="critic", slug="s", task="t", root="/r",
                                   job=setup.TEAM["critic"].format(root="/r"))
        self.assertIn("hiver msg send", brief)
        self.assertIn("`builder`", brief)


if __name__ == "__main__":
    unittest.main()
