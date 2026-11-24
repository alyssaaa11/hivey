//! `hivey task`: an agent says which task it starts and when it finishes, in the shared task log
//! (`~/.hivey/tasks.jsonl`) that the agent fleet's Tasks board reads.

use crate::swarm::tasks;

const HELP: &str = "\
usage: hivey task start \"<title>\"   you are starting a new task: name it in 2-6 words
                                    (e.g. \"Fix failing nightly jobs\"). Run it once when a
                                    request starts new work, not for \"yes\" or \"push it\".
       hivey task done [\"<note>\"]   the task is finished (optional one-line result)
       hivey task list [N]           the last N log lines (default 20)
  The log is ~/.hivey/tasks.jsonl; the agent fleet's Tasks board shows these titles. Claude
  Code agents also get a line on every prompt and at the end of every turn from a hook.";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    let Some(path) = tasks::log_path() else {
        eprintln!("hivey task: HOME is not set");
        return Ok(1);
    };
    let text = args.get(1..).unwrap_or_default().join(" ");
    match args.first().map(String::as_str) {
        Some("start") if !text.trim().is_empty() => {
            write(&path, "title", &text)?;
            Ok(0)
        }
        Some("start") => {
            eprintln!("usage: hivey task start \"<title in 2-6 words>\"");
            Ok(2)
        }
        Some("done") => {
            write(&path, "done", &text)?;
            Ok(0)
        }
        Some("list") => {
            let limit = args.get(1).and_then(|n| n.parse().ok()).unwrap_or(20);
            for entry in tasks::recent(&path, limit) {
                println!("{entry}");
            }
            Ok(0)
        }
        Some("-h" | "--help" | "help") => {
            println!("{HELP}");
            Ok(0)
        }
        _ => {
            eprintln!("{HELP}");
            Ok(2)
        }
    }
}

fn write(path: &std::path::Path, event: &str, text: &str) -> std::io::Result<()> {
    let line = tasks::entry(&tasks::now_iso(), event, text, &tasks::Writer::from_env());
    tasks::append(path, &line)
}
