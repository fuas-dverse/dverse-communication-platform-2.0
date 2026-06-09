pub mod llm;
pub mod message;

pub use llm::{ChatMessage, ClaudeClient, OllamaClient, OLLAMA_DEFAULT_MODEL, OLLAMA_DEFAULT_URL};
pub use message::{A2AMessage, AGENT_A_INBOX, AGENT_A_NAME, AGENT_B_INBOX, AGENT_B_NAME};
