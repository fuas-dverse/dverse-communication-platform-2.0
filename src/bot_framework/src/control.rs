//! Wire protocol for the session **control** plane (issue #111).
//!
//! Two topics, both under `dverse/session/control/...` so the existing
//! admission-flow ACL rule (`session-rule` covers `dverse/session/**`, see
//! `zenoh_router::router::build_acl_json`) carries them without an ACL change.
//!
//! * `dverse/session/control/kick/<kicked_cn>` — `KickNotice`,
//!   admin → kicked member. JSON, plaintext, Ed25519-signed.
//!
//!   Authentication (#145): the notice carries an Ed25519 signature over a
//!   domain-separated, length-prefixed byte form of `(kicked_cn, kicked_at,
//!   banned)` (see [`kick_signing_bytes`]), signed with the admin's vodozemac
//!   `Account` Ed25519 key. Receivers verify against the admin's Ed25519
//!   identity learned during admission (`AdmissionDecision::Allow.admin_ed25519_key`,
//!   pinned into `SessionCryptoState.session_admin_ed25519_b64`). Forged
//!   notices (wrong signer, mutated identifying fields, missing signature)
//!   are silently dropped. The session-rule still allows any admitted cert
//!   holder to publish on this topic, but only the admin can produce a
//!   notice that verifies.
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
    /// Ed25519 signature, base64, over [`kick_signing_bytes`] of
    /// `(kicked_cn, kicked_at, banned)`. Produced with the admin's vodozemac
    /// `Account::sign`. Receivers verify against the admin's Ed25519 key
    /// they learned at admission time (the stored trust anchor); the notice
    /// is silently dropped on signature mismatch or absence (#145).
    pub admin_ed25519_sig_b64: String,
    /// The admin's Ed25519 public key, base64. Informational and useful in
    /// tracing; verification is performed against the STORED admin Ed25519
    /// from admission, not this field. A mismatch between the stored anchor
    /// and the notice-carried key is itself a signal to drop.
    pub admin_ed25519_key_b64: String,
}

/// Canonical, domain-separated byte form signed by the admin and re-built by
/// the receiver. Both admin and receiver call this exact function so the byte
/// layout is impossible to drift between sides.
///
/// Layout (little is structural, all delimiters explicit):
/// ```text
///   b"dverse.kick.v1\0"
///   ‖ u32_be(len(kicked_cn)) ‖ kicked_cn
///   ‖ u32_be(len(kicked_at)) ‖ kicked_at
///   ‖ u8(banned as 0|1)
/// ```
///
/// `reason` is intentionally NOT signed: it is display copy only, and the
/// design choice lets admins fix a typo in the kick reason without
/// re-signing. The unit test
/// `kick_with_mutated_reason_still_verifies` pins that choice.
pub fn kick_signing_bytes(kicked_cn: &str, kicked_at: &str, banned: bool) -> Vec<u8> {
    let mut buf = Vec::with_capacity(
        15 + 4 + kicked_cn.len() + 4 + kicked_at.len() + 1,
    );
    buf.extend_from_slice(b"dverse.kick.v1\0");
    buf.extend_from_slice(&(kicked_cn.len() as u32).to_be_bytes());
    buf.extend_from_slice(kicked_cn.as_bytes());
    buf.extend_from_slice(&(kicked_at.len() as u32).to_be_bytes());
    buf.extend_from_slice(kicked_at.as_bytes());
    buf.push(if banned { 1 } else { 0 });
    buf
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
            admin_ed25519_sig_b64: "sig".into(),
            admin_ed25519_key_b64: "key".into(),
        };
        let json = serde_json::to_string(&n).unwrap();
        let parsed: KickNotice = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.kicked_cn, "bob");
        assert_eq!(parsed.reason.as_deref(), Some("spammed the channel"));
        assert!(parsed.banned);
        assert_eq!(parsed.admin_ed25519_sig_b64, "sig");
        assert_eq!(parsed.admin_ed25519_key_b64, "key");
    }

    /// Canonical signing bytes: stable across runs (no system inputs, no map
    /// ordering), and the layout is exactly what the doc comment claims.
    /// Pinned with an expected byte form so a future "tidy this up" refactor
    /// can't silently break wire compatibility with already-deployed admins.
    #[test]
    fn kick_signing_bytes_layout_is_pinned() {
        let bytes = kick_signing_bytes("bob", "1750000000", true);
        let mut expected: Vec<u8> = Vec::new();
        expected.extend_from_slice(b"dverse.kick.v1\0");
        expected.extend_from_slice(&3u32.to_be_bytes());
        expected.extend_from_slice(b"bob");
        expected.extend_from_slice(&10u32.to_be_bytes());
        expected.extend_from_slice(b"1750000000");
        expected.push(1u8);
        assert_eq!(bytes, expected);

        // banned flag flips ONE byte at the tail.
        let unbanned = kick_signing_bytes("bob", "1750000000", false);
        assert_eq!(unbanned.last(), Some(&0u8));
        assert_eq!(bytes.last(), Some(&1u8));
        assert_eq!(&bytes[..bytes.len() - 1], &unbanned[..unbanned.len() - 1]);
    }

    /// Length-prefix isolation: two distinct field decompositions that would
    /// concatenate to the same naive `cn || kicked_at` string must NOT collide
    /// in the signing bytes. Catches the classic "ambiguous concatenation"
    /// attack where an attacker pushes a `/` or digit boundary across fields.
    #[test]
    fn kick_signing_bytes_disambiguates_field_boundaries() {
        // Naive: "bob1750" + "000000" == "bob17" + "50000000".
        let a = kick_signing_bytes("bob1750", "000000", false);
        let b = kick_signing_bytes("bob17", "50000000", false);
        assert_ne!(a, b);
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
