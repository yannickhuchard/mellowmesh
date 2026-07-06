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
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Seconds of clock skew / transit delay tolerated before an envelope is
/// considered a replay.
pub const REPLAY_WINDOW_SECS: i64 = 120;

/// Derive the symmetric encryption key from a bearer token.
pub fn derive_key(token: &str) -> [u8; 32] {
    let digest = Sha256::digest(format!("mellowmesh-e2e-key-v1:{token}").as_bytes());
    digest.into()
}

/// Derive the public key id from a bearer token. Distinct derivation from
/// the key and from the auth hash, so knowing the id reveals neither.
pub fn derive_key_id(token: &str) -> String {
    let digest = Sha256::digest(format!("mellowmesh-e2e-kid-v1:{token}").as_bytes());
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
    pub method: String,
    /// Path plus query string, e.g. `/tasks?limit=5`.
    pub path_and_query: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
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

/// Encrypt `plaintext` under `key`, binding the key id as associated data.
pub fn seal(key: &[u8; 32], key_id: &str, plaintext: &[u8]) -> anyhow::Result<Envelope> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: plaintext,
                aad: key_id.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("encryption failed"))?;
    Ok(Envelope {
        v: 1,
        key_id: key_id.to_string(),
        nonce: hex_encode(&nonce_bytes),
        ciphertext: hex_encode(&ciphertext),
    })
}

/// Decrypt an envelope. Fails on any tampering of nonce, ciphertext, or
/// key id (bound as associated data).
pub fn open(key: &[u8; 32], envelope: &Envelope) -> anyhow::Result<Vec<u8>> {
    if envelope.v != 1 {
        anyhow::bail!("unsupported envelope version {}", envelope.v);
    }
    let nonce_bytes =
        hex_decode(&envelope.nonce).ok_or_else(|| anyhow::anyhow!("invalid nonce encoding"))?;
    let ciphertext = hex_decode(&envelope.ciphertext)
        .ok_or_else(|| anyhow::anyhow!("invalid ciphertext encoding"))?;
    if nonce_bytes.len() != 12 {
        anyhow::bail!("invalid nonce length");
    }
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: &ciphertext,
                aad: envelope.key_id.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("decryption failed"))
}

pub fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
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
        let envelope = seal(&key, &key_id, payload).unwrap();
        assert_ne!(envelope.ciphertext, hex_encode(payload));
        let opened = open(&key, &envelope).unwrap();
        assert_eq!(opened, payload);
    }

    #[test]
    fn test_open_rejects_tampering_and_wrong_key() {
        let key = derive_key("mm_tok");
        let key_id = derive_key_id("mm_tok");
        let envelope = seal(&key, &key_id, b"secret").unwrap();

        // Wrong key
        assert!(open(&derive_key("mm_other"), &envelope).is_err());

        // Tampered ciphertext
        let mut tampered = envelope.clone();
        let mut ct = tampered.ciphertext.into_bytes();
        ct[0] = if ct[0] == b'0' { b'1' } else { b'0' };
        tampered.ciphertext = String::from_utf8(ct).unwrap();
        assert!(open(&key, &tampered).is_err());

        // Tampered AAD (key id swap)
        let mut swapped = envelope.clone();
        swapped.key_id = derive_key_id("mm_other");
        assert!(open(&key, &swapped).is_err());
    }

    #[test]
    fn test_hex_helpers() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0x10]), "00ff10");
        assert_eq!(hex_decode("00ff10"), Some(vec![0x00, 0xff, 0x10]));
        assert_eq!(hex_decode("0"), None);
        assert_eq!(hex_decode("zz"), None);
    }
}
