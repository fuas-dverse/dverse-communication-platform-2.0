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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg() -> A2AMessage {
        A2AMessage::new("agent-a", "agent-b", "hello".to_string(), 1)
    }

    #[test]
    fn new_id_format() {
        let msg = make_msg();
        assert!(msg.id.starts_with("agent-a-turn1-"));
        assert_eq!(msg.turn, 1);
    }

    #[test]
    fn serde_roundtrip() {
        let mut msg = make_msg();
        msg.thinking = Some("some thought".to_string());
        let json = serde_json::to_string(&msg).unwrap();
        let decoded: A2AMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.from, msg.from);
        assert_eq!(decoded.to, msg.to);
        assert_eq!(decoded.content, msg.content);
        assert_eq!(decoded.thinking, msg.thinking);
        assert_eq!(decoded.turn, msg.turn);
    }

    #[test]
    fn thinking_omitted_when_none() {
        let msg = make_msg();
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("thinking").is_none());
    }

    #[test]
    fn thinking_present_when_some() {
        let mut msg = make_msg();
        msg.thinking = Some("deep thought".to_string());
        let json = serde_json::to_string(&msg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["thinking"], "deep thought");
    }

    #[test]
    fn deser_missing_required_field() {
        // "from" field is missing
        let json = r#"{"to":"agent-b","content":"hi","timestamp_secs":0,"turn":1,"id":"x"}"#;
        let result: Result<A2AMessage, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn deser_extra_unknown_fields() {
        let json = r#"{"id":"x","from":"a","to":"b","content":"hi","timestamp_secs":0,"turn":1,"unknown_key":"value"}"#;
        let result: Result<A2AMessage, _> = serde_json::from_str(json);
        assert!(result.is_ok());
    }

    #[test]
    fn deser_empty_bytes() {
        let result: Result<A2AMessage, _> = serde_json::from_slice(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn deser_invalid_utf8() {
        let result: Result<A2AMessage, _> = serde_json::from_slice(&[0xFF, 0xFE]);
        assert!(result.is_err());
    }
}
