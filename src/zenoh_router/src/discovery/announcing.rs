//! Publishing side of DNS-SD discovery — registers this router as a
//! `_dverse._tcp` service so peers can find us.
//!
//! Single backend: mdns-sd.  Runs in-process, ships its A record alongside
//! the SRV target via `ServiceInfo::new`, no avahi-daemon coupling.

use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use mdns_sd::{ServiceDaemon, ServiceInfo};

use crate::state::AppState;

use super::common::{
    detect_lan_ipv4, instance_name, srv_host_name, SERVICE_TYPE, TXT_KEY_CN, TXT_KEY_IP,
};

/// Register our service on the given mdns-sd daemon.  Returns the fullname
/// so the caller can `unregister` it on shutdown.
pub(super) fn mdns_sd_register(
    daemon: &ServiceDaemon,
    cn: &str,
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

    // TXT props: always `cn`, optionally `ip` for cross-stack address recovery.
    let ip_str;
    let props: &[(&str, &str)] = if let Some(ip) = lan_ip {
        ip_str = ip.to_string();
        &[(TXT_KEY_CN, cn), (TXT_KEY_IP, &ip_str)]
    } else {
        &[(TXT_KEY_CN, cn)]
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
