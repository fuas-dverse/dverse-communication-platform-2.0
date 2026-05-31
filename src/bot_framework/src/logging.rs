//! Tracing subscriber setup shared by every dverse binary.
//!
//! Without a subscriber installed, every `tracing::event!` call — including
//! everything Zenoh, rustls, and our own crates emit — is silently dropped.
//! That made silent failures like the inter-router mTLS handshake stalling
//! (because the dialer never presented a client cert) invisible until
//! someone read the code by hand.  This installs a sensible default so
//! anything `warn` or `error` reaches stderr automatically.
//!
//! Quiet by default; set `RUST_LOG` to drill in:
//!
//! ```text
//! RUST_LOG=zenoh=debug                    # chase a transport bug
//! RUST_LOG=info,zenoh::transport=trace    # everything info, plus the
//!                                         # noisy transport layer
//! ```

use std::sync::OnceLock;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Default filter applied when `RUST_LOG` isn't set.  Our own crates at
/// `info`; Zenoh, rustls, and the usual noisy transitives at `warn` so the
/// log doesn't flood at default verbosity but errors/warns still surface.
///
/// Public so the router (which installs its own subscriber to add the GUI
/// layer) can apply exactly the same defaults without re-declaring them.
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

/// Guards against double-install.  `tracing` only allows one global
/// subscriber per process and panics on the second attempt; we want
/// `init()` to be safe to call from any binary's `main` without coordination.
static INIT: OnceLock<()> = OnceLock::new();

/// Install the global tracing subscriber.  Idempotent: subsequent calls are
/// no-ops.  Call this once near the top of `main()` in every binary.
pub fn init() {
    INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILTER));

        // fmt layer writes to stderr by default — keeps stdout clean for
        // anything pipeline-like and matches the existing `eprintln!` UX
        // in `AppState::push_log`.
        let fmt_layer = fmt::layer()
            .with_target(true)
            .with_level(true)
            .compact();

        let _ = tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .try_init();
    });
}
