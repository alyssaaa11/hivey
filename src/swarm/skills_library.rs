//! hiver: the user's skills library, where swarm and agent creators pick each agent's skills.
//!
//! Settings live in `~/.hiver/skills.json`: `dir` (default `~/SKILLS`, or `~/skills`) and
//! `online` (whether creators may also search skills.sh with `npx skills find`, always asking
//! the user before installing one). Changed with `hiver skills dir|online`, settings → skills,
//! or install.sh.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The skills provider that ships with hiver (the repo's `plugins/skills`).
pub(crate) const BUILTIN_PROVIDER: &str = "hiver.skills";
/// Its instructions, built in so creators always have some even when no provider is linked.
const BUILTIN_GUIDE: &str = include_str!("../../plugins/skills/skills.md");

/// A skills provider: an enabled plugin with a `skills.md` (its instructions) in its folder.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Provider {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) root: PathBuf,
}

/// The skills providers in a `plugin.list` result or the plugin registry file.
pub(crate) fn providers_from(plugins: &[Value]) -> Vec<Provider> {
    plugins
        .iter()
        .filter(|plugin| plugin["enabled"] != false)
        .filter_map(|plugin| {
            let id = plugin["plugin_id"].as_str()?.to_string();
            let root = PathBuf::from(plugin["plugin_root"].as_str()?);
            root.join("skills.md").is_file().then(|| Provider {
                name: plugin["name"].as_str().unwrap_or(&id).to_string(),
                id,
                root,
            })
        })
        .collect()
}

/// The installed skills providers, from the plugin registry.
pub(crate) fn providers() -> Vec<Provider> {
    std::fs::read_to_string(crate::config::config_dir().join("plugins.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        .map(|plugins| providers_from(&plugins))
        .unwrap_or_default()
}

/// The provider chosen in settings → plugins (`hiver skills providers --default`), if any.
pub(crate) fn chosen_provider() -> Option<String> {
    settings()["provider"].as_str().map(str::to_string)
}

pub(crate) fn set_provider(id: &str) -> std::io::Result<()> {
    save("provider", json!(id))
}

/// The provider creators follow: the chosen one when installed, else the built-in one, else
/// the first one.
pub(crate) fn pick_provider<'a>(providers: &'a [Provider], chosen: Option<&str>) -> Option<&'a Provider> {
    chosen
        .and_then(|id| providers.iter().find(|provider| provider.id == id))
        .or_else(|| providers.iter().find(|provider| provider.id == BUILTIN_PROVIDER))
        .or_else(|| providers.first())
}

/// The instructions creators follow (`hiver skills guide`): the picked provider's skills.md,
/// else hiver's built-in one. Also which provider they come from.
pub(crate) fn guide() -> (String, String) {
    let providers = providers();
    pick_provider(&providers, chosen_provider().as_deref())
        .and_then(|provider| {
            let text = std::fs::read_to_string(provider.root.join("skills.md")).ok()?;
            Some((provider.id.clone(), text))
        })
        .unwrap_or_else(|| (BUILTIN_PROVIDER.to_string(), BUILTIN_GUIDE.to_string()))
}

/// A skill in the library: its folder name, and the description from its SKILL.md.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Skill {
    pub(crate) name: String,
    pub(crate) description: String,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn settings_path() -> PathBuf {
    home().join(".hiver").join("skills.json")
}

fn settings() -> Value {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}))
}

fn save(key: &str, value: Value) -> std::io::Result<()> {
    let mut saved = settings();
    saved[key] = value;
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&saved).unwrap_or_default();
    std::fs::write(path, text + "\n")
}

/// `~` at the start of a path is the home folder.
pub(crate) fn expand(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) => home().join(rest.trim_start_matches('/')),
        None => PathBuf::from(path),
    }
}

/// The library folder: the one set, else `~/SKILLS` or `~/skills` if one exists, else
/// `~/SKILLS`.
pub(crate) fn dir() -> PathBuf {
    if let Some(dir) = settings()["dir"].as_str() {
        return expand(dir);
    }
    let home = home();
    ["SKILLS", "skills"]
        .iter()
        .map(|name| home.join(name))
        .find(|path| path.is_dir())
        .unwrap_or_else(|| home.join("SKILLS"))
}

pub(crate) fn set_dir(path: &Path) -> std::io::Result<()> {
    save("dir", json!(path.display().to_string()))
}

/// Whether creators may search skills.sh for skills the library doesn't have (default yes).
pub(crate) fn online() -> bool {
    settings()["online"].as_bool().unwrap_or(true)
}

pub(crate) fn set_online(on: bool) -> std::io::Result<()> {
    save("online", json!(on))
}

/// Folders that look like skills libraries, for settings → skills: the current one, then
/// `~/SKILLS`, `~/skills` and `~/.claude/skills` when they exist (each once).
pub(crate) fn candidates() -> Vec<PathBuf> {
    let home = home();
    let mut found = vec![dir()];
    for path in [
        home.join("SKILLS"),
        home.join("skills"),
        home.join(".claude").join("skills"),
    ] {
        let same = |known: &PathBuf| {
            known == &path
                || std::fs::canonicalize(known).ok() == std::fs::canonicalize(&path).ok()
        };
        if path.is_dir() && !found.iter().any(same) {
            found.push(path);
        }
    }
    found
}

/// The description in a SKILL.md's front matter (one line, folded `>` / `|` blocks joined).
pub(crate) fn description(skill_md: &str) -> String {
    let mut lines = skill_md.lines();
    if lines.next().map(str::trim) != Some("---") {
        return String::new();
    }
    let mut collecting = false;
    let mut parts: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if collecting {
            if line.starts_with(char::is_whitespace) && !line.trim().is_empty() {
                parts.push(line.trim());
                continue;
            }
            break;
        }
        if let Some(value) = line.strip_prefix("description:") {
            let value = value.trim();
            if value.is_empty() || matches!(value, ">" | "|" | ">-" | "|-") {
                collecting = true;
            } else {
                parts.push(value.trim_matches('"'));
                break;
            }
        }
    }
    parts.join(" ")
}

/// The skills in a library folder (sub-folders with a SKILL.md), by name.
pub(crate) fn list(dir: &Path) -> Vec<Skill> {
    let mut skills: Vec<Skill> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let text = std::fs::read_to_string(entry.path().join("SKILL.md")).ok()?;
            Some(Skill {
                name: entry.file_name().to_string_lossy().to_string(),
                description: description(&text),
            })
        })
        .collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "__pycache__" || name == ".DS_Store" {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&entry.path(), &to.join(&name))?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// Copies skills from the library into `<agent>/.claude/skills/` (an existing copy is
/// replaced, so a re-run picks up library changes). The skills copied.
pub(crate) fn copy_into(library: &Path, agent: &Path, names: &[String]) -> Result<Vec<String>, String> {
    let target = agent.join(".claude").join("skills");
    let mut copied = Vec::new();
    for name in names {
        if name.contains('/') || name.starts_with('.') {
            return Err(format!("{name:?} is not a skill name"));
        }
        let source = library.join(name);
        if !source.join("SKILL.md").is_file() {
            return Err(format!("no skill {name:?} in {}", library.display()));
        }
        let dest = target.join(name);
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|err| format!("{}: {err}", dest.display()))?;
        }
        copy_dir(&source, &dest).map_err(|err| format!("{name}: {err}"))?;
        copied.push(name.clone());
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_providers_are_plugins_with_a_skills_md() {
        let root = std::env::temp_dir().join(format!("hiver-skills-prov-{}", std::process::id()));
        let with = root.join("with");
        std::fs::create_dir_all(&with).unwrap();
        std::fs::write(with.join("skills.md"), "guide").unwrap();
        let without = root.join("without");
        std::fs::create_dir_all(&without).unwrap();
        let plugins = [
            json!({ "plugin_id": "x.skills", "name": "X", "plugin_root": with }),
            json!({ "plugin_id": "hiver.dashboard", "plugin_root": without }),
            json!({ "plugin_id": "off.skills", "enabled": false, "plugin_root": with }),
        ];
        let found = providers_from(&plugins);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "x.skills");
        // Chosen, else built in, else first
        let builtin = Provider { id: BUILTIN_PROVIDER.into(), name: "Skills".into(), root: with.clone() };
        let both = vec![found[0].clone(), builtin];
        assert_eq!(pick_provider(&both, Some("x.skills")).unwrap().id, "x.skills");
        assert_eq!(pick_provider(&both, None).unwrap().id, BUILTIN_PROVIDER);
        assert_eq!(pick_provider(&both, Some("gone")).unwrap().id, BUILTIN_PROVIDER);
        assert_eq!(pick_provider(&found, None).unwrap().id, "x.skills");
        assert!(pick_provider(&[], None).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn description_reads_inline_and_folded_front_matter() {
        assert_eq!(
            description("---\nname: a\ndescription: Does a thing\n---\nbody"),
            "Does a thing"
        );
        assert_eq!(
            description("---\nname: b\ndescription: >\n  Folded line one\n  and two.\nother: x\n---"),
            "Folded line one and two."
        );
        assert_eq!(description("no front matter"), "");
    }

    #[test]
    fn list_and_copy_skills_from_a_library() {
        let root = std::env::temp_dir().join(format!("hiver-skills-test-{}", std::process::id()));
        let library = root.join("lib");
        for (name, text) in [("alpha", "---\ndescription: First\n---\n"), ("beta", "---\ndescription: Second\n---\n")] {
            std::fs::create_dir_all(library.join(name).join("scripts")).unwrap();
            std::fs::write(library.join(name).join("SKILL.md"), text).unwrap();
            std::fs::write(library.join(name).join("scripts").join("run.sh"), "echo").unwrap();
        }
        std::fs::create_dir_all(library.join("not-a-skill")).unwrap();
        let names: Vec<String> = list(&library).into_iter().map(|skill| skill.name).collect();
        assert_eq!(names, ["alpha", "beta"]);

        let agent = root.join("agent");
        let copied = copy_into(&library, &agent, &["beta".to_string()]).unwrap();
        assert_eq!(copied, ["beta"]);
        assert!(agent.join(".claude/skills/beta/scripts/run.sh").is_file());
        assert!(copy_into(&library, &agent, &["missing".to_string()]).is_err());
        assert!(copy_into(&library, &agent, &["../lib".to_string()]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
