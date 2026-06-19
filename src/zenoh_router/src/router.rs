use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use bot_framework::announce::AgentAnnounce;
use bot_framework::cert;
use bot_framework::config::{DverseConfig, SessionRole};
use crate::state::SessionCryptoState;
use tokio::sync::watch;
use tracing::{error, info, warn};
use zenoh::Session;

use crate::constants::AGENT_HEARTBEAT_INTERVAL;
use crate::discovery::MdnsHandle;
use crate::state::{AppState, RouterStatus};

// ── Router background task ────────────────────────────────────────────────────

/// Background entry point.  Waits for a `DverseConfig` to appear in AppState,
/// acquires/reuses the router cert, then runs the Zenoh router indefinitely,
/// restarting the session whenever the admitted ACL changes.
pub async fn run(state: Arc<Mutex<AppState>>) {
    let mut current_cfg: Option<DverseConfig> = None;
    // Kept alive for the entire router lifetime; dropped (→ mDNS record withdrawn) on exit.
    let mut _mdns: Option<MdnsHandle> = None;
    // Carries the current set of discovered peer endpoints; starts empty.
    let (_, mut peer_rx): (_, watch::Receiver<Vec<String>>) = watch::channel(vec![]);
    // Spawned once on first Running; kept across session restarts because
    // eviction is wall-clock driven and independent of which Zenoh session
    // is live.
    let mut reaper_started = false;

    loop {
        // ── Phase 1: obtain config ───────────────────────────────────────────
        //
        // Three states feed into the next session boot:
        //   * `staged_config` has a freshly-staged value (login → pick_session,
        //     possibly mid-run) — preferred over a stale `current_cfg`.
        //   * `current_cfg` holds the previous-iteration config — used when the
        //     session restarted for an ACL/peer change but the role didn't
        //     change.
        //   * Neither — first boot, wait for the user to log in.
        let staged_now = state.lock().unwrap().staged_config.take();
        let (cfg, fresh) = if let Some(new_cfg) = staged_now {
            (new_cfg, true)
        } else if let Some(c) = current_cfg.take() {
            (c, false)
        } else {
            info!("waiting for configuration");
            let cfg = loop {
                {
                    let mut s = state.lock().unwrap();
                    if let Some(cfg) = s.staged_config.take() {
                        break cfg;
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            };
            (cfg, true)
        };

        if fresh {
            // Wipe per-session state so leftovers from a previous
            // pick_session can't leak into the new role.
            let session_id = cfg.session_id();
            let is_admin = matches!(cfg.session_role, SessionRole::Admin);
            {
                let mut s = state.lock().unwrap();
                s.session_role = cfg.session_role.clone();
                s.session_id = session_id.clone();
                s.crypto = Some(SessionCryptoState::new(is_admin));
                s.admitted.clear();
                s.pending_requests.clear();
                s.banned_cns.clear();
                s.join_flow = None;
                s.connected_nodes.clear();
            }
            // Drop mDNS so its TXT (cn=, session=) re-publishes with the new
            // session_id. The new handle is constructed below.
            _mdns = None;
        }

        // (Re)publish mDNS if needed.
        if _mdns.is_none() {
            if let Some(handle) = MdnsHandle::publish(
                &cfg.operator_cn(),
                &cfg.session_id(),
                crate::constants::ROUTER_PORT,
            ) {
                peer_rx = handle.peer_rx.clone();
                let vis_state = Arc::clone(&state);
                let mut vis_rx = handle.sessions_rx.clone();
                tokio::spawn(async move {
                    loop {
                        {
                            let v = vis_rx.borrow_and_update().clone();
                            vis_state.lock().unwrap().visible_sessions = v;
                        }
                        if vis_rx.changed().await.is_err() {
                            break;
                        }
                    }
                });
                _mdns = Some(handle);
            }
        }

        // Pre-admit operator's CN so all local agents can communicate immediately.
        // When joining someone else's session, pre-admit the admin's CN too,
        // so the admin's router (which carries that cert) can connect and
        // form the mesh before we've heard a heartbeat from any of their agents.
        let operator_cn = cfg.operator_cn();
        let (newly_admitted_operator, newly_admitted_admin) = {
            let mut st = state.lock().unwrap();
            let added_op = if !st.admitted.contains(&operator_cn) {
                st.admitted.push(operator_cn.clone());
                true
            } else {
                false
            };
            let added_admin = if let SessionRole::Client { admin_cn } = &cfg.session_role {
                if !admin_cn.is_empty() && !st.admitted.contains(admin_cn) {
                    st.admitted.push(admin_cn.clone());
                    Some(admin_cn.clone())
                } else {
                    None
                }
            } else {
                None
            };
            (added_op, added_admin)
        };
        if newly_admitted_operator {
            info!(cn = %operator_cn, "pre-admitted operator CN");
        }
        if let Some(admin_cn) = newly_admitted_admin {
            info!(cn = %admin_cn, "pre-admitted session admin CN");
        }

        // ── Phase 2: bootstrap CA root, then acquire/reuse cert ─────────────
        state.lock().unwrap().router_status = RouterStatus::Acquiring;
        info!("bootstrapping CA root certificate");

        let ca_root_path = std::path::PathBuf::from(&cfg.ca_root_pem_path);
        if let Err(e) = cert::bootstrap_ca_root(&cfg.ca_url, &ca_root_path).await {
            state.lock().unwrap().router_status = RouterStatus::Error(e.to_string());
            warn!(error = %e, ca_url = %cfg.ca_url, "CA root bootstrap failed");
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        info!("checking router certificate");

        let (cert_p, key_p, ca_p) = match acquire_or_reuse(&cfg).await {
            Ok(paths) => paths,
            Err(e) => {
                state.lock().unwrap().router_status = RouterStatus::Error(e.to_string());
                error!(error = %e, "router certificate acquire/renew failed");
                // Don't retry automatically — wait for the user to fix config.
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        // ── Phase 3: run session loop (restarts on ACL or peer change) ──────
        let admitted = state.lock().unwrap().admitted.clone();
        let peers = peer_rx.borrow().clone();
        info!(
            listen = %cfg.router_listen,
            peer_count = peers.len(),
            admitted_count = admitted.len(),
            cert = %cert_p.display(),
            key = %key_p.display(),
            ca = %ca_p.display(),
            "building Zenoh config",
        );
        if !peers.is_empty() {
            info!(?peers, "connecting to peer routers");
        }
        let config = match build_zenoh_config(&cfg.router_listen, &ca_p, &cert_p, &key_p, &admitted, &peers, &cfg.session_id()) {
            Ok(c) => c,
            Err(e) => {
                state.lock().unwrap().router_status = RouterStatus::Error(e.to_string());
                error!(error = %e, "build_zenoh_config failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
                current_cfg = Some(cfg);
                continue;
            }
        };

        state.lock().unwrap().router_status = RouterStatus::Starting;
        info!(listen = %cfg.router_listen, "opening Zenoh router");

        let session = match zenoh::open(config).await {
            Ok(s) => s,
            Err(e) => {
                state.lock().unwrap().router_status = RouterStatus::Error(e.to_string());
                error!(error = %e, "zenoh::open failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
                current_cfg = Some(cfg);
                continue;
            }
        };

        {
            let mut st = state.lock().unwrap();
            st.router_status = RouterStatus::Running;
            st.zenoh_session = Some(session.clone());
        }
        info!(zid = %session.zid(), "router session running");

        // Admission control plane: handles incoming JoinRequests (admin) and
        // AdmissionDecisions (requester) for the lifetime of this Zenoh
        // session. Aborted on session restart so it doesn't outlive its
        // Session handle.
        let admission_handle = {
            let s = session.clone();
            let st = Arc::clone(&state);
            let cn = operator_cn.clone();
            tokio::spawn(async move {
                if let Err(e) = crate::admission_handler::run(s, st, cn).await {
                    warn!(error = %e, "admission control plane exited");
                }
            })
        };

        // Client role: auto-(re)send a JoinRequest. We republish on TWO
        // schedules so the request can't get stuck in dead air:
        //   (a) Every time we enter Running (session restart, ACL reload).
        //   (b) Periodically while still Pending — covers the case where
        //       a peer link comes up *during* a session (e.g. inbound from
        //       the admin's router) and our original put landed before
        //       routes existed for it.
        // Stops on terminal states (Allowed = we have the Megolm key;
        // Denied = no point retrying).
        use crate::state::JoinFlowStatus;
        let mut republish_handle: Option<tokio::task::JoinHandle<()>> = None;
        if let SessionRole::Client { admin_cn } = &cfg.session_role {
            if !admin_cn.is_empty() {
                let needs_request = !matches!(
                    state.lock().unwrap().join_flow,
                    Some(JoinFlowStatus::Allowed) | Some(JoinFlowStatus::Denied { .. })
                );
                if needs_request {
                    if let Err(e) = crate::admission_handler::request_join(
                        &session,
                        &*state,
                        &operator_cn,
                        admin_cn,
                        &cert_p,
                        &key_p,
                        None,
                    )
                    .await
                    {
                        warn!(error = %e, "auto JoinRequest failed");
                    }
                }

                // Periodic republish task. Holds clones of session + state
                // and re-puts the JoinRequest every few seconds until we
                // reach a terminal state. Aborted on session restart so the
                // next loop iteration spawns a fresh one against the new
                // session.
                let s = session.clone();
                let st = Arc::clone(&state);
                let my_cn = operator_cn.clone();
                let admin_cn = admin_cn.clone();
                let cert_p = cert_p.clone();
                let key_p = key_p.clone();
                republish_handle = Some(tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        let terminal = matches!(
                            st.lock().unwrap().join_flow,
                            Some(JoinFlowStatus::Allowed)
                                | Some(JoinFlowStatus::Denied { .. })
                                | None
                        );
                        if terminal {
                            break;
                        }
                        if let Err(e) = crate::admission_handler::request_join(
                            &s, &*st, &my_cn, &admin_cn, &cert_p, &key_p, None,
                        )
                        .await
                        {
                            warn!(error = %e, "periodic republish of JoinRequest failed");
                        }
                    }
                }));
            }
        }

        // Wall-clock stale-eviction task — spawned once.  Drives the
        // Online→Degraded→Offline ladder and removes agents whose heartbeats
        // stop landing.  Independent of the Zenoh session lifecycle, so it
        // outlives ACL/peer restarts.
        if !reaper_started {
            let state_for_reap = Arc::clone(&state);
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(AGENT_HEARTBEAT_INTERVAL);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tick.tick().await;
                    let now = Instant::now();
                    let evicted = {
                        let mut st = state_for_reap.lock().unwrap();
                        st.reap_stale(now)
                    };
                    for (cn, name) in evicted {
                        info!(cn = %cn, agent = %name, "agent stale-evicted");
                    }
                }
            });
            reaper_started = true;
        }

        // session_loop returns when the admitted list or peer set changes.
        if let Err(e) = session_loop(&session, Arc::clone(&state), peer_rx.clone()).await {
            warn!(error = %e, "session loop errored");
        }

        admission_handle.abort();
        if let Some(h) = republish_handle.take() {
            h.abort();
        }
        state.lock().unwrap().zenoh_session = None;
        let _ = session.close().await;

        state.lock().unwrap().router_status = RouterStatus::Reloading;
        info!("reloading router with updated ACL");

        current_cfg = Some(cfg);
    }
}

/// Use the cached cert if it is still fresh; otherwise acquire a new one.
async fn acquire_or_reuse(
    cfg: &DverseConfig,
) -> Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)> {
    let c = cert::cert_path(&cfg.cert_dir, "router");
    let k = cert::key_path(&cfg.cert_dir, "router");
    let ca = cert::ca_path(&cfg.cert_dir, "router");

    let max_age = Duration::from_secs(23 * 3600);
    if cert::needs_renewal(&c, max_age, Some(&cfg.operator_cn())).await {
        let cert_cfg = cfg.cert_config_for("router")?;
        cert::acquire(&cert_cfg).await
    } else {
        Ok((c, k, ca))
    }
}

/// Subscribe to node announce messages and feed the JSON payload into the
/// agent inventory.  Returns when the admitted list (driven by the explicit
/// admission flow — issue #110) or the peer set changes (triggering a
/// session restart).
///
/// Each announce key looks like `dverse/nodes/announce/<cn>/agents/<name>`
/// which stays inside the existing `dverse/nodes/announce/**` ACL rule.
async fn session_loop(
    session: &Session,
    state: Arc<Mutex<AppState>>,
    mut peer_rx: watch::Receiver<Vec<String>>,
) -> Result<()> {
    let subscriber = session
        .declare_subscriber("dverse/nodes/announce/**")
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    let admitted_changed = state.lock().unwrap().admitted_changed.clone();
    let config_changed = state.lock().unwrap().config_changed.clone();

    // Mark current peer set as seen so the first .changed() fires only on a real change.
    peer_rx.borrow_and_update();

    // Log-once-per-CN dedup for unparseable payloads (legacy plain-CN puts).
    let mut seen_legacy: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            msg = subscriber.recv_async() => {
                match msg {
                    Ok(s) => {
                        let bytes = s.payload().to_bytes();
                        let ann: AgentAnnounce = match serde_json::from_slice(&bytes) {
                            Ok(a) => a,
                            Err(_) => {
                                // Old / unparseable payload — log once per CN so a
                                // stale agent on the network doesn't flood the log.
                                let key = s.key_expr().as_str().to_string();
                                let cn_for_log = key
                                    .strip_prefix("dverse/nodes/announce/")
                                    .and_then(|tail| tail.split('/').next())
                                    .unwrap_or("?")
                                    .to_string();
                                if seen_legacy.insert(cn_for_log.clone()) {
                                    warn!(cn = %cn_for_log, "ignored unparseable announce");
                                }
                                continue;
                            }
                        };

                        let cn = ann.cn.clone();
                        if cn.is_empty() { continue; }
                        let now = Instant::now();
                        // Agent inventory only — admission is now explicit (#110).
                        // Receiving a heartbeat from an unadmitted CN updates the
                        // GUI but does NOT grant them session access.
                        state.lock().unwrap().upsert_agent(&ann, now);
                    }
                    Err(e) => return Err(anyhow::anyhow!("subscriber recv: {e}")),
                }
            }
            _ = admitted_changed.notified() => {
                info!("admitted list changed, reloading router");
                return Ok(());
            }
            _ = config_changed.notified() => {
                info!("config changed (logout + new pick_session), restarting under new role");
                return Ok(());
            }
            Ok(()) = peer_rx.changed() => {
                info!("peer set changed, reloading router");
                return Ok(());
            }
        }
    }
}

fn build_zenoh_config(
    listen_addr: &str,
    ca_path: &std::path::Path,
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
    admitted: &[String],
    peers: &[String],
    namespace: &str,
) -> Result<zenoh::Config> {
    let mut cfg = zenoh::Config::default();

    zinsert(&mut cfg, "mode", "\"router\"")?;
    zinsert(&mut cfg, "listen/endpoints", &format!("[\"{listen_addr}\"]"))?;
    zinsert(&mut cfg, "scouting/multicast/enabled", "false")?;
    // Session isolation on the shared fabric: every key this session pub/subs is
    // transparently prefixed with the namespace, so routers + agents of one
    // session never see another session's traffic even though all routers mesh.
    // Agents must use the same namespace (see NodeConfig::with_namespace).
    if !namespace.is_empty() {
        zinsert(&mut cfg, "namespace", &json_str(namespace))?;
    }
    zinsert(&mut cfg, "transport/link/tls/root_ca_certificate", &json_str(&ca_path.to_string_lossy()))?;
    zinsert(&mut cfg, "transport/link/tls/enable_mtls", "true")?;
    tls_identity(&mut cfg, cert_path, key_path)?;
    zinsert(&mut cfg, "access_control", &build_acl_json(admitted, namespace))?;

    if !peers.is_empty() {
        let endpoints_json = serde_json::to_string(peers).unwrap();
        zinsert(&mut cfg, "connect/endpoints", &endpoints_json)?;
        // Peer endpoints are IPs from DNS-SD; skip SNI hostname check.
        // mTLS CA verification is still enforced on both sides.
        zinsert(&mut cfg, "transport/link/tls/verify_name_on_connect", "false")?;
    }

    Ok(cfg)
}

fn build_acl_json(admitted: &[String], namespace: &str) -> String {
    // TODO(#112): once agent payloads are AEAD-wrapped end-to-end, liberalize
    // the main-rule to allow `dverse/**` for `any` cert holder (the plan's
    // stated security model: crypto = membership, ACL = fabric). Until then
    // we keep CN allow-listing so non-members can't even publish on the
    // payload plane, even though they couldn't read it anyway.
    //
    // Zenoh applies the session namespace at the face boundary and the ACL
    // interceptor sees the *namespaced* key, so the rule key_exprs must carry
    // the same prefix the namespace adds.
    let prefix = if namespace.is_empty() {
        String::new()
    } else {
        format!("{namespace}/")
    };
    let announce_key = format!("{prefix}dverse/nodes/announce/**");
    let session_key = format!("{prefix}dverse/session/**");
    let main_key = format!("{prefix}dverse/**");
    let announce_msgs = serde_json::json!(["put", "delete", "declare_subscriber"]);
    let main_msgs = serde_json::json!(["put", "delete", "declare_subscriber", "query", "reply", "declare_queryable"]);
    // session-rule (admission control plane): allow ANY cert holder to
    // pub/sub on `dverse/session/**`. A requester isn't in `admitted` when
    // they send their first `JoinRequest`, and the admin's reply is sealed
    // by Olm to the requester — so security on this topic comes from crypto,
    // not from CN allow-listing (issue #110).
    let session_rule = serde_json::json!({
        "id": "session-rule",
        "messages": main_msgs,
        "flows": ["ingress", "egress"],
        "permission": "allow",
        "key_exprs": [session_key]
    });

    if admitted.is_empty() {
        serde_json::json!({
            "enabled": true,
            "default_permission": "deny",
            "rules": [
                {
                    "id": "announce-rule",
                    "messages": announce_msgs,
                    "flows": ["ingress", "egress"],
                    "permission": "allow",
                    "key_exprs": [announce_key]
                },
                session_rule
            ],
            "subjects": [
                { "id": "any" }
            ],
            "policies": [
                { "rules": ["announce-rule", "session-rule"], "subjects": ["any"] }
            ]
        })
        .to_string()
    } else {
        serde_json::json!({
            "enabled": true,
            "default_permission": "deny",
            "rules": [
                {
                    "id": "announce-rule",
                    "messages": announce_msgs,
                    "flows": ["ingress", "egress"],
                    "permission": "allow",
                    "key_exprs": [announce_key]
                },
                session_rule,
                {
                    "id": "main-rule",
                    "messages": main_msgs,
                    "flows": ["ingress", "egress"],
                    "permission": "allow",
                    "key_exprs": [main_key]
                }
            ],
            "subjects": [
                { "id": "any" },
                { "id": "admitted", "cert_common_names": admitted }
            ],
            "policies": [
                { "rules": ["announce-rule", "session-rule"], "subjects": ["any"] },
                { "rules": ["main-rule"], "subjects": ["admitted"] }
            ]
        })
        .to_string()
    }
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn zinsert(cfg: &mut zenoh::Config, key: &str, value: &str) -> Result<()> {
    cfg.insert_json5(key, value)
        .map_err(|e| anyhow::anyhow!("zenoh config key '{}': {}", key, e))
}

/// Write the router's mTLS identity into both halves of Zenoh's TLS config in
/// one call.  A router has exactly one identity, but the Zenoh config surface
/// exposes it as two pairs (listener and connect side) that have to stay in
/// sync by convention — wrapping the four writes in a single helper makes it
/// structurally impossible to update one half and forget the other.
fn tls_identity(
    cfg: &mut zenoh::Config,
    cert_path: &std::path::Path,
    key_path: &std::path::Path,
) -> Result<()> {
    let cert = json_str(&cert_path.to_string_lossy());
    let key = json_str(&key_path.to_string_lossy());
    // Listener side: cert presented to peers dialling us.
    zinsert(cfg, "transport/link/tls/listen_certificate", &cert)?;
    zinsert(cfg, "transport/link/tls/listen_private_key", &key)?;
    // Connect side: cert presented when we dial a peer router (peer routers
    // run `enable_mtls=true` and require a valid client cert; without these
    // the outgoing handshake stalls and inter-router forwarding never lights
    // up — see ADR-017).
    zinsert(cfg, "transport/link/tls/connect_certificate", &cert)?;
    zinsert(cfg, "transport/link/tls/connect_private_key", &key)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// A mode=router session must open cleanly with a `namespace` set and do a
    /// namespaced self pub/sub round-trip — i.e. namespacing the router doesn't
    /// break its startup or local routing (the shared-fabric isolation, #108).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn router_opens_and_routes_with_namespace() {
        use std::time::Duration;

        let mut cfg = zenoh::Config::default();
        cfg.insert_json5("mode", "\"router\"").unwrap();
        // Ephemeral port so the test never collides with a running router.
        cfg.insert_json5("listen/endpoints", "[\"tcp/127.0.0.1:0\"]").unwrap();
        cfg.insert_json5("scouting/multicast/enabled", "false").unwrap();
        cfg.insert_json5("namespace", "\"test-session\"").unwrap();

        let session = zenoh::open(cfg).await.expect("router opens with namespace");

        // Code uses un-namespaced keys; the namespace is applied transparently,
        // so a self pub/sub on "dverse/agents/ping" must still match.
        let sub = session
            .declare_subscriber("dverse/agents/ping")
            .await
            .expect("declare subscriber");
        tokio::time::sleep(Duration::from_millis(200)).await;
        session
            .put("dverse/agents/ping", "hello")
            .await
            .expect("put");

        let sample = tokio::time::timeout(Duration::from_secs(2), sub.recv_async())
            .await
            .expect("recv did not time out")
            .expect("got a sample");
        assert_eq!(sample.payload().try_to_string().unwrap().as_ref(), "hello");

        session.close().await.unwrap();
    }
}
