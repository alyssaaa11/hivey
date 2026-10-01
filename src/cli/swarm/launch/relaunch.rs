//! `hiver swarm relaunch`: bring a swarm's agents (and addons) back after a restart.
//!
//! A hiver restart restores the layout, but agents without a resumable session come back as
//! plain shells, and addon processes don't survive. Relaunch starts each missing agent in its
//! pane again, continuing its previous Claude conversation when there is one (every agent has
//! its own folder, so `claude --continue` picks the right conversation), and reopens addons.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{
    agent_args, api, import, manifest_path, open_addons, save_manifest, split, start_agent, Addon,
    Options, DEFAULT_CLAUDE_ARGS,
};
use crate::swarm::adapter::{strip_session_args, AgentKind};

pub(in crate::cli::swarm) const HELP: &str = "\
usage: hiver swarm relaunch <swarm> [<agent>...] [--fresh] [--no-addons | --addons-only] [--kickoff TEXT]
         [--claude-args \"...\"]
  Restarts the swarm's agents that aren't running (or the named ones, including the master)
  in their panes, continuing each agent's previous Claude conversation (--fresh: start new),
  and reopens addons whose pane is gone (--no-addons: leave them).";

const RESUME_PROMPT: &str = "hiver restarted this swarm and you were relaunched. Catch up: \
run `hiver msg inbox`, re-read your brief (CLAUDE.md or AGENTS.md) for where you were, then \
continue. Tell the coordinator you're back with `hiver msg send coordinator --fyi \"back\"`.";
const FRESH_PROMPT: &str = "You were (re)started in a running swarm. Read your brief \
(CLAUDE.md or AGENTS.md) carefully; if it records earlier work, you are resuming. Run \
`hiver msg inbox`, then continue (or start your mission). Tell the coordinator with \
`hiver msg send coordinator --fyi \"started\"`.";

struct Request {
    slug: String,
    only: Vec<String>,
    fresh: bool,
    addons: bool,
    /// Only reopen addons; leave agents alone (a provider restarting agents itself).
    addons_only: bool,
    kickoff: Option<String>,
    claude_args: Option<Vec<String>>,
}

fn parse(args: &[String]) -> Result<Request, String> {
    let mut request = Request {
        slug: String::new(),
        only: Vec::new(),
        fresh: false,
        addons: true,
        addons_only: false,
        kickoff: None,
        claude_args: None,
    };
    let mut positional = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--fresh" => request.fresh = true,
            "--no-addons" => request.addons = false,
            "--addons-only" => request.addons_only = true,
            "--kickoff" => {
                request.kickoff = Some(iter.next().cloned().ok_or("missing value for --kickoff")?)
            }
            "--claude-args" => {
                let line = iter.next().ok_or("missing value for --claude-args")?;
                request.claude_args = Some(super::split_args(line));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => positional.push(arg.clone()),
        }
    }
    let mut positional = positional.into_iter();
    request.slug = positional.next().ok_or("missing <swarm>")?;
    request.only = positional.collect();
    Ok(request)
}

/// Whether the CLI has an earlier conversation started in `dir` to continue.
fn has_conversation(dir: &Path, kind: AgentKind) -> bool {
    match kind {
        AgentKind::Claude => has_claude_conversation(dir),
        AgentKind::Codex => has_codex_conversation(dir),
    }
}

/// Codex logs sessions as ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl; the first line
/// (`session_meta`) records the working directory.
fn has_codex_conversation(dir: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let wanted = dir.to_string_lossy().to_string();
    let walk = |path: PathBuf| -> Vec<PathBuf> {
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    };
    let sessions = PathBuf::from(home).join(".codex").join("sessions");
    for year in walk(sessions) {
        for month in walk(year) {
            for day in walk(month) {
                for file in walk(day) {
                    if file.extension().is_none_or(|ext| ext != "jsonl") {
                        continue;
                    }
                    let first = std::fs::File::open(&file).ok().and_then(|f| {
                        use std::io::BufRead;
                        std::io::BufReader::new(f).lines().next()?.ok()
                    });
                    let cwd = first
                        .and_then(|line| serde_json::from_str::<Value>(&line).ok())
                        .and_then(|meta| meta["payload"]["cwd"].as_str().map(str::to_string));
                    if cwd.is_some_and(|cwd| {
                        cwd == wanted || std::fs::canonicalize(&cwd).is_ok_and(|c| c == dir)
                    }) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Claude Code keeps transcripts per working directory in ~/.claude/projects/<mangled cwd>/.
fn has_claude_conversation(dir: &Path) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mangled: String = dir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    std::fs::read_dir(
        PathBuf::from(home)
            .join(".claude")
            .join("projects")
            .join(mangled),
    )
    .map(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
    })
    .unwrap_or(false)
}

/// A pane with no agent in it: the agent's recorded pane, else a shell in the swarm's space
/// whose folder is the agent's home, else a new split.
fn find_pane(
    recorded: Option<&str>,
    workspace: Option<&str>,
    home: &Path,
    beside: &str,
) -> Result<String, String> {
    let free = |pane: &Value| pane["agent"].is_null() && pane["agent_status"] != "working";
    if let Some(pane) = recorded.and_then(|id| api("pane.get", json!({ "pane_id": id })).ok()) {
        if free(&pane["pane"]) {
            return Ok(pane["pane"]["pane_id"]
                .as_str()
                .unwrap_or_default()
                .to_string());
        }
    }
    if let Some(workspace) = workspace {
        let panes = api("pane.list", json!({ "workspace_id": workspace }))?;
        let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
        let found = panes["panes"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|pane| {
                free(pane)
                    && pane["cwd"]
                        .as_str()
                        .map(|cwd| {
                            std::fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from(cwd))
                        })
                        .is_some_and(|cwd| cwd == home)
            });
        if let Some(pane) = found {
            return Ok(pane["pane_id"].as_str().unwrap_or_default().to_string());
        }
    }
    split(beside, "down", home)
}

pub(in crate::cli::swarm) fn run(args: &[String]) -> std::io::Result<i32> {
    let request = match parse(args) {
        Ok(request) => request,
        Err(err) => {
            eprintln!("error: {err}\n{HELP}");
            return Ok(2);
        }
    };
    match relaunch(&request) {
        Ok(0) => {
            eprintln!("nothing to relaunch: every agent is running");
            Ok(0)
        }
        Ok(_) => Ok(0),
        Err(err) => {
            eprintln!("error: {err}");
            Ok(1)
        }
    }
}

fn relaunch(request: &Request) -> Result<usize, String> {
    let list = super::super::call("list", json!({})).map_err(|err| err.to_string())?;
    let swarm = list["result"]["swarms"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|swarm| swarm["slug"] == request.slug.as_str())
        .cloned()
        .ok_or_else(|| format!("no swarm {:?} (hiver swarm list)", request.slug))?;
    let root = PathBuf::from(swarm["root"].as_str().unwrap_or_default());
    let mut manifest: Value = std::fs::read_to_string(manifest_path(&root))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| format!("cannot read {}", manifest_path(&root).display()))?;
    let agents: Vec<Value> = swarm["agents"].as_array().cloned().unwrap_or_default();
    for name in &request.only {
        if !agents.iter().any(|agent| agent["key"] == name.as_str()) {
            return Err(format!("swarm {:?} has no agent {name:?}", request.slug));
        }
    }
    let targets: Vec<&Value> = agents
        .iter()
        .filter(|_| !request.addons_only)
        .filter(|agent| agent["role"] != "script")
        .filter(|agent| {
            let key = agent["key"].as_str().unwrap_or_default();
            if request.only.is_empty() {
                // The master is the user's own session: only relaunched when named.
                agent["role"] != "master" && agent["status"] == "gone"
            } else if request.only.iter().any(|name| name == key) {
                // A running agent keeps running: a second Claude would clash on the name.
                if agent["status"] != "gone" {
                    eprintln!(
                        "  {key}: already running ({}); stop it first to relaunch",
                        agent["status"]
                    );
                    return false;
                }
                true
            } else {
                false
            }
        })
        .collect();

    // Where new panes go: beside any live pane of the swarm, else its recorded master pane,
    // else any restored pane working inside the swarm's folder (after a restart).
    let workspace = manifest["workspace_id"].as_str().map(str::to_string);
    let beside = live_agent_pane(&agents)
        .or(manifest["coordinator_pane_id"].as_str())
        .map(str::to_string)
        .filter(|pane| api("pane.get", json!({ "pane_id": pane })).is_ok())
        .or_else(|| pane_in_folder(&root))
        .ok_or("no pane of this swarm is left to open agents beside; relaunch it with `hiver swarm launch`")?;

    let claude_args = request.claude_args.clone().unwrap_or_else(|| {
        manifest["claude_args"]
            .as_array()
            .map(|args| {
                args.iter()
                    .filter_map(|a| a.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_else(|| super::split_args(DEFAULT_CLAUDE_ARGS))
    });
    let models: BTreeMap<String, String> = agents
        .iter()
        .filter_map(|agent| {
            Some((
                agent["key"].as_str()?.to_string(),
                agent["model"].as_str()?.to_string(),
            ))
        })
        .collect();
    let manifest_args = |field: &str| -> Option<Vec<String>> {
        manifest[field].as_array().map(|args| {
            args.iter()
                .filter_map(|a| a.as_str().map(str::to_string))
                .collect()
        })
    };
    let codex_args = manifest_args("codex_args")
        .unwrap_or_else(|| super::split_args(AgentKind::Codex.default_args()));
    let kinds: BTreeMap<String, AgentKind> = agents
        .iter()
        .filter_map(|agent| {
            Some((
                agent["key"].as_str()?.to_string(),
                AgentKind::parse(agent["kind"].as_str()?)?,
            ))
        })
        .collect();
    let opts = Options {
        root: root.clone(),
        slug: request.slug.clone(),
        agents: Vec::new(),
        channel: None,
        models,
        claude_args,
        codex_args,
        kinds,
        kickoff: String::new(),
        budget_min: None,
        master_pane: None,
        move_master: false,
        addons: Vec::new(),
        heartbeat: None,
        heartbeat_task: None,
    };

    let mut relaunched = 0;
    for agent in targets {
        let key = agent["key"].as_str().unwrap_or_default();
        let is_master = agent["role"] == "master";
        let name = agent["herdr_name"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("{}-{key}", request.slug));
        let home = if is_master {
            // The coordinator works from where it was launched.
            manifest["launch_dir"]
                .as_str()
                .map(PathBuf::from)
                .unwrap_or_else(|| root.clone())
        } else {
            root.join(key)
        };
        let recorded = if is_master {
            manifest["coordinator_pane_id"].as_str()
        } else {
            manifest["agents"][key]["pane_id"].as_str()
        };
        let pane = match find_pane(recorded, workspace.as_deref(), &home, &beside) {
            Ok(pane) => pane,
            Err(err) => {
                eprintln!("  {name}: no pane ({err})");
                continue;
            }
        };
        let kind = opts.kind(key);
        let launch_args = agent_args(&opts, &root, key);
        let resume = !request.fresh && has_conversation(&home, kind);
        let args = if resume {
            kind.continue_args(&launch_args)
        } else {
            launch_args.clone()
        };
        let status = start_agent(&name, &pane, &args, kind);
        let status = if status == "ready" {
            let prompt = request
                .kickoff
                .clone()
                .unwrap_or_else(|| (if resume { RESUME_PROMPT } else { FRESH_PROMPT }).to_string());
            let _ = api("agent.prompt", json!({ "target": name, "text": prompt }));
            if resume { "resumed" } else { "restarted" }.to_string()
        } else {
            status
        };
        eprintln!("  {name}: {status} in {pane}");
        if is_master {
            manifest["coordinator_pane_id"] = json!(pane);
        } else if manifest["agents"][key].is_object() {
            manifest["agents"][key]["args"] = json!(strip_session_args(&launch_args));
            manifest["agents"][key]["kind"] = json!(kind.as_str());
            manifest["agents"][key]["pane_id"] = json!(pane);
            manifest["agents"][key]["status"] = json!(status);
        }
        relaunched += 1;
    }

    if request.addons {
        // Anchor addons on an agent pane that is alive now (after the restarts above).
        let fresh = super::super::call("list", json!({}))
            .ok()
            .and_then(|list| {
                list["result"]["swarms"]
                    .as_array()?
                    .iter()
                    .find(|swarm| swarm["slug"] == request.slug.as_str())?["agents"]
                    .as_array()
                    .cloned()
            })
            .unwrap_or_default();
        let anchor = live_agent_pane(&fresh)
            .map(str::to_string)
            .unwrap_or_else(|| beside.clone());
        relaunched += reopen_addons(&mut manifest, &request.slug, &root, &anchor);
    }
    save_manifest(&root, &manifest)?;
    import(&root)?;
    Ok(relaunched)
}

/// A restart restores an addon's pane as a bare shell: the addon is alive only if something
/// other than a shell runs in the foreground.
fn runs_a_program(pane: &str) -> bool {
    const SHELLS: &[&str] = &["zsh", "bash", "sh", "fish", "dash", "nu", "tcsh", "ksh"];
    api("pane.process_info", json!({ "pane_id": pane }))
        .ok()
        .and_then(|info| {
            info["process_info"]["foreground_processes"]
                .as_array()
                .cloned()
        })
        .is_some_and(|processes| {
            processes.iter().any(|process| {
                let name = process["name"]
                    .as_str()
                    .unwrap_or("")
                    .trim_start_matches('-');
                !name.is_empty() && !SHELLS.contains(&name)
            })
        })
}

/// Any pane whose working folder is inside `root` (a restored shell after a restart).
fn pane_in_folder(root: &Path) -> Option<String> {
    let root = std::fs::canonicalize(root).ok()?;
    let panes = api("pane.list", json!({})).ok()?;
    panes["panes"].as_array()?.iter().find_map(|pane| {
        let cwd = std::fs::canonicalize(pane["cwd"].as_str()?).ok()?;
        cwd.starts_with(&root)
            .then(|| pane["pane_id"].as_str().map(str::to_string))
            .flatten()
    })
}

/// The pane of a running agent (never a script/addon pane, which may be about to close).
fn live_agent_pane(agents: &[Value]) -> Option<&str> {
    agents
        .iter()
        .filter(|agent| agent["status"] != "gone" && agent["role"] != "script")
        .find_map(|agent| agent["pane_id"].as_str())
}

/// Reopens recorded addons whose pane no longer runs them (gone after a restart).
fn reopen_addons(manifest: &mut Value, slug: &str, root: &Path, beside: &str) -> usize {
    let recorded: Vec<Value> = manifest["addons"].as_array().cloned().unwrap_or_default();
    let (alive, dead): (Vec<Value>, Vec<Value>) = recorded
        .into_iter()
        .partition(|addon| addon["pane_id"].as_str().is_some_and(runs_a_program));
    if dead.is_empty() {
        return 0;
    }
    // A pane that survived a restart as a plain shell is closed and replaced.
    for addon in &dead {
        if let Some(pane) = addon["pane_id"].as_str() {
            let _ = api("pane.close", json!({ "pane_id": pane }));
        }
    }
    let specs: Vec<Addon> = dead
        .iter()
        .filter_map(|addon| {
            Some((
                addon["plugin"].as_str()?.to_string(),
                addon["entrypoint"].as_str().map(str::to_string),
            ))
        })
        .collect();
    let channel = manifest["channel_id"].as_str().unwrap_or("").to_string();
    let opened = open_addons(&specs, slug, root, &channel, beside);
    let count = opened.len();
    // An addon that failed to reopen stays recorded (without a pane), so the next relaunch
    // retries it instead of forgetting it.
    let failed: Vec<Value> = dead
        .iter()
        .filter(|addon| {
            !opened
                .iter()
                .any(|o| o["plugin"] == addon["plugin"] && o["entrypoint"] == addon["entrypoint"])
        })
        .map(|addon| json!({ "plugin": addon["plugin"], "entrypoint": addon["entrypoint"], "pane_id": "" }))
        .collect();
    let mut all = alive;
    all.extend(opened);
    all.extend(failed);
    manifest["addons"] = json!(all);
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relaunch_arguments() {
        let request =
            parse(&["app", "scout", "critic", "--fresh", "--no-addons"].map(String::from)).unwrap();
        assert_eq!(request.slug, "app");
        assert_eq!(request.only, ["scout", "critic"]);
        assert!(request.fresh && !request.addons);
        assert!(!request.addons_only);
        assert!(
            parse(&["app", "--addons-only"].map(String::from))
                .unwrap()
                .addons_only
        );
        assert!(parse(&[]).is_err());
        assert!(parse(&["app", "--bogus"].map(String::from)).is_err());
    }

    #[test]
    fn finds_claude_conversations_by_mangled_folder() {
        let home = std::env::temp_dir().join(format!("hiver-relaunch-home-{}", std::process::id()));
        let work = std::env::temp_dir().join(format!("hiver_relaunch.work-{}", std::process::id()));
        std::fs::create_dir_all(&work).unwrap();
        let canonical = std::fs::canonicalize(&work).unwrap();
        let mangled: String = canonical
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let project = home.join(".claude/projects").join(mangled);
        std::fs::create_dir_all(&project).unwrap();
        let previous = std::env::var_os("HOME");
        std::env::set_var("HOME", &home);
        assert!(!has_conversation(&work, AgentKind::Claude));
        assert!(!has_conversation(&work, AgentKind::Codex));
        std::fs::write(project.join("abc.jsonl"), "{}\n").unwrap();
        assert!(has_conversation(&work, AgentKind::Claude));
        let day = home.join(".codex/sessions/2026/10/01");
        std::fs::create_dir_all(&day).unwrap();
        let meta = format!(
            "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":{:?}}}}}\n",
            canonical.to_string_lossy()
        );
        std::fs::write(day.join("rollout-x.jsonl"), meta).unwrap();
        assert!(has_conversation(&work, AgentKind::Codex));
        match previous {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&work);
    }
}
