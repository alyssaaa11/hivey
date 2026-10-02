//! hiver: swarm and agent creators (setup providers: plugins with a `setup` pane).
//!
//! `hiver swarm new` hands a task to a swarm creator, `hiver swarm new --agent` to an agent
//! creator. The one used for each kind is chosen with `hiver swarm providers --default ID` or
//! in settings → plugins (saved in `swarm-setup.json`); when none is chosen, or the chosen one
//! isn't installed, the built-in creator that ships with hiver is used.

use serde_json::{json, Value};

/// The pane entrypoint every setup provider has.
pub(crate) const SETUP_ENTRYPOINT: &str = "setup";
/// The creators that ship with hiver (the repo's `plugins/`, linked by install.sh).
pub(crate) const BUILTIN_SWARM_CREATOR: &str = "hiver.swarm-creator";
pub(crate) const BUILTIN_AGENT_CREATOR: &str = "hiver.agent-creator";

/// An installed setup provider.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Creator {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    /// An agent creator (one solo agent) rather than a swarm creator.
    pub(crate) agent: bool,
}

/// A setup provider is an agent creator when its id or description says so ("Solo agent
/// setup provider: …"); otherwise it's a swarm creator.
pub(crate) fn is_agent_provider(id: &str, description: &str) -> bool {
    id.contains("agent") || description.starts_with("Solo agent")
}

/// The enabled setup providers in a `plugin.list` result or the plugin registry file.
pub(crate) fn from_plugins(plugins: &[Value]) -> Vec<Creator> {
    plugins
        .iter()
        .filter(|plugin| plugin["enabled"] != false)
        .filter(|plugin| {
            plugin["panes"]
                .as_array()
                .is_some_and(|panes| panes.iter().any(|pane| pane["id"] == SETUP_ENTRYPOINT))
        })
        .filter_map(|plugin| {
            let id = plugin["plugin_id"].as_str()?.to_string();
            let description = plugin["description"].as_str().unwrap_or("").to_string();
            Some(Creator {
                name: plugin["name"].as_str().unwrap_or(&id).to_string(),
                agent: is_agent_provider(&id, &description),
                id,
                description,
            })
        })
        .collect()
}

/// The installed creators, read from the plugin registry (for the client's settings, which
/// doesn't go through the server for this).
pub(crate) fn installed() -> Vec<Creator> {
    std::fs::read_to_string(crate::config::config_dir().join("plugins.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok())
        .map(|plugins| from_plugins(&plugins))
        .unwrap_or_default()
}

pub(crate) fn defaults_path() -> std::path::PathBuf {
    crate::config::config_dir().join("swarm-setup.json")
}

/// The key in swarm-setup.json for each kind's chosen creator.
fn default_key(agent: bool) -> &'static str {
    if agent {
        "agent_provider"
    } else {
        "provider"
    }
}

/// The creator chosen for a kind, if any.
pub(crate) fn chosen(agent: bool) -> Option<String> {
    std::fs::read_to_string(defaults_path())
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value[default_key(agent)].as_str().map(str::to_string))
}

pub(crate) fn save_chosen(agent: bool, id: &str) -> std::io::Result<()> {
    let path = defaults_path();
    let mut saved = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    saved[default_key(agent)] = json!(id);
    let text = serde_json::to_string_pretty(&saved).unwrap_or_default();
    std::fs::write(path, text + "\n")
}

/// The creator used for a kind: `chosen` when installed, else the built-in one, else the only
/// one of that kind.
pub(crate) fn pick(
    creators: &[Creator],
    agent: bool,
    chosen: Option<&str>,
) -> Result<String, String> {
    let ids: Vec<&str> = creators
        .iter()
        .filter(|creator| creator.agent == agent)
        .map(|creator| creator.id.as_str())
        .collect();
    if let Some(id) = chosen.filter(|id| ids.contains(id)) {
        return Ok(id.to_string());
    }
    let kind = if agent { "agent" } else { "swarm" };
    let builtin = if agent {
        BUILTIN_AGENT_CREATOR
    } else {
        BUILTIN_SWARM_CREATOR
    };
    match ids.as_slice() {
        _ if ids.contains(&builtin) => Ok(builtin.to_string()),
        [only] => Ok((*only).to_string()),
        [] => Err(format!(
            "no {kind} creator installed (hiver plugin link <hiver repo>/plugins/{kind}-creator)"
        )),
        _ => Err(format!(
            "several {kind} creators; pick one with --provider ({}) or in settings → plugins",
            ids.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str, description: &str, setup: bool) -> Value {
        let pane = if setup { "setup" } else { "relay" };
        json!({ "plugin_id": id, "name": id, "description": description, "enabled": true,
                "panes": [{ "id": pane }] })
    }

    #[test]
    fn creators_are_setup_plugins_split_by_kind() {
        let creators = from_plugins(&[
            plugin("hiver.swarm-creator", "Swarm setup provider", true),
            plugin("agent.creator", "Solo agent setup provider", true),
            plugin("x.solo", "Solo agent setup provider: one agent", true),
            plugin("hiver.slack-relay", "Slack", false),
        ]);
        let kinds: Vec<(&str, bool)> = creators
            .iter()
            .map(|creator| (creator.id.as_str(), creator.agent))
            .collect();
        assert_eq!(
            kinds,
            [
                ("hiver.swarm-creator", false),
                ("agent.creator", true),
                ("x.solo", true)
            ]
        );
    }

    #[test]
    fn pick_prefers_the_chosen_then_the_built_in_one() {
        let creators = from_plugins(&[
            plugin("hiver.swarm-creator", "", true),
            plugin("swarm.creator", "", true),
            plugin("hiver.agent-creator", "", true),
        ]);
        assert_eq!(pick(&creators, false, None).unwrap(), "hiver.swarm-creator");
        assert_eq!(
            pick(&creators, false, Some("swarm.creator")).unwrap(),
            "swarm.creator"
        );
        // A chosen creator that isn't installed falls back to the built-in one
        assert_eq!(
            pick(&creators, false, Some("gone.creator")).unwrap(),
            "hiver.swarm-creator"
        );
        // A creator of the other kind is never picked
        assert_eq!(
            pick(&creators, true, Some("swarm.creator")).unwrap(),
            "hiver.agent-creator"
        );
        let only_other = from_plugins(&[plugin("a.swarm", "", true), plugin("b.swarm", "", true)]);
        assert!(pick(&only_other, false, None).is_err());
        assert!(pick(&only_other, true, None).is_err());
    }
}
