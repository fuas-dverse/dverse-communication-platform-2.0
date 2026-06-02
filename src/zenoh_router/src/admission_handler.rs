//! Admission control plane (issue #110).
//!
//! # Scope of #110 (what this file covers and what it doesn't)
//!
//! In scope (done):
//!   * The full `dverse/session/{requests,admission}/**` wire protocol.
//!   * EncKeyBinding verification on the admin side (cert CN match + ECDSA
//!     signature under the cert's pubkey).
//!   * Olm pre-key wrap of the admin's Megolm `SessionKey` against the
//!     requester's published OTK; requester unwraps and installs a
//!     `GroupReceiver` in `AppState.crypto.group_receivers`.
//!   * Admin allow/deny actions, in-process from a Tauri command.
//!
//! Carve-out (deferred):
//!   * Ping/pong (and other agents) do **not** yet AEAD-wrap their
//!     payloads with the installed `GroupReceiver`. Doing this correctly
//!     requires per-process Megolm `GroupSender`s and a key-exchange
//!     between operator and agent — sharing one ratchet across processes
//!     corrupts the chain index. Tracked as a follow-up; the issue's
//!     "B reaches Main and decrypts ping↔pong" criterion is **NOT** met
//!     by this PR alone.
//!
//! # Runtime model
//!
//! Runs alongside `session_loop` for the duration of one Zenoh session. The
//! handler is role-agnostic — both admin and clients subscribe to the same
//! topics, and ignore messages that aren't addressed to them:
//!
//! * **Admin** listens on `dverse/session/requests/**`, verifies each
//!   `JoinRequest`'s identity binding, drops anything from `banned_cns`, and
//!   queues the rest into `AppState.pending_requests` for the GUI.
//! * **Requester** listens on `dverse/session/admission/<my_cn>`, Olm-decrypts
//!   any incoming `Allow` to recover the Megolm `SessionKey`, instantiates a
//!   `GroupReceiver`, and flips `AppState.join_flow` to drive the GUI.
//!
//! The actual admit/deny *action* (called from a Tauri command) lives in
//! [`admit`] / [`deny`]; both publish an `AdmissionDecision` on the requester's
//! admission topic and (for `admit`) ring the `admitted_changed` doorbell to
//! restart the session under an ACL that includes the new CN.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use std::path::Path;

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use bot_framework::admission::{AdmissionDecision, JoinRequest};
use bot_framework::session_crypto::{
    sign_enc_binding_pem, verify_enc_binding, Curve25519PublicKey, EncKeyBinding, GroupReceiver,
    OlmMessage, SessionKey,
};
use tracing::{info, warn};
use zenoh::Session;

use crate::state::{AppState, JoinFlowStatus, PendingRequest};

/// Requester action — build and publish a `JoinRequest` on
/// `dverse/session/requests/<my_cn>`. Loads the requester's cert PEM and
/// signs the vodozemac-identity-to-CN binding with the matching TLS key.
/// Sets `AppState.join_flow = Pending`.
pub async fn request_join(
    session: &Session,
    state: &Mutex<AppState>,
    my_cn: &str,
    admin_cn: &str,
    cert_path: &Path,
    key_path: &Path,
    note: Option<String>,
) -> Result<()> {
    let cert_pem = tokio::fs::read_to_string(cert_path).await
        .map_err(|e| anyhow!("read cert PEM: {e}"))?;
    let key_pem = tokio::fs::read_to_string(key_path).await
        .map_err(|e| anyhow!("read key PEM: {e}"))?;

    let (identity_b64, fingerprint_b64, otk_b64, binding_sig_b64) = {
        let mut st = state.lock().unwrap();
        let crypto = st.crypto.as_mut()
            .ok_or_else(|| anyhow!("no crypto state — session not initialised"))?;
        let identity_b64 = crypto.identity.curve25519_key_base64();
        let fingerprint_b64 = crypto.identity.ed25519_key_base64();
        let otk = crypto.published_otk
            .ok_or_else(|| anyhow!("no published OTK — cannot accept admission"))?;
        let identity_pub = crypto.identity.curve25519_key();
        let binding = sign_enc_binding_pem(&key_pem, my_cn, &identity_pub)
            .map_err(|e| anyhow!("sign binding: {e}"))?;
        (
            identity_b64,
            fingerprint_b64,
            otk.to_base64(),
            B64.encode(&binding.signature),
        )
    };

    let req = JoinRequest {
        requester_cn: my_cn.to_string(),
        identity_key: identity_b64,
        fingerprint_key: fingerprint_b64,
        one_time_key: otk_b64,
        binding_signature_b64: binding_sig_b64,
        cert_pem,
        note,
        requested_at: unix_epoch_secs(),
    };
    let key = format!("dverse/session/requests/{}", my_cn);
    let payload = serde_json::to_vec(&req)
        .map_err(|e| anyhow!("serialize JoinRequest: {e}"))?;
    session.put(&key, payload).await
        .map_err(|e| anyhow!("publish JoinRequest: {e}"))?;

    state.lock().unwrap().join_flow = Some(JoinFlowStatus::Pending {
        admin_cn: admin_cn.to_string(),
        sent_at: Instant::now(),
    });
    info!(admin_cn = %admin_cn, "JoinRequest published");
    Ok(())
}

/// Spawned per Zenoh session. Drives both the request listener (admin role)
/// and the admission listener (client role) until the session closes, the
/// task is aborted, or one of the subscribers errors out.
pub async fn run(
    session: Session,
    state: Arc<Mutex<AppState>>,
    my_cn: String,
) -> Result<()> {
    let requests_sub = session
        .declare_subscriber("dverse/session/requests/**")
        .await
        .map_err(|e| anyhow!("declare requests subscriber: {e}"))?;

    let admission_key = format!("dverse/session/admission/{}", my_cn);
    let admission_sub = session
        .declare_subscriber(&admission_key)
        .await
        .map_err(|e| anyhow!("declare admission subscriber: {e}"))?;

    info!(my_cn = %my_cn, "admission control plane running");

    loop {
        tokio::select! {
            sample = requests_sub.recv_async() => {
                let sample = match sample {
                    Ok(s) => s,
                    Err(e) => return Err(anyhow!("requests subscriber: {e}")),
                };
                if let Err(e) = handle_request(&state, sample.payload().to_bytes().as_ref()) {
                    warn!(error = %e, "ignored malformed/invalid join request");
                }
            }
            sample = admission_sub.recv_async() => {
                let sample = match sample {
                    Ok(s) => s,
                    Err(e) => return Err(anyhow!("admission subscriber: {e}")),
                };
                if let Err(e) = handle_decision(&state, sample.payload().to_bytes().as_ref()) {
                    warn!(error = %e, "ignored malformed/undecryptable admission decision");
                }
            }
        }
    }
}

/// Admin-side: a `JoinRequest` arrived on the wire. Verify the binding,
/// drop bans, enqueue for the GUI.
fn handle_request(state: &Mutex<AppState>, payload: &[u8]) -> Result<()> {
    let req: JoinRequest = serde_json::from_slice(payload)
        .map_err(|e| anyhow!("parse JoinRequest: {e}"))?;

    // Only the admin acts on requests. Clients silently ignore them — they're
    // subscribed because the topic is shared, but they have no pending queue.
    {
        let st = state.lock().unwrap();
        if !matches!(st.session_role, bot_framework::config::SessionRole::Admin) {
            return Ok(());
        }
        if st.banned_cns.contains(&req.requester_cn) {
            info!(cn = %req.requester_cn, "dropped JoinRequest from banned CN");
            return Ok(());
        }
        // De-dup: a requester republishing while still pending shouldn't
        // pile up rows in the admin's panel.
        if st.pending_requests.iter().any(|p| p.request.requester_cn == req.requester_cn) {
            return Ok(());
        }
    }

    // Verify the binding signature + CN match against the cert in the
    // request. mTLS already gated fabric entry — this proves the
    // vodozemac identity belongs to the cert holder.
    let sig_bytes = B64.decode(&req.binding_signature_b64)
        .map_err(|e| anyhow!("base64 decode signature: {e}"))?;
    let identity_key = Curve25519PublicKey::from_base64(&req.identity_key)
        .map_err(|e| anyhow!("parse identity_key: {e}"))?;
    let binding = EncKeyBinding {
        cn: req.requester_cn.clone(),
        curve25519_key: identity_key.to_bytes(),
        signature: sig_bytes,
    };
    let _verified = verify_enc_binding(&req.cert_pem, &req.requester_cn, &binding)
        .map_err(|e| anyhow!("verify binding: {e}"))?;

    let pending = PendingRequest { request: req, received_at: Instant::now() };
    let cn = pending.request.requester_cn.clone();
    state.lock().unwrap().pending_requests.push_back(pending);
    info!(cn = %cn, "queued verified JoinRequest");
    Ok(())
}

/// Requester-side: an `AdmissionDecision` arrived on the wire. On `Allow`,
/// Olm-decrypt the wrapped Megolm key and install a `GroupReceiver`.
fn handle_decision(state: &Mutex<AppState>, payload: &[u8]) -> Result<()> {
    let dec: AdmissionDecision = serde_json::from_slice(payload)
        .map_err(|e| anyhow!("parse AdmissionDecision: {e}"))?;

    match dec {
        AdmissionDecision::Allow { olm_message_type, olm_ciphertext_b64, admin_identity_key, .. } => {
            let ct = B64.decode(&olm_ciphertext_b64)
                .map_err(|e| anyhow!("base64 decode olm ct: {e}"))?;
            let msg = OlmMessage::from_parts(olm_message_type, &ct)
                .map_err(|e| anyhow!("OlmMessage::from_parts: {e}"))?;
            let admin_id = Curve25519PublicKey::from_base64(&admin_identity_key)
                .map_err(|e| anyhow!("parse admin identity: {e}"))?;

            let mut st = state.lock().unwrap();
            // Only act if we actually have a pending join flow — otherwise
            // someone's replaying an old Allow and we shouldn't tamper with
            // our state.
            if !matches!(st.join_flow, Some(JoinFlowStatus::Pending { .. })) {
                return Ok(());
            }
            let crypto = st.crypto.as_mut()
                .ok_or_else(|| anyhow!("no crypto state to receive admission"))?;
            let (_session, plaintext) = crypto.identity.olm_decrypt_from(admin_id, &msg)
                .map_err(|e| anyhow!("olm decrypt: {e}"))?;
            let session_key = SessionKey::from_bytes(&plaintext)
                .map_err(|e| anyhow!("SessionKey::from_bytes: {e}"))?;
            let receiver = GroupReceiver::new(&session_key);
            // Megolm sessions are keyed by their own session_id; we'd need to
            // expose that, but the admin only runs one group at a time for now
            // so we use the admin's CN as a stable key.
            crypto.group_receivers.insert("admin".to_string(), receiver);
            st.join_flow = Some(JoinFlowStatus::Allowed);
            info!("admission Allow accepted, group receiver installed");
        }
        AdmissionDecision::Deny { reason, .. } => {
            let mut st = state.lock().unwrap();
            if matches!(st.join_flow, Some(JoinFlowStatus::Pending { .. })) {
                st.join_flow = Some(JoinFlowStatus::Denied { reason });
                info!("admission Deny received");
            }
        }
    }
    Ok(())
}

/// Admin action — Allow. Olm-wraps the current group sender's `SessionKey`
/// against the requester's identity + one-time key, publishes
/// `AdmissionDecision::Allow`, then pushes the requester's CN to `admitted`
/// and rings the doorbell so `session_loop` restarts the session under the
/// new ACL.
pub async fn admit(
    session: &Session,
    state: &Mutex<AppState>,
    requester_cn: &str,
) -> Result<()> {
    let (olm_type, olm_ct_b64, admin_id_b64, decision_key) = {
        let mut st = state.lock().unwrap();
        let pending = st
            .pending_requests
            .iter()
            .position(|p| p.request.requester_cn == requester_cn)
            .ok_or_else(|| anyhow!("no pending request for {requester_cn}"))?;
        let req = st.pending_requests.remove(pending).unwrap().request;

        let crypto = st.crypto.as_mut()
            .ok_or_else(|| anyhow!("no crypto state — session not initialised"))?;
        let group_sender = crypto.group_sender.as_ref()
            .ok_or_else(|| anyhow!("admin has no group sender (not in Admin role)"))?;
        let session_key_bytes = group_sender.session_key().to_bytes();
        let admin_id_b64 = crypto.identity.curve25519_key_base64();

        let peer_id = Curve25519PublicKey::from_base64(&req.identity_key)
            .map_err(|e| anyhow!("parse peer identity: {e}"))?;
        let peer_otk = Curve25519PublicKey::from_base64(&req.one_time_key)
            .map_err(|e| anyhow!("parse peer OTK: {e}"))?;
        let (_session, msg) = crypto.identity
            .olm_encrypt_to(peer_id, peer_otk, &session_key_bytes)
            .map_err(|e| anyhow!("olm wrap session key: {e}"))?;
        let (msg_type, ct) = msg.to_parts();

        let decision_key = format!("dverse/session/admission/{}", req.requester_cn);
        (msg_type, B64.encode(&ct), admin_id_b64, decision_key)
    };

    let decision = AdmissionDecision::Allow {
        olm_message_type: olm_type,
        olm_ciphertext_b64: olm_ct_b64,
        admin_identity_key: admin_id_b64,
        admitted_at: unix_epoch_secs(),
    };
    let payload = serde_json::to_vec(&decision)
        .map_err(|e| anyhow!("serialize decision: {e}"))?;
    session.put(&decision_key, payload).await
        .map_err(|e| anyhow!("publish Allow: {e}"))?;

    // Give the put a moment to leave the wire before the session restart
    // closes outbound traffic. Imperfect but practical — a confirmable PUT
    // would be the real fix.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Push to admitted + ring the doorbell so session_loop reloads ACL.
    let notify = {
        let mut st = state.lock().unwrap();
        if !st.admitted.contains(&requester_cn.to_string()) {
            st.admitted.push(requester_cn.to_string());
        }
        st.admitted_changed.clone()
    };
    notify.notify_one();
    info!(cn = %requester_cn, "admitted CN; session will restart under new ACL");
    Ok(())
}

/// Unix epoch seconds as a string — cheap timestamp without pulling chrono.
fn unix_epoch_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bot_framework::session_crypto::{GroupSender, SessionIdentity};

    /// End-to-end Olm-wrap → unwrap → Megolm session round-trip.
    /// Proves the wire format (message_type + base64 ciphertext + base64
    /// admin identity) survives serialization and the requester ends up
    /// holding a working `GroupReceiver` for the admin's payloads.
    #[test]
    fn admit_olm_wraps_and_unwraps_megolm_key() {
        // ── Admin side: mint identity + group sender. ─────────────────
        let admin = SessionIdentity::new();
        let mut group_sender = GroupSender::new();
        let session_key_bytes = group_sender.session_key().to_bytes();

        // ── Requester side: mint identity + one OTK. ──────────────────
        let mut requester = SessionIdentity::new();
        let otks = requester.generate_one_time_keys(1);
        let requester_otk = otks[0];
        requester.mark_keys_as_published();
        let requester_id = requester.curve25519_key();

        // ── Admin wraps the Megolm key to the requester. ──────────────
        let (_olm_sess, msg) = admin
            .olm_encrypt_to(requester_id, requester_otk, &session_key_bytes)
            .expect("olm_encrypt_to");
        let (msg_type, ct) = msg.to_parts();
        let ct_b64 = B64.encode(&ct);
        let admin_id_b64 = admin.curve25519_key_base64();

        // ── Wire round-trip. ──────────────────────────────────────────
        let decoded_ct = B64.decode(&ct_b64).unwrap();
        let decoded_msg = OlmMessage::from_parts(msg_type, &decoded_ct).unwrap();
        let admin_id = Curve25519PublicKey::from_base64(&admin_id_b64).unwrap();

        // ── Requester unwraps. ────────────────────────────────────────
        let (_in_sess, plaintext) = requester
            .olm_decrypt_from(admin_id, &decoded_msg)
            .expect("olm_decrypt_from");
        let recovered_key = SessionKey::from_bytes(&plaintext).expect("SessionKey::from_bytes");
        let mut receiver = GroupReceiver::new(&recovered_key);

        // ── Admin sends a Megolm payload; requester decrypts. ─────────
        let payload = b"hello, session";
        let mmsg = group_sender.encrypt(payload);
        let mmsg_b64 = mmsg.to_base64();
        let mmsg_decoded = bot_framework::session_crypto::MegolmMessage::from_base64(&mmsg_b64)
            .expect("MegolmMessage::from_base64");
        let decrypted = receiver.decrypt(&mmsg_decoded).expect("megolm decrypt");
        assert_eq!(decrypted, payload);
    }

    /// End-to-end `handle_request` path: serialize a JoinRequest with a
    /// valid binding, feed the raw bytes through `handle_request`, assert
    /// the admin's `pending_requests` queue grew by one. This catches
    /// wire-format / serde / binding-verify regressions before the GUI
    /// surfaces them as silent drops.
    #[test]
    fn handle_request_enqueues_valid_join_request() {
        use crate::state::AppState;
        use bot_framework::admission::JoinRequest;
        use bot_framework::config::SessionRole;
        use bot_framework::session_crypto::sign_enc_binding_pem;
        use std::sync::Mutex;

        // Build an admin-role AppState with crypto initialised.
        let mut admin_state = AppState::new(None);
        admin_state.session_role = SessionRole::Admin;
        admin_state.crypto = Some(crate::state::SessionCryptoState::new(true));
        let admin_state = Mutex::new(admin_state);

        // Mint the requester's TLS identity (stand-in for a step-CA leaf).
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "bob");
        params.distinguished_name = dn;
        let kp = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&kp).unwrap();
        let cert_pem = cert.pem();
        let key_pem = kp.serialize_pem();

        // Requester's vodozemac identity + binding.
        let mut bob_identity = SessionIdentity::new();
        let otk = bob_identity.generate_one_time_keys(1)[0];
        bob_identity.mark_keys_as_published();
        let identity_pub = bob_identity.curve25519_key();
        let binding = sign_enc_binding_pem(&key_pem, "bob", &identity_pub).unwrap();

        let req = JoinRequest {
            requester_cn: "bob".into(),
            identity_key: bob_identity.curve25519_key_base64(),
            fingerprint_key: bob_identity.ed25519_key_base64(),
            one_time_key: otk.to_base64(),
            binding_signature_b64: B64.encode(&binding.signature),
            cert_pem,
            note: Some("please let me in".into()),
            requested_at: "0".into(),
        };
        let payload = serde_json::to_vec(&req).unwrap();

        // Feed it through the handler and assert.
        assert!(admin_state.lock().unwrap().pending_requests.is_empty());
        super::handle_request(&admin_state, &payload).expect("valid request must enqueue");
        let pending = &admin_state.lock().unwrap().pending_requests;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].request.requester_cn, "bob");
    }

    /// Negative twin: a JoinRequest whose binding signature is tampered MUST
    /// be rejected — `pending_requests` stays empty.
    #[test]
    fn handle_request_rejects_tampered_binding() {
        use crate::state::AppState;
        use bot_framework::admission::JoinRequest;
        use bot_framework::config::SessionRole;
        use bot_framework::session_crypto::sign_enc_binding_pem;
        use std::sync::Mutex;

        let mut admin_state = AppState::new(None);
        admin_state.session_role = SessionRole::Admin;
        admin_state.crypto = Some(crate::state::SessionCryptoState::new(true));
        let admin_state = Mutex::new(admin_state);

        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, "bob");
        params.distinguished_name = dn;
        let kp = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&kp).unwrap();
        let cert_pem = cert.pem();
        let key_pem = kp.serialize_pem();

        let mut bob_identity = SessionIdentity::new();
        let otk = bob_identity.generate_one_time_keys(1)[0];
        bob_identity.mark_keys_as_published();
        let binding = sign_enc_binding_pem(&key_pem, "bob", &bob_identity.curve25519_key()).unwrap();
        let mut sig = binding.signature.clone();
        // Flip a bit in the signature.
        sig[0] ^= 0x01;

        let req = JoinRequest {
            requester_cn: "bob".into(),
            identity_key: bob_identity.curve25519_key_base64(),
            fingerprint_key: bob_identity.ed25519_key_base64(),
            one_time_key: otk.to_base64(),
            binding_signature_b64: B64.encode(&sig),
            cert_pem,
            note: None,
            requested_at: "0".into(),
        };
        let payload = serde_json::to_vec(&req).unwrap();
        let res = super::handle_request(&admin_state, &payload);
        assert!(res.is_err(), "tampered binding must be rejected");
        assert!(admin_state.lock().unwrap().pending_requests.is_empty());
    }

    /// A fresh requester (no Olm session with the admin) MUST fail to
    /// decrypt an Allow sealed to someone else.
    #[test]
    fn admit_cannot_be_unwrapped_by_other_requester() {
        let admin = SessionIdentity::new();
        let group_sender = GroupSender::new();
        let key_bytes = group_sender.session_key().to_bytes();

        let mut bob = SessionIdentity::new();
        let bob_otk = bob.generate_one_time_keys(1)[0];
        bob.mark_keys_as_published();
        let (_s, msg) = admin
            .olm_encrypt_to(bob.curve25519_key(), bob_otk, &key_bytes)
            .unwrap();

        // Mallory (different identity) tries to decrypt Bob's Allow.
        let mut mallory = SessionIdentity::new();
        let _ = mallory.generate_one_time_keys(1);
        mallory.mark_keys_as_published();
        let err = mallory.olm_decrypt_from(admin.curve25519_key(), &msg);
        assert!(err.is_err(), "mallory must NOT be able to unwrap Bob's Allow");
    }
}

/// Admin action — Deny. Publishes `AdmissionDecision::Deny` and drops the
/// pending request. Does NOT touch `admitted`.
pub async fn deny(
    session: &Session,
    state: &Mutex<AppState>,
    requester_cn: &str,
    reason: Option<String>,
) -> Result<()> {
    {
        let mut st = state.lock().unwrap();
        let pos = st
            .pending_requests
            .iter()
            .position(|p| p.request.requester_cn == requester_cn);
        if let Some(i) = pos {
            st.pending_requests.remove(i);
        }
    }
    let decision_key = format!("dverse/session/admission/{}", requester_cn);
    let decision = AdmissionDecision::Deny { reason, decided_at: unix_epoch_secs() };
    let payload = serde_json::to_vec(&decision)
        .map_err(|e| anyhow!("serialize decision: {e}"))?;
    session.put(&decision_key, payload).await
        .map_err(|e| anyhow!("publish Deny: {e}"))?;
    info!(cn = %requester_cn, "denied CN");
    Ok(())
}
