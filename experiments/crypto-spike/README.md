# crypto-spike — session-crypto core (issue #107)

Throwaway spike proving the per-session encryption substrate on real P-256
certificate material, before it becomes load-bearing in a future
`bot_framework::session_crypto` module. **Not** a workspace member (excluded in
the root `Cargo.toml`) so its dependencies never touch the trimmed workspace
dep tree.

Run: `cargo run` (prints the 8 steps) or `cargo test` (4 unit tests).

## What it proves

1. Mint two P-256 **TLS identities** (self-signed rcgen certs stand in for
   step-CA leaves — identical key alg + SPKI, CA signature irrelevant to the
   mechanics) plus a **separate** P-256 encryption keypair per node.
2. A node signs `(cn ‖ enc_pubkey)` with its **TLS** key.
3. The admin **verifies the binding**: parses the cert, checks the cert CN
   matches the claimed CN, and verifies the signature under the cert's TLS
   public key. This is the identity→enc-key binding the app layer needs because
   Zenoh doesn't expose the publisher's mTLS CN to subscribers.
4. ECIES-wrap a session key to the verified enc pubkey.
5. Unwrap with the recipient's enc private key.
6. AEAD payload round-trip under the session key.
7. **Rotation**: a v1-only holder cannot decrypt v2 traffic; a v2 holder can.
8. **Negative test**: a forged binding (mallory presents bob's public cert but
   substitutes her own enc key, signed with her own TLS key) is rejected.

## Findings → seed for `bot_framework::session_crypto`

**Crate choices (all RustCrypto):**
- `p256` (`ecdh`, `ecdsa`, `pkcs8`) — one curve for identity signing *and* key
  agreement; the cert key is ECDSA P-256, the enc key is a separate P-256 key.
- `hkdf` + `sha2` — HKDF-SHA256 to derive the AEAD wrapping key from the ECDH
  shared secret.
- `chacha20poly1305` (`getrandom`) — AEAD for both the key-wrap and the payload.
- `rcgen` (mint stand-in certs), `x509-parser` (extract cert CN + SPKI),
  `rand_core` (`getrandom`) for `OsRng`.

**ECIES wrap** (`wrap_session_key`): ephemeral P-256 keypair →
`p256::ecdh::diffie_hellman(eph_secret, recipient_enc_pub)` →
`HKDF-SHA256(shared.raw_secret_bytes(), info = "dverse/session-key-wrap/v1")`
→ ChaCha20-Poly1305 seal of the 32-byte session key. On the wire:
`{ ephemeral_pubkey_sec1, nonce, ciphertext }`.

**Identity binding** (`make_binding` / `verify_binding`): ECDSA-P256 sign/verify
over a domain-separated, length-prefixed message
(`"dverse/enc-key-binding/v1\0" ‖ len(cn) ‖ cn ‖ enc_pubkey_sec1`) so cn/key
fields can't be ambiguously concatenated. Verify rejects on any of: claimed CN
≠ expected, cert CN ≠ claimed CN, or signature not valid under the cert key.

**Nonce handling:** random 96-bit nonce per AEAD op via
`ChaCha20Poly1305::generate_nonce(&mut OsRng)`, stored alongside the ciphertext.
Each wrap and each payload gets a fresh nonce; the session key is only ever used
with random nonces, never reused deterministically.

**Key layout for the real module:** each node persists a **second** P-256
keypair (`<node>.enc.key`, PKCS#8) next to the existing TLS `<node>.key`. The
public half + binding signature ride in the join-request; the admin wraps the
session key to it on admit and re-wraps on rotation (kick/ban).

**P-256 encoding gotchas:**
- Cert public key from `x509-parser` is `subject_public_key.data` — the raw
  SEC1 point; feed it straight to `VerifyingKey::from_sec1_bytes`.
- rcgen's `KeyPair::serialize_der()` is PKCS#8 → `SigningKey::from_pkcs8_der`.
- Public keys go on the wire as **uncompressed** SEC1
  (`to_encoded_point(false)`); parse back with `PublicKey::from_sec1_bytes`.
- `ecdsa::Signer::sign` is generic over the signature type — annotate
  `let sig: Signature = key.sign(msg)` or inference fails.
- Pin `rand_core = 0.6` (the version `p256` and the `aead` stack use) to keep
  one `OsRng` type across ECDH key-gen and AEAD nonce generation.
