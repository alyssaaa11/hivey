"""Tests for hivey-task-log.py: prompt and stop lines for the agent fleet's Tasks board."""
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
SCRIPT = HERE / "hivey-task-log.py"
SPEC = importlib.util.spec_from_file_location("hivey_task_log", SCRIPT)
log = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(log)


class TaskLog(unittest.TestCase):
    def test_your_prompt_is_from_you_with_your_words(self):
        line = json.loads(log.line_for({"hook_event_name": "UserPromptSubmit", "prompt": "fix the\nnightly jobs",
                                        "session_id": "s1", "cwd": "/x"}, "T"))
        self.assertEqual(line["event"], "prompt")
        self.assertEqual(line["from"], "you")
        self.assertEqual(line["note"], "fix the nightly jobs")
        self.assertEqual(line["session"], "s1")
        self.assertEqual(line["by"], "hook")

    def test_bus_messages_and_slack_messages(self):
        self.assertEqual(log.prompt_from("[hivey] from ceo: check the KPIs"), ("bus", ""))
        self.assertEqual(log.prompt_from("[hivey] from human (slack): ship it\nmore"), ("you", "ship it"))
        self.assertEqual(log.prompt_from("[Request interrupted by user]")[0], "other")

    def test_stop_keeps_the_done_words_and_skips_a_hook_loop(self):
        line = json.loads(log.line_for({"hook_event_name": "Stop", "last_assistant_message": "All good.\nDONE: `jobs fixed`"}, "T"))
        self.assertEqual((line["event"], line["note"]), ("stop", "jobs fixed"))
        self.assertIsNone(log.line_for({"hook_event_name": "Stop", "stop_hook_active": True}, "T"))
        self.assertIsNone(log.line_for({"hook_event_name": "PreToolUse"}, "T"))

    def test_the_script_appends_to_the_home_log(self):
        home = Path(tempfile.mkdtemp(prefix="hivey-tasklog-", dir="/tmp"))
        try:
            env = {**os.environ, "HOME": str(home), "HERDR_PANE_ID": "w1-2"}
            event = json.dumps({"hook_event_name": "UserPromptSubmit", "prompt": "hello"})
            for _ in range(2):
                subprocess.run([sys.executable, str(SCRIPT)], input=event, text=True, env=env, check=True)
            lines = (home / ".hivey" / "tasks.jsonl").read_text().splitlines()
            self.assertEqual(len(lines), 2)
            self.assertEqual(json.loads(lines[0])["pane"], "w1-2")
        finally:
            shutil.rmtree(home)


if __name__ == "__main__":
    unittest.main()
