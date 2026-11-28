"""evolve item 2026-11-25 #1: a job is only 'ok' when its latest run's log has no error.
Run: python3 -I skills/hivey-agent-map/tests/test_honest_status.py [path/to/agent_map.py]"""
import importlib.util, os, sys, tempfile
src = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "scripts", "agent_map.py")
spec = importlib.util.spec_from_file_location("agent_map", src); a = importlib.util.module_from_spec(spec); spec.loader.exec_module(a)
d = tempfile.mkdtemp(); res = []
def status(text, rc=0, state="ok"):
    f = os.path.join(d, "x.log"); open(f, "w").write(text)
    info = a.log_info(f, rc)
    return a.honest_state(state, info) if hasattr(a, "honest_state") else state
def check(name, got, want):
    res.append(got == want); print(("PASS " if got == want else "FAIL ") + f"{name}: got {got!r}, want {want!r}")
check("exit0_with_error_in_log_is_not_ok", status("--- run\nPREFLIGHT FAIL ~/a: the remote can't be reached: authentication failed\n"), "error in log")
check("exit0_with_fatal_git_text_is_not_ok", status("--- run\nremote: fatal: pack exceeds maximum allowed size (2.00 GiB)\n"), "error in log")
check("clean_run_is_ok", status("--- run\ncommitted 2 changed files\npreflight ok ~/a (push)\ndone: pushed 1 commits\n"), "ok")
check("old_error_then_done_is_ok", status("--- old\npush failed, exit code 1\n--- new\ndone: nothing to commit\n"), "ok")
check("done_then_new_error_is_not_ok", status("--- a\ndone: nothing to commit\n--- b\npull failed, exit code 1\n"), "error in log")
check("pull_all_end_with_failures_is_not_ok", status("FAIL  ~/x: pull failed, exit code 1\n=== t git-pull-all end (1 failed)\n"), "error in log")
check("pull_all_done_after_old_failure_is_ok", status("FAIL  ~/x: pull failed, exit code 1\n=== t git-pull-all end (1 failed)\nok    ~/y\n=== t git-pull-all done (0 failed)\n"), "ok")
check("zero_errors_summary_is_ok", status("checked 10 items, 0 errors\n"), "ok")
check("nonzero_exit_status_kept", status("boom error\n", rc=1, state="exit 1"), "exit 1")
check("running_status_kept", status("error earlier\n", state="running"), "running")
check("error_then_finished_is_not_ok", status("===== run\nAPI Error: 400 usage limits\n===== finished (exit 0)\n"), "error in log")
check("old_run_error_then_clean_finished_run_is_ok", status("===== r1\nAPI Error: 400\n===== finished\n===== r2\npages 10\n===== finished\n"), "ok")
lp = a.log_path
check("log_path_git_job_commit", lp("/u/bin/git-job.sh commit /u/agents /u/logs/agents-commit.log"), "/u/logs/agents-commit.log")
check("log_path_git_job_run", lp("/u/bin/git-job.sh run /u/logs/git-pull-all.log /bin/bash /u/bin/git-pull-all.sh"), "/u/logs/git-pull-all.log")
check("log_path_redirect_still_works", lp("python3 x.py >> /u/logs/x.log 2>&1"), "/u/logs/x.log")
def last_error(text):
    f = os.path.join(d, "x.log"); open(f, "w").write(text)
    return a.log_info(f, 0).get("last_error") or ""
fake_kv, fake_sk = "api_" + "key=" + "abcd1234efgh5678", "sk-" + "A1b2C3d4E5f6G7h8I9j0"  # built at run time: no secret-shaped literal in this file
check("secret_value_masked_in_last_error", fake_kv[8:] in last_error(f"--- run\nAPI Error: 401 {fake_kv} rejected\n"), False)
check("secret_key_masked_in_last_error", fake_sk in last_error(f"--- run\nfatal: bad key {fake_sk}\n"), False)
check("masked_line_still_an_error", "[redacted]" in last_error(f"--- run\nfatal: bad key {fake_sk}\n"), True)
def all_fields(text):
    f = os.path.join(d, "y.log"); open(f, "w").write(text)
    i = a.log_info(f, 0); return repr(i.get("tail")) + repr(i.get("errors")) + repr(i.get("last_error"))
check("secret_masked_in_tail_and_errors", fake_sk in all_fields(f"--- run\nfatal: bad key {fake_sk}\nfinished\n"), False)
check("secret_masked_in_tail_of_ok_run", fake_kv[8:] in all_fields(f"--- run\nusing {fake_kv}\ndone\n"), False)
# the agent map reads only the last 6000 bytes; a cut inside a key must not leave the key's rest readable
key_line = f"fatal: bad key {fake_sk}\n"; cut = len("fatal: bad key ") + 8   # cut just after the first 8 key chars
pad = "done " + "y" * 181 + "\n"; rest = 6000 - (len(key_line) - cut); fill = pad * ((rest - 1) // len(pad)); fill += "z" * (rest - len(fill) - 1) + "\n"
assert len(key_line) - cut + len(fill) == 6000
check("secret_cut_by_6000_byte_read_masked", fake_sk[-10:] in all_fields("--- run\n" + "w" * 300 + "\n" + key_line + fill), False)
# a complete error line that starts exactly where the 6000-byte read starts still counts (it is not a partial line)
err_line = "fatal: remote rejected\n"; pad2 = "info " + "y" * 181 + "\n"  # neutral: "done" would end the run as a success
fill2 = pad2 * ((6000 - len(err_line) - 1) // len(pad2)); fill2 += "z" * (6000 - len(err_line) - len(fill2) - 1) + "\n"
assert len(err_line) + len(fill2) == 6000
check("error_line_at_read_boundary_still_counts", status("w" * 300 + "\n" + err_line + fill2), "error in log")
check("complete_line_at_read_boundary_kept_as_text", "remote rejected" in all_fields("w" * 300 + "\n" + err_line + fill2), True)
# the same text with the read starting one byte into the error line: the cut line's text is not stored (our fixed
# stand-in replaces it), but its error still counts
cut_text = "w" * 300 + "\n" + "X" + err_line + fill2
check("cut_error_line_at_read_boundary_still_counts", status(cut_text), "error in log")
check("cut_line_text_not_stored", "remote rejected" in all_fields(cut_text), False)
fake_pw = "hunter2" + "Abc9"
check("json_quoted_password_masked", fake_pw in all_fields('--- run\nfatal: bad config {"pass' + 'word": "' + fake_pw + '"}\n'), False)
check("json_quoted_token_masked", fake_pw in all_fields("--- run\nerror: {'tok" + "en': '" + fake_pw + "'}\n"), False)
check("api_key_with_space_masked", "Zr8" in all_fields("--- run\nerror: API " + "key: Zr8vQ2 rejected\n"), False)
check("error_word_inside_key_value_still_counts", status("--- run\nAPI " + "key: failed authentication\n"), "error in log")
check("bearer_header_masked", "Pq4" in all_fields("--- run\nerror: 401 Authorization: Bear" + "er Pq4wZ9.k2 rejected\n"), False)
check("json_quoted_authorization_masked", "Hv3" in all_fields('--- run\nerror: 401 {"Authorization": "Bear' + 'er Hv3nK8.p1"}\n'), False)
check("single_quoted_authorization_masked", "Jw5" in all_fields("--- run\nfailed: curl -H 'Authorization: Bas" + "ic Jw5rT2x9' 403\n"), False)
check("escaped_quote_inside_password_masked", "Ty7" in all_fields('--- run\nerror: rejected {"pass' + 'word": "ab\\"cdTy7mQ"}\n'), False)
check("escaped_quote_inside_authorization_masked", "Gk4" in all_fields('--- run\nfailed: {"Authorization": "Bear' + 'er x\\"Gk4pL0"}\n'), False)
check("bearer_alone_masked", "Lm6" in all_fields("--- run\nfailed: sent bear" + "er Lm6tY8u3 got 403\n"), False)
check("short_password_value_masked", "Qx7" in all_fields("--- run\nfatal: login pass" + "word=Qx7 refused\n"), False)
print(f"{sum(res)} passed, {len(res)-sum(res)} failed"); sys.exit(0 if all(res) else 1)
