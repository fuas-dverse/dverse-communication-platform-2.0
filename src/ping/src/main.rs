use std::time::Duration;

use anyhow::Result;
use bot_framework::{
    announce::{AgentAnnouncer, AgentInfo},
    cert,
    config::DverseConfig,
    node::NodeConfig,
    payload_crypto::PayloadCipher,
};
use tracing::{error, info, warn};

const NODE_NAME: &str = "ping";

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
            publishes: vec!["dverse/ping".into()],
            subscribes: vec!["dverse/pong".into()],
        },
    );

    // Per-process Megolm cipher. Each agent has its own outbound sender and
    // discovers peers' session keys by polling `<cert_dir>/megolm/`. See
    // `bot_framework::payload_crypto` for the carve-outs.
    let crypto_dir = cfg.cert_dir.join("megolm");
    let mut cipher = PayloadCipher::new(NODE_NAME, &crypto_dir)?;
    cipher.publish_session_key(&crypto_dir, NODE_NAME)?;

    let pong_sub = session
        .declare_subscriber("dverse/pong")
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    let mut seq: u64 = 0;
    let mut refresh_tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        // Pick up any peer session keys written to the dir since the last loop.
        let _ = cipher.refresh_receivers(&crypto_dir);

        seq += 1;
        let msg = format!("ping #{seq}");
        let wire = cipher.encrypt(msg.as_bytes())?;
        info!(node = NODE_NAME, dir = "tx", plaintext = %msg, wire_len = wire.len(), "sending (encrypted)");
        session
            .put("dverse/ping", wire)
            .await
            .map_err(|e| anyhow::anyhow!("put: {e}"))?;

        tokio::select! {
            sample = pong_sub.recv_async() => {
                match sample {
                    Ok(s) => {
                        let bytes = s.payload().to_bytes();
                        match cipher.decrypt(&bytes) {
                            Ok(pt) => {
                                let payload = String::from_utf8_lossy(&pt).into_owned();
                                info!(node = NODE_NAME, dir = "rx", payload = %payload, "received (decrypted)");
                            }
                            Err(e) => warn!(node = NODE_NAME, error = %e, "received undecryptable pong (likely missing peer key — refresh pending)"),
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "pong subscriber recv error");
                        return Err(anyhow::anyhow!("recv: {e}"));
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(2)) => {
                info!(node = NODE_NAME, "no pong within timeout");
            }
            _ = refresh_tick.tick() => {}
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
