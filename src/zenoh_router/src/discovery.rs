//! DNS-SD peer discovery for the Zenoh router.
//!
//! Public entry point: [`MdnsHandle::publish`].  Drop the handle to withdraw
//! the service record and stop browsing.
//!
//! Backend selection:
//!   * Linux:   try avahi D-Bus (publish + browse) so avahi-daemon is the
//!              single mDNS responder on the host.  Fall back to mdns-sd if
//!              the system bus or avahi is unavailable.
//!   * Windows / macOS:  mdns-sd for both publish and browse — one
//!              `ServiceDaemon` to avoid UDP-5353 socket conflicts.
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

use self::common::{create_filtered_daemon, detect_lan_ipv4, instance_name, SERVICE_TYPE};

/// Discovery handle.  Holds backend resources alive (D-Bus connections,
/// mdns-sd daemon) and exposes a `peer_rx` watch channel listing the current
/// peer endpoints as `tls/<ip>:<port>` strings ready for Zenoh's
/// `connect/endpoints`.
pub struct MdnsHandle {
    _inner: Inner,
    pub peer_rx: watch::Receiver<Vec<String>>,
}

impl MdnsHandle {
    pub fn publish(
        cn: &str,
        session_id: &str,
        port: u16,
        state: &Arc<Mutex<AppState>>,
    ) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            state.lock().unwrap().push_log("mDNS: trying avahi D-Bus backend…".to_string());
            if let Some(handle) = linux_avahi_start(cn, session_id, port, Arc::clone(state)) {
                return Some(handle);
            }
            state.lock().unwrap().push_log(
                "mDNS: avahi unavailable, falling back to mdns-sd".to_string(),
            );
        }
        #[cfg(not(target_os = "linux"))]
        state.lock().unwrap().push_log("mDNS: using mdns-sd backend".to_string());
        mdns_sd_start(cn, session_id, port, state)
    }
}

#[allow(dead_code)]
enum Inner {
    #[cfg(target_os = "linux")]
    Avahi {
        _publish_conn: zbus::blocking::Connection,
        _browse_conn: zbus::blocking::Connection,
    },
    MdnsSd(MdnsSdSession),
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

// ── Linux: avahi D-Bus for publish AND browse ────────────────────────────────

#[cfg(target_os = "linux")]
fn linux_avahi_start(
    cn: &str,
    session_id: &str,
    port: u16,
    state: Arc<Mutex<AppState>>,
) -> Option<MdnsHandle> {
    use zbus::blocking::Connection;

    let publish_conn = match Connection::system() {
        Ok(c) => c,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[avahi]: D-Bus system bus unavailable ({e})"
            ));
            return None;
        }
    };

    if let Err(e) = announcing::avahi_register(cn, session_id, port, &publish_conn) {
        state.lock().unwrap().push_log(format!("mDNS[avahi]: publish failed ({e})"));
        return None;
    }
    state.lock().unwrap().push_log(format!(
        "mDNS[avahi]: published {} on {SERVICE_TYPE} port {port} addr={} session={session_id}",
        instance_name(cn),
        detect_lan_ipv4()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    ));

    match browsing::avahi_start(cn, session_id, Arc::clone(&state)) {
        Some((browse_conn, peer_rx)) => {
            state.lock().unwrap().push_log("mDNS[avahi]: browse started".to_string());
            Some(MdnsHandle {
                _inner: Inner::Avahi {
                    _publish_conn: publish_conn,
                    _browse_conn: browse_conn,
                },
                peer_rx,
            })
        }
        None => {
            state.lock().unwrap().push_log("mDNS[avahi]: browse failed".to_string());
            None
        }
    }
}

// ── Cross-platform: one mdns-sd daemon for both publish and browse ───────────

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
        _inner: Inner::MdnsSd(MdnsSdSession { daemon, fullname }),
        peer_rx,
    })
}
