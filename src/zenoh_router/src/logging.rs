//! Router-side tracing setup.
//!
//! The router has a GUI log panel that needs to display whatever the rest of
//! the codebase logs.  Before this module the panel was fed by direct
//! `AppState::push_log` calls scattered across every file; now all those call
//! sites use `tracing::info!` etc., and a custom [`GuiLogLayer`] subscribed
//! to the global subscriber routes each event into the same `AppState.log`
//! buffer the GUI already reads.
//!
//! Two layers stack on the subscriber:
//!   * `tracing_subscriber::fmt::layer()` — stderr, full level / target /
//!     fields; what you see in the terminal you ran `cargo run` in.
//!   * [`GuiLogLayer`] — append a compact rendered line to `AppState.log`;
//!     what shows up in the bottom panel of the desktop window.
//!
//! Both layers receive every event that passes the env filter, so the same
//! line shows up in stderr AND in the GUI without per-call-site duplication.

use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter, Layer};

use crate::state::AppState;

/// Default RUST_LOG when the env var isn't set.  Mirrors
/// `bot_framework::logging::init`'s defaults — kept in sync intentionally so
/// the router and the CLI agents log at the same verbosity by default.
const DEFAULT_FILTER: &str = concat!(
    "info,",
    "zenoh=warn,",
    "zenoh_runtime=warn,",
    "rustls=warn,",
    "mio=warn,",
    "hyper=warn,",
    "reqwest=warn,",
    "zbus=warn,",
);

/// Install the global tracing subscriber.  Must be called once, early, from
/// the router binary's `main` — `bot_framework::logging::init` is NOT used
/// here because we want the extra GUI layer.
pub fn init_with_gui_sink(state: Arc<Mutex<AppState>>) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

    let fmt_layer = fmt::layer().with_target(true).with_level(true).compact();
    let gui_layer = GuiLogLayer { state };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(gui_layer)
        .try_init();
}

/// A `tracing` Layer that renders each event to a single line and appends it
/// to `AppState.log`.
///
/// Uses `try_lock` so a tracing call from inside a code path that already
/// holds the AppState mutex degrades to "log this one to stderr only" instead
/// of deadlocking.  In practice the migration is structured so this never
/// happens, but the fallback keeps the program correct under contention.
pub struct GuiLogLayer {
    state: Arc<Mutex<AppState>>,
}

impl<S> Layer<S> for GuiLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let level = event.metadata().level();
        let mut line = String::new();
        // Prefix warn/error so they're scannable in the GUI even without colour.
        match *level {
            tracing::Level::WARN => line.push_str("[WARN] "),
            tracing::Level::ERROR => line.push_str("[ERROR] "),
            _ => {}
        }
        line.push_str(&visitor.message);
        for f in &visitor.fields {
            line.push(' ');
            line.push_str(f);
        }

        if let Ok(mut s) = self.state.try_lock() {
            s.push_log(line);
        }
        // If we couldn't lock right now, the fmt layer still wrote this to
        // stderr; we lose it from the GUI panel but the program proceeds.
    }
}

/// Captures the event's message (the literal format string) and every
/// structured field.  Rendered as `"message k=v k=v"`.
#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: Vec<String>,
}

impl MessageVisitor {
    fn put(&mut self, name: &str, value: &str) {
        if name == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{name}={value}"));
        }
    }
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field.name(), value);
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field.name(), &format!("{value:?}"));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), &value.to_string());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), &value.to_string());
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field.name(), &value.to_string());
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), &value.to_string());
    }
}
