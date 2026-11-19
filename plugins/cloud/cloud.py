#!/usr/bin/env python3
"""hivey cloud: run Claude Code cloud sessions and scheduled cloud tasks with your own setup.

Cloud sessions start from a fresh clone of the repo, so nothing in ~/.claude reaches them.
hivey builds a branch (hivey-cloud/<name>, or hivey-cloud-schedule for scheduled tasks) that is your
current commit plus a generated bundle, and starts the cloud session from it:

  .claude/skills|commands|agents/   the skills, commands, agents and plugins you listed
  .claude/rules/hivey-cloud-global.md   your ~/.claude/CLAUDE.md (rules files always load)
  .claude/settings.json             your permissions + safe env, plus the bootstrap hook
  .mcp.json                         the MCP servers you listed (secrets moved out)
  .hivey-cloud/                     bootstrap, envsave shim, secrets.age (age-encrypted)

Secrets (EnvSave opaque keys, read with `envsave get es_...`, which needs no password):
  kind = "encrypted"  value goes into .hivey-cloud/secrets.age, encrypted to your age public
                      key; the cloud environment holds the private key (HIVEY_CLOUD_AGE_KEY)
                      and the bootstrap decrypts it inside the session.
  kind = "network"    a claude.ai network secret: `hivey cloud secrets` copies each value to
                      the clipboard (never printed) for you to paste into the environment; the
                      key never enters the cloud machine.

What goes in is listed per repo in .hivey/cloud.toml (`hivey cloud init` writes one).
"""

from __future__ import annotations

import datetime as dt
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

HOME = Path.home()
CLAUDE = HOME / ".claude"
STATE = HOME / ".hivey" / "cloud"
CONFIG = STATE / "config.json"
SESSIONS = STATE / "sessions.json"
PLUGIN = Path(__file__).resolve().parent
TOML = Path(".hivey") / "cloud.toml"
BRANCH_PREFIX = "hivey-cloud/"
SCHEDULE_BRANCH = "hivey-cloud-schedule"
NETWORK_PLACEHOLDER = "hivey-network-secret"
SKIP_DIRS = {".git", "node_modules", "__pycache__", ".venv", "venv", "dist", "build", ".DS_Store"}
MAX_FILE = 2 * 1024 * 1024
# hivey skills that only make sense on the Mac (left out of hivey_skills; list one in
# `skills` to copy it anyway).
MAC_ONLY_SKILLS = {"hivey-agent-map"}
SECRETISH = re.compile(r"(KEY|TOKEN|SECRET|PASSW|AUTH|CREDENTIAL|COOKIE|SESSION)", re.I)

TEMPLATE = """\
# What `hivey cloud run` / `hivey cloud schedule` copy into Claude Code cloud sessions.
# Cloud sessions only see what is in the repo; hivey adds these on a hivey-cloud/* branch,
# never on your own branches. See `hivey cloud help`.

global_claude_md = true      # ~/.claude/CLAUDE.md -> .claude/rules/hivey-cloud-global.md
settings = true              # ~/.claude/settings.json permissions + env without secrets
hivey_skills = true          # every ~/.claude/skills/hivey* skill (not hivey-agent-map)

skills = []                  # more skills from ~/.claude/skills, e.g. ["envsave", "dataviz"]
commands = []                # ~/.claude/commands/<name>.md
agents = []                  # ~/.claude/agents/<name>.md
plugins = []                 # installed plugins: skills, commands, agents and MCP, e.g. ["context7"]
mcp = []                     # user MCP servers from ~/.claude.json, e.g. ["brain-memory"]

# EnvSave keys the cloud agents need. `key` is the opaque es_... key (envsave reveal NAME).
# kind = "encrypted": decrypted inside the session; envsave.get("es_...") and $NAME work.
# kind = "network":   pasted once into claude.ai (hivey cloud secrets); the proxy adds it to
#                     requests for `host`, the session never sees it.
# [[secrets]]
# name = "OPENAI_API_KEY"
# key = "es_..."
# kind = "network"
# host = "api.openai.com"
"""


# ---------------------------------------------------------------- helpers

def say(msg: str = "") -> None:
    print(msg, flush=True)


def die(msg: str, code: int = 1) -> None:
    print(f"hivey cloud: {msg}", file=sys.stderr)
    raise SystemExit(code)


def run(cmd: list[str], cwd: Path | None = None, check: bool = True, capture: bool = True,
        input: str | None = None) -> subprocess.CompletedProcess:
    proc = subprocess.run(cmd, cwd=cwd, text=True, input=input,
                          capture_output=capture)
    if check and proc.returncode != 0:
        err = (proc.stderr or proc.stdout or "").strip() if capture else ""
        die(f"{' '.join(cmd[:3])} failed: {err}")
    return proc


def git(*args: str, cwd: Path | None = None, check: bool = True) -> str:
    return run(["git", *args], cwd=cwd, check=check).stdout.strip()


def load_json(path: Path, default):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return default


def save_json(path: Path, data, mode: int = 0o600) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(data, indent=2) + "\n")
    os.chmod(tmp, mode)
    tmp.replace(path)


def slugify(text: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    return slug[:40].strip("-") or "task"


def repo_root() -> Path:
    root = run(["git", "rev-parse", "--show-toplevel"], check=False).stdout.strip()
    if not root:
        die("run this inside the git repository the cloud agents should work on")
    return Path(root)


def load_toml(root: Path) -> dict:
    path = root / TOML
    if not path.exists():
        die(f"no {TOML} in {root} yet: run `hivey cloud init` and list what to copy")
    try:
        return tomllib.loads(path.read_text())
    except tomllib.TOMLDecodeError as err:
        die(f"{path}: {err}")
    return {}


def envsave_get(key: str) -> str:
    """Value of an EnvSave opaque key. `envsave get es_...` needs no password (no Touch ID)."""
    if not re.fullmatch(r"es_[A-Za-z0-9]+", key or ""):
        die(f"{key!r} is not an EnvSave opaque key (es_...): get it with `envsave reveal NAME`")
    proc = subprocess.run(["envsave", "get", key], capture_output=True, text=True)
    value = proc.stdout.rstrip("\n")
    if proc.returncode != 0 or not value:
        die(f"envsave has no value for {key} ({proc.stderr.strip() or 'empty'})")
    return value


def need(tool: str, hint: str) -> None:
    if not shutil.which(tool):
        die(f"`{tool}` is not installed: {hint}")


# ---------------------------------------------------------------- bundle

def copy_tree(src: Path, dst: Path, warnings: list[str]) -> int:
    """Copies src into dst following symlinks, skipping caches and big files. Returns bytes."""
    total = 0
    for path in sorted(src.rglob("*")):
        rel = path.relative_to(src)
        if any(part in SKIP_DIRS for part in rel.parts):
            continue
        if path.is_dir():
            continue
        try:
            size = path.stat().st_size
        except OSError:
            continue
        if size > MAX_FILE:
            warnings.append(f"skipped {src.name}/{rel} ({size // 1024} KB, over 2 MB)")
            continue
        out = dst / rel
        out.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, out, follow_symlinks=True)
        total += size
    return total


def installed_plugins() -> dict[str, Path]:
    """plugin name -> install path, from ~/.claude/plugins/installed_plugins.json."""
    data = load_json(CLAUDE / "plugins" / "installed_plugins.json", {})
    out: dict[str, Path] = {}
    for key, entries in (data.get("plugins") or {}).items():
        if entries:
            out[key.split("@")[0]] = Path(entries[-1]["installPath"])
            out[key] = Path(entries[-1]["installPath"])
    return out


def plugin_mcp(path: Path) -> dict:
    servers: dict = {}
    for name in (".mcp.json", ".claude-plugin/plugin.json"):
        data = load_json(path / name, {})
        mcp = data.get("mcpServers")
        if isinstance(mcp, dict):
            servers.update(mcp)
    return servers


def user_mcp() -> dict:
    return (load_json(HOME / ".claude.json", {}).get("mcpServers")) or {}


def local_path(value: str) -> bool:
    return isinstance(value, str) and (value.startswith(str(HOME)) or value.startswith("/Users/"))


def cloud_mcp_entry(name: str, cfg: dict, secret_vars: dict, warnings: list[str]) -> dict | None:
    """The .mcp.json entry for the cloud: secrets moved into the encrypted payload."""
    cfg = json.loads(json.dumps(cfg))
    kind = cfg.get("type", "stdio")
    if kind == "stdio":
        argv = [cfg.get("command", "")] + list(cfg.get("args") or [])
        if any(local_path(a) for a in argv):
            warnings.append(f"MCP {name}: skipped, it runs a file from this Mac ({argv[0]} …)")
            return None
        env = cfg.pop("env", None) or {}
        if env:
            secret_vars.setdefault("mcp", {})[name] = {"env": env}
        return {
            "type": "stdio",
            "command": "bash",
            "args": [".hivey-cloud/mcp-run.sh", name, "--", *argv],
        }
    headers = cfg.pop("headers", None) or {}
    if headers:
        secret_vars.setdefault("mcp", {})[name] = {"headers": headers}
        cfg["headersHelper"] = f"bash .hivey-cloud/mcp-headers.sh {shlex.quote(name)}"
    if cfg.get("headersHelper") and not headers:
        warnings.append(f"MCP {name}: its headersHelper is a local command; it may fail")
    elif not headers:
        warnings.append(f"MCP {name}: no auth headers; if it needs a browser sign-in (OAuth) it "
                        f"can't sign in from the cloud: add it as a claude.ai connector instead")
    return cfg


def merge_settings(root: Path, out_root: Path, spec: dict, warnings: list[str]) -> None:
    path = out_root / ".claude" / "settings.json"
    repo = load_json(root / ".claude" / "settings.json", {})
    settings = json.loads(json.dumps(repo))
    if spec.get("settings", True):
        mine = load_json(CLAUDE / "settings.json", {})
        perms = mine.get("permissions") or {}
        target = settings.setdefault("permissions", {})
        for field in ("allow", "deny", "ask"):
            rules = [r for r in perms.get(field) or [] if not local_path(r) and "/Users/" not in r]
            if rules:
                merged = list(dict.fromkeys((target.get(field) or []) + rules))
                target[field] = merged
        env = {k: v for k, v in (mine.get("env") or {}).items()
               if not SECRETISH.search(k) and not local_path(str(v))}
        if env:
            settings["env"] = {**env, **(settings.get("env") or {})}
        if mine.get("hooks"):
            warnings.append("your global hooks run files on this Mac: not copied")
    hooks = settings.setdefault("hooks", {})
    starts = hooks.setdefault("SessionStart", [])
    command = "bash \"$CLAUDE_PROJECT_DIR/.hivey-cloud/bootstrap.sh\""
    if not any(h.get("command") == command for g in starts for h in g.get("hooks", [])):
        starts.append({"hooks": [{"type": "command", "command": command, "timeout": 300}]})
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(settings, indent=2) + "\n")


def global_rules(spec: dict) -> str:
    parts = [
        "# hivey cloud session",
        "",
        "This session runs in a Claude Code cloud VM, started by `hivey cloud` from the user's Mac.",
        "- EnvSave works through a shim: `envsave get es_...`, Python `envsave.get(\"es_...\")` and",
        "  `$NAME` return the secrets hivey copied (see .hivey-cloud/manifest.json for names).",
        "  There is no vault, no Touch ID, so the EnvSave rules about unlocking the vault don't apply.",
        f"  Network secrets return the placeholder `{NETWORK_PLACEHOLDER}`; the proxy adds the real",
        "  key to requests for their host.",
        "- hivey, its bus and Slack relays, ~/.hivey and the user's wikis are not here. Report",
        "  results in the conversation and push your work to a branch.",
        "- The skills, commands, agents and MCP servers in .claude/ and .mcp.json that hivey added",
        "  are copies of the user's own; don't commit them to other branches.",
    ]
    if spec.get("global_claude_md", True) and (CLAUDE / "CLAUDE.md").exists():
        parts += ["", "# The user's global instructions (~/.claude/CLAUDE.md)", "",
                  (CLAUDE / "CLAUDE.md").read_text().strip()]
    return "\n".join(parts) + "\n"


def build_bundle(root: Path, out_root: Path, spec: dict, recipient: str | None) -> dict:
    """Writes the bundle into out_root (a checkout of the repo). Returns the manifest."""
    warnings: list[str] = []
    copied = {"skills": [], "commands": [], "agents": [], "mcp": [], "secrets": []}
    total = 0
    claude_dir = out_root / ".claude"

    def add_skill(src: Path, name: str) -> None:
        nonlocal total
        dst = claude_dir / "skills" / name
        if (root / ".claude" / "skills" / name).exists():
            warnings.append(f"skill {name}: the repo has its own, kept")
            return
        if not (src / "SKILL.md").exists():
            warnings.append(f"skill {name}: no SKILL.md in {src}")
            return
        total += copy_tree(src, dst, warnings)
        copied["skills"].append(name)

    def add_file(kind: str, src: Path) -> None:
        nonlocal total
        dst = claude_dir / kind / src.name
        if (root / ".claude" / kind / src.name).exists():
            warnings.append(f"{kind[:-1]} {src.stem}: the repo has its own, kept")
            return
        dst.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dst, follow_symlinks=True)
        total += src.stat().st_size
        copied[kind].append(src.stem)

    skills = set(spec.get("skills") or [])
    if spec.get("hivey_skills", True) and (CLAUDE / "skills").is_dir():
        skills |= {p.name for p in (CLAUDE / "skills").iterdir()
                   if p.name.startswith("hivey") and p.name not in MAC_ONLY_SKILLS}
    for name in sorted(skills):
        src = CLAUDE / "skills" / name
        if src.exists():
            add_skill(src.resolve(), name)
        else:
            warnings.append(f"skill {name}: not in ~/.claude/skills")
    for kind in ("commands", "agents"):
        for name in spec.get(kind) or []:
            src = CLAUDE / kind / f"{name.removesuffix('.md')}.md"
            if src.exists():
                add_file(kind, src)
            else:
                warnings.append(f"{kind[:-1]} {name}: not in ~/.claude/{kind}")

    secret_payload: dict = {"vars": {}, "opaque": {}, "network": {}}
    servers: dict = {}
    plugins = installed_plugins()
    for name in spec.get("plugins") or []:
        path = plugins.get(name)
        if not path or not path.exists():
            warnings.append(f"plugin {name}: not installed")
            continue
        for sub in sorted((path / "skills").glob("*")) if (path / "skills").is_dir() else []:
            if sub.is_dir():
                add_skill(sub, sub.name)
        for kind in ("commands", "agents"):
            for md in sorted((path / kind).glob("*.md")) if (path / kind).is_dir() else []:
                add_file(kind, md)
        servers.update(plugin_mcp(path))
    mine = user_mcp()
    for name in spec.get("mcp") or []:
        if name in mine:
            servers[name] = mine[name]
        else:
            warnings.append(f"MCP {name}: not a user MCP server in ~/.claude.json")
    repo_mcp = load_json(root / ".mcp.json", {})
    mcp_out = json.loads(json.dumps(repo_mcp)) if repo_mcp else {}
    mcp_servers = mcp_out.setdefault("mcpServers", {})
    for name, cfg in servers.items():
        if name in mcp_servers:
            warnings.append(f"MCP {name}: the repo's .mcp.json has it, kept")
            continue
        entry = cloud_mcp_entry(name, cfg, secret_payload, warnings)
        if entry:
            mcp_servers[name] = entry
            copied["mcp"].append(name)
    if mcp_servers:
        (out_root / ".mcp.json").write_text(json.dumps(mcp_out, indent=2) + "\n")

    for item in spec.get("secrets") or []:
        name, key = item.get("name"), item.get("key")
        kind = item.get("kind", "encrypted")
        if not name or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
            die(f"secret {name!r}: `name` must be an environment variable name")
        if kind == "network":
            if not item.get("host"):
                die(f"secret {name}: a network secret needs `host`")
            secret_payload["network"][name] = {"key": key, "host": item["host"]}
            secret_payload["opaque"][key] = NETWORK_PLACEHOLDER
        elif kind == "encrypted":
            value = envsave_get(key)
            secret_payload["vars"][name] = value
            secret_payload["opaque"][key] = value
        else:
            die(f"secret {name}: kind must be \"encrypted\" or \"network\"")
        copied["secrets"].append({"name": name, "kind": kind, "host": item.get("host")})

    hc = out_root / ".hivey-cloud"
    hc.mkdir(parents=True, exist_ok=True)
    for script in ("bootstrap.sh", "mcp-run.sh", "mcp-headers.sh", "envsave_shim.py"):
        shutil.copy2(PLUGIN / "cloud" / script, hc / script)
    needs_secrets = secret_payload["vars"] or secret_payload.get("mcp")
    if needs_secrets:
        if not recipient:
            die("these secrets need encryption: run `hivey cloud setup` first")
        plain = json.dumps(secret_payload)
        enc = subprocess.run(["age", "-r", recipient, "-a", "-o", str(hc / "secrets.age")],
                             input=plain, text=True, capture_output=True)
        del plain
        if enc.returncode != 0:
            die(f"age could not encrypt the secrets: {enc.stderr.strip()}")
    elif secret_payload["opaque"]:
        # Only network secrets: their placeholders are not secret.
        (hc / "network.json").write_text(json.dumps(
            {"opaque": secret_payload["opaque"], "network": secret_payload["network"]}, indent=2))
    (claude_dir / "rules").mkdir(parents=True, exist_ok=True)
    (claude_dir / "rules" / "hivey-cloud-global.md").write_text(global_rules(spec))
    merge_settings(root, out_root, spec, warnings)
    if total > 5 * 1024 * 1024:
        warnings.append(f"the bundle is {total // (1024 * 1024)} MB: drop big skills you don't need")
    manifest = {
        "built": dt.datetime.now().isoformat(timespec="seconds"),
        "source_commit": git("rev-parse", "HEAD", cwd=root),
        **copied,
        "bytes": total,
        "warnings": warnings,
    }
    (hc / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    return manifest


def print_manifest(manifest: dict) -> None:
    say(f"  skills:   {len(manifest['skills'])} ({', '.join(manifest['skills'][:8])}"
        f"{' …' if len(manifest['skills']) > 8 else ''})")
    for kind in ("commands", "agents", "mcp"):
        if manifest[kind]:
            say(f"  {kind + ':':9} {', '.join(manifest[kind])}")
    for s in manifest["secrets"]:
        where = f"network secret for {s['host']}" if s["kind"] == "network" else "encrypted"
        say(f"  secret:   {s['name']} ({where})")
    say(f"  size:     {manifest['bytes'] // 1024} KB")
    for w in manifest["warnings"]:
        say(f"  ! {w}")


def make_branch(root: Path, branch: str, push: bool = True) -> tuple[Path, dict]:
    """A temporary worktree on `branch` = HEAD + bundle commit, pushed to origin unless
    `push` is false (then `claude --cloud` uploads the worktree itself)."""
    spec = load_toml(root)
    cfg = load_json(CONFIG, {})
    if push and not git("remote", cwd=root, check=False):
        die("the repo has no git remote: use --upload (Claude Code uploads the repo itself)")
    (STATE / "worktrees").mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix=f"{slugify(root.name)}-", dir=STATE / "worktrees"))
    git("worktree", "add", "--quiet", "-B", branch, str(tmp), "HEAD", cwd=root)
    set_trust(tmp, inherit_from=root)
    try:
        manifest = build_bundle(root, tmp, spec, cfg.get("recipient"))
        git("add", "-A", "-f", ".claude", ".hivey-cloud", cwd=tmp)
        if (tmp / ".mcp.json").exists():
            git("add", "-f", ".mcp.json", cwd=tmp)
        git("-c", "commit.gpgsign=false", "commit", "--quiet", "--no-verify", "-m",
            "chore: hivey cloud bundle (generated, not for merging)", cwd=tmp)
        if push:
            remote = git("remote", cwd=root).splitlines()[0]
            say(f"pushing {branch} to {remote} …")
            run(["git", "push", "--quiet", "--force", "-u", remote, f"{branch}:{branch}"],
                cwd=tmp)
    except SystemExit:
        remove_worktree(root, tmp)
        raise
    return tmp, manifest


def remove_worktree(root: Path, tmp: Path) -> None:
    git("worktree", "remove", "--force", str(tmp), cwd=root, check=False)
    shutil.rmtree(tmp, ignore_errors=True)
    set_trust(tmp, inherit_from=None)


CLAUDE_JSON = HOME / ".claude.json"


def trusted(projects: dict, path: Path) -> bool:
    for p in (path, *path.parents):
        if (projects.get(str(p)) or {}).get("hasTrustDialogAccepted"):
            return True
    return False


def set_trust(path: Path, inherit_from: Path | None) -> None:
    """Claude Code asks "do you trust this folder?" in every new folder, and `claude --cloud`
    runs in a fresh worktree. The worktree is the user's own repo, so it gets the trust the repo
    already has (or hivey's own test repo's): nothing is trusted that wasn't before. With
    `inherit_from=None` the worktree's entry is removed again."""
    data = load_json(CLAUDE_JSON, None)
    if not isinstance(data, dict):
        return
    projects = data.setdefault("projects", {})
    key = str(path.resolve()) if path.exists() else str(path)
    if inherit_from is None:
        if projects.pop(key, None) is None and projects.pop(str(path), None) is None:
            return
    else:
        source = inherit_from.resolve()
        if not (trusted(projects, source) or STATE.resolve() in (source, *source.parents)):
            return
        projects.setdefault(key, {})["hasTrustDialogAccepted"] = True
    mode = CLAUDE_JSON.stat().st_mode & 0o777
    tmp = CLAUDE_JSON.with_name(".claude.json.hivey-tmp")
    tmp.write_text(json.dumps(data, indent=2))
    os.chmod(tmp, mode)
    tmp.replace(CLAUDE_JSON)


# ---------------------------------------------------------------- commands

def cmd_setup(args: list[str]) -> int:
    need("age", "brew install age")
    need("claude", "install Claude Code")
    cfg = load_json(CONFIG, {})
    if cfg.get("recipient") and "--rotate" not in args:
        say(f"age key ready (public: {cfg['recipient']}). New key: hivey cloud setup --rotate")
    else:
        tmp = Path(tempfile.mkdtemp(prefix="hivey-age-"))
        try:
            ident = tmp / "id"
            run(["age-keygen", "-o", str(ident)])
            text = ident.read_text()
            secret = next(l for l in text.splitlines() if l.startswith("AGE-SECRET-KEY-"))
            public = run(["age-keygen", "-y", str(ident)]).stdout.strip()
        finally:
            shutil.rmtree(tmp, ignore_errors=True)
        cfg["recipient"] = public
        save_json(CONFIG, cfg)
        copy_to_clipboard(secret)
        del secret
        say("A new age key was made. hivey keeps only the public half "
            f"({public[:16]}…) in {CONFIG}.")
        say("The private key is on your clipboard (it is not shown or saved anywhere).")
        say("")
        say("1. Keep a copy in EnvSave (you'll be asked to paste it; Touch ID may prompt):")
        say("     envsave set HIVEY_CLOUD_AGE_KEY")
        input("   Press Enter when done… ")
    say("")
    say("2. In claude.ai/code open the environment selector → your environment → Edit:")
    say("   • Environment variables: add   HIVEY_CLOUD_AGE_KEY=<paste the private key>")
    say("     (to paste it again: envsave show HIVEY_CLOUD_AGE_KEY | pbcopy)")
    say("   • Network access: Limited with “Allow package managers” (the bootstrap apt-installs age)")
    say("   • Save. Then pick it as the CLI default with /remote-env in Claude Code.")
    say("3. Install the Claude GitHub App on the repo (or run /web-setup in Claude Code).")
    say("4. In each repo: hivey cloud init, edit .hivey/cloud.toml, hivey cloud secrets.")
    if "--no-open" not in args and sys.platform == "darwin":
        subprocess.run(["open", "https://claude.ai/code"], check=False)
    clear_clipboard_later()
    return 0


def copy_to_clipboard(value: str) -> None:
    if not shutil.which("pbcopy"):
        die("pbcopy is missing (macOS only)")
    subprocess.run(["pbcopy"], input=value, text=True, check=True)


def clear_clipboard_later() -> None:
    if shutil.which("pbcopy"):
        subprocess.Popen(["sh", "-c", "sleep 120; printf '' | pbcopy"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                         start_new_session=True)


def cmd_init(args: list[str]) -> int:
    root = repo_root()
    path = root / TOML
    if path.exists() and "--force" not in args:
        say(f"{path} exists (hivey cloud init --force replaces it)")
        return 0
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(TEMPLATE)
    say(f"wrote {path}: list the skills, MCP servers and secrets cloud agents need")
    return 0


def cmd_bundle(args: list[str]) -> int:
    root = repo_root()
    out = Path(args[args.index("--out") + 1]) if "--out" in args else \
        Path(tempfile.mkdtemp(prefix="hivey-cloud-bundle-"))
    out.mkdir(parents=True, exist_ok=True)
    manifest = build_bundle(root, out, load_toml(root), load_json(CONFIG, {}).get("recipient"))
    say(f"bundle written to {out} (nothing pushed)")
    print_manifest(manifest)
    return 0


def record(entry: dict) -> None:
    data = load_json(SESSIONS, [])
    data.append(entry)
    save_json(SESSIONS, data[-200:])


def cmd_run(args: list[str]) -> int:
    name = None
    if "--name" in args:
        i = args.index("--name")
        name = args[i + 1]
        args = args[:i] + args[i + 2:]
    upload = "--upload" in args
    args = [a for a in args if a != "--upload"]
    task = " ".join(args).strip()
    if not task:
        die("usage: hivey cloud run \"<task>\" [--name <slug>] [--upload]", 2)
    return start_session(repo_root(), task, name, upload)


def start_session(root: Path, task: str, name: str | None, upload: bool) -> int:
    need("claude", "install Claude Code")
    slug = slugify(name or task)
    stamp = dt.datetime.now().strftime("%m%d-%H%M")
    branch = f"{BRANCH_PREFIX}{slug}-{stamp}"
    tmp, manifest = make_branch(root, branch, push=not upload)
    print_manifest(manifest)
    prompt = (f"{task}\n\n(hivey: you are on branch {branch}, which carries a generated "
              f".claude/ bundle. Push your work to a new claude/ branch made from the original "
              f"commit {manifest['source_commit'][:12]} without the bundle files: .claude/"
              f" changes from commit 'chore: hivey cloud bundle', .hivey-cloud/, .mcp.json "
              f"additions.)")
    say(f"starting the cloud session from {branch} …")
    session = None
    env = {**os.environ, **({"CCR_FORCE_BUNDLE": "1"} if upload else {})}
    # `claude --cloud` needs a terminal; `script` gives it one and keeps a log for the ID.
    log = Path(tempfile.mkstemp(prefix="hivey-cloud-", suffix=".log")[1])
    try:
        code = subprocess.call(["script", "-q", str(log), "claude", "--cloud", prompt],
                               cwd=tmp, env=env)
        text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", log.read_text(errors="ignore"))
        found = re.search(r"\b((?:session|cse)_[A-Za-z0-9]{8,})", text)
        session = found.group(1) if found else None
    finally:
        log.unlink(missing_ok=True)
        remove_worktree(root, tmp)
    if session:
        record({"session": session, "branch": branch, "task": task, "repo": str(root),
                "started": dt.datetime.now().isoformat(timespec="seconds")})
        say(f"\nsession {session} — follow it at https://claude.ai/code/{session}")
        say(f"steer it:  hivey cloud send {session} \"…\"   bring it here:  hivey cloud pull {session}")
    return code


def cmd_send(args: list[str]) -> int:
    if len(args) < 2:
        die("usage: hivey cloud send <session> \"<message>\"", 2)
    proc = run(["claude", "-p", " ".join(args[1:]), "--cloud", args[0],
                "--output-format", "json"], check=False)
    out = proc.stdout.strip() or proc.stderr.strip()
    say(out)
    return proc.returncode


def cmd_pull(args: list[str]) -> int:
    if not args:
        die("usage: hivey cloud pull <session>", 2)
    return subprocess.call(["claude", "--teleport", args[0]])


def cmd_list(args: list[str]) -> int:
    data = load_json(SESSIONS, [])
    if "--json" in args:
        say(json.dumps(data, indent=2))
        return 0
    if not data:
        say("no cloud sessions started from hivey yet")
    for e in data[-30:]:
        kind = "schedule" if e.get("schedule") else "session"
        say(f"{e.get('started', '?')[:16]}  {kind:8} {e.get('session') or '-':32} "
            f"{e.get('branch', '')}  {e.get('task', '')[:60]}")
    return 0


def cmd_secrets(args: list[str]) -> int:
    root = repo_root()
    spec = load_toml(root)
    network = [s for s in spec.get("secrets") or [] if s.get("kind") == "network"]
    encrypted = [s for s in spec.get("secrets") or [] if s.get("kind", "encrypted") == "encrypted"]
    if encrypted:
        say(f"{len(encrypted)} encrypted secret(s) travel inside each bundle; nothing to do for them.")
    if not network:
        say("no network secrets in .hivey/cloud.toml")
        return 0
    say("Network secrets are added by hand in claude.ai (there is no API for it):")
    say("claude.ai/code → environment selector → your environment → Edit → Network secrets → Add secret")
    if "--no-open" not in args and sys.platform == "darwin":
        subprocess.run(["open", "https://claude.ai/code"], check=False)
    for s in network:
        value = envsave_get(s.get("key", ""))
        copy_to_clipboard(value)
        del value
        say("")
        say(f"  {s['name']}: type Bearer (or the header the API uses), host {s['host']}")
        say("  its value is on your clipboard — paste it in the form, then Add.")
        input("  Press Enter for the next one… ")
    subprocess.run(["pbcopy"], input="", text=True, check=False)
    say("\nclipboard cleared. Check them: hivey cloud run \"curl each host and report the status\"")
    return 0


def cmd_schedule(args: list[str]) -> int:
    if len(args) < 2:
        die("usage: hivey cloud schedule \"<when>\" \"<task>\"   e.g. \"weekdays at 9am\"", 2)
    when, task = args[0], " ".join(args[1:])
    need("claude", "install Claude Code")
    root = repo_root()
    tmp, manifest = make_branch(root, SCHEDULE_BRANCH)
    remove_worktree(root, tmp)
    print_manifest(manifest)
    prompt = (
        f"/schedule {when}. Create it with exactly this prompt for each run:\n\n"
        f"Before anything else run: git fetch origin {SCHEDULE_BRANCH} && "
        f"git checkout {SCHEDULE_BRANCH} && bash .hivey-cloud/bootstrap.sh && "
        f"cat .claude/rules/hivey-cloud-global.md — that branch carries the user's skills, MCP "
        f"servers, settings and secrets (hivey cloud). Follow those rules and use those skills. "
        f"Then do this task: {task}"
    )
    say(f"{SCHEDULE_BRANCH} is pushed. Opening Claude Code to create the scheduled task "
        f"(confirm what it proposes):")
    record({"schedule": when, "branch": SCHEDULE_BRANCH, "task": task, "repo": str(root),
            "started": dt.datetime.now().isoformat(timespec="seconds")})
    return subprocess.call(["claude", prompt], cwd=root)


def cmd_refresh(args: list[str]) -> int:
    root = repo_root()
    tmp, manifest = make_branch(root, SCHEDULE_BRANCH)
    remove_worktree(root, tmp)
    print_manifest(manifest)
    say(f"{SCHEDULE_BRANCH} refreshed: scheduled tasks pick it up on their next run")
    return 0


def cmd_clean(args: list[str]) -> int:
    root = repo_root()
    remote = git("remote", cwd=root).splitlines()[0]
    heads = git("ls-remote", "--heads", remote, f"refs/heads/{BRANCH_PREFIX}*", cwd=root)
    branches = [l.split("refs/heads/", 1)[1] for l in heads.splitlines() if "refs/heads/" in l]
    if not branches:
        say("no hivey-cloud/* branches on the remote")
        return 0
    say("\n".join(f"  {b}" for b in branches))
    if "--yes" not in args:
        say(f"delete these {len(branches)} branch(es)? rerun with --yes "
            f"(the scheduled-task branch '{SCHEDULE_BRANCH}' is kept)")
        return 0
    run(["git", "push", "--quiet", remote, "--delete", *branches], cwd=root)
    for b in branches:
        git("branch", "-D", b, cwd=root, check=False)
    say(f"deleted {len(branches)} branch(es)")
    return 0


TEST_BRIEF = """\
# hivey cloud self-test agent

You check that `hivey cloud` delivered the user's setup into this cloud session. Change no
files and push nothing. Run each check, then answer with one table: check | PASS/FAIL | evidence
(one line). End with `RESULT: <n>/<total> passed`.

1. skills: `ls .claude/skills` lists hivey-* skills, and the Skill tool lists them too.
2. global rules: you can see the "hivey cloud session" rules and the user's global
   instructions (from .claude/rules/hivey-cloud-global.md) in your context, not only on disk.
3. settings hook: ~/.hivey-cloud/bootstrap.log exists or ~/.hivey-cloud/ready exists (the
   SessionStart hook ran). Show the log's last lines.
4. MCP: name the MCP servers whose tools you have (from .mcp.json: {mcp}).
5. envsave shim: `envsave get {key}` prints a value; its sha256 (`envsave get {key} | tr -d
   '\\n' | sha256sum`) is {digest}. Never print the value itself.
6. env var: `$HIVEY_CLOUD_TEST` in your shell has the same sha256 (use printf '%s').
7. python shim: `python3 -c 'import envsave,hashlib;print(hashlib.sha256(envsave.get("{key}")
   .encode()).hexdigest())'` prints the same sha256.
8. remote tools: `claude --version` and `gh --version` (or say they are missing).
"""


def cmd_test(args: list[str]) -> int:
    """Scaffolds a throwaway repo with the self-test brief and starts it in the cloud."""
    key = args[args.index("--secret") + 1] if "--secret" in args else None
    root = STATE / "test-agent"
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    git("init", "-q", "-b", "main", cwd=root)
    mcp = ["context7"] if "context7" in installed_plugins() else []
    toml = TEMPLATE.replace("plugins = []", f"plugins = {json.dumps(mcp)}")
    digest, mcp_note = "(no --secret given: report checks 5-7 as SKIP)", ", ".join(mcp) or "none"
    if key:
        import hashlib
        digest = hashlib.sha256(envsave_get(key).encode()).hexdigest()
        toml += (f'\n[[secrets]]\nname = "HIVEY_CLOUD_TEST"\nkey = "{key}"\n'
                 f'kind = "encrypted"\n')
    (root / ".hivey").mkdir()
    (root / TOML).write_text(toml)
    (root / "CLAUDE.md").write_text(TEST_BRIEF.format(key=key or "es_none", digest=digest,
                                                      mcp=mcp_note))
    (root / "README.md").write_text("Throwaway repo made by `hivey cloud test`.\n")
    git("add", "-A", cwd=root)
    git("-c", "user.name=hivey", "-c", "user.email=hivey@localhost", "-c", "commit.gpgsign=false",
        "commit", "-qm", "hivey cloud self-test", cwd=root)
    say(f"test agent repo: {root}")
    return start_session(root, "Run the hivey cloud self-test in CLAUDE.md and report the table.",
                         "selftest", upload=True)


COMMANDS = {
    "test": cmd_test,
    "setup": cmd_setup, "init": cmd_init, "bundle": cmd_bundle, "run": cmd_run,
    "send": cmd_send, "pull": cmd_pull, "list": cmd_list, "secrets": cmd_secrets,
    "schedule": cmd_schedule, "refresh": cmd_refresh, "clean": cmd_clean,
}


def main(argv: list[str]) -> int:
    if not argv or argv[0] not in COMMANDS:
        print(__doc__)
        return 2
    return COMMANDS[argv[0]](argv[1:])


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except KeyboardInterrupt:
        sys.exit(130)
