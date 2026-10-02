//! Addons (Slack relay, dashboard) don't survive a server restart: their panes come back as
//! bare shells, so a Slack relay silently stops relaying. Once per server start, after restored
//! agents had time to come back, the engine runs `hiver swarm relaunch <slug> --addons-only`
//! for every swarm with addons; it reopens only addons that aren't running. Only once: an addon
//! the user closes later stays closed.

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::model::{manifest_path, Swarm};

/// Wait after the server starts: restored agents (and the hiver agent) come back first.
const GRACE: Duration = Duration::from_secs(45);

/// When to run the one restore pass. Pure, so the timing is unit-tested.
#[derive(Debug, Default)]
pub(crate) struct Restore {
    started: Option<Instant>,
    done: bool,
}

impl Restore {
    /// True exactly once, the first time it's asked after `GRACE`.
    pub(crate) fn due(&mut self, now: Instant) -> bool {
        let started = *self.started.get_or_insert(now);
        if self.done || now.duration_since(started) < GRACE {
            return false;
        }
        self.done = true;
        true
    }
}

/// Called by the engine every tick with the registered swarms.
pub(crate) fn restore(restore: &mut Restore, swarms: &[Swarm]) {
    if !restore.due(Instant::now()) {
        return;
    }
    for swarm in swarms.iter().filter(|swarm| has_addons(&swarm.root)) {
        spawn_relaunch(&swarm.slug, &swarm.root);
    }
}

fn has_addons(root: &Path) -> bool {
    std::fs::read_to_string(manifest_path(root))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|manifest| {
            manifest["addons"]
                .as_array()
                .map(|addons| !addons.is_empty())
        })
        .unwrap_or(false)
}

/// `hiver swarm relaunch <slug> --addons-only` against this server, detached; its output goes
/// to `<root>/.swarm/addons-restore.log`.
fn spawn_relaunch(slug: &str, root: &Path) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut command = std::process::Command::new(exe);
    if let Some(session) = crate::session::active_name()
        .filter(|name| name.as_str() != crate::session::DEFAULT_SESSION_NAME)
    {
        command.args(["--session", &session]);
    }
    command.args(["swarm", "relaunch", slug, "--addons-only"]);
    command.current_dir(root);
    let log = root.join(".swarm").join("addons-restore.log");
    if let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        if let Ok(copy) = file.try_clone() {
            command.stdout(copy);
        }
        command.stderr(file);
    }
    command.stdin(std::process::Stdio::null());
    if let Err(err) = command.spawn() {
        tracing::warn!(%err, slug, "cannot restore the swarm's addons");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_once_after_the_grace_period() {
        let mut restore = Restore::default();
        let t0 = Instant::now();
        assert!(!restore.due(t0));
        assert!(!restore.due(t0 + Duration::from_secs(30)));
        assert!(restore.due(t0 + GRACE));
        assert!(!restore.due(t0 + GRACE + Duration::from_secs(60)));
    }

    #[test]
    fn only_swarms_with_addons_need_it() {
        let dir = std::env::temp_dir().join(format!("hiver-addons-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".swarm")).unwrap();
        assert!(!has_addons(&dir));
        std::fs::write(manifest_path(&dir), r#"{"addons": []}"#).unwrap();
        assert!(!has_addons(&dir));
        std::fs::write(
            manifest_path(&dir),
            r#"{"addons": [{"plugin": "hiver.slack-relay", "pane_id": "w3:p2"}]}"#,
        )
        .unwrap();
        assert!(has_addons(&dir));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
