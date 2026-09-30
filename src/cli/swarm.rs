//! hiver CLI: `hiver swarm …` and `hiver msg …` (socket method `swarm`).

use serde_json::{json, Value};

use std::io::{BufRead, Write};

use crate::api::schema::{Method, PaneTarget, Request};
use crate::swarm::SwarmParams;

const SWARM_HELP: &str = "\
hiver swarm commands:
  hiver swarm import <root>          register a swarm folder (<root>/.swarm/agents.json)
  hiver swarm list [--json]          swarms, agents, roles, states and queued messages
  hiver swarm master [<slug>] [--focus]
                                     show (or focus) a swarm's master; default: your swarm
  hiver swarm pick                   choose a swarm and jump to its master (interactive)
  hiver swarm install-keys           add hiver keybindings to config.toml and reload
  hiver swarm forget <slug>          unregister (files are kept)";

const MSG_HELP: &str = "\
hiver msg commands:
  hiver msg send <to> <text…> [--fyi|--urgent] [--reply-to ID] [--swarm SLUG] [--from SWARM/AGENT]
      <to>: agent (scout), @all, @role:worker|critic|master, @masters, <swarm>/<agent>, human
      normal: delivered when the agent is idle · --fyi: never wakes it · --urgent: now
  hiver msg compose                                        write a message interactively (popup)
  hiver msg inbox [--agent SWARM/AGENT] [--all] [--json]   pull your messages (marks them read)
  hiver msg log [--swarm SLUG] [--limit N] [--json]        message history";

/// Set by hiver for key commands and popups: the focused pane (they have no pane of their own).
const ACTIVE_PANE_ENV_VAR: &str = "HERDR_ACTIVE_PANE_ID";

fn call(op: &str, mut args: Value) -> std::io::Result<Value> {
    if let Some(object) = args.as_object_mut() {
        // The caller's own pane identifies the sender; the focused pane only picks the swarm.
        if let Ok(pane) = std::env::var(crate::integration::HERDR_PANE_ID_ENV_VAR) {
            object.entry("from_pane").or_insert(json!(pane));
        } else if let Ok(pane) = std::env::var(ACTIVE_PANE_ENV_VAR) {
            object.entry("context_pane").or_insert(json!(pane));
        }
    }
    super::send_request(&Request {
        id: format!("cli:swarm:{op}"),
        method: Method::Swarm(SwarmParams {
            op: op.into(),
            args,
        }),
    })
}

/// Prints errors and returns the result object on success.
fn result(response: &Value) -> Option<&Value> {
    if let Some(error) = response.get("error") {
        eprintln!(
            "error: {}",
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
        );
        return None;
    }
    response.get("result")
}

fn take_flag(args: &mut Vec<String>, flag: &str) -> bool {
    let before = args.len();
    args.retain(|arg| arg != flag);
    args.len() != before
}

fn take_value(args: &mut Vec<String>, flag: &str) -> Result<Option<String>, String> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    if index + 1 >= args.len() {
        return Err(format!("missing value for {flag}"));
    }
    let value = args.remove(index + 1);
    args.remove(index);
    Ok(Some(value))
}

fn ago(ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(ms);
    let secs = now.saturating_sub(ms) / 1000;
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}

pub(super) fn run_swarm_command(args: &[String]) -> std::io::Result<i32> {
    let mut rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let json_out = take_flag(&mut rest, "--json");
    match args.first().map(String::as_str) {
        Some("import") if rest.len() == 1 => {
            let response = call("import", json!({ "root": rest[0] }))?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            print_swarm(&result["swarm"]);
            Ok(0)
        }
        Some("forget") if rest.len() == 1 => {
            let response = call("forget", json!({ "slug": rest[0] }))?;
            Ok(if result(&response).is_some() { 0 } else { 1 })
        }
        Some("list") => {
            let response = call("list", json!({}))?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            if json_out {
                println!("{result}");
            } else if result["swarms"].as_array().is_none_or(Vec::is_empty) {
                println!("no swarms registered (hiver swarm import <root>)");
            } else {
                for swarm in result["swarms"].as_array().into_iter().flatten() {
                    print_swarm(swarm);
                }
            }
            Ok(0)
        }
        Some("master") => {
            let focus = take_flag(&mut rest, "--focus");
            let args = match rest.first() {
                Some(slug) => json!({ "swarm": slug }),
                None => json!({}),
            };
            let response = call("master", args)?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            let pane = result["pane_id"].as_str();
            if !focus {
                println!(
                    "◆ {}/{} {}",
                    result["swarm"].as_str().unwrap_or("?"),
                    result["key"].as_str().unwrap_or("?"),
                    pane.unwrap_or("(not running)")
                );
                return Ok(0);
            }
            match pane {
                Some(pane) => focus_pane(pane),
                None => {
                    eprintln!("error: the master of {} is not running", result["swarm"]);
                    Ok(1)
                }
            }
        }
        Some("pick") => pick_master(),
        Some("install-keys") => install_keys(),
        Some("help" | "--help" | "-h") => {
            println!("{SWARM_HELP}");
            Ok(0)
        }
        _ => {
            eprintln!("{SWARM_HELP}");
            Ok(2)
        }
    }
}

fn print_swarm(swarm: &Value) {
    println!(
        "{}  {}",
        swarm["slug"].as_str().unwrap_or("?"),
        swarm["root"].as_str().unwrap_or("")
    );
    for agent in swarm["agents"].as_array().into_iter().flatten() {
        let role = agent["role"].as_str().unwrap_or("worker");
        let glyph = match role {
            "master" => "◆",
            "critic" => "✎",
            "script" => "▷",
            _ => "●",
        };
        let queued = agent["queued"].as_u64().unwrap_or(0);
        println!(
            "  {glyph} {:<16} {:<8} {:<8} {:<7} {}",
            agent["key"].as_str().unwrap_or("?"),
            role,
            agent["status"].as_str().unwrap_or("?"),
            agent["model"].as_str().unwrap_or(""),
            if queued > 0 {
                format!("✉ {queued} queued")
            } else {
                String::new()
            }
        );
    }
}

pub(super) fn run_msg_command(args: &[String]) -> std::io::Result<i32> {
    let mut rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let json_out = take_flag(&mut rest, "--json");
    let parsed = (|| -> Result<(Value, Option<String>), String> {
        let mut params = json!({});
        for (flag, key) in [
            ("--swarm", "swarm"),
            ("--from", "from"),
            ("--reply-to", "reply_to"),
            ("--agent", "agent"),
        ] {
            if let Some(value) = take_value(&mut rest, flag)? {
                params[key] = json!(value);
            }
        }
        if let Some(limit) = take_value(&mut rest, "--limit")? {
            params["limit"] = json!(limit
                .parse::<u64>()
                .map_err(|_| "--limit must be a number")?);
        }
        let fyi = take_flag(&mut rest, "--fyi");
        let urgent = take_flag(&mut rest, "--urgent");
        if fyi && urgent {
            return Err("--fyi and --urgent are exclusive".into());
        }
        if fyi || urgent {
            params["kind"] = json!(if fyi { "fyi" } else { "urgent" });
        }
        if take_flag(&mut rest, "--all") {
            params["all"] = json!(true);
        }
        Ok((params, rest.first().cloned()))
    })();
    let (mut params, first) = match parsed {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}\n{MSG_HELP}");
            return Ok(2);
        }
    };
    match args.first().map(String::as_str) {
        Some("send") if rest.len() >= 2 => {
            params["to"] = json!(first);
            params["text"] = json!(rest[1..].join(" "));
            let response = call("msg.send", params)?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            if let Some(notice) = result["notice"].as_str() {
                eprintln!("hiver: {notice}");
            }
            if json_out {
                println!("{result}");
            } else {
                for sent in result["sent"].as_array().into_iter().flatten() {
                    println!(
                        "→ {} [{}]",
                        sent["to"].as_str().unwrap_or("?"),
                        sent["id"].as_str().unwrap_or("?")
                    );
                }
            }
            Ok(0)
        }
        Some("inbox") => {
            let response = call("msg.inbox", params)?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            if json_out {
                println!("{result}");
                return Ok(0);
            }
            let messages = result["messages"].as_array().cloned().unwrap_or_default();
            if messages.is_empty() {
                println!("inbox empty ({})", result["agent"].as_str().unwrap_or("?"));
            }
            for msg in messages {
                println!(
                    "[{}] {} ago from {}{}: {}",
                    msg["id"].as_str().unwrap_or("?"),
                    ago(msg["ts"].as_u64().unwrap_or(0)),
                    msg["from"].as_str().unwrap_or("?"),
                    match msg["kind"].as_str() {
                        Some("fyi") => " (fyi)",
                        Some("urgent") => " (urgent)",
                        _ => "",
                    },
                    msg["text"].as_str().unwrap_or("")
                );
            }
            Ok(0)
        }
        Some("log") => {
            let response = call("msg.log", params)?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            if json_out {
                println!("{result}");
                return Ok(0);
            }
            for record in result["records"].as_array().into_iter().flatten() {
                print_record(record);
            }
            Ok(0)
        }
        Some("compose") => compose(),
        Some("help" | "--help" | "-h") => {
            println!("{MSG_HELP}");
            Ok(0)
        }
        _ => {
            eprintln!("{MSG_HELP}");
            Ok(2)
        }
    }
}

fn print_record(record: &Value) {
    let s = |key: &str| record[key].as_str().unwrap_or("");
    let when = ago(record["ts"].as_u64().unwrap_or(0));
    match s("ev") {
        "msg" => {
            let to = match record["swarm"].as_str() {
                Some(slug) => format!("{slug}/{}", s("to")),
                None => s("to").to_string(),
            };
            let kind = match s("kind") {
                "fyi" => " fyi",
                "urgent" => " URGENT",
                _ => "",
            };
            let copy = if record["copy"].as_bool() == Some(true) {
                " (copy)"
            } else {
                ""
            };
            println!(
                "{when:>4} {} → {to}{kind}{copy} [{}]: {}",
                s("from"),
                s("id"),
                s("text")
            );
        }
        "delivered" => println!(
            "{when:>4}   ✓ delivered {} to {} (batch {})",
            s("id"),
            s("to"),
            record["batch"]
        ),
        "read" => println!("{when:>4}   ✓ read {} by {}", s("id"), s("to")),
        "held" => println!(
            "{when:>4}   ⏸ held {} for {} ({})",
            s("id"),
            s("to"),
            s("reason")
        ),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Interactive helpers (used from popups) and keybinding setup
// ---------------------------------------------------------------------------

fn focus_pane(pane_id: &str) -> std::io::Result<i32> {
    let response = super::send_request(&Request {
        id: "cli:swarm:focus".into(),
        method: Method::PaneFocus(PaneTarget {
            pane_id: pane_id.to_string(),
        }),
    })?;
    Ok(if result(&response).is_some() { 0 } else { 1 })
}

/// Prints `label`, reads one line; `None` on end of input (popup closed, ctrl-d).
fn prompt(label: &str) -> Option<String> {
    print!("{label}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line.trim().to_string()),
    }
}

fn pause(message: &str) {
    println!("{message}");
    std::thread::sleep(std::time::Duration::from_millis(1200));
}

fn swarms() -> std::io::Result<Vec<Value>> {
    let response = call("list", json!({}))?;
    Ok(result(&response)
        .and_then(|result| result["swarms"].as_array().cloned())
        .unwrap_or_default())
}

fn master_of(swarm: &Value) -> Option<&Value> {
    swarm["agents"]
        .as_array()?
        .iter()
        .find(|agent| agent["role"] == "master")
}

fn pick_master() -> std::io::Result<i32> {
    let swarms = swarms()?;
    if swarms.is_empty() {
        pause("no swarms registered (hiver swarm import <root>)");
        return Ok(1);
    }
    println!("hiver · jump to a swarm's master\n");
    for (index, swarm) in swarms.iter().enumerate() {
        let agents = swarm["agents"].as_array().cloned().unwrap_or_default();
        let count = |status: &str| agents.iter().filter(|a| a["status"] == status).count();
        let queued: u64 = agents.iter().filter_map(|a| a["queued"].as_u64()).sum();
        let master_status = master_of(swarm)
            .and_then(|master| master["status"].as_str())
            .unwrap_or("none");
        println!(
            "  {:>2}  ◆ {:<20} master {:<8} ●{} working  ⚠{} attention{}",
            index + 1,
            swarm["slug"].as_str().unwrap_or("?"),
            master_status,
            count("working"),
            count("blocked") + count("done"),
            if queued > 0 {
                format!("  ✉{queued}")
            } else {
                String::new()
            }
        );
    }
    let Some(choice) = prompt("\nswarm number (enter to cancel): ") else {
        return Ok(0);
    };
    let Some(swarm) = choice
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|index| swarms.get(index))
    else {
        return Ok(0);
    };
    match master_of(swarm).and_then(|master| master["pane_id"].as_str()) {
        Some(pane) if master_of(swarm).is_some_and(|m| m["status"] != "gone") => focus_pane(pane),
        _ => {
            pause("that swarm's master is not running");
            Ok(1)
        }
    }
}

fn compose() -> std::io::Result<i32> {
    // The focused pane's swarm is the default target; bare names resolve inside it.
    let response = call("master", json!({}))?;
    let home = response["result"]["swarm"].as_str().map(str::to_string);
    let roster = home.as_deref().and_then(|slug| {
        swarms()
            .ok()?
            .into_iter()
            .find(|swarm| swarm["slug"] == slug)
    });
    println!(
        "hiver · send a message{}\n",
        home.as_deref()
            .map(|slug| format!("   swarm: {slug}"))
            .unwrap_or_default()
    );
    if let Some(swarm) = &roster {
        let names: Vec<String> = swarm["agents"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|agent| agent["role"] != "script")
            .map(|agent| {
                format!(
                    "{}{}",
                    agent["key"].as_str().unwrap_or("?"),
                    if agent["role"] == "master" {
                        " (master)"
                    } else {
                        ""
                    }
                )
            })
            .collect();
        println!("  agents: {}", names.join(" · "));
    }
    println!("  also:   @all · @role:worker · @masters · <swarm>/<agent>\n");
    let default_to = if home.is_some() { "master" } else { "" };
    let Some(to) = prompt(&format!(
        "to{}: ",
        if default_to.is_empty() {
            String::new()
        } else {
            format!(" [{default_to}]")
        }
    )) else {
        return Ok(0);
    };
    let to = if to.is_empty() {
        default_to.to_string()
    } else {
        to
    };
    if to.is_empty() {
        pause("cancelled: no recipient");
        return Ok(0);
    }
    let Some(text) = prompt("message: ").filter(|text| !text.is_empty()) else {
        pause("cancelled: empty message");
        return Ok(0);
    };
    let kind = match prompt("kind: [n]ormal (when idle) · [f]yi (no wake) · [u]rgent (now) [n]: ")
        .unwrap_or_default()
        .as_str()
    {
        "f" | "fyi" => "fyi",
        "u" | "urgent" => "urgent",
        _ => "normal",
    };
    let mut args = json!({ "to": to, "text": text, "kind": kind });
    if let Some(slug) = &home {
        args["swarm"] = json!(slug);
    }
    let response = call("msg.send", args)?;
    let Some(result) = result(&response) else {
        pause("");
        return Ok(1);
    };
    let recipients: Vec<&str> = result["sent"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|sent| sent["to"].as_str())
        .collect();
    pause(&format!("✓ sent ({kind}) → {}", recipients.join(", ")));
    Ok(0)
}

const KEYS_MARKER: &str = "# hiver: swarm keybindings";

fn keybindings_toml() -> String {
    format!(
        r#"
{KEYS_MARKER} (hiver swarm install-keys)
[[keys.command]]
key = "prefix+m"
type = "shell"
command = "\"$HERDR_BIN_PATH\" swarm master --focus"
description = "hiver: jump to this swarm's master"

[[keys.command]]
key = "prefix+shift+m"
type = "popup"
command = "\"$HERDR_BIN_PATH\" swarm pick"
description = "hiver: pick a swarm and jump to its master"
width = "70%"
height = "50%"

[[keys.command]]
key = "prefix+a"
type = "popup"
command = "\"$HERDR_BIN_PATH\" msg compose"
description = "hiver: send a message to an agent"
width = "80%"
height = "50%"

[[keys.command]]
key = "prefix+i"
type = "popup"
command = "\"$HERDR_BIN_PATH\" msg log --limit 60; printf '\\n(enter to close) '; read _"
description = "hiver: message log of this swarm"
width = "90%"
height = "80%"
"#
    )
}

fn install_keys() -> std::io::Result<i32> {
    let path = crate::config::config_path();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current.contains(KEYS_MARKER) {
        println!("hiver keybindings already in {}", path.display());
    } else {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(keybindings_toml().as_bytes())?;
        println!("added hiver keybindings to {}", path.display());
    }
    println!(
        "  prefix+m        jump to this swarm's master\n  \
         prefix+shift+m  pick a swarm\n  \
         prefix+a        send a message\n  \
         prefix+i        message log"
    );
    let response = super::send_request(&Request {
        id: "cli:swarm:install-keys:reload".into(),
        method: Method::ServerReloadConfig(crate::api::schema::EmptyParams::default()),
    });
    match response {
        Ok(response) if result(&response).is_some() => println!("config reloaded"),
        _ => println!("start or reload hiver to use them (hiver server reload-config)"),
    }
    Ok(0)
}
