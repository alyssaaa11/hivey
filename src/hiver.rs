//! hiver-specific process setup. Everything hiver adds on top of herdr lives in
//! `src/hiver.rs` and `src/swarm/`; core files only call in through small `// hiver:` hooks.

/// Set to `1` in every pane hiver starts, next to herdr's `HERDR_ENV=1`.
pub(crate) const HIVER_ENV_VAR: &str = "HIVER_ENV";

pub(crate) const SELF_UPDATE_DISABLED: &str =
    "hiver does not self-update; update from source: git pull && cargo build --release";

/// The hiver checkout this binary was built from, or `HIVER_REPO`.
pub(crate) fn repo() -> std::path::PathBuf {
    std::env::var_os("HIVER_REPO")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// `hiver update [--yes] [--check]`: runs `scripts/sync-herdr.sh` in the hiver repo (pull the
/// fork, merge herdr, build, test, install, live handoff). The repo is where this binary was
/// built, or `HIVER_REPO`. Never downloads herdr releases over hiver.
pub(crate) fn run_update(args: &[String]) -> i32 {
    let repo = repo();
    let script = repo.join("scripts").join("sync-herdr.sh");
    if !script.is_file() {
        eprintln!(
            "hiver update: no {} — set HIVER_REPO to your hiver checkout",
            script.display()
        );
        return 1;
    }
    match std::process::Command::new("bash")
        .arg(&script)
        .args(args)
        .current_dir(&repo)
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("hiver update: cannot run {}: {err}", script.display());
            1
        }
    }
}

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
