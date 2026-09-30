//! hiver-specific process setup. Everything hiver adds on top of herdr lives in
//! `src/hiver.rs` and `src/swarm/`; core files only call in through small `// hiver:` hooks.

/// Set to `1` in every pane hiver starts, next to herdr's `HERDR_ENV=1`.
pub(crate) const HIVER_ENV_VAR: &str = "HIVER_ENV";

pub(crate) const SELF_UPDATE_DISABLED: &str =
    "hiver does not self-update; update from source: git pull && cargo build --release";

/// Variables a herdr pane exports that would point a hiver process at herdr's server.
const INHERITED_HERDR_VARS: &[&str] = &[
    "HERDR_ENV",
    "HERDR_SOCKET_PATH",
    "HERDR_CLIENT_SOCKET_PATH",
    "HERDR_SESSION",
    "HERDR_CONFIG_PATH",
    "HERDR_BIN_PATH",
    "HERDR_PANE_ID",
    "HERDR_TAB_ID",
    "HERDR_WORKSPACE_ID",
    "HERDR_PANE_RUNTIME_ID",
    "HERDR_REATTACH_COMMAND",
];

/// hiver panes keep herdr's `HERDR_*` variables so existing tools (`herdr agent prompt`,
/// Claude hooks reading `HERDR_PANE_ID`) talk to hiver unchanged. But when hiver is
/// launched from inside a *herdr* pane, those variables point at herdr's server and
/// would make hiver attach to it or refuse to start as "nested". Drop them in that case.
/// Must run first in `main`, before any thread is spawned.
pub(crate) fn isolate_from_parent_herdr() {
    let in_herdr = std::env::var_os(crate::HERDR_ENV_VAR).is_some();
    let in_hiver = std::env::var_os(HIVER_ENV_VAR).is_some();
    if in_herdr && !in_hiver {
        for var in INHERITED_HERDR_VARS {
            std::env::remove_var(var);
        }
    }
}
