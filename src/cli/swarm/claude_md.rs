//! `hivey claude-md`: the size of agents' CLAUDE.md files (Claude Code warns above 40k
//! characters) and their clean-up: Opus rewrites the file shorter, keeping every rule, path,
//! check and the current status, the original is backed up to `~/.hivey/old/claude-md/`, and
//! the agent exits and resumes its conversation in its own pane (`hivey hibernate restart`) so
//! it loads the new file. The agent fleet page warns about large ones and has a button for it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Claude Code's warning threshold for a CLAUDE.md, in characters.
pub(crate) const LIMIT: usize = 40_000;
/// What a clean-up aims for, leaving room to grow.
const TARGET: usize = 30_000;
/// A rewrite shorter than this share of the original lost too much and is refused.
const MIN_SHARE: f64 = 0.15;
const MODEL_TIMEOUT: Duration = Duration::from_secs(20 * 60);
/// How long to wait for a busy agent to be idle before restarting it.
const IDLE_WAIT: Duration = Duration::from_secs(30 * 60);

const HELP: &str = "\
usage: hivey claude-md                   each Claude agent's CLAUDE.md size (⚠ over the 40k limit)
       hivey claude-md clean <pane> [--model opus] [--force]
                                         rewrite that agent's CLAUDE.md to about 30k characters
                                         with Opus (rules, paths, checks and status kept; the
                                         original backed up to ~/.hivey/old/claude-md/), then
                                         exit the agent and resume its conversation in its pane
                                         so it loads the new file. --force: even under the limit
  The agent fleet page shows a ⚠ on agents whose CLAUDE.md is over the limit, with a button
  that runs the clean-up.";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    if super::wants_help(args) {
        println!("{HELP}");
        return Ok(0);
    }
    match args.first().map(String::as_str) {
        None | Some("list") => list(),
        Some("clean") => {
            let mut rest = args[1..].to_vec();
            let force = super::take_flag(&mut rest, "--force");
            let model = match super::take_value(&mut rest, "--model") {
                Ok(model) => model.unwrap_or_else(|| "opus".into()),
                Err(message) => {
                    eprintln!("{message}\n{HELP}");
                    return Ok(2);
                }
            };
            let [pane] = rest.as_slice() else {
                eprintln!("{HELP}");
                return Ok(2);
            };
            match clean(&crate::cli::normalize_pane_id(pane), &model, force) {
                Ok(()) => Ok(0),
                Err(message) => {
                    eprintln!("error: {message}");
                    Ok(1)
                }
            }
        }
        _ => {
            eprintln!("{HELP}");
            Ok(2)
        }
    }
}

fn agents() -> Result<Vec<Value>, String> {
    Ok(super::api("agent.list", json!({}))?["agents"]
        .as_array()
        .cloned()
        .unwrap_or_default())
}

fn folder_of(agent: &Value) -> Option<PathBuf> {
    agent["foreground_cwd"]
        .as_str()
        .or_else(|| agent["cwd"].as_str())
        .map(PathBuf::from)
}

/// Characters as Claude Code counts them (JavaScript string length: UTF-16 units).
pub(crate) fn size(text: &str) -> usize {
    text.encode_utf16().count()
}

fn chars_k(chars: usize) -> String {
    format!("{:.1}k", chars as f64 / 1000.0)
}

fn list() -> std::io::Result<i32> {
    let agents = match agents() {
        Ok(agents) => agents,
        Err(message) => {
            eprintln!("error: {message}");
            return Ok(1);
        }
    };
    let mut rows = Vec::new();
    for agent in agents.iter().filter(|a| a["agent"] == "claude") {
        let Some(path) = folder_of(agent).map(|folder| folder.join("CLAUDE.md")) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        rows.push((
            size(&text),
            agent["pane_id"].as_str().unwrap_or("?").to_string(),
            agent["name"].as_str().unwrap_or("").to_string(),
            path,
        ));
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.0));
    if rows.is_empty() {
        println!("no Claude agent has a CLAUDE.md in its folder");
    }
    for (chars, pane, name, path) in rows {
        let mark = if chars > LIMIT { "⚠" } else { " " };
        println!(
            "{mark} {:>7}  {pane:<8} {name:<28} {}",
            chars_k(chars),
            path.display()
        );
    }
    println!(
        "limit {} · clean one: hivey claude-md clean <pane>",
        chars_k(LIMIT)
    );
    Ok(0)
}

fn agent_of(pane: &str) -> Result<Value, String> {
    agents()?
        .into_iter()
        .find(|agent| agent["pane_id"] == pane)
        .ok_or_else(|| format!("no agent in {pane}"))
}

fn clean(pane: &str, model: &str, force: bool) -> Result<(), String> {
    let agent = agent_of(pane)?;
    if agent["agent"] != "claude" {
        return Err(format!("{pane} is not a Claude agent"));
    }
    let path = folder_of(&agent)
        .map(|folder| folder.join("CLAUDE.md"))
        .ok_or_else(|| format!("no folder for {pane}"))?;
    let original =
        std::fs::read_to_string(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    let chars = size(&original);
    if chars <= TARGET && !force {
        println!(
            "{} is {} characters, already under {} (--force to clean it anyway)",
            path.display(),
            chars_k(chars),
            chars_k(TARGET)
        );
        return Ok(());
    }
    println!(
        "{}: {} characters; {model} is rewriting it to about {} (a few minutes)…",
        path.display(),
        chars_k(chars),
        chars_k(TARGET)
    );
    let reply = ask_model(model, &prompt(&original))?;
    let cleaned = cleaned_reply(&reply, chars)?;
    let backup = backup(&path, &original)?;
    std::fs::write(&path, &cleaned).map_err(|err| format!("{}: {err}", path.display()))?;
    println!(
        "CLAUDE.md: {} → {} characters (original: {})",
        chars_k(chars),
        chars_k(size(&cleaned)),
        backup.display()
    );
    restart_when_idle(pane)
}

fn prompt(original: &str) -> String {
    format!(
        "Below is the CLAUDE.md of a long-running coding agent: its standing instructions, \
         loaded at the start of every session. It has {chars} characters and Claude Code warns \
         above {LIMIT}. Rewrite it to at most {TARGET} characters.\n\n\
         Keep, word for word where it matters: every rule, constraint and prohibition; every \
         file path, URL, command, tool, key name and environment variable; the mission, \
         deliverables and their checks; who to report to and how; and the current status \
         (latest state, open problems, next steps).\n\
         Shorten: repeated or overlapping rules (merge them), long explanations and examples \
         that restate a rule, and history (logs of finished work, old dated status entries): \
         keep at most a few lines summing up what was done and decided.\n\
         Keep the Markdown structure and the headings' order. Do not invent, reword names or \
         change meanings. Answer with the new file only: no preamble, no code fence.\n\n\
         <claude_md>\n{original}\n</claude_md>\n",
        chars = size(original)
    )
}

/// The rewritten file from the model's reply, refused when empty, still too long, or so short
/// that content must have been lost.
fn cleaned_reply(reply: &str, original_chars: usize) -> Result<String, String> {
    let mut text = reply.trim();
    if let Some(rest) = text.strip_prefix("```") {
        // A fenced reply: drop the fence line (with its language) and the closing fence.
        text = rest.split_once('\n').map_or("", |(_, body)| body);
        text = text.trim_end().strip_suffix("```").unwrap_or(text).trim();
    }
    let chars = size(text);
    if chars == 0 {
        return Err("the model answered nothing; CLAUDE.md is unchanged".into());
    }
    if chars > LIMIT - 1_000 {
        return Err(format!(
            "the rewrite is still {} characters; CLAUDE.md is unchanged",
            chars_k(chars)
        ));
    }
    if (chars as f64) < original_chars as f64 * MIN_SHARE {
        return Err(format!(
            "the rewrite is only {} characters, too short to keep everything; CLAUDE.md is \
             unchanged",
            chars_k(chars)
        ));
    }
    Ok(format!("{text}\n"))
}

fn ask_model(model: &str, prompt: &str) -> Result<String, String> {
    let mut child = Command::new(crate::swarm::autoname::claude_bin())
        .args([
            "-p",
            "--model",
            model,
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
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("cannot start claude: {err}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|err| format!("cannot send the file to claude: {err}"))?;
    }
    // Read while it runs: a long reply would fill the pipe and stall it.
    let mut stdout = child.stdout.take().ok_or("no output from claude")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = stdout.read_to_string(&mut out);
        out
    });
    let deadline = Instant::now() + MODEL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("claude took too long; CLAUDE.md is unchanged".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(500)),
            Err(err) => return Err(err.to_string()),
        }
    };
    let out = reader.join().unwrap_or_default();
    if !status.success() {
        let mut err = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err);
        }
        let detail = if err.trim().is_empty() {
            out.trim()
        } else {
            err.trim()
        };
        return Err(format!(
            "claude failed: {}",
            detail.chars().take(300).collect::<String>()
        ));
    }
    Ok(out)
}

fn backup(path: &Path, original: &str) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    let dir = Path::new(&home).join(".hivey/old/claude-md");
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let folder = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("agent");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let file = dir.join(format!("{folder}-{stamp}-CLAUDE.md"));
    std::fs::write(&file, original).map_err(|err| format!("{}: {err}", file.display()))?;
    Ok(file)
}

/// Exits the agent and resumes it in its pane once it is not in the middle of a turn.
fn restart_when_idle(pane: &str) -> Result<(), String> {
    let deadline = Instant::now() + IDLE_WAIT;
    loop {
        let status = agent_of(pane)?["agent_status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        if matches!(status.as_str(), "idle" | "done") {
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "{pane} stayed {status}; restart it yourself to load the new CLAUDE.md \
                 (hivey hibernate restart {pane})"
            ));
        }
        println!("{pane} is {status}; waiting for it to be idle before restarting…");
        std::thread::sleep(Duration::from_secs(15));
    }
    let response =
        super::call("hibernate.restart", json!({ "pane": pane })).map_err(|err| err.to_string())?;
    if super::result(&response).is_none() {
        return Err(format!(
            "CLAUDE.md is cleaned but {pane} was not restarted (hivey hibernate restart {pane})"
        ));
    }
    println!("{pane} restarts now and resumes its conversation with the new CLAUDE.md");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fenced_reply_is_unwrapped() {
        let body = "# Agent\n".to_string() + &"rule\n".repeat(2000);
        let reply = format!("```markdown\n{body}```\n");
        assert_eq!(cleaned_reply(&reply, 20_000).unwrap(), body);
    }

    #[test]
    fn empty_too_long_or_too_short_rewrites_are_refused() {
        assert!(cleaned_reply("  ", 41_000).is_err());
        assert!(cleaned_reply(&"x".repeat(39_500), 41_000).is_err());
        assert!(cleaned_reply(&"x".repeat(2_000), 41_000).is_err());
        assert!(cleaned_reply(&"x".repeat(25_000), 41_000).is_ok());
    }
}
