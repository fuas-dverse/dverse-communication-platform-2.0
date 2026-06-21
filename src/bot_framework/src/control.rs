//! Wire protocol for the session **control** plane (issue #111).
//!
//! Two topics, both under `dverse/session/control/...` so the existing
//! admission-flow ACL rule (`session-rule` covers `dverse/session/**`, see
//! `zenoh_router::router::build_acl_json`) carries them without an ACL change.
//!
//! * `dverse/session/control/kick/<kicked_cn>` — `KickNotice`,
//!   admin → kicked member. JSON, plaintext.
//!
//!   Authentication note (unmitigated): the session-rule allows ANY cert
//!   holder to publish on `dverse/session/**`, so an admitted member could
//!   forge a `KickNotice` targeted at a peer and the receiver would currently
//!   tear down on the first match — `kick_handler::handle_kick` does NOT
//!   cross-check against a rotation having actually happened, nor does it
//!   verify a sender signature. The issue's acceptance criteria don't require
//!   unforgeable kicks, but this is a known soft-spot to address in a follow
//!   up (signing the notice with the admin's Ed25519 fingerprint is the
//!   intended hardening; the wire type already carries `kicked_cn` and
//!   `reason` so a future signature field can be added additively).
//!
//! * `dverse/session/control/rotation/<member_cn>` — `SessionKeyRotation`,
//!   admin → one remaining member. Carries an Olm `Normal` message whose
//!   plaintext is the new Megolm `SessionKey` bytes, encrypted on the 1:1 Olm
//!   session that was established as a side-effect of admission. Because the
//!   payload is Olm-encrypted to a session only this member's account can
//!   decrypt, the rotation message is authenticated end-to-end even though
//!   the topic itself is on the open session-rule plane.

use serde::{Deserialize, Serialize};

/// Admin → kicked member. The member's GUI tears down its session view and
/// shows the "Kicked" screen with `reason`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KickNotice {
    /// The CN of the member being kicked. Recipients ignore notices whose
    /// `kicked_cn` doesn't match their own operator CN — the topic is broadcast
    /// and ALL session members subscribe (matching the admission-handler's
    /// "subscribe broad, filter in code" pattern).
    pub kicked_cn: String,
    /// Optional admin-supplied reason shown on the Kicked screen.
    pub reason: Option<String>,
    /// Whether the admin also banned the CN. The bit is purely informational
    /// for the kicked client (it shapes the screen copy); the actual ban
    /// state is RAM-only on the admin side (`AppState.banned_cns`).
    pub banned: bool,
    /// Unix-epoch seconds, set by the admin. Recipients use it only for
    /// display; replay protection is enforced by the corresponding rotation
    /// message — a stale KickNotice without a fresh rotation would leave the
    /// member able to decrypt traffic anyway, so its kick screen would be
    /// trivially detectable as a forgery.
    pub kicked_at: String,
}

/// Admin → one remaining member. Carries a freshly-minted Megolm `SessionKey`
/// Olm-encrypted on the persistent 1:1 channel established at admission time.
/// The new `SessionKey` is the post-rotation outbound Megolm session's key —
/// only members reshared to can decrypt post-rotation Megolm traffic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKeyRotation {
    /// Olm `message_type` (1 = Normal, since the pre-key handshake already
    /// happened during admission). Carried explicitly so a future migration
    /// to a different message-type doesn't silently misinterpret the
    /// ciphertext.
    pub olm_message_type: usize,
    /// Olm ciphertext, base64. Plaintext is the wire bytes of a Megolm
    /// `SessionKey` (vodozemac `SessionKey::to_bytes`).
    pub olm_ciphertext_b64: String,
    /// The admin's Curve25519 identity key (base64). Receivers look up the
    /// matching stored Olm session under this identity; mismatches are
    /// dropped silently.
    pub admin_identity_key: String,
    /// Unix-epoch seconds, set by the admin. Display only.
    pub rotated_at: String,
}

/// Convenience constants so call sites (router subscribe, Tauri publish) don't
/// spell the topic prefix out of band.
pub const KICK_NOTICE_PREFIX: &str = "dverse/session/control/kick";
pub const SESSION_KEY_ROTATION_PREFIX: &str = "dverse/session/control/rotation";

/// Subscriber wildcard that catches every kick notice on the session bus.
pub const KICK_NOTICE_SUB: &str = "dverse/session/control/kick/*";
/// Subscriber wildcard that catches every rotation message addressed at any
/// admitted member (each member filters on their own CN at receive time).
pub const SESSION_KEY_ROTATION_SUB: &str = "dverse/session/control/rotation/*";

/// Build the publish topic for a kick notice addressed at `kicked_cn`.
pub fn kick_notice_topic(kicked_cn: &str) -> String {
    format!("{KICK_NOTICE_PREFIX}/{kicked_cn}")
}

/// Build the publish topic for a rotation message addressed at `member_cn`.
pub fn session_key_rotation_topic(member_cn: &str) -> String {
    format!("{SESSION_KEY_ROTATION_PREFIX}/{member_cn}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wire-format stability: KickNotice serialises and deserialises through
    /// JSON with the exact field set the launcher and router expect.
    #[test]
    fn kick_notice_json_round_trip() {
        let n = KickNotice {
            kicked_cn: "bob".into(),
            reason: Some("spammed the channel".into()),
            banned: true,
            kicked_at: "1750000000".into(),
        };
        let json = serde_json::to_string(&n).unwrap();
        let parsed: KickNotice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kicked_cn, "bob");
        assert_eq!(parsed.reason.as_deref(), Some("spammed the channel"));
        assert!(parsed.banned);
    }

    /// SessionKeyRotation wire format: all four required fields survive a
    /// round trip and the explicit `olm_message_type` is preserved exactly.
    #[test]
    fn session_key_rotation_json_round_trip() {
        let r = SessionKeyRotation {
            olm_message_type: 1,
            olm_ciphertext_b64: "ZHVtbXk=".into(),
            admin_identity_key: "QUFB".into(),
            rotated_at: "1750000001".into(),
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: SessionKeyRotation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.olm_message_type, 1);
        assert_eq!(parsed.admin_identity_key, "QUFB");
    }

    #[test]
    fn topic_builders_match_constants() {
        assert_eq!(kick_notice_topic("alice"), "dverse/session/control/kick/alice");
        assert_eq!(
            session_key_rotation_topic("alice"),
            "dverse/session/control/rotation/alice"
        );
    }
}
