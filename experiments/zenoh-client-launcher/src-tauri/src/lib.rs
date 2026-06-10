use futures_util::{pin_mut, stream::StreamExt};
use mdns::RecordKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Child;
use std::sync::Mutex;
use std::time::Duration;
use tauri::State;

#[derive(Default)]
pub struct BotProcesses(Mutex<HashMap<String, Child>>);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BotConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub personality: String,
    pub system_prompt: String,
    pub llm_backend: String, // "ollama" | "claude"
    pub ollama_url: String,
    pub ollama_model: String,
    pub claude_api_key: String,
    pub zenoh_router: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BotStatus {
    pub id: String,
    pub running: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiscoveredRouter {
    pub name: String,       // instance name from SRV record
    pub host: String,       // resolved hostname or IP
    pub port: u16,
    pub zenoh_addr: String, // tcp/host:port ready to use
}

const DVERSE_SERVICE: &str = "_dverse._tcp.local";
const DISCOVERY_TIMEOUT_MS: u64 = 3000;

#[tauri::command]
async fn discover_routers() -> Result<Vec<DiscoveredRouter>, String> {
    let stream = mdns::discover::all(DVERSE_SERVICE, Duration::from_millis(DISCOVERY_TIMEOUT_MS))
        .map_err(|e| format!("mDNS discovery failed: {e}"))?
        .listen();
    pin_mut!(stream);

    let mut routers: HashMap<String, DiscoveredRouter> = HashMap::new();

    let deadline = tokio::time::sleep(Duration::from_millis(DISCOVERY_TIMEOUT_MS));
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => break,
            item = stream.next() => {
                let response = match item {
                    Some(Ok(r)) => r,
                    Some(Err(_)) => continue,
                    None => break,
                };

                let mut host = String::new();
                let mut port: u16 = 7447;
                let mut name = String::new();

                for record in response.records() {
                    match &record.kind {
                        RecordKind::A(addr) => {
                            if host.is_empty() { host = addr.to_string(); }
                        }
                        RecordKind::AAAA(addr) => {
                            if host.is_empty() { host = addr.to_string(); }
                        }
                        RecordKind::SRV { port: p, target, .. } => {
                            port = *p;
                            name = record.name.clone();
                            if host.is_empty() { host = target.clone(); }
                        }
                        RecordKind::PTR(n) => {
                            if name.is_empty() { name = n.clone(); }
                        }
                        _ => {}
                    }
                }

                if !host.is_empty() {
                    if name.is_empty() { name = host.clone(); }
                    let zenoh_addr = format!("tcp/{host}:{port}");
                    routers.insert(zenoh_addr.clone(), DiscoveredRouter { name, host, port, zenoh_addr });
                }
            }
        }
    }

    Ok(routers.into_values().collect())
}

#[tauri::command]
async fn start_bot(
    config: BotConfig,
    processes: State<'_, BotProcesses>,
) -> Result<BotStatus, String> {
    let mut map = processes.0.lock().map_err(|e| e.to_string())?;

    if map.contains_key(&config.id) {
        return Ok(BotStatus { id: config.id, running: true });
    }

    let (program, args) = resolve_bot_agent(&config)?;

    let mut cmd = std::process::Command::new(&program);
    cmd.args(&args)
        .arg("--name").arg(&config.name)
        .arg("--router").arg(&config.zenoh_router)
        .arg("--description").arg(&config.description);

    if config.llm_backend == "ollama" {
        cmd.arg("--ollama-url").arg(&config.ollama_url)
            .arg("--model").arg(&config.ollama_model);
    }

    if !config.claude_api_key.is_empty() {
        cmd.env("ANTHROPIC_API_KEY", &config.claude_api_key);
    }

    if !config.system_prompt.is_empty() {
        cmd.env("BOT_SYSTEM_PROMPT", &config.system_prompt);
    }

    let child = cmd.spawn().map_err(|e| format!("Failed to start bot: {e}"))?;

    let id = config.id.clone();
    map.insert(id.clone(), child);

    Ok(BotStatus { id, running: true })
}

#[tauri::command]
async fn stop_bot(
    id: String,
    processes: State<'_, BotProcesses>,
) -> Result<BotStatus, String> {
    let mut map = processes.0.lock().map_err(|e| e.to_string())?;

    if let Some(mut child) = map.remove(&id) {
        child.kill().map_err(|e| format!("Failed to kill bot: {e}"))?;
    }

    Ok(BotStatus { id, running: false })
}

#[tauri::command]
async fn get_bot_statuses(
    processes: State<'_, BotProcesses>,
) -> Result<Vec<BotStatus>, String> {
    let mut map = processes.0.lock().map_err(|e| e.to_string())?;

    let statuses: Vec<BotStatus> = map
        .iter_mut()
        .map(|(id, child)| {
            let running = child.try_wait().map(|s| s.is_none()).unwrap_or(false);
            BotStatus { id: id.clone(), running }
        })
        .collect();

    // Reap dead processes
    map.retain(|_, child| child.try_wait().map(|s| s.is_none()).unwrap_or(false));

    Ok(statuses)
}

/// Returns (program, extra_args) for running bot_agent.py.
/// Prefers `uv run` if available, falls back to `python3`.
fn resolve_bot_agent(config: &BotConfig) -> Result<(String, Vec<String>), String> {
    let candidates = [
        "./bot_agent.py",
        "../chat-app/bot_agent.py",
        "../../chat-app/bot_agent.py",
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            if which_on_path("uv") {
                return Ok(("uv".into(), vec!["run".into(), path.to_string()]));
            }
            return Ok(("python3".into(), vec![path.to_string()]));
        }
    }

    // Last resort: assume bot_agent is on PATH as a compiled binary
    if which_on_path("bot_agent") {
        return Ok(("bot_agent".into(), vec![]));
    }

    let _ = config; // suppress unused warning
    Err("bot_agent not found. Place bot_agent.py next to the launcher or install it on PATH.".into())
}

fn which_on_path(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(BotProcesses::default())
        .invoke_handler(tauri::generate_handler![
            start_bot,
            stop_bot,
            get_bot_statuses,
            discover_routers,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
