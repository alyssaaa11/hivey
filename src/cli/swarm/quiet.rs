//! `hivey quiet`: quiet mode (no voice, no sounds, Slack posts held) until it is turned off.

use crate::swarm::quiet;

const HELP: &str = "\
usage: hivey quiet            whether quiet mode is on
       hivey quiet on         go quiet: the pet and agents' summaries aren't spoken, hivey plays
                              no sounds, and the Slack relay holds its posts. Agents keep working.
       hivey quiet off        back to normal; the Slack relay posts one catch-up message
  Also in the hivey menu (quiet mode). For quiet every night: hivey voice quiet 22-8.";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("status") => {
            println!("quiet mode: {}", if quiet::is_on() { "on" } else { "off" });
            Ok(0)
        }
        Some("on") => {
            quiet::set(true)?;
            println!("quiet mode: on (no voice, no sounds, Slack posts held)");
            Ok(0)
        }
        Some("off") => {
            quiet::set(false)?;
            println!("quiet mode: off");
            Ok(0)
        }
        Some("-h" | "--help" | "help") => {
            println!("{HELP}");
            Ok(0)
        }
        Some(_) => {
            eprintln!("{HELP}");
            Ok(2)
        }
    }
}
