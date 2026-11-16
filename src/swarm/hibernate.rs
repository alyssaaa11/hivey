//! hivey: hibernation. An agent that stays idle for the configured days (hivey settings →
//! hibernate, or a space's own Settings…; default 2, see `agent_settings`) is stopped to free
//! its memory: its foreground process group (the agent CLI and its MCP servers) gets SIGTERM,
//! the pane keeps its shell, place and name. Opening its space again resumes it with the same
//! conversation (`claude --resume <id>` plus its launch flags), on the CLI version installed
//! now. A message for a hibernated swarm agent wakes it first (see `engine`).
//!
//! A background thread (off the render path) checks focus every second while something is
//! hibernated, and idle times every minute. Idle starts (wall clock) and the hibernated agents
//! are kept in `<session data dir>/hibernated.json`, so restarts neither lose a sleeper nor
//! restart the idle count.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::api::schema::{
    AgentInfo, AgentStartParams, AgentStatus, EmptyParams, Method, PaneInfo, PaneListParams,
    PaneProcessInfo, PaneProcessInfoParams, PaneSendInputParams, Request,
};
use crate::api::ApiRequestSender;

const TICK: Duration = Duration::from_secs(1);
/// How often idle agents are looked for.
const SCAN_EVERY: Duration = Duration::from_secs(60);
/// After SIGTERM, how long an agent gets to exit before SIGKILL.
const KILL_AFTER: Duration = Duration::from_secs(15);
const DAY_MS: u64 = 24 * 3600 * 1000;
/// Wait for a woken agent to be ready for input.
const WAKE_TIMEOUT_MS: u64 = 90_000;

/// A hibernated agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Sleeper {
    pub pane_id: String,
    pub workspace_id: String,
    /// The agent's herdr name, given back when it wakes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Agent label (`claude`, `codex`, …).
    pub agent: String,
    /// The command that resumes it (program first).
    pub argv: Vec<String>,
    pub at_ms: u64,
    /// Its space was left after it fell asleep; opening it again wakes it.
    #[serde(default)]
    pub seen_away: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Idle {
    session: String,
    since_ms: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default)]
    idle: BTreeMap<String, Idle>,
    #[serde(default)]
    sleepers: BTreeMap<String, Sleeper>,
}

#[derive(Default)]
struct Shared {
    path: PathBuf,
    saved: Saved,
    /// Panes to wake on the next tick.
    wake: HashSet<String>,
    /// Panes to hibernate now (menu, `hivey hibernate now`).
    now: HashSet<String>,
    /// SIGTERMed process groups still to confirm: pgid → when.
    terminating: HashMap<u32, Instant>,
}

static SHARED: OnceLock<Mutex<Shared>> = OnceLock::new();

fn shared() -> Option<std::sync::MutexGuard<'static, Shared>> {
    SHARED.get()?.lock().ok()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

pub(crate) fn start(api_tx: ApiRequestSender) {
    if std::env::var("HIVEY_HIBERNATE").is_ok_and(|v| v == "0" || v == "off") {
        return;
    }
    let path = crate::session::data_dir().join("hibernated.json");
    let saved = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    if SHARED
        .set(Mutex::new(Shared {
            path,
            saved,
            ..Shared::default()
        }))
        .is_err()
    {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("hivey-hibernate".into())
        .spawn(move || {
            let mut last_scan: Option<Instant> = None;
            loop {
                std::thread::sleep(TICK);
                tick(&api_tx, &mut last_scan);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "hivey hibernation failed to start");
    }
}

/// The hibernated agents (the swarm engine counts them as present, not gone).
pub(crate) fn sleepers() -> Vec<Sleeper> {
    shared()
        .map(|shared| shared.saved.sleepers.values().cloned().collect())
        .unwrap_or_default()
}

/// Wakes a hibernated agent on the next tick; false when it isn't hibernated.
pub(crate) fn request_wake(pane_id: &str) -> bool {
    let Some(mut shared) = shared() else {
        return false;
    };
    if !shared.saved.sleepers.contains_key(pane_id) {
        return false;
    }
    shared.wake.insert(pane_id.to_string());
    true
}

/// `hivey hibernate …` (socket method `swarm`, ops `hibernate.*`).
pub(crate) fn handle_op(op: &str, args: &Value) -> Result<Value, String> {
    let mut shared = shared().ok_or("hibernation is not running in this server")?;
    let pane = || {
        args.get("pane")
            .and_then(Value::as_str)
            .filter(|pane| !pane.is_empty())
            .map(str::to_string)
            .ok_or_else(|| "a pane is required".to_string())
    };
    match op {
        "hibernate.now" => {
            let pane = pane()?;
            if shared.saved.sleepers.contains_key(&pane) {
                return Err(format!("{pane} is already hibernated"));
            }
            shared.now.insert(pane.clone());
            Ok(json!({ "pane": pane, "hibernating": true }))
        }
        "hibernate.wake" => {
            let pane = pane()?;
            if !shared.saved.sleepers.contains_key(&pane) {
                return Err(format!("{pane} is not hibernated"));
            }
            shared.wake.insert(pane.clone());
            Ok(json!({ "pane": pane, "waking": true }))
        }
        "hibernate.list" => {
            let now = now_ms();
            let idle: Vec<Value> = shared
                .saved
                .idle
                .iter()
                .map(|(pane, idle)| {
                    json!({ "pane": pane, "idle_hours": now.saturating_sub(idle.since_ms) / 3_600_000 })
                })
                .collect();
            Ok(json!({
                "sleepers": shared.saved.sleepers.values().collect::<Vec<_>>(),
                "idle": idle,
            }))
        }
        other => Err(format!("unknown hibernate op {other:?}")),
    }
}

fn dispatch(api_tx: &ApiRequestSender, method: Method, timeout: Duration) -> Result<Value, String> {
    let request = Request {
        id: "hivey:hibernate".into(),
        method,
    };
    let raw = crate::api::dispatch_internal(request, api_tx, Some(timeout));
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

fn list<T: serde::de::DeserializeOwned>(
    api_tx: &ApiRequestSender,
    method: Method,
    key: &str,
) -> Option<Vec<T>> {
    dispatch(api_tx, method, Duration::from_secs(10))
        .ok()
        .and_then(|result| result.get(key).cloned())
        .and_then(|items| serde_json::from_value(items).ok())
}

fn tick(api_tx: &ApiRequestSender, last_scan: &mut Option<Instant>) {
    let (has_sleepers, wants_scan) = {
        let Some(shared) = shared() else { return };
        (
            !shared.saved.sleepers.is_empty() || !shared.terminating.is_empty(),
            !shared.now.is_empty() || last_scan.is_none_or(|at| at.elapsed() >= SCAN_EVERY),
        )
    };
    if !has_sleepers && !wants_scan {
        return;
    }
    let Some(panes) =
        list::<PaneInfo>(api_tx, Method::PaneList(PaneListParams::default()), "panes")
    else {
        return;
    };
    let active_workspace = panes
        .iter()
        .find(|pane| pane.focused)
        .map(|pane| pane.workspace_id.clone());
    if has_sleepers {
        watch_sleepers(api_tx, &panes, active_workspace.as_deref());
    }
    if wants_scan {
        *last_scan = Some(Instant::now());
        if let Some(agents) =
            list::<AgentInfo>(api_tx, Method::AgentList(EmptyParams::default()), "agents")
        {
            scan(api_tx, &agents, active_workspace.as_deref());
        }
    }
}

/// Wakes sleepers whose space was opened (or that were asked to wake), forgets those whose
/// pane is gone, and finishes off agents that ignored SIGTERM.
fn watch_sleepers(api_tx: &ApiRequestSender, panes: &[PaneInfo], active: Option<&str>) {
    let mut to_wake = Vec::new();
    {
        let Some(mut shared) = shared() else { return };
        let mut changed = false;
        let wake = std::mem::take(&mut shared.wake);
        let by_pane: HashMap<&str, &PaneInfo> = panes
            .iter()
            .map(|pane| (pane.pane_id.as_str(), pane))
            .collect();
        let ids: Vec<String> = shared.saved.sleepers.keys().cloned().collect();
        for id in ids {
            let Some(pane) = by_pane.get(id.as_str()) else {
                shared.saved.sleepers.remove(&id);
                changed = true;
                continue;
            };
            let Some(sleeper) = shared.saved.sleepers.get_mut(&id) else {
                continue;
            };
            if sleeper.workspace_id != pane.workspace_id {
                sleeper.workspace_id = pane.workspace_id.clone();
                changed = true;
            }
            let here = active == Some(pane.workspace_id.as_str());
            if !here && !sleeper.seen_away {
                sleeper.seen_away = true;
                changed = true;
            }
            if wake.contains(&id) || (here && sleeper.seen_away) {
                if let Some(sleeper) = shared.saved.sleepers.remove(&id) {
                    to_wake.push(sleeper);
                    changed = true;
                }
            }
        }
        let overdue: Vec<u32> = shared
            .terminating
            .iter()
            .filter(|(_, at)| at.elapsed() >= KILL_AFTER)
            .map(|(pgid, _)| *pgid)
            .collect();
        for pgid in overdue {
            shared.terminating.remove(&pgid);
            if group_alive(pgid) {
                tracing::info!(
                    pgid,
                    "hivey hibernate: agent ignored SIGTERM, sending SIGKILL"
                );
                signal_group(pgid, Signal::Kill);
            }
        }
        if changed {
            save(&shared);
        }
    }
    for sleeper in to_wake {
        wake(api_tx, sleeper);
    }
}

/// Restarts a hibernated agent in its pane, under its old name when it had one.
fn wake(api_tx: &ApiRequestSender, sleeper: Sleeper) {
    tracing::info!(pane = %sleeper.pane_id, agent = %sleeper.agent, "hivey: waking hibernated agent");
    if let Some(mut shared) = shared() {
        shared.saved.idle.remove(&sleeper.pane_id);
    }
    let api_tx = api_tx.clone();
    // `agent.start` waits until the agent is ready; don't hold up the other panes.
    let spawned = std::thread::Builder::new()
        .name("hivey-wake".into())
        .spawn(move || {
            let result = match &sleeper.name {
                Some(name) => dispatch(
                    &api_tx,
                    Method::AgentStart(AgentStartParams {
                        name: name.clone(),
                        kind: sleeper.agent.clone(),
                        pane_id: sleeper.pane_id.clone(),
                        args: sleeper.argv[1..].to_vec(),
                        timeout_ms: Some(WAKE_TIMEOUT_MS),
                    }),
                    Duration::from_millis(WAKE_TIMEOUT_MS + 5_000),
                ),
                None => dispatch(
                    &api_tx,
                    Method::PaneSendInput(PaneSendInputParams {
                        pane_id: sleeper.pane_id.clone(),
                        text: shell_join(&sleeper.argv),
                        keys: vec!["Enter".into()],
                    }),
                    Duration::from_secs(10),
                ),
            };
            if let Err(err) = result {
                tracing::warn!(pane = %sleeper.pane_id, %err, "hivey: hibernated agent did not wake");
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "hivey: cannot wake hibernated agent");
    }
}

/// Updates idle starts and hibernates agents idle past their space's limit (or asked now).
fn scan(api_tx: &ApiRequestSender, agents: &[AgentInfo], active: Option<&str>) {
    let settings = super::agent_settings::load();
    let now = now_ms();
    let busy_claude: HashSet<String> = super::claude_peers::load()
        .into_values()
        .filter(super::claude_peers::Peer::is_busy)
        .map(|peer| peer.session_id)
        .collect();
    let mut candidates = Vec::new();
    {
        let Some(mut shared) = shared() else { return };
        let requested = std::mem::take(&mut shared.now);
        let present: HashSet<&str> = agents.iter().map(|a| a.pane_id.as_str()).collect();
        shared
            .saved
            .idle
            .retain(|pane, _| present.contains(pane.as_str()));
        for info in agents {
            let Some(session) = info.agent_session.as_ref() else {
                shared.saved.idle.remove(&info.pane_id);
                continue;
            };
            let idle = matches!(info.agent_status, AgentStatus::Idle | AgentStatus::Done)
                && !info.launch_pending
                && !busy_claude.contains(&session.value);
            if !idle {
                shared.saved.idle.remove(&info.pane_id);
                if requested.contains(&info.pane_id) {
                    // Asked by hand: a working agent is stopped anyway.
                    candidates.push((info.clone(), true));
                }
                continue;
            }
            let since = match shared.saved.idle.get(&info.pane_id) {
                Some(entry) if entry.session == session.value => entry.since_ms,
                _ => {
                    shared.saved.idle.insert(
                        info.pane_id.clone(),
                        Idle {
                            session: session.value.clone(),
                            since_ms: now,
                        },
                    );
                    now
                }
            };
            if shared.saved.sleepers.contains_key(&info.pane_id) {
                continue;
            }
            if requested.contains(&info.pane_id) {
                candidates.push((info.clone(), true));
                continue;
            }
            let slug = super::agent_settings::space_slug(&info.workspace_id);
            let days = settings.effective(slug.as_deref()).hibernate_days;
            let looking = info.focused || active == Some(info.workspace_id.as_str());
            if days > 0 && !looking && now.saturating_sub(since) >= u64::from(days) * DAY_MS {
                candidates.push((info.clone(), false));
            }
        }
        save(&shared);
    }
    for (info, by_hand) in candidates {
        match hibernate(api_tx, &info, active) {
            Ok(()) => tracing::info!(pane = %info.pane_id, by_hand, "hivey: agent hibernated"),
            Err(err) => tracing::warn!(pane = %info.pane_id, %err, "hivey: cannot hibernate agent"),
        }
    }
}

fn hibernate(
    api_tx: &ApiRequestSender,
    info: &AgentInfo,
    active: Option<&str>,
) -> Result<(), String> {
    let agent = info.agent.clone().ok_or("no agent detected in the pane")?;
    let session = info
        .agent_session
        .as_ref()
        .ok_or("the agent has no session id")?;
    let process: PaneProcessInfo = dispatch(
        api_tx,
        Method::PaneProcessInfo(PaneProcessInfoParams {
            pane_id: Some(info.pane_id.clone()),
        }),
        Duration::from_secs(10),
    )
    .ok()
    .and_then(|result| result.get("process_info").cloned())
    .and_then(|value| serde_json::from_value(value).ok())
    .ok_or("cannot read the pane's processes")?;
    let pgid = process
        .foreground_process_group_id
        .ok_or("the pane has no foreground process")?;
    if process.shell_pid == Some(pgid) {
        return Err("the agent is not in the foreground".into());
    }
    let running = process
        .foreground_processes
        .iter()
        .find(|proc| proc.name == agent || program_name(proc.argv0.as_deref()) == Some(&agent))
        .ok_or_else(|| format!("no {agent} process in the pane"))?;
    let launch_argv = running.argv.clone().unwrap_or_default();
    let argv = resume_argv(
        &agent,
        &session.source,
        session.kind,
        &session.value,
        &launch_argv,
    )
    .ok_or_else(|| format!("{agent} sessions cannot be resumed"))?;
    crate::agent_resume::validate_resume_argv(&argv)?;
    {
        let Some(mut shared) = shared() else {
            return Err("hibernation is not running".into());
        };
        shared.saved.sleepers.insert(
            info.pane_id.clone(),
            Sleeper {
                pane_id: info.pane_id.clone(),
                workspace_id: info.workspace_id.clone(),
                name: info.name.clone(),
                agent,
                argv,
                at_ms: now_ms(),
                seen_away: active != Some(info.workspace_id.as_str()),
            },
        );
        shared.saved.idle.remove(&info.pane_id);
        shared.terminating.insert(pgid, Instant::now());
        save(&shared);
    }
    if !signal_group(pgid, Signal::Term) {
        if let Some(mut shared) = shared() {
            shared.saved.sleepers.remove(&info.pane_id);
            shared.terminating.remove(&pgid);
            save(&shared);
        }
        return Err(format!("cannot stop process group {pgid}"));
    }
    Ok(())
}

fn program_name(argv0: Option<&str>) -> Option<&str> {
    argv0.map(|arg| arg.rsplit('/').next().unwrap_or(arg))
}

/// Flags of the supported CLIs that take no value: a word after one of them is a prompt, not
/// the flag's value, and is dropped (resuming must not send the first prompt again).
const BOOLEAN_FLAGS: &[&str] = &[
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "--chrome",
    "--no-chrome",
    "--verbose",
    "--debug",
    "--ide",
    "--strict-mcp-config",
    "--fork-session",
    "--continue",
    "-c",
    "--dangerously-bypass-approvals-and-sandbox",
    "--no-daemon",
    "--full-auto",
    "--search",
    "--oss",
    "--always-approve",
    "--yolo",
];

/// The launch flags of a running agent without prompts, print mode or session flags.
fn launch_flags(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for arg in args {
        if matches!(arg.as_str(), "-p" | "--print") {
            continue;
        }
        if arg.starts_with('-') {
            out.push(arg.clone());
            continue;
        }
        let takes_value = out.last().is_some_and(|prev| {
            prev.starts_with('-') && !prev.contains('=') && !BOOLEAN_FLAGS.contains(&prev.as_str())
        });
        if takes_value {
            out.push(arg.clone());
        }
    }
    super::adapter::strip_session_args(&out)
}

/// The command that resumes `session` with the agent's launch flags.
fn resume_argv(
    agent: &str,
    source: &str,
    kind: crate::agent_resume::AgentSessionRefKind,
    session: &str,
    launch_argv: &[String],
) -> Option<Vec<String>> {
    if let Some(adapter) = super::adapter::AgentKind::parse(agent) {
        let flags = launch_flags(launch_argv.get(1..).unwrap_or_default());
        return adapter.resume_argv(session, &flags);
    }
    let session_ref = crate::agent_resume::AgentSessionRef {
        kind,
        value: session.to_string(),
    };
    crate::agent_resume::plan(source, agent, &session_ref).map(|plan| plan.argv)
}

/// The command as a shell line (resume arguments carry no apostrophes; see
/// `validate_resume_argv`).
fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| {
            let plain = !arg.is_empty()
                && arg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-./:=@,+%".contains(&b));
            if plain {
                arg.clone()
            } else {
                format!("'{arg}'")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn save(shared: &Shared) {
    let Ok(text) = serde_json::to_string_pretty(&shared.saved) else {
        return;
    };
    if let Some(parent) = shared.path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = shared.path.with_extension("json.tmp");
    if std::fs::write(&tmp, text)
        .and_then(|()| std::fs::rename(&tmp, &shared.path))
        .is_err()
    {
        tracing::warn!(path = %shared.path.display(), "hivey: cannot save hibernation state");
    }
}

#[derive(Clone, Copy)]
enum Signal {
    Term,
    Kill,
}

#[cfg(unix)]
fn signal_group(pgid: u32, signal: Signal) -> bool {
    let Ok(pgid) = libc::pid_t::try_from(pgid) else {
        return false;
    };
    if pgid <= 1 {
        return false;
    }
    let signal = match signal {
        Signal::Term => libc::SIGTERM,
        Signal::Kill => libc::SIGKILL,
    };
    // SAFETY: kill(2) with a negative pid signals that process group; no memory is shared.
    unsafe { libc::kill(-pgid, signal) == 0 }
}

#[cfg(unix)]
fn group_alive(pgid: u32) -> bool {
    let Ok(pgid) = libc::pid_t::try_from(pgid) else {
        return false;
    };
    // SAFETY: signal 0 only checks that the process group exists.
    pgid > 1 && unsafe { libc::kill(-pgid, 0) == 0 }
}

// Hibernation needs process groups; elsewhere nothing is stopped.
#[cfg(not(unix))]
fn signal_group(_pgid: u32, _signal: Signal) -> bool {
    false
}

#[cfg(not(unix))]
fn group_alive(_pgid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn launch_flags_keep_values_and_drop_prompts_and_sessions() {
        let launch = args(&[
            "--dangerously-skip-permissions",
            "fix the bug",
            "--model",
            "opus",
            "--add-dir",
            "/a",
            "--resume",
            "old",
            "-p",
        ]);
        assert_eq!(
            launch_flags(&launch),
            args(&[
                "--dangerously-skip-permissions",
                "--model",
                "opus",
                "--add-dir",
                "/a"
            ])
        );
        assert_eq!(launch_flags(&args(&["hello there"])), Vec::<String>::new());
    }

    #[test]
    fn claude_resumes_its_session_with_its_launch_flags() {
        let argv = resume_argv(
            "claude",
            "herdr:claude",
            crate::agent_resume::AgentSessionRefKind::Id,
            "abc-123",
            &args(&["claude", "--model", "opus", "start"]),
        );
        assert_eq!(
            argv,
            Some(args(&["claude", "--resume", "abc-123", "--model", "opus"]))
        );
    }

    #[test]
    fn shell_join_quotes_only_what_needs_it() {
        assert_eq!(
            shell_join(&args(&["claude", "--resume", "a-1", "--add-dir", "/x y"])),
            "claude --resume a-1 --add-dir '/x y'"
        );
    }

    #[test]
    fn program_name_is_the_last_path_part() {
        assert_eq!(program_name(Some("/opt/bin/codex")), Some("codex"));
        assert_eq!(program_name(Some("claude")), Some("claude"));
        assert_eq!(program_name(None), None);
    }
}
