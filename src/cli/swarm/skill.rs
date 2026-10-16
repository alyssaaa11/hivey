//! `hivey skill …`: the hivey skill for AI coding agents (Claude Code, Codex), embedded in the
//! binary so every machine with hivey can install it (`skills/hivey/` in the repo), with the
//! `hivey-*` skills next to it (`skills/hivey-*/`) and the wiki commands.

use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../../../skills/hivey/SKILL.md")),
    (
        "scripts/handoff.py",
        include_str!("../../../skills/hivey/scripts/handoff.py"),
    ),
    (
        "scripts/new_wiki.py",
        include_str!("../../../skills/hivey/scripts/new_wiki.py"),
    ),
];

/// The other skills hivey ships, installed next to the hivey skill (`skills/<name>`): Claude
/// Code only finds skills one level deep, so they are `hivey-<name>`, never nested.
const SKILLS: &[(&str, &[(&str, &str)])] = &[
    (
        "hivey-create-wiki",
        &[(
            "SKILL.md",
            include_str!("../../../skills/hivey-create-wiki/SKILL.md"),
        )],
    ),
    // Current library docs in seconds, from any agent (no MCP server needed).
    (
        "hivey-context7",
        &[(
            "SKILL.md",
            include_str!("../../../skills/hivey-context7/SKILL.md"),
        )],
    ),
    // How to run each coding agent hivey supports besides Claude Code, headless.
    (
        "hivey-codex",
        &[(
            "SKILL.md",
            include_str!("../../../skills/hivey-codex/SKILL.md"),
        )],
    ),
    (
        "hivey-agy",
        &[
            (
                "SKILL.md",
                include_str!("../../../skills/hivey-agy/SKILL.md"),
            ),
            (
                "references/gemini-prompting.md",
                include_str!("../../../skills/hivey-agy/references/gemini-prompting.md"),
            ),
            (
                "scripts/agy_run.py",
                include_str!("../../../skills/hivey-agy/scripts/agy_run.py"),
            ),
        ],
    ),
    (
        "hivey-grok",
        &[
            (
                "SKILL.md",
                include_str!("../../../skills/hivey-grok/SKILL.md"),
            ),
            (
                "scripts/grok_run.py",
                include_str!("../../../skills/hivey-grok/scripts/grok_run.py"),
            ),
        ],
    ),
];

/// Commands earlier versions installed that a skill of the same name replaced.
const RETIRED_COMMANDS: &[&str] = &["hivey-create-wiki.md"];

/// Claude Code slash commands for an agent's or swarm's wiki, installed into ~/.claude/commands.
/// Everything hivey installs is named `hivey-…` (Claude Code ignores skills in subfolders).
const COMMANDS: &[(&str, &str)] = &[
    (
        "hivey-wiki-ingest.md",
        include_str!("../../../skills/hivey/commands/hivey-wiki-ingest.md"),
    ),
    (
        "hivey-wiki-lint.md",
        include_str!("../../../skills/hivey/commands/hivey-wiki-lint.md"),
    ),
    (
        "hivey-wiki-list.md",
        include_str!("../../../skills/hivey/commands/hivey-wiki-list.md"),
    ),
    (
        "hivey-wiki-log.md",
        include_str!("../../../skills/hivey/commands/hivey-wiki-log.md"),
    ),
    (
        "hivey-wiki-query.md",
        include_str!("../../../skills/hivey/commands/hivey-wiki-query.md"),
    ),
];

const USAGE: &str = "hivey skill commands:
  hivey skill install [--claude] [--codex] [--dir DIR]
                       install the hivey skill for Claude Code (~/.claude/skills/hivey) and
                       Codex (~/.codex/skills/hivey); default: each one that is set up.
                       (and the hivey-create-wiki skill next to it); Claude Code also gets
                       the wiki commands (/hivey-wiki-ingest, …) in ~/.claude/commands; the default install
                       also brings an older install up to date (hivey migrate)
  hivey skill print    print SKILL.md";

pub(crate) fn run_skill_command(args: &[String]) -> std::io::Result<i32> {
    match args.first().map(String::as_str) {
        Some("install") => install(&args[1..]),
        Some("print") => {
            print!("{}", FILES[0].1);
            Ok(0)
        }
        Some("--help" | "-h" | "help") | None => {
            println!("{USAGE}");
            Ok(0)
        }
        Some(other) => {
            eprintln!("unknown skill command {other:?}\n{USAGE}");
            Ok(2)
        }
    }
}

fn install(args: &[String]) -> std::io::Result<i32> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let mut targets: Vec<(String, PathBuf)> = Vec::new();
    let mut explicit = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--claude" => {
                explicit = true;
                targets.push(("Claude Code".into(), home.join(".claude/skills/hivey")));
            }
            "--codex" => {
                explicit = true;
                targets.push(("Codex".into(), home.join(".codex/skills/hivey")));
            }
            "--dir" => {
                explicit = true;
                let Some(dir) = rest.next() else {
                    eprintln!("error: --dir needs a folder");
                    return Ok(2);
                };
                targets.push((dir.clone(), PathBuf::from(dir)));
            }
            other => {
                eprintln!("error: unknown option {other:?}\n{USAGE}");
                return Ok(2);
            }
        }
    }
    if !explicit {
        targets = default_targets(&home);
        if targets.is_empty() {
            println!("no ~/.claude or ~/.codex found; use --claude, --codex or --dir DIR");
            return Ok(0);
        }
    }
    let claude_skill = home.join(".claude/skills/hivey");
    let mut commands = false;
    for (label, dir) in targets {
        println!("{label}: {}", install_into(&dir)?);
        commands |= dir == claude_skill;
    }
    if commands {
        println!(
            "Claude Code commands: {}",
            install_commands(&home.join(".claude/commands"))?
        );
    }
    // The default install also brings an older install up to date (`hivey migrate`): older
    // `hivey update` scripts only call `hivey skill install` with the new binary.
    if !explicit {
        super::migrate::leftovers(&home, false)?;
    }
    Ok(0)
}

/// Claude Code and Codex, when their home folder exists.
pub(super) fn default_targets(home: &Path) -> Vec<(String, PathBuf)> {
    [("Claude Code", ".claude"), ("Codex", ".codex")]
        .into_iter()
        .filter(|(_, app)| home.join(app).is_dir())
        .map(|(label, app)| (label.to_string(), home.join(app).join("skills/hivey")))
        .collect()
}

/// Write the skill into `dir`. A symlinked skill (a dev checkout) is left alone.
pub(super) fn install_into(dir: &Path) -> std::io::Result<String> {
    if std::fs::symlink_metadata(dir).is_ok_and(|meta| meta.file_type().is_symlink()) {
        let target = std::fs::read_link(dir).unwrap_or_default();
        return Ok(format!(
            "{} links to {} (left as is)",
            dir.display(),
            target.display()
        ));
    }
    let mut changes = write_files(dir, FILES)?;
    // Next to the hivey skill in a skills folder (~/.claude/skills/hivey): the hivey-* skills.
    if let (Some(skills), Some("hivey")) = (dir.parent(), dir.file_name().and_then(|n| n.to_str()))
    {
        for (name, files) in SKILLS {
            changes.extend(write_files(&skills.join(name), files)?);
        }
    }
    Ok(match changes.is_empty() {
        true => format!("{} is up to date", show(dir)),
        false => format!("installed {}{}", show(dir), list(&changes)),
    })
}

/// Write the wiki slash commands into `dir` (Claude Code's ~/.claude/commands).
pub(super) fn install_commands(dir: &Path) -> std::io::Result<String> {
    let mut changes = Vec::new();
    for name in RETIRED_COMMANDS {
        let retired = dir.join(name);
        if retired.is_file() {
            std::fs::remove_file(&retired)?;
            changes.push((retired, "removed"));
        }
    }
    let written = write_files(dir, COMMANDS)?;
    let count = written.len();
    changes.extend(written);
    Ok(match (count, changes.is_empty()) {
        (_, true) => format!("{} is up to date", show(dir)),
        (n, false) => format!("{n} installed in {}{}", show(dir), list(&changes)),
    })
}

/// Write each file that is missing or different; what changed: (path, "new" | "updated").
fn write_files(
    dir: &Path,
    files: &[(&str, &str)],
) -> std::io::Result<Vec<(PathBuf, &'static str)>> {
    let mut changes = Vec::new();
    for (rel, content) in files {
        let path = dir.join(rel);
        let current = std::fs::read_to_string(&path);
        if current.as_ref().is_ok_and(|current| current == content) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        changes.push((path, if current.is_ok() { "updated" } else { "new" }));
    }
    Ok(changes)
}

/// One line per changed file, so an install shows exactly what it put on the computer.
fn list(changes: &[(PathBuf, &str)]) -> String {
    changes
        .iter()
        .map(|(path, what)| format!("\n    {what:<8} {}", show(path)))
        .collect()
}

/// A path with the home folder as `~`.
pub(super) fn show(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => match path.strip_prefix(&home) {
            Ok(rel) => format!("~/{}", rel.display()),
            Err(_) => path.display().to_string(),
        },
        _ => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hivey-skill-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn installs_then_is_up_to_date() {
        let tmp = scratch("install");
        let dir = tmp.join("skills/hivey");
        assert!(install_into(&dir)
            .expect("install")
            .starts_with("installed"));
        assert!(dir.join("SKILL.md").is_file());
        assert!(dir.join("scripts/handoff.py").is_file());
        assert!(dir.join("scripts/new_wiki.py").is_file());
        assert!(tmp.join("skills/hivey-create-wiki/SKILL.md").is_file());
        assert!(install_into(&dir).expect("again").ends_with("up to date"));
    }

    #[test]
    fn targets_only_apps_that_exist() {
        let tmp = scratch("targets");
        assert!(default_targets(&tmp).is_empty());
        std::fs::create_dir(tmp.join(".codex")).expect("mkdir");
        let targets = default_targets(&tmp);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].1.ends_with(".codex/skills/hivey"));
    }

    #[test]
    fn installs_wiki_commands() {
        let tmp = scratch("commands");
        let commands = tmp.join(".claude/commands");
        std::fs::create_dir_all(&commands).expect("mkdir");
        std::fs::write(commands.join("hivey-create-wiki.md"), "old").expect("retired");
        assert!(install_commands(&commands)
            .expect("install")
            .starts_with("5 installed"));
        assert!(
            !commands.join("hivey-create-wiki.md").exists(),
            "retired: a skill now"
        );
        for (name, _) in COMMANDS {
            assert!(commands.join(name).is_file(), "{name}");
        }
        assert!(install_commands(&commands)
            .expect("again")
            .ends_with("up to date"));
    }

    #[test]
    fn skills_are_named_hivey_dash() {
        for (name, files) in SKILLS {
            assert!(name.starts_with("hivey-"), "{name}");
            let skill = files
                .iter()
                .find(|(rel, _)| *rel == "SKILL.md")
                .expect("SKILL.md");
            assert!(
                skill.1.starts_with(&format!("---\nname: {name}\n")),
                "{name}"
            );
            let commands: Vec<&str> = COMMANDS.iter().map(|(file, _)| *file).collect();
            assert!(
                !commands.contains(&format!("{name}.md").as_str()),
                "{name}: also a command"
            );
        }
    }

    #[test]
    fn commands_have_frontmatter() {
        for (name, content) in COMMANDS {
            let slug = name.trim_end_matches(".md");
            assert!(
                content.starts_with(&format!("---\nname: {slug}\n")),
                "{name}"
            );
            assert!(
                slug.starts_with("hivey-"),
                "{name}: installed names start with hivey-"
            );
        }
    }

    #[test]
    fn skill_has_frontmatter() {
        assert!(FILES[0].1.starts_with("---\nname: hivey\n"));
    }
}
