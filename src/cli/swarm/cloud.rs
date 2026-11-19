//! `hivey cloud`: Claude Code cloud sessions and scheduled cloud tasks that carry the user's
//! skills, commands, agents, MCP servers, settings and EnvSave secrets. The work is done by the
//! `hivey.cloud` plugin's cloud.py; this module finds the plugin, runs it, and owns the
//! Remote Control switch for hivey's Claude agents (a hivey agent setting).

use std::path::PathBuf;
use std::process::Command;

use serde_json::json;

use super::api;

const PLUGIN: &str = "hivey.cloud";

const HELP: &str = "\
usage: hivey cloud setup [--rotate]     once: make the age key for secrets, then the claude.ai
                                      steps (environment variable, network, GitHub App)
       hivey cloud init                 write .hivey/cloud.toml in this repo: the skills,
                                      commands, agents, plugins, MCP servers and EnvSave keys
                                      cloud agents get
       hivey cloud secrets              paste the network secrets into claude.ai, one by one
                                      from the clipboard (values are never shown)
       hivey cloud run \"<task>\" [--name <slug>] [--model M] [--effort E] [--base REF]
                     [--pr-base BRANCH] [--job NAME --issue N] [--upload]
                                      start a cloud session on a hivey-cloud/<slug> branch
                                      (REF, default this commit, + your bundle); run several
                                      in parallel. --model/--effort: pinned for that session.
                                      --pr-base: the branch its pull request goes into.
                                      --issue N (with --job): the task is GitHub issue N.
                                      --upload: no push, Claude Code uploads the repo itself
       hivey cloud issues --label L [--job NAME] [--max 3] [--base REF] [--pr-base BRANCH]
                     [--model M] [--effort E] [--dry-run]
                                      one cloud agent per open issue with label L that has
                                      none yet (labels model:X, effort:Y override per issue;
                                      where:local issues are skipped)
       hivey cloud status [--job NAME] [--json] [--all]
                                      each job agent: working, pushed, pr-open, merged, with
                                      its pull request (found by its hivey-job:<job>#<n> line)
       hivey cloud close --job NAME     a finished job: forget its cloud agents and delete their
                                      hivey-cloud/* branches (PRs and claude/ branches stay)
       hivey cloud test [--secret es_...]
                                      a throwaway self-test agent in the cloud that checks
                                      skills, rules, hook, MCP and the EnvSave shim
       hivey cloud send <session> \"<message>\"
                                      steer a running cloud session
       hivey cloud pull <session>       continue a cloud session here (claude --teleport)
       hivey cloud list [--json]        the cloud sessions and schedules started from hivey
       hivey cloud schedule \"<when>\" \"<task>\"
                                      a scheduled cloud task (e.g. \"weekdays at 9am\") that
                                      works from the hivey-cloud-schedule branch
       hivey cloud refresh              rebuild hivey-cloud-schedule after changing the bundle
       hivey cloud bundle [--out DIR]   build the bundle locally to inspect it (no push)
       hivey cloud clean [--yes]        delete the hivey-cloud/* branches on the remote
       hivey cloud remote-control [on|off]
                                      start hivey's Claude agents with Remote Control: follow
                                      and steer them from claude.ai and the Claude app";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    if let Some(sub) = args.first().filter(|_| super::wants_help(&rest)) {
        println!("{}", super::sub_help(HELP, "hivey cloud", sub));
        return Ok(0);
    }
    let outcome = match args.first().map(String::as_str) {
        Some("remote-control") => remote_control(&rest),
        Some(
            "setup" | "init" | "secrets" | "run" | "send" | "pull" | "list" | "schedule"
            | "refresh" | "bundle" | "clean" | "test" | "status" | "issues" | "close",
        ) => run_script(args),
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
            eprintln!("hivey cloud: {err}");
            Ok(1)
        }
    }
}

fn remote_control(rest: &[String]) -> Result<i32, String> {
    use crate::swarm::agent_settings;
    let on = match rest.first().map(String::as_str) {
        None | Some("status") => {
            let state = if agent_settings::load().remote_control {
                "on"
            } else {
                "off"
            };
            println!("Remote Control for hivey's Claude agents: {state}");
            return Ok(0);
        }
        Some("on") => true,
        Some("off") => false,
        Some(other) => return Err(format!("remote-control takes on or off, not {other:?}")),
    };
    agent_settings::update(|settings| settings.remote_control = on)
        .map_err(|err| format!("cannot save the agent settings: {err}"))?;
    if on {
        println!(
            "on: Claude agents hivey starts from now on (launch, restart, wake) connect to \
             Remote Control, named after their space. Running ones keep their session until \
             restarted. Find them at claude.ai/code or in the Claude app."
        );
    } else {
        println!("off: Claude agents hivey starts from now on run without Remote Control");
    }
    Ok(0)
}

fn plugin_root() -> Result<PathBuf, String> {
    let plugins = api("plugin.list", json!({}))?;
    plugins["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|plugin| plugin["plugin_id"] == PLUGIN)
        .and_then(|plugin| plugin["plugin_root"].as_str())
        .map(PathBuf::from)
        .ok_or_else(|| format!("the {PLUGIN} plugin is not installed (hivey plugin list)"))
}

fn run_script(args: &[String]) -> Result<i32, String> {
    let status = Command::new("python3")
        .arg(plugin_root()?.join("cloud.py"))
        .args(args)
        .status()
        .map_err(|err| format!("cannot run cloud.py: {err}"))?;
    Ok(status.code().unwrap_or(1))
}
