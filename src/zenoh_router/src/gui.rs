use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use bot_framework::config::DverseConfig;

use crate::state::{Action, AppState, RouterStatus};

// ── Screen state (lives on the GUI thread only) ────────────────────────────────

pub enum Screen {
    Setup(SetupForm),
    /// Config submitted; waiting for the background thread to become Running.
    Loading,
    Main,
}

pub struct SetupForm {
    pub cfg: DverseConfig,
    pub error: Option<String>,
}

impl Default for SetupForm {
    fn default() -> Self {
        Self {
            cfg: DverseConfig::default(),
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
        ui.heading("dverse — first-time setup");
        ui.add_space(8.0);

        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("setup_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    section_header(ui, "Account");

                    row(ui, "Username (email)", |ui| {
                        ui.text_edit_singleline(&mut form.cfg.username);
                    });
                    row(ui, "Password", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut form.cfg.password).password(true));
                    });

                    section_header(ui, "Keycloak");

                    row(ui, "Keycloak URL", |ui| {
                        ui.text_edit_singleline(&mut form.cfg.keycloak_url);
                    });
                    row(ui, "Realm", |ui| {
                        ui.text_edit_singleline(&mut form.cfg.keycloak_realm);
                    });
                    row(ui, "Client ID", |ui| {
                        ui.text_edit_singleline(&mut form.cfg.client_id);
                    });
                    row(ui, "Client Secret", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut form.cfg.client_secret).password(true));
                    });

                    section_header(ui, "Step-CA");

                    row(ui, "CA URL", |ui| {
                        ui.text_edit_singleline(&mut form.cfg.ca_url);
                    });
                    row(ui, "CA Root PEM path", |ui| {
                        ui.text_edit_singleline(&mut form.cfg.ca_root_pem_path);
                    });

                    section_header(ui, "Router");

                    row(ui, "Listen address", |ui| {
                        ui.text_edit_singleline(&mut form.cfg.router_listen);
                    });
                    row(ui, "Cert cache dir", |ui| {
                        let mut s = form.cfg.cert_dir.to_string_lossy().into_owned();
                        if ui.text_edit_singleline(&mut s).changed() {
                            form.cfg.cert_dir = s.into();
                        }
                    });
                });

            ui.add_space(12.0);

            if let Some(err) = &form.error {
                ui.colored_label(egui::Color32::RED, err);
                ui.add_space(6.0);
            }

            if ui.button("Save & Connect").clicked() {
                match validate_and_submit(&form.cfg, state) {
                    Ok(()) => {
                        // Transition handled by the caller via Screen::Loading.
                        // We signal the parent by setting a sentinel on form.error.
                        form.error = None;
                    }
                    Err(e) => form.error = Some(e),
                }
            }
        });
    });

    // If submission succeeded, switch the screen from the outside.
    // We detect success by checking staged_config was just set.
    if state.lock().unwrap().staged_config.is_some() {
        // This won't actually run here — the borrow of `self.screen` prevents it.
        // The caller (RouterApp::update) handles the screen switch after this fn returns.
        // We leave a marker so the update loop sees it via router_status.
    }
}

fn validate_and_submit(cfg: &DverseConfig, state: &Arc<Mutex<AppState>>) -> Result<(), String> {
    if cfg.username.is_empty() {
        return Err("Username is required.".into());
    }
    if cfg.password.is_empty() {
        return Err("Password is required.".into());
    }
    if cfg.client_secret.is_empty() {
        return Err("Client secret is required.".into());
    }
    if cfg.ca_root_pem_path.is_empty() {
        return Err("CA root PEM path is required.".into());
    }
    if !std::path::Path::new(&cfg.ca_root_pem_path).exists() {
        return Err(format!("CA root PEM not found: {}", cfg.ca_root_pem_path));
    }

    cfg.save().map_err(|e| e.to_string())?;

    let mut st = state.lock().unwrap();
    st.push_log(format!("Config saved for {}. Acquiring certificate…", cfg.username));
    st.staged_config = Some(cfg.clone());

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

fn section_header(ui: &mut egui::Ui, label: &str) {
    ui.strong(label);
    ui.end_row();
}

fn row(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    ui.label(label);
    content(ui);
    ui.end_row();
}
