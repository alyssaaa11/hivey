//! The swarm engine: one background thread inside the hivey server.
//!
//! It talks to the app exactly like an API client (`agent.list`, `agent.prompt`,
//! `pane.report_metadata` over the internal request channel), so herdr's state machine
//! needs no swarm-specific changes. Socket requests (`method: "swarm"`) are answered by
//! `handle_request` on the connection thread, sharing state behind one mutex.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::bus::{self, Decision, Kind, Message, Presence, Record, Sender};
use super::claude_peers;
use super::model::{Comms, Role, Swarm, SwarmAgent};
use crate::api::schema::{
    AgentInfo, AgentPromptParams, AgentStatus, EmptyParams, Method, PaneReportAgentSessionParams,
    PaneReportMetadataParams, Request, WorkspaceReportMetadataParams,
};
use crate::api::ApiRequestSender;

const TICK: Duration = Duration::from_secs(1);
const RELOAD_EVERY: Duration = Duration::from_secs(5);
const RATE_WINDOW: Duration = Duration::from_secs(60);
const HOUR: Duration = Duration::from_secs(3600);
// Per-swarm message limits (rate, agent↔agent ping-pong, hourly wake-ups, broadcasts,
// duplicates) come from the manifest's `"comms"` (`super::model::Comms`, defaults there).
const METADATA_SOURCE: &str = "hivey:swarm";
/// Pane glyph of a solo agent (`"solo": true`), which is its own master.
const SOLO_GLYPH: &str = "★";
/// Pane glyph of the hivey agent (`"home": true`).
const HOME_GLYPH: &str = "⬢";
/// How often due schedules are looked for.
const SCHEDULE_CHECK: Duration = Duration::from_secs(30);
/// Default monitoring task; a setup provider can pass its own (`--heartbeat-task`).
pub(crate) const HEARTBEAT_TASK: &str = "Monitoring pass. Check `hivey swarm list` (who is idle, \
blocked or gone) and `hivey msg log --limit 20`. Answer questions, unblock or nudge agents, \
review finished work and re-plan if needed. Message the user only if something needs them.";

struct Engine {
    state: Mutex<State>,
    wake: mpsc::Sender<()>,
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

#[derive(Debug, Clone)]
struct Live {
    name: Option<String>,
    pane_id: String,
    workspace_id: String,
    status: AgentStatus,
    since: Instant,
    /// `(source, agent, session id)` hivey stores for the pane's agent.
    session: Option<(String, String, String)>,
}

#[derive(Default)]
struct State {
    registry_path: PathBuf,
    swarms: Vec<Swarm>,
    /// Undelivered messages per `(swarm slug, agent key)`, in send order.
    queues: HashMap<(String, String), Vec<Message>>,
    /// Message ids whose master was already told the target is blocked/gone.
    escalated: HashSet<String>,
    /// Agents whose master was told during the current blocked/gone episode.
    escalated_agents: HashSet<(String, String)>,
    live_by_name: HashMap<String, Live>,
    live_by_pane: HashMap<String, Live>,
    /// Last metadata signature reported per pane, to report only on change.
    reported: HashMap<String, String>,
    /// Last resume command reported per pane (session id + argv).
    resume_reported: HashMap<String, Vec<String>>,
    /// Last swarm tokens reported per workspace (summary, master pane, info lines).
    workspace_reported: HashMap<String, (String, String, Vec<String>)>,
    last_schedule_check: Option<Instant>,
    /// Info lines per swarm (hover card / `hivey swarm info`), refreshed every RELOAD_EVERY.
    info_cache: HashMap<String, (Instant, Vec<String>)>,
    sent_at: HashMap<String, VecDeque<Instant>>,
    /// Wake-up messages per agent pair (sorted labels), for `comms.pair_wakes`.
    pair_wakes: HashMap<(String, String), VecDeque<Instant>>,
    /// Wake-up messages per sender, for `comms.hourly_wakes`.
    hourly_wakes: HashMap<String, VecDeque<Instant>>,
    /// `@all` / `@role:` / `@masters` sends per sender, for `comms.broadcasts_per_hour`.
    broadcasts: HashMap<String, VecDeque<Instant>>,
    /// Recent texts per (sender, recipient) as (when, text hash), for `comms.dedupe_min`.
    recent_texts: HashMap<(String, String), VecDeque<(Instant, u64)>>,
    seq: u64,
    last_reload: Option<Instant>,
    /// Keeps the hivey agent (`hivey home`) alive.
    home_watch: super::home::Watch,
    /// Reopens addons (Slack relays, dashboards) once after a server restart.
    addons_restore: super::addons::Restore,
    /// Claude Code's session registry by session id, refreshed every tick (claude_peers).
    claude_peers: HashMap<String, claude_peers::Peer>,
    /// Whether a Claude process (by pid) accepts inbox posts without approval.
    inbox_accepts: HashMap<u32, bool>,
}

pub(crate) fn start(api_tx: ApiRequestSender) {
    let (wake, wake_rx) = mpsc::channel();
    let mut state = State {
        // Per session: each `hivey --session <name>` (e.g. one per project) has its own swarms.
        registry_path: crate::session::data_dir().join("swarms.json"),
        ..State::default()
    };
    state.reload();
    state.rebuild_queues();
    if ENGINE
        .set(Engine {
            state: Mutex::new(state),
            wake,
        })
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("hivey-swarm".into())
        .spawn(move || loop {
            let _ = wake_rx.recv_timeout(TICK);
            tick(&api_tx);
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "hivey swarm engine failed to start");
    }
}

fn engine() -> Option<&'static Engine> {
    ENGINE.get()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Registry and queues
// ---------------------------------------------------------------------------

fn read_registry(path: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("roots").cloned())
        .and_then(|roots| serde_json::from_value::<Vec<PathBuf>>(roots).ok())
        .unwrap_or_default()
}

fn write_registry(path: &Path, roots: &[PathBuf]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    let text =
        serde_json::to_string_pretty(&json!({ "roots": roots })).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

impl State {
    fn reload(&mut self) {
        let mut swarms = Vec::new();
        for root in read_registry(&self.registry_path) {
            match Swarm::load(&root) {
                Ok(swarm) => swarms.push(swarm),
                Err(err) => tracing::warn!(root = %root.display(), %err, "skipping swarm"),
            }
        }
        self.swarms = swarms;
        self.last_reload = Some(Instant::now());
    }

    fn rebuild_queues(&mut self) {
        self.queues.clear();
        for swarm in &self.swarms {
            let (pending, held) = bus::pending_from_log(&bus::read_log(&swarm.bus_path()));
            self.escalated.extend(held);
            for msg in pending {
                if let Some(slug) = msg.swarm.clone() {
                    self.queues
                        .entry((slug, msg.to.clone()))
                        .or_default()
                        .push(msg);
                }
            }
        }
    }

    fn swarm(&self, slug: &str) -> Option<&Swarm> {
        self.swarms.iter().find(|swarm| swarm.slug == slug)
    }

    fn live(&self, agent: &SwarmAgent) -> Option<&Live> {
        agent
            .herdr_name
            .as_deref()
            .and_then(|name| self.live_by_name.get(name))
            .or_else(|| {
                // Unnamed agents (started by hand) are matched by their recorded pane.
                agent
                    .pane_id
                    .as_deref()
                    .and_then(|pane| self.live_by_pane.get(pane))
            })
    }

    fn presence(&self, agent: &SwarmAgent) -> Presence {
        let Some(live) = self.live(agent) else {
            return Presence::Gone;
        };
        // A Claude session that says it's busy is mid-turn, whatever the screen shows.
        if self
            .claude_peer(live)
            .is_some_and(claude_peers::Peer::is_busy)
            && matches!(live.status, AgentStatus::Idle | AgentStatus::Done)
        {
            return Presence::Busy;
        }
        match live.status {
            AgentStatus::Idle | AgentStatus::Done => Presence::Idle {
                idle_ms: live.since.elapsed().as_millis() as u64,
            },
            AgentStatus::Blocked => Presence::Blocked,
            AgentStatus::Working | AgentStatus::Unknown => Presence::Busy,
        }
    }

    /// Claude Code's registry entry for the session running in this pane.
    fn claude_peer(&self, live: &Live) -> Option<&claude_peers::Peer> {
        let (_, agent, session_id) = live.session.as_ref()?;
        if agent != "claude" {
            return None;
        }
        self.claude_peers.get(session_id)
    }

    /// The inbox socket to post `pending` to instead of typing it, when the agent is a Claude
    /// session that accepts posts and no message is from the human (those are typed so they
    /// keep the user's authority).
    fn inbox_for(&mut self, agent: &SwarmAgent, pending: &[&Message]) -> Option<PathBuf> {
        if pending.iter().any(|msg| msg.from == bus::HUMAN) {
            return None;
        }
        let peer = self
            .live(agent)
            .and_then(|live| self.claude_peer(live))?
            .clone();
        let socket = peer.messaging_socket_path.clone()?;
        let accepts = *self.inbox_accepts.entry(peer.pid).or_insert_with(|| {
            claude_peers::user_settings_accept()
                || claude_peers::command_line(peer.pid)
                    .is_some_and(|line| claude_peers::args_accept(&line))
        });
        accepts.then_some(socket)
    }

    /// The (non-master, non-script) agent whose home folder `<root>/<agent>` contains `cwd`.
    fn agent_in_dir(&self, cwd: &str) -> Option<(&Swarm, &SwarmAgent)> {
        let cwd = std::fs::canonicalize(cwd).ok()?;
        self.swarms.iter().find_map(|swarm| {
            swarm
                .agents
                .iter()
                .filter(|agent| !matches!(agent.role, Role::Master | Role::Script))
                .find(|agent| {
                    std::fs::canonicalize(swarm.root.join(&agent.key))
                        .is_ok_and(|home| cwd.starts_with(home))
                })
                .map(|agent| (swarm, agent))
        })
    }

    /// The swarm agent running in `pane_id`, if any.
    fn agent_in_pane(&self, pane_id: &str) -> Option<(&Swarm, &SwarmAgent)> {
        let live_name = self
            .live_by_pane
            .get(pane_id)
            .and_then(|live| live.name.clone());
        self.swarms.iter().find_map(|swarm| {
            swarm
                .agents
                .iter()
                .find(|agent| {
                    (live_name.is_some() && agent.herdr_name == live_name)
                        || agent.pane_id.as_deref() == Some(pane_id)
                })
                .map(|agent| (swarm, agent))
        })
    }

    /// Re-reads Claude Code's session registry when a live pane runs Claude.
    fn refresh_claude_peers(&mut self) {
        let any_claude = self.live_by_pane.values().any(|live| {
            live.session
                .as_ref()
                .is_some_and(|(_, agent, _)| agent == "claude")
        });
        self.claude_peers = if any_claude {
            claude_peers::load()
        } else {
            HashMap::new()
        };
        let pids: HashSet<u32> = self.claude_peers.values().map(|peer| peer.pid).collect();
        self.inbox_accepts.retain(|pid, _| pids.contains(pid));
    }

    fn update_live(&mut self, agents: Vec<AgentInfo>) {
        let now = Instant::now();
        let mut by_pane = HashMap::new();
        for info in agents {
            let since = self
                .live_by_pane
                .get(&info.pane_id)
                .filter(|old| old.status == info.agent_status && old.name == info.name)
                .map(|old| old.since)
                .unwrap_or(now);
            let status = if info.launch_pending {
                AgentStatus::Working
            } else {
                info.agent_status
            };
            by_pane.insert(
                info.pane_id.clone(),
                Live {
                    name: info.name.clone(),
                    session: info
                        .agent_session
                        .as_ref()
                        .map(|s| (s.source.clone(), s.agent.clone(), s.value.clone())),
                    workspace_id: info.workspace_id.clone(),
                    pane_id: info.pane_id,
                    status,
                    since,
                },
            );
        }
        self.live_by_name = by_pane
            .values()
            .filter_map(|live| live.name.clone().map(|name| (name, live.clone())))
            .collect();
        self.live_by_pane = by_pane;
    }

    fn next_id(&mut self) -> String {
        self.seq += 1;
        format!("m{}-{}", now_ms(), self.seq)
    }

    /// Logs a message and queues it for delivery (unless it's for the human).
    fn post(&mut self, msg: Message) -> Result<(), String> {
        let slug = msg.swarm.clone();
        let log_swarm = slug
            .as_deref()
            .or_else(|| msg.from.split_once('/').map(|(slug, _)| slug))
            .and_then(|slug| self.swarm(slug))
            .map(Swarm::bus_path);
        if let Some(path) = log_swarm {
            bus::append(&path, &[Record::Msg(msg.clone())]).map_err(|err| err.to_string())?;
        }
        // Cross-swarm: keep a copy in the sender's log too.
        if let Some((from_slug, _)) = msg.from.split_once('/') {
            // Messages to the human (no recipient swarm) are already in the sender's log.
            if slug.is_some() && slug.as_deref() != Some(from_slug) {
                if let Some(swarm) = self.swarm(from_slug) {
                    let copy = Message {
                        copy: true,
                        ..msg.clone()
                    };
                    bus::append(&swarm.bus_path(), &[Record::Msg(copy)])
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        if let Some(slug) = slug {
            if msg.to != bus::HUMAN {
                self.queues
                    .entry((slug, msg.to.clone()))
                    .or_default()
                    .push(msg);
            }
        }
        Ok(())
    }

    /// Counts a wake-up message between two agents; false once the pair is over its limit.
    fn pair_allows_wake(&mut self, a: &str, b: &str, comms: &Comms) -> bool {
        let key = if a <= b {
            (a.to_string(), b.to_string())
        } else {
            (b.to_string(), a.to_string())
        };
        let window = Duration::from_secs(comms.pair_window_min * 60);
        allow_in_window(
            self.pair_wakes.entry(key).or_default(),
            window,
            comms.pair_wakes,
        )
    }

    fn rate_limited(&mut self, sender: &str, comms: &Comms) -> bool {
        let times = self.sent_at.entry(sender.to_string()).or_default();
        !allow_in_window(times, RATE_WINDOW, comms.rate_per_min)
    }

    /// Counts one wake-up from `sender`; false once it's over `comms.hourly_wakes`.
    fn hourly_allows_wake(&mut self, sender: &str, comms: &Comms) -> bool {
        let times = self.hourly_wakes.entry(sender.to_string()).or_default();
        allow_in_window(times, HOUR, comms.hourly_wakes)
    }

    /// Counts one broadcast from `sender`; false once it's over `comms.broadcasts_per_hour`.
    fn broadcast_allowed(&mut self, sender: &str, comms: &Comms) -> bool {
        let times = self.broadcasts.entry(sender.to_string()).or_default();
        allow_in_window(times, HOUR, comms.broadcasts_per_hour)
    }

    /// True if `sender` sent this exact text to `recipient` within `comms.dedupe_min`;
    /// otherwise remembers it and returns false.
    fn is_duplicate(&mut self, sender: &str, recipient: &str, text: &str, comms: &Comms) -> bool {
        if comms.dedupe_min == 0 {
            return false;
        }
        let window = Duration::from_secs(comms.dedupe_min * 60);
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            text.trim().hash(&mut hasher);
            hasher.finish()
        };
        let now = Instant::now();
        let recent = self
            .recent_texts
            .entry((sender.to_string(), recipient.to_string()))
            .or_default();
        while recent
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > window)
        {
            recent.pop_front();
        }
        if recent.iter().any(|(_, seen)| *seen == hash) {
            return true;
        }
        recent.push_back((now, hash));
        false
    }

    /// The limits for messages from `sender`: its swarm's `comms`, or `None` when it is never
    /// limited (the human, the hivey agent, and script panes such as the Slack relay, which
    /// forward the human's messages).
    fn comms_for(&self, sender: &Sender) -> Option<Comms> {
        let slug = sender.swarm.as_deref()?;
        if sender.role == Some(Role::Script) {
            return None;
        }
        let swarm = self.swarm(slug)?;
        (!swarm.home).then(|| swarm.comms.clone())
    }
}

// ---------------------------------------------------------------------------
/// Sliding-window counter: drops entries older than `window`, then records one more and
/// returns true, or returns false if `limit` are already in the window. `limit` 0 = no limit.
fn allow_in_window(times: &mut VecDeque<Instant>, window: Duration, limit: usize) -> bool {
    if limit == 0 {
        return true;
    }
    let now = Instant::now();
    while times
        .front()
        .is_some_and(|at| now.duration_since(*at) > window)
    {
        times.pop_front();
    }
    if times.len() >= limit {
        return false;
    }
    times.push_back(now);
    true
}

// The loop
// ---------------------------------------------------------------------------

struct Delivery {
    slug: String,
    key: String,
    target: String,
    text: String,
    ids: Vec<String>,
    /// Claude Code inbox socket to post to instead of typing (claude_peers).
    inbox: Option<PathBuf>,
}

fn dispatch(api_tx: &ApiRequestSender, method: Method) -> Result<Value, String> {
    let request = Request {
        id: "hivey:swarm".into(),
        method,
    };
    let raw = crate::api::dispatch_internal(request, api_tx, Some(Duration::from_secs(10)));
    let value: Value = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
    if let Some(error) = value.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string());
    }
    Ok(value.get("result").cloned().unwrap_or(Value::Null))
}

fn tick(api_tx: &ApiRequestSender) {
    // Quiet mode's daily hours, swarms or not
    super::quiet::tick();
    let Some(engine) = engine() else { return };
    {
        let Ok(mut state) = engine.state.lock() else {
            return;
        };
        if state
            .last_reload
            .is_none_or(|at| at.elapsed() >= RELOAD_EVERY)
        {
            state.reload();
        }
        // No swarms registered: stay idle (but bring the hivey agent back if it's enabled).
        if state.swarms.is_empty() {
            super::home::watch(&mut state.home_watch, false);
            return;
        }
    }
    let agents = dispatch(api_tx, Method::AgentList(EmptyParams::default()))
        .ok()
        .and_then(|result| result.get("agents").cloned())
        .and_then(|agents| serde_json::from_value::<Vec<AgentInfo>>(agents).ok());

    let (deliveries, metadata, resumes, spaces) = {
        let Ok(mut state) = engine.state.lock() else {
            return;
        };
        let Some(agents) = agents else { return };
        state.update_live(agents);
        state.refresh_claude_peers();
        let home_alive = state
            .swarms
            .iter()
            .filter(|swarm| swarm.home)
            .filter_map(Swarm::master)
            .any(|master| state.live(master).is_some());
        super::home::watch(&mut state.home_watch, home_alive);
        let fields = &mut *state;
        super::addons::restore(&mut fields.addons_restore, &fields.swarms);
        if state
            .last_schedule_check
            .is_none_or(|at| at.elapsed() >= SCHEDULE_CHECK)
        {
            state.last_schedule_check = Some(Instant::now());
            run_due_schedules(&mut state);
        }
        let deliveries = plan_deliveries(&mut state);
        (
            deliveries,
            plan_metadata(&mut state),
            plan_resume(&mut state),
            plan_workspace_tokens(&mut state),
        )
    };

    for delivery in deliveries {
        if post_to_inbox(&delivery) {
            let Ok(mut state) = engine.state.lock() else {
                return;
            };
            confirm_delivery(&mut state, &delivery);
            continue;
        }
        let prompt = Method::AgentPrompt(AgentPromptParams {
            target: delivery.target.clone(),
            text: delivery.text.clone(),
            wait: None,
        });
        match dispatch(api_tx, prompt) {
            Ok(_) => {
                let Ok(mut state) = engine.state.lock() else {
                    return;
                };
                confirm_delivery(&mut state, &delivery);
            }
            Err(err) => {
                tracing::warn!(target = %delivery.target, %err, "hivey delivery failed; will retry")
            }
        }
    }
    for params in resumes {
        let pane_id = params.pane_id.clone();
        if let Err(err) = dispatch(api_tx, Method::PaneReportAgentSession(params)) {
            tracing::warn!(%pane_id, %err, "hivey: resume command not recorded; will retry");
            if let Ok(mut state) = engine.state.lock() {
                state.resume_reported.remove(&pane_id);
            }
        }
    }
    for params in spaces {
        let workspace_id = params.workspace_id.clone();
        if dispatch(api_tx, Method::WorkspaceReportMetadata(params)).is_err() {
            if let Ok(mut state) = engine.state.lock() {
                state.workspace_reported.remove(&workspace_id);
            }
        }
    }
    for (pane_id, params) in metadata {
        if dispatch(api_tx, Method::PaneReportMetadata(params)).is_err() {
            // Pane gone or not ready: forget the signature so it's retried.
            if let Ok(mut state) = engine.state.lock() {
                state.reported.remove(&pane_id);
            }
        }
    }
}

fn plan_deliveries(state: &mut State) -> Vec<Delivery> {
    let mut deliveries = Vec::new();
    let mut escalations = Vec::new();
    // An episode ends when the agent is reachable again; the next one notifies anew.
    let recovered: Vec<(String, String)> = state
        .escalated_agents
        .iter()
        .filter(|(slug, name)| {
            let presence = state
                .swarm(slug)
                .and_then(|swarm| swarm.agent(name))
                .map(|agent| state.presence(agent));
            !matches!(presence, Some(Presence::Blocked | Presence::Gone))
        })
        .cloned()
        .collect();
    for entry in recovered {
        state.escalated_agents.remove(&entry);
    }
    for ((slug, key), queue) in &state.queues {
        if queue.is_empty() {
            continue;
        }
        let Some(swarm) = state.swarm(slug) else {
            continue;
        };
        // Paused swarm: hold everything, no deliveries and no escalations.
        if swarm.paused {
            continue;
        }
        let Some(agent) = swarm.agent(key) else {
            continue;
        };
        let pending: Vec<&Message> = queue.iter().collect();
        match bus::decide(state.presence(agent), &pending) {
            Decision::Hold => {}
            Decision::Deliver => {
                // The live pane is always a valid target, renamed or not.
                let Some(target) = state
                    .live(agent)
                    .map(|live| live.pane_id.clone())
                    .or_else(|| agent.herdr_name.clone())
                else {
                    continue;
                };
                deliveries.push(Delivery {
                    slug: slug.clone(),
                    key: key.clone(),
                    target,
                    text: prompt_text(swarm, key, &pending),
                    ids: queue.iter().map(|msg| msg.id.clone()).collect(),
                    inbox: None,
                });
            }
            Decision::Escalate(reason) => {
                let fresh: Vec<&Message> = pending
                    .iter()
                    .copied()
                    .filter(|msg| !state.escalated.contains(&msg.id))
                    .collect();
                if !fresh.is_empty() {
                    escalations.push((
                        swarm.clone(),
                        agent.clone(),
                        reason,
                        fresh.into_iter().cloned().collect::<Vec<_>>(),
                    ));
                }
            }
        }
    }
    for (swarm, agent, reason, msgs) in escalations {
        escalate(state, &swarm, &agent, reason, &msgs);
    }
    for delivery in &mut deliveries {
        let agent = state
            .swarm(&delivery.slug)
            .and_then(|swarm| swarm.agent(&delivery.key))
            .cloned();
        let pending = state
            .queues
            .get(&(delivery.slug.clone(), delivery.key.clone()))
            .cloned()
            .unwrap_or_default();
        let pending: Vec<&Message> = pending.iter().collect();
        delivery.inbox = agent.and_then(|agent| state.inbox_for(&agent, &pending));
    }
    deliveries
}

/// Posts a delivery to its Claude inbox socket; false means type it instead.
fn post_to_inbox(delivery: &Delivery) -> bool {
    let Some(socket) = delivery.inbox.as_deref() else {
        return false;
    };
    match claude_peers::post(socket, bus::HIVEY, &delivery.text) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(target = %delivery.target, %err, "hivey inbox post failed; typing it");
            false
        }
    }
}

/// The digest, or a pointer to a file when it's too long to paste.
fn prompt_text(swarm: &Swarm, key: &str, pending: &[&Message]) -> String {
    let text = bus::digest(pending);
    if text.chars().count() <= bus::MAX_PROMPT_CHARS {
        return text;
    }
    let path = swarm.overflow_dir().join(format!("{key}-{}.md", now_ms()));
    let written =
        std::fs::create_dir_all(swarm.overflow_dir()).and_then(|_| std::fs::write(&path, &text));
    match written {
        Ok(()) => format!(
            "[hivey · {} messages, too long to paste] read them: cat {}",
            pending.len(),
            path.display()
        ),
        Err(_) => text.chars().take(bus::MAX_PROMPT_CHARS).collect(),
    }
}

fn confirm_delivery(state: &mut State, delivery: &Delivery) {
    let ts = now_ms();
    let records: Vec<Record> = delivery
        .ids
        .iter()
        .map(|id| Record::Delivered {
            id: id.clone(),
            to: delivery.key.clone(),
            ts,
            batch: delivery.ids.len(),
        })
        .collect();
    if let Some(swarm) = state.swarm(&delivery.slug) {
        if let Err(err) = bus::append(&swarm.bus_path(), &records) {
            tracing::warn!(%err, "hivey: cannot record delivery");
        }
    }
    if let Some(queue) = state
        .queues
        .get_mut(&(delivery.slug.clone(), delivery.key.clone()))
    {
        queue.retain(|msg| !delivery.ids.contains(&msg.id));
    }
}

/// Records the held messages and, once per blocked/gone episode, tells the master.
fn escalate(state: &mut State, swarm: &Swarm, agent: &SwarmAgent, reason: &str, msgs: &[Message]) {
    let ts = now_ms();
    let held: Vec<Record> = msgs
        .iter()
        .map(|msg| Record::Held {
            id: msg.id.clone(),
            to: agent.key.clone(),
            ts,
            reason: reason.into(),
        })
        .collect();
    let _ = bus::append(&swarm.bus_path(), &held);
    state
        .escalated
        .extend(msgs.iter().map(|msg| msg.id.clone()));
    if !state
        .escalated_agents
        .insert((swarm.slug.clone(), agent.key.clone()))
    {
        return;
    }

    let Some(master) = swarm.master().filter(|master| master.key != agent.key) else {
        return;
    };
    let senders: Vec<String> = msgs
        .iter()
        .map(|msg| msg.from.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let check = agent
        .herdr_name
        .as_deref()
        .map(|name| format!(" Check it: hivey agent read {name} --source visible"))
        .unwrap_or_default();
    let text = format!(
        "{} is {reason}; {} message(s) from {} are waiting for it.{check}",
        agent.key,
        msgs.len(),
        senders.join(", ")
    );
    let notice = Message {
        id: state.next_id(),
        ts,
        from: bus::HIVEY.into(),
        swarm: Some(swarm.slug.clone()),
        to: master.key.clone(),
        addressed: master.key.clone(),
        kind: Kind::Normal,
        text,
        reply_to: None,
        copy: false,
    };
    if let Err(err) = state.post(notice) {
        tracing::warn!(%err, "hivey: cannot notify master");
    }
}

/// hivey resumes agents after a restart with only `claude --resume <id>` / `codex resume
/// <id>`, dropping the swarm's flags (permission mode, model, add-dirs). Report the full
/// command instead (see `adapter`), again whenever an agent's session changes.
fn plan_resume(state: &mut State) -> Vec<PaneReportAgentSessionParams> {
    let mut out = Vec::new();
    for swarm in &state.swarms {
        for agent in swarm.agents.iter().filter(|a| !a.args.is_empty()) {
            let Some(live) = state.live(agent) else {
                continue;
            };
            let Some((source, agent_label, session_id)) = live.session.clone() else {
                continue;
            };
            if source != agent.kind.herdr_source() {
                continue;
            }
            let Some(argv) = agent.kind.resume_argv(&session_id, &agent.args) else {
                continue;
            };
            if state.resume_reported.get(&live.pane_id) == Some(&argv) {
                continue;
            }
            out.push(PaneReportAgentSessionParams {
                pane_id: live.pane_id.clone(),
                source,
                agent: agent_label,
                // Same clock as the Claude hook's own reports, so ours counts as newest.
                seq: Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_nanos() as u64)
                        .unwrap_or_default(),
                ),
                agent_session_id: Some(session_id),
                agent_session_path: None,
                session_start_source: None,
                resume_argv: Some(argv),
            });
        }
    }
    for params in &out {
        if let Some(argv) = &params.resume_argv {
            state
                .resume_reported
                .insert(params.pane_id.clone(), argv.clone());
        }
    }
    out
}

/// Lines that describe a swarm (`hivey swarm info`, the sidebar hover card): master, agents,
/// the setup provider's own facts (manifest `info`, e.g. Slack channel, memory vault), addons,
/// budget, tasks, schedules, GitHub, folder. hivey knows no provider's files: a provider shows
/// anything extra by writing `"info": {"Slack": "#swarm-x (C0…)", "vault": "…"}` (or a list
/// of `[label, value]` pairs) into `.swarm/agents.json`.
fn swarm_info(state: &State, swarm: &Swarm) -> Vec<String> {
    let manifest: Value = std::fs::read_to_string(super::model::manifest_path(&swarm.root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let status = |agent: &SwarmAgent| match state.live(agent) {
        Some(live) => status_str(live.status),
        None if agent.role == Role::Script => "script",
        None => "gone",
    };
    let mut lines = Vec::new();
    if let Some(master) = swarm.master() {
        if swarm.solo {
            let what = match &master.model {
                Some(model) => format!("{}/{model}", master.kind.as_str()),
                None => master.kind.as_str().to_string(),
            };
            lines.push(format!(
                "{SOLO_GLYPH} agent     {what} · {}",
                status(master)
            ));
        } else {
            lines.push(format!("◆ master    {} · {}", master.key, status(master)));
        }
    }
    if let Some(about) = swarm.profile["description"]
        .as_str()
        .filter(|d| !d.is_empty())
    {
        lines.push(format!("about       {about}"));
    }
    let agents: Vec<String> = swarm
        .agents
        .iter()
        .filter(|a| !matches!(a.role, Role::Master | Role::Script))
        .map(|a| {
            let what = match &a.model {
                Some(model) => format!("{}/{model}", a.kind.as_str()),
                None => a.kind.as_str().to_string(),
            };
            format!("{} {} {what} {}", a.role.glyph(), a.key, status(a))
        })
        .collect();
    if !agents.is_empty() {
        lines.push(format!("agents      {}", agents.join(", ")));
    }
    let extra = provider_info(&manifest);
    // The relay channel is part of hivey's manifest; show it unless the provider already does.
    if let Some(channel) = manifest["channel_id"].as_str().filter(|c| !c.is_empty()) {
        if !extra.iter().any(|(_, value)| value.contains(channel)) {
            lines.push(format!("channel     {channel}"));
        }
    }
    for (label, value) in extra {
        lines.push(format!(
            "{:<11} {value}",
            label.chars().take(11).collect::<String>()
        ));
    }
    let addons: Vec<String> = manifest["addons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|addon| addon["plugin"].as_str())
        .map(|plugin| plugin.rsplit('.').next().unwrap_or(plugin).to_string())
        .collect();
    if !addons.is_empty() {
        lines.push(format!("addons      {}", addons.join(", ")));
    }
    let elapsed = manifest["launched_at"].as_f64().map(|start| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(start);
        ((now - start) / 60.0).max(0.0) as u64
    });
    match (elapsed, manifest["budget_minutes"].as_u64()) {
        (Some(used), Some(budget)) => lines.push(format!("budget      {used}m of {budget}m")),
        (Some(used), None) => lines.push(format!("running     {used}m")),
        _ => {}
    }
    if swarm.paused {
        lines.push("state       ⏸ paused (hivey swarm resume)".to_string());
    }
    let tasks: Vec<Value> = std::fs::read_to_string(swarm.root.join(".swarm").join("tasks.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|board| board["tasks"].as_array().cloned())
        .unwrap_or_default();
    if !tasks.is_empty() {
        let count = |status: &str| tasks.iter().filter(|t| t["status"] == status).count();
        lines.push(format!(
            "tasks       {}/{} approved · {} in progress · {} blocked",
            count("approved"),
            tasks.len(),
            count("in-progress"),
            count("blocked")
        ));
    }
    let github: Vec<&str> = ["product", "swarm"]
        .iter()
        .filter_map(|key| manifest["github"][key].as_str())
        .collect();
    if !github.is_empty() {
        lines.push(format!("GitHub      {}", github.join(" · ")));
    }
    let schedules: Vec<String> = super::schedule::load(&swarm.root)
        .schedules
        .iter()
        .map(|s| format!("{} → {}", s.describe(), s.to))
        .collect();
    if !schedules.is_empty() {
        lines.push(format!("schedules   {}", schedules.join(", ")));
    }
    lines.push(format!("folder      {}", swarm.root.display()));
    lines
}

/// The setup provider's facts: manifest `info` as an object (in key order) or `[label, value]`
/// pairs.
fn provider_info(manifest: &Value) -> Vec<(String, String)> {
    let text = |value: &Value| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    match &manifest["info"] {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), text(v))).collect(),
        Value::Array(pairs) => pairs
            .iter()
            .filter_map(|pair| Some((pair.get(0)?.as_str()?.to_string(), text(pair.get(1)?))))
            .collect(),
        _ => Vec::new(),
    }
}

/// Info lines cut to herdr's 80-character token limit (long lists wrap onto more lines).
fn info_token_lines(lines: &[String]) -> Vec<String> {
    const WIDTH: usize = 78;
    let mut out = Vec::new();
    for line in lines {
        let mut rest = line.as_str();
        let mut first = true;
        while !rest.is_empty() {
            let indent = if first { "" } else { "            " };
            let room = WIDTH - indent.chars().count();
            let cut = if rest.chars().count() <= room {
                rest.len()
            } else {
                let limit = rest.char_indices().nth(room).map_or(rest.len(), |(i, _)| i);
                rest[..limit].rfind(", ").map_or(limit, |i| i + 2)
            };
            out.push(format!("{indent}{}", rest[..cut].trim_end()));
            rest = &rest[cut..];
            first = false;
        }
    }
    out.truncate(MAX_INFO_LINES);
    out
}

const MAX_INFO_LINES: usize = 12;

/// Queues every due schedule of every running swarm as a bus message to its target.
fn run_due_schedules(state: &mut State) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let local = super::schedule::local_now();
    let swarms: Vec<Swarm> = state.swarms.iter().filter(|s| !s.paused).cloned().collect();
    for swarm in swarms {
        let mut book = super::schedule::load(&swarm.root);
        let mut changed = false;
        for index in 0..book.schedules.len() {
            if !book.schedules[index].is_due(now, &local) {
                continue;
            }
            match fire_schedule(state, &swarm, &book.schedules[index]) {
                Ok(true) => {
                    book.schedules[index].mark_run(now, &local);
                    changed = true;
                }
                Ok(false) => {} // the previous wake-up is still waiting: no pile-up
                Err(err) => tracing::warn!(%err, swarm = %swarm.slug, "hivey: schedule failed"),
            }
        }
        if changed {
            if let Err(err) = super::schedule::save(&swarm.root, &book) {
                tracing::warn!(%err, "hivey: cannot save schedules");
            }
        }
    }
}

/// Sends one schedule's task with a status snapshot. `Ok(false)` when its previous wake-up
/// hasn't been delivered yet (nothing is sent).
fn fire_schedule(
    state: &mut State,
    swarm: &Swarm,
    schedule: &super::schedule::Schedule,
) -> Result<bool, String> {
    let target = swarm
        .agent(&schedule.to)
        .ok_or_else(|| format!("swarm {:?} has no agent {:?}", swarm.slug, schedule.to))?
        .key
        .clone();
    let tag = format!("schedule:{}", schedule.id);
    let waiting = state
        .queues
        .get(&(swarm.slug.clone(), target.clone()))
        .is_some_and(|queue| queue.iter().any(|msg| msg.addressed == tag));
    if waiting {
        return Ok(false);
    }
    let text = format!(
        "Scheduled check `{}` ({}): {}\n{}",
        schedule.id,
        schedule.describe(),
        schedule.task,
        status_snapshot(state, swarm, schedule.last_run.unwrap_or(schedule.created))
    );
    let msg = Message {
        id: state.next_id(),
        ts: now_ms(),
        from: bus::HIVEY.into(),
        swarm: Some(swarm.slug.clone()),
        to: target,
        addressed: tag,
        kind: Kind::Normal,
        text,
        reply_to: None,
        copy: false,
    };
    state.post(msg)?;
    Ok(true)
}

/// What changed since `since` (unix seconds): summary, agents needing attention, messages.
fn status_snapshot(state: &State, swarm: &Swarm, since: u64) -> String {
    let attention: Vec<String> = swarm
        .agents
        .iter()
        .filter(|a| a.role != Role::Script)
        .filter_map(|a| match state.live(a).map(|live| live.status) {
            Some(AgentStatus::Blocked) => Some(format!("{} blocked", a.key)),
            Some(AgentStatus::Done) => Some(format!("{} finished", a.key)),
            None if a.role != Role::Master => Some(format!("{} gone", a.key)),
            _ => None,
        })
        .collect();
    let since_ms = since.saturating_mul(1000);
    let messages = bus::read_log(&swarm.bus_path())
        .iter()
        .filter(
            |r| matches!(r, Record::Msg(m) if !m.copy && m.ts > since_ms && m.from != bus::HIVEY),
        )
        .count();
    let mut status = format!("Status: {}", swarm_summary(state, swarm));
    if !attention.is_empty() {
        status.push_str(&format!(" · needs you: {}", attention.join(", ")));
    }
    status.push_str(&format!(
        " · {messages} message(s) since the last check (hivey msg log --limit 20)."
    ));
    status
}

/// One line per swarm for its space row (`●2/3 ⚠1 ✉2 ⏸`).
fn swarm_summary(state: &State, swarm: &Swarm) -> String {
    let members: Vec<&SwarmAgent> = swarm
        .agents
        .iter()
        .filter(|a| a.role != Role::Script)
        .collect();
    let status = |agent: &SwarmAgent| state.live(agent).map(|live| live.status);
    let working = members
        .iter()
        .filter(|a| status(a) == Some(AgentStatus::Working))
        .count();
    let attention = members
        .iter()
        .filter(|a| matches!(status(a), Some(AgentStatus::Blocked | AgentStatus::Done)))
        .count();
    let queued: usize = state
        .queues
        .iter()
        .filter(|((slug, _), _)| *slug == swarm.slug)
        .map(|(_, queue)| queue.len())
        .sum();
    let mut summary = format!("●{working}/{}", members.len());
    if attention > 0 {
        summary.push_str(&format!(" ⚠{attention}"));
    }
    if queued > 0 {
        summary.push_str(&format!(" ✉{queued}"));
    }
    if swarm.paused {
        summary.push_str(" ⏸");
    }
    summary
}

/// Tokens on each swarm's space: `swarm` (the summary shown on its sidebar row) and
/// `master_pane` (clicking the space focuses the master).
fn plan_workspace_tokens(state: &mut State) -> Vec<WorkspaceReportMetadataParams> {
    let mut out = Vec::new();
    for swarm in &state.swarms {
        let master = swarm.master().and_then(|m| state.live(m));
        let workspace = master.map(|live| live.workspace_id.clone()).or_else(|| {
            swarm
                .agents
                .iter()
                .filter(|a| a.role != Role::Script)
                .find_map(|a| state.live(a).map(|live| live.workspace_id.clone()))
        });
        let Some(workspace_id) = workspace else {
            continue;
        };
        let summary = swarm_summary(state, swarm);
        let master_pane = master.map(|live| live.pane_id.clone()).unwrap_or_default();
        let info = match state.info_cache.get(&swarm.slug) {
            Some((at, lines)) if at.elapsed() < RELOAD_EVERY => lines.clone(),
            _ => info_token_lines(&swarm_info(state, swarm)),
        };
        let signature = (summary.clone(), master_pane.clone(), info.clone());
        if state.workspace_reported.get(&workspace_id) == Some(&signature) {
            continue;
        }
        let mut tokens = HashMap::new();
        tokens.insert("swarm".to_string(), Some(summary));
        tokens.insert(
            "master_pane".to_string(),
            (!master_pane.is_empty()).then_some(master_pane),
        );
        // info1..info12 (unused ones cleared) for the sidebar hover card.
        for index in 0..MAX_INFO_LINES {
            tokens.insert(format!("info{}", index + 1), info.get(index).cloned());
        }
        out.push(WorkspaceReportMetadataParams {
            workspace_id,
            source: METADATA_SOURCE.into(),
            tokens,
            seq: None,
            ttl_ms: None,
        });
    }
    for params in &out {
        let token = |key: &str| params.tokens.get(key).cloned().flatten();
        let info: Vec<String> = (1..=MAX_INFO_LINES)
            .filter_map(|i| token(&format!("info{i}")))
            .collect();
        state.workspace_reported.insert(
            params.workspace_id.clone(),
            (
                token("swarm").unwrap_or_default(),
                token("master_pane").unwrap_or_default(),
                info,
            ),
        );
    }
    // Cache the info lines per swarm so files are read at most every RELOAD_EVERY.
    let now = Instant::now();
    let slugs: Vec<String> = state.swarms.iter().map(|s| s.slug.clone()).collect();
    for slug in slugs {
        if state
            .info_cache
            .get(&slug)
            .is_none_or(|(at, _)| at.elapsed() >= RELOAD_EVERY)
        {
            if let Some(swarm) = state.swarm(&slug).cloned() {
                let lines = info_token_lines(&swarm_info(state, &swarm));
                state.info_cache.insert(slug, (now, lines));
            }
        }
    }
    out
}

fn plan_metadata(state: &mut State) -> Vec<(String, PaneReportMetadataParams)> {
    let mut out = Vec::new();
    for swarm in &state.swarms {
        for agent in &swarm.agents {
            let pane_id = match state.live(agent) {
                Some(live) => live.pane_id.clone(),
                None if agent.role == Role::Script => match &agent.pane_id {
                    Some(pane) => pane.clone(),
                    None => continue,
                },
                None => continue,
            };
            let queued = state
                .queues
                .get(&(swarm.slug.clone(), agent.key.clone()))
                .map_or(0, Vec::len);
            let solo_master = swarm.solo && agent.role == Role::Master;
            let glyph = if swarm.home { HOME_GLYPH } else { SOLO_GLYPH };
            let mut title = if solo_master {
                let what = if swarm.home { "hivey agent" } else { "agent" };
                format!("{glyph} {} · {what}", swarm.slug)
            } else {
                format!("{} {} · {}", agent.role.glyph(), agent.key, swarm.slug)
            };
            if let Some(model) = &agent.model {
                title.push_str(&format!(" · {model}"));
            }
            if queued > 0 {
                title.push_str(&format!(" · ✉{queued}"));
            }
            if swarm.paused {
                title.push_str(" · ⏸ paused");
            }
            if state.reported.get(&pane_id) == Some(&title) {
                continue;
            }
            let mut tokens = HashMap::new();
            tokens.insert("role".to_string(), Some(agent.role.as_str().to_string()));
            tokens.insert("swarm".to_string(), Some(swarm.slug.clone()));
            tokens.insert("paused".to_string(), swarm.paused.then(|| "1".to_string()));
            tokens.insert("solo".to_string(), swarm.solo.then(|| "1".to_string()));
            tokens.insert("home".to_string(), swarm.home.then(|| "1".to_string()));
            tokens.insert(
                "queued".to_string(),
                (queued > 0).then(|| queued.to_string()),
            );
            out.push((
                pane_id.clone(),
                PaneReportMetadataParams {
                    pane_id: pane_id.clone(),
                    source: METADATA_SOURCE.into(),
                    agent: None,
                    applies_to_source: None,
                    title: Some(title.clone()),
                    display_agent: Some(if solo_master {
                        format!("{glyph} {}", swarm.slug)
                    } else {
                        format!("{} {}", agent.role.glyph(), agent.key)
                    }),
                    state_labels: HashMap::new(),
                    tokens,
                    clear_title: false,
                    clear_display_agent: false,
                    clear_state_labels: false,
                    seq: None,
                    ttl_ms: None,
                },
            ));
        }
    }
    for (pane_id, params) in &out {
        if let Some(title) = &params.title {
            state.reported.insert(pane_id.clone(), title.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Socket requests
// ---------------------------------------------------------------------------

pub(crate) fn handle_request(id: &str, params: &super::SwarmParams) -> String {
    let result = match engine() {
        None => Err("the swarm engine is not running in this server".to_string()),
        Some(engine) => {
            let outcome = match engine.state.lock() {
                Ok(mut state) => run_op(&mut state, &params.op, &params.args),
                Err(_) => Err("swarm engine state is poisoned".into()),
            };
            let _ = engine.wake.send(());
            outcome
        }
    };
    let value = match result {
        Ok(mut result) => {
            if let Some(object) = result.as_object_mut() {
                object.insert("type".into(), json!("swarm"));
            }
            json!({ "id": id, "result": result })
        }
        Err(message) => json!({ "id": id, "error": { "code": "swarm_error", "message": message } }),
    };
    value.to_string()
}

fn arg<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn run_op(state: &mut State, op: &str, args: &Value) -> Result<Value, String> {
    match op {
        "import" => op_import(state, args),
        "forget" => op_forget(state, args),
        "info" => op_info(state, args),
        "schedule.add" => op_schedule_add(state, args),
        "schedule.list" => op_schedule_list(state, args),
        "schedule.remove" => op_schedule_remove(state, args),
        "schedule.run" => op_schedule_run(state, args),
        "pause" => op_set_paused(state, args, true),
        "resume" => op_set_paused(state, args, false),
        "list" => Ok(op_list(state)),
        "master" => op_master(state, args),
        "msg.send" => op_send(state, args),
        "msg.inbox" => op_inbox(state, args),
        "msg.log" => op_log(state, args),
        other => Err(format!("unknown swarm op {other:?}")),
    }
}

fn op_import(state: &mut State, args: &Value) -> Result<Value, String> {
    let root = arg(args, "root").ok_or("missing root")?;
    let root = std::fs::canonicalize(root).map_err(|err| format!("{root}: {err}"))?;
    let swarm = Swarm::load(&root)?;
    if let Some(other) = state
        .swarms
        .iter()
        .find(|s| s.slug == swarm.slug && s.root != root)
    {
        return Err(format!(
            "swarm {:?} is already registered at {}",
            swarm.slug,
            other.root.display()
        ));
    }
    let mut roots = read_registry(&state.registry_path);
    if !roots.contains(&root) {
        roots.push(root.clone());
        write_registry(&state.registry_path, &roots)?;
    }
    state.reload();
    state.rebuild_queues();
    Ok(json!({ "swarm": swarm_json(state, state.swarm(&swarm.slug).ok_or("import failed")?) }))
}

fn schedule_swarm(state: &State, args: &Value) -> Result<Swarm, String> {
    let slug = match arg(args, "swarm") {
        Some(slug) => slug.to_string(),
        None => context_swarm(state, args).ok_or("which swarm? pass a swarm slug")?,
    };
    state
        .swarm(&slug)
        .cloned()
        .ok_or(format!("no swarm {slug:?}"))
}

fn op_schedule_add(state: &mut State, args: &Value) -> Result<Value, String> {
    use super::schedule::{load, parse_at, parse_every, save, Schedule};
    let swarm = schedule_swarm(state, args)?;
    let task = arg(args, "task").ok_or("missing task")?.to_string();
    let to = arg(args, "to").unwrap_or("coordinator").to_string();
    if swarm.agent(&to).is_none() {
        return Err(format!("swarm {:?} has no agent {to:?}", swarm.slug));
    }
    let every_secs = arg(args, "every").map(parse_every).transpose()?;
    let at = match arg(args, "at") {
        Some(at) => {
            parse_at(at)?;
            Some(at.trim().to_string())
        }
        None => None,
    };
    if every_secs.is_some() == at.is_some() {
        return Err("give exactly one of --every <interval> or --at <HH:MM>".into());
    }
    let mut book = load(&swarm.root);
    let id = match arg(args, "id") {
        Some(id) => id.to_string(),
        None => (1..)
            .map(|n| format!("s{n}"))
            .find(|id| !book.schedules.iter().any(|s| &s.id == id))
            .unwrap_or_default(),
    };
    book.schedules.retain(|s| s.id != id); // re-adding an id replaces it
    let schedule = Schedule {
        id,
        every_secs,
        at,
        to,
        task,
        last_run: None,
        last_day: None,
        created: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
    };
    let added = json!({ "id": schedule.id, "when": schedule.describe(), "to": schedule.to });
    book.schedules.push(schedule);
    save(&swarm.root, &book)?;
    Ok(json!({ "swarm": swarm.slug, "added": added }))
}

fn op_schedule_list(state: &State, args: &Value) -> Result<Value, String> {
    let swarms: Vec<Swarm> = match arg(args, "swarm")
        .map(str::to_string)
        .or_else(|| context_swarm(state, args))
    {
        Some(slug) => vec![state
            .swarm(&slug)
            .cloned()
            .ok_or(format!("no swarm {slug:?}"))?],
        None => state.swarms.clone(),
    };
    let rows: Vec<Value> = swarms
        .iter()
        .flat_map(|swarm| {
            super::schedule::load(&swarm.root)
                .schedules
                .into_iter()
                .map(move |s| {
                    json!({ "swarm": swarm.slug, "id": s.id, "when": s.describe(), "to": s.to,
                            "task": s.task, "last_run": s.last_run })
                })
        })
        .collect();
    Ok(json!({ "schedules": rows }))
}

fn op_schedule_remove(state: &mut State, args: &Value) -> Result<Value, String> {
    let swarm = schedule_swarm(state, args)?;
    let id = arg(args, "id").ok_or("missing id")?;
    let mut book = super::schedule::load(&swarm.root);
    let before = book.schedules.len();
    book.schedules.retain(|s| s.id != id);
    if book.schedules.len() == before {
        return Err(format!("swarm {:?} has no schedule {id:?}", swarm.slug));
    }
    super::schedule::save(&swarm.root, &book)?;
    Ok(json!({ "swarm": swarm.slug, "removed": id }))
}

/// Fires a schedule now (it still waits for its target to be idle).
fn op_schedule_run(state: &mut State, args: &Value) -> Result<Value, String> {
    let swarm = schedule_swarm(state, args)?;
    let id = arg(args, "id").ok_or("missing id")?;
    let mut book = super::schedule::load(&swarm.root);
    let index = book
        .schedules
        .iter()
        .position(|s| s.id == id)
        .ok_or(format!("swarm {:?} has no schedule {id:?}", swarm.slug))?;
    let sent = fire_schedule(state, &swarm, &book.schedules[index])?;
    if sent {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();
        book.schedules[index].mark_run(now, &super::schedule::local_now());
        super::schedule::save(&swarm.root, &book)?;
    }
    Ok(json!({ "swarm": swarm.slug, "id": id, "queued": sent }))
}

fn op_info(state: &State, args: &Value) -> Result<Value, String> {
    let slug = match arg(args, "swarm") {
        Some(slug) => slug.to_string(),
        None => match context_swarm(state, args) {
            Some(slug) => slug,
            None if state.swarms.len() == 1 => state.swarms[0].slug.clone(),
            None => return Err("which swarm? pass a swarm slug".into()),
        },
    };
    let swarm = state.swarm(&slug).ok_or(format!("no swarm {slug:?}"))?;
    Ok(json!({ "swarm": slug, "lines": swarm_info(state, swarm) }))
}

/// Marks the manifest `state` (setup providers may set the same field, e.g. on pause) and
/// reloads, so deliveries stop or resume at once.
fn op_set_paused(state: &mut State, args: &Value, paused: bool) -> Result<Value, String> {
    let slug = arg(args, "slug").ok_or("missing slug")?;
    let root = state
        .swarm(slug)
        .map(|swarm| swarm.root.clone())
        .ok_or(format!("no swarm {slug:?}"))?;
    let path = super::model::manifest_path(&root);
    let mut manifest: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or(format!("cannot read {}", path.display()))?;
    manifest["state"] = json!(if paused { "paused" } else { "running" });
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
    )
    .and_then(|_| std::fs::rename(&tmp, &path))
    .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    state.reload();
    let queued: usize = state
        .queues
        .iter()
        .filter(|((queue_slug, _), _)| queue_slug == slug)
        .map(|(_, queue)| queue.len())
        .sum();
    Ok(json!({ "slug": slug, "paused": paused, "queued": queued }))
}

fn op_forget(state: &mut State, args: &Value) -> Result<Value, String> {
    let slug = arg(args, "slug").ok_or("missing slug")?;
    let root = state
        .swarm(slug)
        .map(|swarm| swarm.root.clone())
        .ok_or(format!("no swarm {slug:?}"))?;
    let roots: Vec<PathBuf> = read_registry(&state.registry_path)
        .into_iter()
        .filter(|r| *r != root)
        .collect();
    write_registry(&state.registry_path, &roots)?;
    state.reload();
    state.queues.retain(|(queue_slug, _), _| queue_slug != slug);
    Ok(json!({ "forgotten": slug }))
}

fn status_str(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Unknown => "unknown",
    }
}

fn swarm_json(state: &State, swarm: &Swarm) -> Value {
    let agents: Vec<Value> = swarm
        .agents
        .iter()
        .map(|agent| {
            let live = state.live(agent);
            let queued = state.queues.get(&(swarm.slug.clone(), agent.key.clone())).map_or(0, Vec::len);
            json!({
                "key": agent.key,
                "role": agent.role.as_str(),
                "kind": agent.kind.as_str(),
                "herdr_name": agent.herdr_name,
                "model": agent.model,
                "pane_id": live.map(|l| l.pane_id.clone()).or_else(|| agent.pane_id.clone()),
                "status": live.map_or(if agent.role == Role::Script { "script" } else { "gone" }, |l| status_str(l.status)),
                "queued": queued,
            })
        })
        .collect();
    json!({ "slug": swarm.slug, "root": swarm.root, "paused": swarm.paused, "solo": swarm.solo, "home": swarm.home, "profile": swarm.profile, "agents": agents })
}

fn op_list(state: &State) -> Value {
    json!({ "swarms": state.swarms.iter().map(|swarm| swarm_json(state, swarm)).collect::<Vec<_>>() })
}

/// The swarm the caller is looking at: its own pane, else the focused pane
/// (`context_pane`, passed by popups and key commands, which have no pane of their own).
fn context_swarm(state: &State, args: &Value) -> Option<String> {
    ["from_pane", "context_pane"]
        .iter()
        .filter_map(|field| arg(args, field))
        .find_map(|pane| {
            state
                .agent_in_pane(pane)
                .map(|(swarm, _)| swarm.slug.clone())
        })
}

/// Who is calling: explicit `from`, else the agent running in `from_pane`, else the human.
fn identify(state: &State, args: &Value) -> Result<Sender, String> {
    if let Some(from) = arg(args, "from") {
        if from == bus::HUMAN {
            return Ok(Sender::human());
        }
        let (slug, key) = from
            .split_once('/')
            .ok_or("--from must be <swarm>/<agent> or human")?;
        let swarm = state.swarm(slug).ok_or(format!("no swarm {slug:?}"))?;
        let agent = swarm
            .agent(key)
            .ok_or(format!("swarm {slug:?} has no agent {key:?}"))?;
        return Ok(Sender {
            swarm: Some(slug.into()),
            key: agent.key.clone(),
            role: Some(agent.role),
        });
    }
    if let Some((swarm, agent)) = arg(args, "from_pane").and_then(|pane| state.agent_in_pane(pane))
    {
        return Ok(Sender {
            swarm: Some(swarm.slug.clone()),
            key: agent.key.clone(),
            role: Some(agent.role),
        });
    }
    // No pane id at all: the caller's environment was scrubbed (Codex runs shell commands
    // without HERDR_PANE_ID). Every agent works in its own folder, so its cwd identifies it.
    // Never used when a pane id was given, so a human cd'd into an agent folder stays human.
    if arg(args, "from_pane").is_none() && arg(args, "context_pane").is_none() {
        if let Some((swarm, agent)) = arg(args, "cwd").and_then(|cwd| state.agent_in_dir(cwd)) {
            return Ok(Sender {
                swarm: Some(swarm.slug.clone()),
                key: agent.key.clone(),
                role: Some(agent.role),
            });
        }
    }
    Ok(Sender::human())
}

fn op_send(state: &mut State, args: &Value) -> Result<Value, String> {
    let to = arg(args, "to").ok_or("missing to")?;
    let text = arg(args, "text").ok_or("missing text")?;
    let kind = match arg(args, "kind") {
        Some(kind) => Kind::parse(kind).ok_or(format!("unknown kind {kind:?}"))?,
        None => Kind::Normal,
    };
    let sender = identify(state, args)?;
    let default_swarm = arg(args, "swarm")
        .map(str::to_string)
        .or_else(|| sender.swarm.clone())
        .or_else(|| context_swarm(state, args));
    let recipients = bus::resolve(to, &sender, default_swarm.as_deref(), &state.swarms)?;
    let comms = state.comms_for(&sender);
    let from = sender.label();
    if let Some(comms) = &comms {
        if state.rate_limited(&from, comms) {
            return Err(format!(
                "{from} is sending too fast ({}/min); slow down: batch what you have into one message",
                comms.rate_per_min
            ));
        }
        if to.trim().starts_with('@') && !state.broadcast_allowed(&from, comms) {
            return Err(format!(
                "{from} already sent {} broadcasts ({to}) this hour; message only the agents who must act",
                comms.broadcasts_per_hour
            ));
        }
    }
    let ts = now_ms();
    let mut sent = Vec::new();
    let mut downgraded = Vec::new();
    let mut over_hourly = false;
    let mut duplicates = Vec::new();
    for (slug, key) in recipients {
        let mut kind = kind;
        let to_label = slug
            .as_ref()
            .map_or(key.clone(), |slug| format!("{slug}/{key}"));
        if let Some(comms) = &comms {
            if state.is_duplicate(&from, &to_label, text, comms) {
                duplicates.push(to_label);
                continue;
            }
            // Agent↔agent only: the human and hivey may always wake an agent.
            if slug.is_some() && kind != Kind::Fyi {
                if !state.pair_allows_wake(&from, &to_label, comms) {
                    kind = Kind::Fyi;
                    downgraded.push(to_label.clone());
                } else if !state.hourly_allows_wake(&from, comms) {
                    kind = Kind::Fyi;
                    over_hourly = true;
                }
            }
        }
        let msg = Message {
            id: state.next_id(),
            ts,
            from: from.clone(),
            swarm: slug.clone(),
            to: key.clone(),
            addressed: to.to_string(),
            kind,
            text: text.to_string(),
            reply_to: arg(args, "reply_to").map(str::to_string),
            copy: false,
        };
        sent.push(json!({ "id": msg.id, "to": to_label }));
        state.post(msg)?;
    }
    let mut notices = Vec::new();
    if let Some(comms) = &comms {
        if !downgraded.is_empty() {
            notices.push(format!(
                "you and {} have woken each other {} times in {} min; saved as FYI so it won't wake them. \
                 Message again only if they must act; otherwise stop replying.",
                downgraded.join(", "),
                comms.pair_wakes,
                comms.pair_window_min
            ));
        }
        if over_hourly {
            notices.push(format!(
                "you've sent {} wake-up messages this hour; saved as FYI so it won't wake anyone. \
                 Batch what's left into one message, or stop until it matters.",
                comms.hourly_wakes
            ));
        }
        if !duplicates.is_empty() {
            notices.push(format!(
                "not sent to {}: you sent the same text in the last {} min",
                duplicates.join(", "),
                comms.dedupe_min
            ));
        }
    }
    let mut result = json!({ "from": from, "sent": sent });
    if !notices.is_empty() {
        result["notice"] = json!(notices.join(" "));
    }
    Ok(result)
}

/// Pulled messages count as delivered, so they aren't typed into the pane later.
fn op_inbox(state: &mut State, args: &Value) -> Result<Value, String> {
    let who = match arg(args, "agent") {
        Some(agent) => {
            let from = if agent.contains('/') {
                agent.to_string()
            } else {
                let slug = arg(args, "swarm").ok_or("--agent needs --swarm or <swarm>/<agent>")?;
                format!("{slug}/{agent}")
            };
            identify(state, &json!({ "from": from }))?
        }
        None => identify(state, args)?,
    };
    let Some(slug) = who.swarm.clone() else {
        return Err("not inside a swarm pane; use --agent <swarm>/<agent>".into());
    };
    let swarm = state.swarm(&slug).ok_or("swarm vanished")?.clone();
    let records = bus::read_log(&swarm.bus_path());
    let read: HashSet<&str> = records
        .iter()
        .filter_map(|r| match r {
            Record::Read { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    let show_all = args.get("all").and_then(Value::as_bool).unwrap_or(false);
    let mine: Vec<&Message> = records
        .iter()
        .filter_map(|r| match r {
            Record::Msg(msg)
                if !msg.copy
                    && msg.to == who.key
                    && msg.swarm.as_deref() == Some(slug.as_str()) =>
            {
                Some(msg)
            }
            _ => None,
        })
        .filter(|msg| show_all || !read.contains(msg.id.as_str()))
        .collect();
    let mine: Vec<&Message> = mine.into_iter().rev().take(50).rev().collect();

    let ts = now_ms();
    let queue_key = (slug.clone(), who.key.clone());
    let queued: HashSet<String> = state
        .queues
        .get(&queue_key)
        .map(|q| q.iter().map(|m| m.id.clone()).collect())
        .unwrap_or_default();
    let mut receipts = Vec::new();
    for msg in &mine {
        if queued.contains(&msg.id) {
            receipts.push(Record::Delivered {
                id: msg.id.clone(),
                to: who.key.clone(),
                ts,
                batch: 0,
            });
        }
        if !read.contains(msg.id.as_str()) {
            receipts.push(Record::Read {
                id: msg.id.clone(),
                to: who.key.clone(),
                ts,
            });
        }
    }
    bus::append(&swarm.bus_path(), &receipts).map_err(|err| err.to_string())?;
    if let Some(queue) = state.queues.get_mut(&queue_key) {
        queue.retain(|msg| !mine.iter().any(|m| m.id == msg.id));
    }
    Ok(json!({ "agent": who.label(), "messages": mine }))
}

fn op_log(state: &State, args: &Value) -> Result<Value, String> {
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(30) as usize;
    let slug = arg(args, "swarm")
        .map(str::to_string)
        .or_else(|| context_swarm(state, args));
    let swarms: Vec<&Swarm> = match slug.as_deref() {
        Some(slug) => vec![state.swarm(slug).ok_or(format!("no swarm {slug:?}"))?],
        None => state.swarms.iter().collect(),
    };
    let mut records: Vec<(u64, Value)> = Vec::new();
    for swarm in swarms {
        for record in bus::read_log(&swarm.bus_path()) {
            let ts = match &record {
                Record::Msg(msg) => msg.ts,
                Record::Delivered { ts, .. }
                | Record::Read { ts, .. }
                | Record::Held { ts, .. } => *ts,
            };
            let mut value = serde_json::to_value(&record).unwrap_or(Value::Null);
            if let Some(object) = value.as_object_mut() {
                object.insert("log".into(), json!(swarm.slug));
            }
            records.push((ts, value));
        }
    }
    records.sort_by_key(|(ts, _)| *ts);
    let skip = records.len().saturating_sub(limit);
    Ok(json!({ "records": records.into_iter().skip(skip).map(|(_, v)| v).collect::<Vec<_>>() }))
}

fn op_master(state: &State, args: &Value) -> Result<Value, String> {
    let slug = match arg(args, "swarm") {
        Some(slug) => slug.to_string(),
        None => match context_swarm(state, args) {
            Some(slug) => slug,
            None if state.swarms.len() == 1 => state.swarms[0].slug.clone(),
            None => return Err("which swarm? pass a swarm slug".into()),
        },
    };
    let swarm = state.swarm(&slug).ok_or(format!("no swarm {slug:?}"))?;
    let master = swarm
        .master()
        .ok_or(format!("swarm {slug:?} has no master"))?;
    let pane = state.live(master).map(|live| live.pane_id.clone());
    Ok(
        json!({ "swarm": slug, "key": master.key, "herdr_name": master.herdr_name, "pane_id": pane }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_swarm(slug: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("hivey-engine-{slug}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".swarm")).unwrap();
        std::fs::write(
            root.join(".swarm/agents.json"),
            json!({
                "slug": slug,
                "coordinator": format!("{slug}-coordinator"),
                "agents": {
                    "scout": {"herdr_name": format!("{slug}-scout"), "pane_id": "w1:p2"},
                    "critic": {"herdr_name": format!("{slug}-critic")}
                }
            })
            .to_string(),
        )
        .unwrap();
        root
    }

    fn state_with(roots: &[&Path]) -> State {
        // One file per call: tests run in parallel, and a shared name let them overwrite each
        // other's registry ("no swarm named …").
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let registry = std::env::temp_dir().join(format!(
            "hivey-registry-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        write_registry(
            &registry,
            &roots.iter().map(|r| r.to_path_buf()).collect::<Vec<_>>(),
        )
        .unwrap();
        let mut state = State {
            registry_path: registry,
            ..State::default()
        };
        state.reload();
        state.rebuild_queues();
        state
    }

    fn info(name: &str, pane: &str, status: AgentStatus) -> AgentInfo {
        serde_json::from_value(json!({
            "terminal_id": format!("t-{pane}"), "name": name, "agent_status": status,
            "workspace_id": "w1", "tab_id": "w1:t1", "pane_id": pane, "focused": false, "revision": 1
        }))
        .unwrap()
    }

    #[test]
    fn send_queues_until_idle_then_delivers_one_digest_and_survives_restart() {
        let root = temp_swarm("eng");
        let mut state = state_with(&[&root]);
        state.update_live(vec![
            info("eng-coordinator", "w1:p1", AgentStatus::Idle),
            info("eng-scout", "w1:p2", AgentStatus::Working),
        ]);
        // Coordinator (pane p1) sends two messages to the working scout.
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "scout", "text": "one", "from_pane": "w1:p1"}),
        )
        .unwrap();
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "scout", "text": "fyi", "kind": "fyi", "from_pane": "w1:p1"}),
        )
        .unwrap();
        assert!(plan_deliveries(&mut state).is_empty(), "held while working");

        // A restarted engine rebuilds the same queue from bus.jsonl.
        let mut restarted = state_with(&[&root]);
        assert_eq!(
            restarted.queues[&("eng".to_string(), "scout".to_string())].len(),
            2
        );

        // Scout goes idle: after the settle time, one digest carries both.
        restarted.update_live(vec![info("eng-scout", "w1:p2", AgentStatus::Idle)]);
        restarted.live_by_name.get_mut("eng-scout").unwrap().since -=
            Duration::from_millis(bus::IDLE_SETTLE_MS);
        let deliveries = plan_deliveries(&mut restarted);
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].target, "w1:p2");
        assert!(
            deliveries[0].text.contains("from coordinator") && deliveries[0].text.contains("FYI"),
            "{}",
            deliveries[0].text
        );
        confirm_delivery(&mut restarted, &deliveries[0]);
        assert!(restarted.queues[&("eng".to_string(), "scout".to_string())].is_empty());
        assert!(
            state_with(&[&root]).queues.values().all(Vec::is_empty),
            "delivery is recorded"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Marks a live pane as running Claude session `session` with a registry entry.
    fn as_claude(state: &mut State, name: &str, session: &str, status: &str, accepts: bool) {
        let live = state.live_by_name.get_mut(name).unwrap();
        live.session = Some(("herdr:claude".into(), "claude".into(), session.into()));
        let pane = live.pane_id.clone();
        let live = live.clone();
        state.live_by_pane.insert(pane, live);
        let peer: claude_peers::Peer = serde_json::from_value(json!({
            "pid": 4242, "sessionId": session, "status": status, "startedAt": 1,
            "messagingSocketPath": "/tmp/cc-socks/4242.sock"
        }))
        .unwrap();
        state.claude_peers.insert(session.into(), peer);
        state.inbox_accepts.insert(4242, accepts);
    }

    #[test]
    fn a_claude_session_that_reports_busy_is_not_typed_into() {
        let root = temp_swarm("busy");
        let mut state = state_with(&[&root]);
        state.update_live(vec![
            info("busy-coordinator", "w1:p1", AgentStatus::Idle),
            info("busy-scout", "w1:p2", AgentStatus::Idle),
        ]);
        // No Claude session in any pane: the registry isn't read and stale entries go.
        state
            .claude_peers
            .insert("stale".into(), claude_peers::Peer::test_new("stale"));
        state.refresh_claude_peers();
        assert!(state.claude_peers.is_empty());
        as_claude(&mut state, "busy-scout", "s-busy", "busy", false);
        state.live_by_name.get_mut("busy-scout").unwrap().since -=
            Duration::from_millis(bus::IDLE_SETTLE_MS);
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "scout", "text": "go", "from_pane": "w1:p1"}),
        )
        .unwrap();
        assert!(plan_deliveries(&mut state).is_empty(), "registry says busy");
        state.claude_peers.get_mut("s-busy").unwrap().status = Some("idle".into());
        assert_eq!(plan_deliveries(&mut state).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_messages_go_to_the_claude_inbox_and_the_humans_are_typed() {
        let root = temp_swarm("inbox");
        let mut state = state_with(&[&root]);
        state.update_live(vec![
            info("inbox-coordinator", "w1:p1", AgentStatus::Idle),
            info("inbox-scout", "w1:p2", AgentStatus::Idle),
        ]);
        as_claude(&mut state, "inbox-scout", "s-in", "idle", true);
        state.live_by_name.get_mut("inbox-scout").unwrap().since -=
            Duration::from_millis(bus::IDLE_SETTLE_MS);
        let key = ("inbox".to_string(), "scout".to_string());
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "scout", "text": "from the master", "from_pane": "w1:p1"}),
        )
        .unwrap();
        let deliveries = plan_deliveries(&mut state);
        assert_eq!(
            deliveries[0].inbox.as_deref(),
            Some(Path::new("/tmp/cc-socks/4242.sock"))
        );
        confirm_delivery(&mut state, &deliveries[0]);
        assert!(state.queues[&key].is_empty());

        // From the human: typed, so it keeps the user's authority.
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "inbox/scout", "text": "from me"}),
        )
        .unwrap();
        let deliveries = plan_deliveries(&mut state);
        assert_eq!(deliveries.len(), 1);
        assert_eq!(deliveries[0].inbox, None);
        confirm_delivery(&mut state, &deliveries[0]);

        // A session that would hold the post for approval is typed into too.
        state.inbox_accepts.insert(4242, false);
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "scout", "text": "again", "from_pane": "w1:p1"}),
        )
        .unwrap();
        assert_eq!(plan_deliveries(&mut state)[0].inbox, None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_unreachable_inbox_falls_back_to_typing() {
        let mut delivery = Delivery {
            slug: "s".into(),
            key: "a".into(),
            target: "w1:p2".into(),
            text: "hi".into(),
            ids: vec![],
            inbox: None,
        };
        assert!(!post_to_inbox(&delivery), "no inbox: type it");
        delivery.inbox = Some(std::env::temp_dir().join("hivey-no-such-inbox.sock"));
        assert!(!post_to_inbox(&delivery), "dead socket: type it");
    }

    #[test]
    fn blocked_target_escalates_to_master_once() {
        let root = temp_swarm("blk");
        let mut state = state_with(&[&root]);
        state.update_live(vec![
            info("blk-coordinator", "w1:p1", AgentStatus::Working),
            info("blk-scout", "w1:p2", AgentStatus::Blocked),
        ]);
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "blk/scout", "text": "go"}),
        )
        .unwrap();
        assert!(plan_deliveries(&mut state).is_empty());
        let master_queue = &state.queues[&("blk".to_string(), "coordinator".to_string())];
        assert_eq!(master_queue.len(), 1);
        assert!(
            master_queue[0].text.starts_with("scout is blocked"),
            "{}",
            master_queue[0].text
        );
        plan_deliveries(&mut state);
        assert_eq!(
            state.queues[&("blk".to_string(), "coordinator".to_string())].len(),
            1,
            "only once"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn master_is_told_once_per_blocked_episode_not_per_message() {
        let root = temp_swarm("epi");
        let mut state = state_with(&[&root]);
        let live = |status| {
            vec![
                info("epi-coordinator", "w1:p1", AgentStatus::Working),
                info("epi-scout", "w1:p2", status),
            ]
        };
        let notices = |state: &State| {
            state
                .queues
                .get(&("epi".to_string(), "coordinator".to_string()))
                .map_or(0, Vec::len)
        };
        state.update_live(live(AgentStatus::Blocked));
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "epi/scout", "text": "a"}),
        )
        .unwrap();
        plan_deliveries(&mut state);
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "epi/scout", "text": "b"}),
        )
        .unwrap();
        plan_deliveries(&mut state);
        assert_eq!(notices(&state), 1, "one notice for the episode");
        // Unblocked, then blocked again: a new episode notifies again.
        state.update_live(live(AgentStatus::Working));
        plan_deliveries(&mut state);
        state.update_live(live(AgentStatus::Blocked));
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "epi/scout", "text": "c"}),
        )
        .unwrap();
        plan_deliveries(&mut state);
        assert_eq!(notices(&state), 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn messages_to_the_human_are_logged_once() {
        let root = temp_swarm("hum");
        let mut state = state_with(&[&root]);
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "human", "text": "hi", "from": "hum/scout"}),
        )
        .unwrap();
        let records = bus::read_log(&root.join(".swarm/bus.jsonl"));
        assert_eq!(records.len(), 1, "{records:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paused_swarms_hold_deliveries_until_resumed() {
        let root = temp_swarm("pau");
        let mut state = state_with(&[&root]);
        state.update_live(vec![info("pau-scout", "w1:p2", AgentStatus::Idle)]);
        state.live_by_name.get_mut("pau-scout").unwrap().since -=
            Duration::from_millis(bus::IDLE_SETTLE_MS);
        run_op(&mut state, "pause", &json!({"slug": "pau"})).unwrap();
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "pau/scout", "text": "hi"}),
        )
        .unwrap();
        assert!(plan_deliveries(&mut state).is_empty(), "held while paused");
        let listed = run_op(&mut state, "list", &json!({})).unwrap();
        assert_eq!(listed["swarms"][0]["paused"], true);
        run_op(&mut state, "resume", &json!({"slug": "pau"})).unwrap();
        state.live_by_name.get_mut("pau-scout").unwrap().since -=
            Duration::from_millis(bus::IDLE_SETTLE_MS);
        assert_eq!(plan_deliveries(&mut state).len(), 1, "released on resume");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn agents_without_a_pane_id_are_identified_by_their_folder() {
        let root = temp_swarm("cwd");
        std::fs::create_dir_all(root.join("scout/src")).unwrap();
        let mut state = state_with(&[&root]);
        let sent = run_op(
            &mut state,
            "msg.send",
            &json!({"to": "critic", "text": "hi", "cwd": root.join("scout/src")}),
        )
        .unwrap();
        assert_eq!(sent["from"], "cwd/scout");
        // A pane id that isn't an agent (the human's shell) wins over the folder.
        let human = run_op(
            &mut state,
            "msg.send",
            &json!({"to": "cwd/critic", "text": "hi", "cwd": root.join("scout"), "from_pane": "w9:p9"}),
        )
        .unwrap();
        assert_eq!(human["from"], "human");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn swarm_spaces_get_a_summary_and_their_master_pane() {
        let root = temp_swarm("sum");
        let mut state = state_with(&[&root]);
        state.update_live(vec![
            info("sum-coordinator", "w1:p1", AgentStatus::Idle),
            info("sum-scout", "w1:p2", AgentStatus::Working),
            info("sum-critic", "w1:p3", AgentStatus::Blocked),
        ]);
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "sum/scout", "text": "x"}),
        )
        .unwrap();
        let reports = plan_workspace_tokens(&mut state);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].workspace_id, "w1");
        assert_eq!(reports[0].tokens["swarm"].as_deref(), Some("●1/3 ⚠1 ✉1"));
        assert_eq!(reports[0].tokens["master_pane"].as_deref(), Some("w1:p1"));
        assert!(
            plan_workspace_tokens(&mut state).is_empty(),
            "only on change"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn swarm_info_reads_manifest_readme_and_wraps_for_tokens() {
        let root = temp_swarm("inf");
        let mut manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".swarm/agents.json")).unwrap(),
        )
        .unwrap();
        manifest["channel_id"] = json!("C123");
        manifest["info"] = json!({"Slack": "#swarm-inf (C123)", "vault": "/v/swarm-inf-wiki/"});
        std::fs::write(root.join(".swarm/agents.json"), manifest.to_string()).unwrap();
        let mut state = state_with(&[&root]);
        state.update_live(vec![info("inf-scout", "w1:p2", AgentStatus::Working)]);
        let info = run_op(&mut state, "info", &json!({"swarm": "inf"})).unwrap();
        let text: Vec<String> = serde_json::from_value(info["lines"].clone()).unwrap();
        let text = text.join("\n");
        assert!(text.contains("◆ master    coordinator · gone"), "{text}");
        assert!(text.contains("● scout claude working"), "{text}");
        assert!(text.contains("Slack       #swarm-inf (C123)"), "{text}");
        assert!(!text.contains("channel     C123"), "not repeated: {text}");
        assert!(text.contains("vault       /v/swarm-inf-wiki/"), "{text}");
        let long = vec![format!(
            "agents      {}",
            vec!["● agent-name claude/sonnet idle"; 6].join(", ")
        )];
        let wrapped = info_token_lines(&long);
        assert!(
            wrapped.len() > 1 && wrapped.iter().all(|l| l.chars().count() <= 80),
            "{wrapped:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn schedules_wake_the_master_once_and_never_pile_up() {
        let root = temp_swarm("sch");
        let mut state = state_with(&[&root]);
        state.update_live(vec![
            info("sch-coordinator", "w1:p1", AgentStatus::Working),
            info("sch-scout", "w1:p2", AgentStatus::Blocked),
        ]);
        let added = run_op(
            &mut state,
            "schedule.add",
            &json!({"swarm": "sch", "every": "15m", "task": "check the board"}),
        )
        .unwrap();
        assert_eq!(added["added"]["when"], "every 15m");
        assert!(run_op(
            &mut state,
            "schedule.add",
            &json!({"swarm": "sch", "task": "x"})
        )
        .is_err());
        // Not due yet: nothing queued.
        run_due_schedules(&mut state);
        assert!(state
            .queues
            .get(&("sch".to_string(), "coordinator".to_string()))
            .is_none_or(Vec::is_empty));
        // Run it now; a second run while the first waits is skipped.
        let first = run_op(
            &mut state,
            "schedule.run",
            &json!({"swarm": "sch", "id": "s1"}),
        )
        .unwrap();
        assert_eq!(first["queued"], true);
        let second = run_op(
            &mut state,
            "schedule.run",
            &json!({"swarm": "sch", "id": "s1"}),
        )
        .unwrap();
        assert_eq!(second["queued"], false, "no pile-up");
        let queue = &state.queues[&("sch".to_string(), "coordinator".to_string())];
        assert_eq!(queue.len(), 1);
        assert!(
            queue[0].text.contains("check the board"),
            "{}",
            queue[0].text
        );
        assert!(
            queue[0].text.contains("needs you: scout blocked"),
            "{}",
            queue[0].text
        );
        let listed = run_op(&mut state, "schedule.list", &json!({"swarm": "sch"})).unwrap();
        assert_eq!(listed["schedules"].as_array().unwrap().len(), 1);
        run_op(
            &mut state,
            "schedule.remove",
            &json!({"swarm": "sch", "id": "s1"}),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn inbox_pull_marks_read_and_cancels_pending_delivery() {
        let root = temp_swarm("inb");
        let mut state = state_with(&[&root]);
        state.update_live(vec![info("inb-scout", "w1:p2", AgentStatus::Working)]);
        run_op(
            &mut state,
            "msg.send",
            &json!({"to": "inb/scout", "text": "hello"}),
        )
        .unwrap();
        let inbox = run_op(&mut state, "msg.inbox", &json!({"from_pane": "w1:p2"})).unwrap();
        assert_eq!(inbox["messages"].as_array().unwrap().len(), 1);
        assert!(state.queues[&("inb".to_string(), "scout".to_string())].is_empty());
        let again = run_op(&mut state, "msg.inbox", &json!({"from_pane": "w1:p2"})).unwrap();
        assert!(
            again["messages"].as_array().unwrap().is_empty(),
            "read messages are not shown twice"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_ping_pong_is_downgraded_to_fyi_after_the_pair_limit() {
        let root = temp_swarm("png");
        let mut state = state_with(&[&root]);
        let limit = Comms::default().pair_wakes;
        for round in 0..limit {
            let (from, to) = if round % 2 == 0 {
                ("png/scout", "coordinator")
            } else {
                ("png/coordinator", "scout")
            };
            let sent = run_op(
                &mut state,
                "msg.send",
                &json!({"to": to, "text": format!("ok {round}"), "from": from}),
            )
            .unwrap();
            assert!(sent.get("notice").is_none(), "round {round} still wakes");
        }
        let sent = run_op(
            &mut state,
            "msg.send",
            &json!({"to": "coordinator", "text": "ok again", "from": "png/scout"}),
        )
        .unwrap();
        assert!(sent["notice"].as_str().unwrap().contains("saved as FYI"));
        let queue = &state.queues[&("png".to_string(), "coordinator".to_string())];
        assert_eq!(queue.last().unwrap().kind, Kind::Fyi);
        // The human is never limited.
        let human = run_op(
            &mut state,
            "msg.send",
            &json!({"to": "png/coordinator", "text": "go"}),
        )
        .unwrap();
        assert!(human.get("notice").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A temp swarm whose manifest carries `comms`.
    fn temp_swarm_with_comms(slug: &str, comms: Value) -> PathBuf {
        let root = temp_swarm(slug);
        let path = root.join(".swarm/agents.json");
        let mut manifest: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        manifest["comms"] = comms;
        std::fs::write(&path, manifest.to_string()).unwrap();
        root
    }

    fn send(state: &mut State, from: &str, to: &str, text: &str) -> Result<Value, String> {
        run_op(
            state,
            "msg.send",
            &json!({"to": to, "text": text, "from": from}),
        )
    }

    #[test]
    fn rate_limit_comes_from_the_senders_swarm() {
        let tight = temp_swarm_with_comms("rt", json!({"rate_per_min": 2, "dedupe_min": 0}));
        let loose = temp_swarm_with_comms("rl", json!({"rate_per_min": 50, "dedupe_min": 0}));
        let mut state = state_with(&[&tight, &loose]);
        for n in 0..2 {
            send(&mut state, "rt/scout", "coordinator", &format!("m{n}")).unwrap();
        }
        let err = send(&mut state, "rt/scout", "coordinator", "m2").unwrap_err();
        assert!(err.contains("too fast (2/min)"), "{err}");
        for n in 0..5 {
            send(&mut state, "rl/scout", "coordinator", &format!("m{n}")).unwrap();
        }
        // The human is never limited.
        for n in 0..5 {
            run_op(
                &mut state,
                "msg.send",
                &json!({"to": "rt/scout", "text": format!("h{n}")}),
            )
            .unwrap();
        }
        let _ = std::fs::remove_dir_all(&tight);
        let _ = std::fs::remove_dir_all(&loose);
    }

    #[test]
    fn wake_ups_over_the_hourly_budget_are_saved_as_fyi() {
        let root = temp_swarm_with_comms(
            "hw",
            json!({"hourly_wakes": 2, "pair_wakes": 50, "rate_per_min": 50, "dedupe_min": 0}),
        );
        let mut state = state_with(&[&root]);
        for n in 0..2 {
            let sent = send(&mut state, "hw/scout", "coordinator", &format!("m{n}")).unwrap();
            assert!(sent.get("notice").is_none());
        }
        let sent = send(&mut state, "hw/scout", "coordinator", "m2").unwrap();
        assert!(sent["notice"]
            .as_str()
            .unwrap()
            .contains("wake-up messages this hour"));
        let queue = &state.queues[&("hw".to_string(), "coordinator".to_string())];
        assert_eq!(queue.last().unwrap().kind, Kind::Fyi);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn broadcasts_are_capped_per_hour() {
        let root = temp_swarm_with_comms(
            "bc",
            json!({"broadcasts_per_hour": 1, "rate_per_min": 50, "dedupe_min": 0}),
        );
        let mut state = state_with(&[&root]);
        send(&mut state, "bc/scout", "@all", "first").unwrap();
        let err = send(&mut state, "bc/scout", "@all", "second").unwrap_err();
        assert!(err.contains("broadcasts"), "{err}");
        // A direct message still goes through.
        send(&mut state, "bc/scout", "coordinator", "direct").unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_same_text_to_the_same_agent_is_dropped() {
        let root = temp_swarm_with_comms("dd", json!({"dedupe_min": 10, "rate_per_min": 50}));
        let mut state = state_with(&[&root]);
        send(&mut state, "dd/scout", "coordinator", "status: done").unwrap();
        let again = send(&mut state, "dd/scout", "coordinator", "status: done ").unwrap();
        assert!(again["sent"].as_array().unwrap().is_empty());
        assert!(again["notice"].as_str().unwrap().contains("same text"));
        let queue = &state.queues[&("dd".to_string(), "coordinator".to_string())];
        assert_eq!(queue.len(), 1);
        // Different text, or the human repeating itself, is delivered.
        send(&mut state, "dd/scout", "coordinator", "status: blocked").unwrap();
        for _ in 0..2 {
            let human = run_op(
                &mut state,
                "msg.send",
                &json!({"to": "dd/coordinator", "text": "go"}),
            )
            .unwrap();
            assert_eq!(human["sent"].as_array().unwrap().len(), 1);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn popup_sends_as_human_into_the_focused_swarm() {
        let a = temp_swarm("pa");
        let b = temp_swarm("pb");
        let mut state = state_with(&[&a, &b]);
        state.update_live(vec![info("pb-scout", "w2:p2", AgentStatus::Working)]);
        let sent = run_op(
            &mut state,
            "msg.send",
            &json!({"to": "master", "text": "hi", "context_pane": "w2:p2"}),
        )
        .unwrap();
        assert_eq!(sent["from"], "human");
        assert_eq!(sent["sent"][0]["to"], "pb/coordinator");
        let master = run_op(&mut state, "master", &json!({"context_pane": "w2:p2"})).unwrap();
        assert_eq!(master["swarm"], "pb");
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn master_to_master_is_logged_in_both_swarms() {
        let a = temp_swarm("ma");
        let b = temp_swarm("mb");
        let mut state = state_with(&[&a, &b]);
        state.update_live(vec![info("ma-coordinator", "w1:p1", AgentStatus::Idle)]);
        let sent = run_op(
            &mut state,
            "msg.send",
            &json!({"to": "@masters", "text": "sync", "from_pane": "w1:p1"}),
        )
        .unwrap();
        assert_eq!(sent["sent"][0]["to"], "mb/coordinator");
        let log = run_op(&mut state, "msg.log", &json!({"swarm": "ma"})).unwrap();
        assert_eq!(log["records"][0]["copy"], true);
        let worker = run_op(
            &mut state,
            "msg.send",
            &json!({"to": "mb/scout", "text": "x", "from": "ma/scout"}),
        );
        assert!(worker.unwrap_err().contains("ask your master"));
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
