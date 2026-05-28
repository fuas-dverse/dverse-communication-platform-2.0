mod constants;
mod discovery;
mod gui;
mod router;
mod state;

use std::sync::{Arc, Mutex};

use clap::Parser;
use bot_framework::config::DverseConfig;
use gui::{LoginForm, RouterApp, Screen};
use state::AppState;

#[derive(Parser)]
#[command(name = "zenoh-router", about = "dverse Zenoh router with admission GUI")]
struct Cli {
    /// Run a local demo without certs or Keycloak.
    #[arg(long)]
    demo: bool,
}

fn main() {
    let cli = Cli::parse();
    if cli.demo { run_demo(); } else { run_normal(); }
}

fn run_normal() {
    let existing_config: Option<DverseConfig> = DverseConfig::load().ok();
    let initial_screen = if existing_config.is_some() {
        Screen::Loading
    } else {
        Screen::Login(LoginForm::default())
    };
    let state = Arc::new(Mutex::new(AppState::new(existing_config)));
    let bg_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(router::run(bg_state));
    });
    launch_gui(state, initial_screen);
}

fn run_demo() {
    let mut state = AppState::new(None);
    state.admitted = vec!["alice".into(), "mybot".into()];
    state.router_status = state::RouterStatus::Running;
    state.session_id = "alice".into();
    state.push_log("[demo] Router started on tcp/0.0.0.0:7447");
    state.push_log("[demo] Session admin: alice");
    state.push_log("[demo] Auto-admitted: alice");
    state.push_log("[demo] Auto-admitted: mybot");
    launch_gui(Arc::new(Mutex::new(state)), Screen::Main);
}

fn launch_gui(state: Arc<Mutex<AppState>>, screen: Screen) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("dverse router")
            .with_inner_size([720.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "dverse router",
        options,
        Box::new(|_cc| Ok(Box::new(RouterApp::new(state, screen)))),
    )
    .expect("eframe error");
}
