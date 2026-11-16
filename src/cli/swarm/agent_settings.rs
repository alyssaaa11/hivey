//! `hivey hibernate` (stop idle agents to free memory, resume them later) and
//! `hivey agent-settings` (hibernation and Claude's auto-compact window, for every agent or one space).

use serde_json::{json, Value};

use crate::swarm::agent_settings::{self as settings, Layer};

const HIBERNATE_HELP: &str = "\
usage: hivey hibernate                 hibernated agents, and how long the others have been idle
       hivey hibernate now <pane>      stop the agent in <pane> now (its memory is freed; the
                                       pane, name and conversation are kept)
       hivey hibernate wake <pane>     resume it now (same conversation, latest CLI version)
  hivey stops agents idle for the configured days by itself (default 2; hivey settings →
  hibernate, a space's right-click menu → Settings…, or hivey agent-settings). Opening a
  hibernated agent's space resumes it, and so does a message sent to it.";

const SETTINGS_HELP: &str = "\
usage: hivey agent-settings [<space>] [--hibernate DAYS|off|default] [--autocompact TOKENS|auto|off|default]
  Without <space>: the defaults for every agent. With it (a swarm or agent slug): that
  space's own settings, where `default` goes back to the defaults. No flags: show them.
  --hibernate DAYS   stop an agent idle this many days to free its memory, and resume it
                     when its space is opened again (default: 2; off: never)
  --autocompact TOKENS
                     Claude Code's auto-compact window (100k to 1M, e.g. 400k, or auto),
                     typed into each Claude agent as /autocompact when it starts or this
                     changes (default: off, each agent keeps its own)
  Also in hivey settings → hibernate / autocompact, and a space's right-click menu → Settings….";

pub(in crate::cli) fn run_hibernate(args: &[String]) -> std::io::Result<i32> {
    let pane = |args: &[String]| args.get(1).map(|pane| crate::cli::normalize_pane_id(pane));
    let (op, body) = match args.first().map(String::as_str) {
        None | Some("list") => ("hibernate.list", json!({})),
        Some("now") | Some("wake") if pane(args).is_some() => {
            let op = if args[0] == "now" {
                "hibernate.now"
            } else {
                "hibernate.wake"
            };
            (op, json!({ "pane": pane(args) }))
        }
        _ => {
            println!("{HIBERNATE_HELP}");
            return Ok(
                if args.first().is_some_and(|a| a == "--help" || a == "-h") {
                    0
                } else {
                    2
                },
            );
        }
    };
    let response = super::call(op, body)?;
    let Some(result) = super::result(&response) else {
        return Ok(1);
    };
    match op {
        "hibernate.list" => print_list(result),
        "hibernate.now" => println!(
            "hibernating {} (its memory is freed in a few seconds)",
            result["pane"].as_str().unwrap_or("?")
        ),
        _ => println!("waking {}", result["pane"].as_str().unwrap_or("?")),
    }
    Ok(0)
}

fn print_list(result: &Value) {
    let sleepers = result["sleepers"].as_array().cloned().unwrap_or_default();
    if sleepers.is_empty() {
        println!("no hibernated agents");
    } else {
        println!("hibernated:");
        for sleeper in &sleepers {
            println!(
                "  {:<8} {:<28} {} ago · resumes with: {}",
                sleeper["pane_id"].as_str().unwrap_or("?"),
                sleeper["name"]
                    .as_str()
                    .unwrap_or(sleeper["agent"].as_str().unwrap_or("?")),
                super::ago(sleeper["at_ms"].as_u64().unwrap_or(0)),
                sleeper["argv"]
                    .as_array()
                    .map(|argv| argv
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" "))
                    .unwrap_or_default(),
            );
        }
    }
    let mut idle = result["idle"].as_array().cloned().unwrap_or_default();
    idle.sort_by_key(|entry| std::cmp::Reverse(entry["idle_hours"].as_u64().unwrap_or(0)));
    if !idle.is_empty() {
        println!("idle:");
        for entry in idle {
            let hours = entry["idle_hours"].as_u64().unwrap_or(0);
            let shown = if hours >= 48 {
                format!("{} days", hours / 24)
            } else {
                format!("{hours} h")
            };
            println!("  {:<8} {shown}", entry["pane"].as_str().unwrap_or("?"));
        }
    }
}

pub(in crate::cli) fn run_agent_settings(args: &[String]) -> std::io::Result<i32> {
    if super::wants_help(args) {
        println!("{SETTINGS_HELP}");
        return Ok(0);
    }
    let mut args = args.to_vec();
    let parsed = (|| -> Result<_, String> {
        let hibernate = super::take_value(&mut args, "--hibernate")?;
        let compact = super::take_value(&mut args, "--autocompact")?;
        Ok((hibernate, compact))
    })();
    let (hibernate, compact) = match parsed {
        Ok(values) => values,
        Err(message) => {
            eprintln!("{message}\n{SETTINGS_HELP}");
            return Ok(2);
        }
    };
    if args.len() > 1 || args.first().is_some_and(|arg| arg.starts_with('-')) {
        eprintln!("{SETTINGS_HELP}");
        return Ok(2);
    }
    let space = args.first().cloned();
    let space = space.as_deref();
    let reset = |value: &Option<String>| value.as_deref() == Some("default");
    if (reset(&hibernate) || reset(&compact)) && space.is_none() {
        eprintln!("`default` is for a space's own settings: hivey agent-settings <space> …");
        return Ok(2);
    }
    let days = match hibernate.as_deref().filter(|v| *v != "default") {
        Some(text) => match settings::parse_days(text) {
            Some(days) => Some(days),
            None => {
                eprintln!("--hibernate takes days (e.g. 2), off or default");
                return Ok(2);
            }
        },
        None => None,
    };
    let tokens = match compact.as_deref().filter(|v| *v != "default") {
        Some(text) => match settings::AutoCompact::parse(text) {
            Some(value) => Some(value),
            None => {
                eprintln!(
                    "--autocompact takes 100k to 1M tokens (e.g. 400k), auto, off or default"
                );
                return Ok(2);
            }
        },
        None => None,
    };
    let saved = if hibernate.is_some() || compact.is_some() {
        settings::update(|all| {
            let layer = all.layer_mut(space);
            if reset(&hibernate) {
                layer.hibernate_days = None;
            } else if days.is_some() {
                layer.hibernate_days = days;
            }
            if reset(&compact) {
                layer.autocompact = None;
            } else if tokens.is_some() {
                layer.autocompact = tokens;
            }
        })?
    } else {
        settings::load()
    };
    print_settings(&saved, space);
    Ok(0)
}

fn print_settings(all: &settings::AgentSettings, space: Option<&str>) {
    let effective = all.effective(space);
    let own = all.layer(space);
    let mark = |set: bool| {
        if set || space.is_none() {
            ""
        } else {
            " (default)"
        }
    };
    let Layer {
        hibernate_days,
        autocompact,
    } = own;
    println!(
        "{}",
        space.map_or("defaults for every agent".to_string(), |s| format!(
            "space {s}"
        ))
    );
    println!(
        "  hibernate after: {}{}",
        settings::days_label(effective.hibernate_days),
        mark(hibernate_days.is_some())
    );
    println!(
        "  autocompact:     {}{}",
        effective.autocompact.label(),
        mark(autocompact.is_some())
    );
}
