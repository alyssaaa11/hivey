"""Tests for hivey evolve's importance score and learning checks. Run: python3 -m unittest discover -s tests"""
import datetime as dt
import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load(home: str):
    os.environ["HIVEY_EVOLVE_HOME"] = home
    spec = importlib.util.spec_from_file_location(f"evolve_{abs(hash(home))}", HERE.parent / "scripts" / "evolve.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def pattern(pid, sig, count, agents):
    return {"id": pid, "signature": sig, "count": count, "agents": agents, "sessions": 1,
            "examples": [{"ts": "2026-11-19T10:00:00", "agent": agents[0], "tool": "Bash", "command": "x",
                          "error": f"fatal: {sig}"}]}


OPS = {"jobs_failing": [], "claude": {"duplicates": [], "folders": []}, "cost_usd": 10, "live_agents": 5}


class Base(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.ev = load(self.tmp.name)
        self.ev.REVIEWS.mkdir(parents=True, exist_ok=True)
        self.ev.FACTS.mkdir(parents=True, exist_ok=True)

    def tearDown(self):
        self.tmp.cleanup()


class ScoreTests(Base):
    def test_formula(self):
        self.assertEqual(self.ev.importance(1, 1, 1, 1), 100)
        self.assertEqual(self.ev.importance(0, 0, 0, 0), 0)
        self.assertEqual(self.ev.importance(1, 0, 0, 0), 35)
        self.assertEqual(self.ev.importance(0.5, 0.5, 0.2, 0.1), round(100 * (0.175 + 0.125 + 0.05 + 0.015)))

    def test_reach_and_frequency(self):
        self.assertEqual(self.ev.reach_score(1), 0.2)
        self.assertEqual(self.ev.reach_score(5), 1.0)
        self.assertEqual(self.ev.reach_score(9), 1.0)
        self.assertEqual(self.ev.frequency_score(20, 100), 1.0)
        self.assertEqual(self.ev.frequency_score(10, 100), 0.5)
        self.assertEqual(self.ev.frequency_score(50, 100), 1.0)

    def test_orders_by_score_and_flags_guesses(self):
        items = [{"title": "low", "pattern_ids": ["P1"]}, {"title": "high", "pattern_ids": ["P2"]}]
        pats = [pattern("P1", "a", 3, ["x"]), pattern("P2", "b", 3, ["x", "y", "z", "w", "v"])]

        def ask(state, q):
            harm = 0.9 if "fatal: b" in state["evidence"] else 0.1
            return {"answers": {"harm": {"noul": harm}, "blocking": {"noul": 0.5}, "certainty": {"noul": 0.3}}}
        out, note = self.ev.score_items(items, pats, OPS, [], 10, ask)
        self.assertEqual([i["title"] for i in out], ["high", "low"])
        self.assertEqual(out[0]["model_rank"], 2)
        self.assertTrue(out[0]["check_first"])
        for k in ("harm", "blocking", "certainty", "reach", "frequency", "score"):
            self.assertIn(k, out[0]["importance"])
        self.assertIn("local model", note)

    def test_fallback_without_local_model(self):
        items = [{"title": "a"}, {"title": "b"}]
        out, note = self.ev.score_items(items, [], OPS, [], 10, None)
        self.assertEqual([i["title"] for i in out], ["a", "b"])
        self.assertIsNone(out[0]["importance"])
        self.assertEqual(note, self.ev.NOT_SCORED)

    def test_fallback_when_every_call_fails(self):
        def ask(state, q):
            raise RuntimeError("server down")
        out, note = self.ev.score_items([{"title": "a"}, {"title": "b"}], [], OPS, [], 10, ask)
        self.assertEqual([i["title"] for i in out], ["a", "b"])
        self.assertEqual(note, self.ev.NOT_SCORED)


class RateTests(Base):
    def test_fix_checks_use_rates(self):
        ev = self.ev
        old = {"date": "2026-11-12", "patterns": [pattern("P1", "sig-a", 10, ["x"])],
               "metrics": {"sessions": 50, "signature_rates": {"sig-a": 20.0}, "cost_per_agent": 3.0},
               "text": {"items": [{"title": "fix a", "pattern_ids": ["P1"]}]}}
        ev.write_json(ev.REVIEWS / "2026-11-12.json", old)
        ev.write_json(ev.DECISIONS, {"2026-11-12": {"1": {"choice": "yes", "ts": "x"}}})
        # same count (10) but 4x the sessions: the rate drops, a count would not
        now_m = {"sessions": 200, "signature_rates": {"sig-a": 5.0, "sig-new": 2.0}, "cost_per_agent": 2.0}
        c = ev.fix_checks(now_m)[0]
        self.assertEqual((c["rate_before"], c["rate_now"]), (20.0, 5.0))
        self.assertEqual(c["regressions"], 1)  # sig-new: 2 per 100 of 200 sessions = 4 occurrences >= 3
        self.assertEqual((c["cost_per_agent_before"], c["cost_per_agent_now"]), (3.0, 2.0))

    def test_per100(self):
        self.assertEqual(self.ev.per100(5, 50), 10.0)
        self.assertEqual(self.ev.per100(5, 0), 500.0)


class PredictionTests(Base):
    def setup_yes(self):
        ev = self.ev
        rev = {"date": "2026-11-12", "patterns": [pattern("P1", "sig-a", 10, ["x"])],
               "metrics": {"sessions": 50, "signature_rates": {"sig-a": 20.0}, "jobs_silent_fail": 8},
               "text": {"items": [{"title": "jobs checked", "pattern_ids": [],
                                   "prediction": {"metric": "jobs_silent_fail", "expected": 0}}]}}
        ev.write_json(ev.REVIEWS / "2026-11-12.json", rev)
        ev.decide("2026-11-12", 1, "yes")
        d = ev.read_json(ev.DECISIONS, {})
        pr = d["2026-11-12"]["1"]["prediction"]
        self.assertEqual((pr["metric"], pr["current"], pr["expected"], pr["deadline"]), ("jobs_silent_fail", 8.0, 0.0, "2026-11-19"))
        return d

    def test_kept(self):
        d = self.setup_yes()
        out = self.ev.evaluate_predictions(d, {"jobs_silent_fail": 0}, "2026-11-19", True)
        self.assertEqual(out[0]["status"], "kept")

    def test_failed(self):
        d = self.setup_yes()
        out = self.ev.evaluate_predictions(d, {"jobs_silent_fail": 9}, "2026-11-19", True)
        self.assertEqual(out[0]["status"], "failed")

    def test_not_due_yet(self):
        d = self.setup_yes()
        self.assertEqual(self.ev.evaluate_predictions(d, {"jobs_silent_fail": 0}, "2026-11-15", True), [])

    def test_broken_logging_never_marks_kept(self):
        d = self.setup_yes()
        out = self.ev.evaluate_predictions(d, {"jobs_silent_fail": 0}, "2026-11-19", False)
        self.assertEqual(out[0]["status"], "unverified")

    def test_signature_rate_prediction(self):
        ev = self.ev
        rev = {"date": "2026-11-12", "patterns": [pattern("P1", "sig-a", 10, ["x"])],
               "metrics": {"sessions": 50, "signature_rates": {"sig-a": 20.0}},
               "text": {"items": [{"title": "teach fix", "pattern_ids": ["P1"]}]}}
        ev.write_json(ev.REVIEWS / "2026-11-12.json", rev)
        ev.decide("2026-11-12", 1, "yes")
        d = ev.read_json(ev.DECISIONS, {})
        self.assertEqual(d["2026-11-12"]["1"]["prediction"]["metric"], "signature_rate")
        out = ev.evaluate_predictions(d, {"signature_rates": {"sig-a": 4.0}}, "2026-11-20", True)
        self.assertEqual(out[0]["status"], "kept")  # 4 <= 10 (half of 20)


class RepeatTests(Base):
    def test_title_and_signature_repeats(self):
        rejected = [{"review": "2026-11-12", "title": "Turn on Autocompact for every agent", "signatures": ["sig-z"]}]
        items = [{"title": "Turn on Autocompact for agents"}, {"title": "Something else", "pattern_ids": ["P1"]},
                 {"title": "Unrelated idea"}]
        n = self.ev.find_repeats(items, [pattern("P1", "sig-z", 3, ["x"])], rejected)
        self.assertEqual(n, 2)
        self.assertIn("repeat_of_rejected", items[0])
        self.assertIn("repeat_of_rejected", items[1])
        self.assertNotIn("repeat_of_rejected", items[2])


class CanaryTests(Base):
    def test_missing_canary_means_broken(self):
        ev = self.ev
        # the hook is "removed": the runner succeeds but no fact is written
        rec = ev.canary(runner=lambda marker, cmd: (True, ""))
        self.assertFalse(rec["arrived"])
        st = ev.canary_status(ev.load_facts(ev.now() - dt.timedelta(days=1)))
        self.assertEqual(st["state"], "broken")
        self.assertTrue(any("Logging broken" in a for a in ev.alarms({"changed": []}, st)))

    def test_canary_arrives_when_hook_writes(self):
        ev = self.ev

        def runner(marker, cmd):  # simulate the hook writing the fact
            with open(ev.FACTS / f"{dt.date.today().isoformat()}.jsonl", "a") as fh:
                fh.write(json.dumps({"ts": ev.iso(), "event": "PostToolUseFailure", "tool": "Bash",
                                     "command": cmd, "error": f"Exit code 3\n{marker}"}) + "\n")
            return True, ""
        rec = ev.canary(runner=runner)
        self.assertTrue(rec["arrived"])
        facts = ev.load_facts(ev.now() - dt.timedelta(days=1))
        self.assertEqual(ev.canary_status(facts)["state"], "ok")
        self.assertTrue(all(ev.is_canary(f) for f in facts))  # kept out of the analysis

    def test_not_run(self):
        self.assertEqual(self.ev.canary_status([])["state"], "not run")


class LearningTests(Base):
    def test_too_early(self):
        L = self.ev.learning_summary()
        self.assertTrue(L["too_early"])
        self.assertIn("0 weeks", L["note"])

    def test_missed_roundtrip(self):
        self.ev.missed_add("the stock swarm stopped posting")
        self.assertEqual(len(self.ev.missed_list()), 1)
        self.assertEqual(self.ev.learning_summary()["rows"][0]["missed"], 1)


FAKE_CODEX = r"""#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
out = args[args.index("-o") + 1]
prompt = sys.stdin.read()
mode = os.environ.get("FAKE_CRITIC", "smart")
if mode == "garbage":
    open(out, "w").write("not json"); sys.exit(1)
diff = prompt.split("\n<diff>\n")[1].split("\n</diff>")[0]
bad = "hivey-safety.py" in diff or "settings.json" in diff
ans = [{"n": n, "answer": "no" if (bad and n in (1, 4)) or (mode == "short" and n == 9) else "yes", "evidence": "e"}
       for n in range(1, 10) if not (mode == "short" and n == 9)]
open(out, "w").write(json.dumps({"answers": ans, "summary": "s"}))
print("tokens used\n1,234")
"""


class VerdictLogTests(Base):
    def test_chain_detects_edit_and_cut(self):
        for i in range(3):
            self.ev.append_verdict({"kind": "fix", "item": f"x#{i}", "verdict": "pass"})
        self.assertEqual(self.ev.verify_verdicts(), {"ok": True, "entries": 3, "problem": None})
        lines = self.ev.VERDICTS.read_text().splitlines()
        self.ev.VERDICTS.write_text("\n".join(lines[:2]) + "\n")
        self.assertFalse(self.ev.verify_verdicts()["ok"])
        self.ev.VERDICTS.write_text("\n".join([lines[0], lines[1].replace('"pass"', '"block"'), lines[2]]) + "\n")
        self.assertIn("changed", self.ev.verify_verdicts()["problem"])
        with self.assertRaises(RuntimeError):
            self.ev.append_verdict({"kind": "fix"})

    def test_empty_log_is_not_ready(self):
        self.assertFalse(self.ev.safety_layer()[3]["done"])


class CriticTests(Base):
    def setUp(self):
        super().setUp()
        fake = Path(self.tmp.name) / "codex"
        fake.write_text(FAKE_CODEX)
        fake.chmod(0o755)
        os.environ["HIVEY_CODEX_BIN"] = str(fake)

    def tearDown(self):
        os.environ.pop("HIVEY_CODEX_BIN", None)
        os.environ.pop("FAKE_CRITIC", None)
        super().tearDown()

    def test_calibration_passes_and_unlocks(self):
        res = self.ev.critic_calibrate()
        self.assertTrue(res["passed"], res)
        self.assertTrue(self.ev.critic_calibrated())
        layer = {s["name"]: s["done"] for s in self.ev.safety_layer()}
        self.assertTrue(layer["Cross-family critic"] and layer["Verdict log"])

    def test_missing_answer_blocks(self):
        os.environ["FAKE_CRITIC"] = "short"
        r = self.ev.run_critic("p", "--- a/x\n+++ b/x\n", "ok")
        self.assertEqual(r["verdict"], "block")
        self.assertEqual(r["answers"][8]["answer"], "missing")
        self.assertEqual(r["tokens"], 1234)

    def test_garbage_blocks_and_calibration_fails(self):
        os.environ["FAKE_CRITIC"] = "garbage"
        self.assertEqual(self.ev.run_critic("p", "d", "e")["verdict"], "block")
        self.assertFalse(self.ev.critic_calibrate()["passed"])
        self.assertFalse(self.ev.critic_calibrated())

    def test_built_needs_passing_verdict(self):
        self.ev.write_json(self.ev.DECISIONS, {"2026-11-19": {"1": {"choice": "yes", "title": "t"}}})
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")
        self.ev.critic_review("p", "--- a/bin/x\n+++ b/bin/x\n", "ok", "2026-11-19#1")
        self.assertTrue(self.ev.mark_built("2026-11-19", 1, "abc")["built"])
        self.ev.record_outcomes([{"review": "2026-11-19", "n": 1, "status": "failed", "expected": 4, "value_at_check": 6}])
        month = list(self.ev.verdict_audit()["months"].values())[-1]
        self.assertEqual(month["passed_then_failed"], ["2026-11-19#1"])


if __name__ == "__main__":
    unittest.main()
