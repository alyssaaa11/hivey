#!/usr/bin/env python3
"""hivey.team-template: the smallest useful swarm setup provider (an example to copy).

What every setup provider does, in four steps:
  1. DESIGN   decide the team for the task (here: a fixed builder + critic)
  2. BRIEF    write each agent's brief to <root>/<agent>/CLAUDE.md
  3. LAUNCH   run `hivey swarm launch …` (space, panes, addons, schedules)
  4. MASTER   turn this pane into the master (here: a Claude coordinator)

hivey opens this pane for `hivey swarm new --provider hivey.team-template "<task>"` with
HIVEY_SETUP_TASK and HIVEY_SETUP_CWD set. Settings (all optional) in
$HERDR_PLUGIN_CONFIG_DIR/config.json:
  {"model": "sonnet", "master_model": "opus", "heartbeat": "30m",
   "addons": ["hivey.dashboard"], "claude_args": "--dangerously-skip-permissions",
   "slack": "ask", "wiki": "ask"}
  slack: "ask" (when Slack is connected: offer a channel #<slug>), true, or false.
  wiki: "ask" (offer an Obsidian wiki vault as the team's memory), true, or false.
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
    "addons": ["hivey.dashboard"],
    "claude_args": "--dangerously-skip-permissions",
    "slack": "ask",
    "wiki": "ask",
}

# --- 1. DESIGN ------------------------------------------------------------------
TEAM = {
    "builder": "Build what the task asks for in {root}/app. Commit small steps. When a piece "
               "works, tell the critic: hivey msg send critic \"DONE <what> → <path>\".",
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

## Talking to teammates (hivey)
- `hivey msg send <agent> "<msg>"`: delivered when they're idle
- `hivey msg send <agent> --fyi "<msg>"`: acknowledgements and status, never wakes anyone
- `hivey msg inbox`: messages waiting for you
Ask when something is unclear; don't guess.
"""

MASTER = """You are the coordinator (master) of the swarm "{slug}" in {root}.
Task: {task}
Team: builder (builds in {root}/app), critic (reviews). They are running now.
Plan the work, send each agent its first assignment with `hivey msg send <agent> "…"`,
answer their questions, and report progress to the user. `hivey swarm list` shows the team;
`hivey msg log` shows the conversation. The user may also write from Slack: those messages
arrive from `human`; answer them with `hivey msg send human "…"`. If you need a Slack channel,
use `hivey slack add {slug}`: it creates #{slug} and invites the user at once."""


def slugify(task):
    words = re.sub(r"[^a-z0-9]+", "-", task.lower()).strip("-").split("-")
    slug = "-".join(w for w in words if w)[:18].strip("-")
    return slug or "swarm"


NAME_RE = re.compile(r"^[a-z][a-z0-9-]{0,31}$")


def ask_name(task):
    """The swarm's name, asked with a suggestion from the task. One name for everything: the
    swarm slug, its Slack channel #<name> and its Obsidian vault <name>-wiki."""
    suggestion = slugify(task)
    while True:
        answer = input(f"Name (swarm, Slack channel #name, Obsidian vault name-wiki) "
                       f"[{suggestion}]: ").strip()
        name = re.sub(r"[^a-z0-9]+", "-", answer.lower()).strip("-") if answer else suggestion
        if NAME_RE.match(name):
            return name
        print("  a-z, 0-9 and -, starting with a letter, at most 32 characters")


def config():
    path = Path(os.environ.get("HERDR_PLUGIN_CONFIG_DIR", ".")) / "config.json"
    try:
        return {**DEFAULTS, **json.loads(path.read_text())}
    except (OSError, json.JSONDecodeError):
        return dict(DEFAULTS)


WIKI_SETTINGS = Path.home() / ".hivey" / "wiki.json"


def new_wiki_script():
    """The hivey skill's new_wiki.py: next to this plugin in the repo, else installed."""
    here = Path(__file__).resolve().parents[2] / "skills" / "hivey" / "scripts" / "new_wiki.py"
    installed = Path.home() / ".claude" / "skills" / "hivey" / "scripts" / "new_wiki.py"
    return next((p for p in (here, installed) if p.is_file()), None)


def want_wiki(cfg, slug):
    """None, or the Obsidian folder for the team's wiki vault (asked the first time)."""
    if cfg["wiki"] is False or new_wiki_script() is None:
        return None
    if cfg["wiki"] is not True and input(
            f"Give it an Obsidian wiki as memory ({slug}-wiki, where the team keeps what it "
            "learns)? [Y/n] ").strip().lower() in ("n", "no"):
        return None
    try:
        return json.loads(WIKI_SETTINGS.read_text())["dir"]
    except (OSError, ValueError, KeyError):
        default = str(Path.home() / "Obsidian")
        answer = input(f"Where is your Obsidian folder? [{default}] ").strip()
        return answer or default


def make_wiki(slug, task, obsidian, agent_dirs):
    """Creates (or reuses) the vault and links every agent's CLAUDE.md to it; its path."""
    cmd = [sys.executable, str(new_wiki_script()), slug, "--dir", obsidian, "--about", task[:80]]
    for agent_dir in agent_dirs:
        cmd += ["--agent", str(agent_dir)]
    done = subprocess.run(cmd, capture_output=True, text=True)
    try:
        return json.loads(done.stdout)["path"]
    except (ValueError, KeyError):
        sys.stderr.write(done.stderr or "could not create the wiki\n")
        return None


def want_slack(cfg, hivey, slug):
    """Whether the swarm gets its own Slack channel #<slug> (`hivey swarm launch --slack`)."""
    if cfg["slack"] is False:
        return False
    if subprocess.run([hivey, "slack", "status"], capture_output=True).returncode != 0:
        print(f"Slack is not connected, so no channel (later: hivey slack connect, "
              f"then hivey slack add {slug})")
        return False
    if cfg["slack"] is True:
        return True
    return input(f"Give it a Slack channel #{slug}, to talk to it from Slack? [Y/n] "
                 ).strip().lower() not in ("n", "no")


def main():
    cfg = config()
    hivey = os.environ.get("HERDR_BIN_PATH", "hivey")
    task = os.environ.get("HIVEY_SETUP_TASK", "").strip() or input("What should the swarm do? ").strip()
    if not task:
        sys.exit("no task")
    slug = ask_name(task)
    root = Path(os.environ.get("HIVEY_SETUP_CWD") or os.getcwd()) / f"swarm-{slug}"

    print(f"hivey team template\n\n  name   {slug}\n  task   {task}\n  root   {root}")
    print(f"  team   coordinator ({cfg['master_model']}) · builder ({cfg['model']}) · critic ({cfg['model']})")
    print(f"  addons {', '.join(cfg['addons']) or 'none'} · heartbeat {cfg['heartbeat'] or 'none'}\n")
    if input("Launch this swarm? [Y/n] ").strip().lower() in ("n", "no"):
        sys.exit("cancelled")
    slack = want_slack(cfg, hivey, slug)
    obsidian = want_wiki(cfg, slug)

    # --- 2. BRIEF -----------------------------------------------------------------
    (root / "app").mkdir(parents=True, exist_ok=True)
    for agent, job in TEAM.items():
        (root / agent).mkdir(exist_ok=True)
        (root / agent / "CLAUDE.md").write_text(
            BRIEF.format(agent=agent, slug=slug, task=task, root=root, job=job.format(root=root)))
    wiki = make_wiki(slug, task, obsidian, [root / agent for agent in TEAM]) if obsidian else None
    if wiki:
        print(f"wiki memory: {wiki}")

    # --- 3. LAUNCH ----------------------------------------------------------------
    cmd = [hivey, "swarm", "launch", str(root), "--slug", slug, *TEAM,
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
    # Facts for hivey's hover card / `hivey swarm info`.
    manifest_path = root / ".swarm" / "agents.json"
    manifest = json.loads(manifest_path.read_text())
    manifest["info"] = {"setup": "hivey.team-template", "task": task[:70]}
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")

    # --- 4. MASTER ----------------------------------------------------------------
    # Launch moved this pane into the swarm's space as pane 1; it becomes the coordinator.
    # Claude asks to trust the new folder first: let hivey answer that in the background.
    pane = os.environ.get("HERDR_PANE_ID", "")
    subprocess.Popen([hivey, "swarm", "accept-trust", "--pane", pane, "--kind", "claude"],
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    os.chdir(root)
    args = ["claude", *shlex.split(cfg["claude_args"]), "--model", cfg["master_model"],
            MASTER.format(slug=slug, root=root, task=task)
            + (f"\nThe team's memory is the Obsidian wiki {wiki} (rules in its CLAUDE.md): "
               "read it before planning and keep it updated." if wiki else "")]
    os.execvp("claude", args)


if __name__ == "__main__":
    main()
