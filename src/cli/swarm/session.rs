//! `hivey swarm session <pane>`: a second session of the agent running in a pane, in a new
//! pane beside it (the sidebar's right-click "New session" items run it).
//!
//! Starts the same program with the same arguments in the same folder, minus the ones that
//! pick a conversation (`--continue`, `--resume ID`, `resume …`), so the new session starts
//! fresh while the original keeps running. The new session has no hivey name: it is the
//! user's, not a swarm member, so messages and relaunches never mix the two up.

use serde_json::{json, Value};

use super::api;
use crate::swarm::adapter::strip_session_args;

pub(super) const HELP: &str = "\
usage: hivey swarm session <pane> [--direction right|down]
  Opens a new session of the agent running in <pane> (same CLI, arguments and folder, a new
  conversation) in a pane split off it (default: right) and focuses it.";

/// Script runners whose first argument is the agent's own script (`node …/codex.js`).
const RUNNERS: &[&str] = &["node", "bun", "deno", "python", "python3"];

pub(super) fn run(args: &[String]) -> std::io::Result<i32> {
    let mut pane = None;
    let mut direction = "right".to_string();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--direction" => match iter.next().map(String::as_str) {
                Some(value @ ("right" | "down")) => direction = value.to_string(),
                _ => {
                    eprintln!("error: --direction takes right or down\n{HELP}");
                    return Ok(2);
                }
            },
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(0);
            }
            flag if flag.starts_with("--") => {
                eprintln!("error: unknown flag {flag}\n{HELP}");
                return Ok(2);
            }
            _ => pane = Some(arg.clone()),
        }
    }
    let Some(pane) = pane else {
        eprintln!("error: missing <pane>\n{HELP}");
        return Ok(2);
    };
    match open_session(&pane, &direction) {
        Ok(new_pane) => {
            println!("{new_pane}");
            Ok(0)
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(1)
        }
    }
}

fn open_session(pane: &str, direction: &str) -> Result<String, String> {
    let info = api("pane.process_info", json!({ "pane_id": pane }))?;
    let process = agent_process(&info["process_info"])
        .ok_or_else(|| format!("no agent is running in {pane}"))?;
    let argv: Vec<String> = process["argv"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|arg| arg.as_str().map(str::to_string))
        .collect();
    let command = fresh_command(&argv).ok_or_else(|| format!("no command line for {pane}"))?;
    let cwd = match process["cwd"].as_str() {
        Some(cwd) => Value::from(cwd),
        None => api("pane.get", json!({ "pane_id": pane }))?["pane"]["cwd"].clone(),
    };
    let split = api(
        "pane.split",
        json!({
            "target_pane_id": pane,
            "direction": direction,
            "cwd": cwd,
            "focus": true,
        }),
    )?;
    let new_pane = split["pane"]["pane_id"]
        .as_str()
        .map(str::to_string)
        .ok_or("pane.split returned no pane")?;
    // Typed into the new shell; it runs once the shell's startup is done.
    api(
        "pane.send_input",
        json!({ "pane_id": new_pane, "text": command, "keys": ["Enter"] }),
    )?;
    Ok(new_pane)
}

/// The agent among the pane's foreground processes: the process group leader (the CLI the
/// shell started; its helpers, e.g. MCP servers, share the group).
fn agent_process(info: &Value) -> Option<&Value> {
    let processes = info["foreground_processes"].as_array()?;
    let leader = info["foreground_process_group_id"].as_u64();
    processes
        .iter()
        .find(|process| leader.is_some() && process["pid"].as_u64() == leader)
        .or_else(|| processes.first())
}

/// The shell command for a new session of `argv`: the same program and arguments, minus
/// the ones that continue or resume a conversation.
fn fresh_command(argv: &[String]) -> Option<String> {
    let program = argv.first()?;
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    let lead = if RUNNERS.contains(&name) && argv.len() > 1 {
        2
    } else {
        1
    };
    let mut words: Vec<String> = argv[..lead].to_vec();
    words.extend(strip_session_args(&argv[lead..]));
    Some(
        words
            .iter()
            .map(|word| shell_quote(word))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn shell_quote(value: &str) -> String {
    if !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'-' | b'.' | b'/' | b':' | b'@' | b'%' | b'+' | b'='
                )
        })
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn new_session_drops_the_conversation_arguments() {
        let command = fresh_command(&argv(&[
            "claude",
            "--dangerously-skip-permissions",
            "--model",
            "opus",
            "--add-dir",
            "/Users/me/my agent",
            "--continue",
        ]));
        assert_eq!(
            command.as_deref(),
            Some(
                "claude --dangerously-skip-permissions --model opus --add-dir '/Users/me/my agent'"
            )
        );
        assert_eq!(
            fresh_command(&argv(&["claude", "--resume", "abc", "--model", "opus"])).as_deref(),
            Some("claude --model opus")
        );
    }

    #[test]
    fn script_runners_keep_their_script() {
        assert_eq!(
            fresh_command(&argv(&[
                "node",
                "/opt/codex/bin/codex.js",
                "resume",
                "--last"
            ]))
            .as_deref(),
            Some("node /opt/codex/bin/codex.js")
        );
        assert_eq!(fresh_command(&[]), None);
    }

    #[test]
    fn the_agent_is_the_process_group_leader() {
        let info = json!({
            "foreground_process_group_id": 7,
            "foreground_processes": [
                { "pid": 9, "argv": ["node", "mcp-server.js"] },
                { "pid": 7, "argv": ["claude"] },
            ],
        });
        assert_eq!(
            agent_process(&info).map(|p| p["pid"].clone()),
            Some(json!(7))
        );
    }
}
