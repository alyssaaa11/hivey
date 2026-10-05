//! The hivey agent (`hivey home`): one Claude in `~/.hivey/agent`, registered as the solo
//! agent `hivey` (manifest `"home": true`), that the user talks to for everything hivey —
//! from its space or from the Slack channel `#hivey`. While `~/.hivey/config.json` says
//! `"enabled": true`, the server of the configured session (default: `default`) keeps it
//! alive: when it's gone, the engine runs `hivey home start`, which relaunches it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;

/// Slug (and agent name) of the hivey agent.
pub(crate) const SLUG: &str = "hivey";
/// Wait after the server starts: restored panes and agents come back first.
const GRACE: Duration = Duration::from_secs(20);
/// How often the engine looks.
const CHECK_EVERY: Duration = Duration::from_secs(30);
/// After a start: time the agent gets to come up before it counts as a failure.
const START_WAIT: Duration = Duration::from_secs(90);
/// Longest pause between attempts while starts keep failing.
const MAX_BACKOFF: Duration = Duration::from_secs(15 * 60);

/// `~/.hivey`: the hivey agent's folder and settings.
pub(crate) fn hivey_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".hivey"))
}

pub(crate) fn config_path() -> Option<PathBuf> {
    hivey_dir().map(|dir| dir.join("config.json"))
}

pub(crate) fn load_config() -> Value {
    config_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// The session whose server keeps the agent alive (`"session"`, default `default`).
pub(crate) fn configured_session(config: &Value) -> String {
    config["session"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(crate::session::DEFAULT_SESSION_NAME)
        .to_string()
}

/// Enabled, and this server is the configured session.
fn wanted_here(config: &Value) -> bool {
    let here = crate::session::active_name()
        .unwrap_or_else(|| crate::session::DEFAULT_SESSION_NAME.to_string());
    config["enabled"] == true && configured_session(config) == here
}

/// When to (re)start the agent. Pure, so the timing is unit-tested.
#[derive(Debug, Default)]
pub(crate) struct Watch {
    started: Option<Instant>,
    last_check: Option<Instant>,
    last_start: Option<Instant>,
    failures: u32,
}

impl Watch {
    /// True when a start should be run now. `wanted` (read only when a check is due):
    /// enabled for this session; `alive`: the agent is running.
    pub(crate) fn due(&mut self, now: Instant, wanted: impl FnOnce() -> bool, alive: bool) -> bool {
        let started = *self.started.get_or_insert(now);
        if now.duration_since(started) < GRACE
            || self
                .last_check
                .is_some_and(|at| now.duration_since(at) < CHECK_EVERY)
        {
            return false;
        }
        self.last_check = Some(now);
        if !wanted() {
            return false;
        }
        if alive {
            self.failures = 0;
            self.last_start = None;
            return false;
        }
        if let Some(at) = self.last_start {
            let wait = START_WAIT
                .saturating_mul(2u32.saturating_pow(self.failures))
                .min(MAX_BACKOFF);
            if now.duration_since(at) < wait {
                return false;
            }
            self.failures += 1;
        }
        self.last_start = Some(now);
        true
    }
}

/// Called by the engine every tick with whether the hivey agent is running.
pub(crate) fn watch(watch: &mut Watch, alive: bool) {
    let mut config = Value::Null;
    let wanted = || {
        config = load_config();
        wanted_here(&config)
    };
    if watch.due(Instant::now(), wanted, alive) {
        spawn_start(&config);
    }
}

/// `hivey home start --quiet` against this server, detached; its output goes to
/// `~/.hivey/agent/.swarm/home-start.log`.
fn spawn_start(config: &Value) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(exe);
    command.args(["home", "start", "--quiet"]);
    let session = configured_session(config);
    if session != crate::session::DEFAULT_SESSION_NAME {
        command.args(["--session", &session]);
    }
    let log = hivey_dir().map(|dir| dir.join("agent").join(".swarm").join("home-start.log"));
    if let Some(file) = log.and_then(|path| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
    }) {
        if let Ok(copy) = file.try_clone() {
            command.stdout(copy);
        }
        command.stderr(file);
    }
    command.stdin(std::process::Stdio::null());
    if let Err(err) = command.spawn() {
        tracing::warn!(%err, "hivey home: cannot start the hivey agent");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_for_grace_then_starts_once_and_backs_off() {
        let mut watch = Watch::default();
        let t0 = Instant::now();
        assert!(
            !watch.due(t0, || true, false),
            "first tick only starts the clock"
        );
        assert!(
            !watch.due(t0 + Duration::from_secs(10), || true, false),
            "grace"
        );
        assert!(
            watch.due(t0 + GRACE, || true, false),
            "gone after grace: start"
        );
        let t1 = t0 + GRACE;
        assert!(
            !watch.due(t1 + CHECK_EVERY, || true, false),
            "give it time to come up"
        );
        assert!(
            watch.due(t1 + START_WAIT, || true, false),
            "still gone: try again"
        );
        let t2 = t1 + START_WAIT;
        assert!(
            !watch.due(t2 + START_WAIT, || true, false),
            "after a failure the wait doubles"
        );
        assert!(watch.due(t2 + START_WAIT * 2, || true, false));
    }

    #[test]
    fn alive_resets_and_disabled_never_starts() {
        let mut watch = Watch::default();
        let t0 = Instant::now();
        watch.due(t0, || true, false);
        assert!(watch.due(t0 + GRACE, || true, false));
        assert!(!watch.due(t0 + GRACE + CHECK_EVERY, || true, true), "alive");
        assert!(
            watch.due(t0 + GRACE + CHECK_EVERY * 2, || true, false),
            "gone again: no backoff left from before"
        );
        let mut off = Watch::default();
        off.due(t0, || false, false);
        assert!(!off.due(t0 + GRACE * 10, || false, false));
    }
}
