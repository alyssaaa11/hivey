//! `hiver slack`: connect hiver to the user's Slack and give swarms and agents a channel.
//! The Slack work is done by the `hiver.slack-relay` plugin's scripts (connect.py,
//! create_channel.py); this module finds the plugin and wires channels into manifests.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::{api, launch, swarms};

pub(super) const RELAY: &str = "hiver.slack-relay";

const HELP: &str = "\
usage: hiver slack connect [--force]   connect your Slack workspace: create the hiver Slack app,
                                     paste its bot token (typed hidden, checked, kept private)
       hiver slack status [--json]     connected? workspace, bot, missing scopes
       hiver slack add <slug> [--channel ID]
                                     give a running swarm or agent its own channel (#<slug>,
                                     created or joined) and open the Slack relay in its space
  New ones: hiver swarm launch … --slack. The hiver agent: hiver home setup --slack (#hiver).";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    if let Some(sub) = args.first().filter(|_| super::wants_help(&rest)) {
        println!("{}", super::sub_help(HELP, "hiver slack", sub));
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
            eprintln!("hiver slack: {err}");
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
        .ok_or_else(|| format!("the {RELAY} plugin is not installed (hiver plugin list)"))
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
        return Err("usage: hiver slack add <slug> [--channel ID]".into());
    };
    let swarm = swarms()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|swarm| swarm["slug"] == slug.as_str())
        .ok_or_else(|| format!("no swarm or agent {slug:?} (hiver swarm directory)"))?;
    let root = PathBuf::from(swarm["root"].as_str().unwrap_or_default());
    let channel = match channel {
        Some(channel) => channel,
        None => {
            if !connected() {
                return Err("Slack is not connected: run hiver slack connect in a terminal".into());
            }
            create_channel(slug, &format!("hiver: talk to {slug}"))?
        }
    };
    ensure_relay(slug, &root, &channel)?;
    println!("{slug} ⇄ Slack {channel}: messages there reach it, its replies are posted there");
    Ok(())
}
