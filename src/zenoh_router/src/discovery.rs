//! DNS-SD peer discovery for the Zenoh router.
//!
//! Public entry point: [`MdnsHandle::publish`].  Drop the handle to withdraw
//! the service record and stop browsing.
//!
//! Backend: one `mdns_sd::ServiceDaemon` per router process, used for both
//! publishing and browsing.  mdns-sd is pure Rust, runs in-process, and
//! ships its own A record alongside the SRV target via `ServiceInfo::new`
//! — the same shape on every platform, no avahi-daemon coupling.
//!
//! Service identity: `DVerse (<cn>)._dverse._tcp.local.` with TXT records
//! carrying the publisher's CN and routable LAN IPv4.  Peers receive PTRs
//! over multicast, resolve, and feed `tls/<ip>:<port>` endpoints back to
//! the router via a `watch::Receiver<Vec<String>>` that drives a Zenoh
//! session reload.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::{Arc, Mutex};

use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::watch;

use crate::state::AppState;

// ── Service identity ─────────────────────────────────────────────────────────

/// Fully-qualified DNS-SD service type with trailing dot, used as the PTR
/// record name and the argument to mdns-sd's `browse()` / `ServiceInfo::new`.
const SERVICE_TYPE: &str = "_dverse._tcp.local.";

// ── TXT record keys ──────────────────────────────────────────────────────────

/// TXT key carrying the publisher's CN.  Peers use this to skip their own
/// service (we receive our announcement back via multicast loopback) and to
/// label discovered peers.
const TXT_KEY_CN: &str = "cn";

/// TXT key carrying the publisher's routable LAN IPv4.
///
/// Workaround for the failure mode where mdns-sd resolves via IPv6 first and
/// reports only the link-local `fe80::` AAAA address — useless for cross-host
/// TLS.  Embedding the LAN IPv4 directly in TXT bypasses it.
const TXT_KEY_IP: &str = "ip";

// ── Name builders ────────────────────────────────────────────────────────────

/// Human-readable service instance name shown by DNS-SD browsers
/// (e.g. `dns-sd -B _dverse._tcp` displays `DVerse (alice)`).
fn instance_name(cn: &str) -> String {
    format!("DVerse ({cn})")
}

/// SRV target hostname for our router instance.  Peers resolve this to the
/// A record mdns-sd registers via `ServiceInfo::new`.  Per-CN (rather than
/// the machine's default hostname) so multiple test routers on one host
/// don't collide.
///
/// Matches the Step-CA x509 template's `SAN = DNS:zenoh-<cn>.local`, so the
/// TLS handshake validates whether agents connect by hostname or by IP.
fn srv_host_name(cn: &str) -> String {
    format!("zenoh-{cn}.local.")
}

/// Zenoh endpoint URI for the router's `connect/endpoints` config.
/// Zenoh's URI form is `<protocol>/<host>:<port>`; dverse runs mTLS.
fn zenoh_tls_endpoint(ip: IpAddr, port: u16) -> String {
    format!("tls/{ip}:{port}")
}

// ── IP detection ─────────────────────────────────────────────────────────────

/// Bind ephemerally for the LAN-detection trick — we never `send()`, so the
/// OS doesn't actually claim any specific interface.
const LAN_DETECT_BIND: &str = "0.0.0.0:0";

/// Well-known public address used to coax the routing table into revealing
/// which local IP would be used for outbound traffic.  No packet is sent —
/// only `connect()` + `local_addr()`.  Any reachable public IP works;
/// Cloudflare's public DNS is chosen for its long-term stability.
const LAN_DETECT_REMOTE: &str = "1.1.1.1:53";

/// Detect this machine's LAN IPv4 — the address the kernel would use to
/// reach the public internet.  Uses a UDP routing trick: bind ephemerally,
/// `connect()` to a public address (no packet leaves), then read back
/// the local address the kernel selected.
fn detect_lan_ipv4() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind(LAN_DETECT_BIND).ok()?;
    sock.connect(LAN_DETECT_REMOTE).ok()?;
    match sock.local_addr().ok()? {
        std::net::SocketAddr::V4(a) if !a.ip().is_loopback() && !a.ip().is_link_local() => {
            Some(*a.ip())
        }
        _ => None,
    }
}

/// Whether an address is unsuitable to hand to Zenoh as a peer endpoint:
/// loopback (only routes inside our own host) or link-local (no scope id,
/// so cross-host TLS would fail).
fn is_unroutable(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

// ── mdns-sd daemon setup ─────────────────────────────────────────────────────

/// Create a new mdns-sd daemon with interface filters applied.
/// Returns `None` (and logs) if the daemon couldn't be initialised — typically
/// because UDP 5353 is bound by another process in an incompatible way.
fn create_filtered_daemon(state: &Arc<Mutex<AppState>>) -> Option<ServiceDaemon> {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[mdns-sd]: daemon init failed ({e}); peer discovery disabled"
            ));
            return None;
        }
    };
    state.lock().unwrap().push_log("mDNS[mdns-sd]: daemon started".to_string());
    apply_interface_filters(&daemon, state);
    Some(daemon)
}

/// Disable IPv6 and any virtual/link-local IPv4 adapter on the given daemon.
///
/// Why each filter exists:
///   * IPv6 — Zenoh peer endpoints are `tls/<ipv4>:<port>`; `fe80::`
///     addresses are not routable across subnets and would only generate
///     dead endpoints.
///   * Virtual adapters — Npcap Loopback (Wireshark), docker0 / br-* / veth*,
///     vEthernet (Hyper-V), VMware, VirtualBox, WireGuard.  Valid locally
///     but reach no remote peer; worse, on Windows a low-metric Npcap
///     adapter silently swallows our PTR responses entirely.
///   * Link-local IPv4 (169.254.0.0/16) — same story; no peer is there.
fn apply_interface_filters(daemon: &ServiceDaemon, state: &Arc<Mutex<AppState>>) {
    let _ = daemon.disable_interface(IfKind::IPv6);

    let Ok(ifaces) = if_addrs::get_if_addrs() else { return };
    let mut already_disabled: HashSet<String> = HashSet::new();

    for iface in &ifaces {
        if !is_virtual_or_link_local(iface) {
            continue;
        }
        if !already_disabled.insert(iface.name.clone()) {
            continue;
        }
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: disabling virtual/link-local iface {} ({})",
            iface.name,
            iface.ip(),
        ));
        let _ = daemon.disable_interface(IfKind::Name(iface.name.clone()));
    }
}

/// Pattern-match an interface name against known virtual-adapter prefixes,
/// or check whether its IPv4 address is in the link-local range.
fn is_virtual_or_link_local(iface: &if_addrs::Interface) -> bool {
    let name = iface.name.to_lowercase();
    let known_virtual = name.contains("npcap")
        || name.contains("loopback")
        || name.contains("vethernet")
        || name.contains("hyper-v")
        || name.contains("vmware")
        || name.contains("virtualbox")
        || name.contains("docker")
        || name.starts_with("br-")
        || name.starts_with("veth")
        || name.starts_with("wg");
    let link_local_v4 = matches!(
        &iface.addr,
        if_addrs::IfAddr::V4(v4) if v4.ip.is_link_local()
    );
    known_virtual || link_local_v4
}

// ── Peer registry ────────────────────────────────────────────────────────────

/// Tracks discovered peers and pushes the flattened endpoint list onto a
/// watch channel whenever it changes.  The flattened list is what the router
/// passes to Zenoh's `connect/endpoints`; the change notification triggers
/// a session reload to pick up new peers.
struct PeerRegistry {
    peers: HashMap<String, Vec<String>>,
    tx: watch::Sender<Vec<String>>,
}

impl PeerRegistry {
    fn new() -> (Self, watch::Receiver<Vec<String>>) {
        let (tx, rx) = watch::channel(vec![]);
        (Self { peers: HashMap::new(), tx }, rx)
    }

    /// Replace the full endpoint list for one peer.  Returns `true` if the
    /// list actually changed (callers use this to suppress duplicate log
    /// lines when the backend re-fires events for already-known peers).
    fn set(&mut self, key: String, endpoints: Vec<String>) -> bool {
        if self.peers.get(&key).map(|v| v.as_slice()) == Some(endpoints.as_slice()) {
            return false;
        }
        self.peers.insert(key, endpoints);
        let _ = self.tx.send(self.flatten());
        true
    }

    /// Forget a peer entirely.  Returns the removed endpoints, for logging.
    fn remove(&mut self, key: &str) -> Option<Vec<String>> {
        let removed = self.peers.remove(key)?;
        let _ = self.tx.send(self.flatten());
        Some(removed)
    }

    fn flatten(&self) -> Vec<String> {
        self.peers.values().flatten().cloned().collect()
    }
}

// ── Public handle ────────────────────────────────────────────────────────────

/// Discovery handle.  Holds the mdns-sd daemon alive and exposes a `peer_rx`
/// watch channel listing the current peer endpoints as `tls/<ip>:<port>`
/// strings, ready for Zenoh's `connect/endpoints`.
pub struct MdnsHandle {
    _inner: MdnsSdSession,
    pub peer_rx: watch::Receiver<Vec<String>>,
}

impl MdnsHandle {
    pub fn publish(cn: &str, port: u16, state: &Arc<Mutex<AppState>>) -> Option<Self> {
        state.lock().unwrap().push_log("mDNS: using mdns-sd backend".to_string());
        mdns_sd_start(cn, port, state)
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
    port: u16,
    state: &Arc<Mutex<AppState>>,
) -> Option<MdnsHandle> {
    let daemon = create_filtered_daemon(state)?;
    let fullname = mdns_sd_register(&daemon, cn, port, state)?;
    let peer_rx = mdns_sd_browse(&daemon, cn, state)?;
    Some(MdnsHandle {
        _inner: MdnsSdSession { daemon, fullname },
        peer_rx,
    })
}

// ── Announcing ───────────────────────────────────────────────────────────────

/// Register our service on the given mdns-sd daemon.  Returns the fullname
/// so the caller can `unregister` it on shutdown.
fn mdns_sd_register(
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

// ── Browsing ─────────────────────────────────────────────────────────────────

/// Start browsing for peers on the given mdns-sd daemon and return a watch
/// receiver of the current endpoint list.  Spawns a background thread that
/// drains the daemon's event channel.
fn mdns_sd_browse(
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
/// returned (see [`TXT_KEY_IP`] for why).
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
        return vec![zenoh_tls_endpoint(IpAddr::V4(ipv4), port)];
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
                IpAddr::V4(_) => Some(zenoh_tls_endpoint(*a, port)),
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
