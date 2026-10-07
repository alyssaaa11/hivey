//! hivey-specific process setup. Everything hivey adds on top of herdr lives in
//! `src/hivey.rs` and `src/swarm/`; core files only call in through small `// hivey:` hooks.

/// Set to `1` in every pane hivey starts, next to herdr's `HERDR_ENV=1`.
pub(crate) const HIVEY_ENV_VAR: &str = "HIVEY_ENV";

pub(crate) const SELF_UPDATE_DISABLED: &str =
    "hivey does not self-update; update from source: git pull && cargo build --release";

/// A hivey window open on this computer: `~/.hivey/windows/<pid>` exists while it runs. The
/// desktop pet shows while any window is open (it checks each pid is alive, so a crashed
/// window doesn't count) and quits itself after the last one closes; opening a window starts
/// the chosen pet (`hivey pet show`).
pub(crate) struct WindowMarker(Option<std::path::PathBuf>);

/// Pets are macOS desktop apps.
const PETS_SUPPORTED: bool = cfg!(target_os = "macos");

impl WindowMarker {
    pub(crate) fn open() -> Self {
        let Some(home) = std::env::var_os("HOME") else {
            return Self(None);
        };
        let dir = std::path::Path::new(&home).join(".hivey").join("windows");
        let path = dir.join(std::process::id().to_string());
        let marker = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(&path, b""))
            .map(|()| path)
            .map_err(|err| tracing::debug!(%err, "hivey window marker"))
            .ok();
        if PETS_SUPPORTED {
            run_in_background(&["pet", "show"]);
        }
        // A notification when Slack isn't connected (quiet offline; `hivey slack check off`)
        run_in_background(&["slack", "check"]);
        Self(marker)
    }
}

impl Drop for WindowMarker {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// `hivey <args>` in the background, e.g. `pet show` (it does nothing when no pet is chosen or
/// it's running) and `slack check`.
fn run_in_background(args: &[&str]) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let spawned = std::process::Command::new(exe)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match spawned {
        // Reaped off the main thread; it may build the pet for a minute
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(err) => tracing::debug!(%err, ?args, "hivey background command"),
    }
}

/// The hivey checkout this binary was built from, or `HIVEY_REPO`.
pub(crate) fn repo() -> std::path::PathBuf {
    std::env::var_os("HIVEY_REPO")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")))
}

/// `hivey update [--yes] [--check]`: runs `scripts/sync-herdr.sh` in the hivey repo (pull the
/// fork, merge herdr, build, test, install, live handoff). The repo is where this binary was
/// built, or `HIVEY_REPO`. Never downloads herdr releases over hivey.
pub(crate) fn run_update(args: &[String]) -> i32 {
    let repo = repo();
    let script = repo.join("scripts").join("sync-herdr.sh");
    if !script.is_file() {
        eprintln!(
            "hivey update: no {} — set HIVEY_REPO to your hivey checkout",
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
            eprintln!("hivey update: cannot run {}: {err}", script.display());
            1
        }
    }
}

/// Variables a hivey pane exports that would point a hivey process at herdr's server.
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

/// hivey panes keep herdr's `HERDR_*` variables so existing tools (`hivey agent prompt`,
/// Claude hooks reading `HERDR_PANE_ID`) talk to hivey unchanged. But when hivey is
/// launched from inside a *herdr* pane, those variables point at herdr's server and
/// would make hivey attach to it or refuse to start as "nested". Drop them in that case.
/// Must run first in `main`, before any thread is spawned.
pub(crate) fn isolate_from_parent_herdr() {
    let in_herdr = std::env::var_os(crate::HERDR_ENV_VAR).is_some();
    let in_hivey = std::env::var_os(HIVEY_ENV_VAR).is_some();
    if in_herdr && !in_hivey {
        for var in INHERITED_HERDR_VARS {
            std::env::remove_var(var);
        }
    }
}
