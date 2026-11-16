//! hivey: auto-compact windows. Claude Code compacts a conversation by itself when its
//! context fills its auto-compact window, which `/autocompact [auto|<tokens>]` sets (100k to
//! 1M). For spaces whose agent settings choose a window (hivey settings → autocompact, or a
//! space's own Settings…; off by default, see `agent_settings`), hivey types that command into
//! each of their Claude agents once, at the prompt: when the agent starts (new session or
//! process, a wake from hibernation included) and when the setting changes.
//!
//! A background thread checks every `TICK`. Off leaves agents alone (their own setting).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde_json::Value;

use super::agent_settings::AutoCompact;
use crate::api::schema::{
    AgentInfo, AgentStatus, EmptyParams, Method, PaneSendInputParams, Request,
};
use crate::api::ApiRequestSender;

const TICK: Duration = Duration::from_secs(10);

/// What was typed into a pane: for which agent run (session id, Claude pid) and window.
type Applied = ((String, u32), AutoCompact);

pub(crate) fn start(api_tx: ApiRequestSender) {
    let spawned = std::thread::Builder::new()
        .name("hivey-autocompact".into())
        .spawn(move || {
            let mut applied: HashMap<String, Applied> = HashMap::new();
            loop {
                std::thread::sleep(TICK);
                tick(&api_tx, &mut applied);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "hivey autocompact failed to start");
    }
}

fn dispatch(api_tx: &ApiRequestSender, method: Method) -> Result<Value, String> {
    let request = Request {
        id: "hivey:autocompact".into(),
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

fn tick(api_tx: &ApiRequestSender, applied: &mut HashMap<String, Applied>) {
    let settings = super::agent_settings::load();
    // Nothing to do while every space leaves its agents alone (the default).
    let any = settings.default.autocompact.unwrap_or_default() != AutoCompact::Off
        || settings.spaces.values().any(|layer| {
            layer
                .autocompact
                .is_some_and(|value| value != AutoCompact::Off)
        });
    if !any {
        applied.clear();
        return;
    }
    let Some(agents) = dispatch(api_tx, Method::AgentList(EmptyParams::default()))
        .ok()
        .and_then(|result| result.get("agents").cloned())
        .and_then(|agents| serde_json::from_value::<Vec<AgentInfo>>(agents).ok())
    else {
        return;
    };
    let peers = super::claude_peers::load();
    let mut seen = HashSet::new();
    for info in &agents {
        let Some(session) = info
            .agent_session
            .as_ref()
            .filter(|session| session.agent == "claude")
        else {
            continue;
        };
        seen.insert(info.pane_id.clone());
        let slug = super::agent_settings::space_slug(&info.workspace_id);
        let wanted = settings.effective(slug.as_deref()).autocompact;
        let Some(command) = wanted.command() else {
            applied.remove(&info.pane_id);
            continue;
        };
        let peer = peers.get(&session.value);
        let run = (session.value.clone(), peer.map_or(0, |peer| peer.pid));
        if applied.get(&info.pane_id) == Some(&(run.clone(), wanted)) {
            continue;
        }
        // Only at the prompt: never in the middle of a turn or of what the user is typing.
        let at_prompt = matches!(info.agent_status, AgentStatus::Idle | AgentStatus::Done)
            && !info.launch_pending
            && !peer.is_some_and(super::claude_peers::Peer::is_busy);
        if !at_prompt {
            continue;
        }
        let sent = dispatch(
            api_tx,
            Method::PaneSendInput(PaneSendInputParams {
                pane_id: info.pane_id.clone(),
                text: command.clone(),
                keys: vec!["Enter".into()],
            }),
        );
        match sent {
            Ok(_) => {
                tracing::info!(pane = %info.pane_id, %command, "hivey: autocompact window set");
                applied.insert(info.pane_id.clone(), (run, wanted));
            }
            Err(err) => tracing::warn!(pane = %info.pane_id, %err, "hivey: cannot set autocompact"),
        }
    }
    applied.retain(|pane, _| seen.contains(pane));
}
