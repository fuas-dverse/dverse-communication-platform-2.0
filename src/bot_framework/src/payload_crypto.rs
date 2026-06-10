//! Per-agent payload encryption (Megolm). What this module gives an agent:
//!
//! * An outbound [`GroupSender`](crate::session_crypto::GroupSender) it owns
//!   exclusively. Megolm's ratchet advances on every encrypt and **cannot be
//!   safely shared across processes** — two writers on the same sender would
//!   diverge their chain index and produce messages a receiver can't decrypt.
//!   So every process has its own sender with a unique `session_id`.
//!
//! * A set of inbound [`GroupReceiver`](crate::session_crypto::GroupReceiver)s,
//!   one per peer sender we want to read. Receivers are keyed by the peer's
//!   `session_id`; the wire format includes that id so receiver lookup is
//!   O(1) instead of probing every receiver.
//!
//! * A file-based key bus: each agent writes its outbound session key as
//!   base64 to `dir/<my_id>.sender.b64`. Other agents read every
//!   `*.sender.b64` in the directory and install matching receivers.
//!
//! # Cross-machine key exchange
//!
//! Same-operator agents share `cert_dir/megolm/` and can exchange keys via
//! the filesystem ([`refresh_receivers`](PayloadCipher::refresh_receivers)).
//! Cross-operator agents use the Zenoh-side relay
//! ([`spawn_agent_key_relay`]): each agent publishes its own `SessionKey`
//! on `dverse/agent_keys/<cn>/<agent_name>` and subscribes to the same
//! key-expr to install peers'. That topic falls under the admitted-only
//! main ACL rule, so non-admitted nodes on the fabric can't read it.
//! Periodic re-publish (every 5 s) lets late joiners catch up.
//!
//! # Wire format
//!
//! JSON `EncryptedPayload`:
//! ```json
//! { "sender_id": "<base64 megolm session_id>", "ciphertext_b64": "<base64>" }
//! ```
//!
//! # Tracing
//!
//! `info!` log at every encrypt / decrypt / receiver install:
//! `cipher=megolm session_id=... plaintext_len=... ciphertext_len=...`.
//! A grep for `cipher = megolm` shows the data plane traffic.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use zenoh::Session;

use crate::session_crypto::{GroupReceiver, GroupSender, MegolmMessage, SessionKey};

const SENDER_FILE_SUFFIX: &str = ".sender.b64";
/// How often each agent re-publishes its session key on the wire. The key
/// never changes within a process lifetime, so re-puts are idempotent — this
/// is just how long a fresh joiner has to wait before existing agents'
/// keys reach it.
const AGENT_KEY_REPUBLISH_INTERVAL: Duration = Duration::from_secs(5);
/// Topic prefix for the cross-machine agent-key relay. Falls under the
/// router's main-rule ACL (`dverse/**` allowed only for admitted CNs), so
/// non-admitted nodes can't read these keys even though they're on the
/// same Zenoh fabric.
const AGENT_KEYS_TOPIC_PREFIX: &str = "dverse/agent_keys";
const AGENT_KEYS_SUBSCRIBE_EXPR: &str = "dverse/agent_keys/**";

/// One AEAD-protected payload on the wire. The `sender_id` lets the receiver
/// dispatch to the right [`GroupReceiver`] in O(1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    pub sender_id: String,
    pub ciphertext_b64: String,
}

/// Per-process Megolm state: one outbound sender + N inbound receivers keyed
/// by peer `session_id`. Holds the key directory it was created against so
/// callers can [`refresh_receivers`](Self::refresh_receivers) on demand.
pub struct PayloadCipher {
    sender: GroupSender,
    receivers: HashMap<String, GroupReceiver>,
    /// Stable label included in tracing logs (e.g. the agent name).
    label: String,
}

impl PayloadCipher {
    /// Mint a fresh sender and load every peer session key already present
    /// in `dir`. Creates `dir` if it doesn't exist.
    pub fn new(label: impl Into<String>, dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| anyhow!("create key dir: {e}"))?;
        let sender = GroupSender::new();
        let mut me = Self { sender, receivers: HashMap::new(), label: label.into() };
        let installed = me.refresh_receivers(dir)?;
        info!(
            cipher = "megolm",
            label = %me.label,
            session_id = %me.sender.session_id(),
            peer_receivers = installed,
            "PayloadCipher initialized"
        );
        Ok(me)
    }

    /// The session_id of our outbound sender. Peers need this to know which
    /// receiver to dispatch our messages to.
    pub fn my_session_id(&self) -> String {
        self.sender.session_id()
    }

    /// How many inbound peer receivers we currently hold.
    pub fn receiver_count(&self) -> usize {
        self.receivers.len()
    }

    /// Write our outbound session key to `dir/<my_id>.sender.b64`. Peers
    /// reading the directory install a [`GroupReceiver`] from it. Safe to
    /// call multiple times — the file is overwritten with the same key, since
    /// we don't rotate the sender within a process lifetime.
    pub fn publish_session_key(&self, dir: &Path, my_id: &str) -> Result<()> {
        let path = dir.join(format!("{}{}", my_id, SENDER_FILE_SUFFIX));
        std::fs::write(&path, self.sender.session_key().to_base64())
            .map_err(|e| anyhow!("write session key {}: {e}", path.display()))?;
        info!(
            cipher = "megolm",
            label = %self.label,
            session_id = %self.sender.session_id(),
            path = %path.display(),
            "published outbound session key"
        );
        Ok(())
    }

    /// Rescan `dir` for `*.sender.b64` files and install receivers for any
    /// new ones. Returns the number of newly installed receivers.
    pub fn refresh_receivers(&mut self, dir: &Path) -> Result<usize> {
        let read_dir = match std::fs::read_dir(dir) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(anyhow!("read key dir {}: {e}", dir.display())),
        };
        let mut newly_installed = 0usize;
        for entry in read_dir {
            let entry = entry.map_err(|e| anyhow!("dir entry: {e}"))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ends_correctly = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.ends_with(SENDER_FILE_SUFFIX))
                .unwrap_or(false);
            if !ends_correctly {
                continue;
            }
            let key_b64 = std::fs::read_to_string(&path)
                .map_err(|e| anyhow!("read {}: {e}", path.display()))?;
            let key = match SessionKey::from_base64(key_b64.trim()) {
                Ok(k) => k,
                Err(e) => {
                    warn!(
                        cipher = "megolm",
                        path = %path.display(),
                        error = %e,
                        "skipping malformed session key file"
                    );
                    continue;
                }
            };
            let receiver = GroupReceiver::new(&key);
            let sid = receiver.session_id();
            // Don't reinstall the receiver for our own sender — we don't
            // need to decrypt our own ciphertext, and reinstalling would
            // shadow the fresh receiver with a stale-keyed copy.
            if sid == self.sender.session_id() {
                continue;
            }
            if self.receivers.contains_key(&sid) {
                continue;
            }
            self.receivers.insert(sid.clone(), receiver);
            newly_installed += 1;
            info!(
                cipher = "megolm",
                label = %self.label,
                peer_session_id = %sid,
                path = %path.display(),
                "installed inbound receiver from peer session key"
            );
        }
        Ok(newly_installed)
    }

    /// Encrypt `plaintext` under our sender and return the wire bytes
    /// (JSON-serialized [`EncryptedPayload`]).
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let msg = self.sender.encrypt(plaintext);
        let payload = EncryptedPayload {
            sender_id: self.sender.session_id(),
            ciphertext_b64: msg.to_base64(),
        };
        let bytes = serde_json::to_vec(&payload)
            .map_err(|e| anyhow!("serialize EncryptedPayload: {e}"))?;
        info!(
            cipher = "megolm",
            op = "encrypt",
            label = %self.label,
            session_id = %self.sender.session_id(),
            plaintext_len = plaintext.len(),
            ciphertext_len = bytes.len(),
            "encrypted payload"
        );
        Ok(bytes)
    }

    /// Decrypt wire `bytes`. Looks up the receiver by `sender_id`; errors if
    /// we have no receiver for that sender (peer's session key hasn't been
    /// shared / installed yet) or the ciphertext is corrupt.
    pub fn decrypt(&mut self, bytes: &[u8]) -> Result<Vec<u8>> {
        let payload: EncryptedPayload = serde_json::from_slice(bytes)
            .map_err(|e| anyhow!("parse EncryptedPayload: {e}"))?;
        let receiver = self
            .receivers
            .get_mut(&payload.sender_id)
            .ok_or_else(|| anyhow!("no receiver for sender_id {}", payload.sender_id))?;
        let msg = MegolmMessage::from_base64(&payload.ciphertext_b64)
            .map_err(|e| anyhow!("MegolmMessage::from_base64: {e}"))?;
        let plaintext = receiver.decrypt(&msg)?;
        info!(
            cipher = "megolm",
            op = "decrypt",
            label = %self.label,
            peer_session_id = %payload.sender_id,
            ciphertext_len = bytes.len(),
            plaintext_len = plaintext.len(),
            "decrypted payload"
        );
        Ok(plaintext)
    }

    /// The exported `SessionKey` of our outbound sender, base64. Distributable
    /// to peers via the [`spawn_agent_key_relay`] wire bus so they can install
    /// a [`GroupReceiver`] for us.
    pub fn my_session_key_b64(&self) -> String {
        self.sender.session_key().to_base64()
    }

    /// Install a peer's session key from a base64 string (programmatic
    /// alternative to [`refresh_receivers`](Self::refresh_receivers), which
    /// scans a directory). Returns `true` when a new receiver was installed,
    /// `false` if it was our own session_id or already installed.
    pub fn install_peer_session_key(&mut self, key_b64: &str) -> Result<bool> {
        let key = SessionKey::from_base64(key_b64.trim())
            .map_err(|e| anyhow!("parse session key: {e}"))?;
        let receiver = GroupReceiver::new(&key);
        let sid = receiver.session_id();
        if sid == self.sender.session_id() {
            return Ok(false);
        }
        if self.receivers.contains_key(&sid) {
            return Ok(false);
        }
        self.receivers.insert(sid.clone(), receiver);
        info!(
            cipher = "megolm",
            label = %self.label,
            peer_session_id = %sid,
            "installed inbound receiver from wire announce"
        );
        Ok(true)
    }
}

// ── Wire bus for cross-machine peer-key exchange ────────────────────────────

/// JSON envelope put on `dverse/agent_keys/<cn>/<agent_name>`. Sender's
/// `SessionKey` exported as base64; the receiving agent reconstructs a
/// [`GroupReceiver`] from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentKeyAnnounce {
    session_key_b64: String,
}

/// Spawn two background tokio tasks that relay peer session keys via Zenoh,
/// closing the cross-machine gap left by the filesystem-only mechanism:
///
/// 1. **Announce** — every [`AGENT_KEY_REPUBLISH_INTERVAL`], put our outbound
///    `SessionKey` on `dverse/agent_keys/<cn>/<agent_name>`. Late joiners
///    get covered by the next tick. Idempotent: the key never changes.
/// 2. **Subscribe** — declare a subscriber on `dverse/agent_keys/**` and
///    feed each received envelope into
///    [`PayloadCipher::install_peer_session_key`].
///
/// The topic falls under the router's main ACL rule (allow-only-for-
/// admitted-CNs on `dverse/**`), so non-admitted nodes can't read it.
///
/// The tasks live for the process lifetime; on Zenoh errors they log and
/// exit (the next process start re-launches them).
pub fn spawn_agent_key_relay(
    cipher: Arc<Mutex<PayloadCipher>>,
    session: Session,
    cn: String,
    agent_name: String,
) {
    // Announce task.
    {
        let topic = format!("{}/{}/{}", AGENT_KEYS_TOPIC_PREFIX, cn, agent_name);
        let sess = session.clone();
        let cip = Arc::clone(&cipher);
        let label = agent_name.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(AGENT_KEY_REPUBLISH_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let envelope = {
                    let c = cip.lock().unwrap();
                    AgentKeyAnnounce { session_key_b64: c.my_session_key_b64() }
                };
                let bytes = match serde_json::to_vec(&envelope) {
                    Ok(b) => b,
                    Err(e) => {
                        warn!(error = %e, "serialize AgentKeyAnnounce");
                        continue;
                    }
                };
                if let Err(e) = sess.put(&topic, bytes).await {
                    warn!(
                        cipher = "megolm",
                        label = %label,
                        error = %e,
                        "agent_keys republish failed, stopping relay announce task"
                    );
                    break;
                }
            }
        });
    }

    // Subscribe task.
    tokio::spawn(async move {
        let sub = match session.declare_subscriber(AGENT_KEYS_SUBSCRIBE_EXPR).await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "agent_keys subscriber failed to start");
                return;
            }
        };
        loop {
            let sample = match sub.recv_async().await {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "agent_keys subscriber recv error, stopping");
                    break;
                }
            };
            let bytes = sample.payload().to_bytes();
            let envelope: AgentKeyAnnounce = match serde_json::from_slice(&bytes) {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, "ignored malformed AgentKeyAnnounce");
                    continue;
                }
            };
            if let Err(e) = cipher
                .lock()
                .unwrap()
                .install_peer_session_key(&envelope.session_key_b64)
            {
                warn!(error = %e, "ignored malformed peer session key");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn two_agents_round_trip_via_filesystem() {
        let dir = TempDir::new().unwrap();

        // Each agent owns its own sender. They share keys via the directory.
        let mut alice = PayloadCipher::new("alice", dir.path()).unwrap();
        alice.publish_session_key(dir.path(), "alice").unwrap();

        let mut bob = PayloadCipher::new("bob", dir.path()).unwrap();
        bob.publish_session_key(dir.path(), "bob").unwrap();

        // Alice picks up bob's key now that it's on disk.
        let installed = alice.refresh_receivers(dir.path()).unwrap();
        assert_eq!(installed, 1, "alice should install bob's receiver");

        // Encrypt one direction at a time.
        let ct = alice.encrypt(b"hello bob").unwrap();
        let pt = bob.decrypt(&ct).expect("bob decrypts alice's payload");
        assert_eq!(pt, b"hello bob");

        let ct = bob.encrypt(b"hi alice").unwrap();
        let pt = alice.decrypt(&ct).expect("alice decrypts bob's payload");
        assert_eq!(pt, b"hi alice");
    }

    /// Programmatic peer-key install (vs filesystem). Mirrors the
    /// `spawn_agent_key_relay` happy path: alice and bob exchange their
    /// `session_key_b64` directly, build receivers, round-trip payloads.
    #[test]
    fn two_agents_round_trip_via_programmatic_install() {
        let dir = TempDir::new().unwrap();
        let mut alice = PayloadCipher::new("alice", dir.path()).unwrap();
        let mut bob = PayloadCipher::new("bob", dir.path()).unwrap();

        // Cross-install — no filesystem involved.
        let installed_in_alice = alice
            .install_peer_session_key(&bob.my_session_key_b64())
            .unwrap();
        let installed_in_bob = bob
            .install_peer_session_key(&alice.my_session_key_b64())
            .unwrap();
        assert!(installed_in_alice && installed_in_bob);

        // Round-trip both directions.
        let ct = alice.encrypt(b"hello bob").unwrap();
        assert_eq!(bob.decrypt(&ct).unwrap(), b"hello bob");
        let ct = bob.encrypt(b"hi alice").unwrap();
        assert_eq!(alice.decrypt(&ct).unwrap(), b"hi alice");

        // Idempotent: re-installing the same key is a no-op.
        let again = alice
            .install_peer_session_key(&bob.my_session_key_b64())
            .unwrap();
        assert!(!again, "second install of same key should return false");

        // Installing our own key is a no-op (self-echo from the wire bus).
        let self_install = alice
            .install_peer_session_key(&alice.my_session_key_b64())
            .unwrap();
        assert!(!self_install, "installing our own session_id must be skipped");
    }

    #[test]
    fn decrypt_fails_without_receiver_for_sender() {
        let dir = TempDir::new().unwrap();

        // Alice publishes a key but bob never refreshes from the directory.
        let mut alice = PayloadCipher::new("alice", dir.path()).unwrap();
        alice.publish_session_key(dir.path(), "alice").unwrap();

        // Bob never reads the key dir — empty receivers map.
        let mut bob = PayloadCipher::new("bob", dir.path()).unwrap();
        // Strip any receiver bob may have grabbed at construction time.
        bob.receivers.clear();

        let ct = alice.encrypt(b"unreadable").unwrap();
        let err = bob.decrypt(&ct).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("no receiver"), "unexpected error: {msg}");
    }

    #[test]
    fn ciphertext_does_not_contain_plaintext() {
        let dir = TempDir::new().unwrap();
        let mut alice = PayloadCipher::new("alice", dir.path()).unwrap();
        let plaintext: &[u8] = b"DVERSE-PAYLOAD-CRYPTO-MARKER-abc-789";
        let ct = alice.encrypt(plaintext).unwrap();
        // The JSON wrapper is plaintext but should not contain the secret.
        assert!(
            !ct.windows(plaintext.len()).any(|w| w == plaintext),
            "plaintext leaked into wire bytes"
        );
    }
}
