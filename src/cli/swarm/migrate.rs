//! `hivey migrate`: brings what an older hivey left on this machine up to the current
//! conventions. Run by `hivey update` and `install.sh`; safe to run any number of times.
//!
//! - the hivey skill and the `hivey-*` skills next to it (`hivey skill install`);
//! - plugins, old copies the `hivey-*` skills replaced, the hivey agent's brief and hooks.

use std::path::{Path, PathBuf};

const USAGE: &str = "usage: hivey migrate [--dry-run]
  bring an older hivey install up to date (run by hivey update; safe to run again):
  - the hivey skill and the hivey-* skills next to it (hivey-wiki-ingest, …)
  - old copies (wiki-ingest, …, agents-create-wiki, agy, grok, swarm, agent-creator) that hivey-* ones
    replaced, the hivey-wiki-* commands the wiki skills replaced and hivey skills in Codex's old
    ~/.codex/skills (now ~/.agents/skills): moved to ~/.hivey/old/, never deleted
  --dry-run   only show what would change";

pub(crate) fn run(args: &[String]) -> std::io::Result<i32> {
    let mut dry = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" | "-n" => dry = true,
            "-h" | "--help" | "help" => {
                println!("{USAGE}");
                return Ok(0);
            }
            other => {
                eprintln!("unknown option {other:?}\n{USAGE}");
                return Ok(2);
            }
        }
    }
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        eprintln!("hivey migrate: HOME is not set");
        return Ok(1);
    };
    if !dry {
        for (label, dir) in super::skill::default_targets(&home) {
            println!(
                "hivey skill for {label}: {}",
                super::skill::install_into(&dir)?
            );
        }
    }
    leftovers(&home, dry)?;
    Ok(0)
}

/// Plugins, old copies, the brief and hooks: the part of `hivey migrate` after the skills are installed. Also
/// run by `hivey skill install`, which update scripts of older versions call with the new
/// binary: so the first update from an older version migrates too.
pub(super) fn leftovers(home: &Path, dry: bool) -> std::io::Result<()> {
    // Plugins first: an old plugin is unlinked before the skill folder it lives in moves.
    for line in update_plugins(&crate::hivey::repo(), dry) {
        println!("{line}");
    }
    for line in retire_old_copies(home, dry)? {
        println!("{line}");
    }
    for line in update_home_brief(home, dry)? {
        println!("{line}");
    }
    // hivey's Claude Code hooks (safety switch, spoken summaries), replacing older ones.
    for line in super::safety::apply(home, dry)? {
        println!("{line}");
    }
    Ok(())
}

/// Copies older versions installed (or that hivey's skills needed) under names that are now
/// `hivey-*`: (path under HOME, the `name:` its frontmatter must have, what replaced it). Only a
/// file or folder with that name is touched, so someone's own `wiki-ingest` stays.
const OLD_COPIES: &[(&str, &str, &str)] = &[
    (
        ".claude/commands/create-wiki.md",
        "create-wiki",
        "the hivey-create-wiki skill",
    ),
    (
        ".claude/commands/wiki-ingest.md",
        "wiki-ingest",
        "/hivey-wiki-ingest",
    ),
    (
        ".claude/commands/wiki-lint.md",
        "wiki-lint",
        "/hivey-wiki-lint",
    ),
    (
        ".claude/commands/wiki-list.md",
        "wiki-list",
        "/hivey-wiki-list",
    ),
    (
        ".claude/commands/wiki-log.md",
        "wiki-log",
        "/hivey-wiki-log",
    ),
    (
        ".claude/commands/wiki-query.md",
        "wiki-query",
        "/hivey-wiki-query",
    ),
    (
        ".claude/skills/agents-create-wiki",
        "agents-create-wiki",
        "the hivey-create-wiki skill",
    ),
    (
        ".codex/skills/agents-create-wiki",
        "agents-create-wiki",
        "the hivey-create-wiki skill",
    ),
    (
        ".claude/skills/swarm",
        "swarm",
        "the hivey-swarm-creator skill",
    ),
    (
        ".codex/skills/swarm",
        "swarm",
        "the hivey-swarm-creator skill",
    ),
    (
        ".claude/skills/agent-creator",
        "agent-creator",
        "the hivey-agent-creator skill",
    ),
    (
        ".codex/skills/agent-creator",
        "agent-creator",
        "the hivey-agent-creator skill",
    ),
    (".claude/skills/agy", "agy", "the hivey-agy skill"),
    (".codex/skills/agy", "agy", "the hivey-agy skill"),
    (".claude/skills/grok", "grok", "the hivey-grok skill"),
    (".codex/skills/grok", "grok", "the hivey-grok skill"),
];

/// What hivey itself installed where it no longer does: (old path, the `name:` it must have, the
/// replacement), all under HOME. The `hivey-*` Claude Code commands are skills now (so Codex has
/// them too) and Codex reads user skills from ~/.agents/skills, no longer ~/.codex/skills.
fn replaced_installs() -> Vec<(String, String, String)> {
    let mut list = Vec::new();
    for name in super::skill::skill_names() {
        list.push((
            format!(".claude/commands/{name}.md"),
            name.to_string(),
            format!(".claude/skills/{name}"),
        ));
        list.push((
            format!(".codex/skills/{name}"),
            name.to_string(),
            format!("{}/{name}", super::skill::CODEX_SKILLS),
        ));
    }
    list
}

/// Where retired copies go: moved, never deleted, so nothing is lost.
const OLD_COPIES_DIR: &str = ".hivey/old";

/// Moves the old copies in `OLD_COPIES` into ~/.hivey/old/ (same relative path), and those in
/// `replaced_installs` once their replacement is installed (and isn't a link back to them).
fn retire_old_copies(home: &Path, dry: bool) -> std::io::Result<Vec<String>> {
    let mut report = Vec::new();
    let mut copies: Vec<(String, String, String)> = OLD_COPIES
        .iter()
        .map(|(rel, name, by)| (rel.to_string(), name.to_string(), by.to_string()))
        .collect();
    for (rel, name, new) in replaced_installs() {
        let new = home.join(new);
        let installed = new.join("SKILL.md").is_file();
        let links_back = match (new.canonicalize(), home.join(&rel).canonicalize()) {
            (Ok(new), Ok(old)) => new.starts_with(old),
            _ => false,
        };
        if installed && !links_back {
            let shown = new.strip_prefix(home).unwrap_or(&new).display().to_string();
            copies.push((rel, name, format!("~/{shown}")));
        }
    }
    for (rel, name, replaced_by) in &copies {
        let path = home.join(rel);
        if std::fs::symlink_metadata(&path).is_err() {
            continue;
        }
        let file = if path.is_dir() {
            path.join("SKILL.md")
        } else {
            path.clone()
        };
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        if !text.starts_with(&format!("---\nname: {name}\n")) {
            continue;
        }
        let mut dest = home.join(OLD_COPIES_DIR).join(rel);
        let mut n = 1;
        while std::fs::symlink_metadata(&dest).is_ok() {
            n += 1;
            dest = home.join(OLD_COPIES_DIR).join(format!("{rel}.{n}"));
        }
        report.push(format!(
            "old copy ~/{rel} → ~/{} (now {replaced_by})",
            dest.strip_prefix(home).unwrap_or(&dest).display()
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

/// Plugins older versions (or skills hivey replaced) linked, and the bundled one taking over.
const REPLACED_PLUGINS: &[(&str, &str)] = &[
    ("swarm.creator", "hivey.swarm-creator"),
    ("agent.creator", "hivey.agent-creator"),
    ("swarm.watcher", "hivey.swarm-watcher"),
];

/// Plugins: unlinks the ones a bundled plugin replaced (and points a creator chosen in
/// `swarm-setup.json` at its replacement), then links bundled plugins (the repo's `plugins/`)
/// that aren't linked yet, so `hivey update` brings new ones too. Linking talks to a hivey
/// server; without one, that part is reported and skipped (the next update does it).
fn update_plugins(repo: &Path, dry: bool) -> Vec<String> {
    let mut report = Vec::new();
    let Some(config_dir) = crate::config::config_path().parent().map(Path::to_path_buf) else {
        return report;
    };
    let registry = std::fs::read_to_string(config_dir.join("plugins.json")).unwrap_or_default();
    let linked: Vec<String> = serde_json::from_str::<Vec<serde_json::Value>>(&registry)
        .unwrap_or_default()
        .iter()
        .filter_map(|plugin| plugin["plugin_id"].as_str().map(str::to_string))
        .collect();
    let hivey = |args: &[&str]| -> bool {
        std::env::current_exe()
            .ok()
            .and_then(|exe| std::process::Command::new(exe).args(args).output().ok())
            .is_some_and(|out| out.status.success())
    };
    for (old, new) in REPLACED_PLUGINS {
        if !linked.iter().any(|id| id == old) {
            continue;
        }
        let done = dry || hivey(&["plugin", "unlink", old]);
        report.push(if done {
            format!("old plugin {old} unlinked (now {new})")
        } else {
            format!("old plugin {old}: could not unlink (start hivey, then hivey migrate)")
        });
    }
    // A creator the user chose that a bundled one replaced: choose the replacement.
    let setup = config_dir.join("swarm-setup.json");
    if let Ok(text) = std::fs::read_to_string(&setup) {
        let mut updated = text.clone();
        for (old, new) in REPLACED_PLUGINS {
            updated = updated.replace(&format!("\"{old}\""), &format!("\"{new}\""));
        }
        if updated != text {
            report.push(format!(
                "{}: chosen creators now hivey's",
                super::skill::show(&setup)
            ));
            if !dry {
                let _ = std::fs::write(&setup, updated);
            }
        }
    }
    let manifests = std::fs::read_dir(repo.join("plugins"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("herdr-plugin.toml"))
        .filter(|manifest| manifest.is_file());
    for manifest in manifests {
        let text = std::fs::read_to_string(&manifest).unwrap_or_default();
        let Some(id) = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("id = \""))
            .and_then(|rest| rest.strip_suffix('"'))
        else {
            continue;
        };
        if linked.iter().any(|linked| linked == id) {
            continue;
        }
        let dir = manifest.parent().unwrap_or(repo).display().to_string();
        let done = dry || hivey(&["plugin", "link", &dir]);
        report.push(if done {
            format!("plugin {id}: linked")
        } else {
            format!("plugin {id}: could not link (start hivey, then: hivey plugin link {dir})")
        });
    }
    report
}

/// The hivey agent's brief is written once and then belongs to the user, so updates never
/// rewrite it. Sections hivey marks in its template (`<!-- hivey section: <id> -->` …
/// `<!-- /hivey section: <id> -->`) that an existing brief lacks are added (before `## Notes`
/// when it has one); nothing already in the brief is changed or removed.
fn update_home_brief(home: &Path, dry: bool) -> std::io::Result<Vec<String>> {
    let path = home.join(".hivey/agent/CLAUDE.md");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let (updated, added) = add_missing_sections(&text, super::home::BRIEF);
    if added.is_empty() {
        return Ok(Vec::new());
    }
    if !dry {
        std::fs::write(&path, updated)?;
    }
    Ok(added
        .iter()
        .map(|id| format!("hivey agent brief ~/.hivey/agent/CLAUDE.md: added section {id}"))
        .collect())
}

/// `brief` with every marked section of `template` it lacks; the section ids added.
fn add_missing_sections(brief: &str, template: &str) -> (String, Vec<String>) {
    const START: &str = "<!-- hivey section: ";
    let mut out = brief.to_string();
    let mut added = Vec::new();
    let mut rest = template;
    while let Some(i) = rest.find(START) {
        let after = &rest[i + START.len()..];
        let Some(id_len) = after.find(" -->") else {
            break;
        };
        let id = after[0..id_len].to_string();
        let end_marker = format!("<!-- /hivey section: {id} -->");
        let Some(end) = rest[i..].find(&end_marker) else {
            break;
        };
        let stop = i + end + end_marker.len();
        let section = rest[i..stop].to_string();
        rest = &rest[stop..];
        if out.contains(&format!("{START}{id} -->")) {
            continue;
        }
        match out.find("\n## Notes") {
            Some(at) => out.insert_str(at + 1, &format!("{section}\n\n")),
            None => {
                let trimmed = out.trim_end().len();
                out.truncate(trimmed);
                out.push_str("\n\n");
                out.push_str(&section);
                out.push('\n');
            }
        }
        added.push(id);
    }
    (out, added)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hivey-migrate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir.canonicalize().expect("canonical")
    }

    #[test]
    fn adds_new_brief_sections_and_keeps_the_users_text() {
        let template = "# t\n\n<!-- hivey section: a -->\n## A\nnew a\n<!-- /hivey section: a -->\n\n\
                        <!-- hivey section: b -->\n## B\nnew b\n<!-- /hivey section: b -->\n\n## Notes\n";
        let brief = "# mine\n\nmy edits\n\n## Notes\n\nmy notes\n";
        let (out, added) = add_missing_sections(brief, template);
        assert_eq!(added, vec!["a", "b"]);
        assert!(
            out.starts_with("# mine\n\nmy edits\n\n<!-- hivey section: a -->"),
            "{out}"
        );
        assert!(
            out.contains("new b\n<!-- /hivey section: b -->\n\n## Notes\n\nmy notes\n"),
            "{out}"
        );
        // Already there (even if the user edited it): left as is.
        let (again, none) = add_missing_sections(&out.replace("new a", "edited a"), template);
        assert!(none.is_empty());
        assert!(again.contains("edited a"));
        // No Notes section: appended at the end.
        let (end, _) = add_missing_sections("# mine\n", template);
        assert!(end.ends_with("<!-- /hivey section: b -->\n"), "{end}");
    }

    #[test]
    fn the_real_template_marks_its_new_sections() {
        let (_, added) = add_missing_sections("# old brief\n", super::super::home::BRIEF);
        assert!(
            added.contains(&"machine-specific-changes".to_string()),
            "{added:?}"
        );
        assert!(added.contains(&"docs".to_string()), "{added:?}");
    }

    #[test]
    fn moves_old_copies_aside_and_keeps_others() {
        let home = scratch("old");
        let commands = home.join(".claude/commands");
        std::fs::create_dir_all(&commands).expect("commands");
        std::fs::write(
            commands.join("wiki-ingest.md"),
            "---\nname: wiki-ingest\n---\n",
        )
        .expect("old");
        std::fs::write(commands.join("wiki-log.md"), "my own log command\n").expect("mine");
        let skill = home.join(".claude/skills/agents-create-wiki");
        std::fs::create_dir_all(&skill).expect("skill");
        std::fs::write(skill.join("SKILL.md"), "---\nname: agents-create-wiki\n").expect("skill");

        let report = retire_old_copies(&home, false).expect("retire");
        assert_eq!(report.len(), 2, "{report:?}");
        assert!(!commands.join("wiki-ingest.md").exists());
        assert!(home
            .join(".hivey/old/.claude/commands/wiki-ingest.md")
            .is_file());
        assert!(home
            .join(".hivey/old/.claude/skills/agents-create-wiki/SKILL.md")
            .is_file());
        assert!(commands.join("wiki-log.md").is_file(), "not hivey's: kept");
        assert!(retire_old_copies(&home, false).expect("again").is_empty());

        // A second old copy later doesn't overwrite the first one kept aside.
        std::fs::write(commands.join("wiki-ingest.md"), "---\nname: wiki-ingest\n").expect("old");
        retire_old_copies(&home, false).expect("third");
        assert!(home
            .join(".hivey/old/.claude/commands/wiki-ingest.md.2")
            .is_file());
    }

    #[cfg(unix)]
    #[test]
    fn moves_replaced_commands_and_codex_skills_once_their_skill_is_installed() {
        let home = scratch("replaced");
        let commands = home.join(".claude/commands");
        std::fs::create_dir_all(&commands).expect("commands");
        for name in ["hivey-wiki-log", "hivey-wiki-lint"] {
            std::fs::write(
                commands.join(format!("{name}.md")),
                format!("---\nname: {name}\n---\n"),
            )
            .expect("command");
        }
        let skill = |dir: PathBuf, name: &str| {
            std::fs::create_dir_all(&dir).expect("skill");
            std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\n---\n"))
                .expect("SKILL.md");
        };
        // Only hivey-wiki-log's skill is installed: its command moves, hivey-wiki-lint's stays.
        skill(home.join(".claude/skills/hivey-wiki-log"), "hivey-wiki-log");
        // Codex: the old copy moves once ~/.agents/skills has the skill...
        skill(home.join(".codex/skills/hivey-codex"), "hivey-codex");
        skill(home.join(".agents/skills/hivey-codex"), "hivey-codex");
        // ...but not when the new one is only a link to the old one.
        skill(home.join(".codex/skills/hivey-grok"), "hivey-grok");
        std::os::unix::fs::symlink(
            home.join(".codex/skills/hivey-grok"),
            home.join(".agents/skills/hivey-grok"),
        )
        .expect("link");

        let report = retire_old_copies(&home, false).expect("retire");
        assert_eq!(report.len(), 2, "{report:?}");
        assert!(!commands.join("hivey-wiki-log.md").exists());
        assert!(home
            .join(".hivey/old/.claude/commands/hivey-wiki-log.md")
            .is_file());
        assert!(
            commands.join("hivey-wiki-lint.md").is_file(),
            "no skill yet"
        );
        assert!(!home.join(".codex/skills/hivey-codex").exists());
        assert!(home
            .join(".hivey/old/.codex/skills/hivey-codex/SKILL.md")
            .is_file());
        assert!(home.join(".codex/skills/hivey-grok/SKILL.md").is_file());
        assert!(home.join(".agents/skills/hivey-grok/SKILL.md").is_file());
    }
}
