#!/usr/bin/env python3
"""hivey agent map: what runs on this Mac, what it costs, and how the pieces feed each other.

    agent_map.py scan  [--days 7] [--out FILE]   collect everything into a snapshot (JSON)
    agent_map.py stats [--days 7]                print a summary (runs scan first)
    agent_map.py log   --agent NAME --provider P --model M [--in N] [--out N]
                       [--cache-read N] [--cache-write N] [--cost USD] [--note TEXT]
                                                 add one entry to the spend log
    agent_map.py link  FROM TO --via WHAT [--evidence TEXT] [--remove]
                                                 confirm (or remove) a link between two nodes
    agent_map.py nightly [--model haiku] [--no-llm] [--open]
                                                 a full report in ~/.hivey/agent-map/reports/<date>/
    agent_map.py open                            open the latest report (hivey menu → agent map)
    agent_map.py schedule [install|remove|status|offer] [--at 06:30] [--model haiku]
    agent_map.py schedule token --command "envsave get es_…"   long-lived Claude login for the run
                                                 the nightly launchd run (com.hivey.agent-map)

Sources (all read-only):
  scheduled jobs   crontab, ~/.config/cron-catchup/jobs.tsv (+ `cron-catchup.py list` status),
                   ~/Library/LaunchAgents/*.plist (non-Apple) with `launchctl list` exit codes
  live agents      `hivey agent list`, plus claude/codex/grok/agy processes outside hivey
  tokens           Claude Code ~/.claude/projects/**/*.jsonl (also DeepSeek/OpenRouter/Muse agents,
                   which run through Claude Code), Codex ~/.codex/sessions (.jsonl/.jsonl.zst),
                   Grok ~/.grok/sessions/*/*/usage.json, claude-api-guard ledger,
                   hivey spend log ~/.hivey/spend/ledger.jsonl (this script's `log`)
  links            heuristic: two nodes whose scripts/briefs name the same Baserow table or the
                   same data folder; confirmed links live in ~/.hivey/agent-map/links.json

Costs are API-list-price estimates (prices in hivey_spend.PRICES, overridable in ~/.hivey/agent-map/prices.json).
Claude Code on a Max plan and Codex on Plus are not billed per token: their cost is "API-equivalent".
Python standard library only.
"""
from __future__ import annotations

import argparse
import collections
import datetime as dt
import glob
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from hivey_spend import SPEND_LOG, agent_of, cost, prices, record_usage  # noqa: E402  one price table

HOME = Path.home()
STATE = HOME / ".hivey" / "agent-map"
LINKS = STATE / "links.json"
GUARD_LEDGER = HOME / ".config" / "claude-api-guard" / "ledger.jsonl"

AGENT_CLIS = {"claude": "claude-code", "codex": "codex", "grok": "grok", "agy": "antigravity", "gemini": "antigravity"}


# ---------------------------------------------------------------- helpers

def sh(cmd: list[str], timeout: int = 20) -> str:
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout).stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def load_json(path: Path, default):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return default


def short(path: str) -> str:
    return path.replace(str(HOME), "~") if path else path


def since(days: int) -> dt.datetime:
    return dt.datetime.now(dt.timezone.utc) - dt.timedelta(days=days)


def parse_ts(s: str) -> dt.datetime | None:
    try:
        t = dt.datetime.fromisoformat(s.replace("Z", "+00:00"))
        return t if t.tzinfo else t.replace(tzinfo=dt.timezone.utc)
    except (ValueError, AttributeError):
        return None


# ---------------------------------------------------------------- scheduled jobs

PATH_RE = re.compile(r"(?:~|/Users/[^/\s]+)/[\w.@%+-]+(?:/[\w.@%+-]+)*")


def paths_in(text: str) -> list[str]:
    return [os.path.expanduser(m.replace("~", str(HOME), 1) if m.startswith("~") else m) for m in PATH_RE.findall(text)]


def log_path(command: str) -> str | None:
    m = re.search(r">>?\s*(\S+\.log)", command)
    return os.path.expanduser(m.group(1)) if m else None


def log_info(path: str | None) -> dict:
    if not path or not os.path.isfile(path):
        return {}
    st = os.stat(path)
    info = {"log": short(path), "log_modified": dt.datetime.fromtimestamp(st.st_mtime).isoformat(timespec="minutes")}
    try:
        with open(path, "rb") as f:
            f.seek(max(0, st.st_size - 6000))
            tail = f.read().decode("utf-8", "replace").splitlines()[-40:]
        tail = [re.sub(r"\x1b\[[0-9;]*[A-Za-z]|\r", "", l)[:300] for l in tail]
        errs = [l.strip() for l in tail
                if re.search(r"error|traceback|exception|failed|fatal", l, re.I)
                and not re.search(r"\b0 failed|failed:? 0\b|0 errors?\b|errors?:? 0\b|rerun picks", l, re.I)]
        if errs:
            info["last_error"] = errs[-1][:200]
            info["errors"] = list(dict.fromkeys(e[:200] for e in errs))[-5:]
        info["tail"] = tail
    except OSError:
        pass
    return info


def job_agent(command: str, cwd: str | None = None) -> str:
    """The agent a job belongs to: first path in the command under a known agent root."""
    for p in ([cwd] if cwd else []) + paths_in(command):
        name = agent_of(p)
        if name.startswith(("~/", "home", "unknown", "/")) or name == "tmp":
            continue
        return name
    return ""


def cron_jobs() -> list[dict]:
    jobs = []
    catchup_status = {}
    cc = HOME / "bin" / "cron-catchup.py"
    if cc.exists():
        for line in sh([sys.executable, "-I", str(cc), "list"]).splitlines():
            m = re.match(r"(\S+)\s+\[([^\]]+)\]\s+last run (\S+ \S+)\s+last due (\S+ \S+)\s+(\S+)", line)
            if m:
                catchup_status[m.group(1)] = {"last_run": m.group(3), "last_due": m.group(4), "state": m.group(5)}
    tsv = HOME / ".config" / "cron-catchup" / "jobs.tsv"
    catchup = {}
    if tsv.exists():
        for line in tsv.read_text().splitlines():
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            parts = line.split("\t")
            if len(parts) >= 3:
                catchup[parts[0]] = (parts[1], parts[2])
    for name, (sched, cmd) in catchup.items():
        st = catchup_status.get(name, {})
        jobs.append({
            "id": f"cron:{name}", "name": name, "kind": "cron (cron-catchup)", "schedule": sched,
            "command": cmd, "agent": job_agent(cmd), "status": st.get("state", "?"),
            "last_run": st.get("last_run"), **log_info(log_path(cmd)),
        })
    for line in sh(["crontab", "-l"]).splitlines():
        s = line.strip()
        if not s or s.startswith("#") or re.match(r"^\w+=", s) or "cron-catchup.py" in s:
            continue
        f = s.split(None, 5)
        if len(f) < 6:
            continue
        sched, cmd = " ".join(f[:5]), f[5]
        name = Path(paths_in(cmd)[0]).stem if paths_in(cmd) else cmd[:30]
        jobs.append({
            "id": f"cron:{name}", "name": name, "kind": "cron", "schedule": sched, "command": cmd,
            "agent": job_agent(cmd), "status": "?", **log_info(log_path(cmd)),
        })
    return jobs


def describe_calendar(cal) -> str:
    if isinstance(cal, list):
        return "; ".join(describe_calendar(c) for c in cal)
    if not isinstance(cal, dict):
        return str(cal)
    days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
    t = f"{cal.get('Hour', '*'):0>2}:{cal.get('Minute', 0):0>2}" if "Hour" in cal else f"min {cal.get('Minute', 0)} hourly"
    if "Weekday" in cal:
        t = f"{days[cal['Weekday'] % 7]} {t}"
    if "Day" in cal:
        t = f"day {cal['Day']} {t}"
    return t if "Weekday" in cal or "Day" in cal else f"daily {t}"


def calendar_period_hours(cal) -> float | None:
    """How long a launchd calendar job may go without running (plus slack), or None."""
    if isinstance(cal, list):
        periods = [calendar_period_hours(c) for c in cal]
        return min((p for p in periods if p), default=None)
    if not isinstance(cal, dict):
        return None
    if "Day" in cal:
        return 32 * 24
    if "Weekday" in cal:
        return 8 * 24
    return 26 if "Hour" in cal else 2


def launchd_jobs() -> list[dict]:
    status = {}
    for line in sh(["launchctl", "list"]).splitlines()[1:]:
        f = line.split("\t")
        if len(f) == 3:
            status[f[2]] = (f[0], f[1])
    jobs = []
    for plist in sorted(glob.glob(str(HOME / "Library/LaunchAgents/*.plist"))):
        try:
            with open(plist, "rb") as fh:
                d = plistlib.load(fh)
        except Exception:
            continue
        label = d.get("Label", Path(plist).stem)
        if label.startswith(("com.apple.", "com.google.", "com.github.facebook")) or "updater" in label.lower():
            continue
        args = d.get("ProgramArguments") or [d.get("Program", "")]
        cmd = " ".join(args)
        if "StartCalendarInterval" in d:
            sched = describe_calendar(d["StartCalendarInterval"])
        elif "StartInterval" in d:
            sched = f"every {d['StartInterval']}s"
        elif d.get("KeepAlive") or d.get("RunAtLoad"):
            sched = "always on" if d.get("KeepAlive") else "at login"
        else:
            sched = "on demand"
        period_h = calendar_period_hours(d.get("StartCalendarInterval"))
        pid, code = status.get(label, ("-", "?"))
        state = "running" if pid != "-" else ("not loaded" if label not in status else ("ok" if code == "0" else f"exit {code}"))
        jobs.append({
            "id": f"launchd:{label}", "name": re.sub(r"^com\.(jcsancho|hivey)\.", "", label), "kind": "launchd",
            "schedule": sched, "command": cmd, "agent": job_agent(cmd, d.get("WorkingDirectory")),
            "status": state, **log_info(d.get("StandardOutPath") or d.get("StandardErrorPath")),
        })
        mod = jobs[-1].get("log_modified")
        if period_h and mod and dt.datetime.now() - dt.datetime.fromisoformat(mod) > dt.timedelta(hours=period_h):
            jobs[-1]["overdue"] = True  # its log did not change since the last time it should have run
    return jobs


# ---------------------------------------------------------------- live agents

def agent_logs() -> dict:
    """Newest log in each agent's or swarm's logs/ folder."""
    out = {}
    for d in sorted(glob.glob(str(HOME / "agents/*/"))) + sorted(glob.glob(str(HOME / "swarms/*/"))):
        if re.search(r"backup|-old|\.bak", d):
            continue
        logs = [p for p in glob.glob(os.path.join(d, "logs", "**", "*"), recursive=True)
                if os.path.isfile(p) and p.endswith((".log", ".txt", ".jsonl", ".out"))]
        if logs:
            out[agent_of(d)] = log_info(max(logs, key=os.path.getmtime))
    return out


def live_agents() -> list[dict]:
    out = []
    seen_cwds = set()
    hv = shutil.which("hivey")
    if hv:
        try:
            data = json.loads(sh([hv, "agent", "list"]) or "{}")
            for a in data.get("result", {}).get("agents", []):
                cwd = a.get("foreground_cwd") or a.get("cwd")
                seen_cwds.add(cwd)
                out.append({
                    "agent": agent_of(cwd), "cli": AGENT_CLIS.get(a.get("agent"), a.get("agent")),
                    "status": a.get("agent_status", "?"), "cwd": short(cwd), "where": f"hivey {a.get('pane_id')}",
                    "role": (a.get("tokens") or {}).get("role"), "session": (a.get("agent_session") or {}).get("value"),
                    "title": a.get("title") or a.get("terminal_title_stripped"),
                })
        except ValueError:
            pass
    for line in sh(["ps", "-axo", "pid=,etime=,command="]).splitlines():
        f = line.split(None, 2)
        if len(f) < 3:
            continue
        pid, etime, cmd = f
        exe = os.path.basename(cmd.split()[0])
        if exe not in AGENT_CLIS or "--version" in cmd or ".app/" in cmd:
            continue
        cwd = ""
        for l in sh(["lsof", "-a", "-p", pid, "-d", "cwd", "-Fn"], timeout=5).splitlines():
            if l.startswith("n"):
                cwd = l[1:]
        if cwd in seen_cwds:
            continue
        headless = " -p " in f" {cmd} " or " exec " in f" {cmd} "
        out.append({
            "agent": agent_of(cwd), "cli": AGENT_CLIS[exe], "status": "running", "cwd": short(cwd),
            "where": f"pid {pid}" + (" (headless)" if headless else ""), "uptime": etime,
        })
    return out


# ---------------------------------------------------------------- tokens

def blank():
    return {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0, "cost_usd": 0.0, "calls": 0}


class Usage:
    def __init__(self, days: int):
        self.cutoff = since(days)
        self.table = prices()
        self.rows = []  # (day, agent, provider, model, kind, usage dict)
        self.last_seen: dict[str, dt.datetime] = {}

    def add(self, ts, agent, provider, model, kind, inp=0, out=0, read=0, write=0, cost_usd=None, calls=1):
        if ts is None or ts < self.cutoff:
            return
        c = cost(model, inp, out, read, write, self.table) if cost_usd is None else cost_usd
        if ts > self.last_seen.get(agent, ts.min.replace(tzinfo=dt.timezone.utc)):
            self.last_seen[agent] = ts
        self.rows.append((ts.astimezone().date().isoformat(), agent, provider, model or "?", kind,
                          {"input": inp, "output": out, "cache_read": read, "cache_write": write,
                           "cost_usd": c, "calls": calls}))

    def claude_code(self):
        seen = set()
        cutoff_epoch = self.cutoff.timestamp()
        for f in glob.glob(str(HOME / ".claude/projects/*/**/*.jsonl"), recursive=True):
            if "/memory/" in f or os.path.getmtime(f) < cutoff_epoch:
                continue
            try:
                fh = open(f, encoding="utf-8", errors="replace")
            except OSError:
                continue
            with fh:
                for line in fh:
                    if '"usage"' not in line:
                        continue
                    try:
                        d = json.loads(line)
                    except ValueError:
                        continue
                    msg = d.get("message") or {}
                    u = msg.get("usage")
                    if not isinstance(u, dict) or msg.get("model") == "<synthetic>":
                        continue
                    key = (msg.get("id"), d.get("requestId"))
                    if key[0] and key in seen:
                        continue
                    seen.add(key)
                    model = msg.get("model", "")
                    provider = ("openrouter" if "deepseek" in model or "/" in model and not model.startswith("/")
                                else "local" if model.startswith("/") or "muse" in model else "anthropic")
                    kind = "headless" if d.get("entrypoint") in ("sdk-cli", "sdk-ts", "sdk-py") else "interactive"
                    self.add(parse_ts(d.get("timestamp", "")), agent_of(d.get("cwd")), provider, model,
                             f"claude-code {kind}", u.get("input_tokens", 0) or 0, u.get("output_tokens", 0) or 0,
                             u.get("cache_read_input_tokens", 0) or 0, u.get("cache_creation_input_tokens", 0) or 0)

    def codex(self):
        cutoff_epoch = self.cutoff.timestamp()
        zstd = shutil.which("zstd")
        for f in glob.glob(str(HOME / ".codex/sessions/**/*.jsonl*"), recursive=True):
            if os.path.getmtime(f) < cutoff_epoch:
                continue
            if f.endswith(".zst"):
                if not zstd:
                    continue
                text = sh([zstd, "-dcq", f], timeout=30)
            else:
                try:
                    text = Path(f).read_text(errors="replace")
                except OSError:
                    continue
            cwd, model, last, ts = None, "", None, None
            for line in text.splitlines():
                if '"session_meta"' in line or '"turn_context"' in line or '"token_count"' in line:
                    try:
                        d = json.loads(line)
                    except ValueError:
                        continue
                    p = d.get("payload") or {}
                    if d.get("type") == "session_meta":
                        cwd = p.get("cwd")
                    elif d.get("type") == "turn_context":
                        model = p.get("model") or model
                        cwd = p.get("cwd") or cwd
                    elif p.get("type") == "token_count" and p.get("info"):
                        last = p["info"].get("total_token_usage")
                        ts = parse_ts(d.get("timestamp", ""))
            if last:
                cached = last.get("cached_input_tokens", 0) or 0
                self.add(ts, agent_of(cwd), "openai", model or "codex", "codex",
                         (last.get("input_tokens", 0) or 0) - cached, last.get("output_tokens", 0) or 0, cached,
                         last.get("cache_write_input_tokens", 0) or 0)

    def grok(self):
        for f in glob.glob(str(HOME / ".grok/sessions/*/*/usage.json")):
            d = load_json(Path(f), {})
            s = d.get("session") or {}
            folder = Path(f).parent.parent.name.replace("%2F", "/")
            ticks = s.get("costUsdTicks")
            self.add(parse_ts(d.get("updatedAt", "")), agent_of(folder), "xai", s.get("primaryModelId", "grok"),
                     "grok cli", s.get("inputTokens", 0) - s.get("cachedReadTokens", 0), s.get("outputTokens", 0),
                     s.get("cachedReadTokens", 0), s.get("cacheCreationTokens", 0),
                     cost_usd=ticks / 1e10 if ticks is not None else None, calls=s.get("modelCalls", 1))

    def ledgers(self):
        for path, provider_default in ((GUARD_LEDGER, "anthropic"), (SPEND_LOG, None)):
            if not path.exists():
                continue
            for line in path.read_text(errors="replace").splitlines():
                try:
                    d = json.loads(line)
                except ValueError:
                    continue
                model = d.get("model", "")
                if model == "claude-code":
                    continue  # already counted from the Claude Code transcripts
                agent = d.get("agent") or (d.get("source") or "api").split(":")[0]
                self.add(parse_ts(d.get("at", "")), agent, d.get("provider") or provider_default or "?", model,
                         "api" if path == GUARD_LEDGER else d.get("kind", "logged"),
                         d.get("input_tokens", 0) or 0, d.get("output_tokens", 0) or 0,
                         d.get("cache_read_tokens", 0) or 0, d.get("cache_write_tokens", 0) or 0,
                         cost_usd=d.get("cost_usd"))

    def collect(self):
        for step in (self.claude_code, self.codex, self.grok, self.ledgers):
            try:
                step()
            except Exception as e:  # one broken source must not hide the others
                print(f"warning: {step.__name__}: {e}", file=sys.stderr)
        return self

    def summary(self) -> dict:
        def group(keyf):
            g = collections.defaultdict(blank)
            for row in self.rows:
                acc = g[keyf(row)]
                for k, v in row[5].items():
                    acc[k] += v
            return g
        by_agent = group(lambda r: r[1])
        agents = []
        for name, tot in sorted(by_agent.items(), key=lambda kv: -kv[1]["cost_usd"]):
            models = collections.Counter()
            providers = collections.Counter()
            for r in self.rows:
                if r[1] == name:
                    models[r[3]] += r[5]["cost_usd"] + 1e-12
                    providers[r[2]] += r[5]["cost_usd"] + 1e-12
            seen = self.last_seen.get(name)
            agents.append({"agent": name, "last_active": seen.astimezone().isoformat(timespec="minutes") if seen else None,
                           **{k: round(v, 4) if isinstance(v, float) else v for k, v in tot.items()},
                           "models": [m for m, _ in models.most_common()], "providers": [p for p, _ in providers.most_common()]})
        day_agent = collections.defaultdict(lambda: collections.defaultdict(blank))
        for r in self.rows:
            acc = day_agent[r[0]][r[1]]
            for k, v in r[5].items():
                acc[k] += v
        daily_agents = {day: sorted(({"agent": a, "cost_usd": round(v["cost_usd"], 4), "calls": v["calls"],
                                      "tokens": v["input"] + v["output"] + v["cache_read"] + v["cache_write"]}
                                     for a, v in per.items()), key=lambda x: -x["cost_usd"])
                        for day, per in sorted(day_agent.items())}
        daily = collections.defaultdict(dict)
        for (day, prov), tot in group(lambda r: (r[0], r[2])).items():
            daily[day][prov] = round(tot["cost_usd"], 4)
        return {
            "agents": agents,
            "providers": {k: {kk: round(vv, 4) if isinstance(vv, float) else vv for kk, vv in v.items()}
                          for k, v in group(lambda r: r[2]).items()},
            "models": {k: {kk: round(vv, 4) if isinstance(vv, float) else vv for kk, vv in v.items()}
                       for k, v in group(lambda r: r[3]).items()},
            "kinds": {k: round(v["cost_usd"], 4) for k, v in group(lambda r: r[4]).items()},
            "daily_cost_by_provider": dict(sorted(daily.items())),
            "daily_by_agent": daily_agents,
            "total_cost_usd": round(sum(r[5]["cost_usd"] for r in self.rows), 2),
            "total_tokens": sum(r[5]["input"] + r[5]["output"] + r[5]["cache_read"] + r[5]["cache_write"] for r in self.rows),
            "sources_without_tokens": ["antigravity (agy): no per-session token data on disk"],
        }


# ---------------------------------------------------------------- descriptions

DESCRIPTIONS = STATE / "descriptions.json"  # {node id: "what it does"}: written by the nightly model or you


def first_sentence(text: str, limit: int = 170) -> str:
    text = re.sub(r"`|\*\*|__|\[([^\]]*)\]\([^)]*\)", lambda m: m.group(1) or "", text)
    text = " ".join(text.split())
    m = re.match(r"(.+?[.!?])(\s|$)", text)
    out = m.group(1) if m else text
    return out if len(out) <= limit else out[: limit - 1].rsplit(" ", 1)[0] + "…"


def describe_markdown(path: Path) -> str:
    """A brief's mission (or its first real paragraph), or a SKILL.md description."""
    try:
        text = path.read_text(errors="replace")
    except OSError:
        return ""
    fm = re.match(r"---\n(.*?)\n---", text, re.S)
    if fm:
        m = re.search(r"^description:\s*\"?(.+?)\"?\s*$", fm.group(1), re.M)
        if m:
            return first_sentence(m.group(1))
        text = text[fm.end():]
    text = re.sub(r"<!-- BEGIN (GUIDELINES|GLOBAL[^>]*)-->.*?<!-- END (GUIDELINES|GLOBAL)[^>]*-->", "", text, flags=re.S)
    text = re.sub(r"^# General guidelines.*?(?=^# (?!General))", "", text, flags=re.S | re.M)
    m = re.search(r"^#{1,3}\s*(your mission|mission|purpose|what (this|it) does)\b.*?\n(.+?)(\n#|\Z)",
                  text, re.I | re.M | re.S)
    body = m.group(3) if m else text
    if BOILERPLATE.search(body[:300]) and m is None:
        return ""  # only the global guidelines: nothing about this agent
    for para in re.split(r"\n\s*\n", body):
        p = re.sub(r"^\s*(\d+\.|[-*])\s+", "", para.strip())  # a mission written as a list: its first item
        if p and not p.startswith(("#", ">", "<!--", "|", "```")) and len(p) > 25 and not BOILERPLATE.search(p):
            return first_sentence(p.split("\n")[0] if re.match(r"\s*(\d+\.|[-*])\s", para) else p)
    return ""


BOILERPLATE = re.compile(r"behavioral guidelines|general guidelines|think before coding|you are claude|"
                         r"before implementing|state your assumptions|always end your final response|"
                         r"^(you are|i am) (the )?[\w -]{1,30}( agent)?\.?$|"
                         r"the user's (original )?request, verbatim|the user's words", re.I)


def describe_script(path: Path) -> str:
    """The module docstring or leading comment block of a script (not a function's docstring)."""
    try:
        lines = path.read_text(errors="replace")[:6000].splitlines()
    except OSError:
        return ""
    i, comments = 0, []
    while i < len(lines):
        l = lines[i].strip()
        if l.startswith("#!") or l.startswith("# ///") or (comments == [] and re.match(r"#\s*(requires-python|dependencies|\]|\[|\")", l)):
            i += 1
            continue
        if l.startswith("#"):
            text = l.lstrip("# ").strip()
            if text and not text.startswith(("-*-", "///", "noqa")):
                comments.append(text)
            i += 1
            continue
        if not l:
            i += 1
            if comments:
                break
            continue
        break
    if i < len(lines) and re.match(r'^[rub]*("""|\'\'\')', lines[i].strip()):
        q = lines[i].strip().lstrip("rub")[:3]
        body = [lines[i].strip()[len(q) + (len(lines[i].strip()) - len(lines[i].strip().lstrip("rub"))):]]
        j = i + 1
        while j < len(lines) and q not in body[-1]:
            body.append(lines[j].strip())
            j += 1
        text = " ".join(body).split(q)[0]
        para = re.split(r"\s{2,}|\n\s*\n", text.strip())[0]
        if para and not BOILERPLATE.search(para):
            return first_sentence(para)
    joined = " ".join(comments)
    return first_sentence(joined) if len(joined) > 20 and not BOILERPLATE.search(joined) else ""


def describe_job(j: dict) -> str:
    for p in job_run_files(j["command"]):
        pp = Path(p)
        if pp.parent.name == "scripts" and (pp.parent.parent / "SKILL.md").is_file():
            d = describe_markdown(pp.parent.parent / "SKILL.md")
            if d:
                return d
        d = describe_script(pp)
        if d:
            return d
    m = re.search(r'claude\b.*?-p\s+["\']/([\w-]+)', j["command"])
    if m and (HOME / ".claude/skills" / m.group(1) / "SKILL.md").is_file():
        return describe_markdown(HOME / ".claude/skills" / m.group(1) / "SKILL.md")
    return ""


# ---------------------------------------------------------------- links

TABLE_RES = [
    re.compile(r'"(\w*table\w*)"\s*:\s*(\d{3,})', re.I),
    re.compile(r"\b([A-Z_]*TABLE[A-Z_]*)\s*=\s*['\"]?(\d{3,})"),
    re.compile(r"rows/table/(\d{3,})"),
]
NOISE_ROOTS = ("bin", "Library", "Downloads", "Documents", "Pictures", "Desktop", "SKILLS", "out", "models", "go")
SKIP_DIRS = ("/logs", "/.local", "/.cache", "/node_modules", "/.git", "/.venv", "/__pycache__")


def node_files(paths: list[str]) -> list[Path]:
    """Script files of a node, and their siblings (scripts, configs, briefs), bounded."""
    files: list[Path] = []
    for p in paths:
        pp = Path(p)
        if pp.is_dir():
            cands = [pp / "CLAUDE.md", pp / "AGENTS.md"] + sorted(pp.glob("scripts/*"))
        elif pp.is_file() and pp.suffix in (".py", ".sh", ".json", ".md", ".toml", ".ts", ".js", ""):
            cands = [pp]
            if pp.parent.name == "scripts":  # a skill's or agent's scripts folder: its siblings are one unit
                cands += sorted(pp.parent.glob("*"))[:40]
                cands += [pp.parent.parent / "SKILL.md", pp.parent.parent / "CLAUDE.md"]
        else:
            continue
        for c in cands:
            if c.is_file() and c.suffix in (".py", ".sh", ".json", ".md", ".toml", ".ts", ".js") and c.stat().st_size < 400_000:
                files.append(c)
    return list(dict.fromkeys(files))[:80]


def resources_of(paths: list[str], own_dir: str | None) -> dict:
    res: dict[str, str] = {}
    for f in node_files(paths):
        try:
            text = f.read_text(errors="replace")
        except OSError:
            continue
        for rx in TABLE_RES:
            for m in rx.finditer(text):
                tid = m.groups()[-1]
                label = m.group(1) if len(m.groups()) > 1 else "table"
                res.setdefault(f"baserow:table/{tid}", f"{label} in {short(str(f))}")
        for p in paths_in(text):
            if any(s in p for s in SKIP_DIRS) or p.endswith((".log", ".py", ".sh", ".plist")):
                continue
            rel = os.path.relpath(p, HOME).split(os.sep)
            if rel[0].startswith(".") or rel[0] in NOISE_ROOTS:
                continue  # dotfolders, tool installs, downloads, trash: not data that flows
            depth = 3 if rel[0] in ("agents", "swarms", "projects") else 2
            if len(rel) < depth:
                continue  # a bare root like ~/agents says nothing
            if "." in rel[depth - 1]:
                continue  # a single file (cookies, bundles), not a shared data folder
            key = "~/" + "/".join(rel[:depth])
            full = os.path.realpath(os.path.join(HOME, key[2:]))
            if own_dir and (full + os.sep).startswith(os.path.realpath(own_dir) + os.sep):
                continue  # inside the node's own folder: not a link to anyone
            res.setdefault(f"dir:{key}", short(str(f)))
    return res


API_MARKERS = {
    "anthropic": r"api\.anthropic\.com|anthropic\.Anthropic\(|messages\.create\(",
    "openai": r"api\.openai\.com|OpenAI\(|chat\.completions\.create|responses\.create\(",
    "openrouter": r"openrouter\.ai/api",
    "xai": r"api\.x\.ai",
    "deepseek": r"api\.deepseek\.com",
    "google": r"generativelanguage\.googleapis|google\.genai|genai\.Client\(",
    "claude-cli": r"\bclaude\b[^\n]{0,40}\s-p\b",
}


def run_files(paths: list[str]) -> list[Path]:
    """The files a node actually runs: its referenced scripts, the local modules they import and the
    scripts they call (one level). Agents (folders): their scripts folder."""
    files: list[Path] = []
    for p in paths:
        pp = Path(p)
        if pp.is_dir():
            files += [f for f in sorted(pp.glob("scripts/*")) if f.is_file()][:40]
        elif pp.is_file() and pp.suffix in (".py", ".sh", ".ts", ".js", ""):
            files.append(pp)
    for f in list(files):
        try:
            text = f.read_text(errors="replace")
        except (OSError, UnicodeError):
            continue
        for mod in re.findall(r"^\s*(?:from|import)\s+([\w.]+)", text, re.M):
            cand = f.parent / (mod.split(".")[0] + ".py")
            if cand.is_file():
                files.append(cand)
        for q in paths_in(text):
            if q.endswith((".py", ".sh")) and os.path.isfile(q):
                files.append(Path(q))
    return [f for f in dict.fromkeys(files) if f.stat().st_size < 400_000][:60]


def api_usage(paths: list[str], command: str = "") -> dict:
    """Which LLM APIs a node's code calls, and which of those already log their spend
    (hivey_spend logs any provider; claude-api-guard only Anthropic; claude -p is in the transcripts)."""
    providers, logged_by, files = set(), set(), []
    if re.search(API_MARKERS["claude-cli"], command):
        providers.add("claude-cli")
        files.append("(job command)")
    for f in run_files(paths):
        try:
            text = f.read_text(errors="replace")
        except OSError:
            continue
        hit = {p for p, rx in API_MARKERS.items() if re.search(rx, text)}
        if hit:
            providers |= hit
            files.append(short(str(f)))
        if "hivey_spend" in text:
            logged_by.add("all")
        if re.search(r"claude[-_]api[-_]guard", text):
            logged_by.add("anthropic")
    if not providers:
        return {}
    unlogged = sorted(p for p in providers if p != "claude-cli" and "all" not in logged_by and p not in logged_by)
    return {"providers": sorted(providers), "unlogged": unlogged, "files": files[:6]}


def job_run_files(command: str) -> list[str]:
    """Script files a job command runs, relative ones resolved against its `cd DIR`."""
    m = re.search(r"\bcd\s+(\S+)", command)
    cwd = os.path.expanduser(m.group(1)) if m else str(HOME)
    out = []
    for tok in re.split(r"[\s;&|<>()]+", command):
        tok = tok.strip("'\"")
        if not tok.endswith((".py", ".sh", ".ts", ".js")):
            continue
        p = os.path.expanduser(tok) if tok.startswith(("~", "/")) else os.path.join(cwd, tok)
        if os.path.isfile(p):
            out.append(p)
    return out or [p for p in paths_in(command) if os.path.isfile(p)]


def build_links(jobs: list[dict], live: list[dict]) -> tuple[list[dict], dict]:
    nodes: dict[str, dict] = {}
    for j in jobs:
        nodes[j["id"]] = {"id": j["id"], "label": j["name"], "type": "job", "agent": j.get("agent"),
                          "paths": paths_in(j["command"]), "run": job_run_files(j["command"]), "command": j["command"]}
    agent_dirs = [d for d in sorted(glob.glob(str(HOME / "agents/*/"))) + sorted(glob.glob(str(HOME / "swarms/*/")))
                  if not re.search(r"backup|-old|\.bak", d)]
    for d in agent_dirs:
        name = agent_of(d)
        nodes[f"agent:{name}"] = {"id": f"agent:{name}", "label": name, "type": "agent", "agent": name,
                                   "paths": [d.rstrip("/")], "dir": d.rstrip("/")}
    for a in live:
        nid = f"agent:{a['agent']}"
        nodes.setdefault(nid, {"id": nid, "label": a["agent"], "type": "agent", "agent": a["agent"], "paths": []})
    for n in nodes.values():
        u = api_usage(n["run"], n["command"]) if n["type"] == "job" else api_usage(n["paths"])
        if u:
            n["llm_api"] = u
    touches: dict[str, list[tuple[str, str]]] = collections.defaultdict(list)
    for n in nodes.values():
        for r, ev in resources_of(n["paths"], n.get("dir")).items():
            touches[r].append((n["id"], ev))
    # a job that runs inside an agent's folder belongs to that agent
    belongs = [{"job": n["id"], "agent": f"agent:{n['agent']}"} for n in nodes.values()
               if n["type"] == "job" and n.get("agent") and f"agent:{n['agent']}" in nodes]
    stores = []
    for r, who in sorted(touches.items()):
        owner = None
        if r.startswith("dir:"):
            full = os.path.realpath(os.path.join(HOME, r[6:]))
            for n in nodes.values():
                if n.get("dir") and (full + os.sep).startswith(os.path.realpath(n["dir"]) + os.sep):
                    owner = n["id"]
        users = list(dict.fromkeys(i for i, _ in who))
        if len(set(users) | ({owner} if owner else set())) < 2 or len(users) > 8:
            continue  # used by one node only, or so common it is noise
        stores.append({"resource": r, "owner": owner,
                       "nodes": [{"id": i, "evidence": ev} for i, ev in who]})
    return list(nodes.values()), {"belongs_to": belongs, "shared_stores": stores}


# ---------------------------------------------------------------- commands

def scan(days: int) -> dict:
    jobs = cron_jobs() + launchd_jobs()
    live = live_agents()
    usage = Usage(days).collect().summary()
    nodes, candidates = build_links(jobs, live)
    overrides = load_json(DESCRIPTIONS, {})
    for j in jobs:
        j["description"] = overrides.get(j["id"]) or describe_job(j)
    by_job = {j["id"]: j["description"] for j in jobs}
    for n in nodes:
        if n["id"] in overrides:
            n["description"] = overrides[n["id"]]
        elif n["type"] == "job":
            n["description"] = by_job.get(n["id"], "")
        elif n.get("dir"):
            n["description"] = next((d for d in (describe_markdown(Path(n["dir"]) / f) for f in
                                                 ("CLAUDE.md", "AGENTS.md", "README.md")) if d), "")
    for n in nodes:
        n.pop("paths", None)
        n.pop("run", None)
        n.pop("command", None)
    return {
        "generated": dt.datetime.now().isoformat(timespec="seconds"),
        "window_days": days,
        "jobs": jobs,
        "live_agents": live,
        "agent_logs": agent_logs(),
        "usage": usage,
        "nodes": nodes,
        "links_confirmed": load_json(LINKS, []),
        "links_candidates": candidates,
        "notes": [
            "Costs are API-list-price estimates; Claude Code (Max) and Codex (Plus) usage is API-equivalent, not billed.",
            "Claude Code transcripts older than the weekly purge (15 days) are gone; longer windows undercount.",
            "Candidate links are heuristic: shared_stores lists Baserow tables and data folders named by 2+ nodes (or by one node and the folder owner). Read the evidence files to decide who writes and who reads before drawing.",
        ],
    }


def fmt_money(v: float) -> str:
    return f"${v:,.2f}"


def fmt_tok(n: int) -> str:
    return f"{n / 1e6:.1f}M" if n >= 1e6 else f"{n / 1e3:.0f}k" if n >= 1e3 else str(n)


def print_stats(snap: dict) -> None:
    u = snap["usage"]
    print(f"hivey agent map — last {snap['window_days']} days — {snap['generated']}\n")
    print(f"Scheduled jobs: {len(snap['jobs'])}   live agents: {len(snap['live_agents'])}   "
          f"est. cost: {fmt_money(u['total_cost_usd'])}   tokens: {fmt_tok(u['total_tokens'])}\n")
    print("Cost by provider")
    for p, v in sorted(u["providers"].items(), key=lambda kv: -kv[1]["cost_usd"]):
        print(f"  {p:12} {fmt_money(v['cost_usd']):>10}  {v['calls']:>7} calls  out {fmt_tok(v['output'])}")
    print("\nTop agents by cost")
    for a in u["agents"][:15]:
        tok = a["input"] + a["output"] + a["cache_read"] + a["cache_write"]
        print(f"  {a['agent'][:28]:28} {fmt_money(a['cost_usd']):>10}  {fmt_tok(tok):>7} tok  {', '.join(a['models'][:2])}")
    print("\nJobs needing attention")
    bad = [j for j in snap["jobs"] if j["status"] not in ("ok", "?", "running") or j.get("last_error")]
    for j in bad:
        print(f"  {j['name']:28} {j['status']:10} {j.get('last_error', '')[:70]}")
    if not bad:
        print("  none")
    print("\nCalls LLM APIs without logging spend (claude -p runs are counted from transcripts)")
    unlogged = [n for n in snap["nodes"] if (n.get("llm_api") or {}).get("unlogged")]
    for n in unlogged:
        print(f"  {n['label'][:28]:28} {', '.join(n['llm_api']['unlogged']):24} {n['llm_api']['files'][0]}")
    if not unlogged:
        print("  none")
    print("\nRunning now")
    for a in snap["live_agents"]:
        if a["status"] in ("working", "running", "blocked"):
            print(f"  {a['agent'][:28]:28} {a['cli']:12} {a['status']:8} {a['where']}")
    c = snap["links_candidates"]
    print(f"\nLinks: {len(snap['links_confirmed'])} confirmed; candidates: {len(c['shared_stores'])} shared stores, "
          f"{len(c['belongs_to'])} jobs inside agent folders")
    for st in c["shared_stores"][:25]:
        owner = f" (owner {st['owner']})" if st["owner"] else ""
        print(f"  {st['resource']}{owner}: " + ", ".join(dict.fromkeys(n['id'] for n in st['nodes'])))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("scan")
    s.add_argument("--days", type=int, default=7)
    s.add_argument("--out", default=str(STATE / "snapshot.json"))
    st = sub.add_parser("stats")
    st.add_argument("--days", type=int, default=7)
    lg = sub.add_parser("log")
    lg.add_argument("--agent", required=True)
    lg.add_argument("--provider", required=True)
    lg.add_argument("--model", required=True)
    lg.add_argument("--in", dest="inp", type=int, default=0)
    lg.add_argument("--out", type=int, default=0)
    lg.add_argument("--cache-read", type=int, default=0)
    lg.add_argument("--cache-write", type=int, default=0)
    lg.add_argument("--cost", type=float)
    lg.add_argument("--kind", default="logged")
    lg.add_argument("--note")
    nt = sub.add_parser("nightly", help="scan, stats page, map update with a cheap model, reports/latest")
    nt.add_argument("--days", type=int, default=7)
    nt.add_argument("--model", default="haiku")
    nt.add_argument("--no-llm", action="store_true", help="no model run: last night's map, unchanged")
    nt.add_argument("--open", action="store_true", help="open the report when done")
    en = sub.add_parser("enhance", help="re-apply hivey's layer (navbar, colours, info) to a report")
    en.add_argument("folder", nargs="?", default=str(STATE / "reports" / "latest"))
    op = sub.add_parser("open", help="open the latest report (builds the first one if there is none)")
    op.add_argument("--model", default="haiku")
    sc = sub.add_parser("schedule", help="the nightly launchd run: install | remove | status")
    sc.add_argument("action", choices=["install", "remove", "status", "offer", "token"], nargs="?", default="status")
    sc.add_argument("--command", help='token: the command printing the `claude setup-token` token, '
                                      'e.g. "envsave get es_…" ("" = the normal claude login)')
    sc.add_argument("--at", default="06:30", help="HH:MM, local time (default 06:30, after the night's jobs)")
    sc.add_argument("--model", default="haiku")
    lk = sub.add_parser("link")
    lk.add_argument("src")
    lk.add_argument("dst")
    lk.add_argument("--via", default="")
    lk.add_argument("--evidence", default="")
    lk.add_argument("--remove", action="store_true")
    a = ap.parse_args()

    if a.cmd in ("scan", "stats"):
        snap = scan(a.days)
        STATE.mkdir(parents=True, exist_ok=True)
        out = Path(getattr(a, "out", STATE / "snapshot.json"))
        out.write_text(json.dumps(snap, indent=1, default=str))
        if a.cmd == "stats":
            print_stats(snap)
        print(f"snapshot: {short(str(out))}", file=sys.stderr if a.cmd == "stats" else sys.stdout)
        return 0
    if a.cmd == "nightly":
        import report
        code = report.nightly(scan, a.days, a.model, not a.no_llm)
        if a.open:
            report.open_latest(a.model)
        return code
    if a.cmd == "enhance":
        import report, enhance
        folder = Path(a.folder).expanduser().resolve()
        enhance.enhance_folder(folder, json.loads((folder / "snapshot.json").read_text()), report.job_state)
        print(f"enhanced {folder}")
        return 0
    if a.cmd == "open":
        import report
        return report.open_latest(a.model)
    if a.cmd == "schedule":
        import report
        if a.action == "token":
            if a.command is None:
                sc.error('token needs --command "envsave get es_…" (or "" for the normal login)')
            return report.set_token_command(a.command)
        hh, _, mm = a.at.partition(":")
        return report.schedule(a.action, int(hh), int(mm or 0), a.model)
    if a.cmd == "log":
        entry = record_usage(a.agent, a.provider, a.model, a.inp, a.out, a.cache_read, a.cache_write,
                             a.cost, a.kind, a.note)
        print(json.dumps(entry))
        return 0 if entry else 1
    if a.cmd == "link":
        links = [l for l in load_json(LINKS, []) if not (l["from"] == a.src and l["to"] == a.dst)]
        if not a.remove:
            links.append({"from": a.src, "to": a.dst, "via": a.via, "evidence": a.evidence,
                          "confirmed": dt.date.today().isoformat()})
        STATE.mkdir(parents=True, exist_ok=True)
        LINKS.write_text(json.dumps(links, indent=1))
        print(f"{len(links)} confirmed links in {short(str(LINKS))}")
        return 0
    return 2


if __name__ == "__main__":
    sys.exit(main())
