use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use bot_framework::config::DverseConfig;

use crate::constants::{CA_URL, CLIENT_ID, CLIENT_SECRET, KEYCLOAK_REALM, KEYCLOAK_URL, ROUTER_LISTEN};
use crate::state::{Action, AppState, RouterStatus};

// ── Screen state (lives on the GUI thread only) ────────────────────────────────

pub enum Screen {
    Setup(SetupForm),
    /// Config submitted; waiting for the background thread to become Running.
    Loading,
    Main,
}

pub struct SetupForm {
    pub username: String,
    pub password: String,
    pub error: Option<String>,
}

impl Default for SetupForm {
    fn default() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            error: None,
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

        // Watch for background-thread transitions.
        let status = self.state.lock().unwrap().router_status.clone();

        match &self.screen {
            Screen::Setup(_) => {
                // If the background thread just received staged_config, switch to Loading.
                // (staged_config is consumed by the background thread, so we detect the
                //  transition via router_status leaving Idle.)
                if !matches!(status, RouterStatus::Idle) {
                    self.screen = Screen::Loading;
                }
            }
            Screen::Loading => match &status {
                RouterStatus::Running => self.screen = Screen::Main,
                RouterStatus::Error(msg) => {
                    let error = Some(msg.clone());
                    self.screen = Screen::Setup(SetupForm { error, ..SetupForm::default() });
                }
                _ => {}
            },
            Screen::Main => {}
        }

        match &mut self.screen {
            Screen::Setup(form) => show_setup(ctx, &mut self.state, form),
            Screen::Loading => show_loading(ctx, &self.state),
            Screen::Main => show_main(ctx, &mut self.state),
        }
    }
}

// ── Setup screen ───────────────────────────────────────────────────────────────

fn show_setup(ctx: &egui::Context, state: &mut Arc<Mutex<AppState>>, form: &mut SetupForm) {
    egui::CentralPanel::default().show(ctx, |ui| {
        // Centre the card vertically.
        let top_pad = (ui.available_height() - 280.0).max(0.0) / 2.0;
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
                    ui.add(
                        egui::TextEdit::singleline(&mut form.username)
                            .hint_text("you@dverse.yordanmitev.me")
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
                });

            ui.add_space(16.0);

            if let Some(err) = &form.error {
                ui.colored_label(egui::Color32::RED, err);
                ui.add_space(8.0);
            }

            if ui.button("  Connect  ").clicked() {
                match build_and_submit(form, state) {
                    Ok(()) => form.error = None,
                    Err(e) => form.error = Some(e),
                }
            }

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
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.weak(label);
    ui.weak(value);
    ui.end_row();
}

fn build_and_submit(form: &SetupForm, state: &Arc<Mutex<AppState>>) -> Result<(), String> {
    if form.username.is_empty() {
        return Err("Username is required.".into());
    }
    if form.password.is_empty() {
        return Err("Password is required.".into());
    }

    let cert_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("dverse")
        .join("certs");

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
    };

    cfg.save().map_err(|e| e.to_string())?;

    let mut st = state.lock().unwrap();
    st.push_log(format!("Signed in as {}. Bootstrapping…", cfg.username));
    st.staged_config = Some(cfg);

    Ok(())
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
    let mut st = state.lock().unwrap();

    egui::TopBottomPanel::top("status_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Router:");
            match &st.router_status {
                RouterStatus::Idle => { ui.label("Idle"); }
                RouterStatus::Acquiring => { ui.colored_label(egui::Color32::YELLOW, "Acquiring cert…"); }
                RouterStatus::Starting => { ui.colored_label(egui::Color32::YELLOW, "Starting…"); }
                RouterStatus::Running => { ui.colored_label(egui::Color32::GREEN, "Running"); }
                RouterStatus::Reloading => { ui.colored_label(egui::Color32::YELLOW, "Reloading ACL…"); }
                RouterStatus::Error(msg) => { ui.colored_label(egui::Color32::RED, format!("Error: {msg}")); }
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
        ui.columns(2, |cols| {
            cols[0].heading("Pending");
            let pending_cns: Vec<String> = st.pending.keys().cloned().collect();
            if pending_cns.is_empty() {
                cols[0].label("(none)");
            } else {
                let mut admit_cn: Option<String> = None;
                let mut deny_cn: Option<String> = None;
                for cn in &pending_cns {
                    cols[0].horizontal(|ui| {
                        ui.label(cn);
                        if ui.button("Admit").clicked() {
                            admit_cn = Some(cn.clone());
                        }
                        if ui.button("Deny").clicked() {
                            deny_cn = Some(cn.clone());
                        }
                    });
                }
                if let Some(cn) = admit_cn {
                    st.action_queue.push(Action::Admit(cn));
                }
                if let Some(cn) = deny_cn {
                    st.action_queue.push(Action::Deny(cn));
                }
            }

            cols[1].heading("Admitted");
            if st.admitted.is_empty() {
                cols[1].label("(none)");
            } else {
                for cn in &st.admitted {
                    cols[1].label(cn);
                }
            }

            cols[1].add_space(12.0);
            cols[1].heading("Denied");
            if st.denied.is_empty() {
                cols[1].label("(none)");
            } else {
                for cn in &st.denied {
                    cols[1].colored_label(egui::Color32::LIGHT_RED, cn);
                }
            }
        });
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────────

