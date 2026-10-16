#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Run the Antigravity CLI (agy) headless from Python.

Examples:
  agy_run.py "What is git rebase?"
  agy_run.py -m claude-opus-4-6-thinking -e high "Explain this repo" --cwd ~/projects/hivey
  agy_run.py -m gemini-3.1-pro-high --schema '{"type":"object","properties":{"major":{"type":"integer"}}}' "Parse v2.14.3"
  echo "long prompt" | agy_run.py -
  agy_run.py --chat -m gemini-3.8-flash-high          # multi-turn session over stream-json
  agy_run.py --cwd ~/out --yolo --expect cat.png "Generate ... save as cat.png"
  agy_run.py --models                                 # list model slugs
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

AGY = os.environ.get("AGY_BIN", str(Path.home() / ".local/bin/agy"))


def base_cmd(a) -> list[str]:
    cmd = [AGY]
    if a.model:
        cmd += ["--model", a.model]
    if a.effort:
        cmd += ["--effort", a.effort]
    if a.agent:
        cmd += ["--agent", a.agent]
    if a.yolo:
        cmd += ["--dangerously-skip-permissions"]
    if a.sandbox:
        cmd += ["--sandbox"]
    if a.cont:
        cmd += ["--continue"]
    if a.conversation:
        cmd += ["--conversation", a.conversation]
    return cmd


def ask(a, prompt: str) -> dict:
    """One headless turn; returns the JSON envelope (response, status, usage, ...)."""
    cmd = base_cmd(a) + ["-p", prompt, "--output-format", "json", "--print-timeout", a.timeout]
    if a.schema:
        cmd += ["--json-schema", a.schema]
    p = subprocess.run(cmd, capture_output=True, text=True, cwd=a.cwd)
    try:
        return json.loads(p.stdout)
    except json.JSONDecodeError:
        return {"status": "ERROR", "error": (p.stderr or p.stdout).strip(), "exit_code": p.returncode}


def chat(a) -> None:
    """Multi-turn session: one agy process, prompts as NDJSON on stdin."""
    cmd = base_cmd(a) + ["--input-format", "stream-json", "--output-format", "stream-json"]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, bufsize=1, cwd=a.cwd)
    try:
        while True:
            try:
                msg = input("you> ").strip()
            except EOFError:
                break
            if msg in ("", "exit", "quit"):
                break
            proc.stdin.write(json.dumps({"event": "user", "message": {"content": msg}}) + "\n")
            proc.stdin.flush()
            for line in proc.stdout:
                ev = json.loads(line)
                step = ev.get("step_update") or {}
                if step.get("text_delta"):
                    print(step["text_delta"], end="", flush=True)
                elif ev.get("event") == "result":
                    r = ev["result"]
                    if r.get("status") != "SUCCESS":
                        print(f"\n[{r.get('status')}] {r.get('error', '')}", file=sys.stderr)
                    print()
                    break
    finally:
        proc.stdin.close()
        proc.wait()


def main() -> int:
    ap = argparse.ArgumentParser(description="Run agy (Antigravity CLI) headless")
    ap.add_argument("prompt", nargs="?", help="prompt text, or '-' to read stdin")
    ap.add_argument("-m", "--model", help="model slug (see --models)")
    ap.add_argument("-e", "--effort", choices=["low", "medium", "high"])
    ap.add_argument("--agent")
    ap.add_argument("--schema", help="JSON schema (string, .json path, or string/number/integer/boolean)")
    ap.add_argument("--cwd", type=lambda s: str(Path(s).expanduser()), help="workspace dir for the agent")
    ap.add_argument("--timeout", default="10m", help="--print-timeout (e.g. 5m, 0s = no limit)")
    ap.add_argument("--yolo", action="store_true", help="--dangerously-skip-permissions")
    ap.add_argument("--sandbox", action="store_true")
    ap.add_argument("-c", "--continue", dest="cont", action="store_true", help="continue last conversation")
    ap.add_argument("--conversation", help="resume conversation id")
    ap.add_argument("--json", action="store_true", help="print the full JSON envelope")
    ap.add_argument("--expect", action="append", default=[], metavar="FILE",
                    help="file the run must create (relative to --cwd); exit 3 if missing. agy can say SUCCESS when a tool failed")
    ap.add_argument("--chat", action="store_true", help="interactive multi-turn via stream-json")
    ap.add_argument("--models", action="store_true", help="list model slugs and exit")
    a = ap.parse_args()

    if a.models:
        return subprocess.run([AGY, "models"]).returncode
    if a.chat:
        chat(a)
        return 0
    if not a.prompt:
        ap.error("prompt required (or --chat / --models)")
    prompt = sys.stdin.read() if a.prompt == "-" else a.prompt

    r = ask(a, prompt)
    denied = r.get("denied_actions") or []
    if denied:
        names = ", ".join(d.get("action", "?") for d in denied)
        print(f"[DENIED] agy was not allowed to: {names} (rerun with --yolo, or allow them in "
              "~/.gemini/antigravity-cli/settings.json)", file=sys.stderr)
    if r.get("status") == "SUCCESS" and not (r.get("response") or "").strip() and r.get("structured_output") is None:
        r["status"] = "EMPTY"
        r.setdefault("error", "agy finished without an answer" + (" (actions were denied)" if denied else ""))
    if a.json:
        print(json.dumps(r, indent=2, ensure_ascii=False))
    elif r.get("status") == "SUCCESS":
        out = r.get("structured_output")
        print(json.dumps(out, indent=2, ensure_ascii=False) if out is not None else r.get("response", ""))
    else:
        print(f"[{r.get('status')}] {r.get('error', '')}", file=sys.stderr)
    if r.get("status") == "EMPTY":
        return 4
    if r.get("status") != "SUCCESS":
        return 1
    base = Path(a.cwd or ".")
    missing = [f for f in a.expect if not (base / Path(f).expanduser()).exists()]
    if missing:
        print(f"[MISSING] expected file(s) not created: {', '.join(missing)}", file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    sys.exit(main())
