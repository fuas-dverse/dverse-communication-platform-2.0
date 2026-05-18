mod gui;
mod router;
mod state;

use std::sync::{Arc, Mutex};

use clap::Parser;

use router::RouterConfig;
use state::AppState;

#[derive(Parser)]
#[command(name = "zenoh-router", about = "Zenoh mTLS router with admission GUI")]
struct Cli {
    /// Zenoh listen endpoint, e.g. "tls/0.0.0.0:7447"
    #[arg(long, default_value = "tls/0.0.0.0:7447")]
    listen: String,

    /// Path to the CA root PEM (Step-CA root certificate)
    #[arg(long)]
    tls_ca: Option<String>,

    /// Path to the router's certificate PEM (issued by Step-CA)
    #[arg(long)]
    tls_cert: Option<String>,

    /// Path to the router's private key PEM
    #[arg(long)]
    tls_key: Option<String>,

    /// CNs that are pre-admitted without operator approval (comma-separated)
    #[arg(long, value_delimiter = ',', default_value = "")]
    pre_admitted: Vec<String>,
}

fn main() {
    let cli = Cli::parse();

    let pre_admitted: Vec<String> = cli
        .pre_admitted
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();

    let state = Arc::new(Mutex::new(AppState::new(pre_admitted)));

    let router_cfg = RouterConfig {
        listen_addr: cli.listen,
        tls_ca: cli.tls_ca,
        tls_cert: cli.tls_cert,
        tls_key: cli.tls_key,
    };

    // Spawn the Zenoh router on a background thread with its own tokio runtime.
    let bg_state = Arc::clone(&state);
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(router::run(router_cfg, bg_state));
    });

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("dverse router")
            .with_inner_size([700.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        "dverse router",
        options,
        Box::new(|_cc| Ok(Box::new(gui::RouterApp::new(state)))),
    )
    .expect("eframe error");
}
