use a2a::{
    extract_think,
    llm::{ChatMessage, OllamaClient, OLLAMA_DEFAULT_MODEL, OLLAMA_DEFAULT_URL},
    message::{A2AMessage, AGENT_A_INBOX, AGENT_A_NAME, AGENT_B_INBOX, AGENT_B_NAME},
};
use anyhow::Result;
use bot_framework::node::NodeConfig;
use clap::Parser;
use tracing::{error, info};

const SYSTEM_PROMPT: &str = "You are Agent B, a pragmatic and direct AI in a \
    peer-to-peer dialogue over a distributed Zenoh network. \
    You are exchanging ideas with Agent A, another AI agent. \
    Keep each response to 2-3 sentences. Be concrete and insightful.";

fn print_thinking(thinking: &str) {
    println!("  ┌─ Agent B thinking ──────────────────────────");
    for line in thinking.lines() {
        println!("  │  {line}");
    }
    println!("  └────────────────────────────────────────────");
}

#[derive(Parser)]
#[command(name = "agent-b", about = "DVerse A2A responder — listens and replies to Agent A")]
struct Args {
    /// Zenoh router endpoint.
    #[arg(long, default_value = "tcp/localhost:7447")]
    router: String,

    /// Ollama base URL.
    #[arg(long, default_value = OLLAMA_DEFAULT_URL)]
    ollama_url: String,

    /// Ollama model name.
    #[arg(long, default_value = OLLAMA_DEFAULT_MODEL)]
    model: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("warn,dverse_agent_b=info")
        .init();
    let args = Args::parse();

    info!(endpoint = %args.router, "connecting to Zenoh router");
    let session = NodeConfig::plain(&args.router).connect().await?;

    let inbox = session
        .declare_subscriber(AGENT_B_INBOX)
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    println!("Agent B online  (model: {})", args.model);
    println!("Listening on {AGENT_B_INBOX}\n");

    let llm = OllamaClient::new(&args.ollama_url, &args.model);
    let mut history: Vec<ChatMessage> = Vec::new();

    loop {
        match inbox.recv_async().await {
            Ok(sample) => {
                let bytes = sample.payload().to_bytes();
                let msg: A2AMessage = match serde_json::from_slice(&bytes) {
                    Ok(m) => m,
                    Err(e) => {
                        error!(error = %e, "failed to deserialize A2AMessage — skipping");
                        continue;
                    }
                };

                println!("── Turn {} ─────────────────────────────────────", msg.turn);
                println!("Received from Agent A: {}\n", msg.content);

                history.push(ChatMessage {
                    role: "user".into(),
                    content: msg.content.clone(),
                });

                let raw = match llm.respond(SYSTEM_PROMPT, &history).await {
                    Ok(t) => t,
                    Err(e) => {
                        error!(error = %e, "LLM call failed — skipping reply");
                        continue;
                    }
                };

                let (thinking, b_text) = extract_think(&raw);
                if let Some(t) = thinking {
                    print_thinking(t);
                }
                println!("Agent B → Agent A: {b_text}\n");

                history.push(ChatMessage {
                    role: "assistant".into(),
                    content: b_text.to_string(),
                });

                let mut reply =
                    A2AMessage::new(AGENT_B_NAME, AGENT_A_NAME, b_text.to_string(), msg.turn);
                reply.thinking = thinking.map(str::to_string);

                session
                    .put(AGENT_A_INBOX, serde_json::to_vec(&reply)?)
                    .await
                    .map_err(|e| anyhow::anyhow!("put: {e}"))?;
            }
            Err(e) => {
                error!(error = %e, "inbox subscriber closed");
                break;
            }
        }
    }

    Ok(())
}
