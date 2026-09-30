"""Tests for the hiver GitHub addon: python3 -m unittest plugins/github/test_github.py"""
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import github  # noqa: E402

CONFIG = dict(github.DEFAULT_CONFIG)


class Planning(unittest.TestCase):
    def test_product_dir_from_manifest_config_or_a_nested_repo(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scout").mkdir()
            (root / "site").mkdir()
            self.assertIsNone(github.find_product(root, {"agents": {"scout": {}}}, CONFIG))
            subprocess.run(["git", "init", "-q", str(root / "site")], check=True)
            self.assertEqual(github.find_product(root, {"agents": {"scout": {}}}, CONFIG), "site")
            (root / "app").mkdir()
            self.assertEqual(github.find_product(root, {}, CONFIG), "app")
            self.assertEqual(github.find_product(root, {"product_dir": "site/"}, CONFIG), "site")

    def test_agent_worktrees_are_not_mistaken_for_the_product(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            subprocess.run(["git", "init", "-q", str(root / "builder")], check=True)
            self.assertIsNone(github.find_product(root, {"agents": {"builder": {}}}, CONFIG))

    def test_workspace_gitignore_excludes_product_worktrees_and_secrets_once(self):
        text = github.workspace_gitignore("node_modules/", "app")
        self.assertTrue(text.startswith("node_modules/\n\n"))
        for line in ("/app/", "/*/repo/", ".env", ".env.*", ".swarm/*.lock"):
            self.assertIn(line + "\n", text)
        self.assertEqual(github.workspace_gitignore(text, "app"), text, "idempotent")
        self.assertNotIn("{product}", github.workspace_gitignore("", ""))
        self.assertNotIn("\n//\n", github.workspace_gitignore("", ""))

    def test_repo_names(self):
        self.assertEqual(github.repo_names("habit", CONFIG), ("habit", "habit-swarm"))
        custom = dict(CONFIG, product_repo="{slug}-app", swarm_repo="agents-{slug}")
        self.assertEqual(github.repo_names("habit", custom), ("habit-app", "agents-habit"))


if __name__ == "__main__":
    unittest.main()
