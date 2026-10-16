#!/usr/bin/env python3
"""Token usage per swarm agent, the coordinator, and the swarm total.

Herdr reports which Claude session each pane runs; every session an agent has had
(restarts create new ones) is remembered in <root>/.swarm/usage.json. Tokens are summed
from the session transcripts in ~/.claude/projects/*/<session>.jsonl (plus any sub-agent
transcripts in <session>/subagents/), deduplicated by message id — Claude Code logs one
line per streamed content block, all carrying the same usage. Reads are incremental
(per-file offsets), so the relay can call this every minute.

Tokens only, no $: price tables go stale; the numbers here are exact.

Usage: usage.py <swarm_root>        # update usage.json and print the summary
"""
import glob
import json
import subprocess
import os
import sys
from pathlib import Path


def write_atomic(path, text):
    """Write text to path via a temp file, so readers never see half a file."""
    path = Path(path)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(text)
    os.replace(tmp, path)


FIELDS = {"input_tokens": "input", "output_tokens": "output",
          "cache_creation_input_tokens": "cache_write", "cache_read_input_tokens": "cache_read"}
PROJECTS = Path.home() / ".claude" / "projects"


def live_sessions():
    r = subprocess.run(["hivey", "agent", "list"], capture_output=True, text=True)
    try:
        agents = json.loads(r.stdout)["result"]["agents"]
    except (json.JSONDecodeError, KeyError):
        return {}
    return {a.get("name"): (a.get("agent_session") or {}).get("value") for a in agents if a.get("name")}


def transcripts(session_id):
    files = glob.glob(str(PROJECTS / "*" / f"{session_id}.jsonl"))
    files += glob.glob(str(PROJECTS / "*" / session_id / "subagents" / "*.jsonl"))
    return files


def scan(path, state):
    """Add new usage lines from `path` to `state` ({offset, last_id, totals, model})."""
    try:
        with open(path) as f:
            f.seek(state.get("offset", 0))
            while True:
                line = f.readline()
                if not line.endswith("\n"):  # EOF, or a partial line still being written
                    break
                state["offset"] = f.tell()
                if '"usage"' not in line:
                    continue
                try:
                    msg = json.loads(line).get("message") or {}
                except json.JSONDecodeError:
                    continue
                if not isinstance(msg, dict) or not msg.get("usage") or msg.get("id") == state.get("last_id"):
                    continue
                state["last_id"] = msg.get("id")
                state["model"] = msg.get("model") or state.get("model")
                tot = state.setdefault("totals", {v: 0 for v in FIELDS.values()})
                for k, v in FIELDS.items():
                    tot[v] += int(msg["usage"].get(k) or 0)
    except FileNotFoundError:
        pass


def collect(root):
    root = Path(root)
    m = json.loads((root / ".swarm" / "agents.json").read_text())
    path = root / ".swarm" / "usage.json"
    data = json.loads(path.read_text()) if path.exists() else {"sessions": {}, "files": {}}
    herdr_names = {info["herdr_name"]: a for a, info in m["agents"].items()}
    herdr_names[m["coordinator"]] = "coordinator"
    for hname, sid in live_sessions().items():
        who = herdr_names.get(hname)
        if who and sid and sid not in data["sessions"].setdefault(who, []):
            data["sessions"][who].append(sid)

    per = {}
    for who, sids in data["sessions"].items():
        tot = {v: 0 for v in FIELDS.values()}
        models = set()
        for sid in sids:
            for f in transcripts(sid):
                st = data["files"].setdefault(f, {})
                scan(f, st)
                for k in tot:
                    tot[k] += st.get("totals", {}).get(k, 0)
                if st.get("model"):
                    models.add(st["model"])
        tot["total"] = sum(tot.values())
        tot["models"] = sorted(models)
        tot["sessions"] = len(sids)
        per[who] = tot
    data["per_agent"] = per
    swarm = {k: sum(p[k] for p in per.values()) for k in list(FIELDS.values()) + ["total"]}
    data["swarm_total"] = swarm
    write_atomic(path, json.dumps(data, indent=1) + "\n")
    return data


def human(n):
    return f"{n / 1e6:.1f}M" if n >= 1e6 else f"{n / 1e3:.0f}k" if n >= 1e3 else str(n)


def main():
    data = collect(Path(sys.argv[1]).expanduser().resolve())
    for who, t in sorted(data["per_agent"].items(), key=lambda kv: -kv[1]["total"]):
        print(f"{who:14} total {human(t['total']):>7}  in {human(t['input']):>6}  out {human(t['output']):>6}  "
              f"cache-w {human(t['cache_write']):>6}  cache-r {human(t['cache_read']):>7}  "
              f"({', '.join(t['models']) or '?'}, {t['sessions']} session(s))")
    s = data["swarm_total"]
    print(f"{'SWARM TOTAL':14} total {human(s['total']):>7}  in {human(s['input']):>6}  out {human(s['output']):>6}  "
          f"cache-w {human(s['cache_write']):>6}  cache-r {human(s['cache_read']):>7}")


if __name__ == "__main__":
    main()
