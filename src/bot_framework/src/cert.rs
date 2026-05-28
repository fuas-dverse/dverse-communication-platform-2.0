//! Certificate lifecycle: acquire from Step-CA via Keycloak OIDC, persist to disk.
//!
//! Flow:
//!   1. Exchange Keycloak credentials for an OIDC ID token.
//!   2. Generate a local keypair and CSR with rcgen.
//!   3. POST the CSR + token to Step-CA's /1.0/sign endpoint.
//!   4. Write cert, key, and CA chain to the configured directory.
//!
//! The issued certificate will have:
//!   - CN  = username portion of the email  (e.g. "mybot")
//!   - SAN = email + zenoh-<username>.local  (set by the Step-CA x509 template)

use anyhow::{bail, Context, Result};
use rcgen::{CertificateParams, KeyPair};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── Configuration ─────────────────────────────────────────────────────────────

/// Everything needed to acquire a certificate for one node identity.
pub struct CertConfig {
    /// e.g. "https://auth.dverse.yordanmitev.me"
    pub keycloak_url: String,
    /// Keycloak realm name, e.g. "master"
    pub keycloak_realm: String,
    /// OIDC client ID registered in Keycloak for Step-CA
    pub client_id: String,
    /// OIDC client secret
    pub client_secret: String,
    /// Keycloak username (email form, e.g. "mybot@dverse.yordanmitev.me")
    pub username: String,
    /// Keycloak password
    pub password: String,
    /// Step-CA base URL, e.g. "https://ca.dverse.yordanmitev.me:9000"
    pub ca_url: String,
    /// PEM-encoded Step-CA root certificate (needed to trust the CA's TLS cert)
    pub ca_root_pem: String,
    /// Directory where cert.pem, key.pem, and ca.pem will be written
    pub out_dir: PathBuf,
    /// Base name for output files (e.g. "mybot" → mybot.crt / mybot.key / mybot-ca.pem)
    pub node_name: String,
}

// ── Paths ─────────────────────────────────────────────────────────────────────

pub fn cert_path(out_dir: &Path, node_name: &str) -> PathBuf {
    out_dir.join(format!("{node_name}.crt"))
}

pub fn key_path(out_dir: &Path, node_name: &str) -> PathBuf {
    out_dir.join(format!("{node_name}.key"))
}

pub fn ca_path(out_dir: &Path, node_name: &str) -> PathBuf {
    out_dir.join(format!("{node_name}-ca.pem"))
}

// ── Keycloak token exchange ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn fetch_id_token(cfg: &CertConfig, client: &Client) -> Result<String> {
    let url = format!(
        "{}/realms/{}/protocol/openid-connect/token",
        cfg.keycloak_url, cfg.keycloak_realm
    );

    let resp: TokenResponse = client
        .post(&url)
        .form(&[
            ("client_id", cfg.client_id.as_str()),
            ("client_secret", cfg.client_secret.as_str()),
            ("grant_type", "password"),
            ("username", cfg.username.as_str()),
            ("password", cfg.password.as_str()),
            ("scope", "openid"),
        ])
        .send()
        .await
        .context("sending Keycloak token request")?
        .json()
        .await
        .context("parsing Keycloak token response")?;

    if let Some(err) = resp.error {
        bail!(
            "Keycloak error: {} — {}",
            err,
            resp.error_description.unwrap_or_default()
        );
    }

    resp.id_token.context("Keycloak response contained no id_token")
}

// ── CSR generation ────────────────────────────────────────────────────────────

fn generate_csr() -> Result<(String, String)> {
    let key_pair = KeyPair::generate().context("generating keypair")?;
    let params = CertificateParams::default();
    let csr = params
        .serialize_request(&key_pair)
        .context("serializing CSR")?;
    Ok((csr.pem().context("encoding CSR as PEM")?, key_pair.serialize_pem()))
}

// ── Step-CA sign ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct SignRequest {
    csr: String,
    ott: String,
}

#[derive(Deserialize)]
struct SignResponse {
    crt: Option<String>,
    ca: Option<String>,
    // Step-CA returns "message" on errors
    message: Option<String>,
}

async fn sign_with_step_ca(cfg: &CertConfig, client: &Client, csr_pem: &str, id_token: &str) -> Result<(String, String)> {
    let url = format!("{}/1.0/sign", cfg.ca_url);

    let body = SignRequest {
        csr: csr_pem.to_string(),
        ott: id_token.to_string(),
    };

    let resp: SignResponse = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .context("sending CSR to Step-CA")?
        .json()
        .await
        .context("parsing Step-CA sign response")?;

    let cert_pem = resp.crt.with_context(|| {
        format!("Step-CA response missing 'crt': {}", resp.message.unwrap_or_default())
    })?;
    let ca_pem = resp.ca.unwrap_or_default();
    Ok((cert_pem, ca_pem))
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Acquire a certificate from Step-CA and write it to `cfg.out_dir`.
///
/// Returns `(cert_path, key_path, ca_path)`.
pub async fn acquire(cfg: &CertConfig) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let ca_cert = reqwest::Certificate::from_pem(cfg.ca_root_pem.as_bytes())
        .context("parsing CA root PEM for HTTP client")?;

    let client = reqwest::ClientBuilder::new()
        .add_root_certificate(ca_cert)
        .build()
        .context("building HTTP client")?;

    println!("Authenticating {} with Keycloak...", cfg.username);
    let id_token = fetch_id_token(cfg, &client).await?;

    println!("Generating keypair and CSR...");
    let (csr_pem, key_pem) = generate_csr()?;

    println!("Requesting certificate from Step-CA...");
    let (cert_pem, ca_pem) = sign_with_step_ca(cfg, &client, &csr_pem, &id_token).await?;

    tokio::fs::create_dir_all(&cfg.out_dir)
        .await
        .context("creating output directory")?;

    let c = cert_path(&cfg.out_dir, &cfg.node_name);
    let k = key_path(&cfg.out_dir, &cfg.node_name);
    let ca = ca_path(&cfg.out_dir, &cfg.node_name);

    tokio::fs::write(&c, &cert_pem).await.context("writing cert")?;
    tokio::fs::write(&k, &key_pem).await.context("writing key")?;
    tokio::fs::write(&ca, &ca_pem).await.context("writing CA chain")?;

    // Restrict key file permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&k, std::fs::Permissions::from_mode(0o600))
            .await
            .context("setting key file permissions")?;
    }

    println!("Certificate written to {}", c.display());
    Ok((c, k, ca))
}

/// Load certificate and key from disk (returns raw PEM strings).
pub async fn load(cert_path: &Path, key_path: &Path) -> Result<(String, String)> {
    let cert = tokio::fs::read_to_string(cert_path)
        .await
        .with_context(|| format!("reading cert from {}", cert_path.display()))?;
    let key = tokio::fs::read_to_string(key_path)
        .await
        .with_context(|| format!("reading key from {}", key_path.display()))?;
    Ok((cert, key))
}

/// Fetch and cache the Step-CA root certificate.
///
/// Uses an unauthenticated TLS-insecure request — acceptable for the one-time
/// bootstrap, the same pattern as `step ca bootstrap`.  The downloaded PEM is
/// then used for all subsequent verified connections.
pub async fn bootstrap_ca_root(ca_url: &str, out_path: &Path) -> Result<String> {
    // Only download if the file does not already exist.
    if out_path.exists() {
        return tokio::fs::read_to_string(out_path)
            .await
            .context("reading cached CA root PEM");
    }

    let client = reqwest::ClientBuilder::new()
        .danger_accept_invalid_certs(true)
        .build()
        .context("building bootstrap HTTP client")?;

    // Step-CA exposes the root cert at GET /roots as JSON {"crts": ["<pem>", ...]}.
    #[derive(serde::Deserialize)]
    struct RootsResponse {
        crts: Vec<serde_json::Value>,
    }

    let url = format!("{ca_url}/roots");
    let resp: RootsResponse = client
        .get(&url)
        .send()
        .await
        .context("fetching CA roots")?
        .json()
        .await
        .context("parsing CA roots response")?;

    // The first root is the active one; Step-CA returns its PEM inline.
    // Each entry in `crts` is a JSON object with a `raw` (base64 DER) field.
    // Re-encode to PEM using the standard header.
    let first = resp.crts.into_iter().next()
        .context("CA returned empty roots list")?;

    // Step-CA actually returns PEM strings directly in some versions; try that first.
    let pem = if let Some(pem_str) = first.as_str() {
        pem_str.to_string()
    } else {
        // Fall back: grab the `raw` base64 DER field and wrap it.
        let raw = first["raw"].as_str()
            .context("CA root entry missing 'raw' field")?;
        format!("-----BEGIN CERTIFICATE-----\n{raw}\n-----END CERTIFICATE-----\n")
    };

    if let Some(parent) = out_path.parent() {
        tokio::fs::create_dir_all(parent).await.context("creating config dir")?;
    }
    tokio::fs::write(out_path, &pem).await.context("writing CA root PEM")?;

    println!("CA root certificate bootstrapped to {}", out_path.display());
    Ok(pem)
}

/// Read a PEM-encoded X.509 cert and return its Subject CN.
/// Returns `None` if the file can't be read, isn't valid PEM, or has no CN
/// in its Subject — all cases the caller should treat as "force renewal".
async fn cert_subject_cn(cert_path: &Path) -> Option<String> {
    use x509_parser::prelude::FromDer;

    let pem_text = tokio::fs::read(cert_path).await.ok()?;
    let (_, pem) = x509_parser::pem::parse_x509_pem(&pem_text).ok()?;
    let (_, cert) = x509_parser::certificate::X509Certificate::from_der(&pem.contents).ok()?;
    cert.subject()
        .iter_common_name()
        .next()?
        .as_str()
        .ok()
        .map(str::to_string)
}

/// Returns true if the cert file is missing, older than `max_age`, or was
/// issued for a different `expected_cn` than the current operator.
///
/// `expected_cn` is optional: pass `Some(cn)` from the router/agent paths
/// to force renewal when the cached cert was issued for a different user
/// (the cert files live at the same path regardless of which user is signed
/// in, so without the check, switching accounts silently keeps presenting
/// the old identity and the TLS SAN mismatches at the next handshake).
/// Pass `None` from generic tooling that only cares about file age.
pub async fn needs_renewal(
    cert_path: &Path,
    max_age: std::time::Duration,
    expected_cn: Option<&str>,
) -> bool {
    let Ok(meta) = tokio::fs::metadata(cert_path).await else { return true };
    if meta
        .modified()
        .map(|m| m.elapsed().unwrap_or(max_age) >= max_age)
        .unwrap_or(true)
    {
        return true;
    }
    if let Some(expected) = expected_cn {
        return match cert_subject_cn(cert_path).await {
            Some(cn) if cn == expected => false,
            _ => true,
        };
    }
    false
}
