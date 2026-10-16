#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Run the Grok Build CLI (`grok`) headless from Python.

Examples:
  grok_run.py "What changed in Python 3.14?"
  grok_run.py --research "Search X for the most discussed AI agent frameworks over the last 7 days ..."
  grok_run.py --research --save ~/Desktop/agents.md "..."      # also write the answer to a file
  grok_run.py --code --cwd ~/projects/app "Fix the failing test in tests/test_api.py"
  grok_run.py --schema '{"type":"object","properties":{"major":{"type":"integer"}}}' "Parse v2.14.3"
  grok_run.py -c "Now go deeper on the second trend"          # continue last session in --cwd
  grok_run.py -r <session-id> "Follow-up question"
  echo "long prompt" | grok_run.py -
  grok_run.py --models | --sessions
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

GROK = os.environ.get("GROK_BIN", str(Path.home() / ".grok/bin/grok"))
SCRATCH = Path("/tmp/grok-runs")

# Research mode: search and fetch the web, nothing else. Without these, headless grok
# cancels on the first web_fetch approval, or wanders into shell/browser automation.
RESEARCH_PERMS = ["--allow", "WebSearch", "--allow", "WebFetch",
                  "--deny", "Bash", "--deny", "Edit", "--deny", "Write", "--deny", "MCPTool"]
RESEARCH_RULES = ("Answer using only web_search and web_fetch. Do not run shell commands, "
                  "read local files, drive a browser or look for local tools. "
                  "Cite every claim with the original URL. If a source can't be reached, "
                  "say so and continue with what you have.")


def build_cmd(a, prompt: str) -> list[str]:
    cmd = [GROK, "-p", prompt, "--output-format", "json", "--no-auto-update"]
    if a.model:
        cmd += ["-m", a.model]
    if a.effort:
        cmd += ["--effort", a.effort]
    if a.max_turns:
        cmd += ["--max-turns", str(a.max_turns)]
    if a.schema:
        schema = Path(a.schema).expanduser()
        cmd += ["--json-schema", schema.read_text() if schema.is_file() else a.schema]
    if a.cont:
        cmd += ["-c"]
    if a.resume:
        cmd += ["-r", a.resume]
    if a.research:
        cmd += RESEARCH_PERMS + ["--rules", RESEARCH_RULES]
    if a.code:
        cmd += ["--always-approve"]
    for rule in a.allow:
        cmd += ["--allow", rule]
    for rule in a.deny:
        cmd += ["--deny", rule]
    return cmd


def run(a, prompt: str) -> dict:
    p = subprocess.run(build_cmd(a, prompt), capture_output=True, text=True, cwd=a.cwd,
                       env={**os.environ, "GROK_DISABLE_AUTOUPDATER": "1"}, timeout=a.timeout)
    try:
        return json.loads(p.stdout)
    except json.JSONDecodeError:
        return {"type": "error", "message": (p.stderr or p.stdout).strip() or f"exit {p.returncode}"}


def main() -> int:
    ap = argparse.ArgumentParser(description="Run grok (Grok Build CLI) headless")
    ap.add_argument("prompt", nargs="?", help="prompt text, or '-' to read stdin")
    mode = ap.add_mutually_exclusive_group()
    mode.add_argument("--research", action="store_true", help="web search/fetch only, cite URLs")
    mode.add_argument("--code", action="store_true", help="auto-approve all tools in --cwd (edits, shell)")
    ap.add_argument("-m", "--model", help="model id (see --models)")
    ap.add_argument("-e", "--effort", help="none|minimal|low|medium|high|xhigh|max")
    ap.add_argument("--max-turns", type=int, help="cap agent turns (limits cost)")
    ap.add_argument("--schema", help="JSON schema string or path to a .json file")
    ap.add_argument("--cwd", type=lambda s: str(Path(s).expanduser()),
                    help="working dir (default: /tmp/grok-runs scratch dir)")
    ap.add_argument("-c", "--continue", dest="cont", action="store_true", help="continue last session in --cwd")
    ap.add_argument("-r", "--resume", help="resume a session id")
    ap.add_argument("--allow", action="append", default=[], help="extra permission allow rule")
    ap.add_argument("--deny", action="append", default=[], help="extra permission deny rule")
    ap.add_argument("--timeout", type=int, default=900, help="seconds before killing grok (default 900)")
    ap.add_argument("--save", help="also write the answer to this file")
    ap.add_argument("--json", action="store_true", help="print the full JSON envelope")
    ap.add_argument("--models", action="store_true", help="list models and exit")
    ap.add_argument("--sessions", action="store_true", help="list recent sessions and exit")
    a = ap.parse_args()

    if a.models:
        return subprocess.run([GROK, "models"]).returncode
    if a.sessions:
        return subprocess.run([GROK, "sessions", "list"], cwd=a.cwd).returncode
    if not a.prompt:
        ap.error("prompt required (or --models / --sessions)")
    prompt = sys.stdin.read() if a.prompt == "-" else a.prompt
    if not a.cwd:
        SCRATCH.mkdir(parents=True, exist_ok=True)
        a.cwd = str(SCRATCH)

    try:
        r = run(a, prompt)
    except subprocess.TimeoutExpired:
        print(f"[TIMEOUT] grok ran longer than {a.timeout}s", file=sys.stderr)
        return 124

    if a.json:
        print(json.dumps(r, indent=2, ensure_ascii=False))
    if r.get("type") == "error":
        if not a.json:
            print(f"[ERROR] {r.get('message')}", file=sys.stderr)
        return 2 if "usage limit" in r.get("message", "") else 1

    text = r.get("text", "")
    if not a.json:
        print(text)
    if a.save:
        Path(a.save).expanduser().write_text(text)
    cost = r.get("total_cost_usd")
    print(f"[grok] stop={r.get('stopReason')} turns={r.get('num_turns')} session={r.get('sessionId')}"
          + (f" cost=${cost:.3f}" if cost is not None else ""), file=sys.stderr)
    if r.get("stopReason") == "cancelled":
        print("[CANCELLED] a tool call needed approval. Use --research, --code, or --allow <Rule>.",
              file=sys.stderr)
        return 3
    if not text.strip():
        return 4
    return 0


if __name__ == "__main__":
    sys.exit(main())
