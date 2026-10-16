# Dashboards and the watcher

Read this when the user asks what the dashboards show, or how to read them.

## hivey.dashboard (pane in the swarm's space)

A terminal pane beside the agents: each agent's state (working, idle, blocked), tokens per
agent, active time against the budget, task counts per status from `.swarm/tasks.json`, and the
latest messages. Reopen it with `hivey swarm addon <name> hivey.dashboard` (e.g. after `q`).

## hivey.swarm-watcher (alerts + Chrome Gantt)

Started with `--addon hivey.swarm-watcher`. It watches every worker and messages you (the
master) when one:
- is **blocked** (an approval or question dialog), at once;
- has **stopped** (idle for a while): it probably finished, is waiting on someone, or asked
  something in its output and is waiting;
- is **stalled**: working a long time with no activity (no message, no task-board change),
  possibly looping or lost;
- and when the **time budget** (`--budget-min`) is 80% and 100% used.

Handle each alert as in step 8 of the skill. If alerts are too noisy, see the plugin's options
(`hivey plugin config-dir hivey.swarm-watcher`).

It also keeps a live **Gantt dashboard**, opened once in the browser and rebuilt every minute
from `.swarm/agents.json`, the task board, the event log and the agents' states, at
`<launch dir>/diagrams/dashboard-swarm-<slug>.html` (the page reloads itself):
- **Gantt**: one lane per agent, with its state over time and one bar per task colored by status
  (in-progress, review, blocked, approved). Tasks not yet started show what they wait on. Lines
  mark "now" and the budget.
- **Header**: time used vs budget, task counts, one card per agent (state, CLI/model, current
  task).
- **Recent events**: claims, status changes, critiques, alerts.
- **Tokens**: the swarm total, the master's share, and each agent's total (tokens only, no
  prices, since price tables go stale).
- **Freshness**: an "updated N s ago" stamp turns amber, then red, when the watcher stops
  updating it; check that the watcher pane is running (`hivey swarm info <name>`).

It's pure script and costs no tokens. Rebuild or reopen it by hand with
`python3 <plugin folder>/dashboard.py <root> --open` (the folder is in `hivey plugin list`).
Give the user its path when you report the launch.

If the `archify` skill is installed, refresh its "where the work stands" diagram
(`references/archify-report.md`) at milestones (a task approved, an agent FINISHED, a blocker
resolved) and when the user asks, not on every monitoring pass: each diagram costs real model
work, while the dashboards update for free.
