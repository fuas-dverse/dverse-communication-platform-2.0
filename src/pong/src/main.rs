use std::time::Duration;

use anyhow::Result;
use bot_framework::{
    announce::{AgentAnnouncer, AgentInfo},
    cert,
    config::DverseConfig,
    node::NodeConfig,
};

const NODE_NAME: &str = "pong";

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = DverseConfig::load()
        .expect("No dverse config found. Run the router first to log in.");

    let cert_path = cert::cert_path(&cfg.cert_dir, NODE_NAME);
    let key_path  = cert::key_path(&cfg.cert_dir, NODE_NAME);
    let ca_path   = cert::ca_path(&cfg.cert_dir, NODE_NAME);

    if cert::needs_renewal(&cert_path, Duration::from_secs(23 * 3600), Some(&cfg.operator_cn())).await {
        println!("[{NODE_NAME}] Acquiring certificate…");
        let cert_cfg = cfg.cert_config_for(NODE_NAME)?;
        cert::acquire(&cert_cfg).await?;
    } else {
        println!("[{NODE_NAME}] Using cached certificate.");
    }

    println!("[{NODE_NAME}] Connecting to {}…", cfg.router_endpoint);
    let session = NodeConfig::mtls(
        &cfg.router_endpoint,
        &ca_path,
        &cert_path,
        &key_path,
    )
    .connect()
    .await?;

    let cn = cfg.operator_cn();
    println!("[{NODE_NAME}] Connected. Announcing as CN={cn}…");
    // Heartbeat announcer — runs in a background tokio task as long as this
    // handle is alive.  Drop = stop heartbeating; the router will mark the
    // agent Degraded → Offline → evict it on its own timer.
    let _announcer = AgentAnnouncer::start(
        session.clone(),
        AgentInfo {
            cn: &cn,
            agent_name: NODE_NAME,
            version: env!("CARGO_PKG_VERSION"),
            key_exprs: vec!["dverse/ping".into(), "dverse/pong".into()],
        },
    );

    let ping_sub = session
        .declare_subscriber("dverse/ping")
        .await
        .map_err(|e| anyhow::anyhow!("declare_subscriber: {e}"))?;

    println!("[{NODE_NAME}] Listening for pings…");
    while let Ok(sample) = ping_sub.recv_async().await {
        let payload = String::from_utf8_lossy(&sample.payload().to_bytes()).into_owned();
        println!("[{NODE_NAME}] ← {payload}");

        let reply = format!("pong (echoing: {payload})");
        println!("[{NODE_NAME}] → {reply}");
        session
            .put("dverse/pong", reply)
            .await
            .map_err(|e| anyhow::anyhow!("put: {e}"))?;

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}
