use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bot_framework::config::{DverseConfig, SessionRole};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::State;
use tracing::info;
use zenoh_router::state as zr;

// ── Constants ─────────────────────────────────────────────────────────────────

const KEYCLOAK_URL: &str = "https://auth.dverse.yordanmitev.me";
const KEYCLOAK_REALM: &str = "dverse";
const CLIENT_ID: &str = "step-ca";
const CLIENT_SECRET: &str = "oeWYn8BLhMsAt7j9qG7qEwIWATnBepAr";
const CA_URL: &str = "https://ca.dverse.yordanmitev.me:9000";
const ROUTER_LISTEN: &str = "tls/0.0.0.0:7447";
const ROUTER_PORT: u16 = 7447;
const REGISTRATION_CLIENT_ID: &str = "dverse-registration";
const REGISTRATION_CLIENT_SECRET: &str = "Xv2kR8nQ5mW4jT7eBpL3hC9gF6dA0sYz";
const DVERSE_SERVICE: &str = "_dverse._tcp.local.";
const DISCOVERY_TIMEOUT_MS: u64 = 3000;

// ── GUI screen (Tauri-side only) ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppScreen {
    Login,
    Register,
    Loading,
    Main,
}

// ── Snapshot DTOs sent to frontend ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RouterStatus {
    Idle,
    Acquiring,
    Starting,
    Running,
    Reloading,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Online,
    Degraded,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub version: String,
    pub publishes: Vec<String>,
    pub subscribes: Vec<String>,
    pub status: AgentStatus,
    pub last_seen_secs_ago: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub cn: String,
    pub agents: HashMap<String, AgentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionRoleDto {
    Admin,
    Client { admin_cn: String },
}

impl From<&SessionRole> for SessionRoleDto {
    fn from(r: &SessionRole) -> Self {
        match r {
            SessionRole::Admin => SessionRoleDto::Admin,
            SessionRole::Client { admin_cn } => SessionRoleDto::Client {
                admin_cn: admin_cn.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSnapshot {
    pub screen: AppScreen,
    pub router_status: RouterStatus,
    pub session_id: String,
    pub session_role: SessionRoleDto,
    pub connected_nodes: Vec<NodeInfo>,
    pub log: Vec<String>,
    pub error: Option<String>,
}

fn map_router_status(s: &zr::RouterStatus) -> RouterStatus {
    match s {
        zr::RouterStatus::Idle => RouterStatus::Idle,
        zr::RouterStatus::Acquiring => RouterStatus::Acquiring,
        zr::RouterStatus::Starting => RouterStatus::Starting,
        zr::RouterStatus::Running => RouterStatus::Running,
        zr::RouterStatus::Reloading => RouterStatus::Reloading,
        zr::RouterStatus::Error(e) => RouterStatus::Error(e.clone()),
    }
}

fn map_agent_status(s: zr::AgentStatus) -> AgentStatus {
    match s {
        zr::AgentStatus::Online => AgentStatus::Online,
        zr::AgentStatus::Degraded => AgentStatus::Degraded,
        zr::AgentStatus::Offline => AgentStatus::Offline,
    }
}

// ── Inner state (GUI screen + pointer to router AppState) ─────────────────────

pub struct InnerState {
    /// Current GUI screen. Starts as Login (no config) or Loading (config exists).
    /// Set to Loading by `login` command; transitions to Main/Login are derived
    /// from router status in `snapshot()`.
    pub screen: AppScreen,
    pub router: Arc<Mutex<zr::AppState>>,
}

impl InnerState {
    fn new(router: Arc<Mutex<zr::AppState>>) -> Self {
        let screen = if DverseConfig::exists() {
            AppScreen::Loading
        } else {
            AppScreen::Login
        };
        Self { screen, router }
    }

    fn snapshot(&self) -> AppSnapshot {
        let now = Instant::now();
        let rs = self.router.lock().unwrap();

        let router_status = map_router_status(&rs.router_status);
        let error = match &rs.router_status {
            zr::RouterStatus::Error(e) => Some(e.clone()),
            _ => None,
        };

        // Screen derivation:
        //   Login/Register → stay (user hasn't submitted credentials yet)
        //   Loading → follow router lifecycle (Running→Main, Error→Login, else stay Loading)
        //   Main → stay (router is up; if it errors the next poll flips back)
        let screen = match &self.screen {
            AppScreen::Login => AppScreen::Login,
            AppScreen::Register => AppScreen::Register,
            AppScreen::Loading => match rs.router_status {
                zr::RouterStatus::Running => AppScreen::Main,
                zr::RouterStatus::Error(_) => AppScreen::Login,
                _ => AppScreen::Loading,
            },
            AppScreen::Main => match rs.router_status {
                zr::RouterStatus::Error(_) => AppScreen::Login,
                _ => AppScreen::Main,
            },
        };

        let mut nodes: Vec<NodeInfo> = rs
            .connected_nodes
            .values()
            .map(|n| {
                let mut agents: HashMap<String, AgentInfo> = n
                    .agents
                    .iter()
                    .map(|(name, ag)| {
                        let info = AgentInfo {
                            version: ag.version.clone(),
                            publishes: ag.publishes.clone(),
                            subscribes: ag.subscribes.clone(),
                            status: map_agent_status(ag.status),
                            last_seen_secs_ago: now
                                .saturating_duration_since(ag.last_seen)
                                .as_secs(),
                        };
                        (name.clone(), info)
                    })
                    .collect();
                // Sort agents for stable display order.
                let sorted: HashMap<String, AgentInfo> = {
                    let mut keys: Vec<String> = agents.keys().cloned().collect();
                    keys.sort();
                    keys.into_iter().map(|k| { let v = agents.remove(&k).unwrap(); (k, v) }).collect()
                };
                NodeInfo { cn: n.cn.clone(), agents: sorted }
            })
            .collect();
        nodes.sort_by(|a, b| a.cn.cmp(&b.cn));

        AppSnapshot {
            screen,
            router_status,
            session_id: rs.session_id.clone(),
            session_role: SessionRoleDto::from(&rs.session_role),
            connected_nodes: nodes,
            log: rs.log.clone(),
            error,
        }
    }
}

pub struct AppStateWrapper(pub Arc<Mutex<InnerState>>);

// ── Bot processes ─────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct BotProcesses(Mutex<HashMap<String, Child>>);

// ── Wire types ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BotConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub personality: String,
    pub system_prompt: String,
    pub llm_backend: String,
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
    pub name: String,
    pub host: String,
    pub port: u16,
    pub zenoh_addr: String,
    pub cn: String,
    pub session: String,
}

// ── Commands: snapshot ────────────────────────────────────────────────────────

#[tauri::command]
fn get_state(state: State<'_, AppStateWrapper>) -> AppSnapshot {
    state.0.lock().unwrap().snapshot()
}

// ── Commands: auth ────────────────────────────────────────────────────────────

#[tauri::command]
async fn login(
    username: String,
    password: String,
    create_session: bool,
    join_admin_cn: String,
    state: State<'_, AppStateWrapper>,
) -> Result<(), String> {
    if username.is_empty() {
        return Err("Username is required.".into());
    }
    if password.is_empty() {
        return Err("Password is required.".into());
    }

    let session_role = if create_session {
        SessionRole::Admin
    } else {
        let admin_cn = join_admin_cn.trim().to_string();
        if admin_cn.is_empty() {
            return Err("Admin username is required to join a session.".into());
        }
        SessionRole::Client { admin_cn }
    };

    let cert_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("dverse")
        .join("certs");

    let cn = username.split('@').next().unwrap_or(&username);
    let router_endpoint = format!("tls/zenoh-{cn}.local:{ROUTER_PORT}");

    let cfg = DverseConfig {
        username: username.clone(),
        password: password.clone(),
        keycloak_url: KEYCLOAK_URL.into(),
        keycloak_realm: KEYCLOAK_REALM.into(),
        client_id: CLIENT_ID.into(),
        client_secret: CLIENT_SECRET.into(),
        ca_url: CA_URL.into(),
        ca_root_pem_path: DverseConfig::ca_root_pem_path_default(),
        cert_dir,
        router_listen: ROUTER_LISTEN.into(),
        router_endpoint,
        session_role,
    };

    cfg.save().map_err(|e| e.to_string())?;

    // Stage config onto the embedded router's AppState so router::run picks it up.
    let router = Arc::clone(&state.0.lock().unwrap().router);
    {
        let mut r = router.lock().unwrap();
        r.session_role = cfg.session_role.clone();
        r.session_id = cfg.session_id();
        r.staged_config = Some(cfg);
    }
    // Flip screen to Loading — snapshot() will transition to Main once running.
    state.0.lock().unwrap().screen = AppScreen::Loading;

    Ok(())
}

#[tauri::command]
fn logout(state: State<'_, AppStateWrapper>) {
    let mut st = state.0.lock().unwrap();
    st.screen = AppScreen::Login;
    // Clear session view in router state so the GUI resets cleanly.
    let mut r = st.router.lock().unwrap();
    r.session_id = String::new();
    r.log.clear();
    r.connected_nodes.clear();
}

// ── Commands: register ────────────────────────────────────────────────────────

#[tauri::command]
async fn register(username: String, password: String, confirm: String) -> Result<String, String> {
    if username.is_empty() {
        return Err("Username is required.".into());
    }
    if username.contains('@') || username.contains(' ') {
        return Err("Username must not contain '@' or spaces.".into());
    }
    if password.len() < 8 {
        return Err("Password must be at least 8 characters.".into());
    }
    if password != confirm {
        return Err("Passwords do not match.".into());
    }
    register_user_async(&username, &password).await?;
    info!(username = %username, "register: account created");
    Ok(format!("Account '{username}' created. You can now sign in."))
}

async fn register_user_async(username: &str, password: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let token_url =
        format!("{KEYCLOAK_URL}/realms/{KEYCLOAK_REALM}/protocol/openid-connect/token");
    let token_resp = client
        .post(&token_url)
        .form(&[
            ("client_id", REGISTRATION_CLIENT_ID),
            ("client_secret", REGISTRATION_CLIENT_SECRET),
            ("grant_type", "client_credentials"),
        ])
        .send()
        .await
        .map_err(|e| format!("Token request failed: {e}"))?;

    let status = token_resp.status();
    let body = token_resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Admin login failed ({status}): {body}"));
    }
    let token: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Token parse error: {e}"))?;
    let access_token = token["access_token"]
        .as_str()
        .ok_or("No access_token in response")?
        .to_string();

    let email = format!("{username}@dverse.yordanmitev.me");
    let users_url = format!("{KEYCLOAK_URL}/admin/realms/{KEYCLOAK_REALM}/users");
    let user_payload = serde_json::json!({
        "username": username,
        "email": email,
        "firstName": username,
        "lastName": "",
        "emailVerified": true,
        "enabled": true,
        "requiredActions": [],
        "credentials": [{"type": "password", "value": password, "temporary": false}]
    });
    let create_resp = client
        .post(&users_url)
        .bearer_auth(&access_token)
        .json(&user_payload)
        .send()
        .await
        .map_err(|e| format!("User creation failed: {e}"))?;

    let status = create_resp.status();
    if status.is_success() || status.as_u16() == 201 {
        Ok(())
    } else {
        let body = create_resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["errorMessage"].as_str().map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {status}: {body}"));
        Err(msg)
    }
}

// ── Commands: discovery ───────────────────────────────────────────────────────

#[tauri::command]
async fn discover_routers() -> Result<Vec<DiscoveredRouter>, String> {
    let daemon =
        ServiceDaemon::new().map_err(|e| format!("mDNS daemon failed: {e}"))?;
    let receiver = daemon
        .browse(DVERSE_SERVICE)
        .map_err(|e| format!("mDNS browse failed: {e}"))?;

    let mut routers: HashMap<String, DiscoveredRouter> = HashMap::new();
    let deadline =
        tokio::time::Instant::now() + Duration::from_millis(DISCOVERY_TIMEOUT_MS);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }
        let ev = tokio::time::timeout(
            remaining,
            tokio::task::spawn_blocking({
                let recv = receiver.clone();
                move || recv.recv()
            }),
        ).await;

        match ev {
            Ok(Ok(Ok(ServiceEvent::ServiceResolved(info)))) => {
                let host = info.get_addresses().iter().next()
                    .map(|a| a.to_string()).unwrap_or_default();
                if host.is_empty() { continue; }
                let port = info.get_port();
                let name = info.get_fullname().to_string();
                let props = info.get_properties();
                let cn = props.get("cn").map(|v| v.val_str()).unwrap_or_default().to_string();
                let session = props.get("session").map(|v| v.val_str())
                    .unwrap_or_else(|| cn.as_str()).to_string();
                let zenoh_addr = format!("tcp/{host}:{port}");
                routers.insert(zenoh_addr.clone(),
                    DiscoveredRouter { name, host, port, zenoh_addr, cn, session });
            }
            Ok(Ok(Ok(ServiceEvent::SearchStopped(_)))) => break,
            Ok(Ok(Ok(_))) => {}
            _ => break,
        }
    }

    let _ = daemon.shutdown();
    let found: Vec<DiscoveredRouter> = routers.into_values().collect();
    info!(count = found.len(), "router discovery finished");
    Ok(found)
}

// ── Commands: bot management ──────────────────────────────────────────────────

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
async fn stop_bot(id: String, processes: State<'_, BotProcesses>) -> Result<BotStatus, String> {
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
    let statuses: Vec<BotStatus> = map.iter_mut()
        .map(|(id, child)| {
            let running = child.try_wait().map(|s| s.is_none()).unwrap_or(false);
            BotStatus { id: id.clone(), running }
        })
        .collect();
    map.retain(|_, child| child.try_wait().map(|s| s.is_none()).unwrap_or(false));
    Ok(statuses)
}

fn resolve_bot_agent(config: &BotConfig) -> Result<(String, Vec<String>), String> {
    let candidates = ["./bot_agent.py", "../chat-app/bot_agent.py", "../../chat-app/bot_agent.py"];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            if which_on_path("uv") {
                return Ok(("uv".into(), vec!["run".into(), path.to_string()]));
            }
            return Ok(("python3".into(), vec![path.to_string()]));
        }
    }
    if which_on_path("bot_agent") {
        return Ok(("bot_agent".into(), vec![]));
    }
    let _ = config;
    Err("bot_agent not found.".into())
}

fn which_on_path(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd).output()
        .map(|o| o.status.success()).unwrap_or(false)
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load saved config if present — router::run will start immediately.
    let existing_config = DverseConfig::load().ok();
    let router_state = Arc::new(Mutex::new(zr::AppState::new(existing_config)));

    // Wire tracing events into AppState.log (same as egui build).
    zenoh_router::logging::init_with_gui_sink(Arc::clone(&router_state));

    let inner = Arc::new(Mutex::new(InnerState::new(Arc::clone(&router_state))));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppStateWrapper(inner))
        .manage(BotProcesses::default())
        .setup(move |_app| {
            // Spawn the embedded router once. It blocks in Phase 1 until
            // `login` stages a DverseConfig onto router_state.staged_config.
            info!("starting embedded dverse router");
            let rs = Arc::clone(&router_state);
            tauri::async_runtime::spawn(async move {
                zenoh_router::router::run(rs).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            login,
            logout,
            register,
            discover_routers,
            start_bot,
            stop_bot,
            get_bot_statuses,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
