"""Tests for hivey-swarm-guard.py: what a swarm worker may not open, and what stays open."""
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
SCRIPT = HERE / "hivey-swarm-guard.py"
SPEC = importlib.util.spec_from_file_location("hivey_swarm_guard", SCRIPT)
guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(guard)


class SwarmGuard(unittest.TestCase):
    def setUp(self):
        # Under /tmp on purpose: on macOS it is a symlink to /private/tmp.
        self.root = Path(tempfile.mkdtemp(prefix="hivey-guard-", dir="/tmp"))
        manifest = {
            "slug": "demo",
            "agents": {
                "coord": {"role": "master"},
                "builder": {"role": "worker"},
                "critic": {"role": "critic"},
                "relay": {"command": "relay.sh"},
            },
        }
        (self.root / ".swarm" / "inbox").mkdir(parents=True)
        (self.root / ".swarm" / "coordinator").mkdir()
        (self.root / ".swarm" / "agents.json").write_text(json.dumps(manifest))
        (self.root / ".swarm" / "TASKS.md").write_text("tasks")
        (self.root / ".swarm" / "inbox" / "builder-1.md").write_text("mine")
        (self.root / ".swarm" / "inbox" / "critic-1.md").write_text("theirs")
        for agent in ("builder", "critic"):
            (self.root / agent / "out").mkdir(parents=True)
            (self.root / agent / "notes.md").write_text(f"{agent}'s plan")
        (self.root / "builder" / "out" / "api.md").write_text("deliverable")
        (self.root / "shared").mkdir()
        self.bus(
            {"ev": "msg", "to": "critic", "swarm": "demo", "from": "demo/builder",
             "text": "DONE: please review builder/out/api.md"},
            {"ev": "msg", "to": "critic", "swarm": "demo", "from": "demo/coord",
             "text": "Team: builder/ builds, you review. Root: " + str(self.root) + "/"},
        )

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def bus(self, *records):
        with open(self.root / ".swarm" / "bus.jsonl", "a") as log:
            for record in records:
                log.write(json.dumps(record) + "\n")

    def swarm_for(self, agent):
        swarm = guard.find_swarm([str(self.root / agent)])
        if swarm:
            swarm.load_handoffs()
        return swarm

    def check(self, agent, tool, data, cwd=None):
        swarm = self.swarm_for(agent)
        self.assertIsNotNone(swarm, agent)
        return guard.check(swarm, tool, data, str(cwd or self.root / agent))

    def bash(self, agent, command, cwd=None):
        return self.check(agent, "Bash", {"command": command}, cwd)

    # --- who is guarded -------------------------------------------------------------------

    def test_master_scripts_and_outsiders_are_not_guarded(self):
        self.assertIsNone(guard.find_swarm([str(self.root)]))
        self.assertIsNone(guard.find_swarm([str(self.root / "shared")]))
        self.assertIsNone(guard.find_swarm([tempfile.gettempdir()]))
        (self.root / "relay").mkdir()
        self.assertIsNone(guard.find_swarm([str(self.root / "relay")]))

    def test_solo_and_opted_out_swarms_are_not_guarded(self):
        manifest_path = self.root / ".swarm" / "agents.json"
        manifest = json.loads(manifest_path.read_text())
        for flag in ({"solo": True}, {"isolation": False}):
            manifest_path.write_text(json.dumps({**manifest, **flag}))
            self.assertIsNone(guard.find_swarm([str(self.root / "builder")]), flag)

    def test_the_agent_is_found_from_where_it_started(self):
        # CLAUDE_PROJECT_DIR comes first, so cd .. doesn't turn a worker into the master.
        swarm = guard.find_swarm([str(self.root / "builder"), str(self.root)])
        self.assertEqual(swarm.me, "builder")
        self.assertIsNotNone(guard.check(swarm, "Read", {"file_path": "critic/notes.md"}, str(self.root)))

    # --- blocked --------------------------------------------------------------------------

    def test_file_tools_on_another_home(self):
        for tool in ("Read", "Edit", "Write", "MultiEdit"):
            self.assertIsNotNone(self.check("builder", tool, {"file_path": "../critic/notes.md"}), tool)
        self.assertIsNotNone(self.check("builder", "Read", {"file_path": str(self.root / "critic" / "notes.md")}))
        real = os.path.realpath(self.root / "critic" / "notes.md")
        self.assertIsNotNone(self.check("builder", "Read", {"file_path": real}))
        self.assertIsNotNone(self.check("critic", "Read", {"file_path": "../builder/notes.md"}))

    def test_private_swarm_files(self):
        for path in (".swarm/coordinator/plan.md", ".swarm/bus.jsonl", ".swarm/inbox/critic-1.md"):
            self.assertIsNotNone(self.check("builder", "Read", {"file_path": str(self.root / path)}), path)

    def test_bash_reads(self):
        for command in [
            "cat ../critic/notes.md",
            "cd ../critic && ls",
            "head -5 ../*/notes.md",
            "python3 -c \"print(open('../critic/notes.md').read())\"",
            f"less {self.root}/critic/notes.md",
            "cp ../critic/notes.md mine.md",
            "grep -n plan ../critic/notes.md",
            "tail ../.swarm/bus.jsonl",
            "echo hi; cat ../critic/notes.md",
        ]:
            self.assertIsNotNone(self.bash("builder", command), command)

    def test_searches_that_reach_other_homes(self):
        self.assertIsNotNone(self.check("builder", "Grep", {"pattern": "plan", "path": str(self.root)}))
        self.assertIsNotNone(self.check("builder", "Grep", {"pattern": "plan", "path": ".."}))
        self.assertIsNotNone(self.check("builder", "Glob", {"pattern": "../**/*.md"}))
        self.assertIsNotNone(self.check("builder", "Glob", {"pattern": "**/*.md", "path": str(self.root)}))
        for command in ["grep -rn plan ..", "rg plan ..", "find .. -name '*.md'", "ls -R ..", "tree .."]:
            self.assertIsNotNone(self.bash("builder", command), command)
        # Run from the root (after a cd), a bare search walks the root.
        self.assertIsNotNone(self.bash("builder", "rg plan", cwd=self.root))

    # --- allowed --------------------------------------------------------------------------

    def test_own_home_and_shared_parts(self):
        for path in ("notes.md", "out/x.md", "../shared/a.md", "../.swarm/TASKS.md",
                     "../.swarm/inbox/builder-1.md", "../README.md"):
            self.assertIsNone(self.check("builder", "Read", {"file_path": path}), path)
        self.assertIsNone(self.check("builder", "Grep", {"pattern": "x"}))
        self.assertIsNone(self.check("builder", "Glob", {"pattern": "**/*.md"}))
        self.assertIsNone(self.check("builder", "Grep", {"pattern": "x", "path": "../shared"}))
        for command in [
            "ls", "ls ..", "cat notes.md", "rg TODO", "grep -rn x out", "find . -name '*.md'",
            "cat ../.swarm/TASKS.md", "ls -lr ..", "git status", "cargo test",
            "hivey msg send critic \"DONE: see builder/out/api.md, unlike critic/notes.md\"",
            "echo critic", "mkdir -p out/../tmp",
        ]:
            self.assertIsNone(self.bash("builder", command), command)

    def test_handoffs_sent_to_the_agent(self):
        self.assertIsNone(self.check("critic", "Read", {"file_path": "../builder/out/api.md"}))
        self.assertIsNone(self.bash("critic", "cat ../builder/out/api.md"))
        # Only what was handed off: the rest of the home stays closed, also when a message
        # names the whole home or the root.
        self.assertIsNotNone(self.check("critic", "Read", {"file_path": "../builder/notes.md"}))
        self.assertIsNotNone(self.check("critic", "Grep", {"pattern": "x", "path": "../builder"}))
        # Hand-offs to someone else don't count.
        self.assertIsNotNone(self.check("builder", "Read", {"file_path": "../critic/out/x.md"}))
        self.bus({"ev": "msg", "to": "builder", "swarm": "demo", "from": "demo/critic",
                  "text": f"Review notes in {self.root}/critic/out/"})
        self.assertIsNone(self.check("builder", "Read", {"file_path": "../critic/out/review.md"}))

    # --- the hook as Claude Code runs it ----------------------------------------------------

    def run_hook(self, event, env_extra=None, project=None):
        env = {**os.environ, "CLAUDE_PROJECT_DIR": str(project or self.root / "builder")}
        env.pop("HIVEY_ISOLATION", None)
        env.update(env_extra or {})
        return subprocess.run(
            [sys.executable, str(SCRIPT)], input=json.dumps(event), capture_output=True,
            text=True, env=env,
        )

    def test_hook_exit_codes(self):
        event = {"tool_name": "Read", "tool_input": {"file_path": "../critic/notes.md"},
                 "cwd": str(self.root / "builder")}
        blocked = self.run_hook(event)
        self.assertEqual(blocked.returncode, 2, blocked.stderr)
        self.assertIn("critic's folder", blocked.stderr)
        self.assertIn("hivey msg send", blocked.stderr)
        self.assertEqual(self.run_hook(event, {"HIVEY_ISOLATION": "off"}).returncode, 0)
        self.assertEqual(self.run_hook(event, project=self.root).returncode, 0, "master")
        ok = dict(event, tool_input={"file_path": "notes.md"})
        self.assertEqual(self.run_hook(ok).returncode, 0)
        self.assertEqual(
            subprocess.run([sys.executable, str(SCRIPT)], input="not json", capture_output=True, text=True).returncode, 0
        )


if __name__ == "__main__":
    unittest.main()
