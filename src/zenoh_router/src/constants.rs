use std::time::Duration;

pub const KEYCLOAK_URL: &str = "https://auth.dverse.yordanmitev.me";
pub const KEYCLOAK_REALM: &str = "dverse";
pub const CLIENT_ID: &str = "step-ca";
pub const CLIENT_SECRET: &str = "oeWYn8BLhMsAt7j9qG7qEwIWATnBepAr";
pub const CA_URL: &str = "https://ca.dverse.yordanmitev.me:9000";
pub const ROUTER_LISTEN: &str = "tls/0.0.0.0:7447";
pub const ROUTER_PORT: u16 = 7447;
/// Service-account client for user self-registration (manage-users scope, dverse realm).
/// Secret is managed in sops and injected into Keycloak at deploy time.
pub const REGISTRATION_CLIENT_ID: &str = "dverse-registration";
pub const REGISTRATION_CLIENT_SECRET: &str = "Xv2kR8nQ5mW4jT7eBpL3hC9gF6dA0sYz";

// ── Agent inventory timing ──────────────────────────────────────────────────
//
// One source of truth for the heartbeat ladder.  bot_framework::announce
// re-exports the same interval so the agent side beats at the rate the
// router expects.

/// How often each agent re-publishes its announce while it's running.
pub const AGENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// `last_seen` past this and the agent is marked `Degraded`.  Two intervals
/// — one missed beat is plausibly a transient hiccup.
pub const AGENT_DEGRADED_AFTER: Duration = Duration::from_secs(10);

/// `last_seen` past this and the agent is marked `Offline`.  Six intervals
/// — fits within a typical TCP retransmit budget.
pub const AGENT_OFFLINE_AFTER: Duration = Duration::from_secs(30);

/// `last_seen` past this and the agent entry is removed entirely.
/// Eighteen intervals — definitively gone, free the memory.
pub const AGENT_EVICT_AFTER: Duration = Duration::from_secs(90);
