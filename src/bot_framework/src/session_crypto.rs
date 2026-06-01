//! Per-session group encryption via [vodozemac](https://github.com/matrix-org/vodozemac)
//! (matrix.org's audited Olm/Megolm). Tracking issue: #109.
//!
//! Model (dverse session ≈ a Matrix room without a homeserver, over Zenoh):
//!   * Each node owns a vodozemac [`Account`]. Its **Curve25519** identity key is
//!     the per-node *encryption* identity — distinct from the step-CA P-256 TLS
//!     cert key. It is **bound to the node's identity** by an ECDSA-P256
//!     signature made with the TLS key over `(cn ‖ curve25519_key)`, verified
//!     against the cert CN ([`sign_enc_binding`] / [`verify_enc_binding`]).
//!     This binding is required because Zenoh doesn't expose the publisher mTLS
//!     CN to the application layer (ported from the #107 spike; only the bound
//!     key type changed from a P-256 enc key to the Curve25519 identity).
//!   * A session is a **Megolm** group session ([`GroupSender`] /
//!     [`GroupReceiver`]) — a ratcheting sender key with per-message forward
//!     secrecy. Agents encrypt payloads with it.
//!   * The Megolm session key is delivered to an admitted member over a 1:1
//!     **Olm** session ([`SessionIdentity::olm_encrypt_to`] /
//!     [`SessionIdentity::olm_decrypt_from`]).
//!   * Kick/ban → mint a new [`GroupSender`] and re-deliver to the remaining
//!     members; the removed node can't decrypt post-rotation traffic.
//!
//! Olm/Megolm here use `SessionConfig::version_1()` (libolm-compatible, no
//! crate features required).

use anyhow::{anyhow, bail, Result};
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::pkcs8::DecodePrivateKey;
use vodozemac::megolm::{
    GroupSession, InboundGroupSession, MegolmMessage, SessionConfig as MegolmConfig, SessionKey,
};
use vodozemac::olm::{Account, OlmMessage, Session, SessionConfig};
use vodozemac::Curve25519PublicKey;
use x509_parser::prelude::*;

// ── Node identity (vodozemac Account) ─────────────────────────────────────────

/// A node's vodozemac identity plus its one-time-key supply. The Curve25519
/// identity key is the encryption identity bound to the step-CA cert.
pub struct SessionIdentity {
    account: Account,
}

impl SessionIdentity {
    /// Create a fresh vodozemac account.
    pub fn new() -> Self {
        Self { account: Account::new() }
    }

    /// The Curve25519 identity (encryption) key — bound to the cert, and the
    /// target of Olm sessions that deliver Megolm keys to this node.
    pub fn curve25519_key(&self) -> Curve25519PublicKey {
        self.account.curve25519_key()
    }

    /// Base64 Curve25519 identity key, for the wire / join-request.
    pub fn curve25519_key_base64(&self) -> String {
        self.account.curve25519_key().to_base64()
    }

    /// Base64 Ed25519 fingerprint key.
    pub fn ed25519_key_base64(&self) -> String {
        self.account.ed25519_key().to_base64()
    }

    /// Generate `count` one-time keys (consumed by peers establishing an Olm
    /// session to deliver this node a Megolm key). Returns the new public OTKs
    /// to publish; call [`Self::mark_keys_as_published`] once published.
    pub fn generate_one_time_keys(&mut self, count: usize) -> Vec<Curve25519PublicKey> {
        self.account.generate_one_time_keys(count).created
    }

    pub fn mark_keys_as_published(&mut self) {
        self.account.mark_keys_as_published();
    }

    /// Establish an outbound Olm session to a peer (by their identity key and
    /// one of their one-time keys) and encrypt `plaintext` (e.g. a Megolm
    /// [`SessionKey`]) for them. Returns the session and the pre-key message.
    pub fn olm_encrypt_to(
        &self,
        peer_identity: Curve25519PublicKey,
        peer_one_time_key: Curve25519PublicKey,
        plaintext: &[u8],
    ) -> Result<(Session, OlmMessage)> {
        let mut session = self
            .account
            .create_outbound_session(SessionConfig::version_1(), peer_identity, peer_one_time_key)
            .map_err(|e| anyhow!("olm outbound session: {e}"))?;
        let msg = session
            .encrypt(plaintext)
            .map_err(|e| anyhow!("olm encrypt: {e}"))?;
        Ok((session, msg))
    }

    /// Accept an inbound Olm pre-key message from a peer and decrypt it,
    /// establishing the inbound session. Consumes a one-time key.
    pub fn olm_decrypt_from(
        &mut self,
        peer_identity: Curve25519PublicKey,
        message: &OlmMessage,
    ) -> Result<(Session, Vec<u8>)> {
        match message {
            OlmMessage::PreKey(pre) => {
                let res = self
                    .account
                    .create_inbound_session(SessionConfig::version_1(), peer_identity, pre)
                    .map_err(|e| anyhow!("olm inbound session: {e}"))?;
                Ok((res.session, res.plaintext))
            }
            OlmMessage::Normal(_) => {
                bail!("expected an Olm pre-key message to establish the session")
            }
        }
    }
}

impl Default for SessionIdentity {
    fn default() -> Self {
        Self::new()
    }
}

// ── Megolm group session (payload encryption) ─────────────────────────────────

/// Outbound Megolm session: encrypts this node's session payloads. Its
/// [`session_key`](Self::session_key) is shared (over Olm) with every member.
pub struct GroupSender {
    inner: GroupSession,
}

impl GroupSender {
    pub fn new() -> Self {
        Self { inner: GroupSession::new(MegolmConfig::version_1()) }
    }

    /// The key to deliver to members so they can decrypt this sender's messages.
    pub fn session_key(&self) -> SessionKey {
        self.inner.session_key()
    }

    pub fn session_id(&self) -> String {
        self.inner.session_id()
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> MegolmMessage {
        self.inner.encrypt(plaintext)
    }
}

impl Default for GroupSender {
    fn default() -> Self {
        Self::new()
    }
}

/// Inbound Megolm session: decrypts one sender's messages, built from the
/// [`SessionKey`] delivered over Olm.
pub struct GroupReceiver {
    inner: InboundGroupSession,
}

impl GroupReceiver {
    pub fn new(key: &SessionKey) -> Self {
        Self { inner: InboundGroupSession::new(key, MegolmConfig::version_1()) }
    }

    pub fn decrypt(&mut self, message: &MegolmMessage) -> Result<Vec<u8>> {
        self.inner
            .decrypt(message)
            .map(|d| d.plaintext)
            .map_err(|e| anyhow!("megolm decrypt: {e}"))
    }
}

// ── Identity binding (Curve25519 enc key ⇄ step-CA P-256 cert) ─────────────────

/// A node's claim that its Curve25519 encryption key belongs to its cert CN,
/// signed with the node's TLS (P-256) key. Travels in the join-request.
#[derive(Debug, Clone)]
pub struct EncKeyBinding {
    pub cn: String,
    pub curve25519_key: [u8; 32],
    /// ECDSA-P256 signature over `binding_message(cn, curve25519_key)`.
    pub signature: Vec<u8>,
}

/// Domain-separated, length-prefixed message so `cn` and the key can't be
/// ambiguously concatenated.
fn binding_message(cn: &str, curve25519_key: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(b"dverse/enc-key-binding/v1\0");
    m.extend_from_slice(&(cn.len() as u32).to_be_bytes());
    m.extend_from_slice(cn.as_bytes());
    m.extend_from_slice(curve25519_key);
    m
}

/// Sign the binding of `curve25519_key` to `cn` with the node's TLS P-256
/// private key (PKCS#8 DER, as produced by rcgen / loaded from `<node>.key`).
pub fn sign_enc_binding(
    tls_key_pkcs8_der: &[u8],
    cn: &str,
    curve25519_key: &Curve25519PublicKey,
) -> Result<EncKeyBinding> {
    let sk = SigningKey::from_pkcs8_der(tls_key_pkcs8_der)
        .map_err(|e| anyhow!("parse TLS key: {e}"))?;
    let key_bytes = curve25519_key.to_bytes();
    let sig: Signature = sk.sign(&binding_message(cn, &key_bytes));
    Ok(EncKeyBinding { cn: cn.to_string(), curve25519_key: key_bytes, signature: sig.to_bytes().to_vec() })
}

/// Verify a binding against the requester's TLS cert (PEM). On success returns
/// the now-trusted Curve25519 encryption key. Rejects on claimed-CN mismatch,
/// cert-CN mismatch, or invalid signature.
pub fn verify_enc_binding(
    tls_cert_pem: &str,
    expected_cn: &str,
    b: &EncKeyBinding,
) -> Result<Curve25519PublicKey> {
    if b.cn != expected_cn {
        bail!("claimed CN {:?} != expected {:?}", b.cn, expected_cn);
    }
    let (_, pem) = parse_x509_pem(tls_cert_pem.as_bytes()).map_err(|e| anyhow!("pem parse: {e}"))?;
    let cert = pem.parse_x509().map_err(|e| anyhow!("x509 parse: {e}"))?;
    let cert_cn = cert
        .subject()
        .iter_common_name()
        .next()
        .and_then(|a| a.as_str().ok())
        .ok_or_else(|| anyhow!("cert has no CN"))?;
    if cert_cn != b.cn {
        bail!("cert CN {:?} != claimed CN {:?}", cert_cn, b.cn);
    }
    let spki = cert.public_key().subject_public_key.data.as_ref();
    let vk = VerifyingKey::from_sec1_bytes(spki).map_err(|e| anyhow!("cert SPKI: {e}"))?;
    let sig = Signature::from_slice(&b.signature).map_err(|e| anyhow!("sig parse: {e}"))?;
    vk.verify(&binding_message(&b.cn, &b.curve25519_key), &sig)
        .map_err(|_| anyhow!("binding signature invalid"))?;
    Curve25519PublicKey::from_slice(&b.curve25519_key).map_err(|e| anyhow!("curve25519 key: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mint a P-256 self-signed cert (stand-in for a step-CA leaf): returns
    /// (cert_pem, pkcs8_key_der).
    fn mint_tls_identity(cn: &str) -> (String, Vec<u8>) {
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, cn);
        params.distinguished_name = dn;
        let kp = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&kp).unwrap();
        (cert.pem(), kp.serialize_der())
    }

    #[test]
    fn megolm_round_trip_and_rotation() {
        let mut sender_v1 = GroupSender::new();
        let mut receiver = GroupReceiver::new(&sender_v1.session_key());
        let msg = sender_v1.encrypt(b"hello session");
        assert_eq!(receiver.decrypt(&msg).unwrap(), b"hello session");

        // Rotation: a new sender; the old receiver cannot decrypt the new
        // sender's messages (different Megolm session) — i.e. a kicked member
        // holding only the old inbound session is locked out. The new key is
        // shared at session start (before the first encrypt), as in real use.
        let mut sender_v2 = GroupSender::new();
        let mut receiver_v2 = GroupReceiver::new(&sender_v2.session_key());
        let v2_msg = sender_v2.encrypt(b"post-rotation");
        assert_ne!(sender_v1.session_id(), sender_v2.session_id());
        assert!(receiver.decrypt(&v2_msg).is_err());
        assert_eq!(receiver_v2.decrypt(&v2_msg).unwrap(), b"post-rotation");
    }

    #[test]
    fn olm_delivers_megolm_key_end_to_end() {
        // Admin (alice) has a Megolm sender; bob is admitted.
        let alice = SessionIdentity::new();
        let mut bob = SessionIdentity::new();
        let bob_otks = bob.generate_one_time_keys(1);

        let mut alice_sender = GroupSender::new();
        let session_key_bytes = alice_sender.session_key().to_bytes();

        // alice Olm-encrypts the Megolm session key to bob.
        let (_alice_session, olm_msg) = alice
            .olm_encrypt_to(bob.curve25519_key(), bob_otks[0], &session_key_bytes)
            .unwrap();

        // bob decrypts it, rebuilds the inbound Megolm session, and reads a
        // payload alice encrypts under the group session.
        let (_bob_session, recovered) = bob.olm_decrypt_from(alice.curve25519_key(), &olm_msg).unwrap();
        let key = SessionKey::from_bytes(&recovered).unwrap();
        let mut bob_receiver = GroupReceiver::new(&key);

        let payload = alice_sender.encrypt(b"hello over the shared fabric");
        assert_eq!(bob_receiver.decrypt(&payload).unwrap(), b"hello over the shared fabric");
    }

    #[test]
    fn enc_key_binding_verifies_and_rejects_forgery() {
        let (bob_cert, bob_key_der) = mint_tls_identity("bob");
        let bob_enc = SessionIdentity::new();

        // Honest binding: bob signs his Curve25519 key with his TLS key.
        let binding = sign_enc_binding(&bob_key_der, "bob", &bob_enc.curve25519_key()).unwrap();
        let trusted = verify_enc_binding(&bob_cert, "bob", &binding).unwrap();
        assert_eq!(trusted.to_bytes(), bob_enc.curve25519_key().to_bytes());

        // Wrong expected CN is rejected.
        assert!(verify_enc_binding(&bob_cert, "alice", &binding).is_err());

        // Forgery: mallory substitutes her own enc key under bob's cert/CN,
        // signed with HER TLS key. Must fail (sig not valid under bob's cert).
        let (_mallory_cert, mallory_key_der) = mint_tls_identity("mallory");
        let mallory_enc = SessionIdentity::new();
        let forged = sign_enc_binding(&mallory_key_der, "bob", &mallory_enc.curve25519_key()).unwrap();
        // forged.cn == "bob" but it was signed with mallory's key; verified
        // against bob's real (public) cert it must be rejected.
        assert!(verify_enc_binding(&bob_cert, "bob", &forged).is_err());
    }
}
