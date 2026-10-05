//! `hivey pet`: the desktop pet (macOS). The work is done by `pets/pet.py` in the hivey repo,
//! next to the pets' sources: choose, build, start, switch, turn off.

const HELP: &str = "\
usage: hivey pet [status]         your pet (running or not) and the pets to choose from
       hivey pet choose           pick one (or none) interactively
       hivey pet use <id>         switch to that pet (built the first time, about a minute)
       hivey pet off              no pet
       hivey pet show             start the chosen pet (each hivey window does this; the pet
                                  quits itself when the last hivey window closes)
       hivey pet list [--json]    the pets
  Pets: hivey-h (Hivey), hivey-dot (Hivey System), hivey-prompt (Hivey Prompt). Also in the pet's
  right-click menu (Switch pet, Turn off pet), hivey settings → pets, and ⌥P.";

pub(in crate::cli) fn run(args: &[String]) -> std::io::Result<i32> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help" | "help"))
    {
        println!("{HELP}");
        return Ok(0);
    }
    let script = crate::hivey::repo().join("pets").join("pet.py");
    if !script.is_file() {
        eprintln!(
            "hivey pet: no {} — set HIVEY_REPO to your hivey checkout",
            script.display()
        );
        return Ok(1);
    }
    let mut command = std::process::Command::new("python3");
    command.arg(&script).args(args);
    if let Ok(exe) = std::env::current_exe() {
        command.env("HIVEY_BIN", exe);
    }
    match command.status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("hivey pet: cannot run {}: {err}", script.display());
            Ok(1)
        }
    }
}
