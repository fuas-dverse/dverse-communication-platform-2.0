use mdns_sd::{IfKind, ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

use crate::state::AppState;

const SERVICE_TYPE: &str = "_dverse._tcp.local.";

/// Registers this router as a `_dverse._tcp` DNS-SD service and browses for peers.
///
/// Backend selection:
///   - Linux:   try avahi D-Bus for both publish AND browse (so avahi-daemon is the
///              single mDNS responder on the host).  Fall back to mdns-sd if the
///              system bus or avahi is unavailable.
///   - Windows / macOS:  mdns-sd for both publish and browse — one `ServiceDaemon`
///              to avoid UDP-5353 socket conflicts between two daemons.
///
/// `peer_rx` carries the current set of peer connect-endpoints as
/// `tls/<ip>:<port>` strings ready for Zenoh `connect/endpoints`.
/// The service record and browse are withdrawn when this handle is dropped.
pub struct MdnsHandle {
    _inner: Inner,
    pub peer_rx: watch::Receiver<Vec<String>>,
}

impl MdnsHandle {
    pub fn publish(cn: &str, port: u16, state: &Arc<Mutex<AppState>>) -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            state.lock().unwrap().push_log("mDNS: trying avahi D-Bus backend…".to_string());
            if let Some(handle) = linux_avahi_start(cn, port, Arc::clone(state)) {
                return Some(handle);
            }
            state.lock().unwrap().push_log(
                "mDNS: avahi unavailable, falling back to mdns-sd".to_string(),
            );
        }
        #[cfg(not(target_os = "linux"))]
        state.lock().unwrap().push_log("mDNS: using mdns-sd backend".to_string());
        mdns_sd_start(cn, port, state)
    }
}

#[allow(dead_code)]
enum Inner {
    #[cfg(target_os = "linux")]
    Avahi {
        _publish_conn: zbus::blocking::Connection,
        _browse_conn: zbus::blocking::Connection,
    },
    MdnsSd(MdnsSdHandle),
}

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

/// Detect the machine's LAN IPv4 address (the one used to reach the internet).
/// Uses a UDP socket routing trick — no packet is actually sent.
fn detect_lan_ipv4() -> Option<std::net::Ipv4Addr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("1.1.1.1:53").ok()?;
    match sock.local_addr().ok()? {
        std::net::SocketAddr::V4(a) if !a.ip().is_loopback() && !a.ip().is_link_local() => {
            Some(*a.ip())
        }
        _ => None,
    }
}

fn mdns_sd_start(
    cn: &str,
    port: u16,
    state: &Arc<Mutex<AppState>>,
) -> Option<MdnsHandle> {
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

    configure_daemon(&daemon, state);

    let instance = format!("DVerse ({cn})");
    let host = format!("zenoh-{cn}.local.");
    let lan_ip = detect_lan_ipv4();

    // Embed the LAN IPv4 in the TXT record so peers that receive only an IPv6
    // address from mDNS can still build a valid tls/<ipv4>:<port> endpoint.
    let ip_str;
    let props: &[(&str, &str)] = if let Some(ip) = lan_ip {
        ip_str = ip.to_string();
        &[("cn", cn), ("ip", &ip_str)]
    } else {
        &[("cn", cn)]
    };

    state.lock().unwrap().push_log(format!(
        "mDNS[mdns-sd]: LAN IP={}",
        lan_ip
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));

    // Pin the service's A record to the LAN IPv4 explicitly.  Without this,
    // mdns-sd auto-populates A records from every active interface (loopback,
    // docker bridges, etc.) and the resulting set is unhelpful to remote peers.
    let svc_result = match lan_ip {
        Some(ip) => ServiceInfo::new(
            SERVICE_TYPE,
            &instance,
            &host,
            std::net::IpAddr::V4(ip),
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

    let (peer_tx, peer_rx) = watch::channel(vec![]);
    spawn_browse_thread(browse_rx, cn.to_string(), Arc::clone(state), peer_tx);

    Some(MdnsHandle {
        _inner: Inner::MdnsSd(MdnsSdHandle { daemon, fullname }),
        peer_rx,
    })
}

/// Apply interface filters to an mdns-sd daemon.
///
/// Disables IPv6 (Zenoh peer endpoints are IPv4-only) and any virtual or
/// link-local IPv4 adapters.  On Windows the Npcap Loopback Adapter (installed
/// by Wireshark) shows up as a link-local 169.254.x.x interface; if mdns-sd
/// joins its multicast group, PTR responses go out via Npcap and never reach
/// the real LAN.  On Linux the docker0 bridge and wireguard tunnels create
/// similar phantom interfaces that flood the multicast group with traffic
/// that no peer is on.
fn configure_daemon(daemon: &ServiceDaemon, state: &Arc<Mutex<AppState>>) {
    let _ = daemon.disable_interface(IfKind::IPv6);

    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        let mut disabled_names: std::collections::HashSet<String> = Default::default();

        for iface in &ifaces {
            let name_lower = iface.name.to_lowercase();
            let is_virtual = name_lower.contains("npcap")
                || name_lower.contains("loopback")
                || name_lower.contains("vethernet")
                || name_lower.contains("hyper-v")
                || name_lower.contains("vmware")
                || name_lower.contains("virtualbox")
                || name_lower.contains("docker")
                || name_lower.starts_with("br-")   // Docker overlay bridge networks
                || name_lower.starts_with("veth")  // Docker/container veth pairs
                || name_lower.starts_with("wg");   // WireGuard tunnels

            let is_link_local_v4 = matches!(
                &iface.addr,
                if_addrs::IfAddr::V4(v4) if v4.ip.is_link_local()
            );

            if (is_virtual || is_link_local_v4) && disabled_names.insert(iface.name.clone()) {
                state.lock().unwrap().push_log(format!(
                    "mDNS[mdns-sd]: disabling virtual/link-local iface {} ({})",
                    iface.name,
                    iface.ip(),
                ));
                let _ = daemon.disable_interface(IfKind::Name(iface.name.clone()));
            }
        }
    }
}

/// Spawn a background thread that drains a `ServiceEvent` channel and updates
/// `peer_tx` with the current set of reachable peer endpoints.
fn spawn_browse_thread(
    browse_rx: mdns_sd::Receiver<ServiceEvent>,
    my_cn: String,
    state: Arc<Mutex<AppState>>,
    peer_tx: watch::Sender<Vec<String>>,
) {
    std::thread::spawn(move || {
        let mut peers: HashMap<String, Vec<String>> = HashMap::new();

        while let Ok(event) = browse_rx.recv() {
            match event {
                ServiceEvent::ServiceFound(svc_type, fullname) => {
                    state.lock().unwrap().push_log(format!(
                        "mDNS[mdns-sd]: found {fullname:?} (type={svc_type:?})"
                    ));
                }
                ServiceEvent::ServiceResolved(info) => {
                    let remote_cn = info.get_property_val_str("cn").unwrap_or_default();
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
                        continue;
                    }

                    let port = info.get_port();

                    // Prefer the IPv4 embedded in the TXT record (set by the remote
                    // router at publish time). This sidesteps the problem where mdns-sd
                    // resolves via IPv6 first and only reports the link-local AAAA address.
                    let txt_ipv4 = info
                        .get_property_val_str("ip")
                        .and_then(|s| s.parse::<std::net::Ipv4Addr>().ok())
                        .filter(|ip| !ip.is_loopback() && !ip.is_link_local());

                    let mut endpoints: Vec<String> = if let Some(ipv4) = txt_ipv4 {
                        state.lock().unwrap().push_log(format!(
                            "mDNS[mdns-sd]: using TXT ip={ipv4} for {remote_cn}"
                        ));
                        vec![format!("tls/{ipv4}:{port}")]
                    } else {
                        info.get_addresses()
                            .iter()
                            .filter_map(|a| {
                                if is_unroutable(a) {
                                    state.lock().unwrap().push_log(format!(
                                        "mDNS[mdns-sd]: skipping unroutable addr {a}"
                                    ));
                                    return None;
                                }
                                match a {
                                    std::net::IpAddr::V4(v4) => {
                                        Some(format!("tls/{v4}:{port}"))
                                    }
                                    _ => None,
                                }
                            })
                            .collect()
                    };
                    endpoints.sort();

                    if endpoints.is_empty() {
                        state.lock().unwrap().push_log(format!(
                            "mDNS[mdns-sd]: resolved {remote_cn} but no routable IPv4 \
                             (no TXT ip, addrs={:?}); skipping",
                            info.get_addresses().iter().collect::<Vec<_>>(),
                        ));
                        continue;
                    }

                    if peers.get(info.get_fullname()) == Some(&endpoints) {
                        state.lock().unwrap().push_log(format!(
                            "mDNS[mdns-sd]: peer {remote_cn} unchanged, ignoring duplicate"
                        ));
                        continue;
                    }

                    peers.insert(info.get_fullname().to_string(), endpoints.clone());
                    state.lock().unwrap().push_log(format!(
                        "Discovered peer router: {} (+{} addr) (CN={remote_cn})",
                        endpoints[0],
                        endpoints.len() - 1,
                    ));
                    let _ = peer_tx.send(peers.values().flatten().cloned().collect());
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    state.lock().unwrap().push_log(format!(
                        "mDNS[mdns-sd]: removed {fullname:?}"
                    ));
                    if let Some(endpoints) = peers.remove(&fullname) {
                        state.lock().unwrap().push_log(format!(
                            "Peer router left: {}",
                            endpoints.first().map(String::as_str).unwrap_or(&fullname),
                        ));
                        let _ = peer_tx.send(peers.values().flatten().cloned().collect());
                    }
                }
                other => {
                    state.lock().unwrap().push_log(format!(
                        "mDNS[mdns-sd]: event {other:?}"
                    ));
                }
            }
        }

        state.lock().unwrap().push_log("mDNS[mdns-sd]: browse loop exited".to_string());
    });
}

fn is_unroutable(addr: &std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

// ── Linux: avahi D-Bus (publish + browse) ────────────────────────────────────

#[cfg(target_os = "linux")]
fn linux_avahi_start(cn: &str, port: u16, state: Arc<Mutex<AppState>>) -> Option<MdnsHandle> {
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

    if let Err(e) = linux_avahi_publish(cn, port, &publish_conn) {
        state.lock().unwrap().push_log(format!("mDNS[avahi]: publish failed ({e})"));
        return None;
    }
    state.lock().unwrap().push_log(format!(
        "mDNS[avahi]: published DVerse ({cn}) on {SERVICE_TYPE} port {port} addr={}",
        detect_lan_ipv4()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));

    match linux_avahi_browse(cn, Arc::clone(&state)) {
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

#[cfg(target_os = "linux")]
fn linux_avahi_publish(
    cn: &str,
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

    let name = format!("DVerse ({cn})");
    let local_ip = detect_lan_ipv4();

    // Embed the LAN IPv4 in the TXT record so remote peers that resolve via IPv6
    // first (or via the machine's default hostname) can still build a usable
    // tls/<ipv4>:<port> endpoint.
    let mut txt: Vec<Vec<u8>> = vec![format!("cn={cn}").into_bytes()];
    if let Some(ipv4) = local_ip {
        txt.push(format!("ip={ipv4}").into_bytes());
    }

    // Best-effort: register a dedicated A record `dverse-<cn>.local.` pointing at
    // the LAN IPv4, so SRV resolution gives a single, routable address.  Fails
    // with a CollisionError if a stale record exists from a previous run — fall
    // back to avahi's default hostname in that case.
    let custom_host = format!("dverse-{cn}.local.");
    let use_custom = if let Some(ipv4) = local_ip {
        let res = conn.call_method(
            Some("org.freedesktop.Avahi"),
            group_path.as_str(),
            Some("org.freedesktop.Avahi.EntryGroup"),
            "AddAddress",
            &(-1i32, 0i32, 0u32, custom_host.as_str(), ipv4.to_string().as_str()),
        );
        match res {
            Ok(_) => true,
            Err(e) => {
                eprintln!(
                    "mDNS[avahi]: AddAddress for {custom_host} failed ({e}); using default host"
                );
                false
            }
        }
    } else {
        false
    };

    let host = if use_custom { custom_host.as_str() } else { "" };

    conn.call_method(
        Some("org.freedesktop.Avahi"),
        group_path.as_str(),
        Some("org.freedesktop.Avahi.EntryGroup"),
        "AddService",
        &(-1i32, -1i32, 0u32, name.as_str(), "_dverse._tcp", "", host, port, &txt),
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

/// Browse `_dverse._tcp` via avahi's D-Bus `ServiceBrowser` interface.
///
/// Critical ordering: the `MessageIterator` (D-Bus `AddMatch`) MUST be registered
/// BEFORE `ServiceBrowserNew` is called.  avahi fires `ItemNew` signals for any
/// already-cached services immediately after the browser is created; if we only
/// set up the subscription afterwards we miss those signals.
///
/// Implementation: spawn the thread first, register a broad match rule from
/// inside the thread (no path filter — we don't know the browser path yet),
/// THEN call `ServiceBrowserNew`, and filter in code by browser_path.  A
/// `mpsc::channel` synchronises the caller so it waits for setup to complete.
#[cfg(target_os = "linux")]
fn linux_avahi_browse(
    my_cn: &str,
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
    let (peer_tx, peer_rx) = watch::channel(vec![]);
    let my_cn = my_cn.to_string();

    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<String, String>>();

    std::thread::spawn(move || {
        // Step 1: register the broad match rule BEFORE creating the browser.
        // No `.path()` filter — we don't know the browser's path yet, and we
        // must not miss signals fired during/immediately after browser creation.
        let rule = match (|| -> zbus::Result<_> {
            Ok(zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .sender("org.freedesktop.Avahi")?
                .interface("org.freedesktop.Avahi.ServiceBrowser")?
                .build())
        })() {
            Ok(r) => r,
            Err(e) => {
                let msg = format!("mDNS[avahi]: match rule error ({e})");
                state.lock().unwrap().push_log(msg.clone());
                ready_tx.send(Err(msg)).ok();
                return;
            }
        };

        let iter = match zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(64)) {
            Ok(i) => i,
            Err(e) => {
                let msg = format!("mDNS[avahi]: iterator error ({e})");
                state.lock().unwrap().push_log(msg.clone());
                ready_tx.send(Err(msg)).ok();
                return;
            }
        };

        // Step 2: NOW create the browser — signals are already queued.
        let browser_path: zbus::zvariant::OwnedObjectPath = match conn
            .call_method(
                Some("org.freedesktop.Avahi"),
                "/",
                Some("org.freedesktop.Avahi.Server"),
                "ServiceBrowserNew",
                &(-1i32, -1i32, "_dverse._tcp", "local", 0u32),
            )
            .and_then(|r| r.body().deserialize().map_err(Into::into))
        {
            Ok(p) => p,
            Err(e) => {
                let msg = format!("mDNS[avahi]: ServiceBrowserNew failed ({e})");
                state.lock().unwrap().push_log(msg.clone());
                ready_tx.send(Err(msg)).ok();
                return;
            }
        };

        let browser_path_str = browser_path.as_str().to_string();
        state
            .lock()
            .unwrap()
            .push_log(format!("mDNS[avahi]: browser at {browser_path_str}"));
        ready_tx.send(Ok(browser_path_str.clone())).ok();

        let mut peers: HashMap<String, Vec<String>> = HashMap::new();

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

            // Filter to signals from our specific ServiceBrowser object — the
            // broad match rule may also deliver signals from other browsers if
            // multiple `ServiceBrowserNew` instances exist on the bus.
            let msg_path = msg.header().path().map(|p| p.as_str().to_owned());
            if msg_path.as_deref() != Some(browser_path_str.as_str()) {
                continue;
            }

            let Some(member) = msg.header().member().map(|m| m.to_owned()) else { continue };

            match member.as_str() {
                "ItemNew" => {
                    let body: zbus::Result<(i32, i32, String, String, String, u32)> =
                        msg.body().deserialize();
                    let Ok((svc_iface, svc_proto, name, svc_type, domain, _)) = body else {
                        state
                            .lock()
                            .unwrap()
                            .push_log("mDNS[avahi]: ItemNew body parse failed".to_string());
                        continue;
                    };

                    state.lock().unwrap().push_log(format!(
                        "mDNS[avahi]: ItemNew iface={svc_iface} proto={svc_proto} \
                         name={name:?} type={svc_type:?} domain={domain:?}"
                    ));

                    let resolve = conn_resolve.call_method(
                        Some("org.freedesktop.Avahi"),
                        "/",
                        Some("org.freedesktop.Avahi.Server"),
                        "ResolveService",
                        &(
                            svc_iface,
                            svc_proto,
                            name.as_str(),
                            svc_type.as_str(),
                            domain.as_str(),
                            -1i32,
                            0u32,
                        ),
                    );
                    let reply = match resolve {
                        Ok(r) => r,
                        Err(e) => {
                            state.lock().unwrap().push_log(format!(
                                "mDNS[avahi]: ResolveService error ({e})"
                            ));
                            continue;
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
                        continue;
                    };

                    let remote_cn: String = txt
                        .iter()
                        .find_map(|e| {
                            std::str::from_utf8(e)
                                .ok()
                                .and_then(|s| s.strip_prefix("cn="))
                                .map(str::to_string)
                        })
                        .unwrap_or_default();
                    let txt_ipv4: Option<std::net::Ipv4Addr> = txt.iter().find_map(|e| {
                        std::str::from_utf8(e)
                            .ok()
                            .and_then(|s| s.strip_prefix("ip="))
                            .and_then(|s| s.parse().ok())
                    });

                    state.lock().unwrap().push_log(format!(
                        "mDNS[avahi]: resolved host={host:?} addr={address} port={port} \
                         cn={remote_cn:?} txt_ip={txt_ipv4:?}"
                    ));

                    if remote_cn.is_empty() || remote_cn == my_cn {
                        state.lock().unwrap().push_log(format!(
                            "mDNS[avahi]: skipping self or empty CN ({remote_cn:?})"
                        ));
                        continue;
                    }

                    // Prefer the IPv4 from the TXT record (peer published their own
                    // routable LAN IP).  Fall back to the resolved A/AAAA address.
                    let chosen_ip: std::net::IpAddr = if let Some(ipv4) = txt_ipv4 {
                        if !ipv4.is_loopback() && !ipv4.is_link_local() {
                            std::net::IpAddr::V4(ipv4)
                        } else {
                            match address.parse() {
                                Ok(ip) => ip,
                                Err(_) => {
                                    state.lock().unwrap().push_log(format!(
                                        "mDNS[avahi]: addr parse failed for {address:?}"
                                    ));
                                    continue;
                                }
                            }
                        }
                    } else {
                        match address.parse() {
                            Ok(ip) => ip,
                            Err(_) => {
                                state.lock().unwrap().push_log(format!(
                                    "mDNS[avahi]: addr parse failed for {address:?}"
                                ));
                                continue;
                            }
                        }
                    };

                    if is_unroutable(&chosen_ip) {
                        state.lock().unwrap().push_log(format!(
                            "mDNS[avahi]: skipping unroutable addr {chosen_ip}"
                        ));
                        continue;
                    }

                    let endpoint = format!("tls/{chosen_ip}:{port}");
                    let key = format!("{name}.{svc_type}.");

                    let entry = peers.entry(key).or_default();
                    if !entry.contains(&endpoint) {
                        entry.push(endpoint.clone());
                        entry.sort();
                        state.lock().unwrap().push_log(format!(
                            "Discovered peer router: {endpoint} (CN={remote_cn})"
                        ));
                        let _ = peer_tx.send(peers.values().flatten().cloned().collect());
                    }
                }
                "ItemRemove" => {
                    let body: zbus::Result<(i32, i32, String, String, String, u32)> =
                        msg.body().deserialize();
                    let Ok((_, _, name, svc_type, _, _)) = body else { continue };
                    state.lock().unwrap().push_log(format!(
                        "mDNS[avahi]: ItemRemove name={name:?} type={svc_type:?}"
                    ));
                    let key = format!("{name}.{svc_type}.");
                    if let Some(endpoints) = peers.remove(&key) {
                        state.lock().unwrap().push_log(format!(
                            "Peer router left: {}",
                            endpoints.first().map(String::as_str).unwrap_or(&key),
                        ));
                        let _ = peer_tx.send(peers.values().flatten().cloned().collect());
                    }
                }
                _ => {}
            }
        }

        state.lock().unwrap().push_log("mDNS[avahi]: signal loop exited".to_string());
    });

    match ready_rx.recv() {
        Ok(Ok(_)) => Some((conn_keepalive, peer_rx)),
        Ok(Err(_)) | Err(_) => None,
    }
}
