use std::collections::HashMap;
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
    Chooser,
    RequestingJoin,
    Loading,
    Main,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JoinFlowDto {
    Pending { admin_cn: String },
    Allowed,
    Denied { reason: Option<String> },
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingRequestDto {
    pub requester_cn: String,
    pub note: Option<String>,
    pub requested_at: String,
    pub received_secs_ago: u64,
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
    /// Admin CNs of sessions visible on the LAN (mirrored from mDNS).
    /// Drives the chooser screen.
    pub visible_sessions: Vec<String>,
    /// Requester-side: state of the active join flow, if any.
    pub join_flow: Option<JoinFlowDto>,
    /// Admin-side: queued join requests awaiting Allow/Deny.
    pub pending_requests: Vec<PendingRequestDto>,
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
    /// Captured at `login` and consumed by `pick_session` when the user
    /// settles on Create-or-Join in the Chooser. `None` outside of that gap.
    pub staged_credentials: Option<(String, String)>,
}

impl InnerState {
    fn new(router: Arc<Mutex<zr::AppState>>) -> Self {
        // Sessions are runtime-only: always start at Login on every launch.
        // We deliberately don't consult `DverseConfig::exists()` — any
        // config file left on disk is overwritten by the next `pick_session`
        // and otherwise ignored by the launcher.
        Self {
            screen: AppScreen::Login,
            router,
            staged_credentials: None,
        }
    }

    fn snapshot(&self) -> AppSnapshot {
        let now = Instant::now();
        let rs = self.router.lock().unwrap();

        let router_status = map_router_status(&rs.router_status);
        let error = match &rs.router_status {
            zr::RouterStatus::Error(e) => Some(e.clone()),
            _ => None,
        };

        // Effective screen: stay on Login/Register/Chooser until a session
        // is staged, then follow the router's lifecycle. For client role,
        // override with RequestingJoin while we wait for an Allow.
        let waiting_for_admission = matches!(
            rs.join_flow,
            Some(zr::JoinFlowStatus::Pending { .. })
        );
        let screen = match self.screen {
            AppScreen::Login => AppScreen::Login,
            AppScreen::Register => AppScreen::Register,
            AppScreen::Chooser => AppScreen::Chooser,
            _ => match rs.router_status {
                zr::RouterStatus::Running => {
                    if waiting_for_admission {
                        AppScreen::RequestingJoin
                    } else {
                        AppScreen::Main
                    }
                }
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

        let join_flow = rs.join_flow.as_ref().map(|jf| match jf {
            zr::JoinFlowStatus::Pending { admin_cn, .. } => {
                JoinFlowDto::Pending { admin_cn: admin_cn.clone() }
            }
            zr::JoinFlowStatus::Allowed => JoinFlowDto::Allowed,
            zr::JoinFlowStatus::Denied { reason } => {
                JoinFlowDto::Denied { reason: reason.clone() }
            }
            zr::JoinFlowStatus::TimedOut => JoinFlowDto::TimedOut,
        });

        let pending_requests: Vec<PendingRequestDto> = rs
            .pending_requests
            .iter()
            .map(|p| PendingRequestDto {
                requester_cn: p.request.requester_cn.clone(),
                note: p.request.note.clone(),
                requested_at: p.request.requested_at.clone(),
                received_secs_ago: now.saturating_duration_since(p.received_at).as_secs(),
            })
            .collect();

        AppSnapshot {
            screen,
            router_status,
            session_id: rs.session_id.clone(),
            session_role: SessionRoleDto::from(&rs.session_role),
            connected_nodes: nodes,
            log: rs.log.clone(),
            error,
            visible_sessions: rs.visible_sessions.clone(),
            join_flow,
            pending_requests,
        }
    }
}

pub struct AppStateWrapper(pub Arc<Mutex<InnerState>>);

// ── Bot tasks ─────────────────────────────────────────────────────────────────

/// A running bot is a tokio task with a cancellation sender.
struct BotTask {
    cancel: tokio::sync::oneshot::Sender<()>,
}

#[derive(Default)]
pub struct BotProcesses(Mutex<HashMap<String, BotTask>>);

// ── Bridge state ──────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct BridgeState(Mutex<Option<BridgeHandle>>);

struct BridgeHandle {
    cancel: tokio::sync::oneshot::Sender<()>,
    ws_port: u16,
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeInfo {
    pub running: bool,
    pub ws_port: Option<u16>,
    /// Opaque connection string for 3rd-party clients (base64 JSON).
    pub connection_string: Option<String>,
    pub token: Option<String>,
}

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

/// Admin action: accept a pending join request. Olm-wraps the Megolm session
/// key to the requester, publishes the Allow, then bumps `admitted` (which
/// restarts the session under a fresh ACL).
#[tauri::command]
async fn admit_request(
    requester_cn: String,
    state: State<'_, AppStateWrapper>,
) -> Result<(), String> {
    let (router, session) = {
        let inner = state.0.lock().unwrap();
        let r = inner.router.clone();
        let s = r.lock().unwrap().zenoh_session.clone();
        (r, s)
    };
    let session = session.ok_or("router not running")?;
    zenoh_router::admission_handler::admit(&session, &*router, &requester_cn)
        .await
        .map_err(|e| e.to_string())
}

/// Admin action: deny a pending join request. Publishes a Deny and drops
/// the row; does not touch `admitted`.
#[tauri::command]
async fn deny_request(
    requester_cn: String,
    reason: Option<String>,
    state: State<'_, AppStateWrapper>,
) -> Result<(), String> {
    let (router, session) = {
        let inner = state.0.lock().unwrap();
        let r = inner.router.clone();
        let s = r.lock().unwrap().zenoh_session.clone();
        (r, s)
    };
    let session = session.ok_or("router not running")?;
    zenoh_router::admission_handler::deny(&session, &*router, &requester_cn, reason)
        .await
        .map_err(|e| e.to_string())
}

// ── Commands: auth ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct LoginPayload {
    pub username: String,
    pub password: String,
}

/// Credentials-only login. We don't know the user's session role yet — that's
/// the Chooser's job (issue #110). Stages the credentials and routes the GUI
/// to the Chooser; `pick_session` is what eventually builds + saves the
/// `DverseConfig` and starts the router.
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
    let mut st = state.0.lock().unwrap();
    st.staged_credentials = Some((payload.username.clone(), payload.password.clone()));
    st.screen = AppScreen::Chooser;
    info!(username = %payload.username, "login: credentials staged, routing to Chooser");
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct PickSessionPayload {
    /// `None` → create a new session (this user becomes the admin).
    /// `Some(cn)` → request to join the session hosted by `cn`.
    pub admin_cn: Option<String>,
}

/// Chooser action: consume the staged credentials, build the `DverseConfig`
/// with the chosen role, save it, and stage it on the embedded router.
/// The router boots; for `Client` roles, the admission handler then
/// auto-publishes a `JoinRequest` once the session is `Running`.
#[tauri::command]
async fn pick_session(
    payload: PickSessionPayload,
    state: State<'_, AppStateWrapper>,
) -> Result<(), String> {
    let (username, password) = {
        let mut st = state.0.lock().unwrap();
        st.staged_credentials
            .take()
            .ok_or_else(|| "no staged credentials — log in first".to_string())?
    };

    let session_role = match payload.admin_cn {
        None => SessionRole::Admin,
        Some(cn) => {
            let trimmed = cn.trim().to_string();
            if trimmed.is_empty() {
                return Err("admin_cn was provided but empty".into());
            }
            SessionRole::Client { admin_cn: trimmed }
        }
    };

    let cert_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("dverse")
        .join("certs");

    let cn = username.split('@').next().unwrap_or(&username);
    let router_endpoint = format!("tls/zenoh-{cn}.local:{ROUTER_PORT}");

    let cfg = DverseConfig {
        username,
        password,
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

    let router = {
        let st = state.0.lock().unwrap();
        Arc::clone(&st.router)
    };
    info!(
        username = %cfg.username,
        session_id = %cfg.session_id(),
        role = ?cfg.session_role,
        "pick_session: staging session config for embedded router"
    );
    let config_changed = {
        let mut r = router.lock().unwrap();
        r.session_role = cfg.session_role.clone();
        r.session_id = cfg.session_id();
        r.staged_config = Some(cfg);
        r.config_changed.clone()
    };
    // Wake the router task — without this, a logout + login + pick-different-
    // session would set `staged_config` but the router would keep using the
    // previous `current_cfg` because nothing tells `session_loop` to exit.
    config_changed.notify_one();
    state.0.lock().unwrap().screen = AppScreen::Loading;
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
    let mut r = router.lock().unwrap();
    let is_admin = matches!(r.session_role, SessionRole::Admin);
    let session_empty = r.connected_nodes.is_empty();

    if is_admin && session_empty {
        // Admin leaving an empty session: tear the session down entirely.
        info!("logout: admin left empty session — deleting session state");
        r.router_status = zr::RouterStatus::Idle;
        r.session_id = String::new();
        r.session_role = SessionRole::Admin;
        r.connected_nodes.clear();
        r.admitted.clear();
        r.crypto = None;
        r.pending_requests.clear();
        r.banned_cns.clear();
        r.log.clear();
    } else {
        info!("logout: resetting session view (embedded router stays running)");
        r.router_status = zr::RouterStatus::Idle;
        r.session_id = String::new();
        r.connected_nodes.clear();
        r.log.clear();
    }
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
async fn discover_routers(
    app_state: State<'_, AppStateWrapper>,
) -> Result<Vec<DiscoveredRouter>, String> {
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
                // Use the `cn` TXT field, NOT the mDNS fullname — the fullname
                // (e.g. "DVerse (test3)._dverse._tcp.local.") is what's *under*
                // the bonnet of mDNS, but admin_cn must be just the operator
                // CN ("test3"), because the chooser hands this value to
                // `pick_session(admin_cn = ...)` which becomes
                // `SessionRole::Client { admin_cn }` and `cfg.session_id()`.
                // Falls back to the fullname only so a missing TXT field
                // doesn't silently drop the row from the chooser.
                let name = info
                    .get_property("cn")
                    .map(|p| p.val_str().to_string())
                    .filter(|s: &String| !s.is_empty())
                    .unwrap_or_else(|| info.get_fullname().to_string());
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
    // Filter out the session we're currently hosting/in.
    let own_cn = {
        let inner = app_state.0.lock().unwrap();
        let r = inner.router.lock().unwrap();
        r.session_id.clone()
    };
    let found: Vec<DiscoveredRouter> = routers
        .into_values()
        .filter(|r| r.name != own_cn)
        .collect();
    info!(count = found.len(), "router discovery finished");
    Ok(found)
}

// ── Commands: bot management ──────────────────────────────────────────────────

#[tauri::command]
async fn start_bot(
    config: BotConfig,
    processes: State<'_, BotProcesses>,
    app_state: State<'_, AppStateWrapper>,
) -> Result<BotStatus, String> {
    let id = config.id.clone();
    {
        let map = processes.0.lock().map_err(|e| e.to_string())?;
        if map.contains_key(&id) {
            return Ok(BotStatus { id, running: true });
        }
    }

    // Read current DverseConfig from the router state so the bot joins the
    // active session without requiring a separate config file.
    let dverse_cfg = {
        let inner = app_state.0.lock().unwrap();
        let _r = inner.router.lock().unwrap();
        bot_framework::config::DverseConfig::load().ok()
    };
    let dverse_cfg = dverse_cfg.ok_or_else(|| "No active session — log in first".to_string())?;

    // Auto-admit the bot's CN so it passes the session ACL when admitted is non-empty.
    // The bot shares the operator's cert CN — admit it explicitly so a reload doesn't block it.
    {
        let inner = app_state.0.lock().unwrap();
        let mut r = inner.router.lock().unwrap();
        let bot_cn = dverse_cfg.operator_cn();
        if !r.admitted.contains(&bot_cn) {
            r.admitted.push(bot_cn);
            r.admitted_changed.notify_one();
        }
    }

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    let bot_name      = config.name.clone();
    let ollama_url    = config.ollama_url.clone();
    let ollama_model  = config.ollama_model.clone();
    let system_prompt = config.system_prompt.clone();
    let task_id       = id.clone();

    tokio::spawn(async move {
        if let Err(e) = run_ollama_bot(dverse_cfg, bot_name, ollama_url, ollama_model, system_prompt, cancel_rx).await {
            error!(id = %task_id, error = %e, "bot task exited with error");
        }
    });

    processes.0.lock().map_err(|e| e.to_string())?
        .insert(id.clone(), BotTask { cancel: cancel_tx });

    info!(id = %id, name = %config.name, "start_bot: bot task started");
    Ok(BotStatus { id, running: true })
}

#[tauri::command]
async fn stop_bot(
    id: String,
    processes: State<'_, BotProcesses>,
) -> Result<BotStatus, String> {
    let mut map = processes.0.lock().map_err(|e| e.to_string())?;
    if let Some(task) = map.remove(&id) {
        let _ = task.cancel.send(());
        info!(id = %id, "stop_bot: bot task cancelled");
    }
    Ok(BotStatus { id, running: false })
}

#[tauri::command]
async fn get_bot_statuses(
    processes: State<'_, BotProcesses>,
) -> Result<Vec<BotStatus>, String> {
    let map = processes.0.lock().map_err(|e| e.to_string())?;
    Ok(map.keys().map(|id| BotStatus { id: id.clone(), running: true }).collect())
}

async fn run_ollama_bot(
    cfg: bot_framework::config::DverseConfig,
    bot_name: String,
    ollama_url: String,
    ollama_model: String,
    system_prompt: String,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    use std::sync::Arc;
    use std::time::Duration;
    use a2a::llm::{ChatMessage, OllamaClient};
    use a2a::message::A2AMessage;
    use bot_framework::{announce::{AgentAnnouncer, AgentInfo}, cert, node::NodeConfig, payload_crypto::{self, PayloadCipher}};

    let cert_path = cert::cert_path(&cfg.cert_dir, &bot_name);
    let key_path  = cert::key_path(&cfg.cert_dir, &bot_name);
    let ca_path   = cert::ca_path(&cfg.cert_dir, &bot_name);

    if cert::needs_renewal(&cert_path, Duration::from_secs(23 * 3600), Some(&cfg.operator_cn())).await {
        let cert_cfg = cfg.cert_config_for(&bot_name)?;
        cert::acquire(&cert_cfg).await?;
    }

    // Bot runs in the same process as the embedded router — connect via
    // loopback instead of the mDNS hostname (tls/zenoh-<cn>.local) which
    // may not resolve on the same machine.
    let local_endpoint = format!("tls/127.0.0.1:{ROUTER_PORT}");
    let session = NodeConfig::mtls(&local_endpoint, &ca_path, &cert_path, &key_path)
        .skip_name_check()
        .with_namespace(cfg.session_id())
        .connect()
        .await?;

    let cn = cfg.operator_cn();
    let inbox_topic = format!("dverse/a2a/{}/inbox", bot_name);

    let _announcer = AgentAnnouncer::start(
        session.clone(),
        AgentInfo {
            cn: &cn,
            agent_name: &bot_name,
            version: env!("CARGO_PKG_VERSION"),
            publishes: vec![format!("dverse/a2a/*/inbox")],
            subscribes: vec![inbox_topic.clone()],
        },
    );

    let crypto_dir = cfg.cert_dir.join("megolm");
    let cipher = Arc::new(Mutex::new(PayloadCipher::new(&bot_name, &crypto_dir)?));
    cipher.lock().unwrap().publish_session_key(&crypto_dir, &bot_name)?;
    payload_crypto::spawn_agent_key_relay(
        Arc::clone(&cipher),
        session.clone(),
        cn.clone(),
        bot_name.clone(),
    );

    let inbox = session
        .declare_subscriber(&inbox_topic)
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    let llm = OllamaClient::new(&ollama_url, &ollama_model);
    let mut history: Vec<ChatMessage> = Vec::new();

    info!(bot = %bot_name, model = %ollama_model, "bot online");

    loop {
        let _ = cipher.lock().unwrap().refresh_receivers(&crypto_dir);

        tokio::select! {
            _ = &mut cancel => {
                info!(bot = %bot_name, "bot cancelled");
                break;
            }
            result = inbox.recv_async() => {
                let sample = match result {
                    Ok(s) => s,
                    Err(e) => { error!(error = %e, "inbox closed"); break; }
                };

                let bytes = sample.payload().to_bytes();
                let plaintext = cipher.lock().unwrap().decrypt(&bytes).unwrap_or_else(|_| bytes.to_vec());

                let msg: A2AMessage = match serde_json::from_slice(&plaintext) {
                    Ok(m) => m,
                    Err(e) => { warn!(error = %e, "bad A2AMessage"); continue; }
                };

                history.push(ChatMessage { role: "user".into(), content: msg.content.clone() });

                let raw = match llm.respond(&system_prompt, &history).await {
                    Ok(t) => t,
                    Err(e) => { error!(error = %e, "Ollama call failed"); continue; }
                };

                let reply_text = if let (Some(close), _) = (raw.find("</think>"), ()) {
                    raw[close + 8..].trim().to_string()
                } else {
                    raw.trim().to_string()
                };

                history.push(ChatMessage { role: "assistant".into(), content: reply_text.clone() });

                let reply = A2AMessage::new(&bot_name, &msg.from, reply_text, msg.turn + 1);
                let reply_topic = format!("dverse/a2a/{}/inbox", msg.from);
                let payload = serde_json::to_vec(&reply)?;
                let wire = cipher.lock().unwrap().encrypt(&payload).unwrap_or(payload);

                session.put(&reply_topic, wire).await
                    .map_err(|e| anyhow::anyhow!("put reply: {e}"))?;
            }
        }
    }

    Ok(())
}

// ── Bridge commands ───────────────────────────────────────────────────────────

#[tauri::command]
async fn start_bridge(
    state: State<'_, AppStateWrapper>,
    bridge: State<'_, BridgeState>,
) -> Result<BridgeInfo, String> {
    // Already running → return current info.
    {
        let guard = bridge.0.lock().map_err(|e| e.to_string())?;
        if let Some(h) = guard.as_ref() {
            let conn = build_connection_string("127.0.0.1", h.ws_port, &h.token);
            return Ok(BridgeInfo {
                running: true,
                ws_port: Some(h.ws_port),
                connection_string: Some(conn),
                token: Some(h.token.clone()),
            });
        }
    }

    let cfg = {
        let inner = state.0.lock().map_err(|e| e.to_string())?;
        let rs = inner.router.lock().map_err(|e| e.to_string())?;
        rs.active_config
            .clone()
            .ok_or("no active session config — start a session first")?
    };

    let token: String = {
        use rand::Rng;
        let bytes: [u8; 16] = rand::thread_rng().gen();
        hex::encode(bytes)
    };

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    let (port_tx, port_rx) = tokio::sync::oneshot::channel::<u16>();

    let cfg2 = cfg.clone();
    let token2 = token.clone();
    tokio::spawn(async move {
        if let Err(e) = run_bridge_bot(cfg2, token2, port_tx, cancel_rx).await {
            error!(error = %e, "bridge bot exited with error");
        }
    });

    // Wait for the bridge to bind and report its actual port (OS-assigned).
    let ws_port = port_rx.await.map_err(|_| "bridge failed to start".to_string())?;

    let conn = build_connection_string("127.0.0.1", ws_port, &token);
    bridge.0.lock().map_err(|e| e.to_string())?.replace(BridgeHandle {
        cancel: cancel_tx,
        ws_port,
        token: token.clone(),
    });

    {
        let inner = state.0.lock().map_err(|e| e.to_string())?;
        let mut rs = inner.router.lock().map_err(|e| e.to_string())?;
        rs.bridge_tokens.push(token.clone());
    }

    Ok(BridgeInfo {
        running: true,
        ws_port: Some(ws_port),
        connection_string: Some(conn),
        token: Some(token),
    })
}

#[tauri::command]
async fn stop_bridge(bridge: State<'_, BridgeState>) -> Result<BridgeInfo, String> {
    let mut guard = bridge.0.lock().map_err(|e| e.to_string())?;
    if let Some(h) = guard.take() {
        let _ = h.cancel.send(());
    }
    Ok(BridgeInfo { running: false, ws_port: None, connection_string: None, token: None })
}

#[tauri::command]
async fn get_bridge_info(bridge: State<'_, BridgeState>) -> Result<BridgeInfo, String> {
    let guard = bridge.0.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        None => Ok(BridgeInfo { running: false, ws_port: None, connection_string: None, token: None }),
        Some(h) => {
            let conn = build_connection_string("127.0.0.1", h.ws_port, &h.token);
            Ok(BridgeInfo {
                running: true,
                ws_port: Some(h.ws_port),
                connection_string: Some(conn),
                token: Some(h.token.clone()),
            })
        }
    }
}

fn build_connection_string(host: &str, port: u16, token: &str) -> String {
    use base64::Engine;
    let json = serde_json::json!({ "host": host, "port": port, "token": token });
    base64::engine::general_purpose::STANDARD.encode(json.to_string())
}

/// Bridge bot: connects to DVerse via mTLS (same as ollama bot), subscribes to
/// all room messages, decrypts with PayloadCipher, and re-publishes plaintext
/// over a local WebSocket. WebSocket clients send JSON and this publishes
/// encrypted back onto Zenoh.
async fn run_bridge_bot(
    cfg: bot_framework::config::DverseConfig,
    token: String,
    port_tx: tokio::sync::oneshot::Sender<u16>,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;
    use a2a::message::A2AMessage;
    use bot_framework::{announce::{AgentAnnouncer, AgentInfo}, cert, node::NodeConfig, payload_crypto::{self, PayloadCipher}};
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio::sync::broadcast;
    use tokio_tungstenite::tungstenite::Message as WsMsg;

    let bridge_name = "dverse-bridge";

    let cert_path = cert::cert_path(&cfg.cert_dir, bridge_name);
    let key_path  = cert::key_path(&cfg.cert_dir, bridge_name);
    let ca_path   = cert::ca_path(&cfg.cert_dir, bridge_name);

    if cert::needs_renewal(&cert_path, Duration::from_secs(23 * 3600), Some(&cfg.operator_cn())).await {
        let cert_cfg = cfg.cert_config_for(bridge_name)?;
        cert::acquire(&cert_cfg).await?;
    }

    let local_endpoint = format!("tls/127.0.0.1:{ROUTER_PORT}");
    let session = NodeConfig::mtls(&local_endpoint, &ca_path, &cert_path, &key_path)
        .skip_name_check()
        .with_namespace(cfg.session_id())
        .connect()
        .await?;

    let cn = cfg.operator_cn();
    let rooms_topic = "dverse/rooms/*/messages".to_string();
    let announce_topic = "dverse/nodes/announce/**".to_string();

    let _announcer = AgentAnnouncer::start(
        session.clone(),
        AgentInfo {
            cn: &cn,
            agent_name: bridge_name,
            version: env!("CARGO_PKG_VERSION"),
            publishes: vec![rooms_topic.clone()],
            subscribes: vec![rooms_topic.clone(), announce_topic.clone()],
        },
    );

    let crypto_dir = cfg.cert_dir.join("megolm");
    let cipher = Arc::new(Mutex::new(PayloadCipher::new(bridge_name, &crypto_dir)?));
    cipher.lock().unwrap().publish_session_key(&crypto_dir, bridge_name)?;
    payload_crypto::spawn_agent_key_relay(
        Arc::clone(&cipher),
        session.clone(),
        cn.clone(),
        bridge_name.to_string(),
    );

    let subscriber = session
        .declare_subscriber(&rooms_topic)
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    let announce_sub = session
        .declare_subscriber(&announce_topic)
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber announce: {e}"))?;

    let a2a_inbox = format!("dverse/a2a/{bridge_name}/inbox");
    let a2a_sub = session
        .declare_subscriber(&a2a_inbox)
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber a2a: {e}"))?;

    // pending_a2a maps turn_number → (room_id, bot_name)
    let pending_a2a: Arc<Mutex<HashMap<u32, (String, String)>>> = Arc::new(Mutex::new(HashMap::new()));
    let a2a_turn_counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));

    // Broadcast channel: Zenoh messages → all WS clients.
    let (tx, _) = broadcast::channel::<String>(256);
    let tx_arc = Arc::new(tx);

    // WebSocket server — bind to port 0 so OS picks a free port.
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let actual_port = listener.local_addr()?.port();
    let _ = port_tx.send(actual_port);
    info!(port = actual_port, "bridge WebSocket listening");

    let tx_ws = Arc::clone(&tx_arc);
    let session_ws = session.clone();
    let cipher_ws = Arc::clone(&cipher);
    let rooms_topic_ws = rooms_topic.clone();
    let token_ws = token.clone();
    let pending_a2a_ws = Arc::clone(&pending_a2a);
    let a2a_turn_counter_ws = Arc::clone(&a2a_turn_counter);
    let bridge_name_ws = bridge_name.to_string();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let mut rx = tx_ws.subscribe();
            let session2 = session_ws.clone();
            let cipher2 = Arc::clone(&cipher_ws);
            let topic2 = rooms_topic_ws.clone();
            let token2 = token_ws.clone();
            let pending_a2a2 = Arc::clone(&pending_a2a_ws);
            let turn_counter2 = Arc::clone(&a2a_turn_counter_ws);
            let bridge_name2 = bridge_name_ws.clone();

            tokio::spawn(async move {
                let ws = match tokio_tungstenite::accept_async(stream).await {
                    Ok(w) => w,
                    Err(e) => { warn!(error = %e, "WS handshake failed"); return; }
                };
                let (mut sink, mut src) = ws.split();

                // Validate token in first message.
                let auth_ok = match src.next().await {
                    Some(Ok(WsMsg::Text(t))) => {
                        serde_json::from_str::<serde_json::Value>(&t)
                            .ok()
                            .and_then(|v| v["token"].as_str().map(|s| s == token2))
                            .unwrap_or(false)
                    }
                    _ => false,
                };
                if !auth_ok {
                    let _ = sink.send(WsMsg::text(r#"{"error":"unauthorized"}"#)).await;
                    return;
                }
                let _ = sink.send(WsMsg::text(r#"{"ok":true}"#)).await;

                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            match msg {
                                Ok(m) => { let _ = sink.send(WsMsg::text(m)).await; }
                                Err(_) => break,
                            }
                        }
                        incoming = src.next() => {
                            match incoming {
                                Some(Ok(WsMsg::Text(t))) => {
                                    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&t) {
                                        if parsed.get("type").and_then(|v| v.as_str()) == Some("a2a") {
                                            let to_agent = parsed["to"].as_str().unwrap_or("").to_string();
                                            let room_id = parsed["room_id"].as_str().unwrap_or("").to_string();
                                            let content = parsed["content"].as_str().unwrap_or("").to_string();
                                            if !to_agent.is_empty() && !room_id.is_empty() {
                                                let turn = turn_counter2.fetch_add(1, Ordering::SeqCst);
                                                pending_a2a2.lock().unwrap().insert(turn, (room_id, to_agent.clone()));
                                                let reply = A2AMessage::new(&bridge_name2, &to_agent, content, turn);
                                                if let Ok(payload) = serde_json::to_vec(&reply) {
                                                    let wire = cipher2.lock().unwrap().encrypt(&payload).unwrap_or(payload);
                                                    let inbox_topic = format!("dverse/a2a/{to_agent}/inbox");
                                                    if let Err(e) = session2.put(&inbox_topic, wire).await {
                                                        warn!(error = %e, "a2a publish failed");
                                                    }
                                                }
                                            }
                                        } else {
                                            let payload = t.as_bytes().to_vec();
                                            let wire = cipher2.lock().unwrap().encrypt(&payload).unwrap_or(payload);
                                            if let Err(e) = session2.put(&topic2, wire).await {
                                                warn!(error = %e, "bridge publish failed");
                                            }
                                        }
                                    } else {
                                        let payload = t.as_bytes().to_vec();
                                        let wire = cipher2.lock().unwrap().encrypt(&payload).unwrap_or(payload);
                                        if let Err(e) = session2.put(&topic2, wire).await {
                                            warn!(error = %e, "bridge publish failed");
                                        }
                                    }
                                }
                                Some(Ok(WsMsg::Close(_))) | None => break,
                                _ => {}
                            }
                        }
                    }
                }
            });
        }
    });

    info!(bridge = bridge_name, "bridge bot online");

    loop {
        let _ = cipher.lock().unwrap().refresh_receivers(&crypto_dir);

        tokio::select! {
            _ = &mut cancel => {
                info!("bridge cancelled");
                break;
            }
            result = subscriber.recv_async() => {
                let sample = match result {
                    Ok(s) => s,
                    Err(e) => { error!(error = %e, "bridge subscriber closed"); break; }
                };
                let bytes = sample.payload().to_bytes();
                let plaintext = cipher.lock().unwrap().decrypt(&bytes).unwrap_or_else(|_| bytes.to_vec());
                if let Ok(text) = String::from_utf8(plaintext) {
                    let _ = tx_arc.send(text);
                }
            }
            result = announce_sub.recv_async() => {
                let sample = match result {
                    Ok(s) => s,
                    Err(e) => { error!(error = %e, "announce subscriber closed"); break; }
                };
                // Announce messages are plain JSON (not Megolm-encrypted).
                let bytes = sample.payload().to_bytes();
                if let Ok(text) = String::from_utf8(bytes.to_vec()) {
                    let _ = tx_arc.send(text);
                }
            }
            result = a2a_sub.recv_async() => {
                let sample = match result {
                    Ok(s) => s,
                    Err(e) => { error!(error = %e, "a2a sub closed"); break; }
                };
                let bytes = sample.payload().to_bytes();
                let plaintext = cipher.lock().unwrap().decrypt(&bytes).unwrap_or_else(|_| bytes.to_vec());
                if let Ok(text) = String::from_utf8(plaintext) {
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&text) {
                        let from = msg["from"].as_str().unwrap_or("").to_string();
                        let content = msg["content"].as_str().unwrap_or("").to_string();
                        let turn = msg["turn"].as_u64().unwrap_or(0) as u32;
                        // The bot replies with turn = our_turn + 1, so look up our_turn = turn - 1
                        let lookup_turn = turn.saturating_sub(1);
                        let room_info = pending_a2a.lock().unwrap().remove(&lookup_turn);
                        if let Some((room_id, _bot_name)) = room_info {
                            let msg_id = uuid::Uuid::new_v4().to_string();
                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs();
                            let created_at = format!("{ts}");
                            let response = serde_json::json!({
                                "type": "a2a_response",
                                "from": from,
                                "room_id": room_id,
                                "content": content,
                                "is_agent": true,
                                "sender": from,
                                "id": msg_id,
                                "created_at": created_at,
                            });
                            let _ = tx_arc.send(response.to_string());
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// ── Tauri entry point ─────────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Create a Tokio runtime to support async operations in Tauri callbacks
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let _guard = rt.enter();
    
    // Sessions are runtime-only: never auto-resume from a persisted config.
    // The embedded router waits in Phase 1 until `pick_session` stages a
    // fresh config; any leftover `~/.config/dverse/config.toml` from a
    // prior run is ignored (and overwritten on the next pick).
    let router_state = Arc::new(Mutex::new(zr::AppState::new(None)));
    // Route tracing events (router, discovery, cert) into AppState.log so the
    // GUI log panel and LoadingScreen show real progress.
    zenoh_router::logging::init_with_gui_sink(Arc::clone(&router_state));

    let inner = Arc::new(Mutex::new(InnerState::new(Arc::clone(&router_state))));

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(AppStateWrapper(inner))
        .manage(BotProcesses::default())
        .manage(BridgeState::default())
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
            admit_request,
            deny_request,
            pick_session,
            start_bridge,
            stop_bridge,
            get_bridge_info,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
