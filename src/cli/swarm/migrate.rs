//! `hivey migrate`: brings what an older hivey left on this machine up to the current
//! conventions. Run by `hivey update` and `install.sh`; safe to run any number of times.
//!
//! - the hivey skill and the `hivey-*` Claude Code commands (`hivey skill install`);
//! - wikis: an agent's or swarm's `<folder>/obsidian/` vault becomes `<folder>/<name>-wiki/`
//!   (Obsidian lists vaults by folder name, so they all showed as "obsidian"). `obsidian` stays as
//!   a link to it so scripts with the old path keep working, and every reference is rewritten:
//!   the CLAUDE.md briefs (the folder's, its agents', the vault's own), the `~/Obsidian/*` links,
//!   `~/.hivey/wiki.json` and Obsidian's own vault list.

use std::path::{Path, PathBuf};

use regex::Regex;

const USAGE: &str = "usage: hivey migrate [--dry-run]
  bring an older hivey install up to date (run by hivey update; safe to run again):
  - the hivey skill and the hivey-* Claude Code commands (/hivey-wiki-ingest, …)
  - old copies (wiki-ingest, …, agents-create-wiki) that hivey-* ones replaced: moved to
    ~/.hivey/old/, never deleted
  - wikis: <folder>/obsidian/ → <folder>/<name>-wiki/ (Obsidian tells them apart by folder
    name), with every CLAUDE.md, ~/Obsidian link and Obsidian's vault list pointed at it
  --dry-run   only show what would change";

const LEGACY_VAULT: &str = "obsidian";

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
        if home.join(".claude").is_dir() {
            let commands = super::skill::install_commands(&home.join(".claude/commands"))?;
            println!("Claude Code commands: {commands}");
        }
    }
    let retired = retire_old_copies(&home, dry)?;
    for line in &retired {
        println!("{line}");
    }
    let machine = Machine::discover(&home);
    let report = migrate_wikis(&machine, dry)?;
    for line in &report {
        println!("{line}");
    }
    if report.is_empty() {
        println!("wikis: all use <name>-wiki already");
    }
    Ok(0)
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
];

/// Where retired copies go: moved, never deleted, so nothing is lost.
const OLD_COPIES_DIR: &str = ".hivey/old";

/// Moves the old copies in `OLD_COPIES` into ~/.hivey/old/ (same relative path).
fn retire_old_copies(home: &Path, dry: bool) -> std::io::Result<Vec<String>> {
    let mut report = Vec::new();
    for (rel, name, replaced_by) in OLD_COPIES {
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

/// Where this machine keeps what a wiki migration touches.
struct Machine {
    home: PathBuf,
    /// Agent and swarm folders that may hold a wiki.
    roots: Vec<PathBuf>,
    /// Obsidian's own settings (`obsidian.json`, its vault list), when present.
    registries: Vec<PathBuf>,
    obsidian_running: bool,
}

impl Machine {
    fn discover(home: &Path) -> Self {
        let mut roots = Vec::new();
        for parent in ["agents", "swarms"] {
            roots.extend(subdirs(&home.join(parent)));
        }
        roots.push(home.join(".hivey/agent"));
        if let Some(config_dir) = crate::config::config_path().parent() {
            let mut registries = vec![config_dir.join("swarms.json")];
            registries.extend(
                subdirs(&config_dir.join("sessions"))
                    .into_iter()
                    .map(|s| s.join("swarms.json")),
            );
            for registry in registries {
                roots.extend(swarm_roots(&registry));
            }
        }
        // ~/Obsidian/<x> → <folder>/obsidian links made by older wiki creators.
        for link in subdirs(&home.join("Obsidian")) {
            if let Ok(target) = std::fs::read_link(&link) {
                if target.file_name().is_some_and(|name| name == LEGACY_VAULT) {
                    roots.extend(target.parent().map(Path::to_path_buf));
                }
            }
        }
        let registries = [
            "Library/Application Support/obsidian/obsidian.json",
            ".config/obsidian/obsidian.json",
        ]
        .into_iter()
        .map(|rel| home.join(rel))
        .filter(|path| path.is_file())
        .collect();
        Machine {
            home: home.to_path_buf(),
            roots,
            registries,
            obsidian_running: obsidian_running(),
        }
    }
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs
}

fn swarm_roots(registry: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(registry) else {
        return Vec::new();
    };
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    value["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|root| root.as_str().map(PathBuf::from))
        .collect()
}

fn obsidian_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-ix", "obsidian"])
        .output()
        .is_ok_and(|out| out.status.success())
}

/// `<name>-wiki` for a folder: its name as a slug (the hivey agent's folder is `~/.hivey/agent`).
fn vault_name(root: &Path) -> String {
    let name = root.file_name().map(|n| n.to_string_lossy().to_lowercase());
    let name = match (name.as_deref(), root.parent().and_then(Path::file_name)) {
        (Some("agent"), Some(parent)) if parent == ".hivey" => "hivey".to_string(),
        (Some(name), _) => name.to_string(),
        (None, _) => "agent".to_string(),
    };
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "agent" } else { slug };
    if slug.ends_with("-wiki") {
        slug.to_string()
    } else {
        format!("{slug}-wiki")
    }
}

/// One wiki moved (now or by an earlier run) from `old` to `new`, both absolute.
struct Move {
    root: PathBuf,
    old: PathBuf,
    new: PathBuf,
}

fn migrate_wikis(machine: &Machine, dry: bool) -> std::io::Result<Vec<String>> {
    let mut report = Vec::new();
    let mut moves: Vec<Move> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for root in &machine.roots {
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        if root == machine.home || !seen.insert(root.clone()) {
            continue;
        }
        let old = root.join(LEGACY_VAULT);
        let Ok(meta) = std::fs::symlink_metadata(&old) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            // Moved by an earlier run: keep fixing references (e.g. Obsidian was open then).
            let target = std::fs::read_link(&old).unwrap_or_default();
            let new = root.join(&target);
            if target.components().count() == 1 && new.join("CLAUDE.md").is_file() {
                moves.push(Move { root, old, new });
            }
            continue;
        }
        if !old.join("CLAUDE.md").is_file() {
            continue;
        }
        let name = vault_name(&root);
        let new = root.join(&name);
        if new.exists() {
            report.push(format!(
                "wiki {}: both obsidian/ and {name}/ exist, left as is (merge them by hand)",
                tilde(&machine.home, &root)
            ));
            continue;
        }
        report.push(format!(
            "wiki {}: obsidian/ → {name}/",
            tilde(&machine.home, &root)
        ));
        if !dry {
            std::fs::rename(&old, &new)?;
            // The old path keeps working for scripts and briefs outside the ones fixed below.
            symlink(Path::new(&name), &old)?;
        }
        moves.push(Move { root, old, new });
    }
    if moves.is_empty() {
        return Ok(report);
    }
    let absolute = absolute_rewrites(&machine.home, &moves);
    for mv in &moves {
        let local = local_rewrite(mv);
        for brief in briefs_of(mv) {
            let Ok(text) = std::fs::read_to_string(&brief) else {
                continue;
            };
            let mut updated = text.clone();
            for (pattern, replacement) in &absolute {
                updated = pattern
                    .replace_all(&updated, replacement.as_str())
                    .into_owned();
            }
            if brief.starts_with(&mv.root) {
                updated = local(&updated);
            }
            if updated != text {
                report.push(format!(
                    "  CLAUDE.md: {} now names the {} wiki",
                    tilde(&machine.home, &brief),
                    file_name(&mv.new)
                ));
                if !dry {
                    std::fs::write(&brief, updated)?;
                }
            }
        }
    }
    // Briefs of other agents can name a moved wiki by its absolute path (a read-only source).
    let moved_roots: Vec<&PathBuf> = moves.iter().map(|mv| &mv.root).collect();
    for root in &seen {
        if moved_roots.contains(&root) {
            continue;
        }
        for brief in briefs_in(root) {
            rewrite_file(&brief, &absolute, dry, &machine.home, &mut report)?;
        }
    }
    for link in subdirs(&machine.home.join("Obsidian")) {
        let Ok(target) = std::fs::read_link(&link) else {
            continue;
        };
        if let Some(mv) = moves.iter().find(|mv| mv.old == target) {
            report.push(format!(
                "  ~/Obsidian/{} → {}",
                file_name(&link),
                tilde(&machine.home, &mv.new)
            ));
            if !dry {
                std::fs::remove_file(&link)?;
                symlink(&mv.new, &link)?;
            }
        }
    }
    let wiki_json = machine.home.join(".hivey/wiki.json");
    rewrite_file(&wiki_json, &absolute, dry, &machine.home, &mut report)?;
    for registry in &machine.registries {
        if machine.obsidian_running {
            report.push(
                "  Obsidian is open: quit it, then run `hivey migrate` again so its vault list \
                 shows the new names"
                    .into(),
            );
            break;
        }
        rewrite_file(registry, &absolute, dry, &machine.home, &mut report)?;
    }
    Ok(report)
}

/// Absolute and `~/` spellings of every moved wiki, as (pattern, replacement); the pattern
/// keeps the character after the path so `…/obsidian-old` is not touched.
fn absolute_rewrites(home: &Path, moves: &[Move]) -> Vec<(Regex, String)> {
    let mut rewrites = Vec::new();
    for mv in moves {
        let mut spellings = vec![(
            mv.old.to_string_lossy().into_owned(),
            mv.new.to_string_lossy().into_owned(),
        )];
        if let (Ok(old), Ok(new)) = (mv.old.strip_prefix(home), mv.new.strip_prefix(home)) {
            spellings.push((
                format!("~/{}", old.display()),
                format!("~/{}", new.display()),
            ));
        }
        for (old, new) in spellings {
            if let Ok(pattern) = Regex::new(&format!(r"{}(\z|[^\w-])", regex::escape(&old))) {
                rewrites.push((pattern, format!("{new}$1")));
            }
        }
    }
    rewrites
}

/// Relative mentions in the folder's own briefs: `obsidian/…`, `../obsidian`, `` `obsidian` ``.
/// Prose ("an obsidian vault") and other paths (`.obsidian/`, `$ROOT/obsidian`) stay.
fn local_rewrite(mv: &Move) -> impl Fn(&str) -> String {
    let name = file_name(&mv.new);
    let quoted = Regex::new(r"`((?:\.\./)?)obsidian([/`])").ok();
    let pathlike = Regex::new(r"(^|[\s(\[])((?:\.\./)?)obsidian/").ok();
    move |text: &str| {
        let mut text = text.to_string();
        if let Some(re) = &quoted {
            text = re
                .replace_all(&text, format!("`${{1}}{name}${{2}}").as_str())
                .into_owned();
        }
        if let Some(re) = &pathlike {
            text = re
                .replace_all(&text, format!("${{1}}${{2}}{name}/").as_str())
                .into_owned();
        }
        text
    }
}

/// The CLAUDE.md briefs a moved wiki's folder owns: its own, its agents' and the vault's.
fn briefs_of(mv: &Move) -> Vec<PathBuf> {
    let mut briefs = briefs_in(&mv.root);
    briefs.push(mv.new.join("CLAUDE.md"));
    briefs.retain(|brief| brief.is_file());
    briefs
}

fn briefs_in(root: &Path) -> Vec<PathBuf> {
    let mut briefs = vec![root.join("CLAUDE.md")];
    for dir in subdirs(root) {
        let is_vault_link = std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_symlink());
        if dir.is_dir() && !is_vault_link && !dir.join(".obsidian").is_dir() {
            briefs.push(dir.join("CLAUDE.md"));
        }
    }
    briefs.retain(|brief| brief.is_file());
    briefs
}

fn rewrite_file(
    path: &Path,
    rewrites: &[(Regex, String)],
    dry: bool,
    home: &Path,
    report: &mut Vec<String>,
) -> std::io::Result<()> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let mut updated = text.clone();
    for (pattern, replacement) in rewrites {
        updated = pattern
            .replace_all(&updated, replacement.as_str())
            .into_owned();
    }
    if updated != text {
        report.push(format!("  {}: wiki paths updated", tilde(home, path)));
        if !dry {
            std::fs::write(path, updated)?;
        }
    }
    Ok(())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn tilde(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => path.display().to_string(),
    }
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(not(unix))]
fn symlink(_target: &Path, _link: &Path) -> std::io::Result<()> {
    Ok(()) // hivey runs on macOS and Linux; nothing links the old path elsewhere
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

    fn vault(dir: &Path) {
        std::fs::create_dir_all(dir.join(".obsidian")).expect("vault");
        std::fs::write(dir.join("CLAUDE.md"), "# rules\n").expect("rules");
    }

    fn machine(home: &Path, roots: Vec<PathBuf>, registries: Vec<PathBuf>) -> Machine {
        Machine {
            home: home.to_path_buf(),
            roots,
            registries,
            obsidian_running: false,
        }
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

    #[test]
    fn names_vaults_after_their_folder() {
        assert_eq!(vault_name(Path::new("/h/agents/btc")), "btc-wiki");
        assert_eq!(
            vault_name(Path::new("/h/swarms/apps_ideas")),
            "apps-ideas-wiki"
        );
        assert_eq!(
            vault_name(Path::new("/h/agents/youtube-wiki")),
            "youtube-wiki"
        );
        assert_eq!(vault_name(Path::new("/h/.hivey/agent")), "hivey-wiki");
    }

    #[test]
    fn moves_a_swarm_wiki_and_rewrites_every_reference() {
        let home = scratch("swarm");
        let root = home.join("swarms/gap-finder");
        vault(&root.join("obsidian"));
        std::fs::create_dir_all(root.join("critic")).expect("agent");
        std::fs::write(
            root.join("CLAUDE.md"),
            "Wiki: `obsidian/wiki/index.md`, vault `obsidian` and /x/obsidian/y.\n\
             Absolute: ~/swarms/gap-finder/obsidian. An obsidian vault; `.obsidian/` stays.\n",
        )
        .expect("brief");
        std::fs::write(
            root.join("critic/CLAUDE.md"),
            "memory: `../obsidian` and ../obsidian/raw/\n",
        )
        .expect("brief");
        let reader = home.join("agents/reader");
        std::fs::create_dir_all(&reader).expect("reader");
        let abs = root.join("obsidian").display().to_string();
        std::fs::write(reader.join("CLAUDE.md"), format!("read-only: {abs}/wiki\n"))
            .expect("brief");
        std::fs::create_dir_all(home.join("Obsidian")).expect("links");
        symlink(
            &root.join("obsidian"),
            &home.join("Obsidian/gap-finder-wiki"),
        )
        .expect("link");
        let registry = home.join("obsidian.json");
        std::fs::write(
            &registry,
            format!(r#"{{"vaults":{{"a":{{"path":"{abs}"}}}}}}"#),
        )
        .expect("registry");

        let m = machine(
            &home,
            vec![root.clone(), reader.clone()],
            vec![registry.clone()],
        );
        let report = migrate_wikis(&m, false).expect("migrate");
        assert!(
            report[0].contains("obsidian/ → gap-finder-wiki/"),
            "{report:?}"
        );

        let new = root.join("gap-finder-wiki");
        assert!(new.join("CLAUDE.md").is_file());
        assert_eq!(
            std::fs::read_link(root.join("obsidian")).expect("compat link"),
            PathBuf::from("gap-finder-wiki")
        );
        let brief = std::fs::read_to_string(root.join("CLAUDE.md")).expect("brief");
        assert_eq!(
            brief,
            "Wiki: `gap-finder-wiki/wiki/index.md`, vault `gap-finder-wiki` and /x/obsidian/y.\n\
             Absolute: ~/swarms/gap-finder/gap-finder-wiki. An obsidian vault; `.obsidian/` stays.\n"
        );
        let critic = std::fs::read_to_string(root.join("critic/CLAUDE.md")).expect("brief");
        assert_eq!(
            critic,
            "memory: `../gap-finder-wiki` and ../gap-finder-wiki/raw/\n"
        );
        let reader_brief = std::fs::read_to_string(reader.join("CLAUDE.md")).expect("brief");
        assert_eq!(reader_brief, format!("read-only: {}/wiki\n", new.display()));
        assert_eq!(
            std::fs::read_link(home.join("Obsidian/gap-finder-wiki")).expect("link"),
            new
        );
        let registry_text = std::fs::read_to_string(&registry).expect("registry");
        assert!(registry_text.contains(&new.display().to_string()));

        // Idempotent: a second run finds nothing left to change.
        let again = migrate_wikis(&m, false).expect("again");
        assert!(again.is_empty(), "{again:?}");
    }

    #[test]
    fn dry_run_changes_nothing() {
        let home = scratch("dry");
        let root = home.join("agents/btc");
        vault(&root.join("obsidian"));
        std::fs::write(root.join("CLAUDE.md"), "wiki at `obsidian/`\n").expect("brief");
        let m = machine(&home, vec![root.clone()], Vec::new());
        let report = migrate_wikis(&m, true).expect("dry run");
        assert!(report.iter().any(|l| l.contains("btc-wiki")), "{report:?}");
        assert!(root.join("obsidian/CLAUDE.md").is_file());
        assert!(!root.join("btc-wiki").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("CLAUDE.md")).expect("brief"),
            "wiki at `obsidian/`\n"
        );
    }

    #[test]
    fn keeps_a_folder_that_has_both_vaults() {
        let home = scratch("both");
        let root = home.join("agents/btc");
        vault(&root.join("obsidian"));
        vault(&root.join("btc-wiki"));
        let report =
            migrate_wikis(&machine(&home, vec![root.clone()], Vec::new()), false).expect("migrate");
        assert!(report[0].contains("left as is"), "{report:?}");
        assert!(root.join("obsidian/CLAUDE.md").is_file());
    }

    #[test]
    fn registry_waits_while_obsidian_is_open() {
        let home = scratch("open");
        let root = home.join("agents/btc");
        vault(&root.join("obsidian"));
        let registry = home.join("obsidian.json");
        let old = root.join("obsidian").display().to_string();
        std::fs::write(&registry, format!(r#"{{"path":"{old}"}}"#)).expect("registry");
        let mut m = machine(&home, vec![root.clone()], vec![registry.clone()]);
        m.obsidian_running = true;
        let report = migrate_wikis(&m, false).expect("migrate");
        assert!(report.iter().any(|l| l.contains("Obsidian is open")));
        assert!(std::fs::read_to_string(&registry)
            .expect("registry")
            .contains(&old));
        // Next run (Obsidian closed) still fixes it, from the link left behind.
        m.obsidian_running = false;
        migrate_wikis(&m, false).expect("again");
        let text = std::fs::read_to_string(&registry).expect("registry");
        assert!(
            text.contains(&root.join("btc-wiki").display().to_string()),
            "{text}"
        );
    }
}
