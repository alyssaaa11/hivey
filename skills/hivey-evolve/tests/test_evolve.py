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
        self.sent = []
        self.ev.send_main = lambda text: (self.sent.append(text) or (True, "ok"))

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


class GoLiveTests(CriticTests):
    """The critic advises, the user decides (user, 2026-11-27)."""
    BAD = "--- a/.claude/hooks/hivey-safety.py\n+++ b/.claude/hooks/hivey-safety.py\n"   # the fake critic says no to 1 and 4
    GOOD = "--- a/bin/x\n+++ b/bin/x\n"
    ANS = [{"n": 1, "test": "test_gate_kept"}, {"n": 4, "reason": "the hook file is named in the proposal"}]

    def setUp(self):
        super().setUp()
        self.ev.write_json(self.ev.DECISIONS, {"2026-11-19": {"1": {"choice": "yes", "title": "t"}}})
        # No real fingerprint in tests: a stand-in key "signs" a message as sha256(key + message). The checks around it
        # (what is signed, refusal, verify on built and audit) are the real ones.
        import hashlib
        self.touches = []
        self.finger = True
        self.ev.GOLIVE_PUBKEY = "testkey"
        self.ev.KEY_BLOB.parent.mkdir(parents=True, exist_ok=True)
        self.ev.KEY_BLOB.write_text("blob")
        fake = lambda key, m: hashlib.sha256(key.encode() + m).hexdigest()   # the "Secure Enclave" holds testkey's half
        self.ev.user_sign = lambda m: (self.touches.append(m) or ((fake("testkey", m), "") if self.finger else (None, "cancelled")))
        self.ev.verify_sig = lambda pub, sig, m: sig == fake(pub, m)

    def test_no_touch_id_no_golive(self):
        last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        self.finger = False
        with self.assertRaises(ValueError):
            self.ev.go_live("2026-11-19", 1, last["sha"], self.ANS, "yes")
        self.assertEqual(len(self.touches), 1)
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")
        self.finger = True
        self.assertTrue(self.ev.go_live("2026-11-19", 1, last["sha"], self.ANS, "yes")["sig"])

    def test_signature_binds_the_golive(self):
        last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        g = self.ev.go_live("2026-11-19", 1, last["sha"], self.ANS, "yes go live")
        self.assertTrue(self.ev.golive_signed(g))
        for k, v in (("user_yes", "something else"), ("diff_sha", "0" * 64), ("answers", [])):
            self.assertFalse(self.ev.golive_signed({**g, k: v}), k)
        self.assertFalse(self.ev.golive_signed({**g, "sig": None}))
        self.ev.GOLIVE_PUBKEY = "otherkey"   # a different pinned key does not accept it
        self.assertFalse(self.ev.golive_signed(g))
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")
        self.ev.GOLIVE_PUBKEY = ""          # no key pinned: no go-live at all
        with self.assertRaises(ValueError):
            self.ev.go_live("2026-11-19", 1, last["sha"], self.ANS, "yes")

    def test_rebuilt_chain_with_unsigned_golive_is_caught(self):
        last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        forged = self.ev.append_verdict({"kind": "golive", "item": "2026-11-19#1", "critic_sha": last["sha"],
                                         "diff_sha": last.get("diff_sha"), "answers": self.ANS, "user_yes": "yes",
                                         "sig": "f" * 64})   # a valid chain line, as an agent could write it
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")
        d = self.ev.read_json(self.ev.DECISIONS, {})   # "built" written straight into the state file
        d["2026-11-19"]["1"].update({"built": True, "built_at": self.ev.iso(), "verdict_sha": forged["sha"]})
        self.ev.write_json(self.ev.DECISIONS, d)
        a = self.ev.verdict_audit()
        self.assertEqual(a["built_unapproved"], ["2026-11-19#1"])
        self.assertIn("2026-11-19#1", sum((m.get("golive_invalid", []) for m in a["months"].values()), []))

    def test_old_signed_golive_replayed_after_new_block_is_refused(self):
        v1 = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        g = self.ev.go_live("2026-11-19", 1, v1["sha"], self.ANS, "yes go live")
        self.ev.critic_review("p2", self.BAD, "e", "2026-11-19#1")   # the change moved on and was blocked again
        copy = {k: v for k, v in g.items() if k not in ("ts", "prev", "sha")}
        self.ev.append_verdict(copy)                                   # the old signed go-live, appended again
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")
        a = self.ev.verdict_audit()
        self.assertIn("2026-11-19#1", sum((m.get("golive_invalid", []) for m in a["months"].values()), []))
        with self.assertRaises(ValueError):                            # the same go-live twice in a row is a copy too
            self.ev.go_live("2026-11-19", 1, v1["sha"], self.ANS, "yes go live")

    def test_audit_checks_every_built_item_whatever_its_date(self):
        d = self.ev.read_json(self.ev.DECISIONS, {})
        d["2026-11-19"]["1"].update({"built": True})                   # no built_at, no verdict
        d["2026-11-20"] = {"1": {"choice": "yes", "built": True, "built_at": 0, "verdict_sha": 5}}   # malformed
        self.ev.write_json(self.ev.DECISIONS, d)
        self.assertEqual(sorted(self.ev.verdict_audit()["built_unapproved"]), ["2026-11-19#1", "2026-11-20#1"])

    def test_unsigned_round_past_stop_and_unsigned_golive_are_caught(self):
        for _ in range(3):
            last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        fake_golive = self.ev.append_verdict({"kind": "golive", "item": "2026-11-19#1", "critic_sha": last["sha"],
                                              "diff_sha": last.get("diff_sha"), "answers": self.ANS, "user_yes": "y"})
        self.assertEqual(self.ev.blocks_in_a_row("2026-11-19#1"), 3)   # it does not reset the count
        with self.assertRaises(ValueError):
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        self.ev.append_verdict({"kind": "fix", "item": "2026-11-19#1", "verdict": "block", "answers": []})   # round 4 by hand
        self.assertEqual(self.ev.verdict_audit()["rounds_unsigned"], ["2026-11-19#1"])
        self.assertTrue(fake_golive)

    def test_public_key_is_read_from_its_locked_file_not_the_code(self):
        self.assertNotIn("MFkwEwYHKoZIzj0CAQYI", (Path(self.ev.__file__)).read_text())   # no machine's key in shipped code
        self.ev.PUBKEY_FILE.write_text("pinnedkey\n")
        self.assertEqual(self.ev._read_pubkey(), "pinnedkey")
        self.assertIn(self.ev.PUBKEY_FILE.resolve(), self.ev.protected_files())   # swapping it is a lock change
        with self.assertRaises(ValueError):   # never replaced
            self.ev.golive_key()

    def test_rounds_before_the_rule_marker_are_not_flagged(self):
        for _ in range(4):   # 4 rounds from before the rule
            self.ev.append_verdict({"kind": "fix", "item": "2026-11-19#1", "verdict": "block", "answers": []})
        self.ev.append_verdict({"kind": "rule", "rule": "golive-signed"})
        self.assertEqual(self.ev.verdict_audit()["rounds_unsigned"], [])
        self.ev.append_verdict({"kind": "fix", "item": "2026-11-19#1", "verdict": "block", "answers": []})   # after it
        self.assertEqual(self.ev.verdict_audit()["rounds_unsigned"], ["2026-11-19#1"])

    @unittest.skipUnless(Path("/usr/bin/swiftc").exists() and Path("/usr/bin/openssl").exists(), "needs swiftc and openssl")
    def test_real_helper_and_openssl_verify(self):
        r = self.ev._touchid(["selftest", "x"], timeout=60)
        ok, pub, sig = r.stdout.splitlines()
        self.assertEqual((r.returncode, ok), (0, "good=true changed=false"))
        import importlib   # the real verify_sig (setUp replaced it), with /usr/bin/openssl
        spec = importlib.util.spec_from_file_location("ev_real", self.ev.__file__)
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        self.assertTrue(mod.verify_sig(pub, sig, b'{"item":"x"}'))
        self.assertFalse(mod.verify_sig(pub, sig, b'{"item":"y"}'))
        self.assertFalse(mod.verify_sig(pub, "not base64!", b'{"item":"x"}'))

    def test_touch_id_source_is_locked(self):
        self.assertIn((self.ev.SCRIPTS / "touchid.swift").resolve(), self.ev.protected_files())

    def test_golive_needs_every_no_answered_latest_verdict_and_user_words(self):
        old = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")
        for sha, ans, yes in [(old["sha"], self.ANS, "yes"),            # an older verdict
                              (last["sha"], self.ANS[:1], "yes"),       # check 4 unanswered
                              (last["sha"], [{"n": 1, "test": "t", "reason": "r"}, self.ANS[1]], "yes"),   # both
                              (last["sha"], self.ANS, "  "),            # no user words
                              (last["sha"][:6], self.ANS, "yes")]:      # sha too short
            with self.assertRaises(ValueError):
                self.ev.go_live("2026-11-19", 1, sha, ans, yes)
        self.assertEqual(self.touches, [])   # a refused go-live never even asks for the finger
        g = self.ev.go_live("2026-11-19", 1, last["sha"][:12], self.ANS, "yes go live")
        self.assertEqual((g["kind"], g["critic_sha"], [a["n"] for a in g["answers"]]), ("golive", last["sha"], [1, 4]))
        rec = self.ev.mark_built("2026-11-19", 1, "abc")
        self.assertEqual((rec["built"], rec["approved_by"], rec["verdict_sha"]), (True, "user go-live", g["sha"]))
        self.assertTrue(self.ev.verify_verdicts()["ok"])

    def test_golive_refused_for_unapproved_item_or_passed_verdict(self):
        with self.assertRaises(ValueError):
            self.ev.go_live("2026-11-19", 2, "x" * 12, self.ANS, "yes")
        v = self.ev.critic_review("p", self.GOOD, "e", "2026-11-19#1")
        with self.assertRaises(ValueError):
            self.ev.go_live("2026-11-19", 1, v["sha"], self.ANS, "yes")

    def test_block_after_pass_or_golive_needs_a_new_one(self):
        self.ev.critic_review("p", self.GOOD, "e", "2026-11-19#1")
        last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")
        self.ev.go_live("2026-11-19", 1, last["sha"], self.ANS, "yes")
        self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        with self.assertRaises(ValueError):
            self.ev.mark_built("2026-11-19", 1, "abc")

    def test_three_blocks_in_a_row_go_to_the_user(self):
        for _ in range(3):
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        self.assertEqual(len(self.sent), 1)
        self.assertIn("3 rounds in a row", self.sent[0])
        with self.assertRaises(ValueError):
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        e = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1", user_asked="one more round")
        self.assertEqual(e["user_asked"], "one more round")
        self.assertEqual(len(self.sent), 1)
        self.ev.go_live("2026-11-19", 1, e["sha"], self.ANS, "yes")
        self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")   # a go-live resets the count

    def test_audit_lists_golive_and_golive_then_failed(self):
        last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        self.ev.go_live("2026-11-19", 1, last["sha"], self.ANS, "yes")
        self.ev.record_outcomes([{"review": "2026-11-19", "n": 1, "status": "failed", "expected": 0, "value_at_check": 2}])
        month = list(self.ev.verdict_audit()["months"].values())[-1]
        self.assertEqual((month["user_golive"], month["golive_then_failed"], month["passed_then_failed"]),
                         (["2026-11-19#1"], ["2026-11-19#1"], []))


    def test_forged_log_line_is_not_trusted(self):
        last = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        with open(self.ev.VERDICTS, "a") as fh:   # a builder writes a go-live by hand
            fh.write(json.dumps({"ts": "x", "kind": "golive", "item": "2026-11-19#1", "prev": last["sha"], "sha": "f" * 64}) + "\n")
        for call in (lambda: self.ev.mark_built("2026-11-19", 1, "abc"),
                     lambda: self.ev.go_live("2026-11-19", 1, last["sha"], self.ANS, "yes")):
            with self.assertRaises(ValueError):
                call()

    def test_incomplete_verdict_and_thin_answers_refused(self):
        os.environ["FAKE_CRITIC"] = "short"   # check 9 missing
        v = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        with self.assertRaises(ValueError):
            self.ev.go_live("2026-11-19", 1, v["sha"], self.ANS + [{"n": 9, "reason": "x" * 40}], "yes")
        os.environ.pop("FAKE_CRITIC")
        v = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        for ans in ([{"n": 1, "test": "t"}, self.ANS[1]], [self.ANS[0], {"n": 4, "reason": "."}]):
            with self.assertRaises(ValueError):
                self.ev.go_live("2026-11-19", 1, v["sha"], ans, "yes")
        g = self.ev.go_live("2026-11-19", 1, v["sha"], self.ANS, "yes")
        self.assertEqual(g["diff_sha"], v["diff_sha"])
        self.ev.mark_built("2026-11-19", 1, "abc")
        with self.assertRaises(ValueError):   # never overwritten
            self.ev.mark_built("2026-11-19", 1, "def")

    def test_item_spelling_parallel_rounds_and_outages(self):
        for bad in ("2026-11-19#01", " 2026-11-19#1", "x"):
            with self.assertRaises(ValueError):
                self.ev.critic_review("p", self.BAD, "e", bad)
        import fcntl
        self.ev.STATE.mkdir(parents=True, exist_ok=True)
        with open(self.ev.STATE / "critic-2026-11-19-1.lock", "a") as lk:   # a round already running
            fcntl.flock(lk, fcntl.LOCK_EX)
            with self.assertRaises(ValueError):
                self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        os.environ["FAKE_CRITIC"] = "garbage"   # Codex down three times: not blocked rounds
        for _ in range(3):
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        self.assertEqual((self.ev.blocks_in_a_row("2026-11-19#1"), self.sent), (0, []))


    def test_round_past_the_stop_needs_touch_id(self):
        for _ in range(3):
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        self.finger = False
        with self.assertRaises(ValueError):
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1", user_asked="try again")
        self.finger = True
        e = self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1", user_asked="try again")
        self.assertEqual(len(self.touches), 2)
        self.assertTrue(self.ev.verify_sig("testkey", e["user_asked_sig"],
                        json.dumps({"kind": "round", "item": e["item"], "after": e["after"], "user_asked": "try again"},
                                   sort_keys=True, separators=(",", ":")).encode()))
        before = len(self.touches)
        with self.assertRaises(ValueError):   # 4 blocks now: the next round needs the finger again
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        self.assertEqual(len(self.touches), before)

    def test_undelivered_alert_text_is_redacted(self):
        fake_key = "api" + "_key=" + "sk-" + "ant-" + "REALSECRET1234567890abcd"   # built from parts: no key-like text in this file
        self.ev.send_main = lambda text: (False, "auth failed: " + fake_key)
        for _ in range(3):
            self.ev.critic_review("p", self.BAD, "e", "2026-11-19#1")
        logged = self.ev.ACTIVITY.read_text()
        self.assertIn("message not delivered", logged)
        self.assertNotIn("REALSECRET", logged)


class ErrorLineTests(Base):
    """A Python crash is labelled by its final exception line, not by 'Traceback (most recent call last):'."""
    TB = ("Exit code 1\nTraceback (most recent call last):\n  File \"/tmp/x.py\", line 1, in <module>\n"
          "    import PIL\nModuleNotFoundError: No module named 'PIL'")

    def test_traceback_uses_final_exception(self):
        self.assertEqual(self.ev.error_line(self.TB), "ModuleNotFoundError: No module named 'PIL'")
        sig = self.ev.signature({"tool": "Bash", "command": "python3 x.py", "error": self.TB})
        self.assertEqual(sig, "Bash|python3|modulenotfounderror: no module named <s>")

    def test_chained_traceback_uses_last_exception(self):
        err = ("Traceback (most recent call last):\n  File \"a\", line 1\nKeyError: 'a'\n\nDuring handling of the above "
               "exception, another exception occurred:\n\nTraceback (most recent call last):\n  File \"b\", line 2\n"
               "json.decoder.JSONDecodeError: Expecting value: line 1 column 1 (char 0)")
        self.assertEqual(self.ev.error_line(err), "json.decoder.JSONDecodeError: Expecting value: line 1 column 1 (char 0)")

    def test_secret_in_exception_message_is_redacted(self):
        # fake key and password built from pieces, so this file holds no secret-shaped literal
        # (the critic's input is redacted too, which would cut a literal one mid-string)
        fake_key, fake_pw = "sk" + "-" + "abcdefghijklmnop1234", "hunter2" * 2
        err = ("Traceback (most recent call last):\n  File \"x\", line 1\n"
               "openai.AuthenticationError: bad key " + fake_key + " api" + "_key" + "=" + fake_pw)
        line = self.ev.error_line(err)
        self.assertNotIn(fake_key, line)
        self.assertNotIn(fake_pw, line)
        self.assertTrue(line.startswith("openai.AuthenticationError: bad key"))
        self.assertNotIn("abcdefghijklmnop", self.ev.signature({"tool": "Bash", "command": "python3 x.py", "error": err}))

    def test_traceback_without_exception_line_and_non_python_unchanged(self):
        self.assertEqual(self.ev.error_line("Traceback (most recent call last):\n  File \"x\", line 3"),
                         "Traceback (most recent call last):")
        self.assertEqual(self.ev.error_line("fatal: not a git repository"), "fatal: not a git repository")
        self.assertEqual(self.ev.error_line("ls: x: No such file or directory"), "ls: x: No such file or directory")


if __name__ == "__main__":
    unittest.main()
