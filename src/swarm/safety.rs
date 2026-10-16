//! hivey: whether the safety hooks are on (`~/.hivey/safety.json`, default on). Set by
//! `hivey safety on|off` and settings → safety; `hivey migrate` registers the hooks to match.

use std::path::{Path, PathBuf};

/// `~/.hivey/safety.json` under `home`.
pub(crate) fn state_path(home: &Path) -> PathBuf {
    home.join(".hivey").join("safety.json")
}

/// Whether the safety hooks should be on for this home folder (default: on).
pub(crate) fn enabled_in(home: &Path) -> bool {
    std::fs::read_to_string(state_path(home))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|state| state["enabled"].as_bool())
        .unwrap_or(true)
}

/// Whether the safety hooks should be on (default: on, also when HOME is unknown).
pub(crate) fn enabled() -> bool {
    std::env::var_os("HOME").is_none_or(|home| enabled_in(Path::new(&home)))
}
