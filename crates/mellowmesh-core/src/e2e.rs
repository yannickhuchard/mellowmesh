//! End-to-end encryption envelopes for relayed traffic.
//!
//! Threat model: the relay operator. A remote client and the hub already
//! share a secret — the client's bearer token — so both sides derive a
//! ChaCha20-Poly1305 key from it. The client seals the entire HTTP request
//! (method, path, headers, body) into an opaque envelope; the daemon looks
//! the key up by a *separately derived* key id, unseals, dispatches locally,
//! and seals the response. The relay forwards ciphertext and sees only the
//! key id, which is useless without the hub's database.
//!
//! The daemon stores `(key_id, e2e_key)` at token-mint time; the plaintext
//! token itself is never stored (only its SHA-256 auth hash), and the e2e
//! key cannot be used to authenticate over the plain API.
//!
//! Replay: sealed requests carry a timestamp; the daemon rejects envelopes
//! outside a ±120s window. Within that window a malicious relay could
//! replay a request — acceptable for v1 and documented.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Seconds of clock skew / transit delay tolerated before an envelope is
/// considered a replay.
pub const REPLAY_WINDOW_SECS: i64 = 120;

/// Context labels giving each traffic direction its own subkey, so a request,
/// a response, a subscription proof, and a streamed delivery are never sealed
/// under the same key even though they all descend from one token.
pub const CTX_REQUEST: &str = "req";
pub const CTX_RESPONSE: &str = "resp";
pub const CTX_PROOF: &str = "proof";
pub const CTX_STREAM: &str = "stream";

/// Derive the master symmetric key from a bearer token. Bearer tokens are
/// 256-bit random secrets, so a domain-separated, salted SHA-256 is a sound
/// PRF here; per-direction subkeys are then split off with [`derive_subkey`].
pub fn derive_key(token: &str) -> [u8; 32] {
    let digest = Sha256::digest(format!("mellowmesh-e2e-key-v2|salt=mm-relay|{token}").as_bytes());
    digest.into()
}

/// Split a per-context subkey off the master key. `SHA-256(master || ctx)`
/// over a high-entropy secret is a PRF, so distinct contexts yield
/// independent keys.
fn derive_subkey(master: &[u8; 32], context: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"mellowmesh-e2e-subkey-v2|");
    hasher.update(master);
    hasher.update(b"|");
    hasher.update(context.as_bytes());
    hasher.finalize().into()
}

/// Derive the public key id from a bearer token. Distinct derivation from
/// the key and from the auth hash, so knowing the id reveals neither.
pub fn derive_key_id(token: &str) -> String {
    let digest = Sha256::digest(format!("mellowmesh-e2e-kid-v2:{token}").as_bytes());
    hex_encode(&digest)
}

/// The opaque envelope that crosses the relay (both directions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    /// Present on requests so the daemon can find the key; echoed on
    /// responses for symmetry.
    pub key_id: String,
    pub nonce: String,
    pub ciphertext: String,
}

/// Plaintext of a sealed request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedRequest {
    /// Unix seconds at sealing time (replay window check).
    pub ts: i64,
    /// Random single-use id. The daemon rejects a repeated jti inside the
    /// replay window, so a captured envelope cannot be replayed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jti: Option<String>,
    pub method: String,
    /// Path plus query string, e.g. `/tasks?limit=5`.
    pub path_and_query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Generate a random single-use id for a [`SealedRequest`].
pub fn new_jti() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex_encode(&bytes)
}

/// Plaintext of a sealed response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedResponse {
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Encrypt `plaintext` for a given `context` (traffic direction) under the
/// master `key`, binding the key id and context as associated data. Uses
/// XChaCha20-Poly1305 with a 192-bit random nonce, so random-nonce collisions
/// are not a practical concern even for high-volume streams.
pub fn seal(
    key: &[u8; 32],
    key_id: &str,
    context: &str,
    plaintext: &[u8],
) -> anyhow::Result<Envelope> {
    let subkey = derive_subkey(key, context);
    let cipher = XChaCha20Poly1305::new((&subkey).into());
    let mut nonce_bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let aad = format!("{key_id}|{context}");
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    Ok(Envelope {
        v: 2,
        key_id: key_id.to_string(),
        nonce: hex_encode(&nonce_bytes),
        ciphertext: hex_encode(&ciphertext),
    })
}

/// Decrypt an envelope sealed for `context`. Fails on any tampering of nonce,
/// ciphertext, key id, or context (all bound as associated data), or if the
/// envelope was sealed for a different context.
pub fn open(key: &[u8; 32], context: &str, envelope: &Envelope) -> anyhow::Result<Vec<u8>> {
    if envelope.v != 2 {
        anyhow::bail!("unsupported envelope version {}", envelope.v);
    }
    let nonce_bytes =
        hex_decode(&envelope.nonce).ok_or_else(|| anyhow::anyhow!("invalid nonce encoding"))?;
    let ciphertext = hex_decode(&envelope.ciphertext)
        .ok_or_else(|| anyhow::anyhow!("invalid ciphertext encoding"))?;
    if nonce_bytes.len() != 24 {
        anyhow::bail!("invalid nonce length");
    }
    let subkey = derive_subkey(key, context);
    let cipher = XChaCha20Poly1305::new((&subkey).into());
    let aad = format!("{}|{context}", envelope.key_id);
    cipher
        .decrypt(
            XNonce::from_slice(&nonce_bytes),
            Payload {
                msg: &ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("decryption failed"))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivations_are_distinct_and_deterministic() {
        let token = "mm_sample_token";
        assert_eq!(derive_key(token), derive_key(token));
        assert_eq!(derive_key_id(token), derive_key_id(token));
        // Key, key id, and the auth hash are all pairwise different.
        let auth_hash = crate::auth::hash_token(token);
        assert_ne!(hex_encode(&derive_key(token)), derive_key_id(token));
        assert_ne!(hex_encode(&derive_key(token)), auth_hash);
        assert_ne!(derive_key_id(token), auth_hash);
        // Different tokens → different keys.
        assert_ne!(derive_key(token), derive_key("mm_other"));
    }

    #[test]
    fn test_seal_open_roundtrip() {
        let key = derive_key("mm_tok");
        let key_id = derive_key_id("mm_tok");
        let payload = br#"{"ts":1,"method":"GET","path_and_query":"/tasks"}"#;
        let envelope = seal(&key, &key_id, CTX_REQUEST, payload).unwrap();
        assert_ne!(envelope.ciphertext, hex_encode(payload));
        let opened = open(&key, CTX_REQUEST, &envelope).unwrap();
        assert_eq!(opened, payload);
    }

    #[test]
    fn test_open_rejects_tampering_and_wrong_key() {
        let key = derive_key("mm_tok");
        let key_id = derive_key_id("mm_tok");
        let envelope = seal(&key, &key_id, CTX_REQUEST, b"secret").unwrap();

        // Wrong key
        assert!(open(&derive_key("mm_other"), CTX_REQUEST, &envelope).is_err());

        // Tampered ciphertext
        let mut tampered = envelope.clone();
        let mut ct = tampered.ciphertext.into_bytes();
        ct[0] = if ct[0] == b'0' { b'1' } else { b'0' };
        tampered.ciphertext = String::from_utf8(ct).unwrap();
        assert!(open(&key, CTX_REQUEST, &tampered).is_err());

        // Tampered AAD (key id swap)
        let mut swapped = envelope.clone();
        swapped.key_id = derive_key_id("mm_other");
        assert!(open(&key, CTX_REQUEST, &swapped).is_err());
    }

    #[test]
    fn test_context_separation() {
        // An envelope sealed for one direction cannot be opened as another,
        // so a captured request envelope can't be re-read as a response, etc.
        let key = derive_key("mm_tok");
        let key_id = derive_key_id("mm_tok");
        let envelope = seal(&key, &key_id, CTX_REQUEST, b"secret").unwrap();
        assert!(open(&key, CTX_REQUEST, &envelope).is_ok());
        assert!(open(&key, CTX_RESPONSE, &envelope).is_err());
        assert!(open(&key, CTX_STREAM, &envelope).is_err());
        assert!(open(&key, CTX_PROOF, &envelope).is_err());
    }

    #[test]
    fn test_hex_helpers() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x10]), "00ff10");
        assert_eq!(hex_decode("00ff10"), Some(vec![0x00, 0xff, 0x10]));
        assert_eq!(hex_decode("0"), None);
        assert_eq!(hex_decode("zz"), None);
    }
}
