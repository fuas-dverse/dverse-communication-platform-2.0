use std::collections::HashMap;
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bot_framework::config::{DverseConfig, SessionRole};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};
use tauri::State;

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

// ── Inner mutable state ───────────────────────────────────────────────────────

pub struct InnerState {
    pub screen: AppScreen,
    pub router_status: RouterStatus,
    pub session_role: SessionRole,
    pub session_id: String,
    pub announces: HashMap<String, bot_framework::announce::AgentAnnounce>,
    pub node_last_seen: HashMap<(String, String), Instant>,
    pub log: Vec<String>,
    pub error: Option<String>,
}

impl InnerState {
    fn new() -> Self {
        let screen = if DverseConfig::exists() {
            AppScreen::Loading
        } else {
            AppScreen::Login
        };
        Self {
            screen,
            router_status: RouterStatus::Idle,
            session_role: SessionRole::Admin,
            session_id: String::new(),
            announces: HashMap::new(),
            node_last_seen: HashMap::new(),
            log: Vec::new(),
            error: None,
        }
    }

    fn push_log(&mut self, msg: impl Into<String>) {
        if self.log.len() >= 200 {
            self.log.remove(0);
        }
        self.log.push(msg.into());
    }

    fn snapshot(&self) -> AppSnapshot {
        let now = Instant::now();
        let mut node_map: HashMap<String, NodeInfo> = HashMap::new();

        for ((cn, agent_name), &last) in &self.node_last_seen {
            let age = now.saturating_duration_since(last);
            let status = if age < Duration::from_secs(10) {
                AgentStatus::Online
            } else if age < Duration::from_secs(30) {
                AgentStatus::Degraded
            } else {
                AgentStatus::Offline
            };
            let key = format!("{cn}/{agent_name}");
            let ann = self.announces.get(&key);
            let agent = AgentInfo {
                version: ann.map(|a| a.version.clone()).unwrap_or_default(),
                publishes: ann.map(|a| a.publishes.clone()).unwrap_or_default(),
                subscribes: ann.map(|a| a.subscribes.clone()).unwrap_or_default(),
                status,
                last_seen_secs_ago: age.as_secs(),
            };
            node_map
                .entry(cn.clone())
                .or_insert_with(|| NodeInfo {
                    cn: cn.clone(),
                    agents: HashMap::new(),
                })
                .agents
                .insert(agent_name.clone(), agent);
        }

        let mut nodes: Vec<NodeInfo> = node_map.into_values().collect();
        nodes.sort_by(|a, b| a.cn.cmp(&b.cn));
        for node in &mut nodes {
            let mut names: Vec<String> = node.agents.keys().cloned().collect();
            names.sort();
            let sorted: HashMap<String, AgentInfo> = names
                .into_iter()
                .map(|n| {
                    let v = node.agents.remove(&n).unwrap();
                    (n, v)
                })
                .collect();
            node.agents = sorted;
        }

        AppSnapshot {
            screen: self.screen.clone(),
            router_status: self.router_status.clone(),
            session_id: self.session_id.clone(),
            session_role: SessionRoleDto::from(&self.session_role),
            connected_nodes: nodes,
            log: self.log.clone(),
            error: self.error.clone(),
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

    let bg = {
        let mut st = state.0.lock().unwrap();
        st.screen = AppScreen::Loading;
        st.session_role = cfg.session_role.clone();
        st.session_id = cfg.session_id();
        st.error = None;
        Arc::clone(&state.0)
    };

    tokio::spawn(async move {
        run_router_bg(bg, cfg).await;
    });

    Ok(())
}

#[tauri::command]
fn logout(state: State<'_, AppStateWrapper>) {
    let mut st = state.0.lock().unwrap();
    st.screen = AppScreen::Login;
    st.router_status = RouterStatus::Idle;
    st.session_id = String::new();
    st.error = None;
    st.log.clear();
    st.announces.clear();
    st.node_last_seen.clear();
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
    register_user_async(&payload.username, &payload.password).await?;
    Ok(format!(
        "Account '{}' created. You can now sign in.",
        payload.username
    ))
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
    Ok(routers.into_values().collect())
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
    let child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start bot: {e}"))?;
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
    
    let inner = Arc::new(Mutex::new(InnerState::new()));
    let bg_inner = Arc::clone(&inner);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppStateWrapper(inner))
        .manage(BotProcesses::default())
        .setup(move |_app| {
            if DverseConfig::exists() {
                if let Ok(cfg) = DverseConfig::load() {
                    let bg = Arc::clone(&bg_inner);
                    tokio::spawn(async move {
                        run_router_bg(bg, cfg).await;
                    });
                }
            }
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

// ── Background: cert + zenoh client + agent subscription ─────────────────────

async fn run_router_bg(state: Arc<Mutex<InnerState>>, cfg: DverseConfig) {
    use bot_framework::cert;

    {
        let mut st = state.lock().unwrap();
        st.router_status = RouterStatus::Acquiring;
        st.session_role = cfg.session_role.clone();
        st.session_id = cfg.session_id();
        st.push_log("bootstrapping CA root certificate");
    }

    let ca_root_path = std::path::PathBuf::from(&cfg.ca_root_pem_path);
    if let Err(e) = cert::bootstrap_ca_root(&cfg.ca_url, &ca_root_path).await {
        let mut st = state.lock().unwrap();
        st.router_status = RouterStatus::Error(e.to_string());
        st.screen = AppScreen::Login;
        st.error = Some(format!("CA bootstrap failed: {e}"));
        return;
    }

    {
        state.lock().unwrap().push_log("checking router certificate");
    }

    let cert_paths = match acquire_or_reuse(&cfg).await {
        Ok(p) => p,
        Err(e) => {
            let mut st = state.lock().unwrap();
            st.router_status = RouterStatus::Error(e.to_string());
            st.screen = AppScreen::Login;
            st.error = Some(format!("Certificate error: {e}"));
            return;
        }
    };

    {
        let mut st = state.lock().unwrap();
        st.router_status = RouterStatus::Running;
        st.screen = AppScreen::Main;
        st.push_log(format!(
            "connected to router at {}",
            cfg.router_endpoint
        ));
    }

    subscribe_agents(state, cfg, cert_paths).await;
}

async fn acquire_or_reuse(
    cfg: &DverseConfig,
) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)> {
    use bot_framework::cert;
    let c = cert::cert_path(&cfg.cert_dir, "router");
    let k = cert::key_path(&cfg.cert_dir, "router");
    let ca = cert::ca_path(&cfg.cert_dir, "router");
    let max_age = Duration::from_secs(23 * 3600);
    if cert::needs_renewal(&c, max_age, Some(&cfg.operator_cn())).await {
        let cert_cfg = cfg.cert_config_for("router")?;
        cert::acquire(&cert_cfg).await
    } else {
        Ok((c, k, ca))
    }
}

async fn subscribe_agents(
    state: Arc<Mutex<InnerState>>,
    cfg: DverseConfig,
    (cert_p, key_p, ca_p): (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf),
) {
    use bot_framework::announce::AgentAnnounce;

    let json_str =
        |s: &str| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""));

    let mut zcfg = zenoh::Config::default();
    let _ = zcfg.insert_json5("mode", "\"client\"");
    let ep_json = format!("[\"{}\"]", cfg.router_endpoint);
    let _ = zcfg.insert_json5("connect/endpoints", &ep_json);
    let _ = zcfg.insert_json5(
        "transport/link/tls/root_ca_certificate",
        &json_str(&ca_p.to_string_lossy()),
    );
    let _ = zcfg.insert_json5("transport/link/tls/enable_mtls", "true");
    let _ = zcfg.insert_json5(
        "transport/link/tls/connect_certificate",
        &json_str(&cert_p.to_string_lossy()),
    );
    let _ = zcfg.insert_json5(
        "transport/link/tls/connect_private_key",
        &json_str(&key_p.to_string_lossy()),
    );
    let _ = zcfg.insert_json5("scouting/multicast/enabled", "false");

    let session = match zenoh::open(zcfg).await {
        Ok(s) => s,
        Err(e) => {
            state
                .lock()
                .unwrap()
                .push_log(format!("zenoh client connect failed: {e}"));
            return;
        }
    };

    let subscriber = match session
        .declare_subscriber("dverse/nodes/announce/**")
        .await
    {
        Ok(s) => s,
        Err(e) => {
            state
                .lock()
                .unwrap()
                .push_log(format!("subscriber failed: {e}"));
            return;
        }
    };

    let evict_after = Duration::from_secs(90);
    let mut reap_tick = tokio::time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            msg = subscriber.recv_async() => {
                match msg {
                    Ok(s) => {
                        let bytes = s.payload().to_bytes();
                        if let Ok(ann) = serde_json::from_slice::<AgentAnnounce>(&bytes) {
                            let key = format!("{}/{}", ann.cn, ann.agent_name);
                            let now = Instant::now();
                            let mut st = state.lock().unwrap();
                            st.node_last_seen.insert(
                                (ann.cn.clone(), ann.agent_name.clone()),
                                now,
                            );
                            st.announces.insert(key, ann);
                        }
                    }
                    Err(_) => break,
                }
            }
            _ = reap_tick.tick() => {
                let now = Instant::now();
                let mut st = state.lock().unwrap();
                st.node_last_seen
                    .retain(|_, last| now.saturating_duration_since(*last) < evict_after);
                let live: std::collections::HashSet<String> = st
                    .node_last_seen
                    .keys()
                    .map(|(cn, ag)| format!("{cn}/{ag}"))
                    .collect();
                st.announces.retain(|k, _| live.contains(k));
            }
        }
    }
}
