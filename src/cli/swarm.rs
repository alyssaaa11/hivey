//! hiver CLI: `hiver swarm …` and `hiver msg …` (socket method `swarm`).

use serde_json::{json, Value};

use std::io::{BufRead, Write};

use crate::api::schema::{Method, PaneTarget, Request};
use crate::swarm::SwarmParams;

mod directory;
mod home;
mod launch;
mod pet;
mod skill;
mod skills;
mod slack;
pub(super) use home::run as run_home_command;
pub(super) use pet::run as run_pet_command;
pub(super) use skill::run_skill_command;
pub(super) use skills::run as run_skills_command;
pub(super) use slack::run as run_slack_command;

const SWARM_HELP: &str = "\
hiver swarm commands:
  hiver swarm new [--provider ID] [--default] <task…>
                                     design + launch a swarm with the default swarm creator
                                     (built in: hiver.swarm-creator)
  hiver swarm new --agent [--provider ID] [--default] <task…>
                                     one solo agent in this folder with the default agent creator
                                     (built in: hiver.agent-creator)
  hiver swarm providers [--default ID]
                                     installed swarm and agent creators (* = used by swarm new);
                                     --default ID makes ID the one used for its kind
  hiver swarm accept-trust [--pane P] [--kind claude|codex] [--timeout 60]
                                     answer an agent CLI's folder-trust prompt in a pane (providers)
  hiver swarm launch <root> --slug S <agent>...  start a designed swarm in its own space
  hiver swarm launch <root> --slug S --solo [--model M]  start one agent in <root> (its own master)
                                     gets its own Slack channel #<slug> when Slack is connected
  hiver swarm addon <swarm> <plugin>...  open addons (dashboard, relays) in a running swarm
  hiver swarm relaunch <swarm> [<agent>...]  restart agents (continuing their conversation) and addons
  hiver swarm register <root>        register a swarm or agent folder (<root>/.swarm/agents.json);
                                     alias: import
  hiver swarm unregister <slug>      remove it from hiver (files are kept); alias: forget
  hiver swarm directory [--json]     every swarm and solo agent: what it does, skills, tools,
                                     busy or idle, address (ask the user before sending work)
  hiver swarm profile <slug> [--description T] [--skills a,b] [--tools x,y]
                                     set (or show) a directory entry
  hiver swarm list [--json]          swarms, agents, roles, states and queued messages
  hiver swarm master [<slug>] [--focus]
                                     show (or focus) a swarm's master; default: your swarm
  hiver swarm info [<slug>]          agents, Slack channel, vault, addons, budget, tasks, repos
  hiver swarm schedule add <swarm> (--every 15m | --at 09:00) [--to AGENT] [--id ID] <task…>
                                     wake the master (or AGENT) with a task on a schedule
  hiver swarm schedule list|remove|run [<swarm>] [<id>]
  hiver swarm pick                   choose a swarm and jump to its master (interactive)
  hiver swarm setup                  swarm sidebar + Option keys (⌥S ⌥M ⌥A ⌥I ⌥L ⌥P ⌥F ⌥Q)
  hiver swarm pause|resume <slug>    hold / release message delivery (manifest state)
  Slack: hiver slack connect (once), hiver slack add <slug> (channel for a running one)";

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

/// `hiver swarm accept-trust`: wait for an agent CLI's "trust this folder" prompt in a pane and
/// answer it. For setup providers that start the master themselves in a new folder (launch
/// already does this for the agents it starts). Run it in the background, then exec the CLI.
fn accept_trust(args: &[String]) -> std::io::Result<i32> {
    let mut rest = args.to_vec();
    let parsed = (|| -> Result<(String, crate::swarm::adapter::AgentKind, u64), String> {
        let pane = take_value(&mut rest, "--pane")?
            .or_else(|| std::env::var(crate::integration::HERDR_PANE_ID_ENV_VAR).ok())
            .ok_or("no pane: pass --pane or run inside hiver")?;
        let kind = match take_value(&mut rest, "--kind")? {
            Some(kind) => crate::swarm::adapter::AgentKind::parse(&kind)
                .ok_or_else(|| format!("--kind must be claude or codex, not {kind:?}"))?,
            None => crate::swarm::adapter::AgentKind::Claude,
        };
        let timeout = match take_value(&mut rest, "--timeout")? {
            Some(secs) => secs.parse().map_err(|_| "--timeout is seconds")?,
            None => 60,
        };
        Ok((pane, kind, timeout))
    })();
    let (pane, kind, timeout) = match parsed {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(2);
        }
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    while std::time::Instant::now() < deadline {
        let pane = canonical_pane_id(&pane);
        let screen = api("pane.read", json!({ "pane_id": pane, "source": "visible" }))
            .ok()
            .and_then(|read| read["read"]["text"].as_str().map(str::to_lowercase))
            .unwrap_or_default();
        if screen.contains("trust this folder") {
            let keys: Vec<&str> = kind.trust_keys().to_vec();
            let _ = api("pane.send_keys", json!({ "pane_id": pane, "keys": keys }));
            println!("accepted the folder-trust prompt in {pane}");
            return Ok(0);
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    println!("no trust prompt appeared within {timeout}s");
    Ok(0)
}

use crate::swarm::creators::{self, Creator, SETUP_ENTRYPOINT};

/// The installed swarm and agent creators (plugins with a setup pane), from the server.
fn providers() -> Result<Vec<Creator>, String> {
    let list = api("plugin.list", json!({}))?;
    let plugins = list["plugins"].as_array().cloned().unwrap_or_default();
    Ok(creators::from_plugins(&plugins))
}

/// The creator `hiver swarm new` uses for a kind (see `creators::pick`).
fn effective_provider(found: &[Creator], agent: bool) -> Result<String, String> {
    let chosen = creators::chosen(agent);
    let picked = creators::pick(found, agent, chosen.as_deref());
    if let (Some(chosen), Ok(picked)) = (&chosen, &picked) {
        if chosen != picked {
            eprintln!("note: the chosen creator {chosen:?} is not installed; using {picked}");
        }
    }
    picked
}

fn kind_name(agent: bool) -> &'static str {
    if agent {
        "agent"
    } else {
        "swarm"
    }
}

fn list_providers(args: &[String]) -> std::io::Result<i32> {
    let mut rest: Vec<String> = args.to_vec();
    let set_default = match take_value(&mut rest, "--default") {
        Ok(value) => value,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(2);
        }
    };
    let found = match providers() {
        Ok(found) => found,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(1);
        }
    };
    if let Some(id) = set_default {
        let Some(creator) = found.iter().find(|creator| creator.id == id) else {
            eprintln!("error: {id:?} is not installed (hiver swarm providers)");
            return Ok(1);
        };
        creators::save_chosen(creator.agent, &id)?;
        println!("{} creator: {id}", kind_name(creator.agent));
        return Ok(0);
    }
    if found.is_empty() {
        println!(
            "no swarm or agent creators installed (plugins with a \"setup\" pane; the built-in\n\
             ones: hiver plugin link <hiver repo>/plugins/swarm-creator and …/agent-creator)"
        );
        return Ok(0);
    }
    for agent in [false, true] {
        let current = creators::pick(&found, agent, creators::chosen(agent).as_deref()).ok();
        println!("{} creators:", kind_name(agent));
        for creator in found.iter().filter(|creator| creator.agent == agent) {
            let mark = if current.as_deref() == Some(creator.id.as_str()) {
                "*"
            } else {
                " "
            };
            println!("{mark} {:<24} {}", creator.id, creator.description);
        }
    }
    Ok(0)
}

/// `hiver swarm new [--agent]`: hand a task to a swarm (or agent) creator. Its "setup" pane
/// opens as a new tab in the current folder with HIVER_SETUP_TASK set; it designs the team (or
/// the agent), writes the briefs and calls `hiver swarm launch`.
fn new_swarm(args: &[String]) -> std::io::Result<i32> {
    let mut rest: Vec<String> = args.to_vec();
    let make_default = take_flag(&mut rest, "--default");
    let agent = take_flag(&mut rest, "--agent");
    let chosen = match take_value(&mut rest, "--provider") {
        Ok(value) => value,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(2);
        }
    };
    let task = rest.join(" ");
    let found = match providers() {
        Ok(found) => found,
        Err(err) => {
            eprintln!("error: {err}");
            return Ok(1);
        }
    };
    let provider = match chosen {
        Some(id) if found.iter().any(|creator| creator.id == id) => id,
        Some(id) => {
            eprintln!("error: setup provider {id:?} is not installed (hiver swarm providers)");
            return Ok(1);
        }
        None => match effective_provider(&found, agent) {
            Ok(id) => id,
            Err(err) => {
                eprintln!("error: {err}");
                return Ok(1);
            }
        },
    };
    if make_default {
        let agent = found
            .iter()
            .find(|creator| creator.id == provider)
            .is_some_and(|creator| creator.agent);
        creators::save_chosen(agent, &provider)?;
        println!("default {} creator: {provider}", kind_name(agent));
    }
    let cwd = std::env::current_dir()?;
    let mut params = json!({
        "plugin_id": provider,
        "entrypoint": SETUP_ENTRYPOINT,
        "placement": "tab",
        "cwd": cwd,
        "focus": true,
        "env": { "HIVER_SETUP_TASK": task, "HIVER_SETUP_CWD": cwd },
    });
    // Open it in the caller's space, else the focused one, else a new space for this folder.
    let caller = std::env::var(crate::integration::HERDR_PANE_ID_ENV_VAR)
        .or_else(|_| std::env::var(ACTIVE_PANE_ENV_VAR))
        .ok()
        .and_then(|pane| api("pane.get", json!({ "pane_id": pane })).ok())
        .and_then(|info| info["pane"]["workspace_id"].as_str().map(str::to_string));
    let focused = || {
        api("workspace.list", json!({})).ok().and_then(|list| {
            list["workspaces"]
                .as_array()?
                .iter()
                .find(|w| w["focused"] == true)?["workspace_id"]
                .as_str()
                .map(str::to_string)
        })
    };
    let workspace = match caller.or_else(focused) {
        Some(id) => id,
        None => {
            let label = cwd
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| "swarm".into());
            match api(
                "workspace.create",
                json!({ "cwd": cwd, "label": label, "focus": true }),
            ) {
                Ok(created) => created["workspace"]["workspace_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                Err(err) => {
                    eprintln!("error: cannot create a space for the setup: {err}");
                    return Ok(1);
                }
            }
        }
    };
    params["workspace_id"] = json!(workspace);
    match api("plugin.pane.open", params) {
        Ok(result) => {
            println!(
                "{provider} is setting up the swarm in {} ({})",
                result["plugin_pane"]["pane"]["pane_id"]
                    .as_str()
                    .unwrap_or("?"),
                cwd.display()
            );
            Ok(0)
        }
        Err(err) => {
            eprintln!("error: cannot open {provider}'s setup pane: {err}");
            Ok(1)
        }
    }
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

/// `--help` or `-h` after a subcommand shows its usage instead of running it: otherwise
/// `hiver swarm new --help` starts a swarm whose task is "--help".
fn wants_help(rest: &[String]) -> bool {
    rest.iter().any(|arg| arg == "--help" || arg == "-h")
}

/// The usage of `command sub` from `help` (its line plus the indented lines under it), or all
/// of `help` when it has no entry for `sub`.
fn sub_help(help: &str, command: &str, sub: &str) -> String {
    let sub = match sub {
        "import" => "register",
        "forget" => "unregister",
        other => other,
    };
    let indent_of = |line: &str| line.len() - line.trim_start().len();
    let mut lines = Vec::new();
    // Indent of the current entry's usage line; continuation lines are indented deeper.
    let mut entry_indent = None;
    for line in help.lines() {
        let line = line.strip_prefix("usage:").unwrap_or(line);
        let usage = line.trim_start();
        if let Some(after) = usage.strip_prefix(command) {
            let word = after.split_whitespace().next().unwrap_or("");
            entry_indent = word
                .split('|')
                .any(|name| name == sub)
                .then(|| indent_of(line));
        } else if entry_indent.is_some_and(|indent| indent_of(line) <= indent) {
            entry_indent = None;
        }
        if entry_indent.is_some() {
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return help.to_string();
    }
    let indent = lines[0].len() - lines[0].trim_start().len();
    let lines: Vec<&str> = lines
        .iter()
        .map(|line| line.get(indent..).unwrap_or(line.trim_start()))
        .collect();
    format!("usage: {}", lines.join("\n       "))
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
    if let Some(sub) = args.first().filter(|_| wants_help(&rest)) {
        if sub == "launch" {
            println!("{}", launch::HELP);
            return Ok(0);
        }
        println!("{}", sub_help(SWARM_HELP, "hiver swarm", sub));
        return Ok(0);
    }
    let json_out = take_flag(&mut rest, "--json");
    match args.first().map(String::as_str) {
        Some("import" | "register") if rest.len() == 1 => {
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
        Some("directory" | "dir") => directory::directory(json_out),
        Some("profile") => directory::profile(&rest),
        Some("forget" | "unregister") if rest.len() == 1 => {
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
        Some("new") => new_swarm(&args[1..]),
        Some("accept-trust") => accept_trust(&args[1..]),
        Some("providers") => list_providers(&args[1..]),
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
            "master" if swarm["home"] == true => "⬢",
            "master" if swarm["solo"] == true => "★",
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
    if let Some(sub) = args.first().filter(|_| wants_help(&rest)) {
        println!("{}", sub_help(MSG_HELP, "hiver msg", sub));
        return Ok(0);
    }
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
    ("⌥P", "choose your desktop pet (or none)"),
    ("⌥F", "zoom this pane to full size and back"),
    ("⌥Q", "quit (detach; everything keeps running)"),
];

fn keybindings_toml() -> String {
    format!(
        r#"
{KEYS_MARKER} v3 (hiver swarm setup)
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

[[keys.command]]
key = "alt+p"
type = "popup"
command = "\"$HERDR_BIN_PATH\" pet choose; printf '\\n(enter to close) '; read _"
description = "hiver: choose your desktop pet (or none)"
width = "80%"
height = "50%"
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

/// `hiver swarm setup`: swarm sidebar, Option keys, and ⌥Q / ⌥F for quit and zoom (the
/// prefix keys ctrl+b q / ctrl+b z keep working too).
const DETACH_KEYS: &str = "[\"alt+q\", \"prefix+q\"]";
const ZOOM_KEYS: &str = "[\"alt+f\", \"prefix+z\"]";

fn install_keys() -> std::io::Result<i32> {
    let path = crate::config::config_path();
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    let mut content = strip_keybindings(&current);
    content = crate::config::upsert_section_bool(&content, "ui", "swarm_sidebar", true);
    content = crate::config::upsert_section_value(&content, "keys", "detach", DETACH_KEYS);
    content = crate::config::upsert_section_value(&content, "keys", "zoom", ZOOM_KEYS);
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
    // The hiver skill for Claude Code / Codex (`hiver skill install`).
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    for (label, dir) in skill::default_targets(&home) {
        match skill::install_into(&dir) {
            Ok(status) => println!("  hiver skill for {label}: {status}"),
            Err(err) => eprintln!("  hiver skill for {label}: {err}"),
        }
    }
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
mod help_tests {
    use super::*;

    #[test]
    fn help_after_a_subcommand_is_detected() {
        let args = |list: &[&str]| list.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        assert!(wants_help(&args(&["--help"])));
        assert!(wants_help(&args(&["--provider", "x", "-h"])));
        assert!(!wants_help(&args(&["build", "a", "helpful", "tool"])));
    }

    #[test]
    fn sub_help_shows_only_that_subcommand() {
        let new = sub_help(SWARM_HELP, "hiver swarm", "new");
        assert!(new.starts_with("usage: hiver swarm new [--provider ID]"));
        assert!(new.contains("design + launch a swarm"));
        assert!(!new.contains("providers"));

        let launch = sub_help(SWARM_HELP, "hiver swarm", "launch");
        assert_eq!(launch.matches("hiver swarm launch").count(), 2);
        // A less indented line after an entry ends it.
        let help = "usage: x a   first\n         more\n       x b   second\n  footer";
        assert_eq!(sub_help(help, "x", "b"), "usage: x b   second");
        let first = sub_help(help, "x", "a");
        assert!(first.contains("more") && !first.contains("second"));

        let resume = sub_help(SWARM_HELP, "hiver swarm", "resume");
        assert!(resume.contains("pause|resume"));
        assert!(sub_help(SWARM_HELP, "hiver swarm", "import").contains("alias: import"));
        assert!(sub_help(MSG_HELP, "hiver msg", "send").contains("--urgent"));
        assert_eq!(sub_help(SWARM_HELP, "hiver swarm", "nope"), SWARM_HELP);
    }
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
        let content = crate::config::upsert_section_value(&content, "keys", "detach", DETACH_KEYS);
        let content = crate::config::upsert_section_value(&content, "keys", "zoom", ZOOM_KEYS);
        assert!(content.contains(r#"detach = ["alt+q", "prefix+q"]"#));
        let content = format!("{content}{}", keybindings_toml());
        let config: crate::config::Config = toml::from_str(&content).expect("valid config");
        assert!(config.ui.swarm_sidebar);
    }
}
