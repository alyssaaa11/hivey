#!/usr/bin/env python3
"""hiver.team-template: the smallest useful swarm setup provider (an example to copy).

What every setup provider does, in four steps:
  1. DESIGN   decide the team for the task (here: a fixed builder + critic)
  2. BRIEF    write each agent's brief to <root>/<agent>/CLAUDE.md
  3. LAUNCH   run `hiver swarm launch …` (space, panes, addons, schedules)
  4. MASTER   turn this pane into the master (here: a Claude coordinator)

hiver opens this pane for `hiver swarm new --provider hiver.team-template "<task>"` with
HIVER_SETUP_TASK and HIVER_SETUP_CWD set. Settings (all optional) in
$HERDR_PLUGIN_CONFIG_DIR/config.json:
  {"model": "sonnet", "master_model": "opus", "heartbeat": "30m",
   "addons": ["hiver.dashboard"], "claude_args": "--dangerously-skip-permissions",
   "slack": "ask"}
  slack: "ask" (when Slack is connected: offer a channel #<slug>), true, or false.
"""
import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

DEFAULTS = {
    "model": "sonnet",
    "master_model": "opus",
    "heartbeat": "30m",
    "addons": ["hiver.dashboard"],
    "claude_args": "--dangerously-skip-permissions",
    "slack": "ask",
}

# --- 1. DESIGN ------------------------------------------------------------------
TEAM = {
    "builder": "Build what the task asks for in {root}/app. Commit small steps. When a piece "
               "works, tell the critic: hiver msg send critic \"DONE <what> → <path>\".",
    "critic": "Review everything the builder marks DONE: correctness, tests, clarity. Send "
              "concrete fixes to the builder; when it's good, tell the coordinator APPROVED.",
}

BRIEF = """# {agent} — swarm "{slug}"

**Task of the swarm:** {task}

## Your job
{job}

## Team
- `coordinator` (master): plans, answers questions, reports to the user
- `builder`: builds the product in `{root}/app`
- `critic`: reviews the builder's work

## Talking to teammates (hiver)
- `hiver msg send <agent> "<msg>"`: delivered when they're idle
- `hiver msg send <agent> --fyi "<msg>"`: acknowledgements and status, never wakes anyone
- `hiver msg inbox`: messages waiting for you
Ask when something is unclear; don't guess.
"""

MASTER = """You are the coordinator (master) of the swarm "{slug}" in {root}.
Task: {task}
Team: builder (builds in {root}/app), critic (reviews). They are running now.
Plan the work, send each agent its first assignment with `hiver msg send <agent> "…"`,
answer their questions, and report progress to the user. `hiver swarm list` shows the team;
`hiver msg log` shows the conversation. The user may also write from Slack: those messages
arrive from `human`; answer them with `hiver msg send human "…"`."""


def slugify(task):
    words = re.sub(r"[^a-z0-9]+", "-", task.lower()).strip("-").split("-")
    slug = "-".join(w for w in words if w)[:18].strip("-")
    return slug or "swarm"


def config():
    path = Path(os.environ.get("HERDR_PLUGIN_CONFIG_DIR", ".")) / "config.json"
    try:
        return {**DEFAULTS, **json.loads(path.read_text())}
    except (OSError, json.JSONDecodeError):
        return dict(DEFAULTS)


def want_slack(cfg, hiver, slug):
    """Whether the swarm gets its own Slack channel #<slug> (`hiver swarm launch --slack`)."""
    if cfg["slack"] is False:
        return False
    if subprocess.run([hiver, "slack", "status"], capture_output=True).returncode != 0:
        print(f"Slack is not connected, so no channel (later: hiver slack connect, "
              f"then hiver slack add {slug})")
        return False
    if cfg["slack"] is True:
        return True
    return input(f"Give it a Slack channel #{slug}, to talk to it from Slack? [Y/n] "
                 ).strip().lower() not in ("n", "no")


def main():
    cfg = config()
    hiver = os.environ.get("HERDR_BIN_PATH", "hiver")
    task = os.environ.get("HIVER_SETUP_TASK", "").strip() or input("What should the swarm do? ").strip()
    if not task:
        sys.exit("no task")
    slug = slugify(task)
    root = Path(os.environ.get("HIVER_SETUP_CWD") or os.getcwd()) / f"swarm-{slug}"

    print(f"hiver team template\n\n  task   {task}\n  root   {root}")
    print(f"  team   coordinator ({cfg['master_model']}) · builder ({cfg['model']}) · critic ({cfg['model']})")
    print(f"  addons {', '.join(cfg['addons']) or 'none'} · heartbeat {cfg['heartbeat'] or 'none'}\n")
    if input("Launch this swarm? [Y/n] ").strip().lower() in ("n", "no"):
        sys.exit("cancelled")
    slack = want_slack(cfg, hiver, slug)

    # --- 2. BRIEF -----------------------------------------------------------------
    (root / "app").mkdir(parents=True, exist_ok=True)
    for agent, job in TEAM.items():
        (root / agent).mkdir(exist_ok=True)
        (root / agent / "CLAUDE.md").write_text(
            BRIEF.format(agent=agent, slug=slug, task=task, root=root, job=job.format(root=root)))

    # --- 3. LAUNCH ----------------------------------------------------------------
    cmd = [hiver, "swarm", "launch", str(root), "--slug", slug, *TEAM,
           "--models", ",".join(f"{a}={cfg['model']}" for a in TEAM),
           "--claude-args", cfg["claude_args"]]
    for addon in cfg["addons"]:
        cmd += ["--addon", addon]
    if cfg["heartbeat"]:
        cmd += ["--heartbeat", cfg["heartbeat"]]
    if slack:
        cmd += ["--slack"]
    launched = subprocess.run(cmd, capture_output=True, text=True)
    sys.stderr.write(launched.stderr)
    if launched.returncode != 0:
        sys.exit(f"launch failed ({launched.returncode})")
    # Facts for hiver's hover card / `hiver swarm info`.
    manifest_path = root / ".swarm" / "agents.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["info"] = {"setup": "hiver.team-template", "task": task[:70]}
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")

    # --- 4. MASTER ----------------------------------------------------------------
    # Launch moved this pane into the swarm's space as pane 1; it becomes the coordinator.
    # Claude asks to trust the new folder first: let hiver answer that in the background.
    pane = os.environ.get("HERDR_PANE_ID", "")
    subprocess.Popen([hiver, "swarm", "accept-trust", "--pane", pane, "--kind", "claude"],
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    os.chdir(root)
    args = ["claude", *shlex.split(cfg["claude_args"]), "--model", cfg["master_model"],
            MASTER.format(slug=slug, root=root, task=task)]
    os.execvp("claude", args)


if __name__ == "__main__":
    main()
