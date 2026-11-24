#!/usr/bin/env python3
"""hivey task log: a Claude Code hook that writes start and finish times to the task log.

Registered on UserPromptSubmit (writes a `prompt` line) and Stop (writes a `stop` line with the
agent's `DONE: …` words), into ~/.hivey/tasks.jsonl, the log the agent fleet's Tasks board
reads. Agents name their tasks themselves with `hivey task start "<title>"`; this hook makes
sure the times are there even when one forgets. A `prompt` line says who it came from: `you`
(typed in the terminal, or from Slack through the bus), `bus` (another agent or hivey) or
`other` (an interrupt marker). Costs no tokens and never blocks the agent: any problem just
means no line. Standard library only.
"""
import datetime as dt
import json
import os
import re
import sys
from pathlib import Path

MAX_TEXT = 120
MAX_BYTES = 4 * 1024 * 1024


def log_path():
    return Path.home() / ".hivey" / "tasks.jsonl"


def short(text):
    text = re.sub(r"\s+", " ", text or "").strip()
    return text if len(text) <= MAX_TEXT else text[: MAX_TEXT - 1].rstrip() + "…"


def prompt_from(text):
    """`you`, `bus` or `other`, and your words when it was you (the fleet's typed_by_you)."""
    if text.startswith("[Request interrupted"):
        return "other", ""
    if not text.startswith("[hivey"):
        return "you", text
    m = re.search(r"(?:^\[hivey\] |\n\d+\. )from human\b[^:]*:\s*(.+)", text)
    return ("you", m.group(1).split("\n")[0]) if m else ("bus", "")


def done_words(event):
    """The last `DONE: …` line of the agent's final message, or ""."""
    text = event.get("last_assistant_message")
    if not isinstance(text, str):
        return ""
    found = re.findall(r"^\s*DONE:\s*(.+?)\s*$", text, re.MULTILINE)
    return found[-1].strip("`* ") if found else ""


def line_for(event, now):
    name = event.get("hook_event_name")
    entry = {"ts": now}
    if name == "UserPromptSubmit":
        source, said = prompt_from((event.get("prompt") or "").strip())
        entry.update(event="prompt", **{"from": source})
        if said:
            entry["note"] = short(said)
    elif name == "Stop":
        if event.get("stop_hook_active"):
            return None
        entry["event"] = "stop"
        words = done_words(event)
        if words:
            entry["note"] = short(words)
    else:
        return None
    for key, value in (("pane", os.environ.get("HERDR_PANE_ID")),
                       ("hv_session", os.environ.get("HERDR_SESSION")),
                       ("session", event.get("session_id")),
                       ("cwd", event.get("cwd"))):
        if value:
            entry[key] = value
    entry["by"] = "hook"
    return json.dumps(entry, ensure_ascii=False)


def append(path, line):
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        if path.stat().st_size > MAX_BYTES:  # keep the newest half, whole lines
            text = path.read_text(errors="ignore")
            cut = text.find("\n", len(text) // 2) + 1
            tmp = path.with_suffix(".jsonl.tmp")
            tmp.write_text(text[cut:] if cut else "")
            os.replace(tmp, path)
    except OSError:
        pass
    with open(path, "a", encoding="utf-8") as fh:
        fh.write(line + "\n")


def main():
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    now = dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    line = line_for(event, now) if isinstance(event, dict) else None
    if line:
        try:
            append(log_path(), line)
        except OSError:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
