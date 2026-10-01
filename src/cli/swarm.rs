//! hiver CLI: `hiver swarm …` and `hiver msg …` (socket method `swarm`).

use serde_json::{json, Value};

use std::io::{BufRead, Write};

use crate::api::schema::{Method, PaneTarget, Request};
use crate::swarm::SwarmParams;

mod launch;

const SWARM_HELP: &str = "\
hiver swarm commands:
  hiver swarm launch <root> --slug S <agent>...  start a swarm in its own space (see --help)
  hiver swarm addon <swarm> <plugin>...  open addons (dashboard, relays) in a running swarm
  hiver swarm relaunch <swarm> [<agent>...]  restart agents (continuing their conversation) and addons
  hiver swarm import <root>          register a swarm folder (<root>/.swarm/agents.json)
  hiver swarm list [--json]          swarms, agents, roles, states and queued messages
  hiver swarm master [<slug>] [--focus]
                                     show (or focus) a swarm's master; default: your swarm
  hiver swarm info [<slug>]          agents, Slack channel, vault, addons, budget, tasks, repos
  hiver swarm schedule add <swarm> (--every 15m | --at 09:00) [--to AGENT] [--id ID] <task…>
                                     wake the master (or AGENT) with a task on a schedule
  hiver swarm schedule list|remove|run [<swarm>] [<id>]
  hiver swarm pick                   choose a swarm and jump to its master (interactive)
  hiver swarm setup                  swarm sidebar + Option keys (⌥S ⌥M ⌥A ⌥I ⌥L ⌥F ⌥Q)
  hiver swarm pause|resume <slug>    hold / release message delivery (manifest state)
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
        // Fallback identity when the environment was scrubbed (see engine::identify).
        if let Ok(cwd) = std::env::current_dir() {
            object.entry("cwd").or_insert(json!(cwd));
        }
        // The caller's own pane identifies the sender; the focused pane only picks the swarm.
        if let Ok(pane) = std::env::var(crate::integration::HERDR_PANE_ID_ENV_VAR) {
            object
                .entry("from_pane")
                .or_insert(json!(canonical_pane_id(&pane)));
        } else if let Ok(pane) = std::env::var(ACTIVE_PANE_ENV_VAR) {
            object
                .entry("context_pane")
                .or_insert(json!(canonical_pane_id(&pane)));
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

/// A pane keeps the id it was started with in `HERDR_PANE_ID` even after it moves to
/// another space; hiver resolves such ids, so ask it for the current one.
fn canonical_pane_id(pane: &str) -> String {
    api("pane.get", json!({ "pane_id": pane }))
        .ok()
        .and_then(|result| result["pane"]["pane_id"].as_str().map(str::to_string))
        .unwrap_or_else(|| pane.to_string())
}

/// One socket API call by method name and JSON params (the documented wire format).
fn api(method: &str, params: Value) -> Result<Value, String> {
    let request: Request = serde_json::from_value(json!({
        "id": format!("cli:swarm:{method}"),
        "method": method,
        "params": params,
    }))
    .map_err(|err| format!("{method}: {err}"))?;
    let response = super::send_request(&request).map_err(|err| err.to_string())?;
    if let Some(error) = response.get("error") {
        return Err(error["message"].as_str().unwrap_or("error").to_string());
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

const SCHEDULE_HELP: &str = "\
usage: hiver swarm schedule add <swarm> (--every 15m | --at 09:00) [--to AGENT] [--id ID] <task…>
       hiver swarm schedule list [<swarm>]
       hiver swarm schedule remove <swarm> <id>
       hiver swarm schedule run <swarm> <id>      (now; still delivered when the target is idle)
  When due, the target (default: the master) gets the task plus a status snapshot, delivered
  when it is idle. A wake-up still waiting is never duplicated. Paused swarms are skipped.";

fn schedule_command(args: &[String]) -> std::io::Result<i32> {
    let mut rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let parsed = (|| -> Result<Value, String> {
        let mut params = json!({});
        for (flag, key) in [
            ("--every", "every"),
            ("--at", "at"),
            ("--to", "to"),
            ("--id", "id"),
        ] {
            if let Some(value) = take_value(&mut rest, flag)? {
                params[key] = json!(value);
            }
        }
        Ok(params)
    })();
    let mut params = match parsed {
        Ok(params) => params,
        Err(err) => {
            eprintln!("error: {err}\n{SCHEDULE_HELP}");
            return Ok(2);
        }
    };
    let (op, ok) = match (args.first().map(String::as_str), rest.as_slice()) {
        (Some("add"), [swarm, task @ ..]) if !task.is_empty() => {
            params["swarm"] = json!(swarm);
            params["task"] = json!(task.join(" "));
            ("schedule.add", true)
        }
        (Some("list"), []) => ("schedule.list", true),
        (Some("list"), [swarm]) => {
            params["swarm"] = json!(swarm);
            ("schedule.list", true)
        }
        (Some(op @ ("remove" | "run")), [swarm, id]) => {
            params["swarm"] = json!(swarm);
            params["id"] = json!(id);
            (
                if op == "remove" {
                    "schedule.remove"
                } else {
                    "schedule.run"
                },
                true,
            )
        }
        _ => ("", false),
    };
    if !ok {
        eprintln!("{SCHEDULE_HELP}");
        return Ok(2);
    }
    let response = call(op, params)?;
    let Some(result) = result(&response) else {
        return Ok(1);
    };
    match op {
        "schedule.add" => println!(
            "added {} to {}: {} → {}",
            result["added"]["id"].as_str().unwrap_or("?"),
            result["swarm"].as_str().unwrap_or("?"),
            result["added"]["when"].as_str().unwrap_or("?"),
            result["added"]["to"].as_str().unwrap_or("?")
        ),
        "schedule.list" => {
            let rows = result["schedules"].as_array().cloned().unwrap_or_default();
            if rows.is_empty() {
                println!("no schedules (hiver swarm schedule add …)");
            }
            for row in rows {
                println!(
                    "{}/{}  {} → {}  {}",
                    row["swarm"].as_str().unwrap_or("?"),
                    row["id"].as_str().unwrap_or("?"),
                    row["when"].as_str().unwrap_or("?"),
                    row["to"].as_str().unwrap_or("?"),
                    row["task"].as_str().unwrap_or("")
                );
            }
        }
        "schedule.remove" => println!("removed {}", result["removed"].as_str().unwrap_or("?")),
        _ => println!(
            "{}",
            if result["queued"] == true {
                "queued: delivered when the target is idle"
            } else {
                "the previous wake-up is still waiting; nothing added"
            }
        ),
    }
    Ok(0)
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
        Some(op @ ("pause" | "resume")) if rest.len() == 1 => {
            let response = call(op, json!({ "slug": rest[0] }))?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            println!(
                "{} {}: {} queued message(s) {}",
                rest[0],
                if op == "pause" { "paused" } else { "resumed" },
                result["queued"],
                if op == "pause" {
                    "held"
                } else {
                    "will be delivered"
                }
            );
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
        Some("launch") => launch::run(&args[1..]),
        Some("addon") => launch::run_addon(&args[1..]),
        Some("relaunch") => launch::relaunch::run(&args[1..]),
        Some("info") => {
            let args = match rest.first() {
                Some(slug) => json!({ "swarm": slug }),
                None => json!({}),
            };
            let response = call("info", args)?;
            let Some(result) = result(&response) else {
                return Ok(1);
            };
            println!(
                "hiver · swarm {}\n",
                result["swarm"].as_str().unwrap_or("?")
            );
            for line in result["lines"].as_array().into_iter().flatten() {
                println!("  {}", line.as_str().unwrap_or(""));
            }
            Ok(0)
        }
        Some("schedule") => schedule_command(&args[1..]),
        Some("pick") => pick_master(),
        Some("setup" | "install-keys") => install_keys(),
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
const KEYS_END: &str = "# end hiver swarm keybindings";

/// One-press Option (Alt) keys; the terminal must send Option as Alt (iTerm: "Use Option as
/// Meta"; Ghostty: macos-option-as-alt = true). Cmd keys never reach terminal programs.
const KEYS: &[(&str, &str)] = &[
    ("⌥S", "pick a swarm and jump to its master"),
    ("⌥M", "jump to this swarm's master"),
    ("⌥A", "send a message"),
    ("⌥I", "swarm info (agents, Slack, vault, addons)"),
    ("⌥L", "message log"),
    ("⌥F", "zoom this pane to full size and back"),
    ("⌥Q", "quit (detach; everything keeps running)"),
];

fn keybindings_toml() -> String {
    format!(
        r#"
{KEYS_MARKER} v2 (hiver swarm setup)
[[keys.command]]
key = "alt+s"
type = "popup"
command = "\"$HERDR_BIN_PATH\" swarm pick"
description = "hiver: pick a swarm and jump to its master"
width = "70%"
height = "50%"

[[keys.command]]
key = "alt+m"
type = "shell"
command = "\"$HERDR_BIN_PATH\" swarm master --focus"
description = "hiver: jump to this swarm's master"

[[keys.command]]
key = "alt+a"
type = "popup"
command = "\"$HERDR_BIN_PATH\" msg compose"
description = "hiver: send a message to an agent"
width = "80%"
height = "50%"

[[keys.command]]
key = "alt+i"
type = "popup"
command = "\"$HERDR_BIN_PATH\" swarm info; printf '\\n(enter to close) '; read _"
description = "hiver: this swarm's agents, Slack, vault, addons"
width = "80%"
height = "80%"

[[keys.command]]
key = "alt+l"
type = "popup"
command = "\"$HERDR_BIN_PATH\" msg log --limit 60; printf '\\n(enter to close) '; read _"
description = "hiver: message log of this swarm"
width = "90%"
height = "80%"
{KEYS_END}
"#
    )
}

/// Removes an earlier hiver keybinding block (v1 had no end marker; it ended with the
/// message-log command).
fn strip_keybindings(content: &str) -> String {
    let Some(marker) = content.find(KEYS_MARKER) else {
        return content.to_string();
    };
    let start = content[..marker].rfind('\n').map_or(0, |i| i + 1);
    let end = if let Some(end) = content[marker..].find(KEYS_END) {
        marker + end + KEYS_END.len()
    } else {
        let log = content[marker..]
            .find("description = \"hiver: message log of this swarm\"")
            .map(|i| marker + i);
        match log.and_then(|i| content[i..].find("height = \"80%\"").map(|j| i + j)) {
            Some(i) => i + "height = \"80%\"".len(),
            None => return content.to_string(),
        }
    };
    let end = content[end..]
        .find('\n')
        .map_or(content.len(), |i| end + i + 1);
    let mut out = content[..start].trim_end_matches('\n').to_string();
    out.push('\n');
    out.push_str(&content[end..]);
    out
}

/// `hiver swarm setup`: swarm sidebar, Option keys, and ⌥Q / ⌥F for quit and zoom.
fn install_keys() -> std::io::Result<i32> {
    let path = crate::config::config_path();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let mut content = strip_keybindings(&current);
    content = crate::config::upsert_section_bool(&content, "ui", "swarm_sidebar", true);
    content = crate::config::upsert_section_value(&content, "keys", "detach", "\"alt+q\"");
    content = crate::config::upsert_section_value(&content, "keys", "zoom", "\"alt+f\"");
    content.push_str(&keybindings_toml());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, content)?;
    println!("hiver swarm setup written to {}", path.display());
    println!("  swarm sidebar on (ui.swarm_sidebar = true)");
    for (key, what) in KEYS {
        println!("  {key}  {what}");
    }
    println!("  (Option must act as Alt in your terminal: iTerm \"Use Option as Meta\", Ghostty macos-option-as-alt = true)");
    let response = super::send_request(&Request {
        id: "cli:swarm:setup:reload".into(),
        method: Method::ServerReloadConfig(crate::api::schema::EmptyParams::default()),
    });
    match response {
        Ok(response) => match result(&response) {
            Some(result)
                if result["diagnostics"]
                    .as_array()
                    .is_some_and(|d| !d.is_empty()) =>
            {
                println!("config reloaded with warnings: {}", result["diagnostics"]);
            }
            Some(_) => println!("config reloaded"),
            None => {}
        },
        Err(_) => println!("start or reload hiver to use them (hiver server reload-config)"),
    }
    Ok(0)
}

#[cfg(test)]
mod keys_tests {
    use super::*;

    #[test]
    fn setup_replaces_the_v1_block_and_keeps_user_config() {
        let v1 = "[ui]\nsidebar_width = 30\n\n# hiver: swarm keybindings (hiver swarm install-keys)\n\
                  [[keys.command]]\nkey = \"prefix+m\"\n\n[[keys.command]]\nkey = \"prefix+i\"\n\
                  description = \"hiver: message log of this swarm\"\nwidth = \"90%\"\nheight = \"80%\"\n\
                  \n[[keys.command]]\nkey = \"ctrl+x\"\ndescription = \"mine\"\n";
        let stripped = strip_keybindings(v1);
        assert!(!stripped.contains("prefix+m"), "{stripped}");
        assert!(
            stripped.contains("sidebar_width = 30") && stripped.contains("ctrl+x"),
            "{stripped}"
        );
        let v2 = format!("{stripped}{}", keybindings_toml());
        let again = strip_keybindings(&v2);
        assert_eq!(
            again.trim_end(),
            stripped.trim_end(),
            "v2 block is removable"
        );
    }

    #[test]
    fn setup_config_parses() {
        let content = crate::config::upsert_section_bool("", "ui", "swarm_sidebar", true);
        let content = crate::config::upsert_section_value(&content, "keys", "detach", "\"alt+q\"");
        let content = format!("{content}{}", keybindings_toml());
        let config: crate::config::Config = toml::from_str(&content).expect("valid config");
        assert!(config.ui.swarm_sidebar);
    }
}
