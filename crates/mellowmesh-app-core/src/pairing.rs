//! Pairing URIs (shown as a QR code by the hub, scanned by the phone).
//!
//! ```text
//! mellowmesh-pair:?v=1&relay=<https url>&hub=<hub id>&code=<one-time code>&fp=<sha256 fingerprint>[&name=<label>]
//! ```
//!
//! * `v`     — optional, format version; only `1` is accepted.
//! * `relay` — the relay base URL. `https` is required; `http` is accepted
//!   only for a loopback host (local development). No credentials, query or
//!   fragment. Max 512 bytes.
//! * `hub`   — the hub id on that relay: 1..=64 of `[A-Za-z0-9_-]` (same rule
//!   as the relay's `is_valid_hub_id`).
//! * `code`  — one-time pairing code: 6..=64 of `[A-Za-z0-9-]`.
//! * `fp`    — SHA-256 fingerprint of the hub's pairing key: 64 hex digits,
//!   optionally prefixed `sha256:` and/or written as 32 colon-separated
//!   pairs. Normalized to lowercase hex without separators.
//! * `name`  — optional display label for the hub (≤ 64 chars, sanitized).
//!
//! Every parameter may appear once; unknown parameters are rejected, as is
//! anything over 2048 bytes. Parsing only validates: the server-side pairing
//! exchange (redeeming `code` for a device token, checking `fp`) does not
//! exist yet.

use crate::error::MeshError;
use mellowmesh_client::sanitize::sanitize_line;

pub const PAIRING_SCHEME: &str = "mellowmesh-pair";
const MAX_URI: usize = 2048;
const MAX_RELAY: usize = 512;
const MAX_HUB_ID: usize = 64;
const MIN_CODE: usize = 6;
const MAX_CODE: usize = 64;
const MAX_NAME: usize = 64;

/// A validated pairing URI.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct PairingInfo {
    pub version: u32,
    /// Relay base URL, without trailing slash.
    pub relay_url: String,
    pub hub_id: String,
    /// The hub's base URL through the relay: `<relay_url>/hub/<hub_id>`.
    pub hub_url: String,
    pub code: String,
    /// Lowercase hex SHA-256 (64 chars).
    pub fingerprint: String,
    pub hub_name: Option<String>,
}

fn bad(msg: impl AsRef<str>) -> MeshError {
    MeshError::invalid(format!("pairing URI: {}", msg.as_ref()))
}

/// Parse and strictly validate a `mellowmesh-pair:` URI.
#[uniffi::export]
pub fn parse_pairing_uri(uri: String) -> Result<PairingInfo, MeshError> {
    let uri = uri.trim();
    if uri.len() > MAX_URI {
        return Err(bad("too long"));
    }
    let (scheme, rest) = uri.split_once(':').ok_or_else(|| bad("missing scheme"))?;
    if !scheme.eq_ignore_ascii_case(PAIRING_SCHEME) {
        return Err(bad(format!("scheme must be '{PAIRING_SCHEME}'")));
    }
    // The only accepted form is an empty path followed by a query
    // (`mellowmesh-pair:?…`); `mellowmesh-pair://…` is rejected.
    let query = rest
        .strip_prefix('?')
        .ok_or_else(|| bad("expected 'mellowmesh-pair:?<parameters>'"))?;
    if query.contains('#') {
        return Err(bad("fragments are not allowed"));
    }

    let mut version = None;
    let mut relay = None;
    let mut hub = None;
    let mut code = None;
    let mut fp = None;
    let mut name = None;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        let slot = match k.as_ref() {
            "v" => &mut version,
            "relay" => &mut relay,
            "hub" => &mut hub,
            "code" => &mut code,
            "fp" => &mut fp,
            "name" => &mut name,
            other => return Err(bad(format!("unknown parameter '{}'", sanitize_line(other)))),
        };
        if slot.is_some() {
            return Err(bad(format!("duplicate parameter '{k}'")));
        }
        *slot = Some(v.into_owned());
    }

    let version = match version.as_deref() {
        None | Some("1") => 1,
        Some(other) => {
            return Err(bad(format!(
                "unsupported version '{}'",
                sanitize_line(other)
            )))
        }
    };
    let relay_url = validate_relay(&relay.ok_or_else(|| bad("missing 'relay'"))?)?;
    let hub_id = hub.ok_or_else(|| bad("missing 'hub'"))?;
    if !is_valid_hub_id(&hub_id) {
        return Err(bad("'hub' must be 1-64 characters of A-Z a-z 0-9 _ -"));
    }
    let code = code.ok_or_else(|| bad("missing 'code'"))?;
    if !(MIN_CODE..=MAX_CODE).contains(&code.len())
        || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(bad("'code' must be 6-64 characters of A-Z a-z 0-9 -"));
    }
    let fingerprint = normalize_fingerprint(&fp.ok_or_else(|| bad("missing 'fp'"))?)?;
    let hub_name = match name {
        None => None,
        Some(n) => {
            let n = sanitize_line(n.trim());
            if n.is_empty() || n.chars().count() > MAX_NAME {
                return Err(bad("'name' must be 1-64 characters"));
            }
            Some(n)
        }
    };

    Ok(PairingInfo {
        version,
        hub_url: format!("{relay_url}/hub/{hub_id}"),
        relay_url,
        hub_id,
        code,
        fingerprint,
        hub_name,
    })
}

/// Same rule as `mellowmesh_relay::is_valid_hub_id`.
pub fn is_valid_hub_id(hub_id: &str) -> bool {
    !hub_id.is_empty()
        && hub_id.len() <= MAX_HUB_ID
        && hub_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn validate_relay(raw: &str) -> Result<String, MeshError> {
    if raw.len() > MAX_RELAY {
        return Err(bad("'relay' too long"));
    }
    let url = url::Url::parse(raw).map_err(|e| bad(format!("invalid 'relay' URL: {e}")))?;
    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&url) => {}
        "http" => return Err(bad("'relay' must use https (http only for loopback)")),
        _ => return Err(bad("'relay' must be an https URL")),
    }
    if url.host().is_none() {
        return Err(bad("'relay' has no host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad("'relay' must not contain credentials"));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(bad("'relay' must not have a query or fragment"));
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

fn is_loopback(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn normalize_fingerprint(raw: &str) -> Result<String, MeshError> {
    let lower = raw.trim().to_ascii_lowercase();
    let body = lower.strip_prefix("sha256:").unwrap_or(&lower);
    let hex: String = if body.contains(':') {
        let parts: Vec<&str> = body.split(':').collect();
        if parts.len() != 32 || parts.iter().any(|p| p.len() != 2) {
            return Err(bad("'fp' colon form must be 32 hex pairs"));
        }
        parts.concat()
    } else {
        body.to_string()
    };
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(bad("'fp' must be a SHA-256 fingerprint (64 hex digits)"));
    }
    Ok(hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FP: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    fn uri(params: &str) -> String {
        format!("mellowmesh-pair:?{params}")
    }

    fn ok_params() -> String {
        format!("relay=https%3A%2F%2Frelay.example.com%2F&hub=hub_01-x&code=ABCD-1234&fp={FP}")
    }

    #[test]
    fn parses_valid_uri() {
        let p = parse_pairing_uri(uri(&ok_params())).unwrap();
        assert_eq!(p.version, 1);
        assert_eq!(p.relay_url, "https://relay.example.com");
        assert_eq!(p.hub_id, "hub_01-x");
        assert_eq!(p.hub_url, "https://relay.example.com/hub/hub_01-x");
        assert_eq!(p.code, "ABCD-1234");
        assert_eq!(p.fingerprint, FP);
        assert_eq!(p.hub_name, None);
    }

    #[test]
    fn accepts_optional_fields_and_fingerprint_forms() {
        let colon = FP
            .as_bytes()
            .chunks(2)
            .map(|c| std::str::from_utf8(c).unwrap().to_uppercase())
            .collect::<Vec<_>>()
            .join(":");
        let p = parse_pairing_uri(uri(&format!(
            "v=1&relay=https://relay.example.com/base&hub=h&code=123456&fp=sha256:{colon}&name=Home%20hub"
        )))
        .unwrap();
        assert_eq!(p.fingerprint, FP);
        assert_eq!(p.relay_url, "https://relay.example.com/base");
        assert_eq!(p.hub_name.as_deref(), Some("Home hub"));
        // Loopback http is allowed for development.
        let p = parse_pairing_uri(uri(&format!(
            "relay=http://127.0.0.1:8080&hub=h&code=123456&fp={FP}"
        )))
        .unwrap();
        assert_eq!(p.hub_url, "http://127.0.0.1:8080/hub/h");
        // Scheme is case-insensitive.
        assert!(parse_pairing_uri(format!("MellowMesh-Pair:?{}", ok_params())).is_ok());
    }

    #[test]
    fn rejects_invalid_uris() {
        let cases = [
            "https://relay.example.com".to_string(),
            "mellowmesh-pair:relay=x".to_string(),
            "mellowmesh-pair://?relay=x".to_string(),
            uri(&format!(
                "relay=http://relay.example.com&hub=h&code=123456&fp={FP}"
            )),
            uri(&format!(
                "relay=http://localhost.evil.com&hub=h&code=123456&fp={FP}"
            )),
            uri(&format!(
                "relay=ftp://relay.example.com&hub=h&code=123456&fp={FP}"
            )),
            uri(&format!(
                "relay=https://u:p@relay.example.com&hub=h&code=123456&fp={FP}"
            )),
            uri(&format!(
                "relay=https://relay.example.com?x=1&hub=h&code=123456&fp={FP}"
            )),
            uri(&format!(
                "relay=https://relay.example.com&hub=bad%20id&code=123456&fp={FP}"
            )),
            uri(&format!(
                "relay=https://relay.example.com&hub=h%2F..&code=123456&fp={FP}"
            )),
            uri(&format!(
                "relay=https://relay.example.com&hub={}&code=123456&fp={FP}",
                "a".repeat(65)
            )),
            uri(&format!(
                "relay=https://relay.example.com&hub=h&code=12345&fp={FP}"
            )),
            uri(&format!(
                "relay=https://relay.example.com&hub=h&code=abc_def&fp={FP}"
            )),
            uri("relay=https://relay.example.com&hub=h&code=123456&fp=abc"),
            uri(&format!(
                "relay=https://relay.example.com&hub=h&code=123456&fp={}",
                &FP[..63]
            )),
            uri(&format!(
                "relay=https://relay.example.com&hub=h&code=123456&fp={}g",
                &FP[..63]
            )),
            uri(&format!("{}&hub=again", ok_params())),
            uri(&format!("{}&token=secret", ok_params())),
            uri(&format!("v=2&{}", ok_params())),
            uri(&format!("{}#frag", ok_params())),
            uri("hub=h&code=123456"),
            uri(&format!("{}&name=", ok_params())),
            format!(
                "mellowmesh-pair:?{}&name={}",
                ok_params(),
                "x".repeat(MAX_URI)
            ),
        ];
        for c in cases {
            assert!(
                matches!(
                    parse_pairing_uri(c.clone()),
                    Err(MeshError::InvalidInput { .. })
                ),
                "should reject {c}"
            );
        }
    }
}
