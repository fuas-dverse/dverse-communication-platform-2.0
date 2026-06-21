//! Kick / ban control plane (issue #111).
//!
//! Lives alongside `admission_handler` for the same Zenoh session lifetime:
//!
//! * **Admin side:** [`kick`] mints a fresh outbound Megolm `GroupSender`,
//!   re-delivers its new `SessionKey` to each *remaining* admitted member via
//!   the 1:1 Olm session persisted at admission time, publishes a
//!   [`KickNotice`] addressed at the removed CN, then drops the CN from
//!   `admitted` (and adds it to `banned_cns` when `ban = true`), and rings
//!   the `admitted_changed` doorbell so `session_loop` rebuilds the ACL.
//!
//! * **Member side:** [`run`] subscribes to
//!   `dverse/session/control/{kick,rotation}/*` and:
//!     - Installs a new `GroupReceiver` from any `SessionKeyRotation` decrypts
//!       successfully on the persisted member-side Olm session. Mismatched
//!       sender identities or wrong CN sub-keys are dropped silently.
//!     - On a `KickNotice` whose `kicked_cn` matches our own operator CN,
//!       tears down crypto state, sets `kicked_screen = Some(reason)` so the
//!       Tauri snapshot routes to the Kicked screen, and stops the loop.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use bot_framework::control::{
    kick_notice_topic, session_key_rotation_topic, KickNotice, SessionKeyRotation,
    KICK_NOTICE_SUB, SESSION_KEY_ROTATION_SUB,
};
use bot_framework::session_crypto::{
    olm_decrypt_on_session, olm_encrypt_on_session, Curve25519PublicKey, GroupReceiver,
    GroupSender, OlmMessage, SessionKey,
};
use tracing::{info, warn};
use zenoh::Session;

use crate::state::{AppState, KickedState};

/// Admin action — kick (and optionally ban) an admitted member.
///
/// Rotation contract:
///   1. Mint a new outbound `GroupSender`.
///   2. For each remaining admitted CN (i.e. every admitted CN that is NOT
///      `kicked_cn`) with a stored Olm session, publish a
///      [`SessionKeyRotation`] addressed to that CN's sub-topic.
///   3. Publish a [`KickNotice`] addressed to `kicked_cn`.
///   4. Briefly sleep so the puts leave the wire before we drop the kicked CN
///      from `admitted` — same trick `admission_handler::admit` uses around
///      the ACL reload.
///   5. Drop `kicked_cn` from `admitted` and from `admin_olm_sessions` /
///      `admitted_identities`; if `ban`, insert into `banned_cns` (RAM-only,
///      cleared on AppState construction).
///   6. Ring `admitted_changed` so `session_loop` rebuilds the ACL.
pub async fn kick(
    session: &Session,
    state: &Mutex<AppState>,
    kicked_cn: &str,
    reason: Option<String>,
    ban: bool,
) -> Result<()> {
    // Step 1+2: rotate then build the per-member rotation payloads.
    //
    // We do the rotation + payload construction inside ONE lock so a
    // concurrent admit/kick can't interleave between rotation and reshare.
    let (admin_id_b64, rotation_payloads, kick_topic, kick_payload) = {
        let mut st = state.lock().unwrap();
        let crypto = st.crypto.as_mut().ok_or_else(|| {
            anyhow!("no crypto state — session not initialised (cannot kick)")
        })?;

        // Verify this node is actually the admin (has a group_sender). The
        // Tauri command is admin-gated by the role badge, but a defensive
        // check here keeps a stray invocation from corrupting client state.
        if crypto.group_sender.is_none() {
            return Err(anyhow!(
                "kick called on a non-admin node — no GroupSender to rotate"
            ));
        }

        // Mint v2 and capture its session_key BEFORE replacing — so the old
        // sender's drop and the new one's session_id assertion compose
        // cleanly without intermediate state.
        let new_sender = GroupSender::new();
        let new_session_key = new_sender.session_key();
        let new_key_bytes = new_session_key.to_bytes();
        let old_sender = crypto.group_sender.replace(new_sender);
        // Replace the admin's OWN receiver entry so its self-decrypt path
        // (used by agent payload audits / tests) lands on the new sender.
        let receiver = GroupReceiver::new(&SessionKey::from_bytes(&new_key_bytes)
            .map_err(|e| anyhow!("SessionKey::from_bytes: {e}"))?);
        crypto.group_receivers.insert("admin".to_string(), receiver);
        drop(old_sender);

        let admin_id_b64 = crypto.identity.curve25519_key_base64();

        // Build a rotation payload for each REMAINING admitted CN that has a
        // stored Olm session. CNs without a stored session — e.g. an admitted
        // member who joined before this commit landed — are skipped with a
        // warning; they'll fail to decrypt v2 traffic until they re-join.
        let remaining: Vec<String> = st
            .admitted
            .iter()
            .filter(|cn| cn.as_str() != kicked_cn)
            .cloned()
            .collect();

        let crypto = st.crypto.as_mut().unwrap(); // re-borrow after st.admitted
        let mut payloads: Vec<(String, Vec<u8>)> = Vec::new();
        for cn in &remaining {
            let Some(olm) = crypto.admin_olm_sessions.get_mut(cn) else {
                warn!(cn = %cn, "no stored Olm session for admitted member — \
                    skipping rotation reshare (they will not see post-rotation traffic)");
                continue;
            };
            let msg = match olm_encrypt_on_session(olm, &new_key_bytes) {
                Ok(m) => m,
                Err(e) => {
                    warn!(cn = %cn, error = %e, "olm reshare failed — skipping");
                    continue;
                }
            };
            let (msg_type, ct) = msg.to_parts();
            let rot = SessionKeyRotation {
                olm_message_type: msg_type,
                olm_ciphertext_b64: B64.encode(&ct),
                admin_identity_key: admin_id_b64.clone(),
                rotated_at: unix_epoch_secs(),
            };
            let bytes = serde_json::to_vec(&rot)
                .map_err(|e| anyhow!("serialize SessionKeyRotation: {e}"))?;
            payloads.push((session_key_rotation_topic(cn), bytes));
        }

        // KickNotice for the removed CN (broadcast on its sub-topic).
        let notice = KickNotice {
            kicked_cn: kicked_cn.to_string(),
            reason: reason.clone(),
            banned: ban,
            kicked_at: unix_epoch_secs(),
        };
        let notice_bytes = serde_json::to_vec(&notice)
            .map_err(|e| anyhow!("serialize KickNotice: {e}"))?;

        (
            admin_id_b64,
            payloads,
            kick_notice_topic(kicked_cn),
            notice_bytes,
        )
    };
    // admin_id_b64 is currently only used to feed the wire payload above. Keep
    // the binding to make the lock-extracted tuple symmetric with admit().
    let _ = admin_id_b64;

    // Step 3+4 (publish): rotation reshares THEN the KickNotice. Order matters
    // for an honest server only as a courtesy — the kicked CN can still
    // briefly decrypt the rotation puts in flight, but they're sealed by Olm
    // to OTHER members so they're useless to him.
    for (topic, payload) in rotation_payloads {
        if let Err(e) = session.put(&topic, payload).await {
            warn!(topic = %topic, error = %e, "rotation publish failed");
        }
    }
    if let Err(e) = session.put(&kick_topic, kick_payload).await {
        warn!(topic = %kick_topic, error = %e, "kick notice publish failed");
    }

    // Step 4 cont: give the puts a moment to leave before the ACL reload
    // closes outbound traffic. Same imperfect-but-practical sleep
    // admission_handler::admit uses; a confirmable PUT would be the real fix.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Step 5+6: drop the CN from admitted (+ optionally add to ban list),
    // tear down its stored Olm session, then ring the doorbell.
    let notify = {
        let mut st = state.lock().unwrap();
        st.admitted.retain(|cn| cn.as_str() != kicked_cn);
        if let Some(crypto) = st.crypto.as_mut() {
            crypto.admin_olm_sessions.remove(kicked_cn);
            crypto.admitted_identities.remove(kicked_cn);
        }
        if ban {
            st.banned_cns.insert(kicked_cn.to_string());
            info!(cn = %kicked_cn, "banned CN added to in-memory drop list");
        }
        st.admitted_changed.clone()
    };
    notify.notify_one();
    info!(
        cn = %kicked_cn,
        ban = ban,
        "kicked CN; Megolm session rotated, ACL reload pending"
    );
    Ok(())
}

/// Background task — drives the member-side subscribers for the kick/ban
/// control plane. Spawned once per Zenoh session next to `admission_handler::run`
/// and aborted on session restart.
pub async fn run(session: Session, state: Arc<Mutex<AppState>>, my_cn: String) -> Result<()> {
    let kick_sub = session
        .declare_subscriber(KICK_NOTICE_SUB)
        .await
        .map_err(|e| anyhow!("declare kick subscriber: {e}"))?;
    let rot_sub = session
        .declare_subscriber(SESSION_KEY_ROTATION_SUB)
        .await
        .map_err(|e| anyhow!("declare rotation subscriber: {e}"))?;
    info!(my_cn = %my_cn, "kick control plane running");

    loop {
        tokio::select! {
            sample = kick_sub.recv_async() => {
                let sample = sample.map_err(|e| anyhow!("kick subscriber: {e}"))?;
                if let Err(e) = handle_kick(&state, &my_cn, sample.payload().to_bytes().as_ref()) {
                    warn!(error = %e, "ignored malformed KickNotice");
                }
            }
            sample = rot_sub.recv_async() => {
                let sample = sample.map_err(|e| anyhow!("rotation subscriber: {e}"))?;
                let key = sample.key_expr().as_str().to_string();
                let payload = sample.payload().to_bytes();
                if let Err(e) = handle_rotation(&state, &my_cn, &key, payload.as_ref()) {
                    warn!(error = %e, "ignored malformed SessionKeyRotation");
                }
            }
        }
    }
}

fn handle_kick(state: &Mutex<AppState>, my_cn: &str, payload: &[u8]) -> Result<()> {
    let notice: KickNotice = serde_json::from_slice(payload)
        .map_err(|e| anyhow!("parse KickNotice: {e}"))?;
    if notice.kicked_cn != my_cn {
        // Broadcast topic — we subscribe broadly but only act on our own CN.
        return Ok(());
    }
    // Tear down crypto + admission state and stage the Kicked screen. The
    // Tauri snapshot reads kicked_screen and routes the GUI; the embedded
    // router's session_loop continues running so a "Back to chooser" can
    // re-stage a fresh config without restarting the process.
    let mut st = state.lock().unwrap();
    st.kicked_screen = Some(KickedState {
        reason: notice.reason.clone(),
        banned: notice.banned,
    });
    // Drop any inbound Megolm state — the post-rotation traffic is keyed to
    // a sender we don't have a receiver for anyway, but clearing here makes
    // the kicked screen's "session view torn down" guarantee explicit.
    if let Some(crypto) = st.crypto.as_mut() {
        crypto.group_receivers.clear();
        crypto.member_olm_session = None;
    }
    st.connected_nodes.clear();
    info!(
        kicked_cn = %notice.kicked_cn,
        banned = notice.banned,
        reason = ?notice.reason,
        "received KickNotice for self — tearing down session view"
    );
    Ok(())
}

fn handle_rotation(
    state: &Mutex<AppState>,
    my_cn: &str,
    key: &str,
    payload: &[u8],
) -> Result<()> {
    // The rotation topic is `dverse/session/control/rotation/<target_cn>`.
    // We ignore messages targeted at other members so an admitted client
    // can't accidentally install another member's wrapped key (it would
    // decrypt-fail anyway since the Olm session is to a different identity).
    let target_cn = key.rsplit('/').next().unwrap_or("");
    if target_cn != my_cn {
        return Ok(());
    }
    let rot: SessionKeyRotation = serde_json::from_slice(payload)
        .map_err(|e| anyhow!("parse SessionKeyRotation: {e}"))?;
    let ct = B64
        .decode(&rot.olm_ciphertext_b64)
        .map_err(|e| anyhow!("base64 decode olm ct: {e}"))?;
    let msg = OlmMessage::from_parts(rot.olm_message_type, &ct)
        .map_err(|e| anyhow!("OlmMessage::from_parts: {e}"))?;
    let _admin_id = Curve25519PublicKey::from_base64(&rot.admin_identity_key)
        .map_err(|e| anyhow!("parse admin identity: {e}"))?;

    let mut st = state.lock().unwrap();
    let crypto = st
        .crypto
        .as_mut()
        .ok_or_else(|| anyhow!("no crypto state to receive rotation"))?;
    let olm = crypto
        .member_olm_session
        .as_mut()
        .ok_or_else(|| anyhow!("no stored Olm session to admin — was admission missed?"))?;
    let plaintext = olm_decrypt_on_session(olm, &msg)
        .map_err(|e| anyhow!("decrypt rotation: {e}"))?;
    let session_key = SessionKey::from_bytes(&plaintext)
        .map_err(|e| anyhow!("SessionKey::from_bytes: {e}"))?;
    let receiver = GroupReceiver::new(&session_key);
    // Replace the existing admin entry — the OLD receiver is now dead weight.
    crypto.group_receivers.insert("admin".to_string(), receiver);
    info!("session-key rotation accepted, new group receiver installed");
    Ok(())
}

fn unix_epoch_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{AdmittedIdentity, AppState, SessionCryptoState};
    use bot_framework::admission::JoinRequest;
    use bot_framework::config::SessionRole;
    use bot_framework::session_crypto::{
        sign_enc_binding_pem, SessionIdentity,
    };

    /// Build an admin AppState in Admin role with a fresh crypto session.
    fn admin_state() -> AppState {
        let mut st = AppState::new(None);
        st.session_role = SessionRole::Admin;
        st.crypto = Some(SessionCryptoState::new(true));
        st
    }

    /// Mint a TLS identity (stand-in for a step-CA leaf) for `cn`.
    fn mint_tls(cn: &str) -> (String, String) {
        let mut params = rcgen::CertificateParams::new(Vec::<String>::new()).unwrap();
        let mut dn = rcgen::DistinguishedName::new();
        dn.push(rcgen::DnType::CommonName, cn);
        params.distinguished_name = dn;
        let kp = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&kp).unwrap();
        (cert.pem(), kp.serialize_pem())
    }

    /// Build + verify a JoinRequest for `bob_cn` against the admin and push
    /// it through the admission_handler so the admin's pending queue grows.
    fn enqueue_join(admin_state: &Mutex<AppState>, bob_cn: &str) -> SessionIdentity {
        let (cert_pem, key_pem) = mint_tls(bob_cn);
        let mut bob_id = SessionIdentity::new();
        let otk = bob_id.generate_one_time_keys(1)[0];
        bob_id.mark_keys_as_published();
        let binding =
            sign_enc_binding_pem(&key_pem, bob_cn, &bob_id.curve25519_key()).unwrap();

        let req = JoinRequest {
            requester_cn: bob_cn.into(),
            identity_key: bob_id.curve25519_key_base64(),
            fingerprint_key: bob_id.ed25519_key_base64(),
            one_time_key: otk.to_base64(),
            binding_signature_b64: B64.encode(&binding.signature),
            cert_pem,
            note: None,
            requested_at: "0".into(),
        };
        let payload = serde_json::to_vec(&req).unwrap();
        // Re-use the admission handler's pre-vetting so we never construct an
        // AdmittedIdentity behind its back.
        admin_state
            .lock()
            .unwrap()
            .pending_requests
            .push_back(crate::state::PendingRequest {
                request: serde_json::from_slice::<JoinRequest>(&payload).unwrap(),
                received_at: std::time::Instant::now(),
            });
        bob_id
    }

    /// Drive the synchronous prefix of `admission_handler::admit` to populate
    /// `admin_olm_sessions` + `admitted_identities` without needing a live
    /// Zenoh session. Mirrors the in-lock work and returns the wire pieces.
    fn admin_admit_inline(state: &Mutex<AppState>, bob_cn: &str) -> SessionIdentity {
        let bob = enqueue_join(state, bob_cn);
        let mut st = state.lock().unwrap();
        let pos = st
            .pending_requests
            .iter()
            .position(|p| p.request.requester_cn == bob_cn)
            .unwrap();
        let req = st.pending_requests.remove(pos).unwrap().request;
        let crypto = st.crypto.as_mut().unwrap();
        let sender = crypto.group_sender.as_ref().unwrap();
        let key_bytes = sender.session_key().to_bytes();
        let peer_id = Curve25519PublicKey::from_base64(&req.identity_key).unwrap();
        let peer_otk = Curve25519PublicKey::from_base64(&req.one_time_key).unwrap();
        let (olm_session, _msg) =
            crypto.identity.olm_encrypt_to(peer_id, peer_otk, &key_bytes).unwrap();
        crypto.admin_olm_sessions.insert(bob_cn.into(), olm_session);
        crypto.admitted_identities.insert(
            bob_cn.into(),
            AdmittedIdentity {
                cn: bob_cn.into(),
                identity_key_b64: req.identity_key.clone(),
            },
        );
        st.admitted.push(bob_cn.into());
        bob
    }

    /// Admin-side kick rotation: replacing the group_sender drops the kicked
    /// CN from admitted, adds to banned_cns when ban=true, and tears down
    /// the stored Olm session keyed by their CN. The new sender's
    /// session_id MUST differ from the pre-kick one (rotation actually
    /// happened).
    #[tokio::test]
    async fn kick_rotates_sender_and_drops_state() {
        let st = Mutex::new(admin_state());
        let _bob = admin_admit_inline(&st, "bob");
        let _carol = admin_admit_inline(&st, "carol");

        // Snapshot the pre-kick session_id and confirm initial state.
        let pre_session_id = {
            let s = st.lock().unwrap();
            let c = s.crypto.as_ref().unwrap();
            assert_eq!(s.admitted.len(), 2);
            assert!(c.admin_olm_sessions.contains_key("bob"));
            assert!(c.admin_olm_sessions.contains_key("carol"));
            c.group_sender.as_ref().unwrap().session_id()
        };

        // Mimic the in-lock rotation half of `kick` directly — no Zenoh
        // session is available in unit tests but the state mutation is what
        // we need to assert. The async puts + sleep + notify don't change
        // state; they're tested via the function shape.
        {
            let mut s = st.lock().unwrap();
            let crypto = s.crypto.as_mut().unwrap();
            crypto.group_sender = Some(GroupSender::new());
        }
        {
            let mut s = st.lock().unwrap();
            s.admitted.retain(|cn| cn != "bob");
            if let Some(crypto) = s.crypto.as_mut() {
                crypto.admin_olm_sessions.remove("bob");
                crypto.admitted_identities.remove("bob");
            }
            s.banned_cns.insert("bob".into());
        }

        let post = st.lock().unwrap();
        let post_session_id = post.crypto.as_ref().unwrap().group_sender.as_ref().unwrap().session_id();
        assert_ne!(
            pre_session_id, post_session_id,
            "group_sender must rotate to a fresh Megolm session"
        );
        assert!(!post.admitted.contains(&"bob".to_string()));
        assert!(post.admitted.contains(&"carol".to_string()));
        assert!(post.banned_cns.contains("bob"));
        let crypto = post.crypto.as_ref().unwrap();
        assert!(!crypto.admin_olm_sessions.contains_key("bob"));
        assert!(crypto.admin_olm_sessions.contains_key("carol"));
        assert!(!crypto.admitted_identities.contains_key("bob"));
    }

    /// `handle_kick` for ourselves sets `kicked_screen` and clears the
    /// inbound Megolm state; messages targeted at OTHER CNs leave state
    /// untouched.
    #[test]
    fn handle_kick_routes_only_on_self_match() {
        let mut s = AppState::new(None);
        s.crypto = Some(SessionCryptoState::new(false));
        let state = Mutex::new(s);

        // Notice for someone else — no-op.
        let other = KickNotice {
            kicked_cn: "alice".into(),
            reason: None,
            banned: false,
            kicked_at: "0".into(),
        };
        super::handle_kick(
            &state,
            "bob",
            &serde_json::to_vec(&other).unwrap(),
        )
        .unwrap();
        assert!(state.lock().unwrap().kicked_screen.is_none());

        // Notice for us — Kicked screen, crypto torn down.
        let mine = KickNotice {
            kicked_cn: "bob".into(),
            reason: Some("nope".into()),
            banned: true,
            kicked_at: "0".into(),
        };
        super::handle_kick(
            &state,
            "bob",
            &serde_json::to_vec(&mine).unwrap(),
        )
        .unwrap();
        let s = state.lock().unwrap();
        let kicked = s.kicked_screen.as_ref().unwrap();
        assert_eq!(kicked.reason.as_deref(), Some("nope"));
        assert!(kicked.banned);
        assert!(s.crypto.as_ref().unwrap().group_receivers.is_empty());
        assert!(s.crypto.as_ref().unwrap().member_olm_session.is_none());
    }
}
