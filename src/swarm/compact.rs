//! hivey: compacting by size. A Claude agent whose context has grown past its space's
//! `compact_at` tokens (hivey settings → compact, or a space's own Settings…; off by default,
//! see `agent_settings`) gets `/compact` typed in once it has been idle a minute, so long-lived
//! agents on 1M-token models compact at, say, 400k instead of running into the window.
//!
//! The context size is read from the end of the session's transcript
//! (`~/.claude/projects/*/<session>.jsonl`): the latest assistant turn's input + cache +
//! output tokens. A background thread checks every `TICK`; after compacting a session it waits
//! `COOLDOWN` before looking at it again.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::schema::{
    AgentInfo, AgentStatus, EmptyParams, Method, PaneSendInputParams, Request,
};
use crate::api::ApiRequestSender;

const TICK: Duration = Duration::from_secs(30);
/// An agent must be idle this long before `/compact` is typed in.
const IDLE_FOR: Duration = Duration::from_secs(60);
/// After compacting a session, leave it alone this long.
const COOLDOWN: Duration = Duration::from_secs(20 * 60);
/// Only the end of a transcript is read.
const TAIL_BYTES: u64 = 512 * 1024;

#[derive(Default)]
struct Compactor {
    /// Transcript path per session id.
    paths: HashMap<String, PathBuf>,
    /// When each pane was first seen idle (with which session).
    idle_since: HashMap<String, (String, Instant)>,
    /// When each session was last compacted.
    compacted: HashMap<String, Instant>,
}

pub(crate) fn start(api_tx: ApiRequestSender) {
    let spawned = std::thread::Builder::new()
        .name("hivey-compact".into())
        .spawn(move || {
            let mut compactor = Compactor::default();
            loop {
                std::thread::sleep(TICK);
                compactor.tick(&api_tx);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "hivey compacting failed to start");
    }
}

fn dispatch(api_tx: &ApiRequestSender, method: Method) -> Result<Value, String> {
    let request = Request {
        id: "hivey:compact".into(),
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

impl Compactor {
    fn tick(&mut self, api_tx: &ApiRequestSender) {
        let settings = super::agent_settings::load();
        // Nothing to do while compacting is off everywhere (the default).
        if settings.default.compact_at.unwrap_or(0) == 0
            && settings
                .spaces
                .values()
                .all(|layer| layer.compact_at.unwrap_or(0) == 0)
        {
            self.idle_since.clear();
            return;
        }
        let Some(agents) = dispatch(api_tx, Method::AgentList(EmptyParams::default()))
            .ok()
            .and_then(|result| result.get("agents").cloned())
            .and_then(|agents| serde_json::from_value::<Vec<AgentInfo>>(agents).ok())
        else {
            return;
        };
        let busy: std::collections::HashSet<String> = super::claude_peers::load()
            .into_values()
            .filter(super::claude_peers::Peer::is_busy)
            .map(|peer| peer.session_id)
            .collect();
        let now = Instant::now();
        self.compacted.retain(|_, at| at.elapsed() < COOLDOWN);
        let mut seen = std::collections::HashSet::new();
        for info in &agents {
            seen.insert(info.pane_id.clone());
            let Some(session) = info
                .agent_session
                .as_ref()
                .filter(|session| session.agent == "claude")
            else {
                continue;
            };
            let idle = matches!(info.agent_status, AgentStatus::Idle | AgentStatus::Done)
                && !info.launch_pending
                && !busy.contains(&session.value);
            if !idle {
                self.idle_since.remove(&info.pane_id);
                continue;
            }
            let since = match self.idle_since.get(&info.pane_id) {
                Some((id, since)) if *id == session.value => *since,
                _ => {
                    self.idle_since
                        .insert(info.pane_id.clone(), (session.value.clone(), now));
                    now
                }
            };
            if now.duration_since(since) < IDLE_FOR || self.compacted.contains_key(&session.value) {
                continue;
            }
            let slug = super::agent_settings::space_slug(&info.workspace_id);
            let limit = settings.effective(slug.as_deref()).compact_at;
            if limit == 0 {
                continue;
            }
            let Some(tokens) = self.context_tokens(&session.value) else {
                continue;
            };
            if tokens < limit {
                continue;
            }
            tracing::info!(pane = %info.pane_id, tokens, limit, "hivey: compacting agent");
            let sent = dispatch(
                api_tx,
                Method::PaneSendInput(PaneSendInputParams {
                    pane_id: info.pane_id.clone(),
                    text: "/compact".into(),
                    keys: vec!["Enter".into()],
                }),
            );
            match sent {
                Ok(_) => {
                    self.compacted.insert(session.value.clone(), now);
                    self.idle_since.remove(&info.pane_id);
                }
                Err(err) => {
                    tracing::warn!(pane = %info.pane_id, %err, "hivey: cannot compact agent")
                }
            }
        }
        self.idle_since.retain(|pane, _| seen.contains(pane));
    }

    /// The session's current context size, from its transcript.
    fn context_tokens(&mut self, session: &str) -> Option<u64> {
        let path = match self.paths.get(session) {
            Some(path) if path.exists() => path.clone(),
            _ => {
                let path = find_transcript(session)?;
                self.paths.insert(session.to_string(), path.clone());
                path
            }
        };
        last_context_tokens(&read_tail(&path)?)
    }
}

fn find_transcript(session: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let projects = Path::new(&home).join(".claude").join("projects");
    let file = format!("{session}.jsonl");
    std::fs::read_dir(projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(&file))
        .find(|path| path.is_file())
}

fn read_tail(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The latest assistant turn's context size (input + cache + output tokens).
fn last_context_tokens(tail: &str) -> Option<u64> {
    tail.lines().rev().find_map(|line| {
        if !line.contains("\"usage\"") {
            return None;
        }
        let entry: Value = serde_json::from_str(line).ok()?;
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            return None;
        }
        let usage = entry.get("message")?.get("usage")?;
        let field = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
        let total = field("input_tokens")
            + field("cache_creation_input_tokens")
            + field("cache_read_input_tokens")
            + field("output_tokens");
        (total > 0).then_some(total)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_latest_assistant_usage() {
        let tail = [
            r#"{"type":"assistant","message":{"usage":{"input_tokens":10,"cache_read_input_tokens":100,"output_tokens":5}}}"#,
            r#"{"type":"user","message":{"content":"hi"}}"#,
            r#"{"type":"assistant","message":{"usage":{"input_tokens":3,"cache_creation_input_tokens":1000,"cache_read_input_tokens":399000,"output_tokens":7}}}"#,
            r#"{"type":"system","subtype":"x"}"#,
            "{\"partial",
        ]
        .join("\n");
        assert_eq!(last_context_tokens(&tail), Some(400_010));
        assert_eq!(last_context_tokens("{\"type\":\"user\"}"), None);
    }
}
