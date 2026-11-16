//! hivey: agent settings, the defaults for every agent and the overrides of one swarm or agent
//! space (`~/.hivey/agent-settings.json`). Set in hivey settings → hibernate / compact (the
//! defaults), a space's right-click menu → Settings… (its overrides) and `hivey agent-settings`.
//!
//! - `hibernate_days`: an agent idle this many days is stopped to free its memory and resumed
//!   (same conversation, latest CLI) when its space is opened again; 0 = never. Default: 2.
//! - `compact_at`: a Claude agent whose context passes this many tokens gets `/compact` typed
//!   in when it is idle; 0 = never (the default).
//!
//! ```json
//! { "default": { "hibernate_days": 2, "compact_at": 0 },
//!   "spaces": { "pharma-assets": { "compact_at": 400000 } } }
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Days an agent may stay idle before it is hibernated, unless set otherwise.
pub(crate) const DEFAULT_HIBERNATE_DAYS: u32 = 2;
/// The day choices the settings offer (0 = off).
pub(crate) const HIBERNATE_CHOICES: &[u32] = &[0, 1, 2, 3, 5, 7, 14];
/// The token choices the settings offer for compacting (0 = off).
pub(crate) const COMPACT_CHOICES: &[u64] =
    &[0, 100_000, 200_000, 300_000, 400_000, 600_000, 800_000];

/// One layer of settings; a missing field falls through to the next layer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Layer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hibernate_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_at: Option<u64>,
}

impl Layer {
    fn is_empty(&self) -> bool {
        self.hibernate_days.is_none() && self.compact_at.is_none()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentSettings {
    #[serde(default)]
    pub default: Layer,
    /// Overrides per swarm / agent slug.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spaces: BTreeMap<String, Layer>,
}

/// What applies to one space after the layers are merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Effective {
    pub hibernate_days: u32,
    pub compact_at: u64,
}

impl AgentSettings {
    pub(crate) fn effective(&self, slug: Option<&str>) -> Effective {
        let space = slug.and_then(|slug| self.spaces.get(slug));
        Effective {
            hibernate_days: space
                .and_then(|layer| layer.hibernate_days)
                .or(self.default.hibernate_days)
                .unwrap_or(DEFAULT_HIBERNATE_DAYS),
            compact_at: space
                .and_then(|layer| layer.compact_at)
                .or(self.default.compact_at)
                .unwrap_or(0),
        }
    }

    /// The layer to edit: the defaults (`None`) or one space's overrides.
    pub(crate) fn layer_mut(&mut self, slug: Option<&str>) -> &mut Layer {
        match slug {
            None => &mut self.default,
            Some(slug) => self.spaces.entry(slug.to_string()).or_default(),
        }
    }

    pub(crate) fn layer(&self, slug: Option<&str>) -> Layer {
        match slug {
            None => self.default.clone(),
            Some(slug) => self.spaces.get(slug).cloned().unwrap_or_default(),
        }
    }

    fn tidy(&mut self) {
        self.spaces.retain(|_, layer| !layer.is_empty());
    }
}

/// Workspace → swarm/agent slug, kept current by the swarm engine: which space's settings
/// apply to a pane.
static SPACE_SLUGS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

pub(crate) fn set_space_slugs(slugs: HashMap<String, String>) {
    if let Ok(mut current) = SPACE_SLUGS.lock() {
        *current = Some(slugs);
    }
}

pub(crate) fn space_slug(workspace_id: &str) -> Option<String> {
    SPACE_SLUGS
        .lock()
        .ok()?
        .as_ref()?
        .get(workspace_id)
        .cloned()
}

pub(crate) fn path_in(home: &Path) -> PathBuf {
    home.join(".hivey").join("agent-settings.json")
}

fn path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| path_in(Path::new(&home)))
}

pub(crate) fn load_from(path: &Path) -> AgentSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn load() -> AgentSettings {
    path().map(|path| load_from(&path)).unwrap_or_default()
}

pub(crate) fn save_to(path: &Path, settings: &AgentSettings) -> std::io::Result<()> {
    let mut settings = settings.clone();
    settings.tidy();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(&settings).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n")?;
    std::fs::rename(tmp, path)
}

/// Loads, changes and saves the settings in one go.
pub(crate) fn update(change: impl FnOnce(&mut AgentSettings)) -> std::io::Result<AgentSettings> {
    let path = path().ok_or_else(|| std::io::Error::other("HOME is not set"))?;
    let mut settings = load_from(&path);
    change(&mut settings);
    save_to(&path, &settings)?;
    Ok(load_from(&path))
}

/// "2 days", "1 day", "off".
pub(crate) fn days_label(days: u32) -> String {
    match days {
        0 => "off".into(),
        1 => "1 day".into(),
        n => format!("{n} days"),
    }
}

/// "400k", "1.5M", "off".
pub(crate) fn tokens_label(tokens: u64) -> String {
    match tokens {
        0 => "off".into(),
        n if n >= 1_000_000 && n % 100_000 == 0 => {
            let tenths = n / 100_000;
            if tenths % 10 == 0 {
                format!("{}M", tenths / 10)
            } else {
                format!("{}.{}M", tenths / 10, tenths % 10)
            }
        }
        n if n >= 1_000 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

/// Days from "2", "2d", "off", "0", "never".
pub(crate) fn parse_days(text: &str) -> Option<u32> {
    let text = text.trim().to_ascii_lowercase();
    if matches!(text.as_str(), "off" | "never" | "no") {
        return Some(0);
    }
    let digits = text
        .trim_end_matches("days")
        .trim_end_matches("day")
        .trim_end_matches('d')
        .trim();
    digits.parse().ok().filter(|days| *days <= 365)
}

/// Tokens from "400k", "400000", "1m", "1.5M", "off".
pub(crate) fn parse_tokens(text: &str) -> Option<u64> {
    let text = text.trim().to_ascii_lowercase();
    if matches!(text.as_str(), "off" | "never" | "no" | "0") {
        return Some(0);
    }
    let (number, unit) = match text.chars().last()? {
        'k' => (&text[..text.len() - 1], 1_000f64),
        'm' => (&text[..text.len() - 1], 1_000_000f64),
        _ => (text.as_str(), 1f64),
    };
    let value: f64 = number.trim().parse().ok()?;
    let tokens = (value * unit).round();
    (10_000.0..=10_000_000.0).contains(&tokens).then_some(tokens as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_space_overrides_the_defaults_field_by_field() {
        let mut settings = AgentSettings::default();
        assert_eq!(
            settings.effective(Some("x")),
            Effective {
                hibernate_days: DEFAULT_HIBERNATE_DAYS,
                compact_at: 0
            }
        );
        settings.default.compact_at = Some(200_000);
        settings.layer_mut(Some("x")).hibernate_days = Some(0);
        assert_eq!(
            settings.effective(Some("x")),
            Effective {
                hibernate_days: 0,
                compact_at: 200_000
            }
        );
        assert_eq!(settings.effective(Some("y")).hibernate_days, 2);
        assert_eq!(settings.effective(None).compact_at, 200_000);
    }

    #[test]
    fn saving_drops_empty_space_layers() {
        let dir = std::env::temp_dir().join(format!("hivey-agent-settings-{}", std::process::id()));
        let path = dir.join("agent-settings.json");
        let mut settings = AgentSettings::default();
        settings.layer_mut(Some("empty"));
        settings.layer_mut(Some("kept")).compact_at = Some(400_000);
        save_to(&path, &settings).unwrap();
        let loaded = load_from(&path);
        assert!(!loaded.spaces.contains_key("empty"));
        assert_eq!(loaded.spaces["kept"].compact_at, Some(400_000));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_and_labels_days_and_tokens() {
        assert_eq!(parse_days("2"), Some(2));
        assert_eq!(parse_days("3d"), Some(3));
        assert_eq!(parse_days("7 days"), Some(7));
        assert_eq!(parse_days("off"), Some(0));
        assert_eq!(parse_days("x"), None);
        assert_eq!(parse_tokens("400k"), Some(400_000));
        assert_eq!(parse_tokens("1.5M"), Some(1_500_000));
        assert_eq!(parse_tokens("350000"), Some(350_000));
        assert_eq!(parse_tokens("off"), Some(0));
        assert_eq!(parse_tokens("5"), None);
        assert_eq!(tokens_label(400_000), "400k");
        assert_eq!(tokens_label(1_500_000), "1.5M");
        assert_eq!(tokens_label(0), "off");
        assert_eq!(days_label(1), "1 day");
        assert_eq!(days_label(0), "off");
    }
}
