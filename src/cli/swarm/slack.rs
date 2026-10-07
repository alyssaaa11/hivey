//! `hivey slack`: connect hivey to the user's Slack and give swarms and agents a channel.
//! The Slack work is done by the `hivey.slack-relay` plugin's scripts (connect.py,
//! create_channel.py); this module finds the plugin and wires channels into manifests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::{api, launch, swarms};

pub(super) const RELAY: &str = "hivey.slack-relay";

const HELP: &str = "\
usage: hivey slack connect [--force]   connect your Slack workspace: create the hivey Slack app,
                                     paste its bot token (typed hidden, checked, kept private)
       hivey slack status [--json]     connected? workspace, bot, missing scopes
       hivey slack add <slug> [--channel ID]
                                     give a running swarm or agent its own channel (#<slug>,
                                     created or joined) and open the Slack relay in its space
       hivey slack check [on|off]      hivey runs this when a window opens: a notification
                                     when Slack isn't connected or lacks permissions (quiet
                                     when offline); off stops the reminder, on brings it back
  New ones: hivey swarm launch … --slack. The hivey agent: hivey home setup --slack (#hivey).";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    if let Some(sub) = args.first().filter(|_| super::wants_help(&rest)) {
        println!("{}", super::sub_help(HELP, "hivey slack", sub));
        return Ok(0);
    }
    let outcome = match args.first().map(String::as_str) {
        Some("connect") => run_script("connect.py", &rest).map(|ok| if ok { 0 } else { 1 }),
        Some("status") => {
            let mut script_args = vec!["--status".to_string()];
            script_args.extend(rest);
            run_script("connect.py", &script_args).map(|ok| if ok { 0 } else { 1 })
        }
        Some("add") => add(&rest).map(|()| 0),
        Some("check") => check(&rest),
        Some("help" | "--help" | "-h") => {
            println!("{HELP}");
            return Ok(0);
        }
        _ => {
            eprintln!("{HELP}");
            return Ok(2);
        }
    };
    match outcome {
        Ok(code) => Ok(code),
        Err(err) => {
            eprintln!("hivey slack: {err}");
            Ok(1)
        }
    }
}

fn plugin_root() -> Result<PathBuf, String> {
    let plugins = api("plugin.list", json!({}))?;
    plugins["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|plugin| plugin["plugin_id"] == RELAY)
        .and_then(|plugin| plugin["plugin_root"].as_str())
        .map(PathBuf::from)
        .ok_or_else(|| format!("the {RELAY} plugin is not installed (hivey plugin list)"))
}

fn script(name: &str) -> Result<Command, String> {
    let mut command = Command::new("python3");
    command.arg(plugin_root()?.join(name)).env(
        "HERDR_PLUGIN_CONFIG_DIR",
        crate::plugin_paths::plugin_config_dir(RELAY),
    );
    Ok(command)
}

/// Runs a plugin script attached to this terminal (it may prompt); true when it succeeded.
fn run_script(name: &str, args: &[String]) -> Result<bool, String> {
    script(name)?
        .args(args)
        .status()
        .map(|status| status.success())
        .map_err(|err| format!("cannot run {name}: {err}"))
}

/// Whether a working Slack token is configured.
pub(super) fn connected() -> bool {
    script("connect.py").is_ok_and(|mut command| {
        command
            .args(["--status", "--json"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

/// `~/.hivey/slack.json`: `remind` (default on) — whether a window opening checks Slack.
fn check_settings_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| Path::new(&home).join(".hivey").join("slack.json"))
}

fn remind() -> bool {
    check_settings_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|saved| saved["remind"].as_bool())
        .unwrap_or(true)
}

fn set_remind(on: bool) -> Result<(), String> {
    let path = check_settings_path().ok_or("no HOME")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let text = serde_json::to_string_pretty(&json!({ "remind": on })).unwrap_or_default();
    std::fs::write(&path, text + "\n").map_err(|err| err.to_string())
}

/// What a window opening should tell the user about Slack, from `connect.py --status --json`:
/// nothing when it works or Slack can't be reached (offline), else a title and a body.
fn check_notice(state: &Value) -> Option<(String, String)> {
    if state["offline"].as_bool() == Some(true) {
        return None;
    }
    let fix = "Run: hivey slack connect --force (or ask your hivey agent). \
               Turn this reminder off: hivey slack check off";
    if state["connected"].as_bool() != Some(true) {
        let why = state["error"].as_str().unwrap_or("no Slack token set");
        return Some(("Slack is not connected".into(), format!("{why}. {fix}")));
    }
    let missing: Vec<&str> = state["missing_scopes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    (!missing.is_empty()).then(|| {
        (
            "Slack is missing permissions".into(),
            format!(
                "{} (add them under OAuth & Permissions and reinstall the app). {fix}",
                missing.join(", ")
            ),
        )
    })
}

/// `hivey slack check [on|off]`: run in the background when a hivey window opens.
fn check(args: &[String]) -> Result<i32, String> {
    match args.first().map(String::as_str) {
        Some(value @ ("on" | "off")) => {
            set_remind(value == "on")?;
            println!("Slack reminder when hivey opens: {value}");
            return Ok(0);
        }
        Some(other) => return Err(format!("unknown option {other:?} (on or off)")),
        None => {}
    }
    if !remind() {
        return Ok(0);
    }
    // The window has just opened: give its server a moment to answer plugin.list.
    let mut command = None;
    for _ in 0..10 {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if let Ok(found) = script("connect.py") {
            command = Some(found);
            break;
        }
    }
    let Some(mut command) = command else {
        // No Slack relay plugin: Slack isn't part of this install.
        return Ok(0);
    };
    let output = command
        .args(["--status", "--json"])
        .stderr(Stdio::null())
        .output()
        .map_err(|err| format!("cannot run connect.py: {err}"))?;
    let state: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    let Some((title, body)) = check_notice(&state) else {
        return Ok(0);
    };
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    Command::new(exe)
        .args([
            "notification",
            "show",
            &title,
            "--body",
            &body,
            "--sound",
            "request",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|err| err.to_string())?;
    Ok(1)
}

/// Creates (or finds and joins) the public channel `#name`; returns its id.
pub(super) fn create_channel(name: &str, purpose: &str) -> Result<String, String> {
    let output = script("create_channel.py")?
        .args([name, "--purpose", purpose])
        .output()
        .map_err(|err| format!("cannot run create_channel.py: {err}"))?;
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    match reply["channel_id"].as_str() {
        Some(id) => {
            println!(
                "Slack #{name}: {id}{}",
                if reply["existing"] == true {
                    " (existing)"
                } else {
                    " (created)"
                }
            );
            Ok(id.to_string())
        }
        None => Err(format!(
            "could not create #{name}: {}",
            reply["error"]
                .as_str()
                .unwrap_or(&String::from_utf8_lossy(&output.stderr))
        )),
    }
}

/// Points the swarm at `channel` and opens the relay in its space when it has none yet.
pub(super) fn ensure_relay(slug: &str, root: &Path, channel: &str) -> Result<(), String> {
    let path = crate::swarm::model::manifest_path(root);
    let mut manifest: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| format!("cannot read {}", path.display()))?;
    let has_relay = manifest["addons"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|addon| addon["plugin"] == RELAY);
    if has_relay && manifest["channel_id"] == channel {
        return Ok(());
    }
    manifest["channel_id"] = json!(channel);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
    )
    .map_err(|err| err.to_string())?;
    if !has_relay {
        launch::run_addon(&[slug.to_string(), RELAY.to_string()]).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn add(args: &[String]) -> Result<(), String> {
    let mut rest = args.to_vec();
    let channel = super::take_value(&mut rest, "--channel")?;
    let [slug] = rest.as_slice() else {
        return Err("usage: hivey slack add <slug> [--channel ID]".into());
    };
    let swarm = swarms()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|swarm| swarm["slug"] == slug.as_str())
        .ok_or_else(|| format!("no swarm or agent {slug:?} (hivey swarm directory)"))?;
    let root = PathBuf::from(swarm["root"].as_str().unwrap_or_default());
    let channel = match channel {
        Some(channel) => channel,
        None => {
            if !connected() {
                return Err("Slack is not connected: run hivey slack connect in a terminal".into());
            }
            create_channel(slug, &format!("hivey: talk to {slug}"))?
        }
    };
    ensure_relay(slug, &root, &channel)?;
    println!("{slug} ⇄ Slack {channel}: messages there reach it, its replies are posted there");
    Ok(())
}

#[cfg(test)]
mod check_tests {
    use super::*;

    #[test]
    fn quiet_when_connected_or_offline() {
        let ok = json!({"connected": true, "team": "AI", "user": "henry", "missing_scopes": []});
        assert_eq!(check_notice(&ok), None);
        let offline = json!({"connected": false, "offline": true, "error": "cannot reach Slack"});
        assert_eq!(check_notice(&offline), None);
    }

    #[test]
    fn notices_a_missing_token_or_scopes() {
        let none = json!({"connected": false, "error": "no Slack token set"});
        let (title, body) = check_notice(&none).unwrap_or_default();
        assert_eq!(title, "Slack is not connected");
        assert!(body.starts_with("no Slack token set."));
        let scopes = json!({"connected": true, "missing_scopes": ["channels:join"]});
        let (title, body) = check_notice(&scopes).unwrap_or_default();
        assert_eq!(title, "Slack is missing permissions");
        assert!(body.starts_with("channels:join"));
        // A broken status script (no JSON) still means Slack isn't working.
        assert!(check_notice(&Value::Null).is_some());
    }
}
