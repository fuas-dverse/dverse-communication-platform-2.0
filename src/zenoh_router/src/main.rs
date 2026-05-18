mod constants;
mod gui;
mod router;
mod state;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use clap::Parser;
use bot_framework::config::DverseConfig;
use gui::{RouterApp, Screen, SetupForm};
use state::AppState;

#[derive(Parser)]
#[command(name = "zenoh-router", about = "dverse Zenoh router with admission GUI")]
struct Cli {
    /// Run a local demo with fake pending nodes (no certs or Keycloak required).
    #[arg(long)]
    demo: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.demo {
        run_demo();
    } else {
        run_normal();
    }
}

// ── Normal mode ───────────────────────────────────────────────────────────────

fn run_normal() {
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

    launch_gui(state, initial_screen);
}

// ── Demo mode ─────────────────────────────────────────────────────────────────

fn run_demo() {
    let mut state = AppState::new(vec!["alice".into()], None);

    // Seed fake pending nodes so every panel is populated.
    state.pending.insert("mybot".into(), Instant::now());
    state.pending.insert("data-collector".into(), Instant::now());
    state.denied.push("rogue-agent".into());
    state.router_status = state::RouterStatus::Running;
    state.push_log("[demo] Router started on tcp/0.0.0.0:7447");
    state.push_log("[demo] Node announced: mybot");
    state.push_log("[demo] Node announced: data-collector");
    state.push_log("[demo] Admitted on startup: alice");

    let state = Arc::new(Mutex::new(state));

    // No background thread — state is purely static for the demo.
    launch_gui(state, Screen::Main);
}

// ── Shared launcher ───────────────────────────────────────────────────────────

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
