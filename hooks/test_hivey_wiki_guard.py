"""Tests for hivey-wiki-guard.py: secrets stay out of wiki vaults, and page leases keep two
agents from overwriting each other."""
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "hivey-wiki-guard.py"
SPEC = importlib.util.spec_from_file_location("hivey_wiki_guard", SCRIPT)
wiki = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(wiki)

# Built at run time so this file itself never holds a key-shaped string.
FAKE_KEY = "sk-ant-" + "a1B2c3D4" * 4


class WikiGuard(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="hivey-wiki-", dir="/tmp"))
        self.vault = self.root / "demo-wiki"
        (self.vault / "wiki").mkdir(parents=True)
        (self.vault / "raw").mkdir()
        (self.vault / "CLAUDE.md").write_text("rules")
        (self.vault / "wiki" / "page.md").write_text("page")
        (self.root / "notes").mkdir()
        self.home = Path(tempfile.mkdtemp(prefix="hivey-home-", dir="/tmp"))

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)
        shutil.rmtree(self.home, ignore_errors=True)

    def event(self, tool, data, session="s1", cwd=None):
        return {"tool_name": tool, "tool_input": data, "session_id": session,
                "cwd": str(cwd or self.root)}

    def page(self):
        return str(self.vault / "wiki" / "page.md")

    def run_hook(self, event, env_extra=None):
        env = {**os.environ, "HOME": str(self.home), "CLAUDE_PROJECT_DIR": str(self.root)}
        for name in ("HIVEY_SAFETY", "HIVEY_WIKI_LOCK"):
            env.pop(name, None)
        env.update(env_extra or {})
        return subprocess.run([sys.executable, str(SCRIPT)], input=json.dumps(event),
                              capture_output=True, text=True, env=env)

    # --- finding vaults ----------------------------------------------------------------

    def test_vaults_are_found_from_any_depth_and_nothing_else_is(self):
        real = os.path.realpath(self.vault)
        self.assertEqual(wiki.find_vault(os.path.realpath(self.page())), real)
        self.assertEqual(wiki.find_vault(os.path.join(real, "wiki", "new", "deep.md")), real)
        self.assertIsNone(wiki.find_vault(os.path.realpath(self.root / "notes" / "a.md")))

    # --- secrets -----------------------------------------------------------------------

    def test_secrets_are_blocked_in_every_write_tool(self):
        for tool, data in [
            ("Write", {"file_path": self.page(), "content": f"key: {FAKE_KEY}"}),
            ("Edit", {"file_path": self.page(), "old_string": "page", "new_string": FAKE_KEY}),
            ("MultiEdit", {"file_path": self.page(), "edits": [{"new_string": FAKE_KEY}]}),
            ("Bash", {"command": f"echo {FAKE_KEY} >> demo-wiki/log.md"}),
        ]:
            result = self.run_hook(self.event(tool, data))
            self.assertEqual(result.returncode, 2, tool)
            self.assertIn("envsave", result.stderr)

    def test_placeholders_and_files_outside_vaults_pass(self):
        ok = [
            self.event("Write", {"file_path": self.page(), "content": "key: es_RQ_abc123 (envsave)"}),
            self.event("Write", {"file_path": self.page(), "content": "api_key: ANTHROPIC_API_KEY"}),
            self.event("Write", {"file_path": str(self.root / "notes" / "a.md"), "content": FAKE_KEY}),
            self.event("Bash", {"command": f"echo {FAKE_KEY} > notes/a.md"}),
        ]
        for event in ok:
            self.assertEqual(self.run_hook(event).returncode, 0, event)

    def test_the_safety_switch_turns_the_scan_off(self):
        event = self.event("Write", {"file_path": self.page(), "content": FAKE_KEY})
        self.assertEqual(self.run_hook(event, {"HIVEY_SAFETY": "off"}).returncode, 0)
        (self.home / ".hivey").mkdir()
        (self.home / ".hivey" / "safety.json").write_text('{"enabled": false}')
        self.assertEqual(self.run_hook(event, {"HIVEY_WIKI_LOCK": "off"}).returncode, 0)

    # --- page leases -------------------------------------------------------------------

    def test_a_second_session_waits_for_the_lease(self):
        write = {"file_path": self.page(), "content": "new"}
        self.assertEqual(self.run_hook(self.event("Write", write, "s1")).returncode, 0)
        self.assertEqual(self.run_hook(self.event("Write", write, "s1")).returncode, 0, "same session")
        other = self.run_hook(self.event("Write", write, "s2"))
        self.assertEqual(other.returncode, 2)
        self.assertIn("re-read the page", other.stderr)
        # Another page is free.
        free = {"file_path": str(self.vault / "wiki" / "other.md"), "content": "x"}
        self.assertEqual(self.run_hook(self.event("Write", free, "s2")).returncode, 0)
        self.assertEqual(self.run_hook(self.event("Write", write, "s2"), {"HIVEY_WIKI_LOCK": "off"}).returncode, 0)

    def test_leases_expire(self):
        vault, page = os.path.realpath(self.vault), os.path.realpath(self.page())
        self.assertIsNone(wiki.take_lease(vault, page, "s1", "coord", now=1000))
        self.assertIsNotNone(wiki.take_lease(vault, page, "s2", "solo", now=1000 + wiki.LEASE - 1))
        self.assertIsNone(wiki.take_lease(vault, page, "s2", "solo", now=1000 + wiki.LEASE + 1))
        holder = json.loads(Path(wiki.lock_file(vault, page)).read_text())
        self.assertEqual(holder["session"], "s2")

    def test_bad_input_never_blocks(self):
        result = subprocess.run([sys.executable, str(SCRIPT)], input="not json",
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0)
        self.assertEqual(self.run_hook(self.event("Read", {"file_path": self.page()})).returncode, 0)


if __name__ == "__main__":
    unittest.main()
