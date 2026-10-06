//! Swarm roster loaded from `<root>/.swarm/agents.json`, hivey's swarm manifest (written by
//! `hivey swarm launch` or a setup provider; format in plugins/README.md).
//!
//! The manifest format is owned by the skill; hivey only reads it and accepts a few
//! optional additions (`role` per agent, `command` for script entries).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Role {
    Master,
    Worker,
    Critic,
    Script,
}

impl Role {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "master" | "coordinator" => Some(Self::Master),
            "worker" => Some(Self::Worker),
            "critic" => Some(Self::Critic),
            "script" => Some(Self::Script),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Worker => "worker",
            Self::Critic => "critic",
            Self::Script => "script",
        }
    }

    pub(crate) fn glyph(self) -> &'static str {
        match self {
            Self::Master => "◆",
            Self::Worker => "●",
            Self::Critic => "✎",
            Self::Script => "▷",
        }
    }

    /// Scripts (relays, watchers) have no prompt to type into.
    pub(crate) fn receives_messages(self) -> bool {
        self != Self::Script
    }

    /// Sort key: master first, scripts last.
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Master => 0,
            Self::Worker => 1,
            Self::Critic => 2,
            Self::Script => 3,
        }
    }
}

/// Border/title color for a `role` token value (pane metadata); `None` for non-swarm panes.
pub(crate) fn role_color(
    role: &str,
    palette: &crate::app::state::Palette,
) -> Option<ratatui::style::Color> {
    Some(match Role::parse(role)? {
        Role::Master => palette.yellow,
        Role::Worker => palette.blue,
        Role::Critic => palette.mauve,
        Role::Script => palette.overlay0,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SwarmAgent {
    /// Short name inside the swarm (`scout`, `critic`, `coordinator`).
    pub key: String,
    /// Server-wide hivey agent name (`app-ideas-scout`); absent for scripts.
    pub herdr_name: Option<String>,
    pub role: Role,
    pub model: Option<String>,
    /// Pane recorded at launch; live panes are resolved by herdr name first.
    pub pane_id: Option<String>,
    /// Arguments the agent was launched with (manifest `args`), re-applied when hivey
    /// resumes the agent after a restart.
    pub args: Vec<String>,
    /// Which CLI runs the agent (manifest `kind`: claude, codex).
    pub kind: super::adapter::AgentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Swarm {
    pub slug: String,
    pub root: PathBuf,
    /// Manifest `"state": "paused"`: deliveries are held (still queued and logged).
    pub paused: bool,
    /// Manifest `"solo": true`: a single agent (`hivey swarm launch --solo`) that is its own
    /// master; shown as one row in the sidebar instead of a swarm tree.
    pub solo: bool,
    /// Manifest `"home": true`: the hivey agent (`hivey home`), kept alive by the server.
    pub home: bool,
    /// Manifest `"profile"`: what the swarm or agent does, for the directory other agents
    /// read before asking for help (`description`, `skills`, `tools`).
    pub profile: Value,
    /// Master first, then by role, then by key (manifest maps are key-sorted).
    pub agents: Vec<SwarmAgent>,
}

pub(crate) fn manifest_path(root: &Path) -> PathBuf {
    root.join(".swarm").join("agents.json")
}

impl Swarm {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let path = manifest_path(root);
        let text = std::fs::read_to_string(&path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        let manifest: Value = serde_json::from_str(&text)
            .map_err(|err| format!("invalid JSON in {}: {err}", path.display()))?;
        Self::from_manifest(root, &manifest)
    }

    pub(crate) fn from_manifest(root: &Path, manifest: &Value) -> Result<Self, String> {
        let slug = manifest
            .get("slug")
            .and_then(Value::as_str)
            .filter(|slug| !slug.is_empty())
            .ok_or("manifest has no \"slug\"")?
            .to_string();
        let mut agents = Vec::new();
        if let Some(entries) = manifest.get("agents").and_then(Value::as_object) {
            for (key, entry) in entries {
                agents.push(agent_from_entry(key, entry));
            }
        }
        // Older manifests name the coordinator only at the top level.
        if !agents.iter().any(|agent| agent.role == Role::Master) {
            if let Some(name) = manifest.get("coordinator").and_then(Value::as_str) {
                agents.push(SwarmAgent {
                    key: "coordinator".into(),
                    herdr_name: Some(name.to_string()),
                    role: Role::Master,
                    model: None,
                    // Written by `hivey swarm launch`; finds the master even if not renamed.
                    pane_id: manifest
                        .get("coordinator_pane_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    args: Vec::new(),
                    kind: Default::default(),
                });
            }
        }
        if let Some(pane) = manifest.get("relay_pane_id").and_then(Value::as_str) {
            if !agents.iter().any(|agent| agent.key == "relay") {
                agents.push(SwarmAgent {
                    key: "relay".into(),
                    herdr_name: None,
                    role: Role::Script,
                    model: None,
                    pane_id: Some(pane.to_string()),
                    args: Vec::new(),
                    kind: Default::default(),
                });
            }
        }
        // Addons opened by `hivey swarm launch --addon` (relays etc.) are script panes.
        for addon in manifest
            .get("addons")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let plugin = addon
                .get("plugin")
                .and_then(Value::as_str)
                .unwrap_or("addon");
            let short = plugin.rsplit('.').next().unwrap_or(plugin);
            let key = if agents.iter().any(|agent| agent.key == short) {
                format!("{short}-addon")
            } else {
                short.to_string()
            };
            agents.push(SwarmAgent {
                key,
                herdr_name: None,
                role: Role::Script,
                model: None,
                pane_id: addon
                    .get("pane_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                args: Vec::new(),
                kind: Default::default(),
            });
        }
        agents.sort_by_key(|agent| agent.role.rank());
        Ok(Self {
            slug,
            root: root.to_path_buf(),
            paused: manifest.get("state").and_then(Value::as_str) == Some("paused"),
            solo: manifest.get("solo").and_then(Value::as_bool) == Some(true),
            home: manifest.get("home").and_then(Value::as_bool) == Some(true),
            profile: manifest
                .get("profile")
                .filter(|profile| profile.is_object())
                .cloned()
                .unwrap_or(Value::Null),
            agents,
        })
    }

    pub(crate) fn master(&self) -> Option<&SwarmAgent> {
        self.agents.iter().find(|agent| agent.role == Role::Master)
    }

    /// Resolve `scout`, `app-ideas-scout`, or the aliases `master` / `coordinator`.
    pub(crate) fn agent(&self, name: &str) -> Option<&SwarmAgent> {
        if name == "master" || name == "coordinator" {
            if let Some(master) = self.master() {
                return Some(master);
            }
        }
        self.agents
            .iter()
            .find(|agent| agent.key == name || agent.herdr_name.as_deref() == Some(name))
    }

    pub(crate) fn bus_path(&self) -> PathBuf {
        self.root.join(".swarm").join("bus.jsonl")
    }

    pub(crate) fn overflow_dir(&self) -> PathBuf {
        self.root.join(".swarm").join("inbox")
    }
}

fn agent_from_entry(key: &str, entry: &Value) -> SwarmAgent {
    let text = |field: &str| entry.get(field).and_then(Value::as_str).map(str::to_string);
    let role = text("role")
        .as_deref()
        .and_then(Role::parse)
        .unwrap_or_else(|| infer_role(key, entry));
    SwarmAgent {
        key: key.to_string(),
        herdr_name: text("herdr_name"),
        role,
        model: text("model"),
        pane_id: text("pane_id"),
        args: entry
            .get("args")
            .and_then(Value::as_array)
            .map(|args| {
                args.iter()
                    .filter_map(|a| a.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        kind: text("kind")
            .as_deref()
            .and_then(super::adapter::AgentKind::parse)
            .unwrap_or_default(),
    }
}

fn infer_role(key: &str, entry: &Value) -> Role {
    match key {
        "coordinator" | "master" => Role::Master,
        "critic" => Role::Critic,
        _ if entry.get("command").is_some() => Role::Script,
        _ => Role::Worker,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Value {
        serde_json::json!({
            "slug": "app-ideas",
            "coordinator": "app-ideas-coordinator",
            "agents": {
                "scout": {"herdr_name": "app-ideas-scout", "pane_id": "wB:p9", "model": "sonnet"},
                "critic": {"herdr_name": "app-ideas-critic", "model": "opus"},
                "watch": {"command": ["python3", "w.py"]}
            },
            "relay_pane_id": "wB:pE",
            "addons": [{"plugin": "hivey.slack-relay", "entrypoint": "relay", "pane_id": "wB:pF"}]
        })
    }

    #[test]
    fn loads_existing_skill_manifest_with_inferred_roles() {
        let swarm = Swarm::from_manifest(Path::new("/tmp/s"), &manifest()).unwrap();
        assert_eq!(swarm.slug, "app-ideas");
        let roles: Vec<_> = swarm
            .agents
            .iter()
            .map(|a| (a.key.as_str(), a.role))
            .collect();
        assert_eq!(roles[0], ("coordinator", Role::Master));
        assert!(roles.contains(&("scout", Role::Worker)));
        assert!(roles.contains(&("critic", Role::Critic)));
        assert!(roles.contains(&("watch", Role::Script)));
        assert!(roles.contains(&("relay", Role::Script)));
        assert!(roles.contains(&("slack-relay", Role::Script)));
        assert_eq!(roles.last().unwrap().1, Role::Script);
    }

    #[test]
    fn resolves_keys_herdr_names_and_master_aliases() {
        let swarm = Swarm::from_manifest(Path::new("/tmp/s"), &manifest()).unwrap();
        assert_eq!(swarm.agent("scout").unwrap().key, "scout");
        assert_eq!(swarm.agent("app-ideas-critic").unwrap().key, "critic");
        assert_eq!(
            swarm.agent("master").unwrap().herdr_name.as_deref(),
            Some("app-ideas-coordinator")
        );
        assert!(swarm.agent("nobody").is_none());
    }

    #[test]
    fn explicit_role_wins_over_inference() {
        let manifest = serde_json::json!({
            "slug": "x",
            "agents": {"lead": {"herdr_name": "x-lead", "role": "master"}}
        });
        let swarm = Swarm::from_manifest(Path::new("/tmp/x"), &manifest).unwrap();
        assert_eq!(swarm.master().unwrap().key, "lead");
        assert_eq!(swarm.agents.len(), 1);
    }
}
