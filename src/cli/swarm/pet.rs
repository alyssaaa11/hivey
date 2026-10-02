//! `hiver pet`: the desktop pet (macOS). The work is done by `pets/pet.py` in the hiver repo,
//! next to the pets' sources: choose, build, start, switch, turn off.

const HELP: &str = "\
usage: hiver pet [status]         your pet (running or not) and the pets to choose from
       hiver pet choose           pick one (or none) interactively
       hiver pet use <id>         switch to that pet (built the first time, about a minute)
       hiver pet off              no pet
       hiver pet show             start the chosen pet (each hiver window does this; the pet
                                  quits itself when the last hiver window closes)
       hiver pet list [--json]    the pets
  Pets: hiver-h (Hiver H), hiver-dot (Hiver), hiver-prompt (Hiver Prompt). Also in the pet's
  right-click menu (Switch pet, Turn off pet), hiver settings → pets, and ⌥P.";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help" | "help"))
    {
        println!("{HELP}");
        return Ok(0);
    }
    let script = crate::hiver::repo().join("pets").join("pet.py");
    if !script.is_file() {
        eprintln!(
            "hiver pet: no {} — set HIVER_REPO to your hiver checkout",
            script.display()
        );
        return Ok(1);
    }
    let mut command = std::process::Command::new("python3");
    command.arg(&script).args(args);
    if let Ok(exe) = std::env::current_exe() {
        command.env("HIVER_BIN", exe);
    }
    match command.status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("hiver pet: cannot run {}: {err}", script.display());
            Ok(1)
        }
    }
}
