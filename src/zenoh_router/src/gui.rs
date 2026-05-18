use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::state::{Action, AppState, RouterStatus};

pub struct RouterApp {
    state: Arc<Mutex<AppState>>,
}

impl RouterApp {
    pub fn new(state: Arc<Mutex<AppState>>) -> Self {
        Self { state }
    }
}

impl eframe::App for RouterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keep the GUI updating so we see live changes from the background thread.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        let mut st = self.state.lock().unwrap();

        egui::TopBottomPanel::top("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Router:");
                match &st.router_status {
                    RouterStatus::Starting => {
                        ui.colored_label(egui::Color32::YELLOW, "Starting…");
                    }
                    RouterStatus::Running => {
                        ui.colored_label(egui::Color32::GREEN, "Running");
                    }
                    RouterStatus::Reloading => {
                        ui.colored_label(egui::Color32::YELLOW, "Reloading ACL…");
                    }
                    RouterStatus::Error(msg) => {
                        ui.colored_label(egui::Color32::RED, format!("Error: {msg}"));
                    }
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
                // Left column: pending nodes.
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

                // Right column: admitted and denied nodes.
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
}
