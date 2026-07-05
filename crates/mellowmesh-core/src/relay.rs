//! Wire frames for the relay protocol.
//!
//! A daemon dials *outbound* to a relay server and registers its hub. Remote
//! clients send ordinary HTTP requests to `https://<relay>/hub/<hub_id>/...`;
//! the relay wraps each request into a [`RelayFrame::Request`], forwards it
//! down the hub's WebSocket link, and the daemon answers with a
//! [`RelayFrame::Response`]. The relay never needs inbound access to the
//! daemon's network, and the remote client's own bearer token travels with
//! the request — the daemon authenticates it exactly like a local request.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RelayFrame {
    /// Daemon → relay, first frame after connecting: claim a hub id.
    Register { hub_id: String, link_key: String },
    /// Relay → daemon: the registration was accepted.
    Registered { hub_id: String },
    /// Relay → daemon: an HTTP request from a remote client.
    Request {
        id: String,
        method: String,
        /// Path below the hub prefix, e.g. `/tasks` or `/decisions/x/respond`.
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
        /// The remote client's `Authorization` header, passed through
        /// verbatim so the daemon enforces its own token auth.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        authorization: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Daemon → relay: the response for a forwarded request.
    Response {
        id: String,
        status: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content_type: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// Relay → daemon: a remote client opened a live subscription stream.
    /// The daemon opens a matching local WebSocket (the query carries the
    /// pattern and the client's token, so auth applies as usual).
    StreamOpen {
        stream_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        query: Option<String>,
    },
    /// Daemon → relay: one message for a live stream, forwarded verbatim to
    /// the remote subscriber.
    StreamData { stream_id: String, text: String },
    /// Either direction: the stream ended (client left, daemon closed, or
    /// the local subscription failed).
    StreamClose { stream_id: String },
    /// Either direction: registration or forwarding error.
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_roundtrip() {
        let frame = RelayFrame::Request {
            id: "req_1".to_string(),
            method: "POST".to_string(),
            path: "/decisions/dec_1/respond".to_string(),
            query: None,
            authorization: Some("Bearer mm_abc".to_string()),
            body: Some(r#"{"option_id":"yes"}"#.to_string()),
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert!(json.contains(r#""type":"request""#));
        let parsed: RelayFrame = serde_json::from_str(&json).unwrap();
        match parsed {
            RelayFrame::Request { id, method, .. } => {
                assert_eq!(id, "req_1");
                assert_eq!(method, "POST");
            }
            _ => panic!("wrong variant"),
        }
    }
}
