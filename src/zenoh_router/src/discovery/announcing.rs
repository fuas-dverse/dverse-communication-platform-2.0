//! Publishing side of DNS-SD discovery — registers this router as a
//! `_dverse._tcp` service so peers can find us.
//!
//! Two backends:
//!   * Linux: [`avahi_register`] calls avahi-daemon's D-Bus EntryGroup API.
//!   * Cross-platform: [`mdns_sd_register`] adds a service to a mdns-sd
//!     daemon (created by [`super::common::create_filtered_daemon`]).

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::state::AppState;

use super::common::{
    detect_lan_ipv4, instance_name, pinned_a_record_host, srv_host_name, txt_cn_entry,
    txt_ip_entry, txt_session_entry, AVAHI_IF_UNSPEC, AVAHI_NO_FLAGS, AVAHI_PROTO_INET,
    AVAHI_PROTO_UNSPEC, SERVICE_NAME, SERVICE_TYPE, TXT_KEY_CN, TXT_KEY_IP, TXT_KEY_SESSION,
};

// ── mdns-sd ──────────────────────────────────────────────────────────────────

/// Register our service on the given mdns-sd daemon.  Returns the fullname
/// so the caller can `unregister` it on shutdown.
pub(super) fn mdns_sd_register(
    daemon: &ServiceDaemon,
    cn: &str,
    session_id: &str,
    port: u16,
    state: &Arc<Mutex<AppState>>,
) -> Option<String> {
    let instance = instance_name(cn);
    let host = srv_host_name(cn);
    let lan_ip = detect_lan_ipv4();

    state.lock().unwrap().push_log(format!(
        "mDNS[mdns-sd]: LAN IP={}",
        lan_ip
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));

    // TXT props: always `cn` + `session`, optionally `ip` for cross-stack
    // address recovery.
    let ip_str;
    let props: &[(&str, &str)] = if let Some(ip) = lan_ip {
        ip_str = ip.to_string();
        &[
            (TXT_KEY_CN, cn),
            (TXT_KEY_SESSION, session_id),
            (TXT_KEY_IP, &ip_str),
        ]
    } else {
        &[(TXT_KEY_CN, cn), (TXT_KEY_SESSION, session_id)]
    };

    // Pin the A record to the LAN IPv4 explicitly; without this mdns-sd
    // populates it with every active interface address — loopback / docker /
    // VPN — and the resulting set is unhelpful to remote peers.
    let svc_result = match lan_ip {
        Some(ip) => ServiceInfo::new(
            SERVICE_TYPE,
            &instance,
            &host,
            IpAddr::V4(ip),
            port,
            props,
        ),
        None => ServiceInfo::new(SERVICE_TYPE, &instance, &host, (), port, props),
    };
    let svc = match svc_result {
        Ok(s) => s,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[mdns-sd]: service info error ({e}); peer discovery disabled"
            ));
            return None;
        }
    };

    let fullname = svc.get_fullname().to_string();
    state.lock().unwrap().push_log(format!(
        "mDNS[mdns-sd]: registering {fullname} on port {port}"
    ));

    if let Err(e) = daemon.register(svc) {
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: register failed ({e}); peer discovery disabled"
        ));
        return None;
    }

    state.lock().unwrap().push_log(format!(
        "mDNS[mdns-sd]: published {instance} on {SERVICE_TYPE} port {port}"
    ));

    Some(fullname)
}

// ── Linux: avahi D-Bus EntryGroup ────────────────────────────────────────────

/// Publish our service via avahi-daemon's D-Bus interface.
/// On success the EntryGroup is committed; on Drop of the bus connection
/// avahi will withdraw the record.
#[cfg(target_os = "linux")]
pub(super) fn avahi_register(
    cn: &str,
    session_id: &str,
    port: u16,
    conn: &zbus::blocking::Connection,
) -> anyhow::Result<()> {
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

    let instance = instance_name(cn);
    let lan_ip = detect_lan_ipv4();

    let mut txt: Vec<Vec<u8>> = vec![txt_cn_entry(cn), txt_session_entry(session_id)];
    if let Some(ipv4) = lan_ip {
        txt.push(txt_ip_entry(ipv4));
    }

    // Try to claim a dedicated A record so SRV resolution lands on the
    // LAN IPv4 only.  Falls back to avahi's default machine hostname on
    // collision (stale entry from a prior crash, etc.).
    let custom_host = pinned_a_record_host(cn);
    let custom_host_registered = lan_ip
        .map(|ipv4| try_register_pinned_a(conn, &group_path, &custom_host, ipv4))
        .unwrap_or(false);
    let srv_host = if custom_host_registered { custom_host.as_str() } else { "" };

    conn.call_method(
        Some("org.freedesktop.Avahi"),
        group_path.as_str(),
        Some("org.freedesktop.Avahi.EntryGroup"),
        "AddService",
        &(
            AVAHI_IF_UNSPEC,
            AVAHI_PROTO_UNSPEC,
            AVAHI_NO_FLAGS,
            instance.as_str(),
            SERVICE_NAME,
            "",  // sub-type: none
            srv_host,
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

    Ok(())
}

/// Best-effort registration of an A record (`hostname → ipv4`) in the given
/// EntryGroup.  Returns `true` on success.  Failure (typically a stale-name
/// `CollisionError`) is logged to stderr and reported as `false` so the
/// caller can fall back to avahi's default hostname behaviour.
#[cfg(target_os = "linux")]
fn try_register_pinned_a(
    conn: &zbus::blocking::Connection,
    group_path: &zbus::zvariant::OwnedObjectPath,
    hostname: &str,
    ipv4: std::net::Ipv4Addr,
) -> bool {
    let res = conn.call_method(
        Some("org.freedesktop.Avahi"),
        group_path.as_str(),
        Some("org.freedesktop.Avahi.EntryGroup"),
        "AddAddress",
        &(
            AVAHI_IF_UNSPEC,
            AVAHI_PROTO_INET,
            AVAHI_NO_FLAGS,
            hostname,
            ipv4.to_string().as_str(),
        ),
    );
    match res {
        Ok(_) => true,
        Err(e) => {
            eprintln!(
                "mDNS[avahi]: AddAddress for {hostname} failed ({e}); using default host"
            );
            false
        }
    }
}
