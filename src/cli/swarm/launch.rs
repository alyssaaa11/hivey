//! `hiver swarm launch`: start a designed swarm in hiver.
//!
//! Takes a roster (agents, models, kinds), gives the swarm **its own space** (the calling
//! pane, the master, moves in as pane 1; workers are tiled beside it), opens addons, starts
//! the agents and writes `<root>/.swarm/agents.json`. Who designs the team and writes the
//! briefs is up to a setup provider (e.g. the /swarm skill, see `hiver swarm new`); its
//! arguments mirror that skill's launch_swarm.py so providers can hand off directly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::{api, canonical_pane_id};
use crate::swarm::adapter::AgentKind;

pub(super) mod relaunch;

const DEFAULT_CLAUDE_ARGS: &str = "--chrome --dangerously-skip-permissions --model opus";
/// Neutral kickoff; a setup provider passes its own with `--kickoff`.
const DEFAULT_KICKOFF: &str = "Read your brief (CLAUDE.md, or AGENTS.md for Codex, in your \
folder) and start your mission. Talk to teammates with `hiver msg send <agent> \"…\"` (the master \
is `coordinator`) and read waiting messages with `hiver msg inbox`. Other swarms and agents are \
listed in `hiver swarm directory`: never send them work without asking the user first.";
/// Kickoff of a solo agent (`--solo`): no teammates to talk to.
const SOLO_KICKOFF: &str = "Read your brief (CLAUDE.md, or AGENTS.md for Codex, in this \
folder) and start your mission. If it has a Status section, resume from it. Read waiting \
messages with `hiver msg inbox`. Other swarms and agents are listed in `hiver swarm directory`: \
never send them work without asking the user first, and don't disturb one that is working.";
/// Entrypoint used when `--addon` names none and the plugin declares no pane.
const ADDON_ENTRYPOINT: &str = "relay";
/// Workers stay silent; only the coordinator speaks (AGENTS Stop hook).
const WORKER_ENV: (&str, &str) = ("AGENTS_TTS", "0");

pub(super) const HELP: &str = "\
usage: hiver swarm launch <root> --slug SLUG <agent>... [--channel ID] [--models a=sonnet,b=opus]
         [--claude-args \"...\"] [--kinds a=codex,b=claude] [--codex-args \"...\"]
         [--kickoff TEXT] [--budget-min N] [--master-pane PANE] [--no-move]
         [--addon PLUGIN[:ENTRYPOINT]]... [--heartbeat 15m [--heartbeat-task TEXT]]
       hiver swarm launch <root> --slug SLUG --solo [--model M] [--kind claude|codex] [options]
         [--description TEXT] [--skills a,b] [--tools x,y]   (profile in hiver swarm directory)
  Starts one Claude per agent in <root>/<agent>/ (CLAUDE.md required) as <slug>-<agent>.
  The calling pane becomes the master <slug>-coordinator and moves into a new space <slug>
  (--no-move keeps it where it is). Writes <root>/.swarm/agents.json and registers the swarm.
  --addon (alias --relay) opens a plugin pane (default entrypoint \"relay\") in the swarm's space
  before the agents start, with HIVER_SWARM_ROOT, HIVER_SWARM_SLUG and HIVER_SWARM_CHANNEL set:
  e.g. --addon hiver.slack-relay. Any plugin can be a relay; see plugins/README.md.
  --heartbeat 15m wakes the master every 15 min with a monitoring task and a status
  snapshot (hiver swarm schedule … adds more, e.g. a daily report at 09:00).
  --solo: a single agent, no workers. It runs in <root> itself (CLAUDE.md or AGENTS.md there)
  as agent <slug>, in a new space <slug>, and is its own master (messages, schedules and
  heartbeats go to it). The calling pane stays where it is.";

struct Options {
    root: PathBuf,
    slug: String,
    agents: Vec<String>,
    channel: Option<String>,
    models: BTreeMap<String, String>,
    claude_args: Vec<String>,
    /// Arguments for Codex agents (`--kinds a=codex`).
    codex_args: Vec<String>,
    /// Which CLI runs each agent; default Claude Code.
    kinds: BTreeMap<String, AgentKind>,
    kickoff: String,
    budget_min: Option<u64>,
    master_pane: Option<String>,
    move_master: bool,
    /// Interval of the master's monitoring wake-up (`--heartbeat 15m`), if any.
    heartbeat: Option<String>,
    /// The provider's monitoring instructions (`--heartbeat-task`); default HEARTBEAT_TASK.
    heartbeat_task: Option<String>,
    /// `(plugin id, pane entrypoint)` addons opened in the swarm's space (e.g. relays);
    /// no entrypoint means the plugin's first pane.
    addons: Vec<Addon>,
    /// One agent working in `root` itself, as its own master (`--solo`).
    solo: bool,
    /// The hiver agent (`--solo --home`, used by `hiver home`): space pinned first, unfocused.
    home: bool,
    /// Directory entry (`--description`, `--skills`, `--tools`); kept when not given.
    profile: serde_json::Map<String, Value>,
}

impl Options {
    fn kind(&self, agent: &str) -> AgentKind {
        self.kinds.get(agent).copied().unwrap_or_default()
    }

    /// Where an agent works: its own folder, or the root for a solo agent.
    fn home(&self, root: &Path, agent: &str) -> PathBuf {
        if self.solo {
            root.to_path_buf()
        } else {
            root.join(agent)
        }
    }
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

/// `a, b,c` -> `["a", "b", "c"]`.
pub(super) fn split_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

type Addon = (String, Option<String>);

/// `plugin.id` or `plugin.id:entrypoint`.
fn parse_addon(spec: &str) -> Result<Addon, String> {
    let (plugin, entry) = match spec.split_once(':') {
        Some((plugin, entry)) => (plugin, Some(entry)),
        None => (spec, None),
    };
    if plugin.is_empty() || entry.is_some_and(str::is_empty) {
        return Err(format!(
            "addon {spec:?}: expected <plugin-id>[:<entrypoint>]"
        ));
    }
    Ok((plugin.to_string(), entry.map(str::to_string)))
}

/// The plugin's first declared pane, else `relay`.
fn default_entrypoint(plugin: &str) -> String {
    api("plugin.list", json!({}))
        .ok()
        .and_then(|list| {
            list["plugins"]
                .as_array()?
                .iter()
                .find(|p| p["plugin_id"] == plugin)?["panes"]
                .as_array()?
                .first()?["id"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_else(|| ADDON_ENTRYPOINT.to_string())
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
        codex_args: split_args(AgentKind::Codex.default_args()),
        kinds: BTreeMap::new(),
        kickoff: DEFAULT_KICKOFF.to_string(),
        budget_min: None,
        master_pane: None,
        move_master: true,
        addons: Vec::new(),
        heartbeat: None,
        heartbeat_task: None,
        solo: false,
        home: false,
        profile: serde_json::Map::new(),
    };
    // `--model` / `--kind` (solo agent), keyed by slug once it is known.
    let mut solo_model = None;
    let mut solo_kind = None;
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
            "--codex-args" => opts.codex_args = split_args(&value("--codex-args")?),
            "--kinds" => {
                for pair in value("--kinds")?.split(',').filter(|p| !p.is_empty()) {
                    let (agent, kind) = pair
                        .split_once('=')
                        .ok_or_else(|| format!("--kinds entry {pair:?} is not agent=kind"))?;
                    let kind = AgentKind::parse(kind)
                        .ok_or_else(|| format!("--kinds {pair:?}: kind must be claude or codex"))?;
                    opts.kinds.insert(agent.to_string(), kind);
                }
            }
            "--master-pane" => opts.master_pane = Some(value("--master-pane")?),
            "--no-move" => opts.move_master = false,
            "--solo" => opts.solo = true,
            "--home" => opts.home = true,
            "--description" => {
                opts.profile
                    .insert("description".into(), json!(value("--description")?));
            }
            flag @ ("--skills" | "--tools") => {
                opts.profile
                    .insert(flag[2..].to_string(), json!(split_list(&value(flag)?)));
            }
            "--model" => solo_model = Some(value("--model")?),
            "--kind" => {
                let kind = value("--kind")?;
                solo_kind = Some(AgentKind::parse(&kind).ok_or("--kind must be claude or codex")?);
            }
            "--heartbeat-task" => opts.heartbeat_task = Some(value("--heartbeat-task")?),
            "--heartbeat" => {
                let every = value("--heartbeat")?;
                crate::swarm::schedule::parse_every(&every)?;
                opts.heartbeat = Some(every);
            }
            "--addon" | "--relay" => opts.addons.push(parse_addon(&value(arg)?)?),
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
    if opts.solo {
        if !opts.agents.is_empty() {
            return Err("--solo takes no agent names (the agent is <root> itself)".into());
        }
        if opts.kickoff == DEFAULT_KICKOFF {
            opts.kickoff = SOLO_KICKOFF.to_string();
        }
        if let Some(model) = solo_model {
            opts.models.insert(opts.slug.clone(), model);
        }
        if let Some(kind) = solo_kind {
            opts.kinds.insert(opts.slug.clone(), kind);
        }
    } else if opts.home {
        return Err("--home needs --solo".into());
    } else if solo_model.is_some() || solo_kind.is_some() {
        return Err("--model and --kind are for --solo; use --models / --kinds".into());
    } else if opts.agents.is_empty() {
        return Err("name at least one agent (or use --solo)".into());
    }
    Ok(opts)
}

/// Every agent needs its CLI's brief. Providers usually write CLAUDE.md; a Codex agent reads
/// AGENTS.md, so link AGENTS.md -> CLAUDE.md when only the latter exists.
fn ensure_brief(home: &Path, kind: AgentKind) -> Result<(), String> {
    let brief = home.join(kind.brief_file());
    if brief.is_file() {
        return Ok(());
    }
    let claude_md = home.join("CLAUDE.md");
    if kind == AgentKind::Codex && claude_md.is_file() {
        #[cfg(unix)]
        return std::os::unix::fs::symlink("CLAUDE.md", &brief)
            .map_err(|err| format!("cannot link {} to CLAUDE.md: {err}", brief.display()));
        #[cfg(not(unix))]
        return std::fs::copy(&claude_md, &brief)
            .map(|_| ())
            .map_err(|err| format!("cannot copy CLAUDE.md to {}: {err}", brief.display()));
    }
    Err(format!(
        "missing {} — write it before launching",
        brief.display()
    ))
}

fn check(opts: &Options, root: &Path) -> Result<(), String> {
    if opts.solo {
        return check_solo(opts, root);
    }
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
        ensure_brief(&root.join(agent), opts.kind(agent))?;
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

fn check_solo(opts: &Options, root: &Path) -> Result<(), String> {
    if !valid_name(&opts.slug) {
        return Err(format!(
            "slug {:?} is invalid (max 32 chars, [a-z0-9_-], starts with a letter)",
            opts.slug
        ));
    }
    ensure_brief(root, opts.kind(&opts.slug))?;
    let live = api("agent.list", json!({}))?;
    let taken = live["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|agent| agent["name"].as_str() == Some(opts.slug.as_str()));
    if taken {
        return Err(format!(
            "agent {:?} is already live — pick another slug, or `hiver swarm relaunch {}`",
            opts.slug, opts.slug
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
    let mut args = match opts.kind(agent) {
        AgentKind::Claude => opts.claude_args.clone(),
        AgentKind::Codex => opts.codex_args.clone(),
    };
    if let Some(model) = opts.models.get(agent) {
        match args.iter().position(|arg| arg == "--model") {
            Some(index) if index + 1 < args.len() => args[index + 1] = model.clone(),
            _ => args.extend(["--model".to_string(), model.clone()]),
        }
    }
    // --add-dir too: settings.json additionalDirectories is ignored until the folder is trusted.
    let settings = opts.home(root, agent).join(".claude").join("settings.json");
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
fn start_agent(name: &str, pane: &str, args: &[String], kind: AgentKind) -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("hiver"));
    let output = std::process::Command::new(exe)
        .args([
            "agent",
            "start",
            name,
            "--kind",
            kind.as_str(),
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
        // Claude Code: "…trust this folder"; Codex: "Trust this folder?".
        if !screen.to_lowercase().contains("trust this folder") {
            break;
        }
        let _ = api(
            "agent.send_keys",
            json!({ "target": name, "keys": kind.trust_keys() }),
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

/// `hiver swarm launch` with these arguments, without printing the manifest (`hiver home`).
pub(super) fn launch_quietly(args: &[String]) -> Result<Value, String> {
    launch(&parse(args)?)
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
    if opts.solo {
        return launch_solo(opts, &root);
    }
    let master = place_master(opts, &root)?;
    let panes = tile_workers(opts, &root, &master)?;

    // Register the swarm before anything runs, so addons (relays) and the bus are live
    // while the agents boot and no early message is missed.
    let mut agents = serde_json::Map::new();
    for (agent, pane) in &panes {
        agents.insert(
            agent.clone(),
            json!({ "herdr_name": format!("{}-{agent}", opts.slug), "pane_id": pane, "status": "starting",
                    "model": model_of(&agent_args(opts, &root, agent)),
                    "args": agent_args(opts, &root, agent),
                    "kind": opts.kind(agent).as_str() }),
        );
    }
    let mut manifest = new_manifest(opts, &root, &master, agents);
    save_manifest(&root, &manifest)?;
    import(&root)?;

    let last_pane = panes
        .last()
        .map_or(master.as_str(), |(_, pane)| pane.as_str())
        .to_string();
    let channel = manifest["channel_id"].as_str().unwrap_or("").to_string();
    let addons = open_addons(&opts.addons, &opts.slug, &root, &channel, &last_pane);
    if !addons.is_empty() {
        manifest["addons"] = json!(addons);
        save_manifest(&root, &manifest)?;
    }

    for (agent, pane) in &panes {
        let name = format!("{}-{agent}", opts.slug);
        let args = agent_args(opts, &root, agent);
        let mut status = start_agent(&name, pane, &args, opts.kind(agent));
        if status == "ready" {
            let _ = api(
                "agent.prompt",
                json!({ "target": name, "text": opts.kickoff }),
            );
            status = "started".into();
        }
        eprintln!("  {name}: {status}");
        manifest["agents"][agent]["status"] = json!(status);
    }
    save_manifest(&root, &manifest)?;
    import(&root)?;
    schedule_heartbeat(opts)?;
    Ok(manifest)
}

/// `--solo`: the agent runs in `root` in a new space and is the swarm's only member.
fn launch_solo(opts: &Options, root: &Path) -> Result<Value, String> {
    let slug = &opts.slug;
    let created = api(
        "workspace.create",
        json!({ "cwd": root, "label": slug, "focus": !opts.home }),
    )?;
    let pane = created["root_pane"]["pane_id"]
        .as_str()
        .map(str::to_string)
        .ok_or("workspace.create returned no pane")?;
    if opts.home {
        pin_first_pane_space(&pane);
    }
    let args = agent_args(opts, root, slug);
    let kind = opts.kind(slug);
    let mut agents = serde_json::Map::new();
    agents.insert(
        slug.clone(),
        json!({ "herdr_name": slug, "role": "master", "pane_id": pane, "status": "starting",
                "model": model_of(&args), "args": args, "kind": kind.as_str() }),
    );
    let mut manifest = new_manifest(opts, root, &pane, agents);
    manifest["solo"] = json!(true);
    if opts.home {
        manifest["home"] = json!(true);
    }
    manifest["coordinator"] = json!(slug);
    manifest["launch_dir"] = json!(root);
    // Relaunch reads the master's CLI flags from here.
    manifest[match kind {
        AgentKind::Claude => "claude_args",
        AgentKind::Codex => "codex_args",
    }] = json!(strip_model(&args));
    save_manifest(root, &manifest)?;
    import(root)?;

    let channel = manifest["channel_id"].as_str().unwrap_or("").to_string();
    let addons = open_addons(&opts.addons, slug, root, &channel, &pane);
    if !addons.is_empty() {
        manifest["addons"] = json!(addons);
        save_manifest(root, &manifest)?;
    }

    let mut status = start_agent(slug, &pane, &args, kind);
    if status == "ready" {
        let _ = api(
            "agent.prompt",
            json!({ "target": slug, "text": opts.kickoff }),
        );
        status = "started".into();
    }
    eprintln!("  {slug}: {status}");
    manifest["agents"][slug.as_str()]["status"] = json!(status);
    save_manifest(root, &manifest)?;
    import(root)?;
    schedule_heartbeat(opts)?;
    Ok(manifest)
}

/// Moves the space holding `pane` to the top of the list (the hiver agent's space comes first).
pub(super) fn pin_first_pane_space(pane: &str) {
    let workspace = api("pane.get", json!({ "pane_id": pane }))
        .ok()
        .and_then(|got| got["pane"]["workspace_id"].as_str().map(str::to_string));
    if let Some(workspace) = workspace {
        let _ = api(
            "workspace.move",
            json!({ "workspace_id": workspace, "insert_index": 0 }),
        );
    }
}

/// The launch flags without `--model` (models are kept per agent) or `--add-dir`
/// (re-read from settings.json on every start).
fn strip_model(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--model" || arg == "--add-dir" {
            iter.next();
        } else {
            out.push(arg.clone());
        }
    }
    out
}

fn schedule_heartbeat(opts: &Options) -> Result<(), String> {
    let Some(every) = &opts.heartbeat else {
        return Ok(());
    };
    // The master's monitoring pass (replaces a /loop heartbeat inside the coordinator).
    let added = super::call(
        "schedule.add",
        json!({ "swarm": opts.slug, "every": every, "id": "heartbeat",
                "task": opts.heartbeat_task.as_deref().unwrap_or(crate::swarm::engine::HEARTBEAT_TASK) }),
    )
    .map_err(|err| err.to_string())?;
    match added.get("error") {
        Some(error) => eprintln!("  heartbeat not scheduled: {}", error["message"]),
        None if opts.solo => eprintln!("  heartbeat: {} wakes every {every}", opts.slug),
        None => eprintln!("  heartbeat: master checks the swarm every {every}"),
    }
    Ok(())
}

fn model_of(args: &[String]) -> Option<String> {
    args.iter()
        .position(|arg| arg == "--model")
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn manifest_path(root: &Path) -> PathBuf {
    root.join(".swarm").join("agents.json")
}

/// Same manifest as launch_swarm.py (its relay and dashboard read it), plus
/// workspace_id, coordinator_pane_id and addons. Unknown existing fields are kept.
fn new_manifest(
    opts: &Options,
    root: &Path,
    master: &str,
    agents: serde_json::Map<String, Value>,
) -> Value {
    let previous: Value = std::fs::read_to_string(manifest_path(root))
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
        ("claude_args", json!(opts.claude_args)),
        ("codex_args", json!(opts.codex_args)),
    ] {
        manifest[key] = value;
    }
    if let Some(object) = manifest.as_object_mut() {
        object.remove("addons");
    }
    if !opts.profile.is_empty() {
        if !manifest["profile"].is_object() {
            manifest["profile"] = json!({});
        }
        for (key, value) in &opts.profile {
            manifest["profile"][key] = value.clone();
        }
    }
    manifest
}

fn save_manifest(root: &Path, manifest: &Value) -> Result<(), String> {
    let path = manifest_path(root);
    std::fs::create_dir_all(root.join(".swarm")).map_err(|err| err.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(manifest).unwrap_or_default() + "\n",
    )
    .and_then(|_| std::fs::rename(&tmp, &path))
    .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

fn import(root: &Path) -> Result<(), String> {
    let response = super::call("import", json!({ "root": root })).map_err(|err| err.to_string())?;
    match response.get("error") {
        Some(error) => Err(error["message"]
            .as_str()
            .unwrap_or("import failed")
            .to_string()),
        None => Ok(()),
    }
}

/// Opens each addon's pane in the swarm's space, stacked under `below`. A missing or
/// failing addon is reported but doesn't stop the launch.
fn open_addons(
    addons: &[Addon],
    slug: &str,
    root: &Path,
    channel: &str,
    below: &str,
) -> Vec<Value> {
    let mut opened = Vec::new();
    let mut target = below.to_string();
    for (plugin, entry) in addons {
        let entrypoint = entry.clone().unwrap_or_else(|| default_entrypoint(plugin));
        let result = api(
            "plugin.pane.open",
            json!({
                "plugin_id": plugin,
                "entrypoint": entrypoint,
                "placement": "split",
                "target_pane_id": target,
                "direction": "down",
                "cwd": root,
                "focus": false,
                "env": {
                    "HIVER_SWARM_ROOT": root,
                    "HIVER_SWARM_SLUG": slug,
                    "HIVER_SWARM_CHANNEL": channel,
                },
            }),
        );
        match result {
            Ok(result) => {
                let pane = result["plugin_pane"]["pane"]["pane_id"].as_str().unwrap_or("").to_string();
                eprintln!("  addon {plugin}:{entrypoint}: started in {pane}");
                if !pane.is_empty() {
                    target = pane.clone();
                }
                opened.push(json!({ "plugin": plugin, "entrypoint": entrypoint, "pane_id": pane }));
            }
            Err(err) => eprintln!(
                "  addon {plugin}:{entrypoint}: not started ({err}); install it with `hiver plugin link <dir>`"
            ),
        }
    }
    opened
}

pub(super) const ADDON_HELP: &str = "\
usage: hiver swarm addon <swarm> <plugin>[:<entrypoint>]...
  Opens addons (dashboard, relays) in a running swarm's space, e.g.
  hiver swarm addon app-ideas hiver.dashboard";

/// `hiver swarm addon <swarm> <plugin>...`: add addons to a swarm that is already running.
pub(super) fn run_addon(args: &[String]) -> std::io::Result<i32> {
    let parsed = (|| -> Result<(String, Vec<Addon>), String> {
        let (slug, specs) = args.split_first().ok_or("missing <swarm>")?;
        if specs.is_empty() {
            return Err("name at least one plugin".into());
        }
        Ok((
            slug.clone(),
            specs
                .iter()
                .map(|spec| parse_addon(spec))
                .collect::<Result<_, _>>()?,
        ))
    })();
    let (slug, addons) = match parsed {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}\n{ADDON_HELP}");
            return Ok(2);
        }
    };
    match add_addons(&slug, &addons) {
        Ok(count) if count > 0 => Ok(0),
        Ok(_) => Ok(1),
        Err(err) => {
            eprintln!("error: {err}");
            Ok(1)
        }
    }
}

fn add_addons(slug: &str, addons: &[Addon]) -> Result<usize, String> {
    let list = super::call("list", json!({})).map_err(|err| err.to_string())?;
    let swarm = list["result"]["swarms"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|swarm| swarm["slug"] == slug)
        .cloned()
        .ok_or_else(|| format!("no swarm {slug:?} (hiver swarm list)"))?;
    let root = PathBuf::from(swarm["root"].as_str().unwrap_or_default());
    // Stack under the last running agent pane of the swarm (its own space).
    let below = swarm["agents"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|agent| agent["status"] != "gone" && agent["role"] != "script")
        .filter_map(|agent| agent["pane_id"].as_str())
        .last()
        .map(str::to_string)
        .ok_or_else(|| format!("swarm {slug:?} has no running pane to open addons beside"))?;
    let mut manifest: Value = std::fs::read_to_string(manifest_path(&root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| format!("cannot read {}", manifest_path(&root).display()))?;
    let channel = manifest["channel_id"].as_str().unwrap_or("").to_string();
    let opened = open_addons(addons, slug, &root, &channel, &below);
    // Keep only addons whose pane still exists (a quit dashboard leaves a stale entry).
    let mut kept: Vec<Value> = manifest["addons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|addon| {
            addon["pane_id"]
                .as_str()
                .is_some_and(|pane| api("pane.get", json!({ "pane_id": pane })).is_ok())
        })
        .cloned()
        .collect();
    kept.extend(opened.iter().cloned());
    manifest["addons"] = json!(kept);
    save_manifest(&root, &manifest)?;
    import(&root)?;
    Ok(opened.len())
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
    fn addon_specs_name_a_plugin_and_optional_entrypoint() {
        assert_eq!(
            parse_addon("hiver.slack-relay").unwrap(),
            ("hiver.slack-relay".into(), None)
        );
        assert_eq!(
            parse_addon("me.discord:bridge").unwrap(),
            ("me.discord".into(), Some("bridge".into()))
        );
        assert!(parse_addon("me.discord:").is_err());
        assert!(parse_addon(":x").is_err());
        let opts = parse(
            &["/r", "--slug", "s", "a", "--relay", "x.y", "--addon", "z:w"].map(String::from),
        )
        .unwrap();
        assert_eq!(opts.addons.len(), 2);
    }

    #[test]
    fn herdr_name_rule() {
        assert!(valid_name("app-ideas-coordinator"));
        assert!(!valid_name("App"));
        assert!(!valid_name("9lives"));
        assert!(!valid_name(&"a".repeat(33)));
    }
}
