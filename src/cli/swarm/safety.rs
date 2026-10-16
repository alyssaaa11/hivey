//! `hivey safety`: hivey's Claude Code hooks (`hooks/` in the repo, embedded in the binary).
//!
//! - `hivey-safety.py` and `hivey-secret-scan.py` (PreToolUse): the safety switch. On by
//!   default; `hivey safety off` unregisters them (the files stay). The choice lives in
//!   `~/.hivey/safety.json` and updates keep it.
//! - `hivey-agent-stop.py` (Stop): speaks each agent's `DONE:` line with `hivey voice say`.
//!
//! The hooks go to `~/.claude/hooks/hivey-*` and are registered in `~/.claude/settings.json`,
//! edited in place (the user's other settings, order and formatting are kept). The hooks they
//! replace (an older damage-control, `secret_scan.py`, `agents_stop.py`) are unregistered and
//! moved to `~/.hivey/old/`. Run by `hivey migrate`, so install and every update apply it.

use std::io;
use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstInputValue, CstRootNode};
use jsonc_parser::{json, ParseOptions};

const USAGE: &str = "usage: hivey safety            whether the safety hooks are on
       hivey safety on|off     turn them on or off (Claude Code: ~/.claude/settings.json)
  The safety hooks block destructive commands (rm -rf on whole folders, wiping disks, git
  history rewrites, curl | sh), reading or writing secrets (~/.ssh, .env, credentials) and
  editing lockfiles by hand, and scan git commits for secrets. They are on by default; agents
  that hit one are told to ask the user. Also in hivey settings → safety. HIVEY_SAFETY=off
  in an agent's environment skips them for that session only.";

/// The hooks hivey installs: file name, contents.
const HOOKS: &[(&str, &str)] = &[
    (
        "hivey-safety.py",
        include_str!("../../../hooks/hivey-safety.py"),
    ),
    (
        "hivey-secret-scan.py",
        include_str!("../../../hooks/hivey-secret-scan.py"),
    ),
    (
        "hivey-agent-stop.py",
        include_str!("../../../hooks/hivey-agent-stop.py"),
    ),
];

/// Tools the safety hook checks besides Bash.
const FILE_TOOLS: &str = "Edit|MultiEdit|Write|NotebookEdit|Read";

/// Older hooks the hivey ones replace: unregistered (any command that mentions them) and
/// their files moved to ~/.hivey/old/.claude/hooks/.
const REPLACED: &[&str] = &["damage-control", "secret_scan.py", "agents_stop.py"];

/// Whether the safety hooks should be on (default: on).
pub(crate) fn enabled(home: &Path) -> bool {
    crate::swarm::safety::enabled_in(home)
}

fn set_enabled(home: &Path, on: bool) -> io::Result<()> {
    let path = crate::swarm::safety::state_path(home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, format!("{{\"enabled\": {on}}}\n"))
}

pub(crate) fn run(args: &[String]) -> io::Result<i32> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("hivey safety: HOME is not set");
        return Ok(1);
    };
    match args.first().map(String::as_str) {
        None | Some("status") => {
            let on = enabled(&home);
            let registered = registered(&home);
            println!(
                "safety: {}{}",
                if on { "on" } else { "off" },
                match (on, registered) {
                    (true, false) => " (not registered yet: run hivey safety on)",
                    (false, true) => " (still registered: run hivey safety off)",
                    _ => "",
                }
            );
            Ok(0)
        }
        Some(state @ ("on" | "off")) => {
            set_enabled(&home, state == "on")?;
            for line in apply(&home, false)? {
                println!("{line}");
            }
            println!("safety: {state}");
            Ok(0)
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(0)
        }
        Some(other) => {
            eprintln!("unknown safety command {other:?}\n{USAGE}");
            Ok(2)
        }
    }
}

/// Whether hivey's safety hook is registered in ~/.claude/settings.json.
pub(crate) fn registered(home: &Path) -> bool {
    std::fs::read_to_string(home.join(".claude/settings.json"))
        .is_ok_and(|text| text.contains("hivey-safety.py"))
}

/// Installs hivey's hooks and registers them as `enabled` says; what changed, for the report.
pub(super) fn apply(home: &Path, dry: bool) -> io::Result<Vec<String>> {
    let claude = home.join(".claude");
    if !claude.is_dir() {
        return Ok(Vec::new());
    }
    let hooks_dir = claude.join("hooks");
    let mut report = Vec::new();
    for (name, content) in HOOKS {
        let path = hooks_dir.join(name);
        let current = std::fs::read_to_string(&path);
        if current.as_ref().is_ok_and(|text| text == content) {
            continue;
        }
        report.push(format!(
            "    {:<8} {}",
            if current.is_ok() { "updated" } else { "new" },
            super::skill::show(&path)
        ));
        if !dry {
            std::fs::create_dir_all(&hooks_dir)?;
            std::fs::write(&path, content)?;
        }
    }

    let settings = claude.join("settings.json");
    let original = std::fs::read_to_string(&settings).unwrap_or_default();
    let updated = register(&original, &hooks_dir, enabled(home))?;
    if updated != original {
        report.push(format!(
            "    updated  {} (hooks: safety {})",
            super::skill::show(&settings),
            if enabled(home) { "on" } else { "off" }
        ));
        if !dry {
            std::fs::write(&settings, &updated)?;
        }
    }

    // The files of the hooks just replaced, out of the way (moved, never deleted).
    for name in REPLACED {
        let path = hooks_dir.join(name);
        if std::fs::symlink_metadata(&path).is_err() || updated.contains(&format!("/{name}")) {
            continue;
        }
        let mut dest = home.join(".hivey/old/.claude/hooks").join(name);
        let mut n = 1;
        while std::fs::symlink_metadata(&dest).is_ok() {
            n += 1;
            dest = home
                .join(".hivey/old/.claude/hooks")
                .join(format!("{name}.{n}"));
        }
        report.push(format!(
            "old hook {} → {} (now hivey's)",
            super::skill::show(&path),
            super::skill::show(&dest)
        ));
        if !dry {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&path, &dest)?;
        }
    }
    Ok(report)
}

fn hook_command(hooks_dir: &Path, name: &str) -> String {
    format!("python3 \"{}\"", hooks_dir.join(name).display())
}

/// `settings.json` with hivey's hook entries as wanted: every command that runs one of
/// hivey's hooks or one it replaces is removed, then the wanted ones are added. Returns the
/// content unchanged when that changes nothing.
fn register(content: &str, hooks_dir: &Path, safety_on: bool) -> io::Result<String> {
    let base = if content.trim().is_empty() {
        "{}\n"
    } else {
        content
    };
    let root = CstRootNode::parse(base, &ParseOptions::default())
        .map_err(|err| io::Error::other(format!("cannot read ~/.claude/settings.json: {err}")))?;
    let before = root.to_serde_value();
    let ours = |command: &str| {
        HOOKS
            .iter()
            .map(|(name, _)| *name)
            .chain(REPLACED.iter().copied())
            .any(|name| command.contains(&format!("/{name}")))
    };
    let object = root.object_value_or_set();
    let hooks = object.object_value_or_set("hooks");
    for event in hooks.properties() {
        let Some(entries) = event.array_value() else {
            continue;
        };
        for entry in entries.elements() {
            let Some(commands) = entry
                .as_object()
                .and_then(|entry| entry.get("hooks"))
                .and_then(|prop| prop.array_value())
            else {
                continue;
            };
            for command in commands.elements() {
                let text = command
                    .to_serde_value()
                    .and_then(|value| value["command"].as_str().map(str::to_string));
                if text.is_some_and(|text| ours(&text)) {
                    command.remove();
                }
            }
            if commands.elements().is_empty() {
                entry.remove();
            }
        }
        if entries.elements().is_empty() {
            event.remove();
        }
    }
    let add = |event: &str, matcher: Option<&str>, names: &[&str]| {
        let commands: Vec<CstInputValue> = names
            .iter()
            .map(|name| {
                let command = hook_command(hooks_dir, name);
                json!({ "type": "command", command: command, timeout: 10u64 })
            })
            .collect();
        let entry = match matcher {
            Some(matcher) => json!({ matcher: matcher, hooks: (CstInputValue::Array(commands)) }),
            None => json!({ hooks: (CstInputValue::Array(commands)) }),
        };
        hooks.array_value_or_set(event).append(entry);
    };
    if safety_on {
        add(
            "PreToolUse",
            Some("Bash"),
            &["hivey-safety.py", "hivey-secret-scan.py"],
        );
        add("PreToolUse", Some(FILE_TOOLS), &["hivey-safety.py"]);
    }
    add("Stop", None, &["hivey-agent-stop.py"]);
    let after = root.to_serde_value();
    if after == before && !content.trim().is_empty() {
        return Ok(content.to_string());
    }
    let mut text = root.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::Value;

    fn value(text: &str) -> Value {
        serde_json::from_str(text).expect("json")
    }

    fn commands(settings: &Value, event: &str) -> Vec<String> {
        settings["hooks"][event]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|entry| entry["hooks"].as_array().cloned().unwrap_or_default())
            .filter_map(|hook| hook["command"].as_str().map(str::to_string))
            .collect()
    }

    const USER: &str = r#"{
  "model": "sonnet",
  "hooks": {
    "SessionStart": [
      { "matcher": "*", "hooks": [{ "type": "command", "command": "bash '/h/.claude/hooks/herdr-agent-state.sh' session" }] }
    ],
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "/h/.claude/hooks/secret_scan.py" }] },
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "uv run \"/h/.claude/hooks/damage-control/bash-tool-damage-control.py\"" }] },
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "my-own-check" }] }
    ],
    "Stop": [ { "hooks": [{ "type": "command", "command": "/h/.claude/hooks/agents_stop.py" }] } ]
  },
  "enabledPlugins": {}
}
"#;

    #[test]
    fn replaces_old_hooks_and_keeps_the_users_own() {
        let dir = Path::new("/h/.claude/hooks");
        let out = register(USER, dir, true).expect("register");
        let settings = value(&out);
        let pre = commands(&settings, "PreToolUse");
        assert!(pre.contains(&"my-own-check".to_string()), "{pre:?}");
        assert!(!pre
            .iter()
            .any(|c| c.contains("damage-control") || c.contains("secret_scan.py")));
        assert_eq!(
            pre.iter().filter(|c| c.contains("hivey-safety.py")).count(),
            2
        );
        assert!(pre.iter().any(|c| c.contains("hivey-secret-scan.py")));
        assert_eq!(
            commands(&settings, "Stop"),
            vec![r#"python3 "/h/.claude/hooks/hivey-agent-stop.py""#.to_string()]
        );
        assert_eq!(settings["model"], "sonnet");
        assert!(commands(&settings, "SessionStart")[0].contains("herdr-agent-state.sh"));
        // The user's key order stays.
        assert!(out.find("\"model\"") < out.find("\"hooks\""));
        assert!(out.find("\"hooks\"") < out.find("\"enabledPlugins\""));
        // Running again changes nothing.
        assert_eq!(register(&out, dir, true).expect("again"), out);
    }

    #[test]
    fn off_keeps_the_voice_and_drops_the_safety_hooks() {
        let dir = Path::new("/h/.claude/hooks");
        let on = register(USER, dir, true).expect("on");
        let off = value(&register(&on, dir, false).expect("off"));
        let pre = commands(&off, "PreToolUse");
        assert_eq!(pre, vec!["my-own-check".to_string()]);
        assert_eq!(commands(&off, "Stop").len(), 1);
    }

    #[test]
    fn starts_from_no_settings() {
        let out = register("", Path::new("/h/.claude/hooks"), true).expect("empty");
        assert_eq!(commands(&value(&out), "PreToolUse").len(), 3);
    }

    #[test]
    fn apply_installs_files_and_moves_the_replaced_ones() {
        let home = std::env::temp_dir().join(format!("hivey-safety-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let hooks = home.join(".claude/hooks");
        std::fs::create_dir_all(hooks.join("damage-control")).expect("dirs");
        std::fs::write(hooks.join("secret_scan.py"), "old").expect("old");
        std::fs::write(
            home.join(".claude/settings.json"),
            USER.replace("/h/", &format!("{}/", home.display())),
        )
        .expect("settings");
        let report = apply(&home, false).expect("apply");
        assert!(hooks.join("hivey-safety.py").is_file(), "{report:?}");
        assert!(!hooks.join("secret_scan.py").exists());
        assert!(home
            .join(".hivey/old/.claude/hooks/damage-control")
            .is_dir());
        assert!(registered(&home));
        set_enabled(&home, false).expect("off");
        apply(&home, false).expect("apply off");
        assert!(!registered(&home));
        assert!(
            hooks.join("hivey-safety.py").is_file(),
            "files stay when off"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn every_hook_is_named_hivey_dash() {
        for (name, _) in HOOKS {
            assert!(name.starts_with("hivey-"), "{name}");
        }
    }
}
