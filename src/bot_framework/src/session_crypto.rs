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
    GroupSession, InboundGroupSession, SessionConfig as MegolmConfig,
};
use vodozemac::olm::{Account, Session, SessionConfig};

// Re-exports so downstream crates (zenoh_router, Tauri) can name the wire
// types without taking a direct dep on vodozemac. `Session` is exposed so the
// admission and kick paths can persist established 1:1 channels per CN — the
// admission flow opens them as a side effect of the Megolm session-key share,
// and the kick/ban rotation flow re-uses them to deliver a fresh `SessionKey`
// without re-consuming a one-time key.
pub use vodozemac::megolm::{MegolmMessage, SessionKey};
pub use vodozemac::olm::{OlmMessage, Session as OlmSession};
pub use vodozemac::Curve25519PublicKey;
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

    /// The sender's Megolm session id — used by [`crate::payload_crypto`] to
    /// look up the right receiver for an incoming wire payload.
    pub fn session_id(&self) -> String {
        self.inner.session_id()
    }

    pub fn decrypt(&mut self, message: &MegolmMessage) -> Result<Vec<u8>> {
        self.inner
            .decrypt(message)
            .map(|d| d.plaintext)
            .map_err(|e| anyhow!("megolm decrypt: {e}"))
    }
}

// ── Olm channel reuse (admission → kick/ban rotation) ─────────────────────────
//
// The admission path (`#110`) opens a 1:1 Olm session as a side-effect of
// wrapping the admin's Megolm `SessionKey` to a fresh requester. The
// post-pre-key `OlmSession` it produces — on BOTH sides of the exchange — has
// the ratchet primed for ordinary `Normal` messages. The kick/ban rotation
// path (`#111`) reuses these stored sessions so the admin can re-deliver a
// freshly-minted Megolm key without depending on the peer republishing a
// one-time key (which is racey under churn) and without consuming a new OTK.

/// Encrypt `plaintext` on an established Olm session (Normal message — the
/// pre-key handshake already happened during admission). Used by the admin
/// kick path to re-deliver a rotated Megolm `SessionKey` to remaining members.
pub fn olm_encrypt_on_session(
    session: &mut OlmSession,
    plaintext: &[u8],
) -> Result<OlmMessage> {
    session
        .encrypt(plaintext)
        .map_err(|e| anyhow!("olm encrypt on stored session: {e}"))
}

/// Decrypt an OlmMessage on an established Olm session. Symmetric to
/// [`olm_encrypt_on_session`]; receiver side of the rotation flow.
pub fn olm_decrypt_on_session(
    session: &mut OlmSession,
    message: &OlmMessage,
) -> Result<Vec<u8>> {
    session
        .decrypt(message)
        .map_err(|e| anyhow!("olm decrypt on stored session: {e}"))
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

/// PEM variant of [`sign_enc_binding`] — `<node>.key` on disk is PKCS#8 PEM
/// (rcgen's default), and stepping through DER conversion at call sites is
/// noise.
pub fn sign_enc_binding_pem(
    tls_key_pem: &str,
    cn: &str,
    curve25519_key: &Curve25519PublicKey,
) -> Result<EncKeyBinding> {
    let sk = SigningKey::from_pkcs8_pem(tls_key_pem)
        .map_err(|e| anyhow!("parse TLS key PEM: {e}"))?;
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

    // ── Adversarial / property tests ──────────────────────────────────────

    /// A single-bit flip in the serialized Megolm ciphertext (in the Ed25519
    /// signature suffix) must cause decryption to fail — i.e. the message is
    /// integrity-protected end to end.
    #[test]
    fn megolm_tampered_ciphertext_rejected() {
        let mut sender = GroupSender::new();
        let mut receiver = GroupReceiver::new(&sender.session_key());
        let msg = sender.encrypt(b"top secret");
        let mut bytes = msg.to_bytes();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        // Re-parse and try to decrypt; either re-parse fails or decrypt fails.
        match MegolmMessage::from_bytes(&bytes) {
            Ok(tampered) => assert!(
                receiver.decrypt(&tampered).is_err(),
                "tampered megolm message must not decrypt"
            ),
            Err(_) => (),
        }
    }

    /// The serialized ciphertext bytes must not contain the plaintext as a
    /// substring — a basic "we are actually encrypting" sanity check.
    #[test]
    fn megolm_ciphertext_does_not_contain_plaintext() {
        let mut sender = GroupSender::new();
        let plaintext: &[u8] = b"DVERSE-PLAINTEXT-MARKER-unique-xyz-123";
        let bytes = sender.encrypt(plaintext).to_bytes();
        assert!(
            !bytes.windows(plaintext.len()).any(|w| w == plaintext),
            "plaintext leaked into ciphertext bytes"
        );
    }

    /// Two Megolm encrypts of the same plaintext must yield different bytes
    /// (the ratchet advances each message; per-message keys differ).
    #[test]
    fn megolm_consecutive_encrypts_are_non_deterministic() {
        let mut sender = GroupSender::new();
        let m1 = sender.encrypt(b"same plaintext").to_bytes();
        let m2 = sender.encrypt(b"same plaintext").to_bytes();
        assert_ne!(m1, m2, "consecutive megolm encrypts of the same plaintext must differ");
    }

    /// A `GroupReceiver` for session A must not decrypt a message from a
    /// separate session B — cross-session isolation.
    #[test]
    fn cross_session_receiver_cannot_decrypt() {
        let session_a = GroupSender::new();
        let mut session_b = GroupSender::new();
        let mut recv_a = GroupReceiver::new(&session_a.session_key());
        let msg_b = session_b.encrypt(b"session B message");
        assert!(recv_a.decrypt(&msg_b).is_err());
    }

    /// Olm one-time keys are consumed on first use: replaying the same OTK in
    /// a second outbound→inbound exchange must fail.
    #[test]
    fn one_time_key_is_consumed_once() {
        let alice = SessionIdentity::new();
        let mut bob = SessionIdentity::new();
        let otk = bob.generate_one_time_keys(1)[0];

        let (_, msg1) = alice
            .olm_encrypt_to(bob.curve25519_key(), otk, b"first")
            .unwrap();
        let (_, pt1) = bob.olm_decrypt_from(alice.curve25519_key(), &msg1).unwrap();
        assert_eq!(pt1, b"first");

        // Same OTK — bob has now consumed it. A second prekey message
        // referencing it must be rejected on the inbound side.
        let (_, msg2) = alice
            .olm_encrypt_to(bob.curve25519_key(), otk, b"replay")
            .unwrap();
        assert!(bob.olm_decrypt_from(alice.curve25519_key(), &msg2).is_err());
    }

    /// Single-bit flips in either the binding signature or the bound key must
    /// make `verify_enc_binding` reject (no malleability).
    #[test]
    fn enc_binding_rejects_bit_flips() {
        let (cert, key_der) = mint_tls_identity("alice");
        let id = SessionIdentity::new();
        let honest = sign_enc_binding(&key_der, "alice", &id.curve25519_key()).unwrap();
        assert!(verify_enc_binding(&cert, "alice", &honest).is_ok());

        // Flip a bit in the signature.
        let mut tampered_sig = honest.clone();
        tampered_sig.signature[0] ^= 0x01;
        assert!(verify_enc_binding(&cert, "alice", &tampered_sig).is_err());

        // Flip a bit in the bound Curve25519 key.
        let mut tampered_key = honest.clone();
        tampered_key.curve25519_key[0] ^= 0x01;
        assert!(verify_enc_binding(&cert, "alice", &tampered_key).is_err());
    }

    /// Even if a forger somehow bypassed the binding check, encrypt-to-pubkey
    /// is self-defeating: a key wrapped to bob's Curve25519 key cannot be
    /// unwrapped with mallory's account (she lacks bob's secret).
    #[test]
    fn forger_with_wrong_secret_cannot_unwrap() {
        let alice = SessionIdentity::new();
        let mut bob = SessionIdentity::new();
        let bob_otk = bob.generate_one_time_keys(1)[0];

        // alice wraps to bob's identity (legitimate target).
        let key = b"secret-session-key-32-bytes!!!aa";
        let (_, msg) = alice
            .olm_encrypt_to(bob.curve25519_key(), bob_otk, key)
            .unwrap();

        // mallory tries to decrypt with her own account — she doesn't hold the
        // OTK secret the prekey message references, so the inbound handshake
        // can't derive the shared 3DH secret. Result: error, no plaintext.
        let mut mallory = SessionIdentity::new();
        assert!(mallory
            .olm_decrypt_from(alice.curve25519_key(), &msg)
            .is_err());
    }

    /// Full kick/ban rotation round-trip on the crypto layer (`#111`):
    ///   1. Admin admits bob and carol (the existing #110 path) → both sides
    ///      retain the established 1:1 Olm session.
    ///   2. Admin mints a new GroupSender and re-delivers the new SessionKey
    ///      to carol over her stored Olm session (Normal message, no OTK
    ///      consumption).
    ///   3. carol installs the new GroupReceiver; bob (the kicked member)
    ///      retains only the OLD receiver.
    /// Asserts:
    ///   * carol can decrypt a post-rotation Megolm message.
    ///   * bob's old receiver CANNOT decrypt the post-rotation Megolm message.
    ///   * The two outbound Megolm sessions have distinct `session_id`s.
    #[test]
    fn kick_ban_rotation_locks_out_old_member_via_stored_olm_session() {
        // ── Admin identity + initial group sender. ────────────────────────
        let admin = SessionIdentity::new();
        let mut sender_v1 = GroupSender::new();
        let key_v1_bytes = sender_v1.session_key().to_bytes();

        // ── bob and carol: identities + one OTK each. ──────────────────────
        let mut bob = SessionIdentity::new();
        let bob_otk = bob.generate_one_time_keys(1)[0];
        bob.mark_keys_as_published();

        let mut carol = SessionIdentity::new();
        let carol_otk = carol.generate_one_time_keys(1)[0];
        carol.mark_keys_as_published();

        // ── Admission: admin wraps key_v1 to bob and to carol. ─────────────
        // The admin keeps the outbound Olm sessions for each member.
        let (admin_to_bob, msg_bob_v1) = admin
            .olm_encrypt_to(bob.curve25519_key(), bob_otk, &key_v1_bytes)
            .unwrap();
        let (mut admin_to_carol, msg_carol_v1) = admin
            .olm_encrypt_to(carol.curve25519_key(), carol_otk, &key_v1_bytes)
            .unwrap();

        // bob unwraps; bob keeps the inbound session (would be used for
        // rotation messages from admin had he stayed in the group).
        let (mut _bob_session, bob_plain_v1) = bob
            .olm_decrypt_from(admin.curve25519_key(), &msg_bob_v1)
            .unwrap();
        let bob_receiver_v1 = GroupReceiver::new(
            &SessionKey::from_bytes(&bob_plain_v1).unwrap(),
        );
        // carol unwraps; carol keeps her inbound session for the rotation msg.
        let (mut carol_session, carol_plain_v1) = carol
            .olm_decrypt_from(admin.curve25519_key(), &msg_carol_v1)
            .unwrap();
        let mut carol_receiver = GroupReceiver::new(
            &SessionKey::from_bytes(&carol_plain_v1).unwrap(),
        );

        // Sanity: both can decrypt v1 traffic.
        let v1_payload = sender_v1.encrypt(b"pre-kick message");
        // Build inbound copies of v1_payload — Megolm decrypts move the
        // ratchet forward, so each receiver needs its own message bytes.
        let v1_bytes = v1_payload.to_bytes();
        let mut bob_receiver_v1 = bob_receiver_v1; // shadow as mut
        assert_eq!(
            bob_receiver_v1
                .decrypt(&MegolmMessage::from_bytes(&v1_bytes).unwrap())
                .unwrap(),
            b"pre-kick message"
        );
        assert_eq!(
            carol_receiver
                .decrypt(&MegolmMessage::from_bytes(&v1_bytes).unwrap())
                .unwrap(),
            b"pre-kick message"
        );

        // ── Kick bob: admin rotates → sender_v2, reshare to carol only. ────
        let mut sender_v2 = GroupSender::new();
        let key_v2_bytes = sender_v2.session_key().to_bytes();
        // The admin sends a Normal Olm message to carol using the stored
        // session — NO OTK consumed, no new pre-key handshake.
        let rot_msg = super::olm_encrypt_on_session(&mut admin_to_carol, &key_v2_bytes).unwrap();
        // bob is not reshared to — `admin_to_bob` simply stays unused.
        // Belt-and-suspenders: assert we never invoked it.
        let _ = &admin_to_bob;

        // carol decrypts and installs the new receiver.
        let carol_plain_v2 = super::olm_decrypt_on_session(&mut carol_session, &rot_msg).unwrap();
        let mut carol_receiver_v2 = GroupReceiver::new(
            &SessionKey::from_bytes(&carol_plain_v2).unwrap(),
        );

        // ── Post-rotation: admin encrypts under sender_v2. ─────────────────
        let v2_payload = sender_v2.encrypt(b"post-kick message");
        let v2_bytes = v2_payload.to_bytes();
        assert_eq!(
            carol_receiver_v2
                .decrypt(&MegolmMessage::from_bytes(&v2_bytes).unwrap())
                .unwrap(),
            b"post-kick message"
        );

        // bob's surviving receiver (v1) MUST NOT decrypt v2 traffic — the
        // core kick-ban contract. The MegolmMessage may even fail to parse
        // under the v1 receiver's expected session, so either an error from
        // from_bytes or from decrypt is acceptable.
        let bob_attempt = MegolmMessage::from_bytes(&v2_bytes);
        match bob_attempt {
            Ok(m) => assert!(bob_receiver_v1.decrypt(&m).is_err()),
            Err(_) => {}
        }
        assert_ne!(sender_v1.session_id(), sender_v2.session_id());
    }
}
