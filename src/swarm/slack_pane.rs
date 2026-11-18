//! The human reaches an agent or swarm over Slack through its master, the pane named "slack"
//! (see autoname's `SLACK_TOKEN`). When that pane is gone (closed, crashed, lost in a restart)
//! and a message from the human is waiting for it, the engine reopens it: `hivey swarm
//! relaunch <slug> <master>` opens a new pane that continues the master's conversation, is
//! named "slack" like before, and gets the waiting messages. The hivey agent itself is kept
//! alive by `home::watch` instead.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::bus::{self, Kind, Message};
use super::model::{Role, Swarm, SwarmAgent};

/// Time a reopen gets to bring the pane up before it's tried again.
const RETRY: Duration = Duration::from_secs(5 * 60);

/// When each swarm's slack pane was last reopened. Pure, so the timing is unit-tested.
#[derive(Debug, Default)]
pub(crate) struct Reopen {
    last: HashMap<String, Instant>,
}

impl Reopen {
    /// True when `slug`'s slack pane should be reopened now (at most once per `RETRY`).
    pub(crate) fn due(&mut self, slug: &str, now: Instant) -> bool {
        if self
            .last
            .get(slug)
            .is_some_and(|at| now.duration_since(*at) < RETRY)
        {
            return false;
        }
        self.last.insert(slug.to_string(), now);
        true
    }
}

/// `agent` is a Slack-connected master (not the hivey agent) with a message from the human
/// that should wake it.
pub(crate) fn wanted(swarm: &Swarm, agent: &SwarmAgent, pending: &[&Message]) -> bool {
    swarm.slack
        && !swarm.home
        && !swarm.paused
        && agent.role == Role::Master
        && pending
            .iter()
            .any(|msg| msg.from == bus::HUMAN && msg.kind != Kind::Fyi)
}

/// Called when the master's pane is gone and messages are waiting for it.
pub(crate) fn reopen(reopen: &mut Reopen, swarm: &Swarm, agent: &SwarmAgent, pending: &[&Message]) {
    if wanted(swarm, agent, pending) && reopen.due(&swarm.slug, Instant::now()) {
        tracing::info!(slug = %swarm.slug, "hivey: reopening the slack pane for a message");
        super::addons::spawn_relaunch(&swarm.slug, &swarm.root, &[&agent.key], "slack-pane.log");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(from: &str, kind: Kind) -> Message {
        Message {
            id: "m1".into(),
            ts: 0,
            from: from.into(),
            swarm: Some("s".into()),
            to: "master".into(),
            addressed: "master".into(),
            kind,
            text: "hi".into(),
            reply_to: None,
            copy: false,
        }
    }

    fn fixture(slack: bool, home: bool) -> (Swarm, SwarmAgent, SwarmAgent) {
        let mut manifest = serde_json::json!({
            "slug": "s",
            "home": home,
            "agents": {
                "master": {"herdr_name": "s-master", "role": "master"},
                "scout": {"herdr_name": "s-scout", "role": "worker"},
            },
        });
        if slack {
            manifest["channel_id"] = serde_json::json!("C1");
        }
        let swarm = Swarm::from_manifest(std::path::Path::new("/tmp/s"), &manifest).expect("swarm");
        let master = swarm.agent("master").cloned().expect("master");
        let scout = swarm.agent("scout").cloned().expect("scout");
        (swarm, master, scout)
    }

    #[test]
    fn only_human_messages_to_a_slack_master_reopen_it() {
        let (swarm, master, scout) = fixture(true, false);
        let human = message(bus::HUMAN, Kind::Normal);
        let agent = message("s/scout", Kind::Normal);
        let fyi = message(bus::HUMAN, Kind::Fyi);
        assert!(wanted(&swarm, &master, &[&human]));
        assert!(!wanted(&swarm, &master, &[&agent]));
        assert!(!wanted(&swarm, &master, &[&fyi]));
        assert!(!wanted(&swarm, &scout, &[&human]));
        let (no_slack, master, _) = fixture(false, false);
        assert!(!wanted(&no_slack, &master, &[&human]));
        let (home, master, _) = fixture(true, true);
        assert!(!wanted(&home, &master, &[&human]));
    }

    #[test]
    fn reopens_once_then_waits_before_retrying() {
        let mut reopen = Reopen::default();
        let t0 = Instant::now();
        assert!(reopen.due("s", t0));
        assert!(!reopen.due("s", t0 + Duration::from_secs(60)));
        assert!(reopen.due("other", t0));
        assert!(reopen.due("s", t0 + RETRY));
    }
}
