#!/usr/bin/env python3
"""hivey agent stop: a Claude Code Stop hook that speaks the agent's run summary.

Agents end their final answer with `DONE: <3 words max>` on its own line. This reads the last
such line and speaks it with `hivey voice say` (the voice, quiet hours and quiet mode are
hivey's settings: `hivey voice`, `hivey quiet`). Swarm workers stay silent: hivey starts them
with AGENTS_TTS=0, so only masters and solo agents speak. Never blocks the agent: any problem
just means nothing is said. Standard library only.
"""
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


def last_assistant_text(event):
    """The agent's final message: from the hook input, else the transcript's last turn."""
    text = event.get("last_assistant_message")
    if isinstance(text, str) and text:
        return text
    transcript = event.get("transcript_path")
    if not transcript:
        return ""
    try:
        lines = Path(transcript).read_text(errors="ignore").splitlines()
    except OSError:
        return ""
    for line in reversed(lines):
        try:
            entry = json.loads(line)
        except ValueError:
            continue
        message = entry.get("message") or {}
        if entry.get("type") != "assistant" and message.get("role") != "assistant":
            continue
        content = message.get("content")
        if isinstance(content, str):
            return content
        if isinstance(content, list):
            parts = [c.get("text", "") for c in content if isinstance(c, dict) and c.get("type") == "text"]
            if parts:
                return "\n".join(parts)
    return ""


def summary(text):
    """The last `DONE: …` line's words, or None."""
    found = re.findall(r"^\s*DONE:\s*(.+?)\s*$", text, re.MULTILINE)
    return found[-1].strip("`* ") if found else None


def main():
    if os.environ.get("AGENTS_TTS") == "0" or os.environ.get("HIVEY_TTS") == "0":
        return 0
    try:
        event = json.load(sys.stdin)
    except ValueError:
        return 0
    if event.get("stop_hook_active"):
        return 0
    said = summary(last_assistant_text(event))
    hivey = shutil.which("hivey") or str(Path.home() / ".local" / "bin" / "hivey")
    if not said or not Path(hivey).exists():
        return 0
    subprocess.Popen(
        [hivey, "voice", "say", said],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
