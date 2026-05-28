//! Browsing side of DNS-SD discovery — watches for other `_dverse._tcp`
//! services on the LAN and exposes their endpoints as a watch channel.

use std::sync::{Arc, Mutex};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::watch;

use crate::state::AppState;

use super::common::{
    is_unroutable, zenoh_tls_endpoint, PeerRegistry, SERVICE_TYPE, TXT_KEY_CN, TXT_KEY_IP,
};

/// Start browsing for peers on the given mdns-sd daemon and return a watch
/// receiver of the current endpoint list.  Spawns a background thread that
/// drains the daemon's event channel.
pub(super) fn mdns_sd_start(
    daemon: &ServiceDaemon,
    my_cn: &str,
    state: &Arc<Mutex<AppState>>,
) -> Option<watch::Receiver<Vec<String>>> {
    let browse_rx = match daemon.browse(SERVICE_TYPE) {
        Ok(rx) => rx,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[mdns-sd]: browse failed ({e}); peer discovery disabled"
            ));
            return None;
        }
    };
    state.lock().unwrap().push_log("mDNS[mdns-sd]: browse started".to_string());

    let (registry, peer_rx) = PeerRegistry::new();
    let my_cn = my_cn.to_string();
    let state = Arc::clone(state);

    std::thread::spawn(move || {
        let mut registry = registry;
        while let Ok(event) = browse_rx.recv() {
            handle_event(event, &my_cn, &state, &mut registry);
        }
        state.lock().unwrap().push_log("mDNS[mdns-sd]: browse loop exited".to_string());
    });

    Some(peer_rx)
}

fn handle_event(
    event: ServiceEvent,
    my_cn: &str,
    state: &Arc<Mutex<AppState>>,
    registry: &mut PeerRegistry,
) {
    match event {
        ServiceEvent::ServiceFound(svc_type, fullname) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[mdns-sd]: found {fullname:?} (type={svc_type:?})"
            ));
        }
        ServiceEvent::ServiceResolved(info) => {
            handle_resolved(info, my_cn, state, registry)
        }
        ServiceEvent::ServiceRemoved(_, fullname) => {
            state
                .lock()
                .unwrap()
                .push_log(format!("mDNS[mdns-sd]: removed {fullname:?}"));
            if let Some(endpoints) = registry.remove(&fullname) {
                state.lock().unwrap().push_log(format!(
                    "Peer router left: {}",
                    endpoints.first().map(String::as_str).unwrap_or(&fullname),
                ));
            }
        }
        other => {
            state.lock().unwrap().push_log(format!(
                "mDNS[mdns-sd]: event {other:?}"
            ));
        }
    }
}

fn handle_resolved(
    info: ServiceInfo,
    my_cn: &str,
    state: &Arc<Mutex<AppState>>,
    registry: &mut PeerRegistry,
) {
    let remote_cn = info.get_property_val_str(TXT_KEY_CN).unwrap_or_default();
    let addrs: Vec<_> = info.get_addresses().iter().copied().collect();
    state.lock().unwrap().push_log(format!(
        "mDNS[mdns-sd]: resolved {:?} cn={remote_cn:?} addrs={addrs:?} port={}",
        info.get_fullname(),
        info.get_port(),
    ));

    if remote_cn.is_empty() || remote_cn == my_cn {
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: skipping self or empty CN ({remote_cn:?})"
        ));
        return;
    }

    let endpoints = endpoints_for(&info, remote_cn, state);
    if endpoints.is_empty() {
        return;
    }

    if registry.set(info.get_fullname().to_string(), endpoints.clone()) {
        state.lock().unwrap().push_log(format!(
            "Discovered peer router: {} (+{} addr) (CN={remote_cn})",
            endpoints[0],
            endpoints.len() - 1,
        ));
    } else {
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: peer {remote_cn} unchanged, ignoring duplicate"
        ));
    }
}

/// Convert a resolved `ServiceInfo` into the list of Zenoh endpoints we'll
/// connect to.  Prefers the peer's TXT `ip=` over whatever SRV/A resolution
/// returned (see [`super::common::TXT_KEY_IP`] for why).
fn endpoints_for(
    info: &ServiceInfo,
    remote_cn: &str,
    state: &Arc<Mutex<AppState>>,
) -> Vec<String> {
    let port = info.get_port();

    let txt_ipv4 = info
        .get_property_val_str(TXT_KEY_IP)
        .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
        .filter(|ip| !ip.is_loopback() && !ip.is_link_local());

    if let Some(ipv4) = txt_ipv4 {
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: using TXT ip={ipv4} for {remote_cn}"
        ));
        return vec![zenoh_tls_endpoint(std::net::IpAddr::V4(ipv4), port)];
    }

    let mut endpoints: Vec<String> = info
        .get_addresses()
        .iter()
        .filter_map(|a| {
            if is_unroutable(a) {
                state.lock().unwrap().push_log(format!(
                    "mDNS[mdns-sd]: skipping unroutable addr {a}"
                ));
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
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: resolved {remote_cn} but no routable IPv4 \
             (no TXT ip, addrs={:?}); skipping",
            info.get_addresses().iter().collect::<Vec<_>>(),
        ));
    }

    endpoints
}
