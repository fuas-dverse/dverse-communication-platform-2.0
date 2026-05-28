//! Agent self-announcement: per-agent heartbeat on
//! `dverse/nodes/announce/<cn>/agents/<agent_name>` so the router can keep
//! a live inventory of who's running where.
//!
//! Each running agent should call [`AgentAnnouncer::start`] once and hold
//! onto the returned handle for the lifetime of its session.  The handle's
//! tokio task publishes one immediate announce then re-publishes every
//! [`AGENT_HEARTBEAT_INTERVAL`].  Dropping the handle aborts the task.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use zenoh::Session;

/// How often each agent re-publishes its announce.  Must equal the router's
/// `crate::constants::AGENT_HEARTBEAT_INTERVAL` — keep these two literally
/// equal; both are tracked in the heartbeat-ladder section of the plan.
pub const AGENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Wire payload — JSON, published on the announce key.  The router decodes
/// this with `serde_json::from_slice::<AgentAnnounce>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnnounce {
    /// Publisher's CN (cert common name).  Cross-checked against the
    /// `<cn>` segment of the key path but only the JSON value is load-bearing.
    pub cn: String,
    /// Short, stable identifier for this agent (e.g. `"ping"`).  Used as the
    /// agent-name key in the router's `connected_nodes` map and as a segment
    /// of the announce key path.
    pub agent_name: String,
    /// Agent's `CARGO_PKG_VERSION`.
    pub version: String,
    /// Zenoh key expressions the agent uses — typically the topics it
    /// publishes to plus those it subscribes from.  Surfaced in the router
    /// GUI so a user can see "who provides what".
    pub key_exprs: Vec<String>,
    /// Self-reported status.  Authoritative status is computed router-side
    /// from `last_seen`; agents only ever report `Online`.
    pub status: AgentStatusWire,
    /// Optional RFC 3339 timestamp.  Informational only — the router stamps
    /// `Instant::now()` server-side.  Optional so we don't pull `chrono`.
    #[serde(default)]
    pub announced_at: Option<String>,
}

/// Status as serialized on the wire.  Kept separate from the router's
/// internal `AgentStatus` so the wire schema is owned by `bot_framework`
/// (the library every agent depends on) and the lifecycle classification
/// (Online/Degraded/Offline) is owned by the router.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatusWire {
    Online,
    Degraded,
    Offline,
}

/// Caller-supplied agent description.  Borrowed so callers can keep their
/// own `String` storage; we clone into the spawned task.
pub struct AgentInfo<'a> {
    pub cn: &'a str,
    pub agent_name: &'a str,
    pub version: &'a str,
    pub key_exprs: Vec<String>,
}

/// Handle for an in-progress announcer.  Drop to stop the heartbeat —
/// the spawned task is aborted via `JoinHandle::abort` in [`Drop`].
pub struct AgentAnnouncer {
    handle: JoinHandle<()>,
}

impl AgentAnnouncer {
    /// Publishes one immediate announce on `dverse/nodes/announce/<cn>/agents/<agent_name>`,
    /// then re-publishes every [`AGENT_HEARTBEAT_INTERVAL`] until the
    /// returned handle drops or `session` closes.
    pub fn start(session: Session, info: AgentInfo<'_>) -> Self {
        let key_expr = format!(
            "dverse/nodes/announce/{}/agents/{}",
            info.cn, info.agent_name
        );
        let payload_template = AgentAnnounce {
            cn: info.cn.to_string(),
            agent_name: info.agent_name.to_string(),
            version: info.version.to_string(),
            key_exprs: info.key_exprs.clone(),
            status: AgentStatusWire::Online,
            announced_at: None,
        };

        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(AGENT_HEARTBEAT_INTERVAL);
            // Default `MissedTickBehavior::Burst` would fire back-to-back if
            // we got descheduled; we'd rather skip and stay aligned.
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tick.tick().await;
                let body = match serde_json::to_vec(&payload_template) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(error = %e, "AgentAnnouncer: failed to serialize announce");
                        continue;
                    }
                };
                if let Err(e) = session.put(&key_expr, body).await {
                    tracing::warn!(error = %e, key_expr = %key_expr, "AgentAnnouncer: put failed");
                }
            }
        });

        Self { handle }
    }
}

impl Drop for AgentAnnouncer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
