use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

// ── Shared message type ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

// ── Ollama client ─────────────────────────────────────────────────────────────

pub const OLLAMA_DEFAULT_URL: &str = "http://localhost:11434";
pub const OLLAMA_DEFAULT_MODEL: &str = "deepseek-r1:1.5b";

pub struct OllamaClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

#[derive(Deserialize)]
struct OllamaMessage {
    content: String,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            model: model.into(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn respond(&self, system: &str, messages: &[ChatMessage]) -> Result<String> {
        let mut all = vec![ChatMessage {
            role: "system".into(),
            content: system.to_string(),
        }];
        all.extend_from_slice(messages);

        let body = OllamaRequest {
            model: self.model.clone(),
            messages: all,
            stream: false,
        };
        let resp = self
            .http
            .post(format!("{}/api/chat", self.base_url))
            .json(&body)
            .send()
            .await
            .context("Ollama API request")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API {status}: {text}");
        }

        let parsed: OllamaResponse = resp.json().await.context("parse Ollama response")?;
        Ok(parsed.message.content)
    }
}

// ── Anthropic Claude client ───────────────────────────────────────────────────

const CLAUDE_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const CLAUDE_DEFAULT_MODEL: &str = "claude-haiku-4-5-20251001";
const CLAUDE_MAX_TOKENS: u32 = 512;

pub struct ClaudeClient {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct ClaudeRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: &'a [ChatMessage],
}

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeBlock>,
}

#[derive(Deserialize)]
struct ClaudeBlock {
    #[serde(rename = "type")]
    kind: String,
    text: Option<String>,
}

impl ClaudeClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: CLAUDE_DEFAULT_MODEL.to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn respond(&self, system: &str, messages: &[ChatMessage]) -> Result<String> {
        let body = ClaudeRequest {
            model: &self.model,
            max_tokens: CLAUDE_MAX_TOKENS,
            system,
            messages,
        };
        let resp = self
            .http
            .post(CLAUDE_MESSAGES_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await
            .context("Anthropic API request")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API {status}: {text}");
        }

        let parsed: ClaudeResponse = resp.json().await.context("parse Anthropic response")?;
        parsed
            .content
            .into_iter()
            .find(|b| b.kind == "text")
            .and_then(|b| b.text)
            .context("no text block in Anthropic response")
    }
}
