//! `hivey quiet`: quiet mode (no voice, no sounds, Slack posts held), by hand or on a daily
//! schedule.

use crate::swarm::quiet;

const HELP: &str = "\
usage: hivey quiet                     whether quiet mode is on, and its daily hours
       hivey quiet on                  go quiet: the pet and agents' summaries aren't spoken,
                                       hivey plays no sounds, and the Slack relay holds its
                                       posts. Agents keep working.
       hivey quiet off                 back to normal; the Slack relay posts one catch-up
                                       message
       hivey quiet schedule START-END  on and off by itself every day, e.g. 22-7 (local hours;
                                       the hivey server switches it). By hand still works and
                                       lasts until the next start or end.
       hivey quiet schedule off        no schedule
  Also in the hivey menu (quiet mode) and hivey settings → quiet.";

const SCHEDULE_USAGE: &str = "usage: hivey quiet schedule START-END|off  (hours 0-23, e.g. 22-7)";

fn status() {
    println!("quiet mode: {}", if quiet::is_on() { "on" } else { "off" });
    match quiet::schedule() {
        Some(hours) => println!("schedule:   {} every day", quiet::describe(Some(hours))),
        None => println!("schedule:   off (by hand only)"),
    }
}

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    match args.first().map(String::as_str) {
        None | Some("status") => {
            status();
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
        Some("schedule") => {
            let hours = match args.get(1).map(String::as_str) {
                Some("off") => None,
                Some(text) => match crate::swarm::voice::parse_quiet_hours(text) {
                    Some(hours) => Some(hours),
                    None => {
                        eprintln!("{SCHEDULE_USAGE}");
                        return Ok(2);
                    }
                },
                None => {
                    eprintln!("{SCHEDULE_USAGE}");
                    return Ok(2);
                }
            };
            quiet::set_schedule(hours)?;
            status();
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
