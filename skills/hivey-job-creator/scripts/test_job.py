"""Tests for job.py (the hivey job coordinator's bookkeeping). No GitHub: a local bare remote
stands in, with refs/pull/<n>/head pushed by hand.

Run: python3 -m unittest skills/hivey-job-creator/scripts/test_job.py
"""

import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def load_job(home: Path):
    os.environ["HOME"] = str(home)
    spec = importlib.util.spec_from_file_location("hivey_job_under_test", HERE / "job.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def git(cwd, *args):
    return subprocess.run(["git", "-c", "user.email=t@t", "-c", "user.name=t", *args], cwd=cwd,
                          check=True, capture_output=True, text=True).stdout.strip()


class JobTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.old_env = dict(os.environ)
        self.job = load_job(self.tmp / "home")
        self.remote = self.tmp / "remote.git"
        git(self.tmp, "init", "-q", "--bare", "-b", "main", str(self.remote))
        repo = self.repo = self.tmp / "repo"
        git(self.tmp, "clone", "-q", str(self.remote), str(repo))
        (repo / "calc.py").write_text("def add(a, b):\n    return a - b\n")
        git(repo, "add", "-A")
        git(repo, "commit", "-qm", "init")
        git(repo, "push", "-q", "origin", "HEAD:main")
        # job branch with the coordinator's acceptance test (fails on main)
        git(repo, "switch", "-qc", "job/demo")
        (repo / "tests").mkdir()
        (repo / "tests/test_calc.py").write_text(
            "import calc\n\ndef test_add():\n    assert calc.add(2, 3) == 5\n")
        git(repo, "add", "-A")
        git(repo, "commit", "-qm", "tests")
        git(repo, "push", "-q", "-u", "origin", "job/demo")

    def tearDown(self):
        os.environ.clear()
        os.environ.update(self.old_env)
        shutil.rmtree(self.tmp, ignore_errors=True)

    def pr(self, number, content):
        """An agent's pull request head: a commit on top of job/demo at refs/pull/<n>/head."""
        git(self.repo, "switch", "-q", "--detach", "origin/job/demo")
        (self.repo / "calc.py").write_text(content)
        git(self.repo, "commit", "-qam", f"fix\n\nhivey-job:demo#{number}")
        git(self.repo, "push", "-q", "origin", f"HEAD:refs/pull/{number}/head")
        git(self.repo, "switch", "-q", "job/demo")

    def make_job(self):
        cwd = os.getcwd()
        os.chdir(self.repo)
        try:
            self.job.main(["init", "demo", "--test-cmd",
                           "python3 -c 'import sys; sys.path.insert(0, \".\"); "
                           "import calc; assert calc.add(2, 3) == 5, \"add is wrong\"'",
                           "--tests", "tests/**"])
        finally:
            os.chdir(cwd)
        self.job.main(["add", "demo", "1", "--model", "sonnet", "--effort", "low"])
        self.job.main(["add", "demo", "2", "--deps", "1"])
        return self.job.load("demo")

    def test_plan_and_ready_follow_the_deps(self):
        job = self.make_job()
        self.assertEqual(job["branch"], "job/demo")
        self.assertEqual(job["issues"]["1"]["model"], "sonnet")
        self.assertEqual(self.job.ready(job), ["1"])
        self.job.main(["set", "demo", "1", "status=merged"])
        self.assertEqual(self.job.ready(self.job.load("demo")), ["2"])

    def test_guards(self):
        self.assertEqual(self.job.touched_protected(["src/a.py", "tests/test_calc.py"], ["tests/**"]),
                         ["tests/test_calc.py"])
        self.assertEqual(self.job.touched_protected(["src/a.py"], ["tests/**"]), [])
        job = {"name": "demo"}
        self.assertEqual(self.job.issue_of(job, {"body": "x\nhivey-job:demo#12\nRefs #12"}), "12")
        self.assertEqual(self.job.issue_of(job, {"body": "`hivey-job:demo#7`."}), "7")
        self.assertIsNone(self.job.issue_of(job, {"body": "hivey-job:other#7"}))

    def test_run_tests_on_a_passing_and_a_failing_pr(self):
        job = self.make_job()
        self.pr(5, "def add(a, b):\n    return a + b\n")
        self.pr(6, "def add(a, b):\n    return a * b\n")
        good = self.job.run_tests(job, self.repo, 5, timeout=60)
        bad = self.job.run_tests(job, self.repo, 6, timeout=60)
        self.assertEqual(good["code"], 0, good["tail"])
        self.assertNotEqual(bad["code"], 0)
        self.assertIn("add is wrong", bad["tail"])
        # the user's checkout is untouched and no worktree is left behind
        self.assertIn("a - b", (self.repo / "calc.py").read_text())
        self.assertEqual(list((self.tmp / "home/.hivey/jobs/worktrees").iterdir()), [])

    def test_a_pr_that_conflicts_with_the_job_branch_says_rebase(self):
        job = self.make_job()
        self.pr(7, "def add(a, b):\n    return b + a\n")
        (self.repo / "calc.py").write_text("def add(a, b):\n    return sum((a, b))\n")
        git(self.repo, "commit", "-qam", "another task merged")
        git(self.repo, "push", "-q", "origin", "job/demo")
        out = self.job.run_tests(job, self.repo, 7, timeout=60)
        self.assertEqual(out["code"], 1)
        self.assertIn("rebase", out["tail"])


    def test_board_data_shows_status_agent_and_links(self):
        self.make_job()
        job = self.job.load("demo")
        job["slug"] = "acme/app"
        job["issues"]["1"].update(status="running", pr=9)
        job["issues"]["3"] = {"status": "planned", "where": "local"}
        self.job.save(job)
        rows = {"1": {"state": "pr-open", "session": "s1", "url": "https://claude.ai/code/s1",
                      "pr": {"url": "https://github.com/acme/app/pull/9"}}}
        data = self.job.board_data("demo", rows)
        one = data["tasks"]["1"]
        self.assertEqual(one["status"], "pr-open")
        self.assertEqual(one["session_url"], "https://claude.ai/code/s1")
        self.assertEqual(one["issue_url"], "https://github.com/acme/app/issues/1")
        self.assertEqual(data["tasks"]["2"]["deps"], [1])
        self.assertEqual(data["tasks"]["3"]["status"], "local")
        summary = self.job.job_summaries()
        self.assertEqual([s["name"] for s in summary], ["demo"])
        self.assertTrue(summary[0]["active"])


if __name__ == "__main__":
    unittest.main()
