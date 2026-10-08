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

    def test_name_is_asked_with_a_suggestion_and_normalized(self):
        from unittest import mock
        cases = [([""], "build-a-todo-app-q"), (["Digital Marketer"], "digital-marketer"),
                 (["9lives", "x" * 40, "ok"], "ok")]
        for answers, expected in cases:
            replies = iter(answers)
            with mock.patch("builtins.input", lambda *_: next(replies)), \
                    mock.patch("builtins.print"):
                self.assertEqual(setup.ask_name("Build a TODO app, quickly!"), expected)

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

    def test_wiki_is_asked_without_asking_for_a_folder(self):
        from unittest import mock
        cases = [("ask", "n", False), ("ask", "", True), (True, None, True), (False, None, False)]
        for mode, answer, expected in cases:
            with mock.patch("builtins.input", lambda *_: answer):
                self.assertEqual(setup.want_wiki({"wiki": mode}), expected)

    def test_make_wiki_puts_the_vault_in_the_swarm_folder(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve() / "swarm-demo"
            agents = [root / name for name in ("builder", "critic")]
            for agent in agents:
                agent.mkdir(parents=True)
                (agent / "CLAUDE.md").write_text(f"# {agent.name}\n")
            from unittest import mock
            with mock.patch.dict(os.environ, {"HOME": tmp}):
                path = setup.make_wiki("demo", "a task", root, agents)
            self.assertEqual(path, str(root / "obsidian"))
            self.assertTrue(Path(path, "index.md").is_file())
            self.assertTrue(Path(path, ".obsidian").is_dir())
            for agent in agents:
                # relative, so the folder can be moved or shared as a whole
                self.assertIn("`../obsidian`", (agent / "CLAUDE.md").read_text())

    def test_briefs_name_the_team_and_hivey_messaging(self):
        brief = setup.BRIEF.format(agent="critic", slug="s", task="t", root="/r",
                                   job=setup.TEAM["critic"].format(root="/r"))
        self.assertIn("hivey msg send", brief)
        self.assertIn("`builder`", brief)


if __name__ == "__main__":
    unittest.main()
