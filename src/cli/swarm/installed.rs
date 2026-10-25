//! `hivey installed`: everything hivey has put on this computer, so the user can see what came
//! with it (shown at the end of `install.sh`). Everything hivey installs for Claude Code and
//! Codex is named `hivey…`, so this lists by that name.

use std::path::{Path, PathBuf};

const USAGE: &str = "usage: hivey installed
  list what hivey has installed on this computer: the hivey program, skills, commands, hooks,
  agents and plugins (with what each is for), and the folders hivey keeps its data in";

pub(crate) fn run(args: &[String]) -> std::io::Result<i32> {
    if let Some(arg) = args.first() {
        let help = matches!(arg.as_str(), "-h" | "--help" | "help");
        if !help {
            eprintln!("unknown option {arg:?}");
        }
        println!("{USAGE}");
        return Ok(if help { 0 } else { 2 });
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("hivey installed: HOME is not set");
        return Ok(1);
    };
    let config_dir = crate::config::config_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    print!("{}", inventory(&home, &config_dir));
    Ok(0)
}

fn inventory(home: &Path, config_dir: &Path) -> String {
    let show = |path: &Path| match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    };
    let mut out = String::new();
    let mut section = |title: &str, lines: Vec<String>, none: &str| {
        out.push_str(&format!("\n{title}\n"));
        if lines.is_empty() {
            out.push_str(&format!("  ({none})\n"));
        }
        for line in lines {
            out.push_str(&format!("  {line}\n"));
        }
    };

    let exe = std::env::current_exe().unwrap_or_default();
    let commit = std::fs::read_to_string(home.join(".local/state/hivey/installed-commit"))
        .map(|c| format!(" ({})", c.trim()))
        .unwrap_or_default();
    section(
        "hivey",
        vec![format!(
            "{}  hivey {}{commit}",
            show(&exe),
            env!("CARGO_PKG_VERSION")
        )],
        "",
    );

    let mut skills = Vec::new();
    for (app, dir) in [
        ("Claude Code", ".claude/skills"),
        ("Codex", super::skill::CODEX_SKILLS),
    ] {
        for path in hivey_entries(&home.join(dir), "") {
            let about = describe(&path.join("SKILL.md"));
            skills.push(format!("{:<22} {app:<11} {about}", file_name(&path)));
        }
    }
    section("Skills", skills, "none");

    // Not named hivey: install.sh installs these for the creators and agents to use.
    let path_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    let tools = [
        (
            Some("skylls"),
            "skylls",
            "skills library: find, save and share skills, agents, swarms",
        ),
        (
            Some("envsave"),
            "envsave",
            "secrets by opaque id, never in code",
        ),
        (None, "find-skills", "find skills on skills.sh"),
    ]
    .into_iter()
    .map(|(program, skill, what)| {
        let program = program.map(|name| {
            if path_dirs.iter().any(|dir| dir.join(name).is_file()) {
                format!("{name} found")
            } else {
                format!("{name} MISSING")
            }
        });
        let skill_state = if home
            .join(".claude/skills")
            .join(skill)
            .join("SKILL.md")
            .is_file()
        {
            "skill on"
        } else {
            "skill MISSING"
        };
        let state = match program {
            Some(program) => format!("{program}, {skill_state}"),
            None => skill_state.to_string(),
        };
        format!("{skill:<22} {state:<30} {what}")
    })
    .collect();
    section(
        "Tools and their skills (Claude Code; optional: rerun hivey setup or ./install.sh to add)",
        tools,
        "",
    );

    let commands = hivey_entries(&home.join(".claude/commands"), ".md")
        .into_iter()
        .map(|path| {
            let name = format!("/{}", file_name(&path).trim_end_matches(".md"));
            format!("{name:<22} {}", describe(&path))
        })
        .collect();
    section("Commands (Claude Code)", commands, "none");

    let settings = std::fs::read_to_string(home.join(".claude/settings.json")).unwrap_or_default();
    let codex_hooks = std::fs::read_to_string(home.join(".codex/hooks.json")).unwrap_or_default();
    let mut hooks = Vec::new();
    let mut hook_paths = hivey_entries(&home.join(".claude/hooks"), "");
    // The agent-state hook hivey's integration installs (named after herdr, which hivey forks).
    hook_paths.push(home.join(".claude/hooks/herdr-agent-state.sh"));
    for path in hook_paths.into_iter().filter(|p| p.exists()) {
        let name = file_name(&path);
        let on = if settings.contains(&name) {
            "on"
        } else {
            "not registered"
        };
        hooks.push(format!("{:<34} Claude Code  {on}", show(&path)));
    }
    let codex = home.join(".codex/herdr-agent-state.sh");
    if codex.exists() {
        let on = if codex_hooks.contains("herdr-agent-state") {
            "on"
        } else {
            "not registered"
        };
        hooks.push(format!("{:<34} Codex        {on}", show(&codex)));
    }
    hooks.insert(
        0,
        format!(
            "safety: {} (hivey safety on|off, settings → safety)",
            if crate::swarm::safety::enabled_in(home) {
                "on"
            } else {
                "off"
            }
        ),
    );
    section("Hooks (Claude Code / Codex)", hooks, "none");

    let agents = hivey_entries(&home.join(".claude/agents"), ".md")
        .into_iter()
        .map(|path| {
            let name = file_name(&path).trim_end_matches(".md").to_string();
            format!("{name:<22} {}", describe(&path))
        })
        .collect();
    section("Agents (Claude Code subagents)", agents, "none");

    let registry = std::fs::read_to_string(config_dir.join("plugins.json")).unwrap_or_default();
    let plugins: Vec<serde_json::Value> = serde_json::from_str(&registry).unwrap_or_default();
    let plugins = plugins
        .iter()
        .map(|plugin| {
            let id = plugin["plugin_id"].as_str().unwrap_or("?");
            let name = plugin["name"].as_str().unwrap_or("");
            let off = if plugin["enabled"] == false {
                "  (off)"
            } else {
                ""
            };
            let root = plugin["plugin_root"].as_str().map(PathBuf::from);
            let from = root.map(|r| format!("  {}", show(&r))).unwrap_or_default();
            format!("{id:<22} {name}{off}{from}")
        })
        .collect();
    section("Plugins (hivey plugin list)", plugins, "none linked");

    let folders = [
        (
            home.join(".hivey"),
            "the hivey agent, settings, Slack token, old copies",
        ),
        (
            config_dir.to_path_buf(),
            "config.toml, sessions, plugins, logs",
        ),
        (
            home.join(".local/state/hivey"),
            "which version is installed",
        ),
    ]
    .into_iter()
    .filter(|(path, _)| path.is_dir())
    .map(|(path, what)| format!("{:<22} {what}", show(&path)))
    .collect();
    section("Data folders", folders, "none");
    out
}

/// `hivey` and `hivey-*` entries of a folder (with `ext`, e.g. ".md"), sorted.
fn hivey_entries(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let name = file_name(path);
            let stem = name
                .strip_suffix(ext)
                .unwrap_or(if ext.is_empty() { &name } else { "" });
            (stem == "hivey" || stem.starts_with("hivey-")) && name.ends_with(ext)
        })
        .collect();
    found.sort();
    found
}

/// The first sentence of a skill's or command's `description:` (folded `>` ones too).
fn describe(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines = text.lines();
    let mut description = String::new();
    while let Some(line) = lines.next() {
        let Some(value) = line.strip_prefix("description:") else {
            continue;
        };
        let value = value.trim();
        if matches!(value, ">" | "|" | ">-" | "|-" | "") {
            for more in lines.by_ref() {
                if !more.starts_with(' ') {
                    break;
                }
                description.push_str(more.trim());
                description.push(' ');
            }
        } else {
            description = value.to_string();
        }
        break;
    }
    let description = description.trim();
    let first = description
        .split_once(". ")
        .map(|(first, _)| first)
        .unwrap_or(description);
    let mut first: String = first.trim_end_matches('.').chars().take(100).collect();
    if first.chars().count() == 100 {
        first.push('…');
    }
    first
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_hivey_things_with_their_purpose() {
        let home = std::env::temp_dir().join(format!("hivey-installed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        for dir in ["hivey", "hivey-create-wiki", "my-skill"] {
            let skill = home.join(".claude/skills").join(dir);
            std::fs::create_dir_all(&skill).expect("skill");
            std::fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {dir}\ndescription: >\n  Does {dir} things. More.\n---\n"),
            )
            .expect("SKILL.md");
        }
        let commands = home.join(".claude/commands");
        std::fs::create_dir_all(&commands).expect("commands");
        std::fs::write(
            commands.join("hivey-wiki-log.md"),
            "---\nname: hivey-wiki-log\ndescription: Show the log. Usage: x\n---\n",
        )
        .expect("command");
        std::fs::write(commands.join("mine.md"), "---\nname: mine\n---\n").expect("command");
        let config = home.join(".config/hivey");
        std::fs::create_dir_all(&config).expect("config");
        std::fs::write(
            config.join("plugins.json"),
            r#"[{"plugin_id":"hivey.dashboard","name":"Dashboard","enabled":true}]"#,
        )
        .expect("plugins");

        let text = inventory(&home, &config);
        assert!(text.contains("hivey-create-wiki"), "{text}");
        assert!(text.contains("Does hivey-create-wiki things"), "{text}");
        assert!(!text.contains("my-skill"), "{text}");
        assert!(
            text.contains("/hivey-wiki-log        Show the log"),
            "{text}"
        );
        assert!(!text.contains("/mine"), "{text}");
        assert!(text.contains("hivey.dashboard"), "{text}");
        assert!(
            text.contains("find-skills            skill MISSING"),
            "{text}"
        );
        assert!(text.contains("envsave "), "{text}");
        assert!(
            text.contains("Agents (Claude Code subagents)\n  (none)"),
            "{text}"
        );
    }
}
