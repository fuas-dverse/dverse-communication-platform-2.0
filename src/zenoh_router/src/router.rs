use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use bot_framework::cert;
use bot_framework::config::DverseConfig;
use zenoh::Session;

use crate::state::{AppState, RouterStatus};

// ── Avahi mDNS helper ─────────────────────────────────────────────────────────

/// Keeps an `avahi-publish-address` child alive.  Kills it on drop so the
/// mDNS record is withdrawn when the router shuts down.
struct AvahiHandle(std::process::Child);

impl Drop for AvahiHandle {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawn `avahi-publish-address -R <hostname> 127.0.0.1` in the background.
/// Returns None (with a log message) if avahi-utils is not installed.
fn publish_avahi(hostname: &str, state: &Arc<Mutex<AppState>>) -> Option<AvahiHandle> {
    match std::process::Command::new("avahi-publish-address")
        .args(["-R", hostname, "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            state.lock().unwrap().push_log(
                format!("mDNS: {hostname} → 127.0.0.1 (via Avahi)"),
            );
            Some(AvahiHandle(child))
        }
        Err(e) => {
            state.lock().unwrap().push_log(
                format!("Warning: avahi-publish-address unavailable ({e}); add {hostname} to /etc/hosts manually"),
            );
            None
        }
    }
}

// ── Router background task ────────────────────────────────────────────────────

/// Background entry point.  Waits for a `DverseConfig` to appear in AppState,
/// acquires/reuses the router cert, then runs the Zenoh router indefinitely,
/// restarting the session whenever the admitted ACL changes.
pub async fn run(state: Arc<Mutex<AppState>>) {
    let mut current_cfg: Option<DverseConfig> = None;
    // Kept alive for the entire router lifetime; dropped (→ avahi record removed) on exit.
    let mut _avahi: Option<AvahiHandle> = None;

    loop {
        // ── Phase 1: obtain config ───────────────────────────────────────────
        let cfg = if let Some(c) = current_cfg.take() {
            c
        } else {
            state.lock().unwrap().push_log("Waiting for configuration…");
            let cfg = loop {
                {
                    let mut s = state.lock().unwrap();
                    if let Some(cfg) = s.staged_config.take() {
                        break cfg;
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            };

            // Register the mDNS hostname once, the first time we get a config.
            if _avahi.is_none() {
                let hostname = format!("zenoh-{}.local", cfg.operator_cn());
                _avahi = publish_avahi(&hostname, &state);
            }

            cfg
        };

        // Pre-admit operator's CN so all local agents can communicate immediately.
        let operator_cn = cfg.operator_cn();
        {
            let mut st = state.lock().unwrap();
            if !st.admitted.contains(&operator_cn) {
                st.admitted.push(operator_cn.clone());
                st.push_log(format!("Pre-admitted operator CN: {operator_cn}"));
            }
        }

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

/// Subscribe to node announce messages; auto-admit any node with a valid cert.
/// Returns when the admitted list changes (triggering an ACL session restart).
async fn session_loop(session: &Session, state: Arc<Mutex<AppState>>) -> Result<()> {
    let subscriber = session
        .declare_subscriber("dverse/nodes/announce/**")
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    loop {
        match subscriber.recv_async().await {
            Ok(s) => {
                let key = s.key_expr().as_str().to_string();
                if key.starts_with("dverse/nodes/announce/") {
                    // CN is sent as payload; fall back to key segment if empty.
                    let payload_cn = String::from_utf8_lossy(&s.payload().to_bytes()).into_owned();
                    let cn = if payload_cn.trim().is_empty() {
                        key.strip_prefix("dverse/nodes/announce/").unwrap_or("").to_string()
                    } else {
                        payload_cn.trim().to_string()
                    };
                    if cn.is_empty() { continue; }
                    let mut st = state.lock().unwrap();
                    if !st.admitted.contains(&cn) {
                        st.admitted.push(cn.clone());
                        st.push_log(format!("Auto-admitted CN: {cn}"));
                        // Signal the outer loop to restart the session with new ACL.
                        return Ok(());
                    }
                }
            }
            Err(e) => return Err(anyhow::anyhow!("subscriber recv: {e}")),
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
