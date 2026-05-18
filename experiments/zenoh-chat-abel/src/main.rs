use serde::{Serialize, Deserialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{self, AsyncBufReadExt};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

const MAX_MSG_SIZE: usize = 256;
const RATE_LIMIT_WINDOW_SECS: u64 = 5;
const RATE_LIMIT_MAX_MSGS: u32 = 3;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ChatMessage {
    username: String,
    timestamp: u64,
    message: String,
    auth_token: Option<String>,
}

#[derive(Debug)]
struct RateLimiter {
    state: HashMap<String, (u64, u32)>,
}

impl RateLimiter {
    fn new() -> Self {
        Self { state: HashMap::new() }
    }

    fn allow(&mut self, username: &str, now: u64) -> bool {
        let entry = self.state.entry(username.to_string()).or_insert((now, 0));
        if now - entry.0 > RATE_LIMIT_WINDOW_SECS {
            *entry = (now, 1);
            true
        } else if entry.1 < RATE_LIMIT_MAX_MSGS {
            entry.1 += 1;
            true
        } else {
            false
        }
    }
}

fn current_timestamp() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn validate_message(raw: &[u8]) -> Option<ChatMessage> {
    let text = std::str::from_utf8(raw).ok()?;
    let msg: ChatMessage = serde_json::from_str(text).ok()?;

    if msg.username.trim().is_empty() || msg.message.trim().is_empty() || msg.timestamp == 0 {
        return None;
    }

    Some(msg)
}

fn format_ts(ts: u64) -> String {
    let secs_in_day = ts % 86400;
    let h = secs_in_day / 3600;
    let m = (secs_in_day % 3600) / 60;
    let s = secs_in_day % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[tokio::main]
async fn main() {
    println!("Enter username:");
    let mut username = String::new();
    std::io::stdin().read_line(&mut username).unwrap();
    let username = username.trim().to_string();

    if username.is_empty() {
        println!("Invalid username");
        return;
    }

    let mut config = zenoh::Config::default();
    config.insert_json5("mode", "\"peer\"").unwrap();

    let session = zenoh::open(config).await.unwrap();

    println!("[✓] Connected as {}", username);

    let counter = Arc::new(Mutex::new(0));
    let rate_limiter = Arc::new(Mutex::new(RateLimiter::new()));

    {
        let session = session.clone();
        let counter = counter.clone();
        let rl = rate_limiter.clone();

        tokio::spawn(async move {
            let mut sub = session.declare_subscriber("chat/global").await.unwrap();

            while let Ok(sample) = sub.recv_async().await {
                let raw = sample.payload().to_bytes();

                let msg = match validate_message(&raw) {
                    Some(m) => m,
                    None => continue,
                };

                let now = current_timestamp();
                if !rl.lock().unwrap().allow(&msg.username, now) {
                    continue;
                }

                let mut c = counter.lock().unwrap();
                *c += 1;

                println!(
                    "\n#{:>4} [{}] {}: {}",
                    *c,
                    format_ts(msg.timestamp),
                    msg.username,
                    msg.message
                );
            }
        });
    }

    let mut reader = io::BufReader::new(io::stdin()).lines();

    loop {
        let line = reader.next_line().await.unwrap();

        let text = match line {
            Some(t) => t.trim().to_string(),
            None => break,
        };

        if text == "/quit" {
            break;
        }

        if text.len() > MAX_MSG_SIZE {
            println!("Too long");
            continue;
        }

        let msg = ChatMessage {
            username: username.clone(),
            timestamp: current_timestamp(),
            message: text,
            auth_token: None,
        };

        let payload = serde_json::to_string(&msg).unwrap();

        session
            .put("chat/global", payload)
            .await
            .unwrap();
    }
}