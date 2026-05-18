mod gui;
mod router;
mod state;

use std::sync::{Arc, Mutex};

use bot_framework::config::DverseConfig;
use gui::{RouterApp, Screen, SetupForm};
use state::AppState;

fn main() {
    // If a config already exists, pre-load it so the background thread can
    // start acquiring the cert immediately while the GUI is initialising.
    let existing_config: Option<DverseConfig> = DverseConfig::load().ok();

    let initial_screen = if existing_config.is_some() {
        Screen::Loading
    } else {
        Screen::Setup(SetupForm::default())
    };

    let state = Arc::new(Mutex::new(AppState::new(vec![], existing_config)));

    let bg_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(router::run(bg_state));
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("dverse router")
            .with_inner_size([720.0, 520.0]),
        ..Default::default()
    };

    eframe::run_native(
        "dverse router",
        options,
        Box::new(|_cc| Ok(Box::new(RouterApp::new(state, initial_screen)))),
    )
    .expect("eframe error");
}
