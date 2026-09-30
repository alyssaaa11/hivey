//! hiver CLI: `hiver swarm …` and `hiver msg …` (socket method `swarm`).

use serde_json::{json, Value};

use crate::api::schema::{AgentTarget, Method, Request};
use crate::swarm::SwarmParams;

const SWARM_HELP: &str = "\
hiver swarm commands:
  hiver swarm import <root>          register a swarm folder (<root>/.swarm/agents.json)
  hiver swarm list [--json]          swarms, agents, roles, states and queued messages
  hiver swarm master [<slug>] [--focus]
                                     show (or focus) a swarm's master; default: your swarm
  hiver swarm forget <slug>          unregister (files are kept)";

const MSG_HELP: &str = "\
hiver msg commands:
  hiver msg send <to> <text…> [--fyi|--urgent] [--reply-to ID] [--swarm SLUG] [--from SWARM/AGENT]
      <to>: agent (scout), @all, @role:worker|critic|master, @masters, <swarm>/<agent>, human
      normal: delivered when the agent is idle · --fyi: never wakes it · --urgent: now
  hiver msg inbox [--agent SWARM/AGENT] [--all] [--json]   pull your messages (marks them read)
  hiver msg log [--swarm SLUG] [--limit N] [--json]        message history";

fn call(op: &str, mut args: Value) -> std::io::Result<Value> {
    if let (Some(object), Ok(pane)) = (
        args.as_object_mut(),
        std::env::var(crate::integration::HERDR_PANE_ID_ENV_VAR),
    ) {
        object.entry("from_pane").or_insert(json!(pane));
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
            let name = result["herdr_name"].as_str().or(result["pane_id"].as_str());
            println!(
                "◆ {}/{} {}",
                result["swarm"].as_str().unwrap_or("?"),
                result["key"].as_str().unwrap_or("?"),
                name.unwrap_or("(not running)")
            );
            match (focus, name) {
                (true, Some(target)) => super::print_response(&super::send_request(&Request {
                    id: "cli:swarm:master:focus".into(),
                    method: Method::AgentFocus(AgentTarget {
                        target: target.to_string(),
                    }),
                })?),
                (true, None) => Ok(1),
                _ => Ok(0),
            }
        }
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
