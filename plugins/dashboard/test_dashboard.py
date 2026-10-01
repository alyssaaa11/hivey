"""Tests for the hiver dashboard: python3 -m unittest plugins/dashboard/test_dashboard.py"""
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import dashboard  # noqa: E402


class Helpers(unittest.TestCase):
    def test_human_and_bar_and_sparkline(self):
        self.assertEqual([dashboard.human(n) for n in (12, 4200, 3_400_000)], ["12", "4k", "3.4M"])
        self.assertEqual(dashboard.bar(5, 10, 4), "██░░")
        self.assertEqual(dashboard.bar(0, 0, 3), "░░░")
        self.assertEqual(dashboard.bar(99, 10, 2), "██")
        self.assertEqual(dashboard.sparkline([0, 7, 14], 10), "▁▄█")
        self.assertEqual(dashboard.sparkline([], 10), "")

    def test_duration_and_task_counts(self):
        self.assertEqual([dashboard.duration(s) for s in (5, 125, 3725)], ["5s", "2m", "1h02"])
        counts = dashboard.task_counts([{"status": "open"}, {"status": "approved"}, {}])
        self.assertEqual((counts["open"], counts["approved"], counts["blocked"]), (2, 1, 0))


class Transcripts(unittest.TestCase):
    def test_usage_is_summed_incrementally_and_deduplicated_by_message_id(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "s.jsonl"

            def line(mid, out):
                usage = {"input_tokens": 10, "output_tokens": out, "cache_read_input_tokens": 100}
                return json.dumps({"message": {"id": mid, "usage": usage}}) + "\n"

            # Claude Code logs one line per streamed block, all with the same usage.
            path.write_text(line("a", 5) + line("a", 5) + '{"type": "user"}\n')
            state = {}
            self.assertEqual(dashboard.scan_transcript(str(path), state), 115)
            with open(path, "a") as f:
                f.write(line("b", 20))
                f.write('{"message": {"id": "c", "usa')  # still being written
            self.assertEqual(dashboard.scan_transcript(str(path), state), 115 + 130)
            self.assertEqual(dashboard.scan_transcript(str(path) + ".missing", {}), 0)

    def test_codex_rollout_keeps_the_latest_running_total(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "rollout-x.jsonl"

            def event(total):
                info = {"total_token_usage": {"total_tokens": total}}
                return json.dumps({"type": "event_msg", "payload": {"type": "token_count", "info": info}}) + "\n"

            path.write_text('{"type": "session_meta", "payload": {}}\n' + event(100) + event(250))
            state = {}
            self.assertEqual(dashboard.scan_codex_rollout(str(path), state), 250)
            with open(path, "a") as f:
                f.write(event(400))
            self.assertEqual(dashboard.scan_codex_rollout(str(path), state), 400)

    def test_recent_messages_skip_receipts_and_copies(self):
        with tempfile.TemporaryDirectory() as tmp:
            bus = Path(tmp) / "bus.jsonl"
            records = [
                {"ev": "msg", "id": "1", "text": "a"},
                {"ev": "delivered", "id": "1"},
                {"ev": "msg", "id": "2", "text": "b", "copy": True},
                {"ev": "msg", "id": "3", "text": "c"},
            ]
            bus.write_text("\n".join(json.dumps(r) for r in records) + "\n{torn")
            self.assertEqual([m["id"] for m in dashboard.recent_messages(bus, 10)], ["1", "3"])
            self.assertEqual(dashboard.recent_messages(Path(tmp) / "none", 5), [])


class Compact(unittest.TestCase):
    def test_short_pane_shows_tokens_per_agent_without_scripts(self):
        snap = {
            "swarm": {"agents": [
                {"key": "coordinator", "role": "master"},
                {"key": "scout", "role": "worker"},
                {"key": "dashboard", "role": "script"},
            ]},
            "manifest": {"launched_at": 1000, "budget_minutes": 60},
            "tokens": {"coordinator": 2_800_000, "scout": 1_900_000, "dashboard": 0},
            "now": 1000 + 540,
        }
        line = "".join(text for text, _, _ in dashboard.compact_parts(snap, "news"))
        self.assertIn("coordinator 2.8M", line)
        self.assertIn("scout 1.9M", line)
        self.assertIn("15%", line)
        self.assertIn("tokens 4.7M", line)
        self.assertNotIn("dashboard", line)


if __name__ == "__main__":
    unittest.main()
