//! Tracing subscriber setup shared by every dverse binary.
//!
//! Historically this module installed its own [`tracing_subscriber`]
//! stack directly.  As the chat-app grew an OTLP collector +
//! Grafana/Jaeger backend, every binary started wanting the *same*
//! "stdout-always, OTLP-when-the-collector-is-configured" behaviour —
//! so the real implementation was lifted into the standalone
//! [`dverse_obs`] crate.  This module remains as a thin compatibility
//! shim so existing call sites
//!
//! ```text
//! bot_framework::logging::init();
//! ```
//!
//! continue to work without edits.
//!
//! Quiet by default; set `RUST_LOG` to drill in:
//!
//! ```text
//! RUST_LOG=zenoh=debug                    # chase a transport bug
//! RUST_LOG=info,zenoh::transport=trace    # everything info, plus the
//!                                         # noisy transport layer
//! ```
//!
//! To make logs queryable from Grafana, point the binary at the chat-app
//! OTel collector (see `experiments/chat-app/docker-compose.yml`):
//!
//! ```text
//! OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
//! OTEL_SERVICE_NAME=dverse-bot-framework \
//! cargo run -p bot-framework
//! ```
//!
//! When the endpoint env is unset the OTLP layer is skipped entirely —
//! no network calls, no overhead — and behaviour matches the original
//! stdout-only subscriber.

/// Default filter applied when `RUST_LOG` is unset.
///
/// Re-exported from [`dverse_obs::DEFAULT_FILTER`] so the router (which
/// installs its own subscriber to layer a GUI sink on top) can apply
/// exactly the same defaults without re-declaring them.
pub use dverse_obs::DEFAULT_FILTER;

/// Install the global tracing subscriber.  Idempotent: subsequent calls
/// are no-ops.  Call this once near the top of `main()` in every binary.
///
/// Delegates to [`dverse_obs::init`] under the default service name
/// `"bot-framework"`.  Binaries that want a more specific
/// `service.name` resource attribute should either set the
/// `OTEL_SERVICE_NAME` env var or call [`dverse_obs::init`] directly.
pub fn init() {
    dverse_obs::init("bot-framework");
}
