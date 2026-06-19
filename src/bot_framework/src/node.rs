//! Zenoh node: opens a session with optional mTLS using Step-CA-issued certs.
//!
//! With a cert issued by the CA, the node connects as `zenoh-<name>.local` —
//! matching the DNS SAN in the certificate.  The Zenoh router can enforce mTLS
//! so only nodes with valid CA-signed certificates can join the network.

use anyhow::Result;
use std::path::{Path, PathBuf};
use zenoh::Session;

/// Configuration for a Zenoh node connection.
pub struct NodeConfig {
    /// Zenoh router endpoint.
    /// Plain TCP:  "tcp/localhost:7447"
    /// TLS / mTLS: "tls/router.local:7447"
    pub router: String,

    /// Path to the CA root certificate PEM (Step-CA root).
    /// Required when `router` uses a `tls/` endpoint.
    pub tls_ca: Option<PathBuf>,

    /// Path to the node's client certificate PEM (issued by Step-CA).
    /// Required for mTLS.
    pub tls_cert: Option<PathBuf>,

    /// Path to the node's private key PEM.
    /// Required for mTLS.
    pub tls_key: Option<PathBuf>,

    /// Session namespace (the session id) applied to the Zenoh session, so this
    /// node's key expressions match its router's namespace on the shared fabric.
    /// `None` (or empty) means no namespace. Must equal the router's namespace.
    pub namespace: Option<String>,

    /// Skip TLS SNI hostname verification. Required when connecting via IP
    /// address (e.g. 127.0.0.1) where the cert's DNS SAN won't match.
    pub skip_name_check: bool,
}

impl NodeConfig {
    /// Create a plain (no TLS) node config.
    pub fn plain(router: impl Into<String>) -> Self {
        Self {
            router: router.into(),
            tls_ca: None,
            tls_cert: None,
            tls_key: None,
            namespace: None,
            skip_name_check: false,
        }
    }

    /// Create an mTLS node config using a Step-CA-issued certificate.
    pub fn mtls(
        router: impl Into<String>,
        tls_ca: impl Into<PathBuf>,
        tls_cert: impl Into<PathBuf>,
        tls_key: impl Into<PathBuf>,
    ) -> Self {
        Self {
            router: router.into(),
            tls_ca: Some(tls_ca.into()),
            tls_cert: Some(tls_cert.into()),
            tls_key: Some(tls_key.into()),
            namespace: None,
            skip_name_check: false,
        }
    }

    /// Skip TLS SNI hostname verification on connect. Use when connecting via
    /// IP address (e.g. 127.0.0.1) where the cert's DNS SAN won't match.
    pub fn skip_name_check(mut self) -> Self {
        self.skip_name_check = true;
        self
    }

    /// Set the session namespace (the session id) for this node. Empty values
    /// are ignored. Must match the router's namespace to communicate.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        let ns = namespace.into();
        self.namespace = (!ns.is_empty()).then_some(ns);
        self
    }

    /// Open a Zenoh session using this configuration.
    pub async fn connect(self) -> Result<Session> {
        let config = self.build_zenoh_config().await?;
        zenoh::open(config)
            .await
            .map_err(|e| anyhow::anyhow!("opening Zenoh session: {e}"))
    }

    async fn build_zenoh_config(&self) -> Result<zenoh::Config> {
        let mut config = zenoh::Config::default();

        zinsert(&mut config, "mode", "\"client\"")?;
        zinsert(&mut config, "connect/endpoints", &format!("[\"{}\"]", self.router))?;
        zinsert(&mut config, "scouting/multicast/enabled", "false")?;

        if let Some(ns) = self.namespace.as_deref().filter(|s| !s.is_empty()) {
            zinsert(&mut config, "namespace", &format!("\"{ns}\""))?;
        }

        if let Some(ca_path) = &self.tls_ca {
            zinsert(&mut config, "transport/link/tls/root_ca_certificate", &path_to_json_str(ca_path))?;
        }

        if self.tls_cert.is_some() || self.tls_key.is_some() {
            zinsert(&mut config, "transport/link/tls/enable_mtls", "true")?;
        }

        if let Some(cert_path) = &self.tls_cert {
            zinsert(&mut config, "transport/link/tls/connect_certificate", &path_to_json_str(cert_path))?;
        }

        if let Some(key_path) = &self.tls_key {
            zinsert(&mut config, "transport/link/tls/connect_private_key", &path_to_json_str(key_path))?;
        }

        if self.skip_name_check {
            zinsert(&mut config, "transport/link/tls/verify_name_on_connect", "false")?;
        }

        Ok(config)
    }
}

/// Encode a path as a JSON5 string value (quoted, forward slashes).
fn path_to_json_str(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    format!("\"{s}\"")
}

/// Thin wrapper: convert zenoh's boxed error into anyhow for `?` ergonomics.
fn zinsert(config: &mut zenoh::Config, key: &str, value: &str) -> Result<()> {
    config
        .insert_json5(key, value)
        .map_err(|e| anyhow::anyhow!("zenoh config key '{}': {}", key, e))
}
