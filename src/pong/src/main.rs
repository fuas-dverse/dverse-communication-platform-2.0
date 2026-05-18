use std::time::Duration;

use anyhow::Result;
use bot_framework::{cert, config::DverseConfig, node::NodeConfig};

const NODE_NAME: &str = "pong";

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = DverseConfig::load()
        .expect("No dverse config found. Run the router first to log in.");

    let cert_path = cert::cert_path(&cfg.cert_dir, NODE_NAME);
    let key_path  = cert::key_path(&cfg.cert_dir, NODE_NAME);

    if cert::needs_renewal(&cert_path, Duration::from_secs(23 * 3600)).await {
        println!("[{NODE_NAME}] Acquiring certificate…");
        let cert_cfg = cfg.cert_config_for(NODE_NAME)?;
        cert::acquire(&cert_cfg).await?;
    } else {
        println!("[{NODE_NAME}] Using cached certificate.");
    }

    println!("[{NODE_NAME}] Connecting to {}…", cfg.router_endpoint);
    let session = NodeConfig::mtls(
        &cfg.router_endpoint,
        &cfg.ca_root_pem_path,
        &cert_path,
        &key_path,
    )
    .connect()
    .await?;

    println!("[{NODE_NAME}] Connected. Announcing…");
    session
        .put(format!("dverse/nodes/announce/{NODE_NAME}"), "")
        .await
        .map_err(|e| anyhow::anyhow!("announce: {e}"))?;

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
