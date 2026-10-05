#!/usr/bin/env python3
"""hivey.github: publish a swarm as two GitHub repos, the product and the workspace.

  <owner>/<slug>          the product the swarm builds (e.g. <root>/app), its own git repo
  <owner>/<slug>-swarm    the swarm workspace: agent folders and briefs, .swarm/ logs, shared/,
                          with the product folder, builders' worktrees, secrets and runtime
                          state excluded by .gitignore

Nothing is created until you confirm in the pane. Before the first push of each repo,
gitleaks (if installed) scans it and any finding aborts. Afterwards the pane offers
"push both" (commits a workspace snapshot, pushes the product's current branch and the
builders' swarm/* branches).

Config ($HERDR_PLUGIN_CONFIG_DIR/config.json, all optional):
  {"owner": "<user or org>", "visibility": "private", "product_dirs": ["app"],
   "product_repo": "{slug}", "swarm_repo": "{slug}-swarm"}
The manifest's "product_dir" (relative to the root) wins over product_dirs.
HIVEY_GITHUB_DRY_RUN=1 prints the commands instead of running them.
"""
import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

DEFAULT_CONFIG = {
    "owner": "",
    "visibility": "private",
    "product_dirs": ["app"],
    "product_repo": "{slug}",
    "swarm_repo": "{slug}-swarm",
}
# The workspace repo never carries these (the product has its own repo).
WORKSPACE_IGNORE = [
    "# hivey.github: the product is published as its own repo",
    "/{product}/",
    "# builders' git worktrees of the product",
    "/*/repo/",
    "# secrets",
    ".env",
    ".env.*",
    "*.pem",
    "*.key",
    "*.p12",
    "# runtime state",
    ".swarm/*.lock",
    ".swarm/*.tmp",
    ".swarm/dashboard_usage.json",
    ".swarm/slack_relay_state.json",
    ".swarm/relay_state.json",
    "node_modules/",
    ".DS_Store",
]
MARKER = WORKSPACE_IGNORE[0]


# ---------------------------------------------------------------------------
# Pure planning (unit-tested in test_github.py)
# ---------------------------------------------------------------------------

def find_product(root, manifest, config):
    """Relative product folder, or None when the swarm builds nothing to publish."""
    candidates = [manifest.get("product_dir")] if manifest.get("product_dir") else []
    candidates += config.get("product_dirs", [])
    for rel in candidates:
        if rel and (Path(root) / rel).is_dir():
            return rel.strip("/")
    # Else a top-level folder that is its own git repo and isn't an agent's home.
    agents = set((manifest.get("agents") or {}).keys())
    for child in sorted(Path(root).iterdir()):
        if child.is_dir() and child.name not in agents and not child.name.startswith(".") \
                and (child / ".git").exists():
            return child.name
    return None


def workspace_gitignore(existing, product):
    """`existing` .gitignore text with hivey's block appended once."""
    if MARKER in existing:
        return existing
    lines = [line.replace("{product}", product) for line in WORKSPACE_IGNORE
             if product or "{product}" not in line]
    block = "\n".join(lines) + "\n"
    if existing and not existing.endswith("\n"):
        existing += "\n"
    return existing + ("\n" if existing else "") + block


def repo_names(slug, config):
    return (config["product_repo"].format(slug=slug), config["swarm_repo"].format(slug=slug))


# ---------------------------------------------------------------------------
# Git / GitHub
# ---------------------------------------------------------------------------

DRY_RUN = os.environ.get("HIVEY_GITHUB_DRY_RUN") == "1"


def run(cmd, cwd, check=True):
    shown = " ".join(cmd)
    if DRY_RUN:
        print(f"  [dry-run] ({cwd}) {shown}")
        return ""
    out = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if check and out.returncode != 0:
        raise RuntimeError(f"{shown}: {(out.stderr or out.stdout).strip()}")
    return out.stdout.strip()


def git(cwd, *args, check=True):
    return run(["git", *args], cwd, check)


def is_repo(path):
    return (Path(path) / ".git").exists()


def has_commits(path):
    return subprocess.run(["git", "rev-parse", "HEAD"], cwd=path, capture_output=True).returncode == 0


def has_origin(path):
    return subprocess.run(["git", "remote", "get-url", "origin"], cwd=path,
                          capture_output=True).returncode == 0


def scan_secrets(path):
    if not shutil.which("gitleaks"):
        print("  (gitleaks not installed: secret scan skipped)")
        return
    if DRY_RUN:
        print(f"  [dry-run] gitleaks git {path}")
        return
    # Scan what is committed (what gets pushed), not ignored files on disk.
    out = subprocess.run(["gitleaks", "git", str(path), "--no-banner", "--redact"],
                         capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"gitleaks found possible secrets in {path}; nothing was pushed.\n"
                           f"{(out.stdout or out.stderr)[-1500:]}")


def publish(path, full_name, visibility, message):
    """Commit (if needed) and create + push the GitHub repo, or push to the existing origin."""
    if not is_repo(path):
        git(path, "init", "-b", "main")
    if not has_commits(path) or git(path, "status", "--porcelain", check=False):
        git(path, "add", "-A")
        git(path, "commit", "-m", message, check=False)
    scan_secrets(path)
    if has_origin(path):
        git(path, "push", "-u", "origin", "HEAD")
    else:
        run(["gh", "repo", "create", full_name, f"--{visibility}", "--source", str(path),
             "--remote", "origin", "--push"], path)


def push_both(root, product):
    if product and is_repo(root / product):
        git(root / product, "push", "origin", "HEAD", check=False)
        branches = git(root / product, "branch", "--list", "swarm/*", "--format=%(refname:short)",
                       check=False).split()
        for branch in branches:
            git(root / product, "push", "origin", branch, check=False)
        print(f"  ✓ product pushed ({1 + len(branches)} branch(es))")
    if is_repo(root):
        if git(root, "status", "--porcelain", check=False) or DRY_RUN:
            git(root, "add", "-A")
            git(root, "commit", "-m", f"swarm snapshot {time.strftime('%Y-%m-%d %H:%M')}", check=False)
        git(root, "push", "origin", "HEAD", check=False)
        print("  ✓ workspace pushed")


# ---------------------------------------------------------------------------
# Pane UI
# ---------------------------------------------------------------------------

def ask(prompt):
    try:
        return input(prompt).strip()
    except EOFError:
        return "q"


def main():
    root = Path(os.environ.get("HIVEY_SWARM_ROOT") or (sys.argv[1] if len(sys.argv) > 1 else ""))
    if not str(root):
        sys.exit("HIVEY_SWARM_ROOT is not set (open with `hivey swarm addon <swarm> hivey.github`)")
    manifest_path = root / ".swarm" / "agents.json"
    manifest = json.loads(manifest_path.read_text())
    slug = os.environ.get("HIVEY_SWARM_SLUG") or manifest["slug"]
    config = dict(DEFAULT_CONFIG)
    try:
        config.update(json.loads((Path(os.environ.get("HERDR_PLUGIN_CONFIG_DIR", ".")) / "config.json").read_text()))
    except (OSError, json.JSONDecodeError):
        pass
    # Read-only lookup: runs in dry-run mode too.
    owner = config["owner"] or subprocess.run(
        ["gh", "api", "user", "-q", ".login"], capture_output=True, text=True).stdout.strip() or "<you>"
    product = find_product(root, manifest, config)
    product_repo, swarm_repo = repo_names(slug, config)
    published = manifest.get("github") or {}

    print(f"hivey · GitHub for swarm {slug}{'   (dry run)' if DRY_RUN else ''}\n")
    if product:
        print(f"  product    {root / product}\n             → {published.get('product') or f'{owner}/{product_repo}'}")
    else:
        print("  product    (none found: set product_dir in the manifest or product_dirs in config)")
    print(f"  workspace  {root}  (without {product + '/' if product else 'the product'}, worktrees, secrets)"
          f"\n             → {published.get('swarm') or f'{owner}/{swarm_repo}'}")
    print(f"  visibility {config['visibility']}\n")

    if not published.get("swarm"):
        answer = ask("create and push these repos? [y/N] ").lower()
        if answer != "y":
            print("nothing created.")
        else:
            try:
                if product:
                    publish(root / product, f"{owner}/{product_repo}", config["visibility"],
                            "initial commit")
                    published["product"] = f"{owner}/{product_repo}"
                gi = root / ".gitignore"
                gi_text = workspace_gitignore(gi.read_text() if gi.exists() else "", product or "")
                if DRY_RUN:
                    print(f"  [dry-run] write {gi}:\n" + "\n".join("    " + l for l in gi_text.splitlines()))
                else:
                    gi.write_text(gi_text)
                publish(root, f"{owner}/{swarm_repo}", config["visibility"], "swarm workspace")
                published["swarm"] = f"{owner}/{swarm_repo}"
                if not DRY_RUN:
                    manifest = json.loads(manifest_path.read_text())
                    manifest["github"] = published
                    tmp = manifest_path.with_suffix(".tmp")
                    tmp.write_text(json.dumps(manifest, indent=2) + "\n")
                    tmp.replace(manifest_path)
                print(f"\n  ✓ published: {', '.join(published.values())}")
            except RuntimeError as err:
                print(f"\n  ✗ {err}")

    while True:
        choice = ask("\n[p] push both now · [s] status · [q] quit: ").lower()
        if choice == "p":
            try:
                push_both(root, product)
            except RuntimeError as err:
                print(f"  ✗ {err}")
        elif choice == "s":
            for label, path in (("product", root / product if product else None), ("workspace", root)):
                if path and is_repo(path):
                    status = git(path, "status", "--short", "--branch", check=False)
                    print(f"  {label}: {status.splitlines()[0] if status else '?'}"
                          f" ({max(0, len(status.splitlines()) - 1)} changed)")
        elif choice == "q":
            return


if __name__ == "__main__":
    main()
