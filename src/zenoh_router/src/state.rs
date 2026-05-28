use std::collections::HashMap;
use std::time::Instant;

use bot_framework::announce::{AgentAnnounce, AgentStatusWire};
use bot_framework::config::{DverseConfig, SessionRole};

use crate::constants::{AGENT_DEGRADED_AFTER, AGENT_EVICT_AFTER, AGENT_OFFLINE_AFTER};

pub struct AppState {
    pub router_status: RouterStatus,
    /// CNs that have been auto-admitted (all valid-cert nodes).
    pub admitted: Vec<String>,
    /// Log lines shown in the GUI.
    pub log: Vec<String>,
    /// Config written by the GUI; background thread consumes it to start/restart.
    pub staged_config: Option<DverseConfig>,
    /// Whether this router hosts the session (Admin) or joined one (Client).
    /// Copied from the accepted config; the GUI reads it for the session badge.
    pub session_role: SessionRole,
    /// Cached `cfg.session_id()` — copied once at config-accept time so the
    /// GUI and DNS-SD threads don't re-derive it.  Empty string until a config
    /// has been accepted.
    pub session_id: String,
    /// Per-CN inventory of agents running on each admitted node.  Populated
    /// from `dverse/nodes/announce/<cn>/agents/<name>` heartbeats and pruned
    /// by the stale-eviction task.
    pub connected_nodes: HashMap<String, NodeInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RouterStatus {
    Idle,
    Acquiring,
    Starting,
    Running,
    Reloading,
    Error(String),
}

/// One known node (one CN).  Agents are keyed by name so re-announcements
/// from the same agent update its row in place instead of forking.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    pub cn: String,
    /// Latest heartbeat across any of this node's agents — refreshed every
    /// time an `upsert_agent` lands.
    pub last_seen: Instant,
    pub agents: HashMap<String, AgentInfo>,
}

/// One agent (one (cn, agent_name) pair) and what the router knows about
/// it.  The agent's name lives in the surrounding `NodeInfo::agents`
/// HashMap key rather than being duplicated here.
#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub version: String,
    pub key_expressions: Vec<String>,
    pub status: AgentStatus,
    pub last_seen: Instant,
}

/// Router-side status, derived from `last_seen`.  Distinct from the wire
/// `AgentStatusWire` so the router fully owns lifecycle classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Heartbeat within `AGENT_DEGRADED_AFTER`.
    Online,
    /// Last heartbeat between `AGENT_DEGRADED_AFTER` and `AGENT_OFFLINE_AFTER` ago.
    Degraded,
    /// Last heartbeat older than `AGENT_OFFLINE_AFTER` but still within
    /// `AGENT_EVICT_AFTER` — kept visible in the GUI as a gray row so the
    /// user sees what just left, before the reaper drops it.
    Offline,
}

impl From<AgentStatusWire> for AgentStatus {
    fn from(w: AgentStatusWire) -> Self {
        match w {
            AgentStatusWire::Online => AgentStatus::Online,
            AgentStatusWire::Degraded => AgentStatus::Degraded,
            AgentStatusWire::Offline => AgentStatus::Offline,
        }
    }
}

impl AppState {
    pub fn new(initial_config: Option<DverseConfig>) -> Self {
        let (session_role, session_id) = match &initial_config {
            Some(cfg) => (cfg.session_role.clone(), cfg.session_id()),
            None => (SessionRole::Admin, String::new()),
        };
        Self {
            router_status: RouterStatus::Idle,
            admitted: Vec::new(),
            log: Vec::new(),
            staged_config: initial_config,
            session_role,
            session_id,
            connected_nodes: HashMap::new(),
        }
    }

    pub fn push_log(&mut self, msg: impl Into<String>) {
        let entry = msg.into();
        eprintln!("{entry}");
        if self.log.len() >= 200 {
            self.log.remove(0);
        }
        self.log.push(entry);
    }

    /// Insert or refresh an agent's row from an incoming announce, stamping
    /// `last_seen = now` and marking the agent `Online`.  Creates the
    /// surrounding `NodeInfo` if this is the first time we've seen the CN.
    pub fn upsert_agent(&mut self, ann: &AgentAnnounce, now: Instant) {
        let node = self
            .connected_nodes
            .entry(ann.cn.clone())
            .or_insert_with(|| NodeInfo {
                cn: ann.cn.clone(),
                last_seen: now,
                agents: HashMap::new(),
            });
        node.last_seen = now;
        node.agents.insert(
            ann.agent_name.clone(),
            AgentInfo {
                version: ann.version.clone(),
                key_expressions: ann.key_exprs.clone(),
                status: AgentStatus::Online,
                last_seen: now,
            },
        );
    }

    /// Walk every known agent, recompute status from `now - last_seen`, and
    /// drop entries past `AGENT_EVICT_AFTER`.  Returns the dropped
    /// `(cn, agent_name)` pairs so the caller can log them.  Empty nodes
    /// (no remaining agents) are removed too.
    pub fn reap_stale(&mut self, now: Instant) -> Vec<(String, String)> {
        let mut evicted: Vec<(String, String)> = Vec::new();

        for node in self.connected_nodes.values_mut() {
            node.agents.retain(|name, ag| {
                let age = now.saturating_duration_since(ag.last_seen);
                if age >= AGENT_EVICT_AFTER {
                    evicted.push((node.cn.clone(), name.clone()));
                    false
                } else {
                    ag.status = if age >= AGENT_OFFLINE_AFTER {
                        AgentStatus::Offline
                    } else if age >= AGENT_DEGRADED_AFTER {
                        AgentStatus::Degraded
                    } else {
                        AgentStatus::Online
                    };
                    true
                }
            });
        }
        // Drop nodes that lost their last agent.
        self.connected_nodes.retain(|_, node| !node.agents.is_empty());

        evicted
    }
}
