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

    def test_wiki_is_asked_and_the_obsidian_folder_once(self):
        from unittest import mock
        with tempfile.TemporaryDirectory() as tmp:
            settings = Path(tmp, "wiki.json")
            default = str(Path.home() / "Obsidian")
            cases = [("ask", ["n"], None), ("ask", ["", ""], default),
                     ("ask", ["", "/x/Obs"], "/x/Obs"), (False, [], None)]
            with mock.patch.object(setup, "WIKI_SETTINGS", settings):
                for mode, answers, expected in cases:
                    replies = iter(answers)
                    with mock.patch("builtins.input", lambda *_: next(replies)):
                        self.assertEqual(setup.want_wiki({"wiki": mode}, "demo"), expected)
                # Once the folder is known it isn't asked again
                settings.write_text(json.dumps({"dir": "/remembered"}))
                with mock.patch("builtins.input", lambda *_: ""):
                    self.assertEqual(setup.want_wiki({"wiki": "ask"}, "demo"), "/remembered")

    def test_make_wiki_creates_the_vault_and_links_the_team(self):
        with tempfile.TemporaryDirectory() as tmp:
            agents = [Path(tmp, name) for name in ("builder", "critic")]
            for agent in agents:
                agent.mkdir()
                (agent / "CLAUDE.md").write_text(f"# {agent.name}\n")
            # new_wiki.py remembers the folder in $HOME/.hivey: keep that in the scratch dir
            from unittest import mock
            with mock.patch.dict(os.environ, {"HOME": tmp}):
                path = setup.make_wiki("demo", "a task", str(Path(tmp, "Obsidian")), agents)
            self.assertTrue(Path(tmp, ".hivey", "wiki.json").is_file())
            self.assertEqual(path, str(Path(tmp, "Obsidian", "demo-wiki")))
            self.assertTrue(Path(path, "index.md").is_file())
            for agent in agents:
                self.assertIn(path, (agent / "CLAUDE.md").read_text())

    def test_briefs_name_the_team_and_hivey_messaging(self):
        brief = setup.BRIEF.format(agent="critic", slug="s", task="t", root="/r",
                                   job=setup.TEAM["critic"].format(root="/r"))
        self.assertIn("hivey msg send", brief)
        self.assertIn("`builder`", brief)


if __name__ == "__main__":
    unittest.main()
