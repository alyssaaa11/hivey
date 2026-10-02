//! `hiver home`: the hiver agent, the user's always-on way into hiver (see `crate::swarm::home`).
//! It lives in `~/.hiver/agent`, runs as the solo agent `hiver` in the first space, and can be
//! reached from the Slack channel `#hiver` through the `hiver.slack-relay` addon.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{launch, slack, sub_help, swarms, take_flag, take_value, wants_help};
use crate::swarm::home::{self as engine_home, SLUG};

const HELP: &str = "\
usage: hiver home                     status (same as hiver home status)
       hiver home setup [--model M] [--slack | --channel C…] [--no-start] [--force]
                                     create ~/.hiver/agent (the hiver agent) and turn it on in
                                     this session; --slack creates (or finds) #hiver and relays it
       hiver home start              start it (or bring it back) in the first space
       hiver home status             settings, folder, Slack channel, running or not
       hiver home enable|disable     keep it alive automatically, or not
  While enabled, the server of its session (default: \"default\") restarts it when it's gone.";

const BRIEF: &str = include_str!("home_brief.md");
const KICKOFF: &str = "Read your CLAUDE.md, follow its 'Start of every session' steps, then \
say hello in one line and wait for the user's requests.";
const DESCRIPTION: &str = "The hiver agent: launches and checks swarms and agents for the user, \
reachable in its space and on Slack #hiver";
const DEFAULT_MODEL: &str = "opus";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    let mut rest: Vec<String> = args.iter().skip(1).cloned().collect();
    if let Some(sub) = args.first().filter(|_| wants_help(&rest)) {
        println!("{}", sub_help(HELP, "hiver home", sub));
        return Ok(0);
    }
    let outcome = match args.first().map(String::as_str) {
        None | Some("status") => status(),
        Some("setup") => setup(&mut rest),
        Some("start") => {
            let quiet = take_flag(&mut rest, "--quiet");
            start(quiet)
        }
        Some(op @ ("enable" | "disable")) => set_enabled(op == "enable"),
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
        Ok(()) => Ok(0),
        Err(err) => {
            eprintln!("hiver home: {err}");
            Ok(1)
        }
    }
}

fn dirs() -> Result<(PathBuf, PathBuf), String> {
    let dir = engine_home::hiver_dir().ok_or("HOME is not set")?;
    let agent = dir.join("agent");
    Ok((dir, agent))
}

fn save_config(config: &Value) -> Result<(), String> {
    let path = engine_home::config_path().ok_or("HOME is not set")?;
    std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
        .map_err(|err| err.to_string())?;
    std::fs::write(
        &path,
        serde_json::to_string_pretty(config).unwrap_or_default() + "\n",
    )
    .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn setup(rest: &mut Vec<String>) -> Result<(), String> {
    let want_slack = take_flag(rest, "--slack");
    let no_start = take_flag(rest, "--no-start");
    let force = take_flag(rest, "--force");
    let model = take_value(rest, "--model")?;
    let channel = take_value(rest, "--channel")?;
    if let Some(extra) = rest.first() {
        return Err(format!("unknown argument {extra:?}\n{HELP}"));
    }
    let (dir, agent) = dirs()?;
    std::fs::create_dir_all(&agent).map_err(|err| err.to_string())?;

    // The brief: written once; the user's edits are kept unless --force.
    let brief = agent.join("CLAUDE.md");
    if force || !brief.exists() {
        std::fs::write(&brief, BRIEF).map_err(|err| err.to_string())?;
        println!("wrote {}", brief.display());
    } else {
        println!("kept {} (--force rewrites it)", brief.display());
    }
    // ~/.hiver/config → hiver's own config folder, so everything is reachable from ~/.hiver.
    let link = dir.join("config");
    if let Some(config_dir) = crate::config::config_path().parent() {
        if std::fs::symlink_metadata(&link).is_err() {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(config_dir, &link);
        }
    }

    let mut config = engine_home::load_config();
    if !config.is_object() {
        config = json!({});
    }
    config["enabled"] = json!(true);
    if let Some(model) = model {
        config["model"] = json!(model);
    } else if config["model"].as_str().is_none() {
        config["model"] = json!(DEFAULT_MODEL);
    }
    // Kept alive by the server of the session this runs in (`hiver --session X home setup`).
    config["session"] = json!(crate::session::active_name()
        .unwrap_or_else(|| crate::session::DEFAULT_SESSION_NAME.to_string()));
    if want_slack {
        if !slack::connected() {
            return Err(
                "Slack is not connected: run hiver slack connect in a terminal, then \
                        hiver home setup --slack"
                    .into(),
            );
        }
        let id = slack::create_channel("hiver", "Talk to the hiver agent (hiver home)")?;
        config["channel"] = json!(id);
    } else if let Some(channel) = channel {
        config["channel"] = json!(channel);
    }
    save_config(&config)?;
    println!(
        "hiver agent on: model {}, session {}, Slack {}",
        config["model"].as_str().unwrap_or(DEFAULT_MODEL),
        engine_home::configured_session(&config),
        config["channel"]
            .as_str()
            .map_or("none".to_string(), |c| format!("#hiver ({c})"))
    );
    if no_start {
        println!(
            "it starts with hiver's {} session",
            engine_home::configured_session(&config)
        );
        return Ok(());
    }
    start(false)
}

fn home_swarm() -> std::io::Result<Option<Value>> {
    Ok(swarms()?.into_iter().find(|swarm| swarm["slug"] == SLUG))
}

fn master_status(swarm: &Value) -> String {
    swarm["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|agent| agent["role"] == "master")
        .and_then(|agent| agent["status"].as_str())
        .unwrap_or("gone")
        .to_string()
}

fn start(quiet: bool) -> Result<(), String> {
    let config = engine_home::load_config();
    if !config.is_object() {
        return Err("not set up yet: hiver home setup".into());
    }
    let (_, agent) = dirs()?;
    if !agent.join("CLAUDE.md").is_file() {
        return Err(format!(
            "{} is missing: hiver home setup",
            agent.join("CLAUDE.md").display()
        ));
    }
    let channel = config["channel"].as_str().filter(|c| !c.is_empty());
    let say = |line: String| {
        if !quiet {
            println!("{line}");
        }
    };
    let swarm = home_swarm().map_err(|err| err.to_string())?;
    match swarm {
        Some(swarm) if master_status(&swarm) != "gone" => {
            say(format!(
                "hiver agent is running ({})",
                master_status(&swarm)
            ));
        }
        Some(_) => {
            // Registered but gone: relaunch continues its conversation (new space if needed).
            say("bringing the hiver agent back…".into());
            launch::relaunch::run(&[SLUG.to_string()]).map_err(|err| err.to_string())?;
        }
        None => {
            say("starting the hiver agent…".into());
            let model = config["model"].as_str().unwrap_or(DEFAULT_MODEL);
            let mut args: Vec<String> = [
                agent.to_string_lossy().as_ref(),
                "--slug",
                SLUG,
                "--solo",
                "--home",
                "--model",
                model,
                "--claude-args",
                "--dangerously-skip-permissions",
                "--kickoff",
                KICKOFF,
                "--description",
                DESCRIPTION,
                "--skills",
                "hiver",
                "--tools",
                "hiver CLI, Slack #hiver",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            if let Some(channel) = channel {
                args.extend(["--addon", slack::RELAY, "--channel", channel].map(str::to_string));
            }
            launch::launch_quietly(&args)?;
        }
    }
    if let Some(channel) = channel {
        slack::ensure_relay(SLUG, &agent, channel)?;
    }
    if !quiet {
        status()?;
    }
    Ok(())
}

fn set_enabled(enabled: bool) -> Result<(), String> {
    let mut config = engine_home::load_config();
    if !config.is_object() {
        return Err("not set up yet: hiver home setup".into());
    }
    config["enabled"] = json!(enabled);
    save_config(&config)?;
    println!(
        "hiver agent {}",
        if enabled {
            "enabled: hiver keeps it running"
        } else {
            "disabled: hiver won't restart it (hiver home start runs it once)"
        }
    );
    Ok(())
}

fn status() -> Result<(), String> {
    let config = engine_home::load_config();
    if !config.is_object() {
        println!("hiver agent: not set up (hiver home setup [--slack])");
        return Ok(());
    }
    let (_, agent) = dirs()?;
    let running = match home_swarm() {
        Ok(Some(swarm)) => master_status(&swarm),
        Ok(None) => "not started".to_string(),
        Err(_) => "unknown (no hiver server)".to_string(),
    };
    println!("⬢ hiver agent   {running}");
    println!(
        "  enabled      {}",
        if config["enabled"] == true {
            "yes (kept alive)"
        } else {
            "no"
        }
    );
    println!(
        "  session      {}",
        engine_home::configured_session(&config)
    );
    println!(
        "  model        {}",
        config["model"].as_str().unwrap_or(DEFAULT_MODEL)
    );
    println!("  folder       {}", agent.display());
    println!(
        "  slack        {}",
        config["channel"]
            .as_str()
            .map_or("none (hiver home setup --slack)".to_string(), |c| format!(
                "#hiver ({c})"
            ))
    );
    println!(
        "  talk to it   its space (first), Slack #hiver, or hiver msg send hiver/master \"…\""
    );
    Ok(())
}
