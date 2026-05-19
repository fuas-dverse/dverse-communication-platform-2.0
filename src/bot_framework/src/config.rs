use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Machine-wide dverse configuration, stored at `~/.config/dverse/config.toml`.
/// Only one user session is supported per machine.
///
/// The server-side constants (Keycloak URL, CA URL, client credentials) are
/// set by the application at save time and are not exposed in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DverseConfig {
    pub username: String,
    #[serde(skip)]
    pub password: String,
    pub keycloak_url: String,
    pub keycloak_realm: String,
    pub client_id: String,
    pub client_secret: String,
    pub ca_url: String,
    /// Path to the cached Step-CA root PEM (bootstrapped on first login).
    pub ca_root_pem_path: String,
    pub cert_dir: PathBuf,
    /// Zenoh listen address for the local router (server side).
    pub router_listen: String,
    /// Zenoh connect endpoint used by local agents (client side).
    pub router_endpoint: String,
}

impl DverseConfig {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dverse")
            .join("config.toml")
    }

    pub fn ca_root_pem_path_default() -> String {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("dverse")
            .join("ca-root.pem")
            .to_string_lossy()
            .into_owned()
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Self = toml::from_str(&text).context("parsing config TOML")?;
        let entry = keyring::Entry::new("dverse", &cfg.username)
            .context("opening keychain entry")?;
        cfg.password = entry
            .get_password()
            .context("reading password from keychain — sign in again to re-enter it")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let entry = keyring::Entry::new("dverse", &self.username)
            .context("opening keychain entry")?;
        entry.set_password(&self.password)
            .context("storing password in keychain")?;
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

    /// Returns the cert CN — the `preferred_username` portion of the Keycloak
    /// login (everything before the first `@`, or the whole string if no `@`).
    pub fn operator_cn(&self) -> String {
        self.username
            .split('@')
            .next()
            .unwrap_or(&self.username)
            .to_string()
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
