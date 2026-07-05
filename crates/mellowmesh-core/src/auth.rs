//! Identity and access-control primitives: principals, bearer tokens, and
//! topic-pattern scopes.
//!
//! A `Principal` is any authenticated actor (human, agent, node, interface).
//! A `TokenRecord` binds a hashed bearer token to a principal with read and
//! write scopes expressed as topic patterns (same wildcard grammar as
//! subscriptions: `*`, `>`, `**`).

use chrono::{DateTime, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::topic::match_topic;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Principal {
    /// Identity URI, e.g. `human://yannick` or `agent://yannick/coder`.
    pub id: String,
    /// `human`, `agent`, `node`, or `interface` — derived from the URI scheme.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenRecord {
    /// Token identifier (safe to display), e.g. `tok_01h...`.
    pub id: String,
    /// Principal URI this token authenticates as.
    pub principal: String,
    /// SHA-256 hex digest of the bearer token. The plaintext is never stored.
    pub token_hash: String,
    /// Topic patterns this token may read (subscribe / history / search).
    pub read_scopes: Vec<String>,
    /// Topic patterns this token may publish or act on.
    pub write_scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub revoked: bool,
}

/// Derive the principal kind from an identity URI scheme.
pub fn kind_of_uri(uri: &str) -> &'static str {
    match uri.split("://").next() {
        Some("human") => "human",
        Some("agent") => "agent",
        Some("node") => "node",
        _ => "interface",
    }
}

/// Generate a new random bearer token (256 bits, hex, `mm_` prefix).
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("mm_{hex}")
}

/// Hash a bearer token for storage or lookup.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether any scope pattern in the list covers the given concrete topic.
pub fn scopes_allow(scopes: &[String], topic: &str) -> bool {
    scopes.iter().any(|scope| match_topic(scope, topic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_and_hash_token() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert!(t1.starts_with("mm_"));
        assert_eq!(t1.len(), 3 + 64);
        assert_ne!(t1, t2);
        assert_eq!(hash_token(&t1), hash_token(&t1));
        assert_ne!(hash_token(&t1), hash_token(&t2));
        // 64 hex chars
        assert_eq!(hash_token(&t1).len(), 64);
    }

    #[test]
    fn test_scopes_allow() {
        let scopes = vec![
            "_agent.coder.**".to_string(),
            "_project.myapp.>".to_string(),
        ];
        assert!(scopes_allow(&scopes, "_agent.coder.inbox"));
        assert!(scopes_allow(&scopes, "_agent.coder"));
        assert!(scopes_allow(&scopes, "_project.myapp.build.logs"));
        assert!(!scopes_allow(&scopes, "_project.otherapp.build"));
        assert!(!scopes_allow(&scopes, "_forum.general"));

        let all = vec!["**".to_string()];
        assert!(scopes_allow(&all, "_forum.general"));
        assert!(scopes_allow(&all, "anything.at.all"));

        let none: Vec<String> = vec![];
        assert!(!scopes_allow(&none, "_forum.general"));
    }

    #[test]
    fn test_kind_of_uri() {
        assert_eq!(kind_of_uri("human://yannick"), "human");
        assert_eq!(kind_of_uri("agent://yannick/coder"), "agent");
        assert_eq!(kind_of_uri("node://workstation"), "node");
        assert_eq!(kind_of_uri("telegram://12345"), "interface");
    }
}
