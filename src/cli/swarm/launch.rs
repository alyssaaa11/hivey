//! `hiver swarm launch`: native replacement for the `/swarm` skill's `launch_swarm.py`.
//!
//! Same arguments, defaults and manifest (`<root>/.swarm/agents.json`), but the swarm
//! gets **its own space**: the calling pane (the coordinator, i.e. the master) moves into
//! a new space named after the swarm and becomes pane 1; workers are tiled beside it.
//! The swarm is then registered with the engine, which labels panes and runs the bus.
//! The Slack relay and the dashboard stay with the skill.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::{api, canonical_pane_id};

const DEFAULT_CLAUDE_ARGS: &str = "--chrome --dangerously-skip-permissions --model opus";
const DEFAULT_KICKOFF: &str =
    "Read your CLAUDE.md carefully, then read the swarm wiki it points to \
(index, overview, your agent page, relevant findings) — it is the swarm's source of truth. If your \
Status section shows earlier work, you are resuming: open the files it lists, catch up with \
`hiver msg inbox` and the Slack channel, post RESUMED, and continue from Next steps. Otherwise \
announce yourself (command in CLAUDE.md) and start your mission. Keep the Status section current \
as you work.";
/// Workers stay silent; only the coordinator speaks (AGENTS Stop hook).
const WORKER_ENV: (&str, &str) = ("AGENTS_TTS", "0");

pub(super) const HELP: &str = "\
usage: hiver swarm launch <root> --slug SLUG <agent>... [--channel ID] [--models a=sonnet,b=opus]
         [--claude-args \"...\"] [--kickoff TEXT] [--budget-min N] [--master-pane PANE] [--no-move]
  Starts one Claude per agent in <root>/<agent>/ (CLAUDE.md required) as <slug>-<agent>.
  The calling pane becomes the master <slug>-coordinator and moves into a new space <slug>
  (--no-move keeps it where it is). Writes <root>/.swarm/agents.json and registers the swarm.";

struct Options {
    root: PathBuf,
    slug: String,
    agents: Vec<String>,
    channel: Option<String>,
    models: BTreeMap<String, String>,
    claude_args: Vec<String>,
    kickoff: String,
    budget_min: Option<u64>,
    master_pane: Option<String>,
    move_master: bool,
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 32
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Splits a command line on whitespace, honoring single and double quotes.
fn split_args(line: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => current.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !current.is_empty() {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, c) => current.push(c),
        }
    }
    if started || !current.is_empty() {
        args.push(current);
    }
    args
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut positional = Vec::new();
    let mut opts = Options {
        root: PathBuf::new(),
        slug: String::new(),
        agents: Vec::new(),
        channel: None,
        models: BTreeMap::new(),
        claude_args: split_args(DEFAULT_CLAUDE_ARGS),
        kickoff: DEFAULT_KICKOFF.to_string(),
        budget_min: None,
        master_pane: None,
        move_master: true,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = |flag: &str| {
            iter.next()
                .cloned()
                .ok_or_else(|| format!("missing value for {flag}"))
        };
        match arg.as_str() {
            "--slug" => opts.slug = value("--slug")?,
            "--channel" => opts.channel = Some(value("--channel")?),
            "--kickoff" => opts.kickoff = value("--kickoff")?,
            "--claude-args" => opts.claude_args = split_args(&value("--claude-args")?),
            "--master-pane" => opts.master_pane = Some(value("--master-pane")?),
            "--no-move" => opts.move_master = false,
            "--budget-min" => {
                opts.budget_min = Some(
                    value("--budget-min")?
                        .parse()
                        .map_err(|_| "--budget-min must be a number")?,
                )
            }
            "--models" => {
                for pair in value("--models")?.split(',').filter(|p| !p.is_empty()) {
                    let (agent, model) = pair
                        .split_once('=')
                        .ok_or_else(|| format!("--models entry {pair:?} is not agent=model"))?;
                    opts.models.insert(agent.to_string(), model.to_string());
                }
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => positional.push(arg.clone()),
        }
    }
    let mut positional = positional.into_iter();
    opts.root = PathBuf::from(positional.next().ok_or("missing <root>")?);
    opts.agents = positional.collect();
    if opts.slug.is_empty() {
        return Err("missing --slug".into());
    }
    if opts.agents.is_empty() {
        return Err("name at least one agent".into());
    }
    Ok(opts)
}

fn check(opts: &Options, root: &Path) -> Result<(), String> {
    let coordinator = format!("{}-coordinator", opts.slug);
    for name in
        std::iter::once(coordinator).chain(opts.agents.iter().map(|a| format!("{}-{a}", opts.slug)))
    {
        if !valid_name(&name) {
            return Err(format!(
                "agent name {name:?} is invalid (max 32 chars, [a-z0-9_-], starts with a letter); \
                 shorten the slug or agent name"
            ));
        }
    }
    for agent in &opts.agents {
        let brief = root.join(agent).join("CLAUDE.md");
        if !brief.is_file() {
            return Err(format!(
                "missing {} — write it before launching",
                brief.display()
            ));
        }
    }
    let live = api("agent.list", json!({}))?;
    let clash: Vec<String> = live["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|agent| agent["name"].as_str())
        .filter(|name| {
            opts.agents
                .iter()
                .any(|a| *name == format!("{}-{a}", opts.slug))
        })
        .map(str::to_string)
        .collect();
    if !clash.is_empty() {
        return Err(format!(
            "agent names already live: {clash:?} — pick another slug"
        ));
    }
    Ok(())
}

/// Puts the master in its own space named after the swarm; returns its (new) pane id.
fn place_master(opts: &Options, root: &Path) -> Result<String, String> {
    let Some(master) = opts
        .master_pane
        .clone()
        .or_else(|| std::env::var(crate::integration::HERDR_PANE_ID_ENV_VAR).ok())
        .map(|pane| canonical_pane_id(&pane))
    else {
        // Launched from outside hiver: an empty master pane in a new space.
        let created = api(
            "workspace.create",
            json!({ "cwd": root, "label": opts.slug, "focus": true }),
        )?;
        return created["root_pane"]["pane_id"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| "workspace.create returned no pane".into());
    };
    let master = if opts.move_master {
        move_to_own_space(opts, &master)?
    } else {
        master
    };
    // Rename the coordinator's agent after the move (a move does not keep the name);
    // a plain shell has no agent to rename.
    if let Err(err) = api(
        "agent.rename",
        json!({ "target": master, "name": format!("{}-coordinator", opts.slug) }),
    ) {
        eprintln!("  note: master not renamed ({err}); hiver finds it by pane instead");
    }
    Ok(master)
}

/// Moves `master` into a new space named after the swarm (or renames its space when it
/// is already alone there); returns its new pane id.
fn move_to_own_space(opts: &Options, master: &str) -> Result<String, String> {
    let pane = api("pane.get", json!({ "pane_id": master }))?;
    let workspace = pane["pane"]["workspace_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let siblings = api("pane.list", json!({ "workspace_id": workspace }))?;
    let alone = siblings["panes"]
        .as_array()
        .is_some_and(|panes| panes.len() == 1);
    if alone {
        // Already alone in its space: just name the space after the swarm.
        api(
            "workspace.rename",
            json!({ "workspace_id": workspace, "label": opts.slug }),
        )?;
        return Ok(master.to_string());
    }
    let moved = api(
        "pane.move",
        json!({
            "pane_id": master,
            "destination": { "type": "new_workspace", "label": opts.slug, "tab_label": "team" },
            "focus": true,
        }),
    )?;
    moved["move_result"]["pane"]["pane_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "pane.move returned no pane".into())
}

fn split(target: &str, direction: &str, cwd: &Path) -> Result<String, String> {
    let (env_key, env_value) = WORKER_ENV;
    let result = api(
        "pane.split",
        json!({
            "target_pane_id": target,
            "direction": direction,
            "cwd": cwd,
            "focus": false,
            "env": { env_key: env_value },
        }),
    )?;
    result["pane"]["pane_id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| "pane.split returned no pane".into())
}

/// Master on the left; the first worker opens a column to its right, and every
/// further worker splits the largest worker pane (right if wide, down if tall).
fn tile_workers(
    opts: &Options,
    root: &Path,
    master: &str,
) -> Result<Vec<(String, String)>, String> {
    let mut panes: Vec<(String, String)> = Vec::new();
    for agent in &opts.agents {
        let cwd = root.join(agent);
        let pane = if panes.is_empty() {
            split(master, "right", &cwd)?
        } else {
            let layout = api("pane.layout", json!({ "pane_id": master }))?;
            let largest = layout["layout"]["panes"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|pane| {
                    panes
                        .iter()
                        .any(|(_, id)| pane["pane_id"].as_str() == Some(id.as_str()))
                })
                .max_by_key(|pane| {
                    pane["rect"]["width"].as_u64().unwrap_or(0)
                        * pane["rect"]["height"].as_u64().unwrap_or(0)
                })
                .cloned();
            let (target, direction) = match largest {
                Some(pane) => {
                    let width = pane["rect"]["width"].as_u64().unwrap_or(0) as f64;
                    let height = pane["rect"]["height"].as_u64().unwrap_or(0) as f64;
                    // Terminal cells are about twice as tall as wide.
                    let direction = if width >= 2.2 * height {
                        "right"
                    } else {
                        "down"
                    };
                    (
                        pane["pane_id"].as_str().unwrap_or(master).to_string(),
                        direction,
                    )
                }
                None => (panes[panes.len() - 1].1.clone(), "down"),
            };
            split(&target, direction, &cwd)?
        };
        panes.push((agent.clone(), pane));
    }
    Ok(panes)
}

fn agent_args(opts: &Options, root: &Path, agent: &str) -> Vec<String> {
    let mut args = opts.claude_args.clone();
    if let Some(model) = opts.models.get(agent) {
        match args.iter().position(|arg| arg == "--model") {
            Some(index) if index + 1 < args.len() => args[index + 1] = model.clone(),
            _ => args.extend(["--model".to_string(), model.clone()]),
        }
    }
    // --add-dir too: settings.json additionalDirectories is ignored until the folder is trusted.
    let settings = root.join(agent).join(".claude").join("settings.json");
    let dirs: Vec<String> = std::fs::read_to_string(&settings)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| {
            value["permissions"]["additionalDirectories"]
                .as_array()
                .map(|dirs| {
                    dirs.iter()
                        .filter_map(|d| d.as_str().map(str::to_string))
                        .collect()
                })
        })
        .unwrap_or_else(|| vec![root.display().to_string()]);
    for dir in dirs {
        args.extend(["--add-dir".to_string(), dir]);
    }
    args
}

/// Starts Claude and accepts its folder-trust dialog (the coordinator created the folder).
/// Runs hiver's own `agent start`, which retries while a new pane's shell initializes and
/// waits until the agent can take input, so the kickoff prompt isn't lost.
fn start_agent(name: &str, pane: &str, args: &[String]) -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hiver"));
    let output = std::process::Command::new(exe)
        .args([
            "agent",
            "start",
            name,
            "--kind",
            "claude",
            "--pane",
            pane,
            "--timeout",
            "60000",
            "--",
        ])
        .args(args)
        .output();
    let response: Value = match output {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                output.stderr
            } else {
                output.stdout
            };
            serde_json::from_slice(&text).unwrap_or(Value::Null)
        }
        Err(err) => return format!("error: {err}"),
    };
    match response["error"]["code"].as_str() {
        None if response.get("result").is_some() => return "ready".into(),
        Some("agent_not_ready") => {}
        Some(code) => {
            return format!(
                "error: {code}: {}",
                response["error"]["message"].as_str().unwrap_or("")
            )
        }
        None => return "error: no response from agent start".into(),
    }
    for _ in 0..3 {
        let screen = api("agent.read", json!({ "target": name, "source": "visible" }))
            .ok()
            .and_then(|read| read["read"]["text"].as_str().map(str::to_string))
            .unwrap_or_default();
        if !screen.contains("trust this folder") {
            break;
        }
        let _ = api(
            "agent.send_keys",
            json!({ "target": name, "keys": ["down", "enter"] }),
        );
        std::thread::sleep(Duration::from_secs(4));
    }
    let _ = api(
        "agent.wait",
        json!({ "target": name, "until": ["idle", "done"], "timeout_ms": 60000 }),
    );
    let status = api("agent.get", json!({ "target": name }))
        .ok()
        .and_then(|got| got["agent"]["agent_status"].as_str().map(str::to_string))
        .unwrap_or_else(|| "unknown".into());
    if matches!(status.as_str(), "idle" | "done") {
        "ready".into()
    } else {
        format!("needs attention (status={status})")
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or_default()
}

pub(super) fn run(args: &[String]) -> std::io::Result<i32> {
    let opts = match parse(args) {
        Ok(opts) => opts,
        Err(err) => {
            eprintln!("error: {err}\n{HELP}");
            return Ok(2);
        }
    };
    match launch(&opts) {
        Ok(manifest) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&manifest).unwrap_or_default()
            );
            Ok(0)
        }
        Err(err) => {
            eprintln!("error: {err}");
            Ok(1)
        }
    }
}

fn launch(opts: &Options) -> Result<Value, String> {
    let root = std::fs::canonicalize(&opts.root)
        .map_err(|err| format!("{}: {err}", opts.root.display()))?;
    check(opts, &root)?;
    let master = place_master(opts, &root)?;
    let panes = tile_workers(opts, &root, &master)?;

    let mut agents = serde_json::Map::new();
    for (agent, pane) in &panes {
        let name = format!("{}-{agent}", opts.slug);
        let args = agent_args(opts, &root, agent);
        let mut status = start_agent(&name, pane, &args);
        if status == "ready" {
            let _ = api(
                "agent.prompt",
                json!({ "target": name, "text": opts.kickoff }),
            );
            status = "started".into();
        }
        let model = args
            .iter()
            .position(|arg| arg == "--model")
            .and_then(|index| args.get(index + 1))
            .cloned();
        eprintln!("  {name}: {status}");
        agents.insert(
            agent.clone(),
            json!({ "herdr_name": name, "pane_id": pane, "status": status, "model": model }),
        );
    }

    // Same manifest as launch_swarm.py (the relay and dashboard read it), plus workspace_id.
    let path = root.join(".swarm").join("agents.json");
    let previous: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    let now = now_secs();
    let workspace = api("pane.get", json!({ "pane_id": master }))
        .ok()
        .and_then(|pane| pane["pane"]["workspace_id"].as_str().map(str::to_string));
    let mut manifest = match previous.clone() {
        Value::Object(map) => Value::Object(map),
        _ => json!({}),
    };
    // A relaunch restarts the budget clock (launched_at); first_launched_at keeps the original.
    for (key, value) in [
        ("slug", json!(opts.slug)),
        (
            "channel_id",
            json!(opts
                .channel
                .clone()
                .or_else(|| previous["channel_id"].as_str().map(str::to_string))),
        ),
        ("root", json!(root)),
        ("workspace_id", json!(workspace)),
        ("coordinator", json!(format!("{}-coordinator", opts.slug))),
        ("coordinator_pane_id", json!(master)),
        ("agents", Value::Object(agents)),
        ("launched_at", json!(now)),
        (
            "first_launched_at",
            json!(previous["first_launched_at"].as_f64().unwrap_or(now)),
        ),
        ("budget_minutes", json!(opts.budget_min)),
        ("launch_dir", json!(std::env::current_dir().ok())),
    ] {
        manifest[key] = value;
    }
    std::fs::create_dir_all(root.join(".swarm")).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
    )
    .and_then(|_| std::fs::rename(&tmp, &path))
    .map_err(|err| format!("cannot write {}: {err}", path.display()))?;

    super::call("import", json!({ "root": root }))
        .map_err(|err| err.to_string())
        .and_then(|response| match response.get("error") {
            Some(error) => Err(error["message"]
                .as_str()
                .unwrap_or("import failed")
                .to_string()),
            None => Ok(()),
        })?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_claude_args_with_quotes() {
        assert_eq!(
            split_args(r#"--model opus --append-system-prompt "be brief" -x '' "#),
            [
                "--model",
                "opus",
                "--append-system-prompt",
                "be brief",
                "-x",
                ""
            ]
        );
    }

    #[test]
    fn parses_launch_swarm_py_arguments() {
        let args: Vec<String> = [
            "/tmp/s",
            "--slug",
            "habit",
            "--channel",
            "C1",
            "builder",
            "critic",
            "--models",
            "builder=sonnet",
            "--budget-min",
            "90",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let opts = parse(&args).unwrap();
        assert_eq!(opts.agents, ["builder", "critic"]);
        assert_eq!(opts.models["builder"], "sonnet");
        assert_eq!(opts.budget_min, Some(90));
        assert!(opts.move_master);
        assert!(
            parse(&["/tmp/s".to_string()]).is_err(),
            "slug and agents required"
        );
    }

    #[test]
    fn model_override_replaces_the_default_model() {
        let mut opts = parse(&["/tmp/s", "--slug", "x", "a"].map(String::from)).unwrap();
        opts.models.insert("a".into(), "sonnet".into());
        let args = agent_args(&opts, Path::new("/nonexistent"), "a");
        let model = args.iter().position(|arg| arg == "--model").unwrap();
        assert_eq!(args[model + 1], "sonnet");
        assert_eq!(args.iter().filter(|arg| *arg == "--model").count(), 1);
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--add-dir" && w[1] == "/nonexistent"));
    }

    #[test]
    fn herdr_name_rule() {
        assert!(valid_name("app-ideas-coordinator"));
        assert!(!valid_name("App"));
        assert!(!valid_name("9lives"));
        assert!(!valid_name(&"a".repeat(33)));
    }
}
