//! `hivey evolve`: hivey's self-improvement loop. Agents' Claude Code hooks log facts all week;
//! every Wednesday after the Claude weekly reset a review finds patterns, AI updates and manual
//! work, and the user answers yes/no on the self-improve page (menu → self-improve). The work is
//! done by the `hivey-evolve` skill's `evolve.py`.

const HELP: &str = "\
usage: hivey evolve open                  the self-improve page (also hivey menu → self-improve)
       hivey evolve status                hooks, Wednesday schedule, facts this week, locks
       hivey evolve hooks install|remove|status
                                          learning-log hooks in ~/.claude/settings.json (backed up)
       hivey evolve schedule install|remove|status [--at 09:10] [--weekday 3]
                                          the Wednesday run (launchd com.hivey.evolve)
       hivey evolve review [--no-llm] [--notify] [--days 7]
                                          run a review now (weekly = review + message to #hivey)
       hivey evolve lock status|accept [PATH]
                                          tamper locks on safety files (hivey-safety.py, check.sh)
       hivey evolve decide DATE N yes|no  answer a review item (the page does this)
  State: ~/.hivey/evolve. Nothing is built on its own: approved items wait for you.";

const SCRIPT: &str = "hivey-evolve/scripts/evolve.py";

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
        eprintln!("hivey evolve: the hivey-evolve skill is not installed (hivey skill install)");
        return Ok(1);
    };
    let mut command = std::process::Command::new("python3");
    command.arg("-I").arg(&script).args(args);
    if let Ok(exe) = std::env::current_exe() {
        command.env("HIVEY_BIN", exe);
    }
    match command.status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("hivey evolve: cannot run {}: {err}", script.display());
            Ok(1)
        }
    }
}
