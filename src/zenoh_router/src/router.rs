use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use bot_framework::cert;
use bot_framework::config::DverseConfig;
use zenoh::Session;

use crate::state::{Action, AppState, RouterStatus};

/// Background entry point.  Waits for a `DverseConfig` to appear in AppState,
/// acquires/reuses the router cert, then runs the Zenoh router indefinitely,
/// restarting the session whenever the admitted ACL changes.
pub async fn run(state: Arc<Mutex<AppState>>) {
    let mut current_cfg: Option<DverseConfig> = None;

    loop {
        // ── Phase 1: obtain config ───────────────────────────────────────────
        let cfg = if let Some(c) = current_cfg.take() {
            c
        } else {
            state.lock().unwrap().push_log("Waiting for configuration…");
            loop {
                {
                    let mut s = state.lock().unwrap();
                    if let Some(cfg) = s.staged_config.take() {
                        break cfg;
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        };

        // ── Phase 2: bootstrap CA root, then acquire/reuse cert ─────────────
        {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Acquiring;
            s.push_log("Bootstrapping CA root certificate…");
        }

        let ca_root_path = std::path::PathBuf::from(&cfg.ca_root_pem_path);
        if let Err(e) = cert::bootstrap_ca_root(&cfg.ca_url, &ca_root_path).await {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Error(e.to_string());
            s.push_log(format!("CA bootstrap error: {e}"));
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }

        {
            state.lock().unwrap().push_log("Checking router certificate…");
        }

        let cert_result = acquire_or_reuse(&cfg).await;

        let (cert_p, key_p, ca_p) = match cert_result {
            Ok(paths) => paths,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.router_status = RouterStatus::Error(e.to_string());
                s.push_log(format!("Certificate error: {e}"));
                // Don't retry automatically — wait for the user to fix config.
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        // ── Phase 3: run session loop (restarts on ACL change) ───────────────
        let admitted = state.lock().unwrap().admitted.clone();
        let config = match build_zenoh_config(&cfg.router_listen, &ca_p, &cert_p, &key_p, &admitted) {
            Ok(c) => c,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.router_status = RouterStatus::Error(e.to_string());
                s.push_log(format!("Config error: {e}"));
                tokio::time::sleep(Duration::from_secs(2)).await;
                current_cfg = Some(cfg);
                continue;
            }
        };

        {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Starting;
            s.push_log(format!("Opening Zenoh router on {}…", cfg.router_listen));
        }

        let session = match zenoh::open(config).await {
            Ok(s) => s,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.router_status = RouterStatus::Error(e.to_string());
                s.push_log(format!("Zenoh open error: {e}"));
                tokio::time::sleep(Duration::from_secs(5)).await;
                current_cfg = Some(cfg);
                continue;
            }
        };

        {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Running;
            s.push_log("Router running.");
        }

        // session_loop returns when the admitted list changes.
        if let Err(e) = session_loop(&session, Arc::clone(&state)).await {
            state.lock().unwrap().push_log(format!("Session error: {e}"));
        }

        let _ = session.close().await;

        {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Reloading;
            s.push_log("Reloading router with updated ACL…");
        }

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
    if cert::needs_renewal(&c, max_age).await {
        let cert_cfg = cfg.cert_config_for("router")?;
        cert::acquire(&cert_cfg).await
    } else {
        Ok((c, k, ca))
    }
}

/// Subscribe to node announce messages and drain the action queue.
/// Returns when the admitted list changes (signalling a session restart).
async fn session_loop(session: &Session, state: Arc<Mutex<AppState>>) -> Result<()> {
    let subscriber = session
        .declare_subscriber("dverse/nodes/announce/**")
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    let admitted_snapshot = state.lock().unwrap().admitted.clone();

    loop {
        tokio::select! {
            sample = subscriber.recv_async() => {
                match sample {
                    Ok(s) => {
                        let key = s.key_expr().as_str().to_string();
                        if let Some(cn) = key.strip_prefix("dverse/nodes/announce/") {
                            let cn = cn.to_string();
                            let mut st = state.lock().unwrap();
                            if st.admitted.contains(&cn) || st.denied.contains(&cn) {
                                continue;
                            }
                            if !st.pending.contains_key(&cn) {
                                st.push_log(format!("Node announced: {cn}"));
                            }
                            st.pending.insert(cn, std::time::Instant::now());
                        }
                    }
                    Err(e) => return Err(anyhow::anyhow!("subscriber recv: {e}")),
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                let actions: Vec<Action> = {
                    let mut st = state.lock().unwrap();
                    std::mem::take(&mut st.action_queue)
                };

                for action in actions {
                    match action {
                        Action::Admit(cn) => {
                            let mut st = state.lock().unwrap();
                            st.pending.remove(&cn);
                            if !st.admitted.contains(&cn) {
                                st.admitted.push(cn.clone());
                                st.push_log(format!("Admitted: {cn}"));
                            }
                        }
                        Action::Deny(cn) => {
                            let mut st = state.lock().unwrap();
                            st.pending.remove(&cn);
                            if !st.denied.contains(&cn) {
                                st.denied.push(cn.clone());
                                st.push_log(format!("Denied: {cn}"));
                            }
                        }
                    }
                }

                let new_admitted = state.lock().unwrap().admitted.clone();
                if new_admitted != admitted_snapshot {
                    return Ok(());
                }
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
) -> Result<zenoh::Config> {
    let mut cfg = zenoh::Config::default();

    zinsert(&mut cfg, "mode", "\"router\"")?;
    zinsert(&mut cfg, "listen/endpoints", &format!("[\"{listen_addr}\"]"))?;
    zinsert(&mut cfg, "scouting/multicast/enabled", "false")?;
    zinsert(&mut cfg, "transport/link/tls/root_ca_certificate", &json_str(&ca_path.to_string_lossy()))?;
    zinsert(&mut cfg, "transport/link/tls/enable_mtls", "true")?;
    zinsert(&mut cfg, "transport/link/tls/listen_certificate", &json_str(&cert_path.to_string_lossy()))?;
    zinsert(&mut cfg, "transport/link/tls/listen_private_key", &json_str(&key_path.to_string_lossy()))?;
    zinsert(&mut cfg, "access_control", &build_acl_json(admitted))?;

    Ok(cfg)
}

fn build_acl_json(admitted: &[String]) -> String {
    let announce_key = "dverse/nodes/announce/**";
    let main_key = "dverse/**";
    let announce_msgs = serde_json::json!(["put", "delete", "declare_subscriber"]);
    let main_msgs = serde_json::json!(["put", "delete", "declare_subscriber", "query", "reply", "declare_queryable"]);

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
                }
            ],
            "subjects": [
                { "id": "any" }
            ],
            "policies": [
                { "rules": ["announce-rule"], "subjects": ["any"] }
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
                { "rules": ["announce-rule"], "subjects": ["any"] },
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
