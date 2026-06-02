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
//! # Scope (issue #110 carve-out follow-up)
//!
//! Same-operator agents share `cert_dir/megolm/` and can exchange keys via
//! the filesystem. **Cross-machine** key exchange (agents on different
//! operators) is not implemented here — that requires the operator to relay
//! agent session keys via Olm to admitted members. Documented as a
//! follow-up; the demo target is local ping↔pong.
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

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::session_crypto::{GroupReceiver, GroupSender, MegolmMessage, SessionKey};

const SENDER_FILE_SUFFIX: &str = ".sender.b64";

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
