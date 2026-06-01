//! DNS-SD peer discovery for the Zenoh router.
//!
//! Public entry point: [`MdnsHandle::publish`].  Drop the handle to withdraw
//! the service record and stop browsing.
//!
//! Internals are split across three sibling modules:
//!   * [`common`]     — shared constants, format helpers, IP detection,
//!                      mdns-sd daemon setup, peer registry.
//!   * [`announcing`] — registers our service.
//!   * [`browsing`]   — watches for other services and tracks endpoints.

mod announcing;
mod browsing;
mod common;

use mdns_sd::ServiceDaemon;
use tokio::sync::watch;
use tracing::info;

use self::common::create_filtered_daemon;

/// Discovery handle.  Holds the mdns-sd daemon alive and exposes a `peer_rx`
/// watch channel listing the current peer endpoints as `tls/<ip>:<port>`
/// strings, ready for Zenoh's `connect/endpoints`.
pub struct MdnsHandle {
    _inner: MdnsSdSession,
    pub peer_rx: watch::Receiver<Vec<String>>,
    /// Admin CNs of sessions visible on the LAN (for the session chooser).
    pub sessions_rx: watch::Receiver<Vec<String>>,
}

impl MdnsHandle {
    pub fn publish(cn: &str, session_id: &str, port: u16) -> Option<Self> {
        info!("mDNS: using mdns-sd backend");
        mdns_sd_start(cn, session_id, port)
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

fn mdns_sd_start(cn: &str, session_id: &str, port: u16) -> Option<MdnsHandle> {
    let daemon = create_filtered_daemon()?;
    let fullname = announcing::mdns_sd_register(&daemon, cn, session_id, port)?;
    let (peer_rx, sessions_rx) = browsing::mdns_sd_start(&daemon, cn, session_id)?;
    Some(MdnsHandle {
        _inner: MdnsSdSession { daemon, fullname },
        peer_rx,
        sessions_rx,
    })
}
