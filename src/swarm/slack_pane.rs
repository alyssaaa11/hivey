//! The human reaches an agent or swarm over Slack through its master, the pane named "slack"
//! (see autoname's `SLACK_TOKEN`). When that pane is gone (closed, crashed, lost in a restart)
//! and a message from the human is waiting for it, the engine reopens it: `hivey swarm
//! relaunch <slug> <master>` opens a new pane that continues the master's conversation, is
//! named "slack" like before, and gets the waiting messages. The hivey agent itself is kept
//! alive by `home::watch` instead.
//!
//! The slack pane is also kept in the leftmost column of its tab: every `SIDE_EVERY` it is
//! swapped with its left-hand neighbour until it has none, so the layout itself is unchanged.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::bus::{self, Kind, Message};
use super::model::{Role, Swarm, SwarmAgent};
use crate::api::schema::{Method, PaneDirection, PaneNeighborParams, PaneSwapParams};

/// How often the slack panes are checked for a left-hand neighbour.
pub(crate) const SIDE_EVERY: Duration = Duration::from_secs(5);
/// Most swaps per check: enough for any real layout, and never a loop.
const MAX_SWAPS: usize = 8;

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

/// Swaps `pane` leftwards until nothing is to its left. `call` runs an API method.
pub(crate) fn keep_left(pane: &str, mut call: impl FnMut(Method) -> Result<Value, String>) {
    for _ in 0..MAX_SWAPS {
        let neighbor = call(Method::PaneNeighbor(PaneNeighborParams {
            pane_id: Some(pane.to_string()),
            direction: PaneDirection::Left,
        }))
        .ok()
        .and_then(|result| neighbor(&result));
        let Some(neighbor) = neighbor else { return };
        let swap = Method::PaneSwap(PaneSwapParams {
            pane_id: None,
            direction: None,
            source_pane_id: Some(pane.to_string()),
            target_pane_id: Some(neighbor),
        });
        if let Err(err) = call(swap) {
            tracing::warn!(%pane, %err, "hivey: cannot move the slack pane left");
            return;
        }
    }
}

/// The `pane.neighbor` result's pane id, when there is one.
fn neighbor(result: &Value) -> Option<String> {
    result["neighbor"]["neighbor_pane_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
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
    fn swaps_left_until_nothing_is_left() {
        // A row of three: a, b, slack. Each swap moves slack one place left.
        let mut row = vec!["a", "b", "slack"];
        let mut swaps = 0;
        keep_left("slack", |method| match method {
            Method::PaneNeighbor(params) => {
                let at = row
                    .iter()
                    .position(|p| Some(*p) == params.pane_id.as_deref());
                let next = at
                    .and_then(|i| i.checked_sub(1))
                    .and_then(|i| row.get(i))
                    .copied();
                Ok(serde_json::json!({"neighbor": {"neighbor_pane_id": next}}))
            }
            Method::PaneSwap(params) => {
                let a = row
                    .iter()
                    .position(|p| Some(*p) == params.source_pane_id.as_deref());
                let b = row
                    .iter()
                    .position(|p| Some(*p) == params.target_pane_id.as_deref());
                row.swap(a.expect("source"), b.expect("target"));
                swaps += 1;
                Ok(Value::Null)
            }
            _ => Err("unexpected".into()),
        });
        assert_eq!(row, vec!["slack", "a", "b"]);
        assert_eq!(swaps, 2);
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
