//! Browsing side of DNS-SD discovery — watches for other `_dverse._tcp`
//! services on the LAN and exposes their endpoints as a watch channel.
//!
//! Two backends, mirroring announcing.rs:
//!   * Linux: avahi D-Bus `ServiceBrowser` + `ResolveService`.
//!   * Cross-platform: mdns-sd `browse()` + `ServiceEvent` channel.

use std::sync::{Arc, Mutex};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use tokio::sync::watch;

use crate::state::AppState;

use super::common::{
    is_unroutable, peer_cache_key, zenoh_tls_endpoint, PeerRegistry, MDNS_DOMAIN,
    AVAHI_IF_UNSPEC, AVAHI_NO_FLAGS, AVAHI_PROTO_UNSPEC, SERVICE_NAME,
    DBUS_MATCH_QUEUE_DEPTH, SERVICE_TYPE, TXT_KEY_CN, TXT_KEY_IP, TXT_KEY_SESSION,
};

// ── mdns-sd browse ───────────────────────────────────────────────────────────

/// Start browsing for peers on the given mdns-sd daemon and return a watch
/// receiver of the current endpoint list.  Spawns a background thread that
/// drains the daemon's event channel.
pub(super) fn mdns_sd_start(
    daemon: &ServiceDaemon,
    my_cn: &str,
    my_session: &str,
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
    let my_session = my_session.to_string();
    let state = Arc::clone(state);

    std::thread::spawn(move || {
        let mut registry = registry;
        while let Ok(event) = browse_rx.recv() {
            handle_mdns_sd_event(event, &my_cn, &my_session, &state, &mut registry);
        }
        state.lock().unwrap().push_log("mDNS[mdns-sd]: browse loop exited".to_string());
    });

    Some(peer_rx)
}

fn handle_mdns_sd_event(
    event: ServiceEvent,
    my_cn: &str,
    my_session: &str,
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
            handle_resolved(info, my_cn, my_session, state, registry)
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
    my_session: &str,
    state: &Arc<Mutex<AppState>>,
    registry: &mut PeerRegistry,
) {
    let remote_cn = info.get_property_val_str(TXT_KEY_CN).unwrap_or_default();
    let remote_session = info.get_property_val_str(TXT_KEY_SESSION).unwrap_or_default();
    let addrs: Vec<_> = info.get_addresses().iter().copied().collect();
    state.lock().unwrap().push_log(format!(
        "mDNS[mdns-sd]: resolved {:?} cn={remote_cn:?} session={remote_session:?} addrs={addrs:?} port={}",
        info.get_fullname(),
        info.get_port(),
    ));

    if remote_cn.is_empty() || remote_cn == my_cn {
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: skipping self or empty CN ({remote_cn:?})"
        ));
        return;
    }

    // Session filter: skip peers that belong to a different session.  Without
    // this, two unrelated users on the same LAN would auto-mesh their routers.
    if remote_session != my_session {
        state.lock().unwrap().push_log(format!(
            "mDNS[mdns-sd]: skipping peer {remote_cn} (session={remote_session:?}, ours={my_session:?})"
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

// ── Linux: avahi D-Bus ServiceBrowser ────────────────────────────────────────

/// Subscribe to avahi's `_dverse._tcp` ServiceBrowser via D-Bus.  Returns the
/// D-Bus connection (caller keeps it alive) and a watch receiver carrying
/// the current peer endpoint list.
///
/// Critical ordering: the D-Bus signal subscription is registered BEFORE
/// `ServiceBrowserNew` is called.  avahi fires `ItemNew` signals for any
/// already-cached services immediately after the browser is created; if we
/// only set up the subscription afterwards we miss those signals.
///
/// Implementation: the worker thread registers a broad MatchRule (no `path`
/// filter — we don't know the browser's object path yet), THEN calls
/// `ServiceBrowserNew`, THEN filters incoming signals by `browser_path`.
/// A `mpsc::channel` blocks the caller until setup is complete.
#[cfg(target_os = "linux")]
pub(super) fn avahi_start(
    my_cn: &str,
    my_session: &str,
    state: Arc<Mutex<AppState>>,
) -> Option<(zbus::blocking::Connection, watch::Receiver<Vec<String>>)> {
    use zbus::blocking::Connection;

    let conn = match Connection::system() {
        Ok(c) => c,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[avahi]: browse D-Bus conn failed ({e})"
            ));
            return None;
        }
    };
    // Resolve calls block on D-Bus replies; using a separate connection
    // prevents them from interleaving with the incoming signal stream.
    let conn_resolve = match Connection::system() {
        Ok(c) => c,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[avahi]: resolve D-Bus conn failed ({e})"
            ));
            return None;
        }
    };

    let conn_keepalive = conn.clone();
    let (registry, peer_rx) = PeerRegistry::new();
    let my_cn = my_cn.to_string();
    let my_session = my_session.to_string();

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<String, String>>();

    std::thread::spawn(move || {
        run_avahi_browse_thread(
            conn, conn_resolve, my_cn, my_session, state, registry, ready_tx,
        );
    });

    match ready_rx.recv() {
        Ok(Ok(_)) => Some((conn_keepalive, peer_rx)),
        Ok(Err(_)) | Err(_) => None,
    }
}

#[cfg(target_os = "linux")]
fn run_avahi_browse_thread(
    conn: zbus::blocking::Connection,
    conn_resolve: zbus::blocking::Connection,
    my_cn: String,
    my_session: String,
    state: Arc<Mutex<AppState>>,
    mut registry: PeerRegistry,
    ready_tx: std::sync::mpsc::Sender<Result<String, String>>,
) {
    // Step 1: subscribe to ServiceBrowser signals BEFORE creating the browser.
    let iter = match subscribe_to_service_browser_signals(&conn) {
        Ok(i) => i,
        Err(msg) => {
            state.lock().unwrap().push_log(msg.clone());
            ready_tx.send(Err(msg)).ok();
            return;
        }
    };

    // Step 2: NOW create the browser — signals are already queued.
    let browser_path = match create_service_browser(&conn) {
        Ok(path) => path,
        Err(msg) => {
            state.lock().unwrap().push_log(msg.clone());
            ready_tx.send(Err(msg)).ok();
            return;
        }
    };

    state.lock().unwrap().push_log(format!("mDNS[avahi]: browser at {browser_path}"));
    ready_tx.send(Ok(browser_path.clone())).ok();

    drain_avahi_signals(
        iter, &browser_path, &conn_resolve, &my_cn, &my_session, &state, &mut registry,
    );
    state.lock().unwrap().push_log("mDNS[avahi]: signal loop exited".to_string());
}

#[cfg(target_os = "linux")]
fn subscribe_to_service_browser_signals(
    conn: &zbus::blocking::Connection,
) -> Result<zbus::blocking::MessageIterator, String> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.Avahi")
        .map_err(|e| format!("mDNS[avahi]: match rule sender error ({e})"))?
        .interface("org.freedesktop.Avahi.ServiceBrowser")
        .map_err(|e| format!("mDNS[avahi]: match rule iface error ({e})"))?
        .build();

    zbus::blocking::MessageIterator::for_match_rule(rule, conn, Some(DBUS_MATCH_QUEUE_DEPTH))
        .map_err(|e| format!("mDNS[avahi]: iterator error ({e})"))
}

#[cfg(target_os = "linux")]
fn create_service_browser(conn: &zbus::blocking::Connection) -> Result<String, String> {
    let path: zbus::zvariant::OwnedObjectPath = conn
        .call_method(
            Some("org.freedesktop.Avahi"),
            "/",
            Some("org.freedesktop.Avahi.Server"),
            "ServiceBrowserNew",
            &(
                AVAHI_IF_UNSPEC,
                AVAHI_PROTO_UNSPEC,
                SERVICE_NAME,
                MDNS_DOMAIN,
                AVAHI_NO_FLAGS,
            ),
        )
        .and_then(|r| r.body().deserialize().map_err(Into::into))
        .map_err(|e| format!("mDNS[avahi]: ServiceBrowserNew failed ({e})"))?;
    Ok(path.as_str().to_string())
}

#[cfg(target_os = "linux")]
fn drain_avahi_signals(
    iter: zbus::blocking::MessageIterator,
    browser_path: &str,
    conn_resolve: &zbus::blocking::Connection,
    my_cn: &str,
    my_session: &str,
    state: &Arc<Mutex<AppState>>,
    registry: &mut PeerRegistry,
) {
    for msg_result in iter {
        let msg = match msg_result {
            Ok(m) => m,
            Err(e) => {
                state.lock().unwrap().push_log(format!(
                    "mDNS[avahi]: D-Bus error ({e}); stopping"
                ));
                break;
            }
        };

        // Filter: only handle signals from OUR ServiceBrowser object — the
        // broad MatchRule above can deliver signals from any browser on the
        // bus if multiple `ServiceBrowserNew` instances exist.
        let msg_path = msg.header().path().map(|p| p.as_str().to_owned());
        if msg_path.as_deref() != Some(browser_path) {
            continue;
        }
        let Some(member) = msg.header().member().map(|m| m.to_owned()) else { continue };

        match member.as_str() {
            "ItemNew" => {
                handle_avahi_item_new(&msg, conn_resolve, my_cn, my_session, state, registry)
            }
            "ItemRemove" => handle_avahi_item_remove(&msg, state, registry),
            _ => {}
        }
    }
}

#[cfg(target_os = "linux")]
fn handle_avahi_item_new(
    msg: &zbus::Message,
    conn_resolve: &zbus::blocking::Connection,
    my_cn: &str,
    my_session: &str,
    state: &Arc<Mutex<AppState>>,
    registry: &mut PeerRegistry,
) {
    let body: zbus::Result<(i32, i32, String, String, String, u32)> = msg.body().deserialize();
    let Ok((svc_iface, svc_proto, name, svc_type, domain, _)) = body else {
        state
            .lock()
            .unwrap()
            .push_log("mDNS[avahi]: ItemNew body parse failed".to_string());
        return;
    };

    state.lock().unwrap().push_log(format!(
        "mDNS[avahi]: ItemNew iface={svc_iface} proto={svc_proto} \
         name={name:?} type={svc_type:?} domain={domain:?}"
    ));

    let Some((host, address, port, remote_cn, txt_ipv4, remote_session)) =
        resolve_avahi_service(conn_resolve, svc_iface, svc_proto, &name, &svc_type, &domain, state)
    else {
        return;
    };

    state.lock().unwrap().push_log(format!(
        "mDNS[avahi]: resolved host={host:?} addr={address} port={port} \
         cn={remote_cn:?} session={remote_session:?} txt_ip={txt_ipv4:?}"
    ));

    if remote_cn.is_empty() || remote_cn == my_cn {
        state.lock().unwrap().push_log(format!(
            "mDNS[avahi]: skipping self or empty CN ({remote_cn:?})"
        ));
        return;
    }

    // Session filter: skip peers that belong to a different session.  Without
    // this, two unrelated users on the same LAN would auto-mesh their routers.
    if remote_session != my_session {
        state.lock().unwrap().push_log(format!(
            "mDNS[avahi]: skipping peer {remote_cn} (session={remote_session:?}, ours={my_session:?})"
        ));
        return;
    }

    let Some(chosen_ip) = pick_peer_address(&address, txt_ipv4, state) else {
        return;
    };
    if is_unroutable(&chosen_ip) {
        state.lock().unwrap().push_log(format!(
            "mDNS[avahi]: skipping unroutable addr {chosen_ip}"
        ));
        return;
    }

    let endpoint = zenoh_tls_endpoint(chosen_ip, port);
    let key = peer_cache_key(&name, &svc_type);
    if registry.add_one(key, endpoint.clone()) {
        state.lock().unwrap().push_log(format!(
            "Discovered peer router: {endpoint} (CN={remote_cn})"
        ));
    }
}

/// Tuple returned by avahi `ResolveService`:
/// `(host, address, port, cn, txt_ipv4, session)`.
#[cfg(target_os = "linux")]
fn resolve_avahi_service(
    conn: &zbus::blocking::Connection,
    iface: i32,
    proto: i32,
    name: &str,
    svc_type: &str,
    domain: &str,
    state: &Arc<Mutex<AppState>>,
) -> Option<(String, String, u16, String, Option<std::net::Ipv4Addr>, String)> {
    let reply = match conn.call_method(
        Some("org.freedesktop.Avahi"),
        "/",
        Some("org.freedesktop.Avahi.Server"),
        "ResolveService",
        &(
            iface,
            proto,
            name,
            svc_type,
            domain,
            AVAHI_PROTO_UNSPEC,
            AVAHI_NO_FLAGS,
        ),
    ) {
        Ok(r) => r,
        Err(e) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[avahi]: ResolveService error ({e})"
            ));
            return None;
        }
    };

    type Resolved = (
        i32, i32, String, String, String, String,
        i32, String, u16, Vec<Vec<u8>>, u32,
    );
    let body: zbus::Result<Resolved> = reply.body().deserialize();
    let Ok((_, _, _, _, _, host, _, address, port, txt, _)) = body else {
        state.lock().unwrap().push_log(
            "mDNS[avahi]: ResolveService body parse failed".to_string(),
        );
        return None;
    };

    let (remote_cn, txt_ipv4, remote_session) = parse_dverse_txt(&txt);
    Some((host, address, port, remote_cn, txt_ipv4, remote_session))
}

/// Extract our DVerse-specific TXT entries (`cn=…`, `ip=…`, `session=…`)
/// from the raw byte vectors avahi hands us.
#[cfg(target_os = "linux")]
fn parse_dverse_txt(txt: &[Vec<u8>]) -> (String, Option<std::net::Ipv4Addr>, String) {
    let cn_prefix = format!("{TXT_KEY_CN}=");
    let ip_prefix = format!("{TXT_KEY_IP}=");
    let session_prefix = format!("{TXT_KEY_SESSION}=");
    let mut cn = String::new();
    let mut ip = None;
    let mut session = String::new();
    for entry in txt {
        let Ok(s) = std::str::from_utf8(entry) else { continue };
        if let Some(rest) = s.strip_prefix(&cn_prefix) {
            cn = rest.to_string();
        } else if let Some(rest) = s.strip_prefix(&ip_prefix) {
            if let Ok(parsed) = rest.parse::<std::net::Ipv4Addr>() {
                ip = Some(parsed);
            }
        } else if let Some(rest) = s.strip_prefix(&session_prefix) {
            session = rest.to_string();
        }
    }
    (cn, ip, session)
}

/// Choose the address for the peer endpoint:
///   - Prefer TXT `ip=` if it's a routable IPv4.
///   - Otherwise parse the address avahi resolved (could be v4 or v6).
#[cfg(target_os = "linux")]
fn pick_peer_address(
    avahi_address: &str,
    txt_ipv4: Option<std::net::Ipv4Addr>,
    state: &Arc<Mutex<AppState>>,
) -> Option<std::net::IpAddr> {
    if let Some(ipv4) = txt_ipv4 {
        if !ipv4.is_loopback() && !ipv4.is_link_local() {
            return Some(std::net::IpAddr::V4(ipv4));
        }
    }
    match avahi_address.parse() {
        Ok(ip) => Some(ip),
        Err(_) => {
            state.lock().unwrap().push_log(format!(
                "mDNS[avahi]: addr parse failed for {avahi_address:?}"
            ));
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn handle_avahi_item_remove(
    msg: &zbus::Message,
    state: &Arc<Mutex<AppState>>,
    registry: &mut PeerRegistry,
) {
    let body: zbus::Result<(i32, i32, String, String, String, u32)> = msg.body().deserialize();
    let Ok((_, _, name, svc_type, _, _)) = body else { return };
    state.lock().unwrap().push_log(format!(
        "mDNS[avahi]: ItemRemove name={name:?} type={svc_type:?}"
    ));
    let key = peer_cache_key(&name, &svc_type);
    if let Some(endpoints) = registry.remove(&key) {
        state.lock().unwrap().push_log(format!(
            "Peer router left: {}",
            endpoints.first().map(String::as_str).unwrap_or(&key),
        ));
    }
}
