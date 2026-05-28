use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Child;
use std::sync::Mutex;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
