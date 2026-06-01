//! Throwaway spike for issue #107 — proves the per-session encryption
//! substrate on real P-256 certificate material before it becomes
//! load-bearing in `bot_framework::session_crypto`.
//!
//! Model:
//!   * Each node has a P-256 *TLS identity* (cert, CN) and a *separate* P-256
//!     *encryption keypair* (ECDH only).
//!   * The enc keypair is bound to the identity by an ECDSA signature made
//!     with the TLS key over `(cn ‖ enc_pubkey)`. The admin verifies that
//!     signature against the requester's cert (CN match), since Zenoh doesn't
//!     expose the publisher's mTLS CN to the application layer.
//!   * Admission = ECIES-wrap the session key to the verified enc pubkey
//!     (ephemeral ECDH → HKDF-SHA256 → AEAD). Kick = rotate the session key.
//!
//! Trust chain: step-CA → mTLS cert (identity) → ECDSA signature → enc key.
//!
//! Run: `cargo run`  (or `cargo test`). Self-signed rcgen certs stand in for
//! step-CA leaves — same key alg + SPKI structure, so the encrypt-to-cert path
//! is identical; the CA signature is irrelevant to the mechanics here.

use anyhow::{anyhow, bail, Context, Result};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{AeadCore, ChaCha20Poly1305, Key, KeyInit, Nonce};
use hkdf::Hkdf;
use p256::ecdsa::signature::{Signer, Verifier};
use p256::ecdsa::{Signature, SigningKey, VerifyingKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::pkcs8::DecodePrivateKey;
use p256::{PublicKey, SecretKey};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;
use x509_parser::prelude::*;

/// HKDF info string — domain-separates the key-wrap KDF from any other use.
const HKDF_INFO: &[u8] = b"dverse/session-key-wrap/v1";

/// A node's full key material: TLS identity (cert + signing key) plus a
/// distinct encryption keypair used only for ECDH.
struct Identity {
    cn: String,
    tls_cert_pem: String,
    tls_signing_key: SigningKey,
    enc_secret: SecretKey,
    enc_public: PublicKey,
}

/// What a requester sends so an admin can trust its encryption key:
/// the enc pubkey, an ECDSA signature over `(cn ‖ enc_pubkey)` made with the
/// TLS key, and the TLS cert that anchors the identity.
struct Binding {
    cn: String,
    enc_pub_sec1: Vec<u8>,
    signature: Vec<u8>,
    tls_cert_pem: String,
}

/// An ECIES-wrapped session key: ephemeral pubkey + AEAD nonce + ciphertext.
struct WrappedKey {
    eph_pub_sec1: Vec<u8>,
    nonce: Vec<u8>,
    ct: Vec<u8>,
}

fn sec1(pubkey: &PublicKey) -> Vec<u8> {
    pubkey.to_encoded_point(false).as_bytes().to_vec()
}

/// Mint a P-256 TLS identity (self-signed, standing in for a step-CA leaf)
/// plus a separate P-256 encryption keypair.
fn mint_identity(cn: &str) -> Result<Identity> {
    let mut params = rcgen::CertificateParams::new(Vec::<String>::new())
        .context("rcgen params")?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, cn);
    params.distinguished_name = dn;

    let kp = rcgen::KeyPair::generate().context("rcgen keypair")?; // P-256
    let cert = params.self_signed(&kp).context("self-sign")?;
    let tls_cert_pem = cert.pem();
    let tls_signing_key = SigningKey::from_pkcs8_der(&kp.serialize_der())
        .map_err(|e| anyhow!("parse TLS signing key: {e}"))?;

    let enc_secret = SecretKey::random(&mut OsRng);
    let enc_public = enc_secret.public_key();

    Ok(Identity { cn: cn.to_string(), tls_cert_pem, tls_signing_key, enc_secret, enc_public })
}

/// Unambiguous message bytes for the identity→enc-key binding signature.
fn binding_message(cn: &str, enc_pub_sec1: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(b"dverse/enc-key-binding/v1\0");
    m.extend_from_slice(&(cn.len() as u32).to_be_bytes());
    m.extend_from_slice(cn.as_bytes());
    m.extend_from_slice(enc_pub_sec1);
    m
}

/// Sign `(cn ‖ enc_pubkey)` with the TLS identity key.
fn make_binding(id: &Identity) -> Binding {
    let enc_pub_sec1 = sec1(&id.enc_public);
    let msg = binding_message(&id.cn, &enc_pub_sec1);
    let sig: Signature = id.tls_signing_key.sign(&msg);
    Binding {
        cn: id.cn.clone(),
        enc_pub_sec1,
        signature: sig.to_bytes().to_vec(),
        tls_cert_pem: id.tls_cert_pem.clone(),
    }
}

/// Verify a binding and return the now-trusted encryption public key.
/// Fails if the claimed CN, the cert CN, and the signature don't all agree.
fn verify_binding(b: &Binding, expected_cn: &str) -> Result<PublicKey> {
    if b.cn != expected_cn {
        bail!("claimed CN {:?} != expected {:?}", b.cn, expected_cn);
    }
    let (_, pem) = parse_x509_pem(b.tls_cert_pem.as_bytes())
        .map_err(|e| anyhow!("pem parse: {e}"))?;
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
    let msg = binding_message(&b.cn, &b.enc_pub_sec1);
    let sig = Signature::from_slice(&b.signature).map_err(|e| anyhow!("sig parse: {e}"))?;
    vk.verify(&msg, &sig).map_err(|_| anyhow!("binding signature invalid"))?;
    PublicKey::from_sec1_bytes(&b.enc_pub_sec1).map_err(|e| anyhow!("enc pubkey: {e}"))
}

fn random_session_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    OsRng.fill_bytes(&mut k);
    k
}

/// Derive the AEAD wrapping key from an ECDH shared secret.
fn wrap_key_from_shared(shared: &p256::ecdh::SharedSecret) -> Result<[u8; 32]> {
    let hk = Hkdf::<Sha256>::new(None, shared.raw_secret_bytes().as_slice());
    let mut wrap_key = [0u8; 32];
    hk.expand(HKDF_INFO, &mut wrap_key)
        .map_err(|_| anyhow!("hkdf expand"))?;
    Ok(wrap_key)
}

/// ECIES-wrap a session key to a recipient's encryption public key.
fn wrap_session_key(recipient_enc_pub: &PublicKey, session_key: &[u8; 32]) -> Result<WrappedKey> {
    let eph = SecretKey::random(&mut OsRng);
    let shared =
        p256::ecdh::diffie_hellman(eph.to_nonzero_scalar(), recipient_enc_pub.as_affine());
    let wrap_key = wrap_key_from_shared(&shared)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&wrap_key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(&nonce, session_key.as_slice())
        .map_err(|_| anyhow!("wrap seal"))?;
    Ok(WrappedKey { eph_pub_sec1: sec1(&eph.public_key()), nonce: nonce.to_vec(), ct })
}

/// Unwrap a session key with the recipient's encryption private key.
fn unwrap_session_key(recipient_enc_secret: &SecretKey, w: &WrappedKey) -> Result<[u8; 32]> {
    let eph_pub = PublicKey::from_sec1_bytes(&w.eph_pub_sec1)
        .map_err(|e| anyhow!("ephemeral pubkey: {e}"))?;
    let shared =
        p256::ecdh::diffie_hellman(recipient_enc_secret.to_nonzero_scalar(), eph_pub.as_affine());
    let wrap_key = wrap_key_from_shared(&shared)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&wrap_key));
    let pt = cipher
        .decrypt(Nonce::from_slice(&w.nonce), w.ct.as_ref())
        .map_err(|_| anyhow!("unwrap auth failed"))?;
    pt.try_into().map_err(|_| anyhow!("unwrapped key wrong length"))
}

/// AEAD-seal a payload under the session key. Returns `(nonce, ciphertext)`.
fn seal_payload(session_key: &[u8; 32], pt: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(session_key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ct = cipher.encrypt(&nonce, pt).map_err(|_| anyhow!("seal"))?;
    Ok((nonce.to_vec(), ct))
}

/// AEAD-open a payload under the session key.
fn open_payload(session_key: &[u8; 32], nonce: &[u8], ct: &[u8]) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(session_key));
    cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| anyhow!("open: auth failed"))
}

fn main() -> Result<()> {
    println!("=== dverse session-crypto spike (issue #107) ===\n");

    // [1] Two P-256 TLS identities + separate enc keypairs.
    let alice = mint_identity("alice")?;
    let bob = mint_identity("bob")?;
    println!("[1] minted P-256 TLS identities + separate enc keypairs (alice, bob) ✓");

    // [2] bob binds his enc pubkey to his identity with his TLS key.
    let bob_binding = make_binding(&bob);
    println!("[2] bob signed (cn ‖ enc_pubkey) with his TLS key ✓");

    // [3] admin (alice) verifies the binding before trusting bob's enc key.
    let bob_enc_pub =
        verify_binding(&bob_binding, "bob").context("verifying bob's binding")?;
    println!("[3] alice verified bob's binding (cert CN + signature) ✓");

    // [4] alice mints a session key and ECIES-wraps it to bob.
    let session_v1 = random_session_key();
    let wrapped = wrap_session_key(&bob_enc_pub, &session_v1)?;
    println!("[4] alice ECIES-wrapped the session key to bob's enc pubkey ✓");

    // [5] bob unwraps with his enc private key.
    let bob_v1 = unwrap_session_key(&bob.enc_secret, &wrapped)?;
    assert_eq!(bob_v1, session_v1, "unwrapped key must equal the original");
    println!("[5] bob unwrapped the session key; matches original ✓");

    // [6] AEAD payload round-trip under the session key.
    let msg = b"hello over the shared fabric";
    let (nonce, ct) = seal_payload(&session_v1, msg)?;
    let pt = open_payload(&bob_v1, &nonce, &ct)?;
    assert_eq!(pt, msg, "payload round-trip must match");
    println!("[6] AEAD payload sealed by alice, opened by bob ✓");

    // [7] rotation: a v1-only holder can't read v2 traffic; a v2 holder can.
    let session_v2 = random_session_key();
    let (n2, ct2) = seal_payload(&session_v2, b"post-rotation message")?;
    assert!(
        open_payload(&session_v1, &n2, &ct2).is_err(),
        "a v1-only holder must NOT decrypt v2 traffic"
    );
    let wrapped_v2 = wrap_session_key(&bob_enc_pub, &session_v2)?;
    let bob_v2 = unwrap_session_key(&bob.enc_secret, &wrapped_v2)?;
    assert_eq!(open_payload(&bob_v2, &n2, &ct2)?, b"post-rotation message");
    println!("[7] after rotation, v1-only holder is locked out; v2 holder reads v2 ✓");

    // [8] negative test: mallory presents bob's (public) cert but substitutes
    //     her own enc key, signed with her own TLS key. Must be rejected.
    let mallory = mint_identity("mallory")?;
    let mallory_enc = sec1(&mallory.enc_public);
    let forged_sig: Signature =
        mallory.tls_signing_key.sign(&binding_message("bob", &mallory_enc));
    let forged = Binding {
        cn: "bob".to_string(),
        enc_pub_sec1: mallory_enc.clone(),
        signature: forged_sig.to_bytes().to_vec(),
        tls_cert_pem: bob.tls_cert_pem.clone(), // bob's real, public cert
    };
    assert!(
        verify_binding(&forged, "bob").is_err(),
        "a forged binding (enc key not signed by bob's TLS key) must be rejected"
    );
    println!("[8] forged binding (mallory-as-bob) rejected ✓");

    println!("\n=== all 8 steps passed ===");
    let _ = &alice; // alice's keys are the admin side; kept for clarity.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_wrap_unwrap_and_payload() -> Result<()> {
        let bob = mint_identity("bob")?;
        let enc_pub = verify_binding(&make_binding(&bob), "bob")?;
        let key = random_session_key();
        let wrapped = wrap_session_key(&enc_pub, &key)?;
        assert_eq!(unwrap_session_key(&bob.enc_secret, &wrapped)?, key);
        let (n, ct) = seal_payload(&key, b"msg")?;
        assert_eq!(open_payload(&key, &n, &ct)?, b"msg");
        Ok(())
    }

    #[test]
    fn rotation_locks_out_old_key() -> Result<()> {
        let v1 = random_session_key();
        let v2 = random_session_key();
        let (n, ct) = seal_payload(&v2, b"after rotation")?;
        assert!(open_payload(&v1, &n, &ct).is_err());
        assert_eq!(open_payload(&v2, &n, &ct)?, b"after rotation");
        Ok(())
    }

    #[test]
    fn binding_cn_mismatch_rejected() -> Result<()> {
        let bob = mint_identity("bob")?;
        // verifying bob's valid binding against the wrong expected CN fails.
        assert!(verify_binding(&make_binding(&bob), "alice").is_err());
        Ok(())
    }

    #[test]
    fn forged_enc_key_rejected() -> Result<()> {
        let bob = mint_identity("bob")?;
        let mallory = mint_identity("mallory")?;
        let mallory_enc = sec1(&mallory.enc_public);
        let forged_sig: Signature =
            mallory.tls_signing_key.sign(&binding_message("bob", &mallory_enc));
        let forged = Binding {
            cn: "bob".to_string(),
            enc_pub_sec1: mallory_enc.clone(),
            signature: forged_sig.to_bytes().to_vec(),
            tls_cert_pem: bob.tls_cert_pem.clone(),
        };
        assert!(verify_binding(&forged, "bob").is_err());
        Ok(())
    }
}
