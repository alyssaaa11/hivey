//! hivey: agent settings, the defaults for every agent and the overrides of one swarm or agent
//! space (`~/.hivey/agent-settings.json`). Set in hivey settings → hibernate / autocompact (the
//! defaults), a space's right-click menu → Settings… (its overrides) and `hivey agent-settings`.
//!
//! - `hibernate_hours`: an agent idle this many hours is stopped to free its memory and resumed
//!   (same conversation, latest CLI) when its space is opened again; 0 = never. Default: 48.
//!   Older files say `hibernate_days`, still read (× 24) until the layer is set again.
//! - `autocompact`: Claude Code's auto-compact window, passed to each Claude agent as
//!   `--autocompact <tokens|auto>` when hivey starts or resumes it (launch, new session, wake
//!   from hibernation, restart, resume after a hivey restart). The flag lasts for that session
//!   only, so Claude's own settings are not touched. `off` (the default) passes nothing.
//!
//! ```json
//! { "default": { "hibernate_hours": 2 },
//!   "spaces": { "pharma-assets": { "autocompact": "400000" } } }
//! ```

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Hours an agent may stay idle before it is hibernated, unless set otherwise.
pub(crate) const DEFAULT_HIBERNATE_HOURS: u32 = 48;
/// The hour choices the settings offer (0 = off).
pub(crate) const HIBERNATE_CHOICES: &[u32] = &[0, 1, 2, 4, 8, 24, 48, 72, 120, 168, 336];
/// The longest idle time `--hibernate` takes: a year.
const HIBERNATE_MAX_HOURS: u32 = 365 * 24;
/// The auto-compact choices the settings offer.
pub(crate) const AUTOCOMPACT_CHOICES: &[AutoCompact] = &[
    AutoCompact::Off,
    AutoCompact::Auto,
    AutoCompact::Tokens(100_000),
    AutoCompact::Tokens(200_000),
    AutoCompact::Tokens(300_000),
    AutoCompact::Tokens(400_000),
    AutoCompact::Tokens(600_000),
    AutoCompact::Tokens(1_000_000),
];
/// Claude Code's `/autocompact` takes 100k to 1M tokens.
const AUTOCOMPACT_MIN: u64 = 100_000;
const AUTOCOMPACT_MAX: u64 = 1_000_000;

/// Claude Code's auto-compact window for an agent: how full its context gets before Claude
/// summarizes it by itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub(crate) enum AutoCompact {
    /// hivey leaves the agent's own setting alone.
    #[default]
    Off,
    /// Claude Code's window tuned for the model (`/autocompact auto`).
    Auto,
    Tokens(u64),
}

impl AutoCompact {
    /// "off", "auto", "400k", "1M".
    pub(crate) fn label(self) -> String {
        match self {
            Self::Off => "off".into(),
            Self::Auto => "auto".into(),
            Self::Tokens(tokens) => tokens_label(tokens),
        }
    }

    /// The value of Claude Code's `--autocompact` flag (`None`: no flag).
    pub(crate) fn flag_value(self) -> Option<String> {
        match self {
            Self::Off => None,
            Self::Auto => Some("auto".into()),
            Self::Tokens(tokens) => Some(tokens.to_string()),
        }
    }

    /// From "off", "auto", "400k", "400000", "1M".
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        match text.as_str() {
            "off" | "never" | "no" | "0" => return Some(Self::Off),
            "auto" => return Some(Self::Auto),
            _ => {}
        }
        let (number, unit) = match text.chars().last()? {
            'k' => (&text[..text.len() - 1], 1_000f64),
            'm' => (&text[..text.len() - 1], 1_000_000f64),
            _ => (text.as_str(), 1f64),
        };
        let value: f64 = number.trim().parse().ok()?;
        let tokens = (value * unit).round();
        (AUTOCOMPACT_MIN as f64..=AUTOCOMPACT_MAX as f64)
            .contains(&tokens)
            .then_some(Self::Tokens(tokens as u64))
    }
}

/// A Claude agent's arguments with `--autocompact` set to `value`: an earlier one is replaced
/// (or dropped for off), so a restart always gets the space's current setting.
pub(crate) fn with_autocompact(args: &[String], value: AutoCompact) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--autocompact" {
            iter.next();
            continue;
        }
        if arg.starts_with("--autocompact=") {
            continue;
        }
        out.push(arg.clone());
    }
    if let Some(flag) = value.flag_value() {
        out.push("--autocompact".into());
        out.push(flag);
    }
    out
}

/// `with_autocompact` with the setting of the space `slug` (`None`: the defaults), for Claude
/// agents only; other agents' arguments are returned as they are.
pub(crate) fn claude_args_for_space(
    agent: &str,
    args: &[String],
    slug: Option<&str>,
) -> Vec<String> {
    if agent != "claude" {
        return args.to_vec();
    }
    let settings = load();
    let args = with_autocompact(args, settings.effective(slug).autocompact);
    with_remote_control(args, settings.remote_control, slug)
}

/// A Claude agent's arguments with `--remote-control <slug>` added when `on` (kept when the
/// arguments already ask for Remote Control, removed when off).
pub(crate) fn with_remote_control(args: Vec<String>, on: bool, slug: Option<&str>) -> Vec<String> {
    let has = args
        .iter()
        .any(|arg| arg == "--remote-control" || arg.starts_with("--remote-control="));
    if on == has {
        return args;
    }
    if !on {
        let mut out = Vec::with_capacity(args.len());
        let mut iter = args.into_iter().peekable();
        while let Some(arg) = iter.next() {
            if arg == "--remote-control" {
                if iter.peek().is_some_and(|next| !next.starts_with('-')) {
                    iter.next();
                }
                continue;
            }
            if arg.starts_with("--remote-control=") {
                continue;
            }
            out.push(arg);
        }
        return out;
    }
    let mut out = args;
    match slug {
        Some(slug) => out.push(format!("--remote-control={slug}")),
        None => out.push("--remote-control".into()),
    }
    out
}

impl From<AutoCompact> for String {
    fn from(value: AutoCompact) -> Self {
        match value {
            AutoCompact::Off => "off".into(),
            AutoCompact::Auto => "auto".into(),
            AutoCompact::Tokens(tokens) => tokens.to_string(),
        }
    }
}

impl TryFrom<String> for AutoCompact {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value).ok_or_else(|| format!("invalid autocompact value {value:?}"))
    }
}

/// One layer of settings; a missing field falls through to the next layer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Layer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hibernate_hours: Option<u32>,
    /// Older files: the idle time in days. Read through `hibernate()`, cleared by `set_hibernate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hibernate_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub autocompact: Option<AutoCompact>,
}

impl Layer {
    fn is_empty(&self) -> bool {
        self.hibernate().is_none() && self.autocompact.is_none()
    }

    /// This layer's idle hours before hibernation, if it sets them.
    pub(crate) fn hibernate(&self) -> Option<u32> {
        self.hibernate_hours
            .or(self.hibernate_days.map(|days| days.saturating_mul(24)))
    }

    pub(crate) fn set_hibernate(&mut self, hours: Option<u32>) {
        self.hibernate_hours = hours;
        self.hibernate_days = None;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentSettings {
    #[serde(default)]
    pub default: Layer,
    /// Overrides per swarm / agent slug.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub spaces: BTreeMap<String, Layer>,
    /// Start every Claude agent with Remote Control (`--remote-control <slug>`), so it can be
    /// followed and steered from claude.ai and the Claude app and reached by cloud sessions.
    /// Set with `hivey cloud remote-control on|off`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remote_control: bool,
}

/// What applies to one space after the layers are merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Effective {
    pub hibernate_hours: u32,
    pub autocompact: AutoCompact,
}

impl AgentSettings {
    pub(crate) fn effective(&self, slug: Option<&str>) -> Effective {
        let space = slug.and_then(|slug| self.spaces.get(slug));
        Effective {
            hibernate_hours: space
                .and_then(Layer::hibernate)
                .or(self.default.hibernate())
                .unwrap_or(DEFAULT_HIBERNATE_HOURS),
            autocompact: space
                .and_then(|layer| layer.autocompact)
                .or(self.default.autocompact)
                .unwrap_or_default(),
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
/// "off", "2 hours", "1 day", "3 days", "30 hours".
pub(crate) fn hours_label(hours: u32) -> String {
    match hours {
        0 => "off".into(),
        1 => "1 hour".into(),
        24 => "1 day".into(),
        n if n % 24 == 0 => format!("{} days", n / 24),
        n => format!("{n} hours"),
    }
}

/// "400k", "1.5M", "off".
fn tokens_label(tokens: u64) -> String {
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
/// `--hibernate` values: "off", "2h", "90m" is not taken; "2" or "2d" are days (as before).
pub(crate) fn parse_hibernate(text: &str) -> Option<u32> {
    let text = text.trim().to_ascii_lowercase();
    if matches!(text.as_str(), "off" | "never" | "no") {
        return Some(0);
    }
    let hours_unit = ["hours", "hour", "hrs", "hr", "h"]
        .iter()
        .find_map(|unit| text.strip_suffix(unit));
    let hours = match hours_unit {
        Some(digits) => digits.trim().parse::<u32>().ok()?,
        None => {
            let digits = ["days", "day", "d"]
                .iter()
                .find_map(|unit| text.strip_suffix(unit))
                .unwrap_or(&text);
            digits.trim().parse::<u32>().ok()?.checked_mul(24)?
        }
    };
    Some(hours).filter(|hours| *hours <= HIBERNATE_MAX_HOURS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_control_is_added_once_and_removed_when_off() {
        let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let base = args(&["--model", "opus"]);
        let on = with_remote_control(base.clone(), true, Some("docs"));
        assert_eq!(on, args(&["--model", "opus", "--remote-control=docs"]));
        assert_eq!(with_remote_control(on.clone(), true, Some("docs")), on);
        assert_eq!(with_remote_control(on, false, Some("docs")), base);
        let spaced = args(&["--remote-control", "x", "--model", "opus"]);
        assert_eq!(with_remote_control(spaced, false, None), base);
    }

    #[test]
    fn a_space_overrides_the_defaults_field_by_field() {
        let mut settings = AgentSettings::default();
        assert_eq!(
            settings.effective(Some("x")),
            Effective {
                hibernate_hours: DEFAULT_HIBERNATE_HOURS,
                autocompact: AutoCompact::Off
            }
        );
        settings.default.autocompact = Some(AutoCompact::Tokens(200_000));
        settings.layer_mut(Some("x")).set_hibernate(Some(0));
        assert_eq!(
            settings.effective(Some("x")),
            Effective {
                hibernate_hours: 0,
                autocompact: AutoCompact::Tokens(200_000)
            }
        );
        assert_eq!(settings.effective(Some("y")).hibernate_hours, 48);
        assert_eq!(
            settings.effective(None).autocompact,
            AutoCompact::Tokens(200_000)
        );
    }

    #[test]
    fn saving_drops_empty_space_layers_and_round_trips_autocompact() {
        let dir = std::env::temp_dir().join(format!("hivey-agent-settings-{}", std::process::id()));
        let path = dir.join("agent-settings.json");
        let mut settings = AgentSettings::default();
        settings.layer_mut(Some("empty"));
        settings.layer_mut(Some("kept")).autocompact = Some(AutoCompact::Tokens(400_000));
        settings.default.autocompact = Some(AutoCompact::Auto);
        save_to(&path, &settings).unwrap();
        let loaded = load_from(&path);
        assert!(!loaded.spaces.contains_key("empty"));
        assert_eq!(
            loaded.spaces["kept"].autocompact,
            Some(AutoCompact::Tokens(400_000))
        );
        assert_eq!(loaded.default.autocompact, Some(AutoCompact::Auto));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_labels_and_commands() {
        assert_eq!(parse_hibernate("2"), Some(48));
        assert_eq!(parse_hibernate("3d"), Some(72));
        assert_eq!(parse_hibernate("7 days"), Some(168));
        assert_eq!(parse_hibernate("off"), Some(0));
        assert_eq!(parse_hibernate("x"), None);
        assert_eq!(
            AutoCompact::parse("400k"),
            Some(AutoCompact::Tokens(400_000))
        );
        assert_eq!(
            AutoCompact::parse("1M"),
            Some(AutoCompact::Tokens(1_000_000))
        );
        assert_eq!(
            AutoCompact::parse("350000"),
            Some(AutoCompact::Tokens(350_000))
        );
        assert_eq!(AutoCompact::parse("auto"), Some(AutoCompact::Auto));
        assert_eq!(AutoCompact::parse("off"), Some(AutoCompact::Off));
        assert_eq!(AutoCompact::parse("50k"), None);
        assert_eq!(AutoCompact::parse("2M"), None);
        assert_eq!(AutoCompact::Tokens(400_000).label(), "400k");
        assert_eq!(AutoCompact::Tokens(1_000_000).label(), "1M");
        let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            with_autocompact(
                &args(&["--model", "opus", "--autocompact", "200000"]),
                AutoCompact::Tokens(400_000)
            ),
            args(&["--model", "opus", "--autocompact", "400000"])
        );
        assert_eq!(
            with_autocompact(&args(&["--autocompact=auto", "--chrome"]), AutoCompact::Off),
            args(&["--chrome"])
        );
        assert_eq!(
            with_autocompact(&args(&[]), AutoCompact::Auto),
            args(&["--autocompact", "auto"])
        );
        assert_eq!(
            claude_args_for_space("codex", &args(&["--x"]), None),
            args(&["--x"])
        );
        assert_eq!(hours_label(1), "1 hour");
        assert_eq!(hours_label(2), "2 hours");
        assert_eq!(hours_label(48), "2 days");
        assert_eq!(hours_label(0), "off");
    }

    #[test]
    fn hibernate_takes_hours_and_still_reads_days() {
        assert_eq!(parse_hibernate("2h"), Some(2));
        assert_eq!(parse_hibernate("3 hours"), Some(3));
        assert_eq!(parse_hibernate("2"), Some(48));
        assert_eq!(parse_hibernate("1d"), Some(24));
        assert_eq!(parse_hibernate("off"), Some(0));
        assert_eq!(parse_hibernate("x"), None);
        assert_eq!(parse_hibernate("400d"), None);
        let old: Layer = serde_json::from_str(r#"{"hibernate_days":3}"#).expect("old layer");
        assert_eq!(old.hibernate(), Some(72));
        let mut layer = old;
        layer.set_hibernate(Some(2));
        assert_eq!(
            serde_json::to_string(&layer).expect("layer json"),
            r#"{"hibernate_hours":2}"#
        );
    }
}
