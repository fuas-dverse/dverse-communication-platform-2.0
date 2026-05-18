use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Machine-wide dverse configuration, stored at `~/.config/dverse/config.toml`.
/// Only one user session is supported per machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DverseConfig {
    /// Keycloak user email, e.g. "alice@dverse.yordanmitev.me"
    pub username: String,
    /// Keycloak password (stored locally for non-interactive cert renewal)
    pub password: String,
    pub keycloak_url: String,
    pub keycloak_realm: String,
    pub client_id: String,
    pub client_secret: String,
    /// Step-CA base URL
    pub ca_url: String,
    /// Path to the Step-CA root PEM file on disk
    pub ca_root_pem_path: String,
    /// Directory where per-node certificates are cached
    pub cert_dir: PathBuf,
    /// Zenoh listen address for the local router
    pub router_listen: String,
}

impl Default for DverseConfig {
    fn default() -> Self {
        let cert_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dverse")
            .join("certs");
        Self {
            username: String::new(),
            password: String::new(),
            keycloak_url: String::from("https://auth.dverse.yordanmitev.me"),
            keycloak_realm: String::from("master"),
            client_id: String::from("step-ca"),
            client_secret: String::new(),
            ca_url: String::from("https://ca.dverse.yordanmitev.me:9000"),
            ca_root_pem_path: String::new(),
            cert_dir,
            router_listen: String::from("tls/0.0.0.0:7447"),
        }
    }
}

impl DverseConfig {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dverse")
            .join("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).context("parsing config TOML")
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("creating config dir")?;
        }
        let text = toml::to_string_pretty(self).context("serializing config")?;
        std::fs::write(&path, &text)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn exists() -> bool {
        Self::config_path().exists()
    }

    /// Build a `CertConfig` for the given node name using the stored credentials.
    pub fn cert_config_for(&self, node_name: &str) -> Result<crate::cert::CertConfig> {
        let ca_root_pem = std::fs::read_to_string(&self.ca_root_pem_path)
            .with_context(|| format!("reading CA root PEM from {}", self.ca_root_pem_path))?;
        Ok(crate::cert::CertConfig {
            keycloak_url: self.keycloak_url.clone(),
            keycloak_realm: self.keycloak_realm.clone(),
            client_id: self.client_id.clone(),
            client_secret: self.client_secret.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            ca_url: self.ca_url.clone(),
            ca_root_pem,
            out_dir: self.cert_dir.clone(),
            node_name: node_name.to_string(),
        })
    }
}
