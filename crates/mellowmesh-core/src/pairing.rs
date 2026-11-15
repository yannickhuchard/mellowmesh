//! One-step device pairing: the `POST /pair`, `GET /pair/:id` and
//! `/devices` shapes, and the `mellowmesh-pair:` link the apps scan
//! (contract: `docs/network.md`, "Pairing a device").

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Device name used when `POST /pair` names none.
pub const DEFAULT_DEVICE_NAME: &str = "Phone";

/// Maximum length (characters) of a device name.
pub const MAX_DEVICE_NAME: usize = 64;

/// How long a freshly minted device token stays valid if it is never used.
pub const DEVICE_TOKEN_TTL_SECS: i64 = 15 * 60;

/// `POST /pair` body. Every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Address the device will use (`http(s)://…`). Omitted: LAN mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// The hub's LAN listener.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LanStatus {
    /// The persisted `lan_access` setting.
    pub enabled: bool,
    /// `ip:port` of the LAN listener, when there is one.
    pub address: Option<String>,
    /// Whether the hub currently accepts connections on that address.
    pub listening: bool,
}

/// `POST /pair` response (`201`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairResponse {
    /// The `mellowmesh-pair:` link (contains the token: a secret).
    pub link: String,
    pub url: String,
    pub token_id: String,
    pub name: String,
    /// Network name.
    pub network: String,
    /// Owner principal the token authenticates as.
    pub principal: String,
    /// When the token expires if the device never uses it.
    pub expires_at: DateTime<Utc>,
    pub lan: LanStatus,
    /// Short claim code typed instead of scanning the QR (same lifetime,
    /// single use). Empty when the daemon predates claim codes.
    #[serde(default)]
    pub code: String,
}

/// `POST /pair/claim` body: the short code read off the hub's screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRequest {
    pub code: String,
}

/// `POST /pair/claim` response: the same link the QR encodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimResponse {
    pub link: String,
}

/// Alphabet of claim codes: no 0/O, 1/I/L — every character survives being
/// read aloud or copied from a screen.
pub const CLAIM_CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";

/// Length of a claim code (characters of [`CLAIM_CODE_ALPHABET`]).
pub const CLAIM_CODE_LEN: usize = 6;

/// A fresh claim code, formatted `XXX-XXX` for reading.
pub fn generate_claim_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut code = String::with_capacity(CLAIM_CODE_LEN + 1);
    for i in 0..CLAIM_CODE_LEN {
        if i == CLAIM_CODE_LEN / 2 {
            code.push('-');
        }
        code.push(CLAIM_CODE_ALPHABET[rng.gen_range(0..CLAIM_CODE_ALPHABET.len())] as char);
    }
    code
}

/// A claim code as typed, normalized for lookup: uppercased, separators
/// dropped, the easy misreadings mapped back (`0`→`O` is not in the
/// alphabet, so `0`/`O` both normalize away from valid codes; `1`/`I`/`L`
/// likewise). Returns `None` when what remains is not a plausible code.
pub fn normalize_claim_code(typed: &str) -> Option<String> {
    let cleaned: String = typed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (cleaned.len() == CLAIM_CODE_LEN
        && cleaned
            .bytes()
            .all(|b| CLAIM_CODE_ALPHABET.contains(&b)))
    .then_some(cleaned)
}

/// `GET /pair/:token_id` response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairStatus {
    pub token_id: String,
    pub name: String,
    /// True once the token has authenticated a request.
    pub connected: bool,
    pub last_seen: Option<DateTime<Utc>>,
    /// True when the token expired unused (or was revoked).
    pub expired: bool,
}

/// One entry of `GET /devices`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub token_id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub connected: bool,
    /// Expiry of a pending (never used) device; `None` once connected.
    pub expires_at: Option<DateTime<Utc>>,
}

/// A device name as stored: control characters removed, whitespace
/// collapsed, at most [`MAX_DEVICE_NAME`] characters; [`DEFAULT_DEVICE_NAME`]
/// when nothing is left.
pub fn sanitize_device_name(name: Option<&str>) -> String {
    let cleaned: String = name
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_DEVICE_NAME)
        .collect::<String>()
        .trim()
        .to_string();
    if cleaned.is_empty() {
        DEFAULT_DEVICE_NAME.to_string()
    } else {
        cleaned
    }
}

/// Percent-encode a query value: only RFC 3986 unreserved characters pass.
fn pct(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The v2 pairing link the apps scan or paste.
pub fn pairing_link(url: &str, token: &str, network_name: &str, principal: &str) -> String {
    format!(
        "mellowmesh-pair:?v=2&url={}&token={}&name={}&principal={}",
        pct(url.trim_end_matches('/')),
        pct(token),
        pct(network_name),
        pct(principal)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_is_percent_encoded() {
        let link = pairing_link(
            "http://192.168.1.20:40000/",
            "mm_abc",
            "Yannick's mesh",
            "human://yannick",
        );
        assert_eq!(
            link,
            "mellowmesh-pair:?v=2&url=http%3A%2F%2F192.168.1.20%3A40000&token=mm_abc\
             &name=Yannick%27s%20mesh&principal=human%3A%2F%2Fyannick"
        );
    }

    #[test]
    fn device_names() {
        assert_eq!(sanitize_device_name(None), "Phone");
        assert_eq!(sanitize_device_name(Some("  \n\t ")), "Phone");
        assert_eq!(
            sanitize_device_name(Some(" Pixel\u{1b}[31m  8 ")),
            "Pixel [31m 8"
        );
        let long = "x".repeat(100);
        assert_eq!(sanitize_device_name(Some(&long)).chars().count(), 64);
        let req: PairRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(req, PairRequest::default());
    }
}
