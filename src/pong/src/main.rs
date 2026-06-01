use std::time::Duration;

use anyhow::Result;
use bot_framework::{
    announce::{AgentAnnouncer, AgentInfo},
    cert,
    config::DverseConfig,
    node::NodeConfig,
};
use tracing::info;

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

    let ping_sub = session
        .declare_subscriber("dverse/ping")
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    info!(node = NODE_NAME, "listening for pings");
    while let Ok(sample) = ping_sub.recv_async().await {
        let payload = String::from_utf8_lossy(&sample.payload().to_bytes()).into_owned();
        info!(node = NODE_NAME, dir = "rx", payload = %payload, "received");

        let reply = format!("pong (echoing: {payload})");
        info!(node = NODE_NAME, dir = "tx", payload = %reply, "sending");
        session
            .put("dverse/pong", reply)
            .await
            .map_err(|e| anyhow::anyhow!("put: {e}"))?;

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}
