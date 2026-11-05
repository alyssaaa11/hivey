//! Claude Code's own session registry and inbox sockets, used for Claude agents.
//!
//! Each running Claude Code session (v2.1.224+) writes `~/.claude/sessions/<pid>.json` with its
//! session id, a `busy`/`idle` status and the path of its inbox socket (cross-session
//! messaging). hivey reads it for two things:
//! - status: a session that reports `busy` is not typed into, even when the screen looks idle
//!   (the gap between two turns);
//! - delivery: an agent-to-agent digest is posted to the inbox socket instead of being typed
//!   into the pane. Claude Code reads it between tool calls and starts a turn when idle, and
//!   labels it as a teammate's request. Messages from the human are still typed, so they keep
//!   the user's authority.
//!
//! Claude Code holds socket messages for approval in a session that bypasses permission
//! prompts unless that session accepts them. Only managed settings, the `--settings` flag or
//! user settings can say `crossSessionInbound: accept` (project and local files may only make
//! it stricter), so hivey launches Claude agents with [`ACCEPT_SETTINGS`] and posts only to a
//! session whose process carries it, or when the user settings accept. Anything else (an
//! agent started before this, Codex/Grok/agy, Windows) keeps the typed delivery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The `--settings` value hivey adds to Claude agents so their inbox accepts hivey's posts.
pub(crate) const ACCEPT_SETTINGS: &str = r#"{"crossSessionInbound":"accept"}"#;

/// One entry of the registry, the fields hivey uses.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Peer {
    pub pid: u32,
    pub session_id: String,
    #[serde(default)]
    pub messaging_socket_path: Option<PathBuf>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub started_at: u64,
}

impl Peer {
    pub(crate) fn is_busy(&self) -> bool {
        self.status.as_deref() == Some("busy")
    }

    #[cfg(test)]
    pub(crate) fn test_new(session_id: &str) -> Self {
        Self {
            pid: 1,
            session_id: session_id.into(),
            messaging_socket_path: None,
            status: None,
            started_at: 0,
        }
    }
}

/// `$CLAUDE_CONFIG_DIR` or `~/.claude`.
fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|dir| !dir.is_empty()) {
        return PathBuf::from(dir);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".claude")
}

/// The registry's sessions by session id. When a session id appears twice (an old entry
/// left behind by a crashed process), the most recently started one wins.
pub(crate) fn load() -> HashMap<String, Peer> {
    load_from(&config_dir().join("sessions"))
}

fn load_from(dir: &Path) -> HashMap<String, Peer> {
    let mut peers: HashMap<String, Peer> = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return peers;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let Some(peer) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Peer>(&text).ok())
        else {
            continue;
        };
        let newer = peers
            .get(&peer.session_id)
            .is_none_or(|old| peer.started_at >= old.started_at);
        if newer {
            peers.insert(peer.session_id.clone(), peer);
        }
    }
    peers
}

/// Whether a session with this command line accepts socket posts without approval.
pub(crate) fn args_accept(command_line: &str) -> bool {
    let compact: String = command_line.split_whitespace().collect();
    compact.contains(r#""crossSessionInbound":"accept""#)
}

/// Whether the user settings make every session accept peer messages.
pub(crate) fn user_settings_accept() -> bool {
    std::fs::read_to_string(config_dir().join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|value| value["crossSessionInbound"] == "accept")
}

/// The command line of a running process, or None.
#[cfg(unix)]
pub(crate) fn command_line(pid: u32) -> Option<String> {
    let output = std::process::Command::new("ps")
        .args(["-ww", "-o", "args=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(not(unix))]
pub(crate) fn command_line(_pid: u32) -> Option<String> {
    None
}

/// The JSON line Claude Code's inbox reads: one user message.
pub(crate) fn wire_line(from: &str, text: &str) -> String {
    let line = serde_json::json!({
        "type": "user",
        "from": from,
        "from_name": from,
        "message": { "role": "user", "content": text },
    });
    format!("{line}\n")
}

/// Posts `text` to a session's inbox socket. The caller falls back to typing on error.
#[cfg(unix)]
pub(crate) fn post(socket: &Path, from: &str, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(wire_line(from, text).as_bytes())?;
    stream.flush()
}

#[cfg(not(unix))]
pub(crate) fn post(_socket: &Path, _from: &str, _text: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "inbox posts need an auth line on Windows",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hivey-peers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok();
        dir
    }

    #[test]
    fn registry_keeps_the_newest_entry_per_session() {
        let dir = temp_dir("registry");
        let write = |pid: u32, started: u64, status: &str| {
            let entry = serde_json::json!({
                "pid": pid, "sessionId": "s-1", "startedAt": started, "status": status,
                "messagingSocketPath": format!("/tmp/cc-socks/{pid}.sock"), "kind": "interactive",
            });
            std::fs::write(dir.join(format!("{pid}.json")), entry.to_string()).ok();
        };
        write(10, 100, "busy");
        write(20, 200, "idle");
        std::fs::write(dir.join("30.json"), "not json").ok();
        std::fs::write(dir.join("40.key"), "secret").ok();
        let peers = load_from(&dir);
        assert_eq!(peers.len(), 1);
        let peer = &peers["s-1"];
        assert_eq!(peer.pid, 20);
        assert!(!peer.is_busy());
        assert_eq!(
            peer.messaging_socket_path.as_deref(),
            Some(Path::new("/tmp/cc-socks/20.sock"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_an_accept_on_the_command_line_counts() {
        assert!(args_accept(
            r#"claude --dangerously-skip-permissions --settings {"crossSessionInbound":"accept"}"#
        ));
        assert!(args_accept(
            r#"claude --settings { "crossSessionInbound": "accept" }"#
        ));
        assert!(!args_accept(
            r#"claude --settings {"crossSessionInbound":"hold"}"#
        ));
        assert!(!args_accept("claude --model opus"));
    }

    #[test]
    fn wire_line_is_one_user_message() {
        let line = wire_line("demo/master", "hello\nworld");
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let value: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap_or_default();
        assert_eq!(value["type"], "user");
        assert_eq!(value["message"]["content"], "hello\nworld");
    }

    #[cfg(unix)]
    #[test]
    fn post_writes_the_line_to_the_socket() {
        use std::io::Read;
        let dir = temp_dir("post");
        let socket = dir.join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket);
        let Ok(listener) = listener else { return };
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.read_to_string(&mut text);
            }
            text
        });
        assert!(post(&socket, "demo/master", "ping").is_ok());
        let text = reader.join().unwrap_or_default();
        assert_eq!(text, wire_line("demo/master", "ping"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
