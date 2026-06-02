//! Wire protocol for the session-admission flow (issue #110).
//!
//! Two topics, both under `dverse/session/...` so the ACL can carve them out
//! from the admitted-only `dverse/agents/...` plane:
//!
//! * `dverse/session/requests/<requester_cn>` — `JoinRequest`, requester → admin.
//!   Carries the requester's vodozemac Curve25519 identity (bound to their step-CA
//!   cert via `session_crypto::EncKeyBinding`), one pre-published one-time key,
//!   and their cert PEM so the admin can verify the binding.
//!
//! * `dverse/session/admission/<requester_cn>` — `AdmissionDecision`,
//!   admin → requester. On `Allow`, carries an Olm pre-key message whose
//!   plaintext is the base64 Megolm `SessionKey` of the admin's current group
//!   sender; only the requester (whose curve25519 priv key the OTK was minted
//!   under) can decrypt it.
//!
//! Both payloads serialize to JSON.

use serde::{Deserialize, Serialize};

/// Requester → admin. All keys/signatures are base64 over their canonical wire
/// bytes (vodozemac's `to_base64` / `EncKeyBinding`'s raw bytes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRequest {
    pub requester_cn: String,
    /// Curve25519 identity key (vodozemac) — base64.
    pub identity_key: String,
    /// Ed25519 fingerprint — base64. Currently informational; future kick/ban
    /// rotation messages may sign with the matching priv key.
    pub fingerprint_key: String,
    /// A single pre-published one-time key. Consumed by the admin's
    /// `olm_encrypt_to` to wrap the session key for this requester.
    pub one_time_key: String,
    /// ECDSA-P256 signature binding `identity_key` to `requester_cn`, made
    /// with the requester's TLS key. Encoded as raw signature bytes, base64.
    pub binding_signature_b64: String,
    /// The requester's step-CA cert (PEM). The admin verifies the binding
    /// signature against this cert's public key and that its CN matches
    /// `requester_cn`.
    pub cert_pem: String,
    /// Optional note shown in the admin's pending-requests panel.
    pub note: Option<String>,
    /// RFC3339 timestamp set by the requester.
    pub requested_at: String,
}

/// Admin → requester. `tag = "decision"` so the JSON has a discriminant field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum AdmissionDecision {
    /// Membership granted. The Olm pre-key message wraps the Megolm
    /// `SessionKey` of the admin's group sender; only the requester can
    /// decrypt it (it was sealed to their one-time key).
    Allow {
        /// Olm `message_type` (0 = pre-key, 1 = normal). Always 0 here.
        olm_message_type: usize,
        /// Olm ciphertext, base64.
        olm_ciphertext_b64: String,
        /// The admin's Curve25519 identity key (base64) — the requester needs
        /// it to instantiate the inbound Olm session.
        admin_identity_key: String,
        /// RFC3339 timestamp.
        admitted_at: String,
    },
    /// Denied. The requester's GUI bounces back to the chooser with a toast.
    Deny {
        reason: Option<String>,
        decided_at: String,
    },
}
