pub mod llm;
pub mod message;

pub use llm::{ChatMessage, ClaudeClient, OllamaClient, OLLAMA_DEFAULT_MODEL, OLLAMA_DEFAULT_URL};
pub use message::{A2AMessage, AGENT_A_INBOX, AGENT_A_NAME, AGENT_B_INBOX, AGENT_B_NAME};

/// Split raw model output into (thinking, response).
/// Returns (Some(think_text), response) when a `<think>…</think>` block is present,
/// (None, full_text) otherwise.
pub fn extract_think(raw: &str) -> (Option<&str>, &str) {
    if let (Some(open), Some(close)) = (raw.find("<think>"), raw.find("</think>")) {
        let thinking = raw[open + 7..close].trim();
        let response = raw[close + 8..].trim();
        return (Some(thinking), response);
    }
    (None, raw.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_think_no_tag() {
        let (thinking, response) = extract_think("  hello world  ");
        assert!(thinking.is_none());
        assert_eq!(response, "hello world");
    }

    #[test]
    fn extract_think_with_tag() {
        let (thinking, response) = extract_think("<think>reasoning</think> answer");
        assert_eq!(thinking, Some("reasoning"));
        assert_eq!(response, "answer");
    }

    #[test]
    fn extract_think_empty_after_tag() {
        let (thinking, response) = extract_think("<think>thought</think>");
        assert_eq!(thinking, Some("thought"));
        assert_eq!(response, "");
    }

    #[test]
    fn extract_think_no_close_tag() {
        let raw = "<think>unfinished reasoning";
        let (thinking, response) = extract_think(raw);
        assert!(thinking.is_none());
        assert_eq!(response, raw.trim());
    }
}
