use std::time::Duration;

use anyhow::Result;
use bot_framework::{
    announce::{AgentAnnouncer, AgentInfo},
    cert,
    config::DverseConfig,
    node::NodeConfig,
    payload_crypto::PayloadCipher,
};
use tracing::{info, warn};

const NODE_NAME: &str = "pong";

#[tokio::main]
async fn main() -> Result<()> {
    bot_framework::logging::init();
    let cfg = DverseConfig::load()
        .expect("No dverse config found. Run the router first to log in.");

    let cert_path = cert::cert_path(&cfg.cert_dir, NODE_NAME);
    let key_path  = cert::key_path(&cfg.cert_dir, NODE_NAME);
    let ca_path   = cert::ca_path(&cfg.cert_dir, NODE_NAME);

    if cert::needs_renewal(&cert_path, Duration::from_secs(23 * 3600), Some(&cfg.operator_cn())).await {
        info!(node = NODE_NAME, "acquiring certificate");
        let cert_cfg = cfg.cert_config_for(NODE_NAME)?;
        cert::acquire(&cert_cfg).await?;
    } else {
        info!(node = NODE_NAME, "using cached certificate");
    }

    info!(node = NODE_NAME, endpoint = %cfg.router_endpoint, "connecting to router");
    let session = NodeConfig::mtls(
        &cfg.router_endpoint,
        &ca_path,
        &cert_path,
        &key_path,
    )
    .with_namespace(cfg.session_id())
    .connect()
    .await?;

    let cn = cfg.operator_cn();
    info!(node = NODE_NAME, cn = %cn, "connected, announcing");
    // Heartbeat announcer — runs in a background tokio task as long as this
    // handle is alive.  Drop = stop heartbeating; the router will mark the
    // agent Degraded → Offline → evict it on its own timer.
    let _announcer = AgentAnnouncer::start(
        session.clone(),
        AgentInfo {
            cn: &cn,
            agent_name: NODE_NAME,
            version: env!("CARGO_PKG_VERSION"),
            publishes: vec!["dverse/pong".into()],
            subscribes: vec!["dverse/ping".into()],
        },
    );

    let crypto_dir = cfg.cert_dir.join("megolm");
    let mut cipher = PayloadCipher::new(NODE_NAME, &crypto_dir)?;
    cipher.publish_session_key(&crypto_dir, NODE_NAME)?;

    let ping_sub = session
        .declare_subscriber("dverse/ping")
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    info!(node = NODE_NAME, "listening for pings (encrypted)");
    let mut refresh_tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            sample = ping_sub.recv_async() => {
                let sample = match sample {
                    Ok(s) => s,
                    Err(_) => break,
                };
                let bytes = sample.payload().to_bytes();
                let payload = match cipher.decrypt(&bytes) {
                    Ok(pt) => String::from_utf8_lossy(&pt).into_owned(),
                    Err(e) => {
                        warn!(node = NODE_NAME, error = %e, "dropped undecryptable ping (peer key not yet installed)");
                        continue;
                    }
                };
                info!(node = NODE_NAME, dir = "rx", payload = %payload, "received (decrypted)");

                let reply = format!("pong (echoing: {payload})");
                let wire = cipher.encrypt(reply.as_bytes())?;
                info!(node = NODE_NAME, dir = "tx", plaintext = %reply, wire_len = wire.len(), "sending (encrypted)");
                session
                    .put("dverse/pong", wire)
                    .await
                    .map_err(|e| anyhow::anyhow!("put: {e}"))?;

                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ = refresh_tick.tick() => {
                let _ = cipher.refresh_receivers(&crypto_dir);
            }
        }
    }

    Ok(())
}
