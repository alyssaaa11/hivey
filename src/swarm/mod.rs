//! hiver swarms: roster (`model`), message bus (`bus`) and the server-side engine.
//!
//! Reached through one socket method, `swarm`, whose `op` selects the operation
//! (`import`, `forget`, `list`, `master`, `msg.send`, `msg.inbox`, `msg.log`).

pub(crate) mod adapter;
pub(crate) mod addons;
pub(crate) mod bus;
pub(crate) mod creators;
pub(crate) mod engine;
pub(crate) mod home;
pub(crate) mod model;
pub(crate) mod schedule;
pub(crate) mod skills_library;

pub(crate) use engine::{handle_request, start};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct SwarmParams {
    pub op: String,
    #[serde(default)]
    pub args: serde_json::Value,
}
