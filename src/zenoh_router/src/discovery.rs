use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::sync::{Arc, Mutex};

use crate::state::AppState;

const SERVICE_TYPE: &str = "_dverse._tcp.local.";

/// Registers this router as a `_dverse._tcp` DNS-SD service.
///
/// On Linux: uses the avahi D-Bus API so avahi-daemon owns the mDNS socket.
/// Falls back to mdns-sd if avahi is absent, or on non-Linux platforms.
///
/// The record is withdrawn when this handle is dropped.
pub struct MdnsHandle {
    _inner: Inner,
}

enum Inner {
    #[cfg(target_os = "linux")]
    Avahi(AvahiHandle),
    MdnsSd(MdnsSdHandle),
}

impl MdnsHandle {
    pub fn publish(cn: &str, port: u16, state: &Arc<Mutex<AppState>>) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            match linux::publish(cn, port) {
                Ok(handle) => {
                    state.lock().unwrap().push_log(format!(
                        "mDNS: published DVerse ({cn}) on {SERVICE_TYPE} port {port} (via avahi)"
                    ));
                    return Some(Self { _inner: Inner::Avahi(handle) });
                }
                Err(e) => {
                    state.lock().unwrap().push_log(format!(
                        "Warning: avahi D-Bus unavailable ({e}); falling back to mdns-sd"
                    ));
                }
            }
        }

        mdns_sd_publish(cn, port, state).map(|h| Self { _inner: Inner::MdnsSd(h) })
    }
}

// ── Linux: avahi D-Bus ────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
struct AvahiHandle {
    // Keeping the connection alive: avahi-daemon removes services automatically
    // when the owning D-Bus connection closes.
    _conn: zbus::blocking::Connection,
}

#[cfg(target_os = "linux")]
mod linux {
    use super::AvahiHandle;
    use anyhow::Result;
    use zbus::blocking::Connection;

    pub fn publish(cn: &str, port: u16) -> Result<AvahiHandle> {
        let conn = Connection::system()?;

        let group_path: zbus::zvariant::OwnedObjectPath = conn
            .call_method(
                Some("org.freedesktop.Avahi"),
                "/",
                Some("org.freedesktop.Avahi.Server"),
                "EntryGroupNew",
                &(),
            )?
            .body()
            .deserialize()?;

        // AddService(interface, protocol, flags, name, type, domain, host, port, txt)
        // interface=-1 (AVAHI_IF_UNSPEC), protocol=-1 (AVAHI_PROTO_UNSPEC)
        let name = format!("DVerse ({cn})");
        let txt: Vec<Vec<u8>> = vec![format!("cn={cn}").into_bytes()];

        conn.call_method(
            Some("org.freedesktop.Avahi"),
            group_path.as_str(),
            Some("org.freedesktop.Avahi.EntryGroup"),
            "AddService",
            &(
                -1i32,
                -1i32,
                0u32,
                name.as_str(),
                "_dverse._tcp",
                "",
                "",
                port,
                &txt,
            ),
        )?;

        conn.call_method(
            Some("org.freedesktop.Avahi"),
            group_path.as_str(),
            Some("org.freedesktop.Avahi.EntryGroup"),
            "Commit",
            &(),
        )?;

        Ok(AvahiHandle { _conn: conn })
    }
}

// ── mdns-sd (non-Linux or avahi-absent fallback) ──────────────────────────────

struct MdnsSdHandle {
    daemon: ServiceDaemon,
    fullname: String,
}

impl Drop for MdnsSdHandle {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.shutdown();
    }
}

fn mdns_sd_publish(cn: &str, port: u16, state: &Arc<Mutex<AppState>>) -> Option<MdnsSdHandle> {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "Warning: mDNS unavailable ({e}); peer discovery disabled"
            ));
            return None;
        }
    };

    let instance = format!("DVerse ({cn})");
    let host = format!("zenoh-{cn}.local.");
    let props: &[(&str, &str)] = &[("cn", cn)];

    let svc = match ServiceInfo::new(SERVICE_TYPE, &instance, &host, (), port, props) {
        Ok(s) => s,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "Warning: mDNS service info error ({e}); peer discovery disabled"
            ));
            return None;
        }
    };

    let fullname = svc.get_fullname().to_string();

    if let Err(e) = daemon.register(svc) {
        state.lock().unwrap().push_log(format!(
            "Warning: mDNS register failed ({e}); peer discovery disabled"
        ));
        return None;
    }

    state.lock().unwrap().push_log(format!(
        "mDNS: published {instance} on {SERVICE_TYPE} port {port}"
    ));

    Some(MdnsSdHandle { daemon, fullname })
}
