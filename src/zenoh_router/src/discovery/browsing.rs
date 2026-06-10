//! Browsing side of DNS-SD discovery — watches for other `_dverse._tcp`
//! services on the LAN and exposes their endpoints as a watch channel.

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::watch;
use tracing::{info, warn};

use super::common::{
    is_unroutable, zenoh_tls_endpoint, PeerRegistry, SessionRegistry, SERVICE_TYPE, TXT_KEY_CN,
    TXT_KEY_IP, TXT_KEY_SESSION,
};

/// Start browsing for peers on the given mdns-sd daemon. Returns two watch
/// receivers: the current peer endpoint list (for Zenoh `connect/endpoints`)
/// and the list of session-host admin CNs visible on the LAN (for the chooser).
/// Spawns a background thread that drains the daemon's event channel.
pub(super) fn mdns_sd_start(
    daemon: &ServiceDaemon,
    my_cn: &str,
) -> Option<(watch::Receiver<Vec<String>>, watch::Receiver<Vec<String>>)> {
    let browse_rx = match daemon.browse(SERVICE_TYPE) {
        Ok(rx) => rx,
        Err(e) => {
            warn!(error = %e, "mdns-sd browse failed; peer discovery disabled");
            return None;
        }
    };
    info!("mdns-sd browse started");

    let (registry, peer_rx) = PeerRegistry::new();
    let (sessions, sessions_rx) = SessionRegistry::new();
    let my_cn = my_cn.to_string();

    std::thread::spawn(move || {
        let mut registry = registry;
        let mut sessions = sessions;
        while let Ok(event) = browse_rx.recv() {
            handle_event(event, &my_cn, &mut registry, &mut sessions);
        }
        info!("mdns-sd browse loop exited");
    });

    Some((peer_rx, sessions_rx))
}

fn handle_event(
    event: ServiceEvent,
    my_cn: &str,
    registry: &mut PeerRegistry,
    sessions: &mut SessionRegistry,
) {
    match event {
        ServiceEvent::ServiceFound(svc_type, fullname) => {
            info!(svc_type = %svc_type, fullname = %fullname, "mdns-sd service found");
        }
        ServiceEvent::ServiceResolved(info) => {
            handle_resolved(info, my_cn, registry, sessions)
        }
        ServiceEvent::ServiceRemoved(_, fullname) => {
            info!(fullname = %fullname, "mdns-sd service removed");
            if let Some(endpoints) = registry.remove(&fullname) {
                info!(
                    target_endpoint = %endpoints.first().map(String::as_str).unwrap_or(&fullname),
                    "peer router left"
                );
            }
        }
        other => {
            info!(event = ?other, "mdns-sd other event");
        }
    }
}

fn handle_resolved(
    info: ServiceInfo,
    my_cn: &str,
    registry: &mut PeerRegistry,
    sessions: &mut SessionRegistry,
) {
    let remote_cn = info.get_property_val_str(TXT_KEY_CN).unwrap_or_default();
    let remote_session = info.get_property_val_str(TXT_KEY_SESSION).unwrap_or_default();
    let addrs: Vec<_> = info.get_addresses().iter().copied().collect();
    info!(
        fullname = %info.get_fullname(),
        cn = %remote_cn,
        session = %remote_session,
        ?addrs,
        port = info.get_port(),
        "mdns-sd resolved peer"
    );

    // All-sessions registry (for the chooser): record every session *host* —
    // a router whose own CN equals its session= TXT (i.e. the session admin).
    // Done before the mesh filters so other sessions are still discoverable.
    if !remote_cn.is_empty() && remote_cn == remote_session && sessions.add(remote_cn.to_string())
    {
        info!(admin_cn = %remote_cn, "discovered session on the LAN");
    }

    if remote_cn.is_empty() || remote_cn == my_cn {
        info!(cn = %remote_cn, "mdns-sd skipping self or empty CN");
        return;
    }

    // Shared fabric: connect to *every* discovered router regardless of session.
    // Cross-session traffic can't leak because each router + its agents run under
    // a per-session Zenoh `namespace`, so a router only ever pub/subs its own
    // session's keys; other sessions' traffic is merely relayed, never seen.
    // (The earlier same-session peer filter is intentionally removed here.)

    let endpoints = endpoints_for(&info, remote_cn);
    if endpoints.is_empty() {
        return;
    }

    if registry.set(info.get_fullname().to_string(), endpoints.clone()) {
        info!(
            endpoint = %endpoints[0],
            extra_count = endpoints.len() - 1,
            cn = %remote_cn,
            "discovered peer router"
        );
    } else {
        info!(cn = %remote_cn, "mdns-sd peer unchanged, ignoring duplicate");
    }
}

/// Convert a resolved `ServiceInfo` into the list of Zenoh endpoints we'll
/// connect to.  Prefers the peer's TXT `ip=` over whatever SRV/A resolution
/// returned (see [`super::common::TXT_KEY_IP`] for why).
fn endpoints_for(info: &ServiceInfo, remote_cn: &str) -> Vec<String> {
    let port = info.get_port();

    let txt_ipv4 = info
        .get_property_val_str(TXT_KEY_IP)
        .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
        .filter(|ip| !ip.is_loopback() && !ip.is_link_local());

    if let Some(ipv4) = txt_ipv4 {
        info!(ip = %ipv4, cn = %remote_cn, "mdns-sd using TXT ip");
        return vec![zenoh_tls_endpoint(std::net::IpAddr::V4(ipv4), port)];
    }

    let mut endpoints: Vec<String> = info
        .get_addresses()
        .iter()
        .filter_map(|a| {
            if is_unroutable(a) {
                info!(addr = %a, "mdns-sd skipping unroutable addr");
                return None;
            }
            match a {
                std::net::IpAddr::V4(_) => Some(zenoh_tls_endpoint(*a, port)),
                _ => None,
            }
        })
        .collect();
    endpoints.sort();

    if endpoints.is_empty() {
        warn!(
            cn = %remote_cn,
            addrs = ?info.get_addresses().iter().collect::<Vec<_>>(),
            "mdns-sd resolved peer with no routable IPv4; skipping"
        );
    }

    endpoints
}
