use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Topic structure for A2A communication:
///   dverse/a2a/{recipient}/inbox
///
/// Agent A publishes to AGENT_B_INBOX; Agent B publishes to AGENT_A_INBOX.
/// Each agent subscribes only to its own inbox.
pub const AGENT_A_NAME: &str = "agent-a";
pub const AGENT_B_NAME: &str = "agent-b";
pub const AGENT_A_INBOX: &str = "dverse/a2a/agent-a/inbox";
pub const AGENT_B_INBOX: &str = "dverse/a2a/agent-b/inbox";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct A2AMessage {
    /// Unique ID: "{from}-turn{turn}-{unix_secs}"
    pub id: String,
    pub from: String,
    pub to: String,
    pub content: String,
    /// Raw <think>…</think> reasoning from the model, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// Unix epoch seconds (UTC).
    pub timestamp_secs: u64,
    /// Exchange turn number (1-based).
    pub turn: u32,
}

impl A2AMessage {
    pub fn new(from: &str, to: &str, content: String, turn: u32) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            id: format!("{from}-turn{turn}-{ts}"),
            from: from.to_string(),
            to: to.to_string(),
            content,
            thinking: None,
            timestamp_secs: ts,
            turn,
        }
    }
}
