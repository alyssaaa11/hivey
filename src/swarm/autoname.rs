//! Automatic pane names: every agent pane gets a name of at most three words for the
//! job it's doing now, which the pane border shows first.
//!
//! A background thread (off the render path) lists agents every `TICK`, reads the tail of
//! each agent's own transcript (Claude Code `~/.claude/projects/*/<session>.jsonl`, Codex
//! `~/.codex/sessions/**/rollout-*<session>.jsonl`) and, when the latest request changed,
//! asks a small model (`claude -p --model haiku`) to name it. The name is reported as the
//! pane metadata token `job` (source `hivey:autoname`), so every client sees it.
//! Names are cached per agent session in `<session data dir>/pane-names.json`, so a
//! restart doesn't name everything again. `HIVEY_AUTO_PANE_NAMES=0` turns it off.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::api::schema::{AgentInfo, EmptyParams, Method, PaneReportMetadataParams, Request};
use crate::api::ApiRequestSender;

const TICK: Duration = Duration::from_secs(15);
/// Least time between two names for the same pane.
const MIN_RENAME_GAP: Duration = Duration::from_secs(45);
/// Wait after a failed naming before trying that pane again.
const RETRY_AFTER: Duration = Duration::from_secs(300);
const MODEL_TIMEOUT: Duration = Duration::from_secs(60);
/// Only the end of a transcript is read.
const TAIL_BYTES: u64 = 512 * 1024;
const SOURCE: &str = "hivey:autoname";
pub(crate) const TOKEN: &str = "job";
const MAX_WORDS: usize = 3;
const MAX_CHARS: usize = 32;

pub(crate) fn start(api_tx: ApiRequestSender) {
    if std::env::var("HIVEY_AUTO_PANE_NAMES").is_ok_and(|v| v == "0" || v == "false") {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("hivey-autoname".into())
        .spawn(move || {
            let mut namer = Namer::load(crate::session::data_dir().join("pane-names.json"));
            loop {
                std::thread::sleep(TICK);
                namer.tick(&api_tx);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "hivey pane namer failed to start");
    }
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct Named {
    /// The latest request the name was made from.
    fingerprint: String,
    name: String,
}

#[derive(Default)]
struct Namer {
    cache_path: PathBuf,
    /// Name per agent session id.
    names: HashMap<String, Named>,
    /// Transcript path per agent session id.
    paths: HashMap<String, PathBuf>,
    /// Transcript size + mtime last read per session, to skip unchanged ones.
    seen: HashMap<String, (u64, Option<SystemTime>)>,
    last_try: HashMap<String, Instant>,
    failed: HashMap<String, Instant>,
}

impl Namer {
    fn load(cache_path: PathBuf) -> Self {
        let mut names: HashMap<String, Named> = std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        // Names cached by an older build may break today's rules (e.g. hyphenated).
        names.retain(|_, named| match clean_name(&named.name) {
            Some(name) => {
                named.name = name;
                true
            }
            None => false,
        });
        Self {
            cache_path,
            names,
            ..Self::default()
        }
    }

    fn save(&self) {
        if let Ok(raw) = serde_json::to_string(&self.names) {
            let tmp = self.cache_path.with_extension("json.tmp");
            if std::fs::write(&tmp, raw).is_ok() {
                let _ = std::fs::rename(&tmp, &self.cache_path);
            }
        }
    }

    fn tick(&mut self, api_tx: &ApiRequestSender) {
        let Some(agents) = dispatch(api_tx, Method::AgentList(EmptyParams::default()))
            .ok()
            .and_then(|result| result.get("agents").cloned())
            .and_then(|agents| serde_json::from_value::<Vec<AgentInfo>>(agents).ok())
        else {
            return;
        };
        let mut changed = false;
        for agent in agents {
            let Some(session) = agent.agent_session.as_ref() else {
                continue;
            };
            let kind = session.agent.as_str();
            if kind != "claude" && kind != "codex" {
                continue;
            }
            let id = session.value.clone();
            if let Some(named) = self.names.get(&id) {
                if agent.tokens.get(TOKEN) != Some(&named.name) {
                    report(api_tx, &agent.pane_id, &named.name);
                }
            }
            let Some(path) = self.transcript(kind, &id) else {
                continue;
            };
            let stamp = std::fs::metadata(&path)
                .map(|meta| (meta.len(), meta.modified().ok()))
                .unwrap_or_default();
            if self.seen.get(&id) == Some(&stamp) {
                continue;
            }
            if self
                .last_try
                .get(&id)
                .is_some_and(|at| at.elapsed() < MIN_RENAME_GAP)
                || self
                    .failed
                    .get(&id)
                    .is_some_and(|at| at.elapsed() < RETRY_AFTER)
            {
                continue;
            }
            self.seen.insert(id.clone(), stamp);
            let recent = read_recent(&path);
            let Some(latest) = recent.prompts.last().cloned() else {
                continue;
            };
            if self.names.get(&id).is_some_and(|n| n.fingerprint == latest) {
                continue;
            }
            self.last_try.insert(id.clone(), Instant::now());
            match ask_model(&recent).as_deref().and_then(clean_name) {
                Some(name) => {
                    self.failed.remove(&id);
                    report(api_tx, &agent.pane_id, &name);
                    self.names.insert(
                        id,
                        Named {
                            fingerprint: latest,
                            name,
                        },
                    );
                    changed = true;
                }
                None => {
                    self.failed.insert(id, Instant::now());
                }
            }
        }
        if changed {
            self.save();
        }
    }

    fn transcript(&mut self, kind: &str, id: &str) -> Option<PathBuf> {
        if let Some(path) = self.paths.get(id).filter(|path| path.is_file()) {
            return Some(path.clone());
        }
        // Agent ids are file names: never let one walk out of the transcript folders.
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let found = match kind {
            "claude" => find_claude_transcript(&home.join(".claude/projects"), id),
            _ => find_suffix(&home.join(".codex/sessions"), &format!("{id}.jsonl"), 4),
        }?;
        self.paths.insert(id.to_string(), found.clone());
        Some(found)
    }
}

fn find_claude_transcript(projects: &Path, id: &str) -> Option<PathBuf> {
    let file = format!("{id}.jsonl");
    std::fs::read_dir(projects)
        .ok()?
        .flatten()
        .map(|entry| entry.path().join(&file))
        .find(|path| path.is_file())
}

fn find_suffix(dir: &Path, suffix: &str, depth: usize) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth > 0 {
                if let Some(found) = find_suffix(&path, suffix, depth - 1) {
                    return Some(found);
                }
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(suffix))
        {
            return Some(path);
        }
    }
    None
}

#[derive(Debug, Default, PartialEq)]
struct Recent {
    /// The user's latest requests, oldest first (at most 3).
    prompts: Vec<String>,
    /// The agent's latest reply text.
    reply: Option<String>,
}

fn read_recent(path: &Path) -> Recent {
    let Ok(mut file) = std::fs::File::open(path) else {
        return Recent::default();
    };
    let len = file.metadata().map(|meta| meta.len()).unwrap_or_default();
    let start = len.saturating_sub(TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return Recent::default();
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return Recent::default();
    }
    let text = String::from_utf8_lossy(&bytes);
    // The first line may be cut by the seek.
    let skip = usize::from(start > 0);
    parse_recent(text.lines().skip(skip))
}

/// Pulls the user's real requests and the agent's last reply out of transcript lines
/// (Claude Code: `{"type":"user","message":{...}}`; Codex: `{"payload":{"role":...}}`).
fn parse_recent<'a>(lines: impl Iterator<Item = &'a str>) -> Recent {
    let mut recent = Recent::default();
    for line in lines {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("isMeta").and_then(Value::as_bool) == Some(true)
            || value.get("isSidechain").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let Some(message) = value.get("message").or_else(|| value.get("payload")) else {
            continue;
        };
        let Some(text) = message_text(message.get("content")) else {
            continue;
        };
        match message.get("role").and_then(Value::as_str) {
            Some("user") if is_request(&text) => {
                recent.prompts.push(clip(&text, 400));
                if recent.prompts.len() > 3 {
                    recent.prompts.remove(0);
                }
            }
            Some("assistant") => recent.reply = Some(clip(&text, 400)),
            _ => {}
        }
    }
    recent
}

fn message_text(content: Option<&Value>) -> Option<String> {
    let text = match content? {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("text" | "input_text" | "output_text")
                )
            })
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Leaves out what the harness injects as a "user" turn (commands, reminders, Codex's
/// AGENTS.md and environment context).
fn is_request(text: &str) -> bool {
    !(text.starts_with('<')
        || text.starts_with("# AGENTS.md")
        || text.starts_with("Caveat:")
        || text.starts_with("[Request interrupted"))
}

fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(max).collect()
}

fn ask_model(recent: &Recent) -> Option<String> {
    let mut prompt = String::from(
        "Name a terminal pane after the job its coding agent is doing now, in at most 3 words \
         (e.g. \"auto pane names\", \"fix login bug\", \"pharma market research\"). Base it on \
         the latest request, using the earlier ones and the reply only as context. Reply with \
         only the name: lowercase words separated by spaces, no hyphens, quotes or punctuation.\n\n",
    );
    for (i, request) in recent.prompts.iter().enumerate() {
        let tag = if i + 1 == recent.prompts.len() {
            "Latest request"
        } else {
            "Earlier request"
        };
        prompt.push_str(&format!("{tag}: {request}\n"));
    }
    if let Some(reply) = &recent.reply {
        prompt.push_str(&format!("Agent's last reply: {reply}\n"));
    }
    let mut child = Command::new(claude_bin())
        .args([
            "-p",
            "--model",
            "haiku",
            "--no-session-persistence",
            "--tools",
            "",
            "--strict-mcp-config",
            "--settings",
            r#"{"disableAllHooks":true}"#,
        ])
        // Not a pane's agent: keep hivey's pane hooks and swarm identity out of it.
        .env_remove("HERDR_ENV")
        .env_remove("HIVEY_ENV")
        .env_remove("HERDR_PANE_ID")
        .env_remove("HERDR_SOCKET_PATH")
        .env_remove("HERDR_CLIENT_SOCKET_PATH")
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| tracing::debug!(%err, "hivey pane namer: claude not started"))
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(prompt.as_bytes());
    }
    let deadline = Instant::now() + MODEL_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out)
}

fn claude_bin() -> PathBuf {
    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join("claude"))
            .find(|path| path.is_file())
    });
    on_path
        .or_else(|| {
            let home = PathBuf::from(std::env::var_os("HOME")?);
            [".bun/bin", ".local/bin", ".claude/local"]
                .iter()
                .map(|dir| home.join(dir).join("claude"))
                .chain([PathBuf::from("/opt/homebrew/bin/claude")])
                .find(|path| path.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("claude"))
}

/// The model's last non-empty line, trimmed to three plain words.
fn clean_name(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).rfind(|line| !line.is_empty())?;
    let words = line
        // Hyphenated or snake_case answers (`fix-login-bug`) still count word by word.
        .split(|c: char| c.is_whitespace() || c == '-' || c == '_')
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .take(MAX_WORDS)
        .collect::<Vec<_>>();
    let name: String = words.join(" ").chars().take(MAX_CHARS).collect();
    let name = name.trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// Pane border text: the auto name first, then the pane's own label (swarm title, manual
/// name or agent), so the current job stays visible when the border is narrow.
pub(crate) fn border_label(job: Option<&str>, label: Option<String>) -> Option<String> {
    match (job, label) {
        (Some(job), Some(label)) => Some(format!("{job} · {label}")),
        (Some(job), None) => Some(job.to_string()),
        (None, label) => label,
    }
}

fn report(api_tx: &ApiRequestSender, pane_id: &str, name: &str) {
    let mut tokens = HashMap::new();
    tokens.insert(TOKEN.to_string(), Some(name.to_string()));
    let params = PaneReportMetadataParams {
        pane_id: pane_id.to_string(),
        source: SOURCE.into(),
        agent: None,
        applies_to_source: None,
        title: None,
        display_agent: None,
        state_labels: HashMap::new(),
        tokens,
        clear_title: false,
        clear_display_agent: false,
        clear_state_labels: false,
        seq: None,
        ttl_ms: None,
    };
    if let Err(err) = dispatch(api_tx, Method::PaneReportMetadata(params)) {
        tracing::debug!(%pane_id, %err, "hivey pane namer: name not reported");
    }
}

fn dispatch(api_tx: &ApiRequestSender, method: Method) -> Result<Value, String> {
    let request = Request {
        id: SOURCE.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn border_label_puts_the_job_first() {
        assert_eq!(
            border_label(Some("fix login"), Some("★ web · agent".into())).as_deref(),
            Some("fix login · ★ web · agent")
        );
        assert_eq!(
            border_label(Some("fix login"), None).as_deref(),
            Some("fix login")
        );
        assert_eq!(border_label(None, Some("x".into())).as_deref(), Some("x"));
        assert_eq!(border_label(None, None), None);
    }

    #[test]
    fn clean_name_keeps_three_plain_words() {
        assert_eq!(
            clean_name("Sure!\n\"Auto-Name Hivey Panes Now.\"\n").as_deref(),
            Some("auto name hivey")
        );
        assert_eq!(clean_name("  \n "), None);
        assert_eq!(clean_name("“...”"), None);
    }

    #[test]
    fn parse_recent_reads_claude_and_codex_turns() {
        let lines = [
            r#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"user","message":{"role":"user","content":"fix the login bug"}}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta"}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"x"}]}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Fixed it."}]}}"#,
            r##"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions"}]}}"##,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"now add tests"}]}}"#,
            "not json",
        ];
        let recent = parse_recent(lines.into_iter());
        assert_eq!(recent.prompts, vec!["fix the login bug", "now add tests"]);
        assert_eq!(recent.reply.as_deref(), Some("Fixed it."));
    }

    #[test]
    fn parse_recent_keeps_the_last_three_requests() {
        let lines: Vec<String> = (1..=5)
            .map(|i| format!(r#"{{"message":{{"role":"user","content":"task {i}"}}}}"#))
            .collect();
        let recent = parse_recent(lines.iter().map(String::as_str));
        assert_eq!(recent.prompts, vec!["task 3", "task 4", "task 5"]);
    }
}
