//! Shared constants, naming conventions, and helpers used by both the
//! announcing and browsing sides of DNS-SD discovery.
//!
//! Everything that defines the on-the-wire identity of a DVerse router lives
//! here: change [`instance_name`] and the label seen by every remote browser
//! changes too, without hunting through the publish + browse code paths.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::sync::{Arc, Mutex};

use mdns_sd::{IfKind, ServiceDaemon};
use tokio::sync::watch;

use crate::state::AppState;

// ── Service identity ─────────────────────────────────────────────────────────

/// Fully-qualified DNS-SD service type with trailing dot, used as the PTR
/// record name and the argument to mdns-sd's `browse()` / `ServiceInfo::new`.
pub(super) const SERVICE_TYPE: &str = "_dverse._tcp.local.";

// ── TXT record keys ──────────────────────────────────────────────────────────

/// TXT key carrying the publisher's CN.  Peers use this to skip their own
/// service (we receive our announcement back via multicast loopback) and to
/// label discovered peers.
pub(super) const TXT_KEY_CN: &str = "cn";

/// TXT key carrying the publisher's routable LAN IPv4.
///
/// Workaround for the failure mode where mdns-sd resolves via IPv6 first and
/// reports only the link-local `fe80::` AAAA address — useless for cross-host
/// TLS.  Embedding the LAN IPv4 directly in TXT bypasses it.
pub(super) const TXT_KEY_IP: &str = "ip";

// ── Name builders ────────────────────────────────────────────────────────────

/// Human-readable service instance name shown by DNS-SD browsers
/// (e.g. `dns-sd -B _dverse._tcp` displays `DVerse (alice)`).
pub(super) fn instance_name(cn: &str) -> String {
    format!("DVerse ({cn})")
}

/// SRV target hostname for our router instance.  Peers resolve this to the
/// A record mdns-sd registers via `ServiceInfo::new`.  Per-CN (rather than
/// the machine's default hostname) so multiple test routers on one host
/// don't collide.
///
/// Matches the Step-CA x509 template's `SAN = DNS:zenoh-<cn>.local`, so the
/// TLS handshake validates whether agents connect by hostname or by IP.
pub(super) fn srv_host_name(cn: &str) -> String {
    format!("zenoh-{cn}.local.")
}

/// Zenoh endpoint URI for the router's `connect/endpoints` config.
/// Zenoh's URI form is `<protocol>/<host>:<port>`; dverse runs mTLS.
pub(super) fn zenoh_tls_endpoint(ip: IpAddr, port: u16) -> String {
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
pub(super) fn detect_lan_ipv4() -> Option<Ipv4Addr> {
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
pub(super) fn is_unroutable(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xffc0) == 0xfe80,
    }
}

// ── mdns-sd daemon setup ─────────────────────────────────────────────────────

/// Create a new mdns-sd daemon with interface filters applied.
/// Returns `None` (and logs) if the daemon couldn't be initialised — typically
/// because UDP 5353 is bound by another process in an incompatible way.
pub(super) fn create_filtered_daemon(state: &Arc<Mutex<AppState>>) -> Option<ServiceDaemon> {
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
pub(super) struct PeerRegistry {
    peers: HashMap<String, Vec<String>>,
    tx: watch::Sender<Vec<String>>,
}

impl PeerRegistry {
    pub(super) fn new() -> (Self, watch::Receiver<Vec<String>>) {
        let (tx, rx) = watch::channel(vec![]);
        (Self { peers: HashMap::new(), tx }, rx)
    }

    /// Replace the full endpoint list for one peer.  Returns `true` if the
    /// list actually changed (callers use this to suppress duplicate log
    /// lines when the backend re-fires events for already-known peers).
    pub(super) fn set(&mut self, key: String, endpoints: Vec<String>) -> bool {
        if self.peers.get(&key).map(|v| v.as_slice()) == Some(endpoints.as_slice()) {
            return false;
        }
        self.peers.insert(key, endpoints);
        let _ = self.tx.send(self.flatten());
        true
    }

    /// Forget a peer entirely.  Returns the removed endpoints, for logging.
    pub(super) fn remove(&mut self, key: &str) -> Option<Vec<String>> {
        let removed = self.peers.remove(key)?;
        let _ = self.tx.send(self.flatten());
        Some(removed)
    }

    fn flatten(&self) -> Vec<String> {
        self.peers.values().flatten().cloned().collect()
    }
}
