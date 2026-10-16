#!/usr/bin/env python3
"""Scaffold (or update) the folder of one hivey solo agent.

Writes, inside the agent folder only (never the global Claude Code config):
  CLAUDE.md               a skeleton with the brief's sections and Status markers, if missing
  .claude/settings.json   additionalDirectories (the folder + --dirs; hivey launch turns them
                          into --add-dir), allow/deny rules (git push is always denied),
                          enabledMcpjsonServers, and with --gate the done-gate (Stop) and
                          resume-context (SessionStart) hooks
  .mcp.json               project-scoped MCP servers given with --mcp
  .codex/config.toml      the same MCP servers for a Codex agent
  .agent/config.json      the run check (--check-cmd) and optional progress log
  .claude/agents/reviewer.md   with --reviewer (Claude Code only)
  .gitignore              keeps files that may hold keys and runtime state out of git

Idempotent: re-running only adds. Skills are not installed here: follow `hivey skills guide`.
Standard library only. Prints a JSON report.
"""
import argparse
import json
import re
import shutil
import subprocess
from pathlib import Path

HERE = Path(__file__).absolute().parent  # keep the installed path (hooks point at it)
PROJECT_HOOKS = {  # event -> script registered in the agent's own .claude/settings.json
    "Stop": HERE / "agent_gate.py",
    "SessionStart": HERE / "agent_context.py",
}
NAME_RE = re.compile(r"^[a-z][a-z0-9-]{0,31}$")
KINDS = ("claude", "codex", "grok", "agy")
ALWAYS_DENY = ["Bash(git push:*)"]
GITIGNORE = [".mcp.json", ".codex/", ".agent/state/", "logs/"]

SKELETON = """# {name} — <one-line role>

> You are **{name}**, a solo hivey agent (address `{name}/master`). This file defines your role.

## Rules

## Start of every session

## The ask

## Your mission

## Deliverables
| # | Deliverable | Path | Done when | Check |
|---|-------------|------|-----------|-------|

## Where you work
- Home: `{dir}`

## Your toolkit

## Method

## Run protocol (every run, on demand or scheduled)

## Memory

## Other agents

## Definition of done

---

## Status
<!-- STATUS:START -->
_Not started yet._
<!-- STATUS:END -->
"""


def add_unique(lst, items):
    for item in items:
        if item not in lst:
            lst.append(item)


def read_json(path):
    if not path.exists():
        return {}
    try:
        return json.loads(path.read_text())
    except json.JSONDecodeError as err:
        raise SystemExit(f"{path} is not valid JSON ({err}); fix it first")


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + "\n")


def toml_str(value):
    return json.dumps(str(value))  # JSON string escapes are valid TOML basic strings


def codex_mcp_tables(servers):
    """Render .mcp.json server entries as Codex [mcp_servers.<name>] TOML tables."""
    out = []
    for name, cfg in servers.items():
        out.append(f"[mcp_servers.{toml_str(name)}]")
        if cfg.get("url"):
            out.append(f"url = {toml_str(cfg['url'])}")
        else:
            out.append(f"command = {toml_str(cfg.get('command', ''))}")
            if cfg.get("args"):
                out.append("args = [" + ", ".join(toml_str(a) for a in cfg["args"]) + "]")
        if cfg.get("env"):
            out.append(f"[mcp_servers.{toml_str(name)}.env]")
            out += [f"{toml_str(k)} = {toml_str(v)}" for k, v in cfg["env"].items()]
        out.append("")
    return out


def parse_mcp(spec):
    """NAME=JSON -> (name, server config)."""
    name, sep, raw = spec.partition("=")
    name = name.strip()
    if not sep or not name:
        raise SystemExit(f"--mcp {spec!r}: expected NAME=JSON, e.g. "
                         "'fetch={\"command\":\"uvx\",\"args\":[\"mcp-server-fetch\"]}'")
    try:
        cfg = json.loads(raw)
    except json.JSONDecodeError as err:
        raise SystemExit(f"--mcp {name}: invalid JSON ({err})")
    if not isinstance(cfg, dict) or not (cfg.get("command") or cfg.get("url")):
        raise SystemExit(f"--mcp {name}: the JSON needs a \"command\" or a \"url\"")
    return name, cfg


def add_project_hooks(settings):
    """Register the done gate + resume-context hooks in the agent's settings (idempotent)."""
    hooks = settings.setdefault("hooks", {})
    added = []
    for event, script in PROJECT_HOOKS.items():
        cmd = f"python3 {script}"
        entries = hooks.setdefault(event, [])
        if any(h.get("command") == cmd for m in entries for h in m.get("hooks", [])):
            continue
        entries.append({"hooks": [{"type": "command", "command": cmd, "timeout": 330}]})
        added.append(event)
    return added


def in_git_repo(folder):
    try:
        r = subprocess.run(["git", "-C", str(folder), "rev-parse", "--is-inside-work-tree"],
                           capture_output=True, text=True)
    except FileNotFoundError:
        return None
    return r.returncode == 0


def main():
    p = argparse.ArgumentParser(
        description="Scaffold or update a hivey solo agent's folder (see SKILL.md step 5).",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="example:\n  setup_agent.py ~/agents/seo-auditor --name seo-auditor --kind claude \\\n"
               "      --check-cmd 'bash scripts/check.sh' --gate --reviewer --git")
    p.add_argument("folder", help="the agent folder (created if missing)")
    p.add_argument("--name", required=True, help="agent name / slug, [a-z][a-z0-9-]{0,31}")
    p.add_argument("--kind", choices=KINDS, default="claude", help="coding agent (default claude)")
    p.add_argument("--dirs", default="", help="comma-separated extra folders the agent may use")
    p.add_argument("--allow", action="append", default=[], help="Claude Code allow rule (repeatable)")
    p.add_argument("--deny", action="append", default=[], help="Claude Code deny rule (repeatable)")
    p.add_argument("--mcp", action="append", default=[], metavar="NAME=JSON",
                   help="add a project MCP server to .mcp.json (repeatable)")
    p.add_argument("--enable-mcps", default="", help="pre-approve servers already in .mcp.json")
    p.add_argument("--check-cmd", help="run check, run in the agent folder; exit 0 = run done")
    p.add_argument("--progress-log", help="a newest-first log file the resume context summarizes")
    p.add_argument("--gate", action="store_true",
                   help="register the done gate and resume-context hooks (Claude Code only)")
    p.add_argument("--reviewer", action="store_true",
                   help="install the read-only reviewer subagent (Claude Code only)")
    p.add_argument("--git", action="store_true", help="git init the folder if it isn't in a repo")
    args = p.parse_args()

    if not NAME_RE.match(args.name):
        raise SystemExit(f"invalid name {args.name!r}: must match {NAME_RE.pattern}")
    agent_dir = Path(args.folder).expanduser().resolve()
    if agent_dir == Path.home().resolve():
        raise SystemExit("refusing to use your home folder as an agent folder")
    if args.gate and not (args.check_cmd or read_json(agent_dir / ".agent" / "config.json").get("check")):
        raise SystemExit("--gate needs a run check: pass --check-cmd")
    agent_dir.mkdir(parents=True, exist_ok=True)
    result = {"agent": args.name, "kind": args.kind, "dir": str(agent_dir), "notes": []}

    brief = agent_dir / "CLAUDE.md"
    if not brief.exists():
        brief.write_text(SKELETON.format(name=args.name, dir=agent_dir))
        result["claude_md"] = "skeleton written"
    else:
        result["claude_md"] = "kept"

    settings_path = agent_dir / ".claude" / "settings.json"
    settings = read_json(settings_path)
    perms = settings.setdefault("permissions", {})
    dirs = [str(agent_dir)] + [str(Path(d.strip()).expanduser().resolve())
                               for d in args.dirs.split(",") if d.strip()]
    add_unique(perms.setdefault("additionalDirectories", []), dirs)
    if args.allow:
        add_unique(perms.setdefault("allow", []), args.allow)
    add_unique(perms.setdefault("deny", []), ALWAYS_DENY + args.deny)
    if args.kind == "grok" and len(dirs) > 1:
        result["notes"].append("grok can't use extra folders: keep its work inside the agent folder")
    if args.kind != "claude" and (args.allow or args.deny):
        result["notes"].append("allow/deny rules apply to Claude Code only")

    mcp_names = [m.strip() for m in args.enable_mcps.split(",") if m.strip()]
    mcp_path = agent_dir / ".mcp.json"
    if args.mcp:
        mcp = read_json(mcp_path)
        servers = mcp.setdefault("mcpServers", {})
        for spec in args.mcp:
            name, cfg = parse_mcp(spec)
            servers[name] = cfg
            add_unique(mcp_names, [name])
        write_json(mcp_path, mcp)
    if mcp_names:
        servers = read_json(mcp_path).get("mcpServers", {})
        result["mcps_missing"] = [m for m in mcp_names if m not in servers]
        found = {m: servers[m] for m in mcp_names if m in servers}
        add_unique(settings.setdefault("enabledMcpjsonServers", []), sorted(found))
        result["mcps"] = sorted(found)
        result["mcps_with_env"] = sorted(m for m, cfg in found.items() if cfg.get("env"))
        if args.kind == "codex":
            codex_cfg = agent_dir / ".codex" / "config.toml"
            codex_cfg.parent.mkdir(exist_ok=True)
            all_servers = read_json(mcp_path).get("mcpServers", {})
            codex_cfg.write_text("# Project-scoped Codex config written by hivey-agent-creator "
                                 "(may hold MCP keys; git-ignored)\n\n"
                                 + "\n".join(codex_mcp_tables(all_servers)))
            result["codex_config"] = str(codex_cfg)
        elif args.kind in ("grok", "agy"):
            result["notes"].append(f"{args.kind} doesn't read .mcp.json: set up its MCP servers "
                                   "with its own CLI, or prefer a CLI tool")

    cfg_path = agent_dir / ".agent" / "config.json"
    if args.check_cmd or args.progress_log:
        cfg = read_json(cfg_path)
        if args.check_cmd:
            cfg["check"] = args.check_cmd
        if args.progress_log:
            cfg["progress_log"] = str(Path(args.progress_log).expanduser())
        write_json(cfg_path, cfg)
        result["agent_config"] = cfg
    (agent_dir / ".agent" / "state").mkdir(parents=True, exist_ok=True)
    (agent_dir / "logs").mkdir(exist_ok=True)

    if args.gate:
        if args.kind == "claude":
            result["hooks_added"] = add_project_hooks(settings)
        else:
            result["notes"].append("the done gate is a Claude Code hook: the brief tells this "
                                   "agent to run the check itself")
    if args.reviewer:
        if args.kind != "claude":
            result["reviewer"] = "skipped: the reviewer subagent is Claude Code only"
        else:
            dest = agent_dir / ".claude" / "agents" / "reviewer.md"
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(HERE.parent / "references" / "reviewer-agent.md", dest)
            result["reviewer"] = str(dest)

    write_json(settings_path, settings)
    result["settings"] = str(settings_path)

    gitignore = agent_dir / ".gitignore"
    lines = gitignore.read_text().splitlines() if gitignore.exists() else []
    add_unique(lines, GITIGNORE)
    gitignore.write_text("\n".join(lines) + "\n")

    repo = in_git_repo(agent_dir)
    if args.git and repo is False:
        subprocess.run(["git", "init", "-q", str(agent_dir)], check=True)
        repo = True
        result["git"] = "initialized"
    else:
        result["git"] = {True: "in a repo", False: "not a repo (pass --git)", None: "git missing"}[repo]

    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
