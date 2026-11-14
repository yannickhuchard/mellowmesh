//! Status-aware request path.
//!
//! `MellowMeshClient`'s typed methods fold HTTP failures into `anyhow`
//! strings, which loses the status code the shells need (401 → re-pair,
//! 403 → explain scope, 404 → stale item). This module keeps the client's
//! transport choices — the **same** sealed `/e2e/request` tunnel (via the
//! client's public `e2e_request`) when E2E is on, plain HTTP with a bearer
//! token otherwise — but surfaces `(status, body)` so errors map precisely
//! to [`MeshError`]. Live subscriptions use the client directly.

use std::time::Duration;

use mellowmesh_client::MellowMeshClient;
use serde::de::DeserializeOwned;

use crate::error::MeshError;

pub(crate) struct Api {
    base_url: String,
    token: Option<String>,
    e2e: bool,
    http: reqwest::Client,
    client: MellowMeshClient,
}

/// True when `base_url`'s host is this machine.
pub(crate) fn url_is_local(base_url: &str) -> bool {
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Percent-encode a query value.
pub(crate) fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Percent-encode one path segment (RFC 3986 unreserved chars pass through).
pub(crate) fn path_segment(value: &str) -> Result<String, MeshError> {
    if value.is_empty() || value == "." || value == ".." || value.len() > 256 {
        return Err(MeshError::invalid(format!("invalid identifier '{value}'")));
    }
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(out)
}

impl Api {
    /// `e2e`: explicit choice, or `None` for the client's default (on for a
    /// non-local hub when a token is present).
    pub(crate) fn new(
        base_url: &str,
        token: Option<String>,
        e2e: Option<bool>,
    ) -> Result<Self, MeshError> {
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        let parsed = url::Url::parse(&base_url)
            .map_err(|e| MeshError::invalid(format!("invalid base_url: {e}")))?;
        let local = url_is_local(&base_url);
        match parsed.scheme() {
            "https" => {}
            "http" if local => {}
            "http" => {
                return Err(MeshError::invalid(
                    "base_url must use https (http is only allowed for a loopback hub)",
                ))
            }
            other => {
                return Err(MeshError::invalid(format!(
                    "unsupported base_url scheme '{other}'"
                )))
            }
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(MeshError::invalid("base_url must not contain credentials"));
        }
        let token = token.filter(|t| !t.trim().is_empty());
        let e2e = e2e.unwrap_or(token.is_some() && !local);
        if e2e && token.is_none() {
            return Err(MeshError::invalid(
                "end-to-end encryption needs a token (it derives the key from it)",
            ));
        }
        let mut client = MellowMeshClient::loopback(0)
            .with_base_url(base_url.clone())
            .with_e2e(e2e);
        if let Some(t) = &token {
            client = client.with_token(t.clone());
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| MeshError::network(e.to_string()))?;
        Ok(Self {
            base_url,
            token,
            e2e,
            http,
            client,
        })
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn e2e(&self) -> bool {
        self.e2e
    }

    pub(crate) fn client(&self) -> &MellowMeshClient {
        &self.client
    }

    /// Send a request and return `(status, body)` without judging the status.
    pub(crate) async fn raw(
        &self,
        method: reqwest::Method,
        path_and_query: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(u16, String), MeshError> {
        if self.e2e {
            return self
                .client
                .e2e_request(method.as_str(), path_and_query, body.map(|b| b.to_string()))
                .await
                .map_err(|e| {
                    let text = e.to_string();
                    // The relay/hub rejected the envelope's key id.
                    if text.contains("(401") {
                        MeshError::Unauthorized {
                            msg: crate::error::clean_msg(&text),
                        }
                    } else {
                        MeshError::network(text)
                    }
                });
        }
        let mut req = self
            .http
            .request(method, format!("{}{}", self.base_url, path_and_query));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = &body {
            req = req.json(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| MeshError::network(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| MeshError::network(e.to_string()))?;
        Ok((status, text))
    }

    /// Send and require a 2xx, returning the body.
    pub(crate) async fn call(
        &self,
        method: reqwest::Method,
        path_and_query: &str,
        body: Option<serde_json::Value>,
    ) -> Result<String, MeshError> {
        let (status, text) = self.raw(method, path_and_query, body).await?;
        if (200..300).contains(&status) {
            Ok(text)
        } else {
            Err(MeshError::from_status(status, &text))
        }
    }

    pub(crate) async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, MeshError> {
        let text = self.call(reqwest::Method::GET, path, None).await?;
        serde_json::from_str(&text)
            .map_err(|e| MeshError::server(format!("unexpected response from {path}: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_policy() {
        assert!(Api::new("http://127.0.0.1:40000", None, None).is_ok());
        assert!(!Api::new("http://127.0.0.1:40000/", None, None)
            .unwrap()
            .e2e());
        // Remote + token → E2E by default; remote http refused.
        assert!(
            Api::new("https://relay.example.com/hub/h", Some("t".into()), None)
                .unwrap()
                .e2e()
        );
        assert!(Api::new("http://relay.example.com", None, None).is_err());
        assert!(Api::new("ftp://x", None, None).is_err());
        assert!(Api::new("https://u:p@relay.example.com", None, None).is_err());
        // E2E without a token is refused instead of silently going plaintext.
        assert!(Api::new("https://relay.example.com", None, Some(true)).is_err());
    }

    #[test]
    fn encoding() {
        assert_eq!(path_segment("d_1-a").unwrap(), "d_1-a");
        assert_eq!(path_segment("../x").unwrap(), "..%2Fx");
        assert!(path_segment("..").is_err());
        assert_eq!(enc("_agent.a.*"), "_agent.a.*");
        assert_eq!(enc("a b&c"), "a+b%26c");
    }
}
