use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

use bot_framework::cert::{self, CertConfig};
use bot_framework::node::NodeConfig;

#[derive(Parser)]
#[command(name = "bot-framework", about = "DVerse node certificate and Zenoh management")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Acquire a certificate from Step-CA via Keycloak and write it to disk.
    GetCert {
        #[arg(long, env = "NODE_NAME")]
        node_name: String,

        #[arg(long, env = "KEYCLOAK_URL", default_value = "https://auth.dverse.yordanmitev.me")]
        keycloak_url: String,

        #[arg(long, env = "KEYCLOAK_REALM", default_value = "master")]
        keycloak_realm: String,

        #[arg(long, env = "KEYCLOAK_CLIENT_ID", default_value = "step-ca")]
        client_id: String,

        #[arg(long, env = "KEYCLOAK_CLIENT_SECRET")]
        client_secret: String,

        #[arg(long, env = "KEYCLOAK_USERNAME")]
        username: String,

        #[arg(long, env = "KEYCLOAK_PASSWORD")]
        password: String,

        #[arg(long, env = "STEP_CA_URL", default_value = "https://ca.dverse.yordanmitev.me:9000")]
        ca_url: String,

        /// Path to Step-CA root certificate PEM
        #[arg(long, env = "STEP_CA_ROOT")]
        ca_root: PathBuf,

        /// Directory to write cert, key, and CA chain into
        #[arg(long, env = "CERT_DIR", default_value = "./certs")]
        out_dir: PathBuf,
    },

    /// Check whether the cert on disk needs renewal (exits 1 if it does).
    CheckCert {
        #[arg(long, env = "NODE_NAME")]
        node_name: String,

        #[arg(long, env = "CERT_DIR", default_value = "./certs")]
        cert_dir: PathBuf,

        /// Treat the cert as stale after this many hours (default: 23)
        #[arg(long, default_value = "23")]
        max_age_hours: u64,
    },

    /// Open a Zenoh session and print the session ID (smoke-test for connectivity).
    Connect {
        #[arg(long, env = "ZENOH_ROUTER", default_value = "tcp/localhost:7447")]
        router: String,

        #[arg(long, env = "ZENOH_TLS_CA")]
        tls_ca: Option<PathBuf>,

        #[arg(long, env = "ZENOH_TLS_CERT")]
        tls_cert: Option<PathBuf>,

        #[arg(long, env = "ZENOH_TLS_KEY")]
        tls_key: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    bot_framework::logging::init();
    let cli = Cli::parse();

    match cli.command {
        Command::GetCert {
            node_name,
            keycloak_url,
            keycloak_realm,
            client_id,
            client_secret,
            username,
            password,
            ca_url,
            ca_root,
            out_dir,
        } => {
            let ca_root_pem = tokio::fs::read_to_string(&ca_root).await?;

            let cfg = CertConfig {
                keycloak_url,
                keycloak_realm,
                client_id,
                client_secret,
                username,
                password,
                ca_url,
                ca_root_pem,
                out_dir,
                node_name,
            };

            let (crt, key, ca) = cert::acquire(&cfg).await?;
            println!("  cert: {}", crt.display());
            println!("   key: {}", key.display());
            println!("    ca: {}", ca.display());
        }

        Command::CheckCert { node_name, cert_dir, max_age_hours } => {
            let crt = cert::cert_path(&cert_dir, &node_name);
            let stale = cert::needs_renewal(&crt, Duration::from_secs(max_age_hours * 3600), None).await;
            if stale {
                eprintln!("Certificate {} needs renewal.", crt.display());
                std::process::exit(1);
            } else {
                println!("Certificate {} is current.", crt.display());
            }
        }

        Command::Connect { router, tls_ca, tls_cert, tls_key } => {
            let node_cfg = match (tls_ca, tls_cert, tls_key) {
                (Some(ca), Some(cert), Some(key)) => NodeConfig::mtls(router, ca, cert, key),
                _ => NodeConfig::plain(router),
            };

            println!("Connecting to Zenoh router...");
            let session = node_cfg.connect().await?;
            println!("Connected. Session ZID: {}", session.zid());
        }
    }

    Ok(())
}
