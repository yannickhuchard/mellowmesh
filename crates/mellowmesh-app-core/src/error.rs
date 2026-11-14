//! The single error type crossing the FFI boundary.

use mellowmesh_client::sanitize::sanitize_line;

/// Every fallible call in the app core returns this. Each variant carries a
/// human-readable, already-sanitized `msg` that shells may show as-is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum MeshError {
    /// The hub (or relay) could not be reached, timed out, or the transport
    /// failed (TLS, sealed-envelope decoding, WebSocket).
    #[error("network error: {msg}")]
    Network { msg: String },
    /// No token, or the token is unknown/revoked (HTTP 401).
    #[error("unauthorized: {msg}")]
    Unauthorized { msg: String },
    /// The token is valid but not allowed to do this (HTTP 403), e.g. a
    /// publish outside its write scope or answering a decision the principal
    /// is not the `required_decider` of.
    #[error("forbidden: {msg}")]
    Forbidden { msg: String },
    /// The addressed object does not exist (HTTP 404).
    #[error("not found: {msg}")]
    NotFound { msg: String },
    /// Rejected input: a malformed pairing URI or MeshUI card, an unknown
    /// conversation id, or a 400/409/422 from the hub.
    #[error("invalid input: {msg}")]
    InvalidInput { msg: String },
    /// The hub answered with an unexpected status or an unparseable body.
    #[error("server error: {msg}")]
    Server { msg: String },
}

/// Longest message carried in an error (hub bodies are untrusted text).
const MAX_ERROR_MSG: usize = 300;

pub(crate) fn clean_msg(msg: &str) -> String {
    let line = sanitize_line(msg);
    let trimmed = line.trim();
    if trimmed.chars().count() > MAX_ERROR_MSG {
        let mut s: String = trimmed.chars().take(MAX_ERROR_MSG).collect();
        s.push('…');
        s
    } else {
        trimmed.to_string()
    }
}

impl MeshError {
    pub(crate) fn invalid(msg: impl AsRef<str>) -> Self {
        MeshError::InvalidInput {
            msg: clean_msg(msg.as_ref()),
        }
    }

    pub(crate) fn network(msg: impl AsRef<str>) -> Self {
        MeshError::Network {
            msg: clean_msg(msg.as_ref()),
        }
    }

    pub(crate) fn server(msg: impl AsRef<str>) -> Self {
        MeshError::Server {
            msg: clean_msg(msg.as_ref()),
        }
    }

    /// Map a non-success HTTP status (and its body) to an error variant.
    pub(crate) fn from_status(status: u16, body: &str) -> Self {
        let msg = clean_msg(body);
        let msg = if msg.is_empty() {
            format!("HTTP {status}")
        } else {
            msg
        };
        match status {
            401 => MeshError::Unauthorized { msg },
            403 => MeshError::Forbidden { msg },
            404 => MeshError::NotFound { msg },
            400 | 409 | 413 | 422 => MeshError::InvalidInput { msg },
            _ => MeshError::Server {
                msg: format!("HTTP {status}: {msg}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping() {
        assert!(matches!(
            MeshError::from_status(401, "x"),
            MeshError::Unauthorized { .. }
        ));
        assert!(matches!(
            MeshError::from_status(403, "x"),
            MeshError::Forbidden { .. }
        ));
        assert!(matches!(
            MeshError::from_status(404, ""),
            MeshError::NotFound { .. }
        ));
        assert!(matches!(
            MeshError::from_status(409, "already answered"),
            MeshError::InvalidInput { .. }
        ));
        assert!(matches!(
            MeshError::from_status(500, "boom"),
            MeshError::Server { .. }
        ));
    }

    #[test]
    fn error_messages_are_sanitized_and_bounded() {
        let e = MeshError::from_status(400, &format!("\u{1b}[31mbad\n{}", "x".repeat(1000)));
        let MeshError::InvalidInput { msg } = e else {
            panic!("wrong variant")
        };
        assert!(!msg.contains('\u{1b}'));
        assert!(!msg.contains('\n'));
        assert!(msg.chars().count() <= MAX_ERROR_MSG + 1);
    }
}
