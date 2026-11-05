#!/usr/bin/env python3
"""hivey spend log: record the tokens and cost of every LLM API call a script makes.

One line per call in ~/.hivey/spend/ledger.jsonl, read by agent_map.py (the hivey agent map).
Calls made through Claude Code, Codex or Grok CLI are already counted from their own transcripts:
use this only for scripts that call an API directly (Anthropic SDK, OpenAI, OpenRouter, xAI,
DeepSeek, Gemini, a local server...).

From Python (standard library only; it never raises, so logging cannot break the caller):

    import sys, os; sys.path.insert(0, os.path.expanduser("~/.claude/skills/hivey-agent-map/scripts"))
    import hivey_spend
    resp = client.messages.create(...)            # or requests/httpx JSON, or a dict
    hivey_spend.record(resp, agent="x-ai-agents-trends")

    hivey_spend.record_usage(agent="n", provider="xai", model="grok-4.7", input=1200, output=300)

From a shell script, pipe the raw JSON response through it (it is printed back unchanged):

    curl -s https://openrouter.ai/api/v1/chat/completions ... \
      | python3 ~/.claude/skills/hivey-agent-map/scripts/hivey_spend.py --agent my-job

The agent name defaults to $HIVEY_AGENT, else the agent of the current folder (~/agents/<name>).
Understands: Anthropic Messages, OpenAI Chat Completions and Responses (also OpenRouter, xAI,
DeepSeek and other OpenAI-compatible APIs, with their reported `cost` when present), Gemini
`usageMetadata`. A streamed call: pass the final message / the last chunk that carries usage.
"""
from __future__ import annotations

import datetime as dt
import json
import os
import sys
from pathlib import Path

HOME = Path.home()
STATE = HOME / ".hivey" / "agent-map"
SPEND_LOG = Path(os.environ.get("HIVEY_SPEND_LOG", HOME / ".hivey" / "spend" / "ledger.jsonl"))
ALIASES = STATE / "agents.json"  # {"/path/prefix": "agent-name"} overrides

# USD per million tokens: (input, output, cache read, cache write). Substring match, longest wins.
# Override or extend in ~/.hivey/agent-map/prices.json: {"model-substring": [in, out, read, write]}.
PRICES = {
    "claude-opus": (4.00, 20.00, 0.20, 5.00),
    "claude-sonnet": (2.00, 10.00, 0.20, 2.50),
    "claude-haiku": (0.10, 0.50, 0.01, 0.125),
    "deepseek": (0.30, 1.20, 0.03, 0.30),
    "gpt-": (1.25, 10.00, 0.125, 1.25),
    "codex": (1.25, 10.00, 0.125, 1.25),
    "grok": (3.00, 15.00, 0.75, 3.00),
    "gemini": (1.25, 10.00, 0.31, 1.25),
    "muse": (0.0, 0.0, 0.0, 0.0),  # local MLX
    "/users/": (0.0, 0.0, 0.0, 0.0),  # local model served from a folder
}


def prices() -> dict:
    p = dict(PRICES)
    try:
        for k, v in json.loads((STATE / "prices.json").read_text()).items():
            p[k.lower()] = tuple(v)
    except (OSError, ValueError):
        pass
    return p


def price_for(model: str, table: dict | None = None) -> tuple:
    table = table or prices()
    m = (model or "").lower()
    best = max((k for k in table if k in m), key=len, default="")
    return table.get(best, (0.0, 0.0, 0.0, 0.0))


def cost(model: str, inp: int, out: int, read: int = 0, write: int = 0, table: dict | None = None) -> float:
    pi, po, pr, pw = price_for(model, table)
    return (inp * pi + out * po + read * pr + write * pw) / 1e6


def agent_of(path: str | None) -> str:
    """Name the agent a working folder belongs to (~/agents/<name>, ~/swarms/<name>, ...)."""
    if not path:
        return "unknown"
    p = os.path.realpath(os.path.expanduser(path))
    try:
        aliases = json.loads(ALIASES.read_text())
    except (OSError, ValueError):
        aliases = {}
    for prefix, name in sorted(aliases.items(), key=lambda kv: -len(kv[0])):
        if p.startswith(os.path.realpath(os.path.expanduser(prefix))):
            return name
    rel = os.path.relpath(p, HOME)
    parts = rel.split(os.sep)
    if rel.startswith(".."):
        return "tmp" if p.startswith(("/tmp", "/private/tmp", "/var/folders")) else p
    if rel == ".":
        return "home (headless)"
    if parts[0] in ("agents", "swarms") and len(parts) > 1:
        return parts[1] if parts[0] == "agents" else f"swarm:{parts[1]}"
    if parts[:2] == [".hivey", "agent-map"]:
        return "hivey-agent-map"
    if parts[:2] == [".hivey", "agent"]:
        return "hivey"
    if parts[:2] == [".claude", "skills"] and len(parts) > 2:
        return f"skill:{parts[2]}"
    if parts[0] == "projects" and len(parts) > 1:
        return f"project:{parts[1]}"
    return "~/" + parts[0]


def _as_dict(obj):
    if obj is None or isinstance(obj, dict):
        return obj or {}
    for attr in ("model_dump", "to_dict", "dict"):  # pydantic / SDK objects
        f = getattr(obj, attr, None)
        if callable(f):
            try:
                return f()
            except Exception:
                pass
    if hasattr(obj, "json") and callable(obj.json):  # requests / httpx response
        try:
            return obj.json()
        except Exception:
            pass
    return getattr(obj, "__dict__", {}) or {}


def _n(d: dict, *keys) -> int:
    for k in keys:
        v = d.get(k)
        if isinstance(v, (int, float)):
            return int(v)
    return 0


def parse(response) -> dict | None:
    """Tokens, model and reported cost from any supported response shape; None if no usage."""
    d = _as_dict(response)
    model = d.get("model") or d.get("modelVersion") or ""
    meta = d.get("usageMetadata")
    if isinstance(meta, dict):  # Gemini
        read = _n(meta, "cachedContentTokenCount")
        return {"model": model, "provider": "google", "input": _n(meta, "promptTokenCount") - read,
                "output": _n(meta, "candidatesTokenCount") + _n(meta, "thoughtsTokenCount"),
                "cache_read": read, "cache_write": 0, "cost_usd": None}
    u = _as_dict(d.get("usage"))
    if not u:
        return None
    reported = u.get("cost")
    if reported is None and isinstance(u.get("cost_in_usd_ticks"), (int, float)):  # xAI
        reported = u["cost_in_usd_ticks"] / 1e10
    if "prompt_tokens" in u:  # OpenAI Chat Completions and compatible APIs
        details = _as_dict(u.get("prompt_tokens_details"))
        read = _n(details, "cached_tokens") or _n(u, "prompt_cache_hit_tokens")
        return {"model": model, "provider": None, "input": _n(u, "prompt_tokens") - read,
                "output": _n(u, "completion_tokens"), "cache_read": read, "cache_write": 0, "cost_usd": reported}
    if "cache_creation_input_tokens" in u or "cache_read_input_tokens" in u or str(model).startswith("claude"):
        return {"model": model, "provider": "anthropic", "input": _n(u, "input_tokens"),
                "output": _n(u, "output_tokens"), "cache_read": _n(u, "cache_read_input_tokens"),
                "cache_write": _n(u, "cache_creation_input_tokens"), "cost_usd": reported}
    if "input_tokens" in u:  # OpenAI Responses
        read = _n(_as_dict(u.get("input_tokens_details")), "cached_tokens")
        return {"model": model, "provider": None, "input": _n(u, "input_tokens") - read,
                "output": _n(u, "output_tokens"), "cache_read": read, "cache_write": 0, "cost_usd": reported}
    return None


def guess_provider(model: str) -> str:
    m = (model or "").lower()
    for key, prov in (("claude", "anthropic"), ("deepseek", "deepseek"), ("grok", "xai"), ("gemini", "google"),
                      ("gpt", "openai"), ("o3", "openai"), ("o4", "openai"), ("muse", "local")):
        if key in m:
            return "openrouter" if "/" in m and not m.startswith("/") else prov
    return "local" if m.startswith("/") else "?"


def record_usage(agent: str | None = None, provider: str | None = None, model: str = "", input: int = 0,
                 output: int = 0, cache_read: int = 0, cache_write: int = 0, cost_usd: float | None = None,
                 kind: str = "api", note: str | None = None) -> dict | None:
    try:
        entry = {
            "at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "agent": agent or os.environ.get("HIVEY_AGENT") or agent_of(os.getcwd()),
            "provider": provider or guess_provider(model), "model": model or "?", "kind": kind,
            "input_tokens": int(input), "output_tokens": int(output),
            "cache_read_tokens": int(cache_read), "cache_write_tokens": int(cache_write),
            "cost_usd": round(cost_usd if cost_usd is not None else cost(model, input, output, cache_read, cache_write), 6),
            "cost_source": "reported" if cost_usd is not None else "estimated",
        }
        if note:
            entry["note"] = note
        SPEND_LOG.parent.mkdir(parents=True, exist_ok=True)
        with open(SPEND_LOG, "a") as fh:
            fh.write(json.dumps(entry) + "\n")
        return entry
    except Exception as e:  # never break the caller
        print(f"hivey_spend: not logged: {e}", file=sys.stderr)
        return None


def record(response, agent: str | None = None, provider: str | None = None, model: str | None = None,
           kind: str = "api", note: str | None = None) -> dict | None:
    """Log one API response. Returns the ledger entry, or None when it carried no usage."""
    try:
        u = parse(response)
    except Exception:
        u = None
    if not u:
        return None
    m = model or u["model"]
    return record_usage(agent, provider or u["provider"], m, u["input"], u["output"], u["cache_read"],
                        u["cache_write"], u["cost_usd"], kind, note)


def main() -> int:
    import argparse
    ap = argparse.ArgumentParser(description="Log the usage of a JSON API response read from stdin "
                                             "(echoed back unchanged to stdout).")
    ap.add_argument("--agent")
    ap.add_argument("--provider")
    ap.add_argument("--model")
    ap.add_argument("--kind", default="api")
    ap.add_argument("--note")
    a = ap.parse_args()
    raw = sys.stdin.read()
    sys.stdout.write(raw)
    try:
        data = json.loads(raw)
    except ValueError:
        # a stream: the last data line carrying usage
        data = None
        for line in raw.splitlines():
            line = line.removeprefix("data:").strip()
            if '"usage' in line:
                try:
                    data = json.loads(line)
                except ValueError:
                    pass
    if data is None or not record(data, a.agent, a.provider, a.model, a.kind, a.note):
        print("hivey_spend: no usage found in the response", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
