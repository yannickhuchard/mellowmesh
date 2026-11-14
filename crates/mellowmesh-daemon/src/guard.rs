//! Host / Origin enforcement for every route.
//!
//! The daemon listens on loopback only, so a legitimate request always names
//! a loopback host. Rejecting any other `Host` defeats DNS-rebinding, and
//! rejecting any `Origin` other than the daemon's own (the dashboard is
//! served same-origin) stops a web page in the user's browser from driving
//! the API or opening a WebSocket subscription. Requests without an `Origin`
//! header (CLI, SDKs, the relay link's loopback dispatch) are unaffected.

use crate::state::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{header, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

/// Split `host[:port]` (with bracketed IPv6) into host and optional port.
fn split_host_port(value: &str) -> Option<(&str, Option<u16>)> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &value[..end + 2];
        let after = &rest[end + 1..];
        return match after {
            "" => Some((host, None)),
            p => Some((host, Some(p.strip_prefix(':')?.parse().ok()?))),
        };
    }
    match value.rsplit_once(':') {
        Some((h, p)) => Some((h, Some(p.parse().ok()?))),
        None => Some((value, None)),
    }
}

fn is_loopback_name(host: &str) -> bool {
    host == "127.0.0.1" || host == "[::1]" || host.eq_ignore_ascii_case("localhost")
}

/// True when a `Host` header value names this machine's loopback interface.
pub fn is_allowed_host(value: &str) -> bool {
    matches!(split_host_port(value), Some((h, _)) if is_loopback_name(h))
}

/// True when an `Origin` header value is this daemon's own loopback origin.
/// `port` is the daemon's port; when it is unknown (0) the port from the
/// request's `Host` header is used instead.
pub fn is_allowed_origin(origin: &str, port: u16, host_port: Option<u16>) -> bool {
    let Some(rest) = origin.strip_prefix("http://") else {
        return false;
    };
    match split_host_port(rest) {
        Some((h, Some(p))) if is_loopback_name(h) => {
            let expected = if port != 0 { Some(port) } else { host_port };
            expected == Some(p)
        }
        _ => false,
    }
}

pub async fn host_origin_guard(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| req.uri().authority().map(|a| a.as_str().to_string()));
    let host_port = match host.as_deref() {
        Some(h) if is_allowed_host(h) => split_host_port(h).and_then(|(_, p)| p),
        _ => {
            return (StatusCode::FORBIDDEN, "Host not allowed").into_response();
        }
    };

    if let Some(origin) = req.headers().get(header::ORIGIN) {
        let allowed = origin
            .to_str()
            .map(|o| is_allowed_origin(o, state.port, host_port))
            .unwrap_or(false);
        if !allowed {
            return (StatusCode::FORBIDDEN, "Cross-origin request rejected").into_response();
        }
    }

    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_values() {
        assert!(is_allowed_host("127.0.0.1"));
        assert!(is_allowed_host("127.0.0.1:40000"));
        assert!(is_allowed_host("localhost:1"));
        assert!(is_allowed_host("LOCALHOST"));
        assert!(is_allowed_host("[::1]:40000"));
        assert!(is_allowed_host("[::1]"));
        assert!(!is_allowed_host("evil.example:40000"));
        assert!(!is_allowed_host("127.0.0.1.evil.example"));
        assert!(!is_allowed_host("localhost.evil.example:40000"));
        assert!(!is_allowed_host("10.0.0.1:40000"));
        assert!(!is_allowed_host(""));
        assert!(!is_allowed_host("127.0.0.1:notaport"));
    }

    #[test]
    fn origin_values() {
        assert!(is_allowed_origin("http://127.0.0.1:40000", 40000, None));
        assert!(is_allowed_origin("http://localhost:40000", 40000, None));
        assert!(is_allowed_origin("http://[::1]:40000", 40000, None));
        assert!(!is_allowed_origin("http://127.0.0.1:3000", 40000, None));
        assert!(!is_allowed_origin("http://127.0.0.1", 40000, None));
        assert!(!is_allowed_origin("https://127.0.0.1:40000", 40000, None));
        assert!(!is_allowed_origin("http://evil.example:40000", 40000, None));
        assert!(!is_allowed_origin("null", 40000, None));
        // Unknown daemon port: fall back to the Host header's port.
        assert!(is_allowed_origin("http://127.0.0.1:5000", 0, Some(5000)));
        assert!(!is_allowed_origin("http://127.0.0.1:5001", 0, Some(5000)));
    }
}
