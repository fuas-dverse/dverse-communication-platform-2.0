//! Shared structured-logging bootstrap for every dverse Rust binary.
//!
//! # Why this exists
//!
//! Every dverse service used to bring its own ad-hoc `tracing` setup (when
//! it had one at all).  As soon as we added a Grafana/Jaeger stack to the
//! chat-app, "make my service's logs queryable from Grafana" became a
//! repeated chore — and the Python backend already standardised on OTLP
//! to the in-cluster collector at `OTEL_EXPORTER_OTLP_ENDPOINT`.  This
//! crate gives the Rust side the same single-call wiring.
//!
//! # What [`init`] does
//!
//! 1. Always installs a `tracing-subscriber` stack with:
//!    * an `EnvFilter` honouring `RUST_LOG`, falling back to
//!      [`DEFAULT_FILTER`] (our crates at `info`, the noisy transitives
//!      at `warn`; see the constant doc for the full list);
//!    * a compact `fmt` layer writing to stderr so engineers running
//!      `cargo run` still see logs locally.
//!
//! 2. Attaches an OTLP logs + traces exporter pointed at the resolved
//!    endpoint (see "OTLP endpoint resolution" below):
//!    * an [`opentelemetry_appender_tracing`] layer that forwards every
//!      `tracing` event to an OTLP logs exporter;
//!    * a [`tracing_opentelemetry`] layer that exports spans over OTLP.
//!
//!    The resource attribute `service.name` defaults to the value passed
//!    into [`init`], overridable via `OTEL_SERVICE_NAME` (matching the
//!    convention used by `experiments/chat-app/backend/telemetry.py`).
//!
//! Stdout is always live; OTLP is layered on top.  If the resolved
//! collector endpoint is unreachable the exporter silently retries in
//! the background and the host process is unaffected.
//!
//! # OTLP endpoint resolution
//!
//! With no env var set, the endpoint comes from the profile-baked
//! [`DEFAULT_OTLP_ENDPOINT`]:
//!
//! * Debug builds (`cargo run`, `cargo build`, every workspace test)
//!   point at `http://localhost:4317`, which is where the chat-app
//!   OTel collector listens when its docker-compose stack is up.
//! * Release builds (`cargo build --release`) point at
//!   `https://logs.dverse.yordanmitev.me:4317`, the central dverse
//!   sink, so any binary shipped from this workspace lands in the
//!   production stream by default.
//!
//! Override with `OTEL_EXPORTER_OTLP_ENDPOINT=<url>`.  Opt out entirely
//! by setting it to the empty string, useful in CI runs or on
//! air-gapped boxes where the periodic connection-refused warnings
//! would be noise.
//!
//! # Idempotency
//!
//! `tracing` allows only one global subscriber per process and panics on
//! the second install attempt.  [`init`] is wrapped in a [`OnceLock`] so
//! it is safe to call from any binary's `main` without coordination, and
//! safe to call from library tests that happen to share a process.
//!
//! # Runtime requirements
//!
//! The OTLP exporter uses Tokio under the hood
//! ([`opentelemetry_sdk::runtime::Tokio`]).  Call [`init`] from inside
//! `#[tokio::main]` (or any tokio context) whenever the OTLP branch is
//! active, which is the default.  Pass
//! `OTEL_EXPORTER_OTLP_ENDPOINT=""` if you need to call [`init`] from a
//! non-tokio context.

use std::sync::OnceLock;

use opentelemetry::{trace::TracerProvider as _, KeyValue};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    logs::LoggerProvider, runtime, trace::TracerProvider as SdkTracerProvider, Resource,
};
use tracing_subscriber::{
    fmt, layer::SubscriberExt, registry::Registry, util::SubscriberInitExt, EnvFilter, Layer,
};

/// Default filter applied when `RUST_LOG` is unset.
///
/// Our own crates surface at `info`; Zenoh, rustls, and the loudest
/// transitive dependencies are clamped to `warn` so the log doesn't
/// flood at default verbosity but errors/warnings still surface.
///
/// Made public so a binary that wants to layer something extra on top
/// (e.g. the Tauri launcher's in-GUI log sink) can reuse the exact
/// same defaults rather than re-declaring them and drifting.
pub const DEFAULT_FILTER: &str = concat!(
    "info,",
    "zenoh=warn,",
    "zenoh_runtime=warn,",
    "rustls=warn,",
    "mio=warn,",
    "hyper=warn,",
    "reqwest=warn,",
    "zbus=warn,",
);

/// Env var overriding the OTLP endpoint.  Setting it to a URL points
/// the exporter at that endpoint; setting it to the empty string is an
/// explicit opt-out (skip the OTLP branch entirely, useful in CI and
/// air-gapped tests).  Leaving it unset falls through to
/// [`DEFAULT_OTLP_ENDPOINT`].
const OTLP_ENDPOINT_ENV: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// Built-in fallback for the OTLP endpoint when
/// [`OTLP_ENDPOINT_ENV`] is unset.  Debug builds (`cargo run`,
/// `cargo build`, every workspace test) point at the chat-app collector
/// on localhost; release builds (`cargo build --release`) point at the
/// central dverse sink so any binary shipped from this workspace ends
/// up in the production stream by default.  Either default is
/// override-able via the env var.
#[cfg(debug_assertions)]
pub const DEFAULT_OTLP_ENDPOINT: &str = "http://localhost:4317";
#[cfg(not(debug_assertions))]
pub const DEFAULT_OTLP_ENDPOINT: &str = "https://logs.dverse.yordanmitev.me:4317";

/// Env var override for the `service.name` resource attribute.  Falls
/// back to the value passed into [`init`] when unset.
const SERVICE_NAME_ENV: &str = "OTEL_SERVICE_NAME";

/// One-shot guard so [`init`] is safe to call from any number of crates
/// in the same process without panicking the global subscriber install.
static INIT: OnceLock<()> = OnceLock::new();

/// Install the global tracing subscriber.
///
/// `service_name` is the default value for the `service.name` OTel
/// resource attribute when OTLP export is enabled — typically the binary
/// name (e.g. `"dverse-ping"`).  `OTEL_SERVICE_NAME` overrides it.
///
/// Idempotent: a second call from the same process is a no-op.
pub fn init(service_name: &str) {
    init_with_extra_layer(service_name, NoopLayer);
}

/// Like [`init`], but also installs an additional `tracing-subscriber`
/// [`Layer`] in front of the standard fmt + OTLP stack.
///
/// Used by the router, which needs its `GuiLogLayer` to feed the desktop
/// log panel from the same global subscriber that exports OTLP — and
/// can't just call [`init`] separately because `tracing` allows exactly
/// one global subscriber per process.
///
/// The extra layer is added at the bottom of the stack (closest to the
/// underlying `Registry`), so it sees every event before the filter
/// drops or the formatter renders.
pub fn init_with_extra_layer<L>(service_name: &str, extra: L)
where
    L: Layer<Registry> + Send + Sync + 'static,
{
    INIT.get_or_init(|| {
        let extra_boxed: Box<dyn Layer<Registry> + Send + Sync> = Box::new(extra);
        let make_filter = || {
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER))
        };
        let make_fmt = || fmt::layer().with_target(true).with_level(true).compact();

        // OTLP export is on by default; the endpoint is resolved from
        // the env var (with empty-string opt-out) or the profile-baked
        // [`DEFAULT_OTLP_ENDPOINT`].  Provider construction is split
        // from subscriber install so the fallback to stdout-only
        // doesn't have to clone or re-build the extra layer; the layer
        // is moved into exactly one branch below.
        let providers = match resolve_otlp_endpoint() {
            Some(endpoint) => match build_otlp_providers(service_name, &endpoint) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!(
                        "[dverse-obs] OTLP exporter init failed for {endpoint}; \
                         continuing with stdout-only logging: {e}"
                    );
                    None
                }
            },
            None => None,
        };

        match providers {
            Some((log_provider, trace_provider)) => {
                let logs_layer = OpenTelemetryTracingBridge::new(&log_provider);
                let tracer = trace_provider.tracer("dverse");
                let traces_layer = tracing_opentelemetry::layer().with_tracer(tracer);
                let _ = tracing_subscriber::registry()
                    .with(extra_boxed)
                    .with(make_filter())
                    .with(make_fmt())
                    .with(logs_layer)
                    .with(traces_layer)
                    .try_init();
            }
            None => {
                let _ = tracing_subscriber::registry()
                    .with(extra_boxed)
                    .with(make_filter())
                    .with(make_fmt())
                    .try_init();
            }
        }
    });
}

/// Resolve the OTLP endpoint from the environment, applying the
/// profile-baked default when the env var is unset.  Returns `None`
/// when the user explicitly opts out by setting
/// [`OTLP_ENDPOINT_ENV`] to the empty string.
fn resolve_otlp_endpoint() -> Option<String> {
    match std::env::var(OTLP_ENDPOINT_ENV) {
        Ok(s) if s.is_empty() => None,
        Ok(s) => Some(s),
        Err(_) => Some(DEFAULT_OTLP_ENDPOINT.to_string()),
    }
}

/// Build the OTLP logs + traces providers against `endpoint`.  Kept
/// separate from subscriber install so [`init_with_extra_layer`] can
/// decide between the OTLP-on and OTLP-off subscriber stacks without
/// duplicating the extra-layer plumbing.
fn build_otlp_providers(
    service_name: &str,
    endpoint: &str,
) -> Result<(LoggerProvider, SdkTracerProvider), Box<dyn std::error::Error + Send + Sync>> {
    let resolved_name =
        std::env::var(SERVICE_NAME_ENV).unwrap_or_else(|_| service_name.to_string());
    let resource = Resource::new(vec![KeyValue::new("service.name", resolved_name)]);

    let log_exporter = opentelemetry_otlp::LogExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("log exporter: {e}").into()
        })?;
    let log_provider = LoggerProvider::builder()
        .with_resource(resource.clone())
        .with_batch_exporter(log_exporter, runtime::Tokio)
        .build();

    let span_exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("span exporter: {e}").into()
        })?;
    let trace_provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(span_exporter, runtime::Tokio)
        .build();

    Ok((log_provider, trace_provider))
}

/// No-op layer used by [`init`] so it can share the install path with
/// [`init_with_extra_layer`] without callers having to pass anything.
struct NoopLayer;
impl<S: tracing::Subscriber> Layer<S> for NoopLayer {}
