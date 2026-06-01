use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use bot_framework::config::{DverseConfig, SessionRole};

use crate::constants::{CA_URL, CLIENT_ID, CLIENT_SECRET, KEYCLOAK_REALM, KEYCLOAK_URL, REGISTRATION_CLIENT_ID, REGISTRATION_CLIENT_SECRET, ROUTER_LISTEN, ROUTER_PORT};
use crate::state::{AgentStatus, AppState, RouterStatus};

// ── Screen state (GUI thread only) ─────────────────────────────────────────────

pub enum Screen {
    Login(LoginForm),
    Register(RegisterForm),
    Loading,
    Main,
}

pub struct LoginForm {
    pub username: String,
    pub password: String,
    pub error: Option<String>,
    /// Radio: true = create a new session (this user becomes admin),
    /// false = join an existing session whose admin's CN is `join_admin_cn`.
    pub create_session: bool,
    /// Admin CN to join when `create_session == false`.
    pub join_admin_cn: String,
}

impl Default for LoginForm {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            error: None,
            create_session: true,
            join_admin_cn: String::new(),
        }
    }
}

pub struct RegisterForm {
    pub username: String,   // chosen username (becomes Keycloak username + email prefix)
    pub password: String,
    pub confirm: String,
    pub error: Option<String>,
    pub working: bool,
    pub success: Option<String>,
}

impl Default for RegisterForm {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            confirm: String::new(),
            error: None,
            working: false,
            success: None,
        }
    }
}

// ── App ────────────────────────────────────────────────────────────────────────

pub struct RouterApp {
    state: Arc<Mutex<AppState>>,
    screen: Screen,
}

impl RouterApp {
    pub fn new(state: Arc<Mutex<AppState>>, screen: Screen) -> Self {
        Self { state, screen }
    }
}

impl eframe::App for RouterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(200));

        let status = self.state.lock().unwrap().router_status.clone();

        // Screen transitions driven by background thread status.
        match &self.screen {
            Screen::Login(_) | Screen::Register(_) => {
                if !matches!(status, RouterStatus::Idle) {
                    self.screen = Screen::Loading;
                }
            }
            Screen::Loading => match &status {
                RouterStatus::Running => self.screen = Screen::Main,
                RouterStatus::Error(msg) => {
                    let mut form = LoginForm::default();
                    form.error = Some(msg.clone());
                    self.screen = Screen::Login(form);
                }
                _ => {}
            },
            Screen::Main => {}
        }

        match &mut self.screen {
            Screen::Login(form) => {
                if let Some(next) = show_login(ctx, &mut self.state, form) {
                    self.screen = next;
                }
            }
            Screen::Register(form) => {
                if let Some(next) = show_register(ctx, form) {
                    self.screen = next;
                }
            }
            Screen::Loading => show_loading(ctx, &self.state),
            Screen::Main => show_main(ctx, &mut self.state),
        }
    }
}

// ── Login screen ───────────────────────────────────────────────────────────────

/// Returns `Some(Screen)` to transition to, or `None` to stay.
fn show_login(ctx: &egui::Context, state: &mut Arc<Mutex<AppState>>, form: &mut LoginForm) -> Option<Screen> {
    let mut next: Option<Screen> = None;

    egui::CentralPanel::default().show(ctx, |ui| {
        let top_pad = (ui.available_height() - 320.0).max(0.0) / 2.0;
        ui.add_space(top_pad);

        ui.vertical_centered(|ui| {
            ui.heading("Sign in to dverse");
            ui.add_space(4.0);
            ui.weak(format!("Connecting to {KEYCLOAK_URL}"));
            ui.add_space(20.0);

            egui::Grid::new("login_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Username");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut form.username)
                            .hint_text("you@dverse.yordanmitev.me")
                            .min_size(egui::vec2(260.0, 0.0)),
                    );
                    ui.end_row();

                    ui.label("Password");
                    let p = ui.add(
                        egui::TextEdit::singleline(&mut form.password)
                            .password(true)
                            .min_size(egui::vec2(260.0, 0.0)),
                    );
                    ui.end_row();

                    // Submit on Enter in any field.
                    if (r.lost_focus() || p.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        try_login(form, state);
                    }
                });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // Session role selector: create your own session, or join an
            // existing one whose admin's CN you know.
            ui.horizontal(|ui| {
                ui.radio_value(&mut form.create_session, true, "Create new session");
                ui.add_space(12.0);
                ui.radio_value(&mut form.create_session, false, "Join session");
            });
            if !form.create_session {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Admin username");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.join_admin_cn)
                            .hint_text("e.g. alice")
                            .min_size(egui::vec2(220.0, 0.0)),
                    );
                });
            }

            ui.add_space(16.0);

            if let Some(err) = &form.error {
                ui.colored_label(egui::Color32::RED, err);
                ui.add_space(8.0);
            }

            ui.horizontal(|ui| {
                if ui.button("  Sign in  ").clicked() {
                    try_login(form, state);
                }
                ui.add_space(12.0);
                if ui.button("Register").clicked() {
                    next = Some(Screen::Register(RegisterForm::default()));
                }
            });

            ui.add_space(24.0);
            ui.separator();
            ui.add_space(8.0);

            egui::Grid::new("info_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    info_row(ui, "Identity provider", KEYCLOAK_URL);
                    info_row(ui, "Certificate authority", CA_URL);
                    info_row(ui, "Router listens on", ROUTER_LISTEN);
                });
        });
    });

    next
}

fn try_login(form: &mut LoginForm, state: &Arc<Mutex<AppState>>) {
    if form.username.is_empty() {
        form.error = Some("Username is required.".into());
        return;
    }
    if form.password.is_empty() {
        form.error = Some("Password is required.".into());
        return;
    }

    // Build session role from the radio + admin CN field.  When joining,
    // the admin CN is required and must be non-empty.
    let session_role = if form.create_session {
        SessionRole::Admin
    } else {
        let admin_cn = form.join_admin_cn.trim().to_string();
        if admin_cn.is_empty() {
            form.error = Some("Admin username is required to join a session.".into());
            return;
        }
        SessionRole::Client { admin_cn }
    };

    let cert_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("dverse")
        .join("certs");

    // The Step-CA x509 template sets SAN = DNS:zenoh-<preferred_username>.local,
    // so the endpoint must use that hostname for TLS to verify correctly.
    let cn = form.username.split('@').next().unwrap_or(&form.username);
    let router_endpoint = format!("tls/zenoh-{cn}.local:{ROUTER_PORT}");

    let cfg = DverseConfig {
        username: form.username.clone(),
        password: form.password.clone(),
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

    if let Err(e) = cfg.save() {
        form.error = Some(e.to_string());
        return;
    }

    let mut st = state.lock().unwrap();
    st.push_log(format!("Signed in as {}. Bootstrapping…", cfg.username));
    st.staged_config = Some(cfg);
    form.error = None;
}

// ── Register screen ────────────────────────────────────────────────────────────

fn show_register(ctx: &egui::Context, form: &mut RegisterForm) -> Option<Screen> {
    let mut next: Option<Screen> = None;

    egui::CentralPanel::default().show(ctx, |ui| {
        let top_pad = (ui.available_height() - 360.0).max(0.0) / 2.0;
        ui.add_space(top_pad);

        ui.vertical_centered(|ui| {
            ui.heading("Create a dverse account");
            ui.add_space(4.0);
            ui.weak(format!("Your account will be created on {KEYCLOAK_URL}"));
            ui.add_space(20.0);

            egui::Grid::new("register_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Username");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.username)
                            .hint_text("alice")
                            .min_size(egui::vec2(260.0, 0.0)),
                    );
                    ui.end_row();

                    ui.label("Password");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.password)
                            .password(true)
                            .min_size(egui::vec2(260.0, 0.0)),
                    );
                    ui.end_row();

                    ui.label("Confirm");
                    ui.add(
                        egui::TextEdit::singleline(&mut form.confirm)
                            .password(true)
                            .min_size(egui::vec2(260.0, 0.0)),
                    );
                    ui.end_row();
                });

            ui.add_space(16.0);

            if let Some(err) = &form.error {
                ui.colored_label(egui::Color32::RED, err);
                ui.add_space(8.0);
            }
            if let Some(ok) = &form.success {
                ui.colored_label(egui::Color32::GREEN, ok);
                ui.add_space(8.0);
            }

            ui.horizontal(|ui| {
                let btn = ui.add_enabled(!form.working, egui::Button::new("  Create account  "));
                if btn.clicked() {
                    form.error = None;
                    form.success = None;
                    match validate_registration(form) {
                        Ok(()) => {
                            form.working = true;
                            // Synchronous call — runs on GUI thread, acceptable for a local tool.
                            match register_user(&form.username, &form.password) {
                                Ok(()) => {
                                    form.success = Some(format!(
                                        "Account '{}' created. You can now sign in.",
                                        form.username
                                    ));
                                    form.working = false;
                                }
                                Err(e) => {
                                    form.error = Some(e);
                                    form.working = false;
                                }
                            }
                        }
                        Err(e) => form.error = Some(e),
                    }
                }

                ui.add_space(12.0);
                if ui.button("Back to sign in").clicked() {
                    next = Some(Screen::Login(LoginForm {
                        username: form.username.clone(),
                        ..Default::default()
                    }));
                }
            });
        });
    });

    next
}

fn validate_registration(form: &RegisterForm) -> Result<(), String> {
    if form.username.is_empty() {
        return Err("Username is required.".into());
    }
    if form.username.contains('@') || form.username.contains(' ') {
        return Err("Username must not contain '@' or spaces.".into());
    }
    if form.password.len() < 8 {
        return Err("Password must be at least 8 characters.".into());
    }
    if form.password != form.confirm {
        return Err("Passwords do not match.".into());
    }
    Ok(())
}

/// Create a Keycloak user via the Admin REST API using the hardcoded admin credentials.
fn register_user(username: &str, password: &str) -> Result<(), String> {
    // Keycloak Admin REST API is synchronous-friendly via blocking reqwest.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    rt.block_on(async move {
        register_user_async(username, password).await
    })
}

async fn register_user_async(username: &str, password: &str) -> Result<(), String> {
    let client = reqwest::Client::new();

    // 1. Obtain a service-account token using client credentials.
    //    The dverse-registration client has manage-users scoped to create-only.
    let token_url = format!("{KEYCLOAK_URL}/realms/{KEYCLOAK_REALM}/protocol/openid-connect/token");
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

    let token: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("Token parse error: {e}"))?;
    let access_token = token["access_token"]
        .as_str()
        .ok_or("No access_token in response")?
        .to_string();

    // 2. Create the user.
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
        "credentials": [{
            "type": "password",
            "value": password,
            "temporary": false
        }]
    });

    let create_resp = client
        .post(&users_url)
        .bearer_auth(&access_token)
        .json(&user_payload)
        .send()
        .await
        .map_err(|e| format!("User creation request failed: {e}"))?;

    let status = create_resp.status();
    if status.is_success() || status.as_u16() == 201 {
        Ok(())
    } else {
        let body = create_resp.text().await.unwrap_or_default();
        // Keycloak returns {"errorMessage": "..."} on conflict etc.
        let msg = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["errorMessage"].as_str().map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {status}: {body}"));
        Err(msg)
    }
}

// ── Loading screen ─────────────────────────────────────────────────────────────

fn show_loading(ctx: &egui::Context, state: &Arc<Mutex<AppState>>) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(ui.available_height() / 3.0);
        ui.vertical_centered(|ui| {
            ui.heading("Connecting…");
            ui.add_space(12.0);
            let st = state.lock().unwrap();
            if let Some(last) = st.log.last() {
                ui.label(last);
            }
        });
    });
}

// ── Main screen ────────────────────────────────────────────────────────────────

fn show_main(ctx: &egui::Context, state: &mut Arc<Mutex<AppState>>) {
    let st = state.lock().unwrap();

    // Session badge — declared first so it renders above status_bar.  egui's
    // top panels stack in declaration order.
    //
    // Hidden entirely until a config has been accepted (signalled by a
    // non-empty `session_id`).  Otherwise the Admin variant would render
    // `Admin · ` with an empty CN during the Idle / Acquiring states.
    if !st.session_id.is_empty() {
        egui::TopBottomPanel::top("session_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Session:");
                match &st.session_role {
                    SessionRole::Admin => {
                        ui.colored_label(
                            egui::Color32::LIGHT_BLUE,
                            format!("Admin · {}", st.session_id),
                        );
                    }
                    SessionRole::Client { admin_cn } => {
                        ui.colored_label(
                            egui::Color32::LIGHT_GREEN,
                            format!("Joined · admin: {admin_cn}"),
                        );
                    }
                }
            });
        });
    }

    egui::TopBottomPanel::top("status_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Router:");
            match &st.router_status {
                RouterStatus::Idle     => { ui.label("Idle"); }
                RouterStatus::Acquiring => { ui.colored_label(egui::Color32::YELLOW, "Acquiring cert…"); }
                RouterStatus::Starting  => { ui.colored_label(egui::Color32::YELLOW, "Starting…"); }
                RouterStatus::Running   => { ui.colored_label(egui::Color32::GREEN,  "Running"); }
                RouterStatus::Reloading => { ui.colored_label(egui::Color32::YELLOW, "Reloading ACL…"); }
                RouterStatus::Error(m)  => { ui.colored_label(egui::Color32::RED,    format!("Error: {m}")); }
            }
        });
    });

    egui::TopBottomPanel::bottom("log_panel")
        .resizable(true)
        .min_height(120.0)
        .show(ctx, |ui| {
            ui.label("Log");
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &st.log {
                        ui.monospace(line);
                    }
                });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Connected nodes");
        ui.add_space(6.0);
        if st.connected_nodes.is_empty() {
            ui.weak("Waiting for nodes to connect…");
        } else {
            let now = Instant::now();
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Sort CNs alphabetically for a stable display order between
                // repaints; egui repaints frequently and HashMap iteration
                // order would otherwise jitter.
                let mut cns: Vec<&String> = st.connected_nodes.keys().collect();
                cns.sort();
                for cn in cns {
                    let node = &st.connected_nodes[cn];
                    egui::CollapsingHeader::new(cn)
                        .default_open(true)
                        .show(ui, |ui| {
                            if node.agents.is_empty() {
                                ui.weak("(no agents reported)");
                                return;
                            }
                            let mut agent_names: Vec<&String> = node.agents.keys().collect();
                            agent_names.sort();
                            for name in agent_names {
                                let ag = &node.agents[name];
                                ui.horizontal(|ui| {
                                    match ag.status {
                                        AgentStatus::Online => {
                                            ui.colored_label(egui::Color32::GREEN, "●");
                                        }
                                        AgentStatus::Degraded => {
                                            ui.colored_label(egui::Color32::YELLOW, "●");
                                        }
                                        AgentStatus::Offline => {
                                            ui.colored_label(egui::Color32::GRAY, "●");
                                        }
                                    }
                                    ui.monospace(name);
                                    ui.weak(format!("v{}", ag.version));
                                    ui.weak("·");
                                    ui.weak(format_ago(now.saturating_duration_since(ag.last_seen)));
                                });
                                if !ag.key_expressions.is_empty() {
                                    ui.indent(format!("ke_{cn}_{name}"), |ui| {
                                        for ke in &ag.key_expressions {
                                            ui.monospace(format!("• {ke}"));
                                        }
                                    });
                                }
                                ui.add_space(2.0);
                            }
                        });
                }
            });
        }
    });
}

// ── Helpers ────────────────────────────────────────────────────────────────────

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.weak(label);
    ui.weak(value);
    ui.end_row();
}

/// Render a duration as a human-friendly "Ns ago" / "Nm Ms ago" string.
fn format_ago(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m {}s ago", secs / 60, secs % 60)
    } else {
        format!("{}h {}m ago", secs / 3600, (secs % 3600) / 60)
    }
}

