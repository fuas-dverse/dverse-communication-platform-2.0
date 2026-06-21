use std::io::Write as _;
use std::time::Duration;

use a2a::{
    extract_think,
    llm::{ChatMessage, OllamaClient, OLLAMA_DEFAULT_MODEL, OLLAMA_DEFAULT_URL},
    message::{A2AMessage, AGENT_A_INBOX, AGENT_A_NAME, AGENT_B_INBOX, AGENT_B_NAME},
};
use anyhow::Result;
use bot_framework::node::NodeConfig;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing::{info, warn};

const DIRECT_SYSTEM: &str = "You are Agent A, an AI assistant in the DVerse network. \
    Answer the user directly and concisely. \
    Tip: prefix any question with /zenoh to convene the AI council.";

const COUNCIL_SYSTEM: &str = "You are Agent A in a peer-to-peer AI council deliberating over Zenoh. \
    You are exchanging perspectives with Agent B, another AI agent. \
    Keep each contribution to 2-3 sentences. Be intellectually rigorous.";

const SYNTHESIS_SYSTEM: &str = "You are Agent A synthesizing the outcome of an AI council. \
    Produce a clear, unified response that captures the consensus and key insights \
    from the discussion. Address the user's original query directly.";

fn print_thinking(agent: &str, thinking: &str) {
    println!("  ┌─ {agent} thinking ──────────────────────────");
    for line in thinking.lines() {
        println!("  │  {line}");
    }
    println!("  └────────────────────────────────────────────");
}

async fn run_council(
    session: &zenoh::Session,
    inbox_rx: &mut mpsc::Receiver<A2AMessage>,
    llm: &OllamaClient,
    query: &str,
    turns: u32,
) -> Result<String> {
    info!(turns, query, "convening AI council over Zenoh");

    let mut council_history: Vec<ChatMessage> = vec![ChatMessage {
        role: "user".into(),
        content: query.to_string(),
    }];
    let mut transcript: Vec<(String, String)> = Vec::new();

    for turn in 1..=turns {
        println!("\n  ── Turn {turn} ──────────────────────────────────");

        let a_raw = llm.respond(COUNCIL_SYSTEM, &council_history).await?;
        let (a_think, a_text) = extract_think(&a_raw);

        if let Some(t) = a_think {
            print_thinking("Agent A", t);
        }
        println!("  Agent A → Agent B: {a_text}\n");

        council_history.push(ChatMessage {
            role: "assistant".into(),
            content: a_text.to_string(),
        });
        transcript.push((AGENT_A_NAME.to_string(), a_text.to_string()));

        let mut msg = A2AMessage::new(AGENT_A_NAME, AGENT_B_NAME, a_text.to_string(), turn);
        msg.thinking = a_think.map(str::to_string);
        session
            .put(AGENT_B_INBOX, serde_json::to_vec(&msg)?)
            .await
            .map_err(|e| anyhow::anyhow!("put: {e}"))?;

        tokio::select! {
            reply = inbox_rx.recv() => {
                match reply {
                    Some(r) => {
                        if let Some(t) = &r.thinking {
                            print_thinking("Agent B", t);
                        }
                        println!("  Agent B → Agent A: {}\n", r.content);
                        council_history.push(ChatMessage {
                            role: "user".into(),
                            content: r.content.clone(),
                        });
                        transcript.push((AGENT_B_NAME.to_string(), r.content));
                    }
                    None => anyhow::bail!("inbox channel closed"),
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                warn!(turn, "timeout waiting for Agent B — synthesizing with partial transcript");
                break;
            }
        }
    }

    println!("\n  ── Synthesizing council response ────────────────");

    let transcript_text = transcript
        .iter()
        .map(|(agent, msg)| format!("{agent}: {msg}"))
        .collect::<Vec<_>>()
        .join("\n\n");

    let synthesis_msgs = vec![ChatMessage {
        role: "user".into(),
        content: format!(
            "User query: {query}\n\nCouncil discussion:\n{transcript_text}\n\n\
            Synthesize a unified council response addressing the user's query."
        ),
    }];

    let raw = llm.respond(SYNTHESIS_SYSTEM, &synthesis_msgs).await?;
    let (synth_think, consensus) = extract_think(&raw);
    if let Some(t) = synth_think {
        print_thinking("Agent A (synthesis)", t);
    }
    Ok(consensus.to_string())
}

#[derive(Parser)]
#[command(
    name = "agent-a",
    about = "DVerse Agent A — interactive REPL, type /zenoh <query> to convene the AI council"
)]
struct Args {
    /// Number of council exchange turns when /zenoh is used.
    #[arg(long, default_value_t = 2)]
    turns: u32,

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
        .with_env_filter("warn,dverse_agent_a=info")
        .init();
    let args = Args::parse();

    info!(endpoint = %args.router, "connecting to Zenoh router");
    let session = NodeConfig::plain(&args.router).connect().await?;

    let raw_inbox = session
        .declare_subscriber(AGENT_A_INBOX)
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    let (inbox_tx, mut inbox_rx) = mpsc::channel::<A2AMessage>(16);
    tokio::spawn(async move {
        while let Ok(sample) = raw_inbox.recv_async().await {
            let bytes = sample.payload().to_bytes();
            if let Ok(msg) = serde_json::from_slice::<A2AMessage>(&bytes) {
                let _ = inbox_tx.send(msg).await;
            }
        }
    });

    let llm = OllamaClient::new(&args.ollama_url, &args.model);
    let mut chat_history: Vec<ChatMessage> = Vec::new();

    println!("Agent A online  (model: {})", args.model);
    println!("  Regular text   → direct LLM response");
    println!("  /zenoh <query> → AI council over Zenoh");
    println!("  exit / Ctrl-D  → quit\n");

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        print!("You: ");
        std::io::stdout().flush()?;

        let line = match lines.next_line().await? {
            None => break,
            Some(l) => l,
        };
        let line = line.trim().to_string();

        if line.is_empty() {
            continue;
        }
        if line == "exit" || line == "quit" {
            break;
        }

        if let Some(query) = line.strip_prefix("/zenoh ") {
            let query = query.trim();
            println!("\n[Convening AI council over Zenoh — topic: \"{query}\"]\n");
            match run_council(&session, &mut inbox_rx, &llm, query, args.turns).await {
                Ok(consensus) => println!("Council: {consensus}\n"),
                Err(e) => println!("[Council error: {e}]\n"),
            }
        } else {
            chat_history.push(ChatMessage {
                role: "user".into(),
                content: line.clone(),
            });
            match llm.respond(DIRECT_SYSTEM, &chat_history).await {
                Ok(raw) => {
                    let (thinking, reply) = extract_think(&raw);
                    if let Some(t) = thinking {
                        print_thinking("Agent A", t);
                    }
                    chat_history.push(ChatMessage {
                        role: "assistant".into(),
                        content: reply.to_string(),
                    });
                    println!("Agent A: {reply}\n");
                }
                Err(e) => println!("[Error: {e}]\n"),
            }
        }
    }

    println!("Agent A disconnected.");
    Ok(())
}
