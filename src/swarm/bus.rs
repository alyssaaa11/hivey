//! The swarm message bus: addressing, the on-disk log, the delivery policy and digests.
//!
//! What herdr's bare `agent prompt` lacks, and where this module fixes it:
//! - timing: `decide` delivers only when the target is idle (or the message is urgent);
//! - queue/inbox: undelivered messages are rebuilt from `bus.jsonl` (`pending_from_log`),
//!   blocked/gone targets are held and their master is told;
//! - addressing: `resolve` expands `@all`, `@role:<role>`, `@masters`, `<slug>/<agent>`;
//! - record: every message and receipt is appended to `<root>/.swarm/bus.jsonl`.
//!
//! Everything here is pure or file-local so it can be tested without a server.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::model::{Role, Swarm};

/// Longest digest pasted into a pane; longer ones go to a file the prompt points at.
pub(crate) const MAX_PROMPT_CHARS: usize = 4000;
/// An idle agent must stay idle this long before a normal message is typed into it,
/// so a message never lands in the gap between two turns.
pub(crate) const IDLE_SETTLE_MS: u64 = 3000;

pub(crate) const HUMAN: &str = "human";
pub(crate) const HIVER: &str = "hiver";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    #[default]
    Normal,
    /// Informational: never wakes the target; rides along with its next delivery.
    Fyi,
    /// Delivered even while the target is working (the agent CLI queues it).
    Urgent,
}

impl Kind {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "normal" => Some(Self::Normal),
            "fyi" => Some(Self::Fyi),
            "urgent" => Some(Self::Urgent),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Message {
    pub id: String,
    /// Milliseconds since the Unix epoch.
    pub ts: u64,
    /// `<slug>/<agent>`, `human`, or `hiver`.
    pub from: String,
    /// Recipient swarm slug (absent for messages to the human).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swarm: Option<String>,
    /// Recipient agent key inside `swarm`, or `human`.
    pub to: String,
    /// The address as the sender wrote it (`@all`, `critic`, `tonight-up/coordinator`).
    pub addressed: String,
    #[serde(default)]
    pub kind: Kind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    /// Sender-side copy of a cross-swarm message: shown in logs, never delivered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub copy: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub(crate) enum Record {
    Msg(Message),
    Delivered {
        id: String,
        to: String,
        ts: u64,
        /// Messages delivered together in one prompt.
        batch: usize,
    },
    Read {
        id: String,
        to: String,
        ts: u64,
    },
    /// Target was blocked/gone; its master was told (once per message).
    Held {
        id: String,
        to: String,
        ts: u64,
        reason: String,
    },
}

pub(crate) fn append(path: &Path, records: &[Record]) -> std::io::Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut buf = String::new();
    for record in records {
        buf.push_str(&serde_json::to_string(record).map_err(std::io::Error::other)?);
        buf.push('\n');
    }
    // One write per batch keeps concurrent appenders from interleaving lines.
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(buf.as_bytes())
}

/// Reads a bus log, skipping lines it can't parse (a torn last line after a crash).
pub(crate) fn read_log(path: &Path) -> Vec<Record> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Undelivered messages (in send order) and the ids whose master was already told.
pub(crate) fn pending_from_log(records: &[Record]) -> (Vec<Message>, Vec<String>) {
    let mut delivered = std::collections::HashSet::new();
    let mut held = Vec::new();
    for record in records {
        match record {
            Record::Delivered { id, .. } => {
                delivered.insert(id.as_str());
            }
            Record::Held { id, .. } => held.push(id.clone()),
            _ => {}
        }
    }
    let pending = records
        .iter()
        .filter_map(|record| match record {
            Record::Msg(msg)
                if !msg.copy && msg.to != HUMAN && !delivered.contains(msg.id.as_str()) =>
            {
                Some(msg.clone())
            }
            _ => None,
        })
        .collect();
    (pending, held)
}

// ---------------------------------------------------------------------------
// Addressing
// ---------------------------------------------------------------------------

/// Who is sending: an agent of a swarm, the human, or hiver itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sender {
    pub swarm: Option<String>,
    pub key: String,
    pub role: Option<Role>,
}

impl Sender {
    pub(crate) fn human() -> Self {
        Self {
            swarm: None,
            key: HUMAN.into(),
            role: None,
        }
    }

    pub(crate) fn label(&self) -> String {
        match &self.swarm {
            Some(slug) => format!("{slug}/{}", self.key),
            None => self.key.clone(),
        }
    }

    /// Masters, the human and hiver may address other swarms; workers go through their master.
    fn may_cross_swarms(&self) -> bool {
        self.swarm.is_none() || self.role == Some(Role::Master)
    }

    fn is(&self, swarm: &Swarm, key: &str) -> bool {
        self.swarm.as_deref() == Some(swarm.slug.as_str()) && self.key == key
    }
}

/// One resolved recipient: `(swarm slug, agent key)`; `(None, "human")` for the human.
pub(crate) type Recipient = (Option<String>, String);

/// Expands an address into recipients. `default_swarm` is the swarm bare names refer to
/// (the sender's own, or `--swarm`).
pub(crate) fn resolve(
    address: &str,
    sender: &Sender,
    default_swarm: Option<&str>,
    swarms: &[Swarm],
) -> Result<Vec<Recipient>, String> {
    let address = address.trim();
    if address.is_empty() {
        return Err("empty address".into());
    }
    if address == HUMAN || address == "@human" {
        return Ok(vec![(None, HUMAN.into())]);
    }
    let find_swarm = |slug: &str| {
        swarms
            .iter()
            .find(|swarm| swarm.slug == slug)
            .ok_or_else(|| format!("no swarm named {slug:?} (hiver swarm list)"))
    };
    let home = || -> Result<&Swarm, String> {
        match default_swarm {
            Some(slug) => find_swarm(slug),
            None => Err(format!(
                "{address:?} needs a swarm: use <swarm>/<agent> or --swarm <swarm>"
            )),
        }
    };
    let members = |swarm: &Swarm, keep: &dyn Fn(Role) -> bool| -> Vec<Recipient> {
        swarm
            .agents
            .iter()
            .filter(|agent| agent.role.receives_messages() && keep(agent.role))
            .filter(|agent| !sender.is(swarm, &agent.key))
            .map(|agent| (Some(swarm.slug.clone()), agent.key.clone()))
            .collect()
    };

    let recipients = if address == "@masters" {
        if !sender.may_cross_swarms() {
            return Err("only masters can message @masters; ask your master".into());
        }
        swarms
            .iter()
            .flat_map(|swarm| members(swarm, &|role| role == Role::Master))
            .collect()
    } else if address == "@all" {
        members(home()?, &|_| true)
    } else if let Some(role) = address.strip_prefix("@role:") {
        let role = Role::parse(role).ok_or_else(|| format!("unknown role {role:?}"))?;
        members(home()?, &|candidate| candidate == role)
    } else if let Some((slug, name)) = address.split_once('/') {
        let swarm = find_swarm(slug)?;
        let crossing = sender.swarm.as_deref() != Some(slug);
        if crossing && !sender.may_cross_swarms() {
            return Err(format!(
                "only masters can message another swarm; ask your master to contact {address}"
            ));
        }
        vec![direct(swarm, name)?]
    } else {
        vec![direct(home()?, address)?]
    };
    if recipients.is_empty() {
        return Err(format!("{address:?} matches nobody"));
    }
    Ok(recipients)
}

fn direct(swarm: &Swarm, name: &str) -> Result<Recipient, String> {
    let agent = swarm
        .agent(name)
        .ok_or_else(|| format!("swarm {:?} has no agent {name:?}", swarm.slug))?;
    if !agent.role.receives_messages() {
        return Err(format!("{} is a script and can't read messages", agent.key));
    }
    Ok((Some(swarm.slug.clone()), agent.key.clone()))
}

// ---------------------------------------------------------------------------
// Delivery policy
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Presence {
    /// Idle or done for `idle_ms`.
    Idle { idle_ms: u64 },
    /// Working, or a state hiver can't read (treated as busy).
    Busy,
    /// Waiting on a dialog: typing into it could answer the dialog by accident.
    Blocked,
    /// No live pane.
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Deliver,
    Hold,
    /// Keep holding, and tell the target's master (once per message).
    Escalate(&'static str),
}

pub(crate) fn decide(presence: Presence, pending: &[&Message]) -> Decision {
    let wakes = pending.iter().any(|msg| msg.kind != Kind::Fyi);
    if !wakes {
        return Decision::Hold;
    }
    let urgent = pending.iter().any(|msg| msg.kind == Kind::Urgent);
    match presence {
        Presence::Blocked => Decision::Escalate("blocked"),
        Presence::Gone => Decision::Escalate("gone"),
        Presence::Idle { idle_ms } if idle_ms >= IDLE_SETTLE_MS => Decision::Deliver,
        Presence::Idle { .. } | Presence::Busy if urgent => Decision::Deliver,
        Presence::Idle { .. } | Presence::Busy => Decision::Hold,
    }
}

// ---------------------------------------------------------------------------
// Digest
// ---------------------------------------------------------------------------

/// Shortens `app-ideas/scout` to `scout` when it's from the recipient's own swarm.
fn sender_label(from: &str, recipient_swarm: Option<&str>) -> String {
    match (from.split_once('/'), recipient_swarm) {
        (Some((slug, key)), Some(home)) if slug == home => key.to_string(),
        _ => from.to_string(),
    }
}

/// One prompt for everything waiting: wake-up messages first, FYIs after.
pub(crate) fn digest(pending: &[&Message]) -> String {
    let home = pending.first().and_then(|msg| msg.swarm.as_deref());
    let line = |msg: &Message| {
        let from = sender_label(&msg.from, home);
        let tag = match msg.kind {
            Kind::Urgent => " (urgent)",
            _ => "",
        };
        let reply = msg
            .reply_to
            .as_deref()
            .map(|id| format!(" (re {id})"))
            .unwrap_or_default();
        format!("from {from}{tag}{reply} [{}]: {}", msg.id, msg.text.trim())
    };
    let wakes: Vec<_> = pending.iter().filter(|msg| msg.kind != Kind::Fyi).collect();
    let fyis: Vec<_> = pending.iter().filter(|msg| msg.kind == Kind::Fyi).collect();
    let mut out = String::new();
    if pending.len() == 1 {
        out.push_str(&format!("[hiver] {}", line(pending[0])));
    } else {
        out.push_str(&format!("[hiver · {} messages]", pending.len()));
        for (index, msg) in wakes.iter().enumerate() {
            out.push_str(&format!("\n{}. {}", index + 1, line(msg)));
        }
        if !fyis.is_empty() {
            out.push_str("\nFYI (no reply needed):");
            for msg in &fyis {
                out.push_str(&format!("\n- {}", line(msg)));
            }
        }
    }
    out.push_str(
        "\n(Reply only if you must act or were asked a question. Acks and status updates: hiver msg send <agent> --fyi \"…\")",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn swarm(slug: &str, workers: &[&str]) -> Swarm {
        let mut agents = serde_json::Map::new();
        agents.insert(
            "coordinator".into(),
            serde_json::json!({"herdr_name": format!("{slug}-coordinator")}),
        );
        for worker in workers {
            agents.insert(
                (*worker).into(),
                serde_json::json!({"herdr_name": format!("{slug}-{worker}")}),
            );
        }
        agents.insert(
            "critic".into(),
            serde_json::json!({"herdr_name": format!("{slug}-critic")}),
        );
        agents.insert("relay".into(), serde_json::json!({"command": ["relay"]}));
        Swarm::from_manifest(
            Path::new("/tmp"),
            &serde_json::json!({"slug": slug, "agents": agents}),
        )
        .unwrap()
    }

    fn swarms() -> Vec<Swarm> {
        vec![
            swarm("ideas", &["scout", "analyst"]),
            swarm("tonight", &["backend"]),
        ]
    }

    fn agent(swarm: &str, key: &str, role: Role) -> Sender {
        Sender {
            swarm: Some(swarm.into()),
            key: key.into(),
            role: Some(role),
        }
    }

    fn msg(id: &str, kind: Kind) -> Message {
        Message {
            id: id.into(),
            ts: 1,
            from: "ideas/scout".into(),
            swarm: Some("ideas".into()),
            to: "analyst".into(),
            addressed: "analyst".into(),
            kind,
            text: format!("text {id}"),
            reply_to: None,
            copy: false,
        }
    }

    fn keys(recipients: &[Recipient]) -> Vec<String> {
        recipients
            .iter()
            .map(|(swarm, key)| format!("{}/{key}", swarm.as_deref().unwrap_or("-")))
            .collect()
    }

    // --- addressing -------------------------------------------------------

    #[test]
    fn all_reaches_everyone_in_the_swarm_except_sender_and_scripts() {
        let sender = agent("ideas", "scout", Role::Worker);
        let got = resolve("@all", &sender, Some("ideas"), &swarms()).unwrap();
        assert_eq!(
            keys(&got),
            ["ideas/coordinator", "ideas/analyst", "ideas/critic"]
        );
    }

    #[test]
    fn role_addressing_selects_by_role() {
        let sender = agent("ideas", "coordinator", Role::Master);
        let got = resolve("@role:worker", &sender, Some("ideas"), &swarms()).unwrap();
        // Manifest maps are key-sorted (serde_json::Map), so compare as a set.
        let mut got = keys(&got);
        got.sort();
        assert_eq!(got, ["ideas/analyst", "ideas/scout"]);
        let got = resolve("@role:critic", &sender, Some("ideas"), &swarms()).unwrap();
        assert_eq!(keys(&got), ["ideas/critic"]);
        assert!(resolve("@role:boss", &sender, Some("ideas"), &swarms()).is_err());
    }

    #[test]
    fn masters_reach_all_other_masters_and_workers_may_not() {
        let master = agent("ideas", "coordinator", Role::Master);
        let got = resolve("@masters", &master, Some("ideas"), &swarms()).unwrap();
        assert_eq!(keys(&got), ["tonight/coordinator"]);
        let got = resolve("@masters", &Sender::human(), None, &swarms()).unwrap();
        assert_eq!(keys(&got), ["ideas/coordinator", "tonight/coordinator"]);
        let worker = agent("ideas", "scout", Role::Worker);
        assert!(resolve("@masters", &worker, Some("ideas"), &swarms()).is_err());
    }

    #[test]
    fn cross_swarm_is_for_masters_and_the_human_only() {
        let master = agent("ideas", "coordinator", Role::Master);
        let got = resolve("tonight/master", &master, Some("ideas"), &swarms()).unwrap();
        assert_eq!(keys(&got), ["tonight/coordinator"]);
        let worker = agent("ideas", "scout", Role::Worker);
        let err = resolve("tonight/backend", &worker, Some("ideas"), &swarms()).unwrap_err();
        assert!(err.contains("ask your master"), "{err}");
        // Own swarm via the qualified form is fine for anyone.
        assert!(resolve("ideas/critic", &worker, Some("ideas"), &swarms()).is_ok());
    }

    #[test]
    fn direct_names_resolve_by_key_herdr_name_and_alias_but_not_scripts() {
        let sender = agent("ideas", "scout", Role::Worker);
        for name in ["analyst", "ideas-analyst"] {
            let got = resolve(name, &sender, Some("ideas"), &swarms()).unwrap();
            assert_eq!(keys(&got), ["ideas/analyst"]);
        }
        let got = resolve("master", &sender, Some("ideas"), &swarms()).unwrap();
        assert_eq!(keys(&got), ["ideas/coordinator"]);
        assert!(resolve("relay", &sender, Some("ideas"), &swarms())
            .unwrap_err()
            .contains("script"));
        assert!(resolve("ghost", &sender, Some("ideas"), &swarms()).is_err());
    }

    #[test]
    fn human_without_swarm_must_qualify_bare_names() {
        assert!(resolve("scout", &Sender::human(), None, &swarms()).is_err());
        assert!(resolve("ideas/scout", &Sender::human(), None, &swarms()).is_ok());
        assert_eq!(
            keys(
                &resolve(
                    "human",
                    &agent("ideas", "scout", Role::Worker),
                    Some("ideas"),
                    &swarms()
                )
                .unwrap()
            ),
            ["-/human"]
        );
    }

    // --- timing / policy --------------------------------------------------

    #[test]
    fn normal_messages_wait_while_the_agent_works() {
        let m = msg("m1", Kind::Normal);
        assert_eq!(decide(Presence::Busy, &[&m]), Decision::Hold);
        assert_eq!(
            decide(Presence::Idle { idle_ms: 500 }, &[&m]),
            Decision::Hold
        );
        assert_eq!(
            decide(
                Presence::Idle {
                    idle_ms: IDLE_SETTLE_MS
                },
                &[&m]
            ),
            Decision::Deliver
        );
    }

    #[test]
    fn urgent_messages_interrupt_but_never_type_into_a_dialog() {
        let m = msg("m1", Kind::Urgent);
        assert_eq!(decide(Presence::Busy, &[&m]), Decision::Deliver);
        assert_eq!(
            decide(Presence::Idle { idle_ms: 0 }, &[&m]),
            Decision::Deliver
        );
        assert_eq!(
            decide(Presence::Blocked, &[&m]),
            Decision::Escalate("blocked")
        );
    }

    #[test]
    fn fyi_never_wakes_anyone() {
        let m = msg("m1", Kind::Fyi);
        assert_eq!(
            decide(Presence::Idle { idle_ms: 60_000 }, &[&m]),
            Decision::Hold
        );
        // …but rides along once something real wakes the agent.
        let n = msg("m2", Kind::Normal);
        assert_eq!(
            decide(Presence::Idle { idle_ms: 60_000 }, &[&m, &n]),
            Decision::Deliver
        );
    }

    #[test]
    fn blocked_or_gone_targets_hold_and_escalate() {
        let m = msg("m1", Kind::Normal);
        assert_eq!(
            decide(Presence::Blocked, &[&m]),
            Decision::Escalate("blocked")
        );
        assert_eq!(decide(Presence::Gone, &[&m]), Decision::Escalate("gone"));
    }

    // --- record / queue ---------------------------------------------------

    #[test]
    fn log_roundtrip_rebuilds_the_queue_after_restart() {
        let dir = std::env::temp_dir().join(format!("hiver-bus-{}", std::process::id()));
        let path = dir.join("bus.jsonl");
        let _ = std::fs::remove_file(&path);
        let mut copy = msg("m3", Kind::Normal);
        copy.copy = true;
        let mut to_human = msg("m4", Kind::Normal);
        to_human.to = HUMAN.into();
        append(
            &path,
            &[
                Record::Msg(msg("m1", Kind::Normal)),
                Record::Msg(msg("m2", Kind::Fyi)),
                Record::Msg(copy),
                Record::Msg(to_human),
                Record::Delivered {
                    id: "m1".into(),
                    to: "analyst".into(),
                    ts: 2,
                    batch: 1,
                },
                Record::Held {
                    id: "m2".into(),
                    to: "analyst".into(),
                    ts: 3,
                    reason: "gone".into(),
                },
            ],
        )
        .unwrap();
        // A torn last line (crash mid-write) is ignored.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"ev\":\"msg\",")
            .unwrap();
        let records = read_log(&path);
        assert_eq!(records.len(), 6);
        let (pending, held) = pending_from_log(&records);
        assert_eq!(
            pending.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["m2"]
        );
        assert_eq!(held, ["m2"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- digest -----------------------------------------------------------

    #[test]
    fn digest_batches_messages_with_fyis_last() {
        let a = msg("m1", Kind::Fyi);
        let mut b = msg("m2", Kind::Urgent);
        b.from = "tonight/coordinator".into();
        let c = msg("m3", Kind::Normal);
        let text = digest(&[&a, &b, &c]);
        assert!(text.starts_with("[hiver · 3 messages]"), "{text}");
        let urgent = text.find("from tonight/coordinator (urgent)").unwrap();
        let normal = text.find("2. from scout [m3]").unwrap();
        let fyi = text
            .find("FYI (no reply needed):\n- from scout [m1]")
            .unwrap();
        assert!(urgent < normal && normal < fyi, "{text}");
        let single = digest(&[&c]);
        assert!(
            single.starts_with("[hiver] from scout [m3]: text m3"),
            "{single}"
        );
    }
}
