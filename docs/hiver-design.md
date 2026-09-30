# hiver — design doc

> A fork of [herdr](https://github.com/herdrdev/herdr) (Apache-2.0) built for **swarms of agents**:
> one swarm per space, a master you can always find, a built-in message bus, and a shared whiteboard.

Status: draft v1 · 2026-09-30 · base: herdr `main` @ 2026-09-30 (0.9.3+)

---

## 1. Problem

Running several `/swarm` teams in herdr today (see screenshot in the conversation):

| Pain | Root cause in herdr / the swarm skill |
|---|---|
| Spaces list doesn't show swarms | The skill opens each swarm as a **tab** in the coordinator's space; two swarms share `satellites`. |
| Can't find the master to talk to | The Agents panel is a flat list of every pane; the master is one row of ~15. |
| Every pane border says `claude` | Border title = metadata title → manual label → agent kind. Launcher sets neither title nor label. |
| Can't tell agent vs critic vs script | No notion of role; only `slack-relay` is renamed. |
| Agent↔agent talk is slow and noisy | No bus: Slack + a relay polling every 10 s. An `@all` FYI wakes every agent, each spends a turn to answer "DONE: noted, no action". |
| Task board / notes are files behind a lock | `swarm_tasks.py` + `tasks.lock`; no live view except a Chrome dashboard. |
| Watcher is a script in a pane | `swarm_relay.py`; dies with its pane, duplicates state herdr already has. |

## 2. Goals / non-goals

**Goals**
1. A swarm is a first-class object: **one swarm = one space**, one folder per agent, master in pane 1.
2. The sidebar is a **tree**: Swarm ▸ master ▸ workers / critic / scripts. Collapsed = masters only.
3. **Role is visible everywhere**: glyph + color on sidebar rows and pane borders.
4. **Native message bus**: direct, broadcast, role-addressed; delivered when the agent is idle; FYIs that don't wake anyone.
5. **Whiteboard**: task board + shared notes, one writer (the server), a live panel.
6. **Supervisor** in the server: blocked / idle / stalled / gone / budget alerts to the master.
7. Stay **rebasable on upstream herdr** — we want its fixes forever.
8. **Compatible with existing `.swarm/` folders** and the swarm skill's scripts during migration.

**Non-goals (v1)**
- Designing teams or writing CLAUDE.md files — stays in the `/swarm` skill (needs an LLM).
- Obsidian vault management — stays in the skill.
- Swarms spanning SSH machines (local only in v1; herdr's remote machinery keeps working for solo panes).
- Mobile layout for the new views (falls back to herdr's current mobile Agents list).

## 3. Concepts

| hiver concept | Backed by | Notes |
|---|---|---|
| **Swarm** | a herdr `Workspace` + `<root>/.swarm/` | `Workspace.swarm: Option<SwarmMembership { root, slug }>` |
| **Agent** | a pane in the swarm's workspace + `<root>/<agent>/` | name `<slug>-<agent>` (unchanged, herdr names are server-global) |
| **Role** | `master` · `worker` · `critic` · `script` · `human` | stored in `agents.json`, mirrored to pane metadata token `role` |
| **Message** | line in `<root>/.swarm/bus.jsonl` | durable; in-memory queue per agent |
| **Board** | `tasks.json` (existing format) + `<root>/.swarm/board/*.md` | server is the only writer |
| **Solo space** | any workspace with `swarm = None` | rendered exactly like herdr today |

The Space/Agent split in herdr goes away in the UI: a swarm row *is* its space, its children *are* its agents.

## 4. Fork strategy ("soft fork")

herdr is ~308k lines of Rust with releases every 1–2 weeks. A hard fork rots in a month.

**Rule: all swarm logic lives in new modules; core files get small, named hooks only.**

```
src/swarm/            NEW  model, manifest I/O, roles, supervisor
src/swarm/bus.rs      NEW  message bus + delivery policy
src/swarm/board.rs    NEW  tasks + notes, single-writer
src/api/schema/swarm.rs NEW  swarm.* / msg.* / board.* / task.* methods + events
src/client/shell/swarm_sidebar.rs NEW  tree renderer
src/client/shell/board_view.rs    NEW  whiteboard panel
src/cli/swarm.rs      NEW  `hiver swarm|msg|board|task …`
```

Core files we expect to touch (the "hook budget" — keep each diff small and commented `// hiver:`):

| File | Change |
|---|---|
| `src/workspace.rs` | `swarm: Option<SwarmMembership>` field |
| `src/persist/snapshot.rs` | persist that field (mirrors `worktree_space`) |
| `src/api/schema.rs`, `src/api/server.rs` | new `Method` variants → delegate to `src/swarm` |
| `src/client/shell/sidebar.rs`, `endpoint_sidebar.rs` | swap in tree renderer when any swarm exists; carry swarm data over the client/server endpoint protocol |
| `src/ui/panes.rs` | border color from role (today: focus accent only) |
| `src/config/*` | `[swarm]` section |
| `src/main.rs`, `build.rs`, packaging | binary / product name |

**Upstream sync**: `upstream` remote = herdrdev/herdr; rebase `main` weekly (CI job opens a PR with the rebase; `just test` must pass). Anything generally useful (per-pane border color from a token, grouping the agent panel by token) is offered upstream as a PR so the hook budget shrinks over time.

**Naming & coexistence**
- Repo `hiver` (private), binary `hiver`, config `~/.config/hiver/`, socket/session dir separate from herdr → both run side by side during migration.
- Set `HIVER_ENV=1` **and** `HERDR_ENV=1` in panes; accept `herdr`-style CLI so existing skill scripts work if `herdr` is symlinked to `hiver`.
- Apache-2.0: keep `LICENSE`, add `NOTICE` ("hiver is derived from herdr, © herdr authors"), mark modified files, don't use herdr's name/logo for the product.

## 5. Data model (on disk)

Existing `.swarm/agents.json` stays valid. Additive fields only:

```jsonc
{
  "slug": "app-ideas",
  "root": "/Users/jcsancho/swarms/apps_ideas",
  "coordinator": "app-ideas-coordinator",
  "workspace_id": "wB",            // NEW (replaces tab_id; tab_id still read)
  "layout": "master-left",         // NEW  master-left | master-tab
  "agents": {
    "coordinator": { "herdr_name": "app-ideas-coordinator", "role": "master", "model": "opus" }, // NEW entry
    "scout":  { "herdr_name": "app-ideas-scout",  "role": "worker", "model": "sonnet", "pane_id": "wB:p9" },
    "critic": { "herdr_name": "app-ideas-critic", "role": "critic", "model": "opus" },
    "relay":  { "role": "script", "command": ["python3", "…/swarm_relay.py", "…"] }             // NEW
  },
  "budget_minutes": 120,
  "supervisor": { "idle_after_s": 60, "stall_after_s": 2700 }   // NEW (defaults from config)
}
```

Missing `role` → inferred: `coordinator` → master, `critic` → critic, entries with `command` → script, else worker.

New files:
- `.swarm/bus.jsonl` — every message + delivery receipts (append-only).
- `.swarm/board/NOTES.md`, `DECISIONS.md`, `<topic>.md` — whiteboard sections.
- `tasks.json`, `TASKS.md`, `events.jsonl`, `usage.json` — **unchanged format**; hiver becomes their writer.

## 6. UI

### 6.1 Sidebar tree (replaces Spaces + Agents panels when ≥1 swarm exists)

```
 SWARMS                          ⌄
 ▾ ◆ app-ideas     ●3 ⚠1  74% ▮▮▮▯   ← swarm row: rollup, counts, budget
     ◆ coordinator  idle   opus      ← master always first, gold
     ● scout        working sonnet
     ⚠ analyst      blocked sonnet   ← attention sorts up
     ● marketer     done    sonnet
     ✎ critic       working opus     ← purple
     ▷ relay        script           ← dim
 ▸ ◆ tonight-up    ●4     31%
     ◆ coordinator  working          ← collapsed: master still shown
     ⚠ backend      blocked          ← …plus anyone needing attention
 ▸ ◆ techcrunch    ✓ done
 ─ SOLO ─────────────────────────
   ○ agents · master
   ○ satellites
```

Rules
- Collapsed swarm = swarm row + master row + agents that are `blocked` or `done`-unseen.
- Expanded order: master, then by attention (blocked > done-unseen > working > idle), then critic, scripts last.
- Swarm rollup icon = worst child state (same logic as herdr's space rollup in `workspace/aggregate.rs`).
- Click swarm row → focus workspace + master pane. Click agent → focus its pane. `→`/`←` expand/collapse.
- Row tokens are configurable like herdr's `[ui.sidebar.*]` rows (reuse `tokens.rs`), under `[ui.sidebar.swarm]` and `[ui.sidebar.swarm_agent]`.

### 6.2 Pane borders

`◆ coordinator · app-ideas · opus` in the role color; focused pane keeps bold/accent title. Scripts get dim borders.

```toml
[swarm.roles.master]  glyph = "◆"  color = "#f9e2af"
[swarm.roles.worker]  glyph = "●"  color = "#89b4fa"
[swarm.roles.critic]  glyph = "✎"  color = "#cba6f7"
[swarm.roles.script]  glyph = "▷"  color = "#6c7086"
[swarm.roles.human]   glyph = "☺"  color = "#a6e3a1"
```

### 6.3 Layouts on launch

- `master-left` (default): master 40% left, workers tiled right, scripts in a narrow bottom strip.
- `master-tab`: tab 1 = master full-size + whiteboard; tab 2 = the team grid. For swarms > 6 agents.

### 6.4 Keys (defaults, prefix = `ctrl+b`)

| Key | Action |
|---|---|
| `prefix m` | jump to master of current swarm |
| `prefix 1..9` | jump to master of swarm N |
| `prefix M` | swarm picker popup (fuzzy: swarm / agent) |
| `prefix w` | toggle whiteboard panel |
| `prefix !` | next agent needing attention (any swarm) |
| `prefix s` | send message popup (to: agent / @all / @role) |

### 6.5 Whiteboard panel

A native view (not a terminal pane) opened as a split or its own tab:

```
┌ BOARD · app-ideas ─────────────────────────────────────────────────────┐
│ OPEN        │ IN-PROGRESS      │ REVIEW          │ APPROVED    │ BLOCKED │
│ T10 top-10  │ T9 competitors   │ T8 rubric v1.5  │ T1 rubric   │ T11 ⚠   │
│   ↳ T9,T11  │   scout · 12m    │   → critic      │ T2 longlist │ demand  │
├ NOTES ─────────────────────────────┬ BUS (last 20) ─────────────────────┤
│ ## Decisions                       │ 09:37 strategist → critic, coord   │
│ - rubric weights frozen at v1.5    │   RUBRIC v1.5 applied …            │
│ ## Open questions                  │ 09:31 coord → @all (fyi)           │
│ - include B2B ideas? (user)        │   Slack posts carry emoji now      │
└────────────────────────────────────┴────────────────────────────────────┘
```

Enter on a task → its notes + check output; `c` runs the check; the critic/master can approve from here.

### 6.6 Timeline

Terminal port of the Gantt in `swarm_dashboard.py` (per-agent state bars from `events.jsonl`, budget line). The HTML dashboard stays available.

## 7. Message bus

### CLI

```bash
hiver msg send scout "rerun T9 with EU data"            # direct
hiver msg send @all --fyi "Slack posts now carry emoji"  # broadcast, non-waking
hiver msg send @role:worker "freeze scope at 10 ideas"   # role-addressed
hiver msg send critic --urgent "stop: wrong rubric file" # interrupt now
hiver msg send coordinator --reply-to m_0142 "done, see shared/T9.md"
hiver msg send tonight-up/coordinator "need your backend API contract"   # master → master
hiver msg send @masters --fyi "laptop restarts at 18:00"                # all masters
hiver msg inbox [--agent scout] [--unread]               # pull
hiver msg log [--swarm app-ideas] [--follow]
```

Sender defaults to the calling pane's agent (from `HERDR_PANE_ID`); the user sending from the popup is `human`.

### Delivery policy (the core improvement)

| Target state | normal | `--fyi` | `--urgent` |
|---|---|---|---|
| idle / done | inject now | append to inbox, **no wake** | inject now |
| working | queue; inject on next idle | inbox | inject now (Claude queues it) |
| blocked | queue + tell master | inbox | queue + tell master |
| gone | queue + tell master | inbox | queue + tell master |

- **Batching**: queued messages are delivered as one digest prompt on idle (`[hiver · 3 messages] …`) → one turn, not three.
- FYIs pending in an inbox are prepended to that agent's next delivered prompt.
- Injection reuses herdr's existing `agent prompt` path (send text + submit).
- Every send/deliver/read is written to `bus.jsonl` and `events.jsonl`; pending deliveries survive a server restart.
- **Cross-swarm**: `<slug>/<agent>` addresses another swarm. Only masters (and `human`) may send cross-swarm; workers ask their own master. `@masters` reaches every master. Cross-swarm messages are logged in both swarms' `bus.jsonl`.
- Size cap per injected prompt (e.g. 4 KB); longer bodies are written to `.swarm/board/msg-<id>.md` and the prompt links to it.

### Slack bridge (optional)

Decided: **the bus is the only transport; Slack is a mirror.** A hiver plugin (herdr's plugin system is kept) copies bus traffic → `#swarm-<slug>` (agent emoji per sender) and Slack `@agent` / `@all` / unaddressed messages → bus sends from `human` (unaddressed → master). Mirror scope per swarm: `all` | `masters` (only messages to/from masters and human, default) | `off`. If Slack is down the swarm is unaffected. Replaces `swarm_relay.py`'s polling.

## 8. Board (tasks + notes)

- `hiver task add|claim|set|list|check` — same rules as `swarm_tasks.py` (claim only own/unowned with deps approved; `approved` only by critic/master and only if the check passes; checks run outside the lock, now in a server worker thread with a timeout).
- `hiver board read [section]`, `hiver board write <section> --append|--replace` — notes.
- Server is the single writer → no `tasks.lock` races. Still renders `TASKS.md` for agents that `cat` it.
- Events: `task.updated`, `board.updated` → UI refresh + available to subscribers/plugins.

## 9. Supervisor

Runs inside the server, driven by herdr's existing agent state transitions (no polling of `agent get`):

| Condition | Default | Action |
|---|---|---|
| worker `blocked` | immediately, once per episode | bus → master (`--urgent`) with screen tail |
| worker idle/done | ≥ 60 s | bus → master (normal) with output tail |
| working, no activity (no bus msg, no task change) | ≥ 45 min | bus → master |
| pane gone | immediately | bus → master; offer relaunch (`hiver swarm relaunch <agent>`) |
| budget | 80 % / 100 % | bus → master + swarm row turns yellow / red |

Token usage per agent (port of `swarm_usage.py`) feeds a `$tokens` token on agent rows.

## 10. API surface (socket, JSON-RPC style like herdr)

Methods: `swarm.create`, `swarm.import` (adopt an existing `.swarm/`), `swarm.list`, `swarm.get`, `swarm.launch`, `swarm.relaunch_agent`, `swarm.close`, `msg.send`, `msg.inbox`, `msg.log`, `task.add|claim|set|list|check`, `board.read|write`.

Events: `swarm.created|updated|closed`, `swarm.alert`, `msg.sent|delivered`, `task.updated`, `board.updated`.

`hiver swarm launch <root>`: reads `agents.json`, creates the workspace, **creates `<root>/<agent>/` for any missing agent** (errors if its `CLAUDE.md` is missing — the skill writes those), starts each agent with its model/args/env (`AGENTS_TTS=0` for workers), accepts the folder-trust prompt, sends kickoff, sets roles/titles, starts script entries. Replaces `launch_swarm.py`.

## 11. What the `/swarm` skill becomes

Keeps: team design, plan confirmation, model choice, CLAUDE.md writing, skills copy (`setup_agent.py`), shared rules, Obsidian vault, Slack channel creation.

delegates to hiver when `HIVER_ENV=1`: launch (`hiver swarm launch`), messaging (`hiver msg`), tasks (`hiver task`), watcher (built in), dashboard (built in). Falls back to the current scripts under plain herdr.

## 12. Phases & acceptance checks

| Phase | Scope | Done when |
|---|---|---|
| **P0** Fork | private fork, rename binary/config/socket, NOTICE, CI with upstream-rebase job | `cargo build --release` ok; `just test` green; `hiver` and `herdr` run side by side |
| **P1** Swarm model + UI | `SwarmMembership`, `swarm.import/list/get`, tree sidebar, role colors on rows + borders, `prefix m` | `hiver swarm import ~/swarms/apps_ideas` shows the tree; master reachable in ≤ 2 keys from anywhere; borders show names |
| **P2** Launch | `hiver swarm launch`, layouts, script entries | a swarm launched by the skill via hiver gets its own space, master in pane 1 |
| **P3** Bus | `msg.*`, delivery policy, digest, persistence, Slack bridge plugin | tests: working agent → queued then delivered on idle; `--fyi` never wakes; `@all` + `@role:`; queue survives restart |
| **P4** Board | `task.*`, `board.*`, whiteboard panel | `swarm_tasks.py` test cases pass against `hiver task`; kanban updates live |
| **P5** Supervisor | alerts, budget, tokens, timeline | `swarm_relay.py` no longer needed; alerts land on the master |
| **P6** Skill | `/swarm` uses hiver when `HIVER_ENV=1` | a new swarm runs end-to-end with no relay pane |

## 13. Risks

- **Upstream drift** — mitigated by the hook budget + weekly rebase CI; the riskiest hook is the client/server sidebar protocol (`endpoint_sidebar.rs`), which changed in 0.9.0.
- **Prompt injection timing** — injecting into a Claude pane mid-dialog can answer a dialog by accident. Only inject when state is `idle`/`done`; never when `blocked`.
- **Message storms** — agents replying to replies. Cap per-agent sends/min, and the digest keeps turns bounded.
- **Name**: "Hiver" is also a SaaS company (hiverhq.com, shared inbox); fine private, check before any public/Homebrew release.
- **Scope creep** — the swarm skill's LLM parts must not move into Rust.

## 14. Open questions

1. ~~Slack~~ — decided: bridge/mirror only.
2. ~~Binary name~~ — decided: `hiver` (no brew formula, crate name free).
3. ~~Master-to-master~~ — decided: yes, `<slug>/<agent>` + `@masters`, masters/human only.
4. Retire `.swarm/tasks.lock` compatibility once all swarms run under hiverr?
