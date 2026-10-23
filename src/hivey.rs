//! hivey-specific process setup. Everything hivey adds on top of herdr lives in
//! `src/hivey.rs` and `src/swarm/`; core files only call in through small `// hivey:` hooks.

/// `hivey --agent [claude args…]`: Claude Code in the hivey agent's folder (`~/.hivey/agent`),
/// to talk to the hivey agent directly. Returns its exit code; `None` when not asked for.
pub(crate) fn maybe_run_agent(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some("--agent") {
        return None;
    }
    let Some(dir) = std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".hivey/agent"))
    else {
        eprintln!("hivey --agent: HOME is not set");
        return Some(1);
    };
    if !dir.join("CLAUDE.md").is_file() {
        eprintln!(
            "the hivey agent isn't set up yet ({} has no CLAUDE.md): run hivey home setup",
            dir.display()
        );
        return Some(1);
    }
    match std::process::Command::new("claude")
        .args(&args[2..])
        .current_dir(&dir)
        .status()
    {
        Ok(status) => Some(status.code().unwrap_or(1)),
        Err(err) => {
            eprintln!("hivey --agent: cannot start claude (is Claude Code installed?): {err}");
            Some(1)
        }
    }
}

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
        // The hivey menu's quiet mode label
        crate::swarm::quiet::is_on();
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

/// The hivey checkout this binary was built from, or `HIVEY_REPO`. A release binary installed
/// with npm was built elsewhere: its plugins, pets and scripts are in `@hivey/cli/repo`.
pub(crate) fn repo() -> std::path::PathBuf {
    if let Some(repo) = std::env::var_os("HIVEY_REPO") {
        return std::path::PathBuf::from(repo);
    }
    let built = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if built.is_dir() {
        return built;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| npm_repo(&exe))
        .unwrap_or(built)
}

/// `@hivey/cli/repo` for the binary npm installed at `@hivey/cli/native/<platform>/hivey`.
fn npm_repo(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let exe = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
    let native = exe.parent()?.parent()?;
    if native.file_name()? != "native" {
        return None;
    }
    let repo = native.parent()?.join("repo");
    repo.is_dir().then_some(repo)
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

#[cfg(test)]
mod tests {
    #[test]
    fn npm_repo_is_found_in_the_npm_package() {
        let root = std::env::temp_dir().join(format!("hivey-npm-repo-{}", std::process::id()));
        let cli = root.join("node_modules").join("@hivey").join("cli");
        let exe = cli.join("native").join("darwin-arm64").join("hivey");
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"").unwrap();
        assert_eq!(super::npm_repo(&exe), None, "no @hivey/cli/repo yet");

        std::fs::create_dir_all(cli.join("repo")).unwrap();
        let found = super::npm_repo(&exe).expect("repo of the npm package");
        assert!(found.ends_with("@hivey/cli/repo"), "{}", found.display());

        let elsewhere = root.join("bin").join("hivey");
        std::fs::create_dir_all(elsewhere.parent().unwrap()).unwrap();
        std::fs::write(&elsewhere, b"").unwrap();
        assert_eq!(super::npm_repo(&elsewhere), None);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
