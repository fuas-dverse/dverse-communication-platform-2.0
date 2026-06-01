use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bot_framework::config::{DverseConfig, SessionRole};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{error, info, warn};
// The embedded router's state is the source of truth for status / session /
// connected-nodes / log. The Tauri layer only adds GUI screen routing.
use zenoh_router::state as zr;

// ── Constants (mirrored from zenoh_router) ────────────────────────────────────

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

// ── State types ───────────────────────────────────────────────────────────────

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AppScreen {
    Login,
    Register,
    Loading,
    Main,
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

// ── Status mappers (library types → serde DTOs) ───────────────────────────────

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

// ── Inner mutable state ───────────────────────────────────────────────────────
//
// The embedded router owns status / session / connected-nodes / log via its
// `AppState`. The Tauri layer keeps only the GUI screen-routing hint; the
// effective screen is derived from the router status at snapshot time.

pub struct InnerState {
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

        // Effective screen: stay on Login/Register until a session is staged,
        // then follow the router's lifecycle. Frontend renders purely off this.
        let screen = match self.screen {
            AppScreen::Login => AppScreen::Login,
            AppScreen::Register => AppScreen::Register,
            _ => match rs.router_status {
                zr::RouterStatus::Running => AppScreen::Main,
                zr::RouterStatus::Error(_) => AppScreen::Login,
                _ => AppScreen::Loading,
            },
        };

        let mut nodes: Vec<NodeInfo> = rs
            .connected_nodes
            .values()
            .map(|n| {
                let agents = n
                    .agents
                    .iter()
                    .map(|(name, ag)| {
                        let agent = AgentInfo {
                            version: ag.version.clone(),
                            publishes: ag.publishes.clone(),
                            subscribes: ag.subscribes.clone(),
                            status: map_agent_status(ag.status),
                            last_seen_secs_ago: now
                                .saturating_duration_since(ag.last_seen)
                                .as_secs(),
                        };
                        (name.clone(), agent)
                    })
                    .collect();
                NodeInfo { cn: n.cn.clone(), agents }
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

// ── Wire DTOs ─────────────────────────────────────────────────────────────────

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
}

// ── Commands: state ───────────────────────────────────────────────────────────

#[tauri::command]
fn get_state(state: State<'_, AppStateWrapper>) -> AppSnapshot {
    state.0.lock().unwrap().snapshot()
}

// ── Commands: auth ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginPayload {
    pub username: String,
    pub password: String,
    pub create_session: bool,
    pub join_admin_cn: String,
}

#[tauri::command]
async fn login(
    payload: LoginPayload,
    state: State<'_, AppStateWrapper>,
) -> Result<(), String> {
    if payload.username.is_empty() {
        return Err("Username is required.".into());
    }
    if payload.password.is_empty() {
        return Err("Password is required.".into());
    }

    let session_role = if payload.create_session {
        SessionRole::Admin
    } else {
        let admin_cn = payload.join_admin_cn.trim().to_string();
        if admin_cn.is_empty() {
            return Err("Admin username is required to join a session.".into());
        }
        SessionRole::Client { admin_cn }
    };

    let cert_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("dverse")
        .join("certs");

    let cn = payload
        .username
        .split('@')
        .next()
        .unwrap_or(&payload.username);
    let router_endpoint = format!("tls/zenoh-{cn}.local:{ROUTER_PORT}");

    let cfg = DverseConfig {
        username: payload.username.clone(),
        password: payload.password.clone(),
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

    // Stage the config onto the embedded router's AppState; the router::run
    // task (spawned once at startup) is waiting in Phase 1 and picks it up.
    let router = {
        let st = state.0.lock().unwrap();
        Arc::clone(&st.router)
    };
    info!(
        username = %cfg.username,
        session_id = %cfg.session_id(),
        role = ?cfg.session_role,
        "login: staging session config for embedded router"
    );
    {
        let mut r = router.lock().unwrap();
        r.session_role = cfg.session_role.clone();
        r.session_id = cfg.session_id();
        r.staged_config = Some(cfg);
    }
    {
        let mut st = state.0.lock().unwrap();
        st.screen = AppScreen::Loading;
    }

    Ok(())
}

#[tauri::command]
fn logout(state: State<'_, AppStateWrapper>) {
    // Soft logout: return the GUI to Login and clear the router's session view.
    // The embedded router task keeps running in the background (matching the
    // prior soft-logout behaviour); a full session teardown is future work.
    let router = {
        let mut st = state.0.lock().unwrap();
        st.screen = AppScreen::Login;
        Arc::clone(&st.router)
    };
    info!("logout: resetting session view (embedded router stays running)");
    let mut r = router.lock().unwrap();
    r.router_status = zr::RouterStatus::Idle;
    r.session_id = String::new();
    r.connected_nodes.clear();
    r.log.clear();
}

// ── Commands: register ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterPayload {
    pub username: String,
    pub password: String,
    pub confirm: String,
}

#[tauri::command]
async fn register(payload: RegisterPayload) -> Result<String, String> {
    if payload.username.is_empty() {
        return Err("Username is required.".into());
    }
    if payload.username.contains('@') || payload.username.contains(' ') {
        return Err("Username must not contain '@' or spaces.".into());
    }
    if payload.password.len() < 8 {
        return Err("Password must be at least 8 characters.".into());
    }
    if payload.password != payload.confirm {
        return Err("Passwords do not match.".into());
    }
    match register_user_async(&payload.username, &payload.password).await {
        Ok(()) => {
            info!(username = %payload.username, "register: account created");
            Ok(format!(
                "Account '{}' created. You can now sign in.",
                payload.username
            ))
        }
        Err(e) => {
            warn!(username = %payload.username, error = %e, "register: failed");
            Err(e)
        }
    }
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

// ── Commands: router discovery ────────────────────────────────────────────────

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
        if remaining.is_zero() {
            break;
        }
        let ev = tokio::time::timeout(
            remaining,
            tokio::task::spawn_blocking({
                let recv = receiver.clone();
                move || recv.recv()
            }),
        )
        .await;

        match ev {
            Ok(Ok(Ok(ServiceEvent::ServiceResolved(info)))) => {
                let host = info
                    .get_addresses()
                    .iter()
                    .next()
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                if host.is_empty() {
                    continue;
                }
                let port = info.get_port();
                let name = info.get_fullname().to_string();
                let zenoh_addr = format!("tcp/{host}:{port}");
                routers.insert(
                    zenoh_addr.clone(),
                    DiscoveredRouter { name, host, port, zenoh_addr },
                );
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
        .arg("--name")
        .arg(&config.name)
        .arg("--router")
        .arg(&config.zenoh_router)
        .arg("--description")
        .arg(&config.description);
    if config.llm_backend == "ollama" {
        cmd.arg("--ollama-url")
            .arg(&config.ollama_url)
            .arg("--model")
            .arg(&config.ollama_model);
    }
    if !config.claude_api_key.is_empty() {
        cmd.env("ANTHROPIC_API_KEY", &config.claude_api_key);
    }
    if !config.system_prompt.is_empty() {
        cmd.env("BOT_SYSTEM_PROMPT", &config.system_prompt);
    }
    let child = cmd.spawn().map_err(|e| {
        error!(id = %config.id, name = %config.name, error = %e, "start_bot: spawn failed");
        format!("Failed to start bot: {e}")
    })?;
    let id = config.id.clone();
    info!(id = %id, name = %config.name, backend = %config.llm_backend, "start_bot: bot started");
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
        info!(id = %id, "stop_bot: bot stopped");
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
    map.retain(|_, child| child.try_wait().map(|s| s.is_none()).unwrap_or(false));
    Ok(statuses)
}

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
    if which_on_path("bot_agent") {
        return Ok(("bot_agent".into(), vec![]));
    }
    let _ = config;
    Err("bot_agent not found.".into())
}

fn which_on_path(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ── Tauri entry point ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Create a Tokio runtime to support async operations in Tauri callbacks
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let _guard = rt.enter();
    
    // The embedded router's AppState. If a config already exists it's loaded as
    // `staged_config`, so router::run proceeds immediately; otherwise it waits
    // in Phase 1 until `login` stages one.
    let existing_config = DverseConfig::load().ok();
    let router_state = Arc::new(Mutex::new(zr::AppState::new(existing_config)));
    // Route tracing events (router, discovery, cert) into AppState.log so the
    // GUI log panel and LoadingScreen show real progress.
    zenoh_router::logging::init_with_gui_sink(Arc::clone(&router_state));

    let inner = Arc::new(Mutex::new(InnerState::new(Arc::clone(&router_state))));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppStateWrapper(inner))
        .manage(BotProcesses::default())
        .setup(move |_app| {
            // Run the real router (mode=router, discovery, ACL, agent inventory)
            // in-process — once. It drives the shared AppState the snapshot reads.
            info!("starting embedded dverse router");
            let rs = Arc::clone(&router_state);
            tokio::spawn(async move {
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
