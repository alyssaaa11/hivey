//! The task log: one shared, append-only file where agents say which task they start and when
//! they finish, so the agent fleet's Tasks board reads it instead of guessing from terminals.
//!
//! `~/.hivey/tasks.jsonl`, one JSON object per line:
//!
//! ```json
//! {"ts":"2026-11-24T10:00:00Z","event":"title","title":"Fix failing nightly jobs",
//!  "pane":"w1-2","hv_session":"","session":"<claude session id>","cwd":"/Users/me/agents/x","by":"agent"}
//! ```
//!
//! Events:
//! - `title`: the agent named the task it is starting (`hivey task start "<title>"`).
//! - `done`: the agent says the task is finished (`hivey task done ["<note>"]`).
//! - `prompt` / `stop`: written by the Claude Code hook `hivey-task-log.py` on every prompt and
//!   at the end of every turn (`by: "hook"`), so start and finish times are there even when an
//!   agent forgets to name its task. `prompt` carries `from`: `you`, `bus` or `other`.
//!
//! Lines are appended with one `write` on a file opened in append mode, so several agents can
//! write at once. The file is trimmed to its newest half when it passes `MAX_BYTES`.

use serde_json::{json, Map, Value};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The file is trimmed to its newest half past this size (about two weeks of busy use).
const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// A title or note longer than this is cut.
const MAX_TEXT: usize = 120;

pub(crate) fn log_path() -> Option<PathBuf> {
    crate::swarm::home::hivey_dir().map(|dir| dir.join("tasks.jsonl"))
}

/// Who is writing: the hivey pane, the hivey session and the Claude Code session, read from the
/// environment hivey and Claude Code give every agent.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Writer {
    pub pane: String,
    pub hv_session: String,
    pub session: String,
    pub cwd: String,
}

impl Writer {
    pub(crate) fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        Self {
            pane: var("HERDR_PANE_ID"),
            hv_session: var(crate::session::SESSION_ENV_VAR),
            session: var("CLAUDE_CODE_SESSION_ID"),
            cwd: std::env::current_dir()
                .map(|dir| dir.display().to_string())
                .unwrap_or_default(),
        }
    }
}

/// One line of the log, without its trailing newline.
pub(crate) fn entry(ts: &str, event: &str, text: &str, writer: &Writer) -> String {
    let mut map = Map::new();
    map.insert("ts".into(), json!(ts));
    map.insert("event".into(), json!(event));
    let text = clean(text);
    if !text.is_empty() {
        let key = if event == "title" { "title" } else { "note" };
        map.insert(key.into(), json!(text));
    }
    for (key, value) in [
        ("pane", &writer.pane),
        ("hv_session", &writer.hv_session),
        ("session", &writer.session),
        ("cwd", &writer.cwd),
    ] {
        if !value.is_empty() {
            map.insert(key.into(), json!(value));
        }
    }
    map.insert("by".into(), json!("agent"));
    Value::Object(map).to_string()
}

/// One line of text, at most `MAX_TEXT` characters.
fn clean(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let one_line = one_line.trim_matches(|c| c == '"' || c == '\'' || c == '`');
    if one_line.chars().count() <= MAX_TEXT {
        return one_line.to_string();
    }
    let cut: String = one_line.chars().take(MAX_TEXT - 1).collect();
    format!("{}…", cut.trim_end())
}

pub(crate) fn now_iso() -> String {
    let now = time::OffsetDateTime::now_utc();
    let now = now.replace_nanosecond(0).unwrap_or(now);
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Appends one line to the log at `path`, trimming the file first when it is too big.
pub(crate) fn append(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        trim(path)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(format!("{line}\n").as_bytes())
}

/// Keeps the newest half of the file (whole lines), swapped in with a rename.
fn trim(path: &Path) -> io::Result<()> {
    let text = std::fs::read_to_string(path)?;
    let half = text.len() / 2;
    let start = text[half..].find('\n').map_or(text.len(), |i| half + i + 1);
    let tmp = path.with_extension("jsonl.tmp");
    std::fs::write(&tmp, &text[start..])?;
    std::fs::rename(tmp, path)
}

/// The last `limit` entries of the log at `path`, oldest first; unreadable lines are skipped.
pub(crate) fn recent(path: &Path, limit: usize) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out: Vec<Value> = text
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(Value::is_object)
        .take(limit)
        .collect();
    out.reverse();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer() -> Writer {
        Writer {
            pane: "w1-2".into(),
            hv_session: String::new(),
            session: "abc".into(),
            cwd: "/x".into(),
        }
    }

    #[test]
    fn a_title_line_names_the_task_and_who_wrote_it() {
        let line = entry(
            "2026-11-24T10:00:00Z",
            "title",
            "  \"Fix failing\n nightly jobs\" ",
            &writer(),
        );
        let v: Value = serde_json::from_str(&line).expect("json");
        assert_eq!(v["title"], "Fix failing nightly jobs");
        assert_eq!(v["event"], "title");
        assert_eq!(v["pane"], "w1-2");
        assert_eq!(v["session"], "abc");
        assert_eq!(v["by"], "agent");
        assert!(v.get("hv_session").is_none());
        assert!(v.get("note").is_none());
    }

    #[test]
    fn a_done_line_keeps_its_note_and_long_text_is_cut() {
        let long = "word ".repeat(60);
        let v: Value = serde_json::from_str(&entry("t", "done", &long, &writer())).expect("json");
        let note = v["note"].as_str().expect("note");
        assert!(note.ends_with('…'));
        assert!(note.chars().count() <= MAX_TEXT);
        assert!(v.get("title").is_none());
    }

    #[test]
    fn appends_reads_back_and_trims_to_the_newest_half() {
        let dir = std::env::temp_dir().join(format!("hivey-tasks-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tasks.jsonl");
        for i in 0..5 {
            append(&path, &entry("t", "title", &format!("task {i}"), &writer())).expect("append");
        }
        let got = recent(&path, 3);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0]["title"], "task 2");
        assert_eq!(got[2]["title"], "task 4");
        trim(&path).expect("trim");
        let kept = recent(&path, 100);
        assert!(kept.len() < 5 && !kept.is_empty());
        assert_eq!(kept.last().expect("last")["title"], "task 4");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn now_is_rfc3339_to_the_second() {
        let now = now_iso();
        assert!(now.ends_with('Z') && !now.contains('.'), "{now}");
    }
}
