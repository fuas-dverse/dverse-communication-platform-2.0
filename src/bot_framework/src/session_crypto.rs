//! Per-session group encryption via [vodozemac](https://github.com/matrix-org/vodozemac)
//! (matrix.org's audited Olm/Megolm). Tracking issue: #109.
//!
//! Target model (see the plan + the #107 feasibility spike):
//!   * Each node owns a vodozemac [`Account`] — its Curve25519 identity key is
//!     the per-node *encryption* identity, distinct from the step-CA P-256 TLS
//!     cert key.
//!   * That Curve25519 key is **bound to the node's identity** by an ECDSA-P256
//!     signature made with the TLS key over `(cn ‖ curve25519_key)`, verified
//!     against the cert CN (Zenoh doesn't expose the publisher mTLS CN to the
//!     app, so the binding must be explicit — ported from the #107 spike).
//!   * A session is a **Megolm** group session; admission shares the Megolm key
//!     over an **Olm** 1:1 channel; kick/ban rotates the Megolm session.
//!
//! WIP: this scaffold currently creates the account and exposes its identity
//! keys. The binding (needs P-256 in this crate), Olm key delivery, Megolm
//! payload encryption, and rotation land next under #109.

use vodozemac::olm::Account;

/// A node's vodozemac identity. The Curve25519 key is the encryption identity
/// bound to the step-CA cert; the Ed25519 key is vodozemac's signing key.
pub struct SessionIdentity {
    account: Account,
}

impl SessionIdentity {
    /// Create a fresh vodozemac account (new identity + one-time keys on demand).
    pub fn new() -> Self {
        Self { account: Account::new() }
    }

    /// Base64 Curve25519 identity key — the value bound to the cert and used
    /// for Olm key agreement when delivering Megolm session keys.
    pub fn curve25519_key(&self) -> String {
        self.account.curve25519_key().to_base64()
    }

    /// Base64 Ed25519 fingerprint key.
    pub fn ed25519_key(&self) -> String {
        self.account.ed25519_key().to_base64()
    }
}

impl Default for SessionIdentity {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_exposes_identity_keys() {
        let id = SessionIdentity::new();
        // Distinct, non-empty base64 keys (Curve25519 ≠ Ed25519).
        let c = id.curve25519_key();
        let e = id.ed25519_key();
        assert!(!c.is_empty() && !e.is_empty());
        assert_ne!(c, e);
    }
}
