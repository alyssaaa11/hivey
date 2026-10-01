//! `hiver skill …`: the hiver skill for AI coding agents (Claude Code, Codex), embedded in the
//! binary so every machine with hiver can install it (`skills/hiver/` in the repo).

use std::path::{Path, PathBuf};

const FILES: &[(&str, &str)] = &[
    ("SKILL.md", include_str!("../../../skills/hiver/SKILL.md")),
    (
        "scripts/handoff.py",
        include_str!("../../../skills/hiver/scripts/handoff.py"),
    ),
];

const USAGE: &str = "hiver skill commands:
  hiver skill install [--claude] [--codex] [--dir DIR]
                       install the hiver skill for Claude Code (~/.claude/skills/hiver) and
                       Codex (~/.codex/skills/hiver); default: each one that is set up
  hiver skill print    print SKILL.md";

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
                targets.push(("Claude Code".into(), home.join(".claude/skills/hiver")));
            }
            "--codex" => {
                explicit = true;
                targets.push(("Codex".into(), home.join(".codex/skills/hiver")));
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
    for (label, dir) in targets {
        println!("{label}: {}", install_into(&dir)?);
    }
    Ok(0)
}

/// Claude Code and Codex, when their home folder exists.
pub(super) fn default_targets(home: &Path) -> Vec<(String, PathBuf)> {
    [("Claude Code", ".claude"), ("Codex", ".codex")]
        .into_iter()
        .filter(|(_, app)| home.join(app).is_dir())
        .map(|(label, app)| (label.to_string(), home.join(app).join("skills/hiver")))
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
    let mut changed = 0;
    for (rel, content) in FILES {
        let path = dir.join(rel);
        if std::fs::read_to_string(&path).is_ok_and(|current| current == *content) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)?;
        changed += 1;
    }
    Ok(match changed {
        0 => format!("{} is up to date", dir.display()),
        _ => format!("installed {}", dir.display()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hiver-skill-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn installs_then_is_up_to_date() {
        let tmp = scratch("install");
        let dir = tmp.join("skills/hiver");
        assert!(install_into(&dir)
            .expect("install")
            .starts_with("installed"));
        assert!(dir.join("SKILL.md").is_file());
        assert!(dir.join("scripts/handoff.py").is_file());
        assert!(install_into(&dir).expect("again").ends_with("up to date"));
    }

    #[test]
    fn targets_only_apps_that_exist() {
        let tmp = scratch("targets");
        assert!(default_targets(&tmp).is_empty());
        std::fs::create_dir(tmp.join(".codex")).expect("mkdir");
        let targets = default_targets(&tmp);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].1.ends_with(".codex/skills/hiver"));
    }

    #[test]
    fn skill_has_frontmatter() {
        assert!(FILES[0].1.starts_with("---\nname: hiver\n"));
    }
}
