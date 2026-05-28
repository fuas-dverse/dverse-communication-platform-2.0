//! DNS-SD peer discovery for the Zenoh router.
//!
//! Public entry point: [`MdnsHandle::publish`].  Drop the handle to withdraw
//! the service record and stop browsing.
//!
//! Backend: one `mdns_sd::ServiceDaemon` per router process, used for both
//! publishing and browsing.  We previously had a Linux-specific avahi D-Bus
//! path but removed it after repeated cross-platform flakiness — Chromium's
//! bundled mDNS responder, the 0.9-rc avahi-daemon's conflict-detection
//! regression, and the avahi-vs-cert SAN hostname disagreements all
//! conspired to make it unusable.  mdns-sd works the same on every platform
//! and runs entirely in-process.
//!
//! Internals are split across three sibling modules:
//!   * [`common`]     — shared constants, format helpers, IP detection,
//!                      mdns-sd daemon setup, peer registry.
//!   * [`announcing`] — registers our service.
//!   * [`browsing`]   — watches for other services and tracks endpoints.

mod announcing;
mod browsing;
mod common;

use std::sync::{Arc, Mutex};

use mdns_sd::ServiceDaemon;
use tokio::sync::watch;

use crate::state::AppState;

use self::common::create_filtered_daemon;

/// Discovery handle.  Holds the mdns-sd daemon alive and exposes a `peer_rx`
/// watch channel listing the current peer endpoints as `tls/<ip>:<port>`
/// strings, ready for Zenoh's `connect/endpoints`.
pub struct MdnsHandle {
    _inner: MdnsSdSession,
    pub peer_rx: watch::Receiver<Vec<String>>,
}

impl MdnsHandle {
    pub fn publish(
        cn: &str,
        session_id: &str,
        port: u16,
        state: &Arc<Mutex<AppState>>,
    ) -> Option<Self> {
        state.lock().unwrap().push_log("mDNS: using mdns-sd backend".to_string());
        mdns_sd_start(cn, session_id, port, state)
    }
}

/// Owns the mdns-sd daemon + the registered service fullname so we can
/// unregister cleanly on drop.
struct MdnsSdSession {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Drop for MdnsSdSession {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

fn mdns_sd_start(
    cn: &str,
    session_id: &str,
    port: u16,
    state: &Arc<Mutex<AppState>>,
) -> Option<MdnsHandle> {
    let daemon = create_filtered_daemon(state)?;
    let fullname = announcing::mdns_sd_register(&daemon, cn, session_id, port, state)?;
    let peer_rx = browsing::mdns_sd_start(&daemon, cn, session_id, state)?;
    Some(MdnsHandle {
        _inner: MdnsSdSession { daemon, fullname },
        peer_rx,
    })
}
