# Write your own swarm setup provider

hiver runs swarms; it doesn't decide **how a team is designed**. That's the job of a
**setup provider**: a plugin you can write in any language, like the included `/swarm` skill
(`swarm.skill`) or the example `hiver.team-template`.

See the flow as a diagram: [`diagrams/hiver-new-swarm.html`](diagrams/hiver-new-swarm.html).

```
you ── hiver swarm new "task" ──► hiver opens YOUR plugin's "setup" pane in your folder
                                     │  1 DESIGN  the team for the task (ask the user to confirm)
                                     │  2 BRIEF   write <root>/<agent>/CLAUDE.md for each agent
                                     │  3 LAUNCH  hiver swarm launch <root> --slug … <agents…>
                                     ▼  4 MASTER  this pane becomes the master (e.g. exec claude)
                           hiver runs the swarm: own space, addons, message bus, schedules
```

## The contract

| Your plugin… | hiver… |
|---|---|
| declares a pane with `id = "setup"` in `herdr-plugin.toml` | lists it in `hiver swarm providers`, opens it for `hiver swarm new` |
| reads `HIVER_SETUP_TASK` (the task) and `HIVER_SETUP_CWD` (the user's folder) | sets both, plus `HERDR_BIN_PATH` (the hiver binary) and `HERDR_PLUGIN_CONFIG_DIR` |
| writes one folder per agent with its brief: `CLAUDE.md` (Codex reads `AGENTS.md`; hiver links it if missing) | starts each agent in its folder, with the brief as its instructions |
| runs `hiver swarm launch <root> --slug <slug> <agents…> [options]` **from its own pane** | moves that pane into a new space named after the swarm as **pane 1, the master**; opens addons; starts agents; writes `<root>/.swarm/agents.json` |
| turns its pane into the master (any CLI, usually Claude or Codex) | finds the master by its pane, delivers messages to it when idle, wakes it on schedule |
| optionally adds `info` to the manifest | shows it in the hover card and `hiver swarm info` |

### `hiver swarm launch` options a provider uses

| Option | Effect |
|---|---|
| `<agent>...` | one folder + pane per agent (names: `[a-z][a-z0-9_-]`, slug + name ≤ 32 chars) |
| `--models a=sonnet,b=opus` | model per agent |
| `--kinds b=codex` | run an agent with Codex instead of Claude Code |
| `--claude-args "…"`, `--codex-args "…"` | CLI flags (defaults: no approval prompts) |
| `--kickoff "…"` | first prompt each agent gets (default: read your brief, use `hiver msg`) |
| `--addon hiver.dashboard`, `--addon hiver.slack-relay` (+ `--channel C0…`) | addon panes in the swarm's space |
| `--heartbeat 15m --heartbeat-task "…"` | wake the master on a schedule with your monitoring instructions |
| `--budget-min 120` | time budget shown in the dashboard and info |

More schedules after launch: `hiver swarm schedule add <slug> --at 09:00 "…"`.

## Walkthrough: the example provider

[`plugins/team-template`](../plugins/team-template) is about 140 lines of Python. Copy the folder
to start your own.

**`herdr-plugin.toml`**: a pane named `setup`. The command runs with your folder as its working
directory, so reach your files through `$HERDR_PLUGIN_ROOT`:

```toml
id = "me.my-provider"
name = "My provider"
version = "0.1.0"
min_herdr_version = "0.9.0"
description = "What kind of team this designs"
platforms = ["linux", "macos"]

[[panes]]
id = "setup"
title = "my provider"
placement = "tab"
command = ["sh", "-c", "exec python3 \"$HERDR_PLUGIN_ROOT/setup.py\""]
```

**`setup.py`**, the four steps:

```python
task = os.environ.get("HIVER_SETUP_TASK") or input("What should the swarm do? ")
root = Path(os.environ["HIVER_SETUP_CWD"]) / f"swarm-{slug}"

# 1 DESIGN: here a fixed team; yours could ask questions, call an LLM, read a template…
TEAM = {"builder": "Build …", "critic": "Review …"}
if input("Launch this swarm? [Y/n] ").lower() in ("n", "no"):
    sys.exit("cancelled")

# 2 BRIEF: one folder + CLAUDE.md per agent (tell them about `hiver msg send`)
for agent, job in TEAM.items():
    (root / agent).mkdir(parents=True, exist_ok=True)
    (root / agent / "CLAUDE.md").write_text(BRIEF.format(agent=agent, job=job, …))

# 3 LAUNCH: from this pane, so it becomes the master
subprocess.run([hiver, "swarm", "launch", str(root), "--slug", slug, *TEAM,
                "--models", "builder=sonnet,critic=sonnet", "--addon", "hiver.dashboard",
                "--heartbeat", "30m"], check=True)

# 4 MASTER: answer Claude's folder-trust prompt in the background, then become the coordinator
subprocess.Popen([hiver, "swarm", "accept-trust", "--pane", os.environ["HERDR_PANE_ID"],
                  "--kind", "claude"], start_new_session=True)
os.chdir(root)
os.execvp("claude", ["claude", "--dangerously-skip-permissions", "--model", "opus", MASTER_PROMPT])
```

Optionally, after launch, write facts for the hover card:

```python
manifest = json.loads((root / ".swarm/agents.json").read_text())
manifest["info"] = {"setup": "me.my-provider", "task": task[:70]}
(root / ".swarm/agents.json").write_text(json.dumps(manifest, indent=2))
```

## Install, run, test

```bash
hiver plugin link ~/path/to/my-provider          # or: hiver plugin install owner/repo/path
hiver swarm providers                            # it's listed
cd ~/projects/<project>
hiver swarm new --provider me.my-provider --default "<task>"   # --default: use it next time
```

Testing tips:
- Put cheap settings in your plugin's config while developing. The example reads
  `$(hiver plugin config-dir hiver.team-template)/config.json`, e.g.
  `{"model": "haiku", "master_model": "haiku", "heartbeat": ""}`.
- A provider whose setup pane only prints `$HIVER_SETUP_TASK` lets you check the wiring without
  starting any agent.
- `hiver swarm list`, `hiver swarm info <slug>` and `hiver msg log --swarm <slug>` show what
  happened; `hiver swarm forget <slug>` and closing the space clean up.
- Unit-test the pure parts (naming, briefs, config) like `plugins/team-template/test_setup.py`.

## Ideas for providers

- **Templates:** "Expo app team", "research + writer + fact-checker", "data pipeline".
- **Interactive designers:** ask a few questions, or let an LLM propose the team (that's what
  `/swarm` does), then launch.
- **From an issue:** read a GitHub issue and build a team to resolve it.
- **Mixed teams:** Codex builders (`--kinds builder=codex`) reviewed by a Claude critic.
