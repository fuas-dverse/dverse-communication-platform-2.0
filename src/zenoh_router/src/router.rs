use anyhow::Result;
use std::sync::{Arc, Mutex};
use zenoh::Session;

use crate::state::{Action, AppState, RouterStatus};

pub struct RouterConfig {
    pub listen_addr: String,
    pub tls_ca: Option<String>,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

/// Build a Zenoh router config with the given admitted CNs baked into the ACL.
fn build_config(rc: &RouterConfig, admitted: &[String]) -> Result<zenoh::Config> {
    let mut cfg = zenoh::Config::default();

    zinsert(&mut cfg, "mode", "\"router\"")?;
    zinsert(
        &mut cfg,
        "listen/endpoints",
        &format!("[\"{}\"]", rc.listen_addr),
    )?;
    zinsert(&mut cfg, "scouting/multicast/enabled", "false")?;

    if let Some(ca) = &rc.tls_ca {
        zinsert(&mut cfg, "transport/link/tls/root_ca_certificate", &json_str(ca))?;
    }
    if rc.tls_cert.is_some() || rc.tls_key.is_some() {
        zinsert(&mut cfg, "transport/link/tls/enable_mtls", "true")?;
    }
    if let Some(cert) = &rc.tls_cert {
        zinsert(&mut cfg, "transport/link/tls/listen_certificate", &json_str(cert))?;
    }
    if let Some(key) = &rc.tls_key {
        zinsert(&mut cfg, "transport/link/tls/listen_private_key", &json_str(key))?;
    }

    let acl = build_acl_json(admitted);
    zinsert(&mut cfg, "access_control", &acl)?;

    Ok(cfg)
}

/// Generate ACL JSON that:
///  - allows everyone to publish/subscribe on `dverse/nodes/announce/**`
///  - allows only admitted CNs to publish/subscribe on `dverse/**`
///  - denies everything else by default
fn build_acl_json(admitted: &[String]) -> String {
    let announce_key = "dverse/nodes/announce/**";
    let main_key = "dverse/**";

    // Subject for the announce key — allow any authenticated peer.
    let announce_subject = r#"{"interfaces": ["all"]}"#;

    // Subjects for admitted CNs.
    let admitted_subjects: String = admitted
        .iter()
        .enumerate()
        .map(|(i, cn)| {
            format!(
                r#"{{ "id": {id}, "cert_common_names": ["{cn}"] }}"#,
                id = i + 2,
                cn = cn
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    if admitted.is_empty() {
        // Only the announce key is open; nothing else is admitted yet.
        format!(
            r#"{{
  "enabled": true,
  "default_permission": "deny",
  "rules": [
    {{
      "id": 1,
      "messages": ["put", "declare_subscriber"],
      "flows": ["ingress", "egress"],
      "permission": "allow",
      "key_exprs": ["{announce_key}"],
      "subject": {announce_subject}
    }}
  ]
}}"#
        )
    } else {
        format!(
            r#"{{
  "enabled": true,
  "default_permission": "deny",
  "rules": [
    {{
      "id": 1,
      "messages": ["put", "declare_subscriber"],
      "flows": ["ingress", "egress"],
      "permission": "allow",
      "key_exprs": ["{announce_key}"],
      "subject": {announce_subject}
    }},
    {{
      "id": 2,
      "messages": ["put", "declare_subscriber", "declare_queryable", "get"],
      "flows": ["ingress", "egress"],
      "permission": "allow",
      "key_exprs": ["{main_key}"],
      "subject": {{ "and": [{admitted_subjects}] }}
    }}
  ]
}}"#
        )
    }
}

/// Background task: open the router session, subscribe to announces, process the
/// action queue.  Restarts the session whenever the admitted list changes.
pub async fn run(rc: RouterConfig, state: Arc<Mutex<AppState>>) {
    loop {
        let admitted = {
            let s = state.lock().unwrap();
            s.admitted.clone()
        };

        {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Starting;
            s.push_log("Building Zenoh router session...");
        }

        let config = match build_config(&rc, &admitted) {
            Ok(c) => c,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.router_status = RouterStatus::Error(e.to_string());
                s.push_log(format!("Config error: {e}"));
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        let session = match zenoh::open(config).await {
            Ok(s) => s,
            Err(e) => {
                let mut s = state.lock().unwrap();
                s.router_status = RouterStatus::Error(e.to_string());
                s.push_log(format!("Zenoh open error: {e}"));
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Running;
            s.push_log("Router session up.");
        }

        if let Err(e) = session_loop(&session, Arc::clone(&state)).await {
            let mut s = state.lock().unwrap();
            s.push_log(format!("Session error: {e}"));
        }

        // Close the old session before reopening.
        let _ = session.close().await;

        {
            let mut s = state.lock().unwrap();
            s.router_status = RouterStatus::Reloading;
            s.push_log("Reloading router with updated ACL...");
        }
    }
}

/// Inner loop: subscribe to announces + drain the action queue.
/// Returns when the admitted list changes (triggering a session restart).
async fn session_loop(session: &Session, state: Arc<Mutex<AppState>>) -> Result<()> {
    let announce_key = "dverse/nodes/announce/**";

    let subscriber = session
        .declare_subscriber(announce_key)
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    let admitted_snapshot = state.lock().unwrap().admitted.clone();

    loop {
        tokio::select! {
            sample = subscriber.recv_async() => {
                match sample {
                    Ok(s) => {
                        // Key is dverse/nodes/announce/<cn>; extract the CN.
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
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {
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

                // If the admitted list changed, restart the session with new ACL.
                let new_admitted = state.lock().unwrap().admitted.clone();
                if new_admitted != admitted_snapshot {
                    return Ok(());
                }
            }
        }
    }
}

fn json_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn zinsert(cfg: &mut zenoh::Config, key: &str, value: &str) -> Result<()> {
    cfg.insert_json5(key, value)
        .map_err(|e| anyhow::anyhow!("zenoh config key '{}': {}", key, e))
}
