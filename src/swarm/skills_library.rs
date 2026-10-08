//! hivey: where swarm and agent creators get each agent's skills. The library is skylls (the
//! user's published skills and the ones friends shared); the skills plugin's guide says how.
//!
//! Settings live in `~/.hivey/skills.json`: `provider` (the skills plugin creators follow) and
//! `online` (whether creators may also search skills.sh with `npx skills find`, always asking
//! the user before installing one). Changed with `hivey skills online|providers` or settings.

use serde_json::{json, Value};
use std::path::PathBuf;

/// The skills provider that ships with hivey (the repo's `plugins/skills`).
pub(crate) const BUILTIN_PROVIDER: &str = "hivey.skills";
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

/// The provider chosen in settings → plugins (`hivey skills providers --default`), if any.
pub(crate) fn chosen_provider() -> Option<String> {
    settings()["provider"].as_str().map(str::to_string)
}

pub(crate) fn set_provider(id: &str) -> std::io::Result<()> {
    save("provider", json!(id))
}

/// The provider creators follow: the chosen one when installed, else the built-in one, else
/// the first one.
pub(crate) fn pick_provider<'a>(
    providers: &'a [Provider],
    chosen: Option<&str>,
) -> Option<&'a Provider> {
    chosen
        .and_then(|id| providers.iter().find(|provider| provider.id == id))
        .or_else(|| {
            providers
                .iter()
                .find(|provider| provider.id == BUILTIN_PROVIDER)
        })
        .or_else(|| providers.first())
}

/// The instructions creators follow (`hivey skills guide`): the picked provider's skills.md,
/// else hivey's built-in one. Also which provider they come from.
pub(crate) fn guide() -> (String, String) {
    let providers = providers();
    pick_provider(&providers, chosen_provider().as_deref())
        .and_then(|provider| {
            let text = std::fs::read_to_string(provider.root.join("skills.md")).ok()?;
            Some((provider.id.clone(), text))
        })
        .unwrap_or_else(|| (BUILTIN_PROVIDER.to_string(), BUILTIN_GUIDE.to_string()))
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn settings_path() -> PathBuf {
    home().join(".hivey").join("skills.json")
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

/// Whether creators may search skills.sh for skills the library doesn't have (default yes).
pub(crate) fn online() -> bool {
    settings()["online"].as_bool().unwrap_or(true)
}

pub(crate) fn set_online(on: bool) -> std::io::Result<()> {
    save("online", json!(on))
}

/// Whether the skylls CLI is on PATH (the skills library creators use).
pub(crate) fn skylls_installed() -> bool {
    let names: &[&str] = if cfg!(windows) {
        &["skylls.exe", "skylls.cmd", "skylls"]
    } else {
        &["skylls"]
    };
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| names.iter().any(|name| dir.join(name).is_file()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_providers_are_plugins_with_a_skills_md() {
        let root = std::env::temp_dir().join(format!("hivey-skills-prov-{}", std::process::id()));
        let with = root.join("with");
        std::fs::create_dir_all(&with).unwrap();
        std::fs::write(with.join("skills.md"), "guide").unwrap();
        let without = root.join("without");
        std::fs::create_dir_all(&without).unwrap();
        let plugins = [
            json!({ "plugin_id": "x.skills", "name": "X", "plugin_root": with }),
            json!({ "plugin_id": "hivey.dashboard", "plugin_root": without }),
            json!({ "plugin_id": "off.skills", "enabled": false, "plugin_root": with }),
        ];
        let found = providers_from(&plugins);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "x.skills");
        // Chosen, else built in, else first
        let builtin = Provider {
            id: BUILTIN_PROVIDER.into(),
            name: "Skills".into(),
            root: with.clone(),
        };
        let both = vec![found[0].clone(), builtin];
        assert_eq!(
            pick_provider(&both, Some("x.skills")).unwrap().id,
            "x.skills"
        );
        assert_eq!(pick_provider(&both, None).unwrap().id, BUILTIN_PROVIDER);
        assert_eq!(
            pick_provider(&both, Some("gone")).unwrap().id,
            BUILTIN_PROVIDER
        );
        assert_eq!(pick_provider(&found, None).unwrap().id, "x.skills");
        assert!(pick_provider(&[], None).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
