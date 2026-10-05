# hivey — everything that can be an addon

Status: proposal · 2026-09-30

**Idea:** hivey's core is small. It holds the swarm model (roster, roles), the message bus
with its delivery policy, the launcher, and the addon host. Everything else is an addon,
**including the terminal runtime** (herdr today), so hivey can run swarms on other
terminal multiplexers too (tmux, cmux, …).

## What exists today

| Addon | Kind | Status |
|---|---|---|
| `hivey.slack-relay` | pane | done: Slack ↔ bus, mirror scope, token command |
| `hivey.dashboard` | pane | done: states, token bars + sparkline, budget, tasks, messages |
| `hivey.github` | pane (interactive) | done: product repo + swarm workspace repo, gitleaks gate |

Contract: [`plugins/README.md`](../plugins/README.md). Addons are ordinary herdr-format plugins,
installable from any GitHub repo (`hivey plugin install owner/repo/path`).

## Candidates

| # | Part (today) | Addon | Why it should be swappable |
|---|---|---|---|
| 1 | **Terminal runtime** (herdr core, compiled in) | **runtime driver**: `herdr` (native, today), `tmux`, `cmux`, `zellij`, `wezterm`, `kitty` | Use hivey with the multiplexer you already run; herdr stays the richest driver (tree sidebar, colored titles). |
| 2 | Claude-specific code in the launcher and dashboard (start args, folder-trust dialog, transcript token counting) | **agent adapters**: `claude`, `codex`, `opencode`, `gemini`, … | Mixed swarms (a Codex builder beside a Claude critic); each adapter knows how to start, unblock, and meter its agent. |
| 3 | Slack relay | more **relays**: Discord, Telegram, email, webhook | Done for Slack; the contract already fits others. |
| 4 | Watcher in the skill's `swarm_relay.py` (idle / stall / budget alerts) | **supervisor** addon | Alerting policy varies per team (quiet hours, escalate to phone, auto-nudge). |
| 5 | Task board (`swarm_tasks.py`) + planned whiteboard | **board** addon (CLI + panel) | Some teams want GitHub Issues or Linear instead of a local board. |
| 6 | Chrome Gantt dashboard (`swarm_dashboard.py`) | **web dashboard** addon | Same data as `hivey.dashboard`, rendered in a browser. |
| 7 | GitHub publishing | **VCS/publish** addons: GitLab, Gitea, "zip to Drive" | Done for GitHub. |
| 8 | Obsidian wiki vault (skill steps 5 and 8) | **memory** addon: Obsidian, plain markdown, a vector store | Memory backends differ per user. |
| 9 | Stop-hook voice (`tts`, AGENTS hooks) | **notifier** addon: voice, macOS notification, phone push | Personal preference. |
| 10 | Budget (minutes only) | **budget/cost policy** addon: token caps per agent, auto-pause at a limit | Money matters differ per project. |
| 11 | Team design in `/swarm` | **swarm templates**: "research swarm", "Expo app swarm" (roster + brief skeletons + checks) | Reusable, shareable team recipes. |
| 12 | `envsave` token lookups | **secrets provider** addon | Other users use 1Password, Keychain, `pass`. |

## Addon kinds needed

- **pane** (exists): a long-running process in the swarm's space. Relays, dashboards, interactive tools.
- **action** (herdr plugins already support these): a one-shot command, bindable to a key.
  E.g. "push both" (`prefix+g`), "pause swarm".
- **event hook**: the engine emits swarm events (`msg.sent`, `msg.delivered`, `agent.blocked`,
  `budget.reached`) to subscribing plugins. This needs one engine change (publish swarm events
  on the existing event hub); then relays and supervisors no longer need to poll `bus.jsonl`.
- **driver** (new): a process that implements the runtime protocol below.
- **adapter** (new): per agent kind; a small manifest plus commands (start args, unblock
  rules, usage meter).

## Runtime drivers: herdr as one addon

The engine and launcher were built as API clients, so they already use a small, well-defined
set of calls. That set becomes **hivey runtime protocol v1** (the same JSON as herdr's socket API):

- agents: `agent.list`, `agent.get`, `agent.start`, `agent.prompt`, `agent.read`, `agent.send_keys`, `agent.wait`, `agent.rename`, `agent.focus`
- panes: `pane.get`, `pane.list`, `pane.split`, `pane.move`, `pane.layout`, `pane.focus`, `pane.report_metadata`
- spaces: `workspace.create`, `workspace.rename`
- addons: `plugin.pane.open`

Plan:
1. **Engine behind a `Runtime` trait.** Two implementations: *in-process* (today: inside the herdr-based server) and *socket* (any process speaking protocol v1). The swarm engine can then also run as a standalone daemon: `hivey daemon --runtime <socket>`.
2. **herdr driver = the current fork.** No change; it's protocol v1 natively, plus the native UI (tree sidebar, role colors).
3. **tmux driver** (first foreign driver): a small process that maps protocol v1 onto `tmux` commands (`split-window`, `send-keys`, `capture-pane`, `select-pane -T` for titles). Agent status comes from herdr's own detection engine applied to `capture-pane` output, reused as a library; that's the hard part, and the reason herdr stays the best driver.
4. The UI parts that only herdr has (tree, colors) degrade gracefully elsewhere: on tmux, the `hivey.dashboard` pane is the swarm overview.

Cost: step 1 is small (one trait over the existing dispatch). Step 3 is a separate project (~1–2 weeks), mostly about status detection.

## Suggested order
1. `/swarm` skill switch-over (in progress): launch with `--addon hivey.slack-relay --addon hivey.dashboard`, the watcher as the skill's own addon, product/workspace split for GitHub.
2. Event hooks (small engine change) → relays and supervisor stop polling.
3. Agent adapters (#2), since mixed swarms are an immediate win.
4. `Runtime` trait + standalone daemon (step 1 above).
5. tmux driver.
