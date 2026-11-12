//! `hivey agent-map`: what runs on this computer, what it costs and how agents feed each other.
//! The work is done by the `hivey-agent-map` skill's `agent_map.py` (installed with the hivey
//! skills); this command runs it, so the menu's "agent fleet" item (its Jobs tab shows the agent jobs pages) and launchd
//! can reach it.

const HELP: &str = "\
usage: hivey agent-map open              open the latest map (its navbar has Stats, Logs, Sessions);
                                         the first time, builds the pages in seconds, the map after
       hivey agent-map fleet [--no-llm]  the agent fleet: every live agent at a desk on
                                         a 3D spaceship, one ship per hivey session, its current job
                                         read from its terminal and session log and summed up by
                                         Haiku, its context and compact/clear advice; click an agent
                                         to open it in hivey (⚙ on the page: updates, music, space)
       hivey agent-map refresh           scan again into the latest report (free, no model); the
                                         pages' Refresh button does this
       hivey agent-map nightly [--model sonnet] [--no-llm] [--open]
                                         scan, stats page, map update with a cheap model
       hivey agent-map schedule [install|remove|status|offer] [--at 06:30] [--model sonnet]
                                         the nightly launchd run (com.hivey.agent-map)
       hivey agent-map schedule token --command \"envsave get es_…\"
                                         long-lived Claude login (claude setup-token) for the run
       hivey agent-map context [AGENT…] [--set standard|1M]
                                         each agent's context window (list), or set it
       hivey agent-map stats|scan|link|log …   see `hivey agent-map --help-script`
  Reports: ~/.hivey/agent-map/reports/latest. Also in the hivey menu → agent fleet (📊 Jobs tab).";

const SCRIPT: &str = "hivey-agent-map/scripts/agent_map.py";

/// The installed skill's script, else the one in the hivey repo.
fn script() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let installed = home.map(|home| home.join(".claude/skills").join(SCRIPT));
    let repo = crate::hivey::repo().join("skills").join(SCRIPT);
    installed
        .into_iter()
        .chain(std::iter::once(repo))
        .find(|path| path.is_file())
}

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    if args.is_empty()
        || args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-h" | "--help" | "help"))
    {
        println!("{HELP}");
        return Ok(0);
    }
    let Some(script) = script() else {
        eprintln!(
            "hivey agent-map: the hivey-agent-map skill is not installed (hivey skill install)"
        );
        return Ok(1);
    };
    let mut command = std::process::Command::new("python3");
    command.arg("-I").arg(&script);
    if args.first().map(String::as_str) == Some("--help-script") {
        command.arg("--help");
    } else if matches!(args.first().map(String::as_str), Some("fleet" | "office"))
        && args.len() == 1
    {
        command.args(["fleet", "open"]);
    } else if matches!(args.first().map(String::as_str), Some("fleet" | "office")) {
        // "office" is the command's name before the rename, kept so older menus still work
        command.arg("fleet").args(&args[1..]);
    } else {
        command.args(args);
    }
    if let Ok(exe) = std::env::current_exe() {
        command.env("HIVEY_BIN", exe);
    }
    match command.status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("hivey agent-map: cannot run {}: {err}", script.display());
            Ok(1)
        }
    }
}
