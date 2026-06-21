use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use bot_framework::admission::JoinRequest;
use bot_framework::announce::{AgentAnnounce, AgentStatusWire};
use bot_framework::config::{DverseConfig, SessionRole};
use bot_framework::session_crypto::{
    Curve25519PublicKey, GroupReceiver, GroupSender, OlmSession, SessionIdentity,
};
use tokio::sync::Notify;

use crate::constants::{AGENT_DEGRADED_AFTER, AGENT_EVICT_AFTER, AGENT_OFFLINE_AFTER};

pub struct AppState {
    pub router_status: RouterStatus,
    /// CNs that have been auto-admitted (all valid-cert nodes).
    pub admitted: Vec<String>,
    /// Log lines shown in the GUI.
    pub log: Vec<String>,
    /// Config written by the GUI; background thread consumes it to start/restart.
    pub staged_config: Option<DverseConfig>,
    /// Last accepted config — set by the router loop after it takes staged_config.
    /// Kept alive so bridge and other commands can read session params mid-run.
    pub active_config: Option<DverseConfig>,
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
    /// Admin CNs of sessions visible on the LAN, mirrored from DNS-SD browsing.
    /// Drives the session chooser; independent of which session we're in.
    pub visible_sessions: Vec<String>,
    /// Vodozemac state for this router: identity, group sender (admin) /
    /// receivers (clients), pending join requests, ban list. `None` until a
    /// session config has been accepted.
    pub crypto: Option<SessionCryptoState>,
    /// Admin side: join requests received from `dverse/session/requests/**`
    /// that have been crypto-verified (CN matches cert, binding signature
    /// valid) and are awaiting an Allow/Deny click. Bans are dropped before
    /// reaching this queue.
    pub pending_requests: VecDeque<PendingRequest>,
    /// Admin side, RAM-only (cleared on session end). Requesters in this set
    /// have their `JoinRequest` silently dropped.
    pub banned_cns: HashSet<String>,
    /// Requester side: state of the most recent join request we issued,
    /// `None` outside the join flow. The GUI bounces back to the chooser on
    /// terminal states (Denied / Banned / Timeout).
    pub join_flow: Option<JoinFlowStatus>,
    /// "Session must restart" doorbell. The admission handler rings it after
    /// pushing a new CN to `admitted`; `session_loop` `await`s `notified()`
    /// and returns on a ring, triggering an ACL reload.
    pub admitted_changed: Arc<Notify>,
    /// "A fresh config has been staged" doorbell. `pick_session` rings it
    /// after writing to `staged_config`, so the router can drop its
    /// currently-running session and pick up the new role mid-run
    /// (logout → pick a different session must actually swap, not stick).
    pub config_changed: Arc<Notify>,
    /// The live Zenoh session, set after `zenoh::open` succeeds and cleared
    /// before `session.close()`. Exposed so Tauri commands (admit/deny) can
    /// publish without each task holding its own session handle.
    pub zenoh_session: Option<zenoh::Session>,
    /// Bridge tokens issued by the admin. Each token allows a plain-TCP
    /// client to connect without mTLS, restricted to rooms + announce topics.
    pub bridge_tokens: Vec<String>,
}

/// Per-session crypto material. Lifetime = one Zenoh session as a member
/// (created on session-config-accept; dropped/reset on session change).
pub struct SessionCryptoState {
    /// This node's vodozemac identity (Curve25519/Ed25519) — used to receive
    /// pre-key Olm messages (admission grants) and to sign new ones.
    pub identity: SessionIdentity,
    /// The one-time key this node has published. Used by peers to seal an
    /// Olm session to us. Re-rolled on each join attempt.
    pub published_otk: Option<Curve25519PublicKey>,
    /// Admin only: the outbound Megolm sender for this session. Its
    /// `session_key` is what gets Olm-wrapped to each admitted member.
    pub group_sender: Option<GroupSender>,
    /// Inbound Megolm sessions keyed by `MegolmSession.session_id()` —
    /// admitted clients hold the admin's sender; the admin holds its own.
    pub group_receivers: HashMap<String, GroupReceiver>,
    /// Admin-side: 1:1 Olm sessions established as a side-effect of admitting
    /// a member. Keyed by the member's CN. Survives across kick/ban rotations
    /// so the admin can re-deliver a fresh Megolm `SessionKey` over a Normal
    /// (post-pre-key) Olm message without consuming a new one-time key. Dropped
    /// when the member is kicked (so a re-admission opens a fresh handshake).
    pub admin_olm_sessions: HashMap<String, OlmSession>,
    /// Admin-side: identity information remembered for each admitted member.
    /// Populated at admit time; needed to address rotation messages and to
    /// surface the admitted list to the GUI for the per-member Kick / Ban
    /// buttons.
    pub admitted_identities: HashMap<String, AdmittedIdentity>,
    /// Member-side: the established 1:1 Olm session to the admin. Held so the
    /// member can decrypt a rotation message that lands later in the session.
    /// `None` on the admin side and until the member's Olm-Allow lands.
    pub member_olm_session: Option<OlmSession>,
}

/// Per-admitted-CN identity record kept by the admin. Captured at admit time
/// from the verified `JoinRequest`; lets the kick handler address a rotation
/// message to each remaining member without reading the original (already
/// consumed) join queue.
#[derive(Debug, Clone)]
pub struct AdmittedIdentity {
    pub cn: String,
    /// Base64 Curve25519 identity key — informational, also used to log a
    /// fingerprint-style identifier in the GUI / tracing output.
    pub identity_key_b64: String,
}

impl SessionCryptoState {
    /// Mint a fresh identity. Admin role starts a group sender; clients
    /// leave that `None` until they receive an admission Allow.
    pub fn new(is_admin: bool) -> Self {
        let mut identity = SessionIdentity::new();
        let otks = identity.generate_one_time_keys(1);
        let published_otk = otks.into_iter().next();
        identity.mark_keys_as_published();
        let group_sender = if is_admin { Some(GroupSender::new()) } else { None };
        Self {
            identity,
            published_otk,
            group_sender,
            group_receivers: HashMap::new(),
            admin_olm_sessions: HashMap::new(),
            admitted_identities: HashMap::new(),
            member_olm_session: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub request: JoinRequest,
    pub received_at: Instant,
}

/// Requester-side state of the active join flow.
#[derive(Debug, Clone)]
pub enum JoinFlowStatus {
    /// Request sent, awaiting an `AdmissionDecision`.
    Pending { admin_cn: String, sent_at: Instant },
    /// `Allow` received and Megolm key successfully unwrapped.
    Allowed,
    /// `Deny` received.
    Denied { reason: Option<String> },
    /// Local-only state when the request times out without an answer.
    TimedOut,
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
    /// Zenoh key expressions this agent publishes on (`session.put`).
    /// Rendered with a `→` arrow in the GUI tree.
    pub publishes: Vec<String>,
    /// Zenoh key expressions this agent subscribes from
    /// (`session.declare_subscriber`).  Rendered with a `←` arrow in the GUI.
    pub subscribes: Vec<String>,
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
            active_config: None,
            session_role,
            session_id,
            connected_nodes: HashMap::new(),
            visible_sessions: Vec::new(),
            crypto: None,
            pending_requests: VecDeque::new(),
            banned_cns: HashSet::new(),
            join_flow: None,
            admitted_changed: Arc::new(Notify::new()),
            config_changed: Arc::new(Notify::new()),
            zenoh_session: None,
            bridge_tokens: Vec::new(),
        }
    }

    /// Append a single line to the in-memory log ring that backs the GUI's
    /// bottom panel.  Now called only by `logging::GuiLogLayer` — business
    /// code uses `tracing::info!` / `warn!` / `error!` and the layer routes
    /// each event here.  Capped at 200 lines.
    pub fn push_log(&mut self, msg: impl Into<String>) {
        let entry = msg.into();
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
                publishes: ann.publishes.clone(),
                subscribes: ann.subscribes.clone(),
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
