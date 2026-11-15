pub mod autostart;
pub mod mcp;
pub mod sanitize;

use futures_util::Stream;
use futures_util::StreamExt;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use mellowmesh_core::network::{
    AccessCheck, Network, NetworkPatch, NetworkView, Space, Team, TeamRole, WhoAmI,
};
use mellowmesh_core::pairing::{DeviceInfo, PairRequest, PairResponse, PairStatus};
use mellowmesh_core::task::{Task, TaskGraph, TaskUpdate};
use mellowmesh_core::telemetry::TraceSession;
pub use mellowmesh_core::topic::NamedTopic;
use reqwest::Method;
use std::sync::OnceLock;
use std::time::Duration;

/// Maximum `limit` the daemon accepts on `/history`.
const MAX_HISTORY_LIMIT: usize = 1000;

#[derive(Clone)]
pub struct MellowMeshClient {
    base_url: String,
    token: Option<String>,
    /// Transparent end-to-end encryption: when enabled (and a token is
    /// present), every API call is sealed and sent through `/e2e/request`
    /// instead of plain HTTP — a relay in the middle sees only ciphertext.
    ///
    /// `Some(_)` is an explicit choice (`with_e2e` or `MELLOWMESH_E2E`);
    /// `None` means "default": on for a non-local base URL with a token,
    /// off otherwise.
    e2e: Option<bool>,
}

/// Response from the single dispatch point, uniform across the plain and
/// sealed transports.
struct ApiResponse {
    status: u16,
    body: String,
}

impl ApiResponse {
    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    fn ensure(self, context: &str) -> anyhow::Result<Self> {
        if self.is_success() {
            Ok(self)
        } else {
            Err(anyhow::anyhow!(
                "{context} failed: {}",
                sanitize::sanitize_text(&self.body)
            ))
        }
    }

    /// Like [`Self::ensure`], for endpoints whose errors are
    /// `{ "error": "…" }`: the message alone is reported.
    fn ensure_api(self, context: &str) -> anyhow::Result<Self> {
        if self.is_success() {
            return Ok(self);
        }
        let message = serde_json::from_str::<serde_json::Value>(&self.body)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
            .unwrap_or_else(|| self.body.clone());
        Err(anyhow::anyhow!(
            "{context} failed ({}): {}",
            self.status,
            sanitize::sanitize_text(&message)
        ))
    }

    fn json<T: serde::de::DeserializeOwned>(&self) -> anyhow::Result<T> {
        Ok(serde_json::from_str(&self.body)?)
    }
}

/// The process-wide HTTP client. Building a `reqwest::Client` is expensive
/// (connection pool, TLS setup), so every `MellowMeshClient` shares this one.
/// It carries NO default headers: the bearer token is attached per request,
/// which keeps the E2E envelope POST free of any Authorization header.
fn shared_http() -> &'static reqwest::Client {
    static HTTP: OnceLock<reqwest::Client> = OnceLock::new();
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

/// Remote base URL override (e.g. `https://relay.example.com/hub/<hub_id>`),
/// letting the same client and CLI drive a hub through a relay from any
/// machine. When set, the daemon autostart is skipped.
fn remote_base_url() -> Option<String> {
    std::env::var("MELLOWMESH_URL")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
}

/// Parse an E2E switch value: `Some(true)` for 1/true/yes, `Some(false)` for
/// 0/false/no, `None` (use the default) for anything else.
fn parse_e2e_switch(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" => Some(true),
        "0" | "false" | "no" => Some(false),
        _ => None,
    }
}

fn e2e_env_setting() -> Option<bool> {
    std::env::var("MELLOWMESH_E2E")
        .ok()
        .and_then(|v| parse_e2e_switch(&v))
}

/// Percent-encode a query-string value.
fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

/// Percent-encode one URL path segment. Only RFC 3986 unreserved characters
/// (`A-Z a-z 0-9 - . _ ~`) pass through; everything else, including `/`,
/// `?`, `#` and `%`, is encoded as `%XX`. Empty, `.` and `..` segments are
/// rejected: URL resolution would otherwise collapse them and change which
/// endpoint the request reaches.
fn path_segment(value: &str) -> anyhow::Result<String> {
    if value.is_empty() || value == "." || value == ".." {
        anyhow::bail!("Invalid identifier {value:?}: must not be empty, '.' or '..'");
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

/// Percent-encode a multi-segment relative path (e.g. a wiki page path such
/// as `procedures/deploy.md`): each `/`-separated segment is encoded with
/// [`path_segment`], so empty, `.` and `..` segments are rejected.
fn path_segments(value: &str) -> anyhow::Result<String> {
    value
        .split('/')
        .map(path_segment)
        .collect::<anyhow::Result<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

/// `/wiki/<wiki>/pages/<page path>` with every component encoded.
fn wiki_page_path(wiki: &str, page: &str) -> anyhow::Result<String> {
    Ok(format!(
        "/wiki/{}/pages/{}",
        path_segment(wiki)?,
        path_segments(page)?
    ))
}

/// Build the `/history` path, optionally filtered server-side by a topic
/// pattern. The limit is clamped to what the daemon accepts.
fn history_path(limit: usize, pattern: Option<&str>) -> String {
    let limit = limit.min(MAX_HISTORY_LIMIT);
    match pattern {
        Some(p) => format!("/history?limit={limit}&pattern={}", enc(p)),
        None => format!("/history?limit={limit}"),
    }
}

/// True when `base_url`'s host is this machine, parsed properly (so that
/// `http://127.0.0.1@evil.com` or `http://localhost.evil.com` do not count).
fn url_is_local(base_url: &str) -> bool {
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

impl MellowMeshClient {
    pub fn new(port: u16) -> Self {
        // Token resolution: explicit env var wins; otherwise anonymous
        // (fine against a daemon in open mode).
        let token = std::env::var("MELLOWMESH_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty());
        let base_url = remote_base_url().unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
        Self {
            base_url,
            token,
            e2e: e2e_env_setting(),
        }
    }

    /// A client hard-wired to this machine's own daemon on `port`, ignoring
    /// the `MELLOWMESH_URL` / `MELLOWMESH_E2E` environment. Those variables
    /// configure a *remote* CLI; the daemon's own internal dispatch (MCP
    /// endpoint, connectors) must always talk to loopback, never bounce back
    /// out through a relay.
    pub fn loopback(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            token: None,
            e2e: Some(false),
        }
    }

    /// Use an explicit bearer token instead of the `MELLOWMESH_TOKEN` env var.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// Point the client at an explicit base URL (e.g. a relay hub URL or a
    /// test server), bypassing the `MELLOWMESH_URL` env var.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Enable or disable transparent end-to-end encryption explicitly,
    /// overriding both the `MELLOWMESH_E2E` env var and the default (which
    /// is on for a non-local base URL with a token).
    pub fn with_e2e(mut self, enabled: bool) -> Self {
        self.e2e = Some(enabled);
        self
    }

    /// Whether E2E is requested: the explicit setting if there is one,
    /// otherwise on for a remote (non-local) hub when a token is present.
    fn e2e_requested(&self) -> bool {
        self.e2e
            .unwrap_or_else(|| self.token.is_some() && !self.is_local())
    }

    fn e2e_enabled(&self) -> bool {
        self.e2e_requested() && self.token.is_some()
    }

    /// Whether a bearer token is configured for this client.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Whether this client talks to a daemon on this machine (loopback),
    /// rather than to a remote hub through a relay.
    pub fn targets_local(&self) -> bool {
        self.is_local()
    }

    /// True when the base URL is this machine (no relay in the path), so a
    /// bearer token on the wire never reaches a third party.
    fn is_local(&self) -> bool {
        url_is_local(&self.base_url)
    }

    /// Guard against the silent-plaintext-downgrade footgun: if E2E was asked
    /// for but no token is present, refuse rather than sending in the clear.
    fn ensure_e2e_consistent(&self) -> anyhow::Result<()> {
        if self.e2e_requested() && self.token.is_none() {
            anyhow::bail!(
                "MELLOWMESH_E2E is enabled but no token is set — refusing to send in plaintext. \
                 Set MELLOWMESH_TOKEN (E2E derives its key from the token)."
            );
        }
        Ok(())
    }

    /// The single dispatch point every API method goes through. In E2E mode
    /// the entire request is sealed and tunneled via `/e2e/request`; nothing
    /// can accidentally fall back to plaintext.
    async fn send(
        &self,
        method: Method,
        path_and_query: &str,
        body: Option<serde_json::Value>,
    ) -> anyhow::Result<ApiResponse> {
        self.ensure_e2e_consistent()?;
        if self.e2e_enabled() {
            let (status, text) = self
                .e2e_request(method.as_str(), path_and_query, body.map(|b| b.to_string()))
                .await?;
            return Ok(ApiResponse { status, body: text });
        }
        let mut req = shared_http().request(method, format!("{}{}", self.base_url, path_and_query));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        if let Some(b) = &body {
            req = req.json(b);
        }
        let resp = req.send().await?;
        Ok(ApiResponse {
            status: resp.status().as_u16(),
            body: resp.text().await?,
        })
    }

    /// Send an end-to-end encrypted request through the relay: the method,
    /// path, bearer token, and body are sealed with a key derived from the
    /// token, POSTed to `<base>/e2e/request` as opaque ciphertext, and the
    /// response is unsealed. A relay in the middle sees only ciphertext.
    ///
    /// Requires a token (the shared secret). Returns `(status, body)`.
    pub async fn e2e_request(
        &self,
        method: &str,
        path_and_query: &str,
        body: Option<String>,
    ) -> anyhow::Result<(u16, String)> {
        use mellowmesh_core::e2e::{
            derive_key, derive_key_id, new_jti, open, seal, Envelope, SealedRequest,
            SealedResponse, CTX_REQUEST, CTX_RESPONSE,
        };
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("End-to-end encryption requires a bearer token"))?;
        let key = derive_key(token);
        let key_id = derive_key_id(token);

        let sealed = SealedRequest {
            ts: chrono::Utc::now().timestamp(),
            jti: Some(new_jti()),
            method: method.to_string(),
            path_and_query: path_and_query.to_string(),
            authorization: Some(format!("Bearer {token}")),
            body,
        };
        let envelope = seal(&key, &key_id, CTX_REQUEST, &serde_json::to_vec(&sealed)?)?;

        // Use the header-less client: the token is inside the ciphertext and
        // must never leak to the relay as an Authorization header.
        let resp = shared_http()
            .post(format!("{}/e2e/request", self.base_url))
            .json(&envelope)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "E2E transport failed ({}): {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        let reply: Envelope = resp.json().await?;
        let opened = open(&key, CTX_RESPONSE, &reply)?;
        let sealed_resp: SealedResponse = serde_json::from_slice(&opened)?;
        Ok((sealed_resp.status, sealed_resp.body.unwrap_or_default()))
    }

    pub async fn connect() -> anyhow::Result<Self> {
        Self::connect_with_port(40000).await
    }

    pub async fn connect_with_port(port: u16) -> anyhow::Result<Self> {
        // A remote hub URL means there is no local daemon to autostart.
        if remote_base_url().is_none() && !autostart::is_daemon_running_async(port).await {
            autostart::spawn_daemon_async(port).await?;
        }
        Ok(Self::new(port))
    }

    pub async fn publish(&self, msg: &Message) -> anyhow::Result<()> {
        self.send(Method::POST, "/publish", Some(serde_json::to_value(msg)?))
            .await?
            .ensure("Publish")?;
        Ok(())
    }

    pub async fn subscribe(
        &self,
        pattern: &str,
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<Message>>> {
        self.subscribe_with_options(pattern, false).await
    }

    pub async fn subscribe_with_options(
        &self,
        pattern: &str,
        case_insensitive: bool,
    ) -> anyhow::Result<impl Stream<Item = anyhow::Result<Message>>> {
        use mellowmesh_core::e2e::{
            derive_key, derive_key_id, new_jti, open, seal, Envelope, SealedRequest, CTX_PROOF,
            CTX_STREAM,
        };
        use tokio_tungstenite::tungstenite::Message as WsMessage;

        self.ensure_e2e_consistent()?;

        // Local hubs expose /ws directly; relayed hubs expose it at
        // <relay>/hub/<id>/ws with the same query parameters.
        let ws_base = self
            .base_url
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        let mut url = url::Url::parse(&format!("{ws_base}/ws"))?;
        url.query_pairs_mut().append_pair("pattern", pattern);
        if case_insensitive {
            url.query_pairs_mut()
                .append_pair("case_insensitive", "true");
        }

        // Encrypted subscriptions: instead of the raw token, the query
        // carries a sealed proof (which authenticates the subscriber), and
        // every delivered message arrives as a sealed envelope.
        let e2e_key = if self.e2e_enabled() {
            let token = self.token.as_ref().unwrap();
            let key = derive_key(token);
            let key_id = derive_key_id(token);
            let proof = SealedRequest {
                ts: chrono::Utc::now().timestamp(),
                jti: Some(new_jti()),
                method: "SUBSCRIBE".to_string(),
                path_and_query: "/ws".to_string(),
                authorization: Some(format!("Bearer {token}")),
                body: None,
            };
            let envelope = seal(&key, &key_id, CTX_PROOF, &serde_json::to_vec(&proof)?)?;
            url.query_pairs_mut()
                .append_pair("e2e_kid", &envelope.key_id);
            url.query_pairs_mut()
                .append_pair("e2e_nonce", &envelope.nonce);
            url.query_pairs_mut()
                .append_pair("e2e_ct", &envelope.ciphertext);
            Some(key)
        } else {
            if let Some(token) = &self.token {
                // Putting the raw token in the URL is safe only on a direct
                // local connection. Through a relay it would be handed to the
                // relay operator, so require E2E for remote subscriptions.
                if !self.is_local() {
                    anyhow::bail!(
                        "Refusing to send a bearer token in a relay subscription URL. \
                         End-to-end encryption is required to subscribe remotely \
                         (it is on by default for remote hubs; unset MELLOWMESH_E2E=0)."
                    );
                }
                url.query_pairs_mut().append_pair("token", token);
            }
            None
        };

        let (ws_stream, _) = tokio_tungstenite::connect_async(url.as_str()).await?;

        let decode = move |text: &str| -> anyhow::Result<Message> {
            let payload = if let Some(key) = &e2e_key {
                let envelope: Envelope = serde_json::from_str(text)?;
                String::from_utf8(open(key, CTX_STREAM, &envelope)?)?
            } else {
                text.to_string()
            };
            Ok(serde_json::from_str(&payload)?)
        };

        // Control frames are not messages: tungstenite answers pings itself,
        // so Ping/Pong are skipped; a Close frame ends the stream cleanly.
        // Binary frames are not part of the protocol and are ignored.
        let stream = ws_stream
            .take_while(|res| futures_util::future::ready(!matches!(res, Ok(WsMessage::Close(_)))))
            .filter_map(move |res| {
                let item = match res {
                    Ok(WsMessage::Text(text)) => Some(decode(&text)),
                    Ok(_) => None,
                    Err(e) => Some(Err(anyhow::Error::from(e))),
                };
                futures_util::future::ready(item)
            });
        Ok(stream)
    }

    pub async fn get_history(&self, limit: usize) -> anyhow::Result<Vec<Message>> {
        self.send(Method::GET, &history_path(limit, None), None)
            .await?
            .ensure("Get history")?
            .json()
    }

    /// The most recent `limit` messages (at most 1000) whose topic matches
    /// `pattern`, filtered server-side so the limit applies to matching
    /// messages rather than to the global history. Results are re-checked
    /// against the pattern locally, so an older daemon that ignores the
    /// `pattern` parameter still yields only matching messages.
    pub async fn get_history_matching(
        &self,
        pattern: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<Message>> {
        let messages: Vec<Message> = self
            .send(Method::GET, &history_path(limit, Some(pattern)), None)
            .await?
            .ensure("Get history")?
            .json()?;
        Ok(messages
            .into_iter()
            .filter(|m| mellowmesh_core::topic::match_topic(pattern, &m.topic))
            .take(limit)
            .collect())
    }

    pub async fn search_messages(&self, query: &str) -> anyhow::Result<Vec<Message>> {
        self.send(Method::GET, &format!("/search?query={}", enc(query)), None)
            .await?
            .ensure("Search")?
            .json()
    }

    pub async fn list_topics(&self) -> anyhow::Result<Vec<String>> {
        self.send(Method::GET, "/topics", None)
            .await?
            .ensure("List topics")?
            .json()
    }

    pub async fn register_agent(&self, agent: &AgentRegistration) -> anyhow::Result<()> {
        self.send(Method::POST, "/agents", Some(serde_json::to_value(agent)?))
            .await?
            .ensure("Agent registration")?;
        Ok(())
    }

    pub async fn list_agents(&self) -> anyhow::Result<Vec<AgentRegistration>> {
        self.send(Method::GET, "/agents", None)
            .await?
            .ensure("List agents")?
            .json()
    }

    pub async fn register_named_topic(&self, name: &str, topic: &str) -> anyhow::Result<()> {
        let payload = NamedTopic {
            name: name.to_string(),
            topic: topic.to_string(),
        };
        self.send(
            Method::POST,
            "/named-topics",
            Some(serde_json::to_value(&payload)?),
        )
        .await?
        .ensure("Named topic registration")?;
        Ok(())
    }

    pub async fn list_named_topics(&self) -> anyhow::Result<Vec<NamedTopic>> {
        self.send(Method::GET, "/named-topics", None)
            .await?
            .ensure("List named topics")?
            .json()
    }

    pub async fn remove_named_topic(&self, name: &str) -> anyhow::Result<()> {
        self.send(
            Method::DELETE,
            &format!("/named-topics/{}", path_segment(name)?),
            None,
        )
        .await?
        .ensure("Remove named topic")?;
        Ok(())
    }

    pub async fn create_task(&self, task: &Task) -> anyhow::Result<()> {
        self.send(Method::POST, "/tasks", Some(serde_json::to_value(task)?))
            .await?
            .ensure("Task creation")?;
        Ok(())
    }

    pub async fn list_tasks(&self) -> anyhow::Result<Vec<Task>> {
        self.send(Method::GET, "/tasks", None)
            .await?
            .ensure("List tasks")?
            .json()
    }

    pub async fn claim_task(&self, task_id: &str, agent_id: &str) -> anyhow::Result<()> {
        self.claim_task_with_lease(task_id, agent_id, None).await
    }

    /// Claim a task with an explicit lease duration. The claim is released
    /// automatically by the daemon if the lease expires without renewal;
    /// publishing progress on `_task.<id>.progress` renews it.
    pub async fn claim_task_with_lease(
        &self,
        task_id: &str,
        agent_id: &str,
        lease_seconds: Option<u64>,
    ) -> anyhow::Result<()> {
        let mut payload = serde_json::json!({ "claimed_by": agent_id });
        if let Some(lease) = lease_seconds {
            payload["lease_seconds"] = serde_json::json!(lease);
        }
        self.send(
            Method::POST,
            &format!("/tasks/{}/claim", path_segment(task_id)?),
            Some(payload),
        )
        .await?
        .ensure("Claim task")?;
        Ok(())
    }

    pub async fn complete_task(&self, task_id: &str) -> anyhow::Result<()> {
        let path = format!("/tasks/{}/complete", path_segment(task_id)?);
        self.send(Method::POST, &path, None)
            .await?
            .ensure("Complete task")?;
        Ok(())
    }

    pub async fn create_decision(&self, decision: &Decision) -> anyhow::Result<()> {
        self.send(
            Method::POST,
            "/decisions",
            Some(serde_json::to_value(decision)?),
        )
        .await?
        .ensure("Decision creation")?;
        Ok(())
    }

    pub async fn list_decisions(&self) -> anyhow::Result<Vec<Decision>> {
        self.send(Method::GET, "/decisions", None)
            .await?
            .ensure("List decisions")?
            .json()
    }

    pub async fn respond_decision(&self, decision_id: &str, option_id: &str) -> anyhow::Result<()> {
        self.respond_decision_as(decision_id, option_id, None).await
    }

    /// Respond to a decision with an explicit `responded_by` hint. Used by
    /// interface connectors relaying a human's answer (e.g. a Telegram
    /// button tap); the daemon records it in the audit trail. Ignored when
    /// the caller is an authenticated human principal.
    pub async fn respond_decision_as(
        &self,
        decision_id: &str,
        option_id: &str,
        responded_by: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut payload = serde_json::json!({ "option_id": option_id });
        if let Some(by) = responded_by {
            payload["responded_by"] = serde_json::json!(by);
        }
        self.send(
            Method::POST,
            &format!("/decisions/{}/respond", path_segment(decision_id)?),
            Some(payload),
        )
        .await?
        .ensure("Respond to decision")?;
        Ok(())
    }

    pub async fn enable_trace(
        &self,
        target_type: &str,
        target: &str,
        level: &str,
        duration: &str,
        reason: Option<String>,
        enabled_by: &str,
    ) -> anyhow::Result<TraceSession> {
        self.send(
            Method::POST,
            "/traces",
            Some(serde_json::json!({
                "target_type": target_type,
                "target": target,
                "level": level,
                "duration": duration,
                "reason": reason,
                "enabled_by": enabled_by,
            })),
        )
        .await?
        .ensure("Enable trace")?
        .json()
    }

    pub async fn disable_trace(&self, id: &str) -> anyhow::Result<()> {
        let path = format!("/traces/{}", path_segment(id)?);
        self.send(Method::DELETE, &path, None)
            .await?
            .ensure("Disable trace")?;
        Ok(())
    }

    pub async fn list_traces(&self) -> anyhow::Result<Vec<TraceSession>> {
        self.send(Method::GET, "/traces", None)
            .await?
            .ensure("List traces")?
            .json()
    }

    pub async fn get_metrics(&self) -> anyhow::Result<serde_json::Value> {
        self.send(Method::GET, "/metrics", None)
            .await?
            .ensure("Get metrics")?
            .json()
    }

    pub async fn get_forum(&self, pattern: Option<String>) -> anyhow::Result<Vec<Message>> {
        let path = match pattern {
            Some(pat) => format!("/forum?pattern={}", enc(&pat)),
            None => "/forum".to_string(),
        };
        self.send(Method::GET, &path, None)
            .await?
            .ensure("Get forum")?
            .json()
    }

    pub async fn store_summary(&self, topic: &str, summary: &str) -> anyhow::Result<()> {
        self.send(
            Method::POST,
            "/summaries",
            Some(serde_json::json!({ "topic": topic, "summary": summary })),
        )
        .await?
        .ensure("Store summary")?;
        Ok(())
    }

    pub async fn get_context(
        &self,
        topic: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<mellowmesh_core::persistence::ContextResult> {
        let mut path = format!("/context?topic={}", enc(topic));
        if let Some(lim) = limit {
            path.push_str(&format!("&limit={lim}"));
        }
        self.send(Method::GET, &path, None)
            .await?
            .ensure("Get context")?
            .json()
    }

    pub async fn add_identity_mapping(&self, ext_id: &str, fm_id: &str) -> anyhow::Result<()> {
        self.send(
            Method::POST,
            "/identity-mappings",
            Some(serde_json::json!({ "external_id": ext_id, "mellowmesh_id": fm_id })),
        )
        .await?
        .ensure("Add identity mapping")?;
        Ok(())
    }

    pub async fn list_identity_mappings(&self) -> anyhow::Result<Vec<(String, String)>> {
        #[derive(serde::Deserialize)]
        struct MappingItem {
            external_id: String,
            mellowmesh_id: String,
        }
        let mappings: Vec<MappingItem> = self
            .send(Method::GET, "/identity-mappings", None)
            .await?
            .ensure("List identity mappings")?
            .json()?;
        Ok(mappings
            .into_iter()
            .map(|m| (m.external_id, m.mellowmesh_id))
            .collect())
    }

    pub async fn shutdown_daemon(&self) -> anyhow::Result<()> {
        self.send(Method::POST, "/shutdown", None)
            .await?
            .ensure("Shutdown")?;
        Ok(())
    }

    pub async fn list_wiki_pages(
        &self,
        wiki: &str,
        query: Option<&str>,
        doc_type: Option<&str>,
        tag: Option<&str>,
    ) -> anyhow::Result<Vec<mellowmesh_core::okf::OKFDocument>> {
        let mut params = Vec::new();
        if let Some(q) = query {
            params.push(format!("query={}", enc(q)));
        }
        if let Some(dt) = doc_type {
            params.push(format!("doc_type={}", enc(dt)));
        }
        if let Some(t) = tag {
            params.push(format!("tag={}", enc(t)));
        }
        let mut path = format!("/wiki/{}/pages", path_segment(wiki)?);
        if !params.is_empty() {
            path.push('?');
            path.push_str(&params.join("&"));
        }
        self.send(Method::GET, &path, None)
            .await?
            .ensure("List wiki pages")?
            .json()
    }

    pub async fn get_wiki_page(
        &self,
        wiki: &str,
        path: &str,
    ) -> anyhow::Result<mellowmesh_core::okf::OKFDocument> {
        let url_path = wiki_page_path(wiki, path)?;
        self.send(Method::GET, &url_path, None)
            .await?
            .ensure("Get wiki page")?
            .json()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn write_wiki_page(
        &self,
        wiki: &str,
        path: &str,
        doc_type: &str,
        title: &str,
        description: Option<&str>,
        tags: Vec<String>,
        resource: Option<&str>,
        body: &str,
    ) -> anyhow::Result<mellowmesh_core::okf::OKFDocument> {
        self.send(
            Method::POST,
            &wiki_page_path(wiki, path)?,
            Some(serde_json::json!({
                "doc_type": doc_type,
                "title": title,
                "description": description,
                "tags": tags,
                "resource": resource,
                "body": body,
            })),
        )
        .await?
        .ensure("Write wiki page")?
        .json()
    }

    pub async fn delete_wiki_page(&self, wiki: &str, path: &str) -> anyhow::Result<()> {
        let url_path = wiki_page_path(wiki, path)?;
        self.send(Method::DELETE, &url_path, None)
            .await?
            .ensure("Delete wiki page")?;
        Ok(())
    }

    pub async fn sync_wiki(&self, wiki: &str) -> anyhow::Result<()> {
        let path = format!("/wiki/{}/sync", path_segment(wiki)?);
        self.send(Method::POST, &path, None)
            .await?
            .ensure("Sync wiki")?;
        Ok(())
    }

    pub async fn get_wiki_graph(&self, wiki: &str) -> anyhow::Result<serde_json::Value> {
        let path = format!("/wiki/{}/graph", path_segment(wiki)?);
        self.send(Method::GET, &path, None)
            .await?
            .ensure("Get wiki graph")?
            .json()
    }

    pub async fn add_schema(
        &self,
        topic_pattern: &str,
        version: &str,
        schema_content: &str,
    ) -> anyhow::Result<()> {
        self.send(
            Method::POST,
            "/schemas",
            Some(serde_json::json!({
                "topic_pattern": topic_pattern,
                "version": version,
                "schema_content": schema_content,
            })),
        )
        .await?
        .ensure("Add schema")?;
        Ok(())
    }

    pub async fn list_schemas(
        &self,
    ) -> anyhow::Result<Vec<mellowmesh_core::persistence::TopicSchema>> {
        self.send(Method::GET, "/schemas", None)
            .await?
            .ensure("List schemas")?
            .json()
    }

    pub async fn set_schema_status(
        &self,
        topic_pattern: &str,
        version: &str,
        status: &str,
    ) -> anyhow::Result<()> {
        self.send(
            Method::POST,
            "/schemas/status",
            Some(serde_json::json!({
                "topic_pattern": topic_pattern,
                "version": version,
                "status": status,
            })),
        )
        .await?
        .ensure("Set schema status")?;
        Ok(())
    }

    /// Create a scoped bearer token for a principal. Returns the server
    /// response, which includes the plaintext token (shown exactly once).
    pub async fn create_token(
        &self,
        principal: &str,
        display_name: Option<&str>,
        read_scopes: Option<Vec<String>>,
        write_scopes: Option<Vec<String>>,
    ) -> anyhow::Result<serde_json::Value> {
        self.send(
            Method::POST,
            "/auth/tokens",
            Some(serde_json::json!({
                "principal": principal,
                "display_name": display_name,
                "read_scopes": read_scopes,
                "write_scopes": write_scopes,
            })),
        )
        .await?
        .ensure("Create token")?
        .json()
    }

    /// Create a full-access or scoped token with a label (e.g. the device
    /// it is for), without touching the principal's display name.
    pub async fn create_labeled_token(
        &self,
        principal: &str,
        label: &str,
        read_scopes: Vec<String>,
        write_scopes: Vec<String>,
    ) -> anyhow::Result<serde_json::Value> {
        self.send(
            Method::POST,
            "/auth/tokens",
            Some(serde_json::json!({
                "principal": principal,
                "label": label,
                "read_scopes": read_scopes,
                "write_scopes": write_scopes,
            })),
        )
        .await?
        .ensure("Create token")?
        .json()
    }

    pub async fn list_tokens(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.send(Method::GET, "/auth/tokens", None)
            .await?
            .ensure("List tokens")?
            .json()
    }

    pub async fn revoke_token(&self, id: &str) -> anyhow::Result<()> {
        let path = format!("/auth/tokens/{}", path_segment(id)?);
        self.send(Method::DELETE, &path, None)
            .await?
            .ensure("Revoke token")?;
        Ok(())
    }

    pub async fn remove_schema(&self, topic_pattern: &str, version: &str) -> anyhow::Result<()> {
        self.send(
            Method::DELETE,
            &format!(
                "/schemas?topic_pattern={}&version={}",
                enc(topic_pattern),
                enc(version)
            ),
            None,
        )
        .await?
        .ensure("Remove schema")?;
        Ok(())
    }

    // -----------------------------------------------------------------
    // Network: identity, network setting, spaces, teams, access checks
    // (contract: docs/network.md)
    // -----------------------------------------------------------------

    /// Who the hub thinks this client is (`GET /whoami`).
    pub async fn whoami(&self) -> anyhow::Result<WhoAmI> {
        self.send(Method::GET, "/whoami", None)
            .await?
            .ensure("Who am I")?
            .json()
    }

    /// The network plus the spaces and teams this client may see.
    pub async fn get_network(&self) -> anyhow::Result<NetworkView> {
        self.send(Method::GET, "/network", None)
            .await?
            .ensure("Get network")?
            .json()
    }

    /// Update the network setting (owner only). Unset fields are kept.
    pub async fn set_network(&self, patch: &NetworkPatch) -> anyhow::Result<Network> {
        self.send(Method::PUT, "/network", Some(serde_json::to_value(patch)?))
            .await?
            .ensure("Set network")?
            .json()
    }

    pub async fn list_spaces(&self) -> anyhow::Result<Vec<Space>> {
        self.send(Method::GET, "/spaces", None)
            .await?
            .ensure("List spaces")?
            .json()
    }

    /// Create a space (owner only).
    pub async fn create_space(&self, space: &Space) -> anyhow::Result<Space> {
        self.send(Method::POST, "/spaces", Some(serde_json::to_value(space)?))
            .await?
            .ensure("Create space")?
            .json()
    }

    /// Replace a space (owner only).
    pub async fn replace_space(&self, space: &Space) -> anyhow::Result<Space> {
        let path = format!("/spaces/{}", path_segment(&space.id)?);
        self.send(Method::PUT, &path, Some(serde_json::to_value(space)?))
            .await?
            .ensure("Update space")?
            .json()
    }

    pub async fn delete_space(&self, id: &str) -> anyhow::Result<()> {
        let path = format!("/spaces/{}", path_segment(id)?);
        self.send(Method::DELETE, &path, None)
            .await?
            .ensure("Remove space")?;
        Ok(())
    }

    pub async fn list_teams(&self) -> anyhow::Result<Vec<Team>> {
        self.send(Method::GET, "/teams", None)
            .await?
            .ensure("List teams")?
            .json()
    }

    /// Create a team and its closed team space `team-<id>` (owner only).
    pub async fn create_team(&self, team: &Team) -> anyhow::Result<Team> {
        self.send(Method::POST, "/teams", Some(serde_json::to_value(team)?))
            .await?
            .ensure("Create team")?
            .json()
    }

    /// Replace a team's name, description, color and members (owner only).
    pub async fn replace_team(&self, team: &Team) -> anyhow::Result<Team> {
        let path = format!("/teams/{}", path_segment(&team.id)?);
        self.send(Method::PUT, &path, Some(serde_json::to_value(team)?))
            .await?
            .ensure("Update team")?
            .json()
    }

    /// Delete a team and its team space (owner only).
    pub async fn delete_team(&self, id: &str) -> anyhow::Result<()> {
        let path = format!("/teams/{}", path_segment(id)?);
        self.send(Method::DELETE, &path, None)
            .await?
            .ensure("Remove team")?;
        Ok(())
    }

    /// Add (or change the role of) a team member; returns the team.
    pub async fn add_team_member(
        &self,
        team_id: &str,
        principal: &str,
        role: TeamRole,
    ) -> anyhow::Result<Team> {
        let path = format!("/teams/{}/members", path_segment(team_id)?);
        self.send(
            Method::POST,
            &path,
            Some(serde_json::json!({ "principal": principal, "role": role })),
        )
        .await?
        .ensure("Add team member")?
        .json()
    }

    pub async fn remove_team_member(&self, team_id: &str, principal: &str) -> anyhow::Result<()> {
        let path = format!(
            "/teams/{}/members/{}",
            path_segment(team_id)?,
            path_segment(principal)?
        );
        self.send(Method::DELETE, &path, None)
            .await?
            .ensure("Remove team member")?;
        Ok(())
    }

    /// What `principal` (or, when `None`, a visitor) may do on `topic`
    /// (owner only).
    pub async fn access_check(
        &self,
        topic: &str,
        principal: Option<&str>,
    ) -> anyhow::Result<AccessCheck> {
        let mut path = format!("/access/check?topic={}", enc(topic));
        if let Some(p) = principal {
            path.push_str(&format!("&principal={}", enc(p)));
        }
        self.send(Method::GET, &path, None)
            .await?
            .ensure("Access check")?
            .json()
    }

    // -----------------------------------------------------------------
    // Task boards
    // -----------------------------------------------------------------

    /// Update a task (`PATCH /tasks/:id`); returns the updated task.
    pub async fn update_task(&self, task_id: &str, update: &TaskUpdate) -> anyhow::Result<Task> {
        let path = format!("/tasks/{}", path_segment(task_id)?);
        self.send(Method::PATCH, &path, Some(serde_json::to_value(update)?))
            .await?
            .ensure("Update task")?
            .json()
    }

    /// The task graph: `scope` is `all`, `mine`, `agent:<principal>` or
    /// `team:<id>` (default `all`).
    pub async fn task_graph(&self, scope: Option<&str>) -> anyhow::Result<TaskGraph> {
        let path = match scope {
            Some(s) => format!("/tasks/graph?scope={}", enc(s)),
            None => "/tasks/graph".to_string(),
        };
        self.send(Method::GET, &path, None)
            .await?
            .ensure("Task graph")?
            .json()
    }

    // -----------------------------------------------------------------
    // Pairing (owner only; contract: docs/network.md "Pairing a device")
    // -----------------------------------------------------------------

    /// Start pairing a device (`POST /pair`). Without a url the hub opens
    /// its LAN listener and pairs over the LAN.
    pub async fn pair(&self, request: &PairRequest) -> anyhow::Result<PairResponse> {
        self.send(Method::POST, "/pair", Some(serde_json::to_value(request)?))
            .await?
            .ensure_api("Pairing")?
            .json()
    }

    /// Whether the device being paired has connected (`GET /pair/:id`).
    pub async fn pair_status(&self, token_id: &str) -> anyhow::Result<PairStatus> {
        let path = format!("/pair/{}", path_segment(token_id)?);
        self.send(Method::GET, &path, None)
            .await?
            .ensure_api("Pairing status")?
            .json()
    }

    /// Paired and pending devices, newest first (`GET /devices`).
    pub async fn list_devices(&self) -> anyhow::Result<Vec<DeviceInfo>> {
        self.send(Method::GET, "/devices", None)
            .await?
            .ensure_api("List devices")?
            .json()
    }

    /// Revoke a device's token (`DELETE /devices/:id`).
    pub async fn remove_device(&self, token_id: &str) -> anyhow::Result<()> {
        let path = format!("/devices/{}", path_segment(token_id)?);
        self.send(Method::DELETE, &path, None)
            .await?
            .ensure_api("Remove device")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segment_encodes_reserved_characters() {
        assert_eq!(path_segment("task_01abc").unwrap(), "task_01abc");
        assert_eq!(path_segment("a-b.c_d~e").unwrap(), "a-b.c_d~e");
        assert_eq!(path_segment("../shutdown?").unwrap(), "..%2Fshutdown%3F");
        assert_eq!(path_segment("a/b").unwrap(), "a%2Fb");
        assert_eq!(path_segment("Mario Galaxy").unwrap(), "Mario%20Galaxy");
        assert_eq!(path_segment("x#y%z").unwrap(), "x%23y%25z");
        assert_eq!(path_segment("é").unwrap(), "%C3%A9");
    }

    #[test]
    fn path_segment_rejects_dot_segments_and_empty() {
        assert!(path_segment("..").is_err());
        assert!(path_segment(".").is_err());
        assert!(path_segment("").is_err());
        // Dots inside a longer segment are harmless.
        assert_eq!(path_segment("...").unwrap(), "...");
    }

    #[test]
    fn encoded_ids_cannot_escape_their_route() {
        // A crafted id must stay a single segment under its route after URL
        // resolution.
        let path = format!("/tasks/{}/complete", path_segment("../shutdown?").unwrap());
        let url = url::Url::parse(&format!("http://127.0.0.1:40000{path}")).unwrap();
        assert_eq!(url.path(), "/tasks/..%2Fshutdown%3F/complete");
        assert_eq!(url.query(), None);
    }

    #[test]
    fn wiki_paths_encode_each_segment() {
        assert_eq!(
            wiki_page_path("default", "procedures/deploy.md").unwrap(),
            "/wiki/default/pages/procedures/deploy.md"
        );
        assert_eq!(
            wiki_page_path("my wiki", "a b/c?d.md").unwrap(),
            "/wiki/my%20wiki/pages/a%20b/c%3Fd.md"
        );
        assert!(wiki_page_path("default", "../../shutdown").is_err());
        assert!(wiki_page_path("default", "a/./b").is_err());
        assert!(wiki_page_path("default", "a//b").is_err());
        assert!(wiki_page_path("default", "/abs").is_err());
        assert!(wiki_page_path("..", "page.md").is_err());
        assert!(wiki_page_path("a/b", "page.md")
            .unwrap()
            .starts_with("/wiki/a%2Fb/pages/"));
    }

    #[test]
    fn is_local_parses_the_host() {
        assert!(url_is_local("http://127.0.0.1:40000"));
        assert!(url_is_local("http://localhost:40000"));
        assert!(url_is_local("http://LOCALHOST"));
        assert!(url_is_local("http://[::1]:40000"));
        assert!(url_is_local("https://127.0.0.1"));
        assert!(!url_is_local("http://127.0.0.1@evil.com"));
        assert!(!url_is_local("http://127.0.0.1:40000@evil.com"));
        assert!(!url_is_local("http://localhost.evil.com"));
        assert!(!url_is_local("http://127.0.0.1.evil.com"));
        assert!(!url_is_local("https://relay.example.com/hub/abc"));
        assert!(!url_is_local("not a url"));
    }

    #[test]
    fn history_path_encodes_pattern_and_clamps_limit() {
        assert_eq!(history_path(20, None), "/history?limit=20");
        assert_eq!(
            history_path(20, Some("_project.**")),
            "/history?limit=20&pattern=_project.**"
        );
        assert_eq!(
            history_path(5, Some("_forum.games.mario galaxy&x=1")),
            "/history?limit=5&pattern=_forum.games.mario+galaxy%26x%3D1"
        );
        assert_eq!(history_path(50_000, None), "/history?limit=1000");
    }

    #[test]
    fn e2e_switch_parsing() {
        assert_eq!(parse_e2e_switch("1"), Some(true));
        assert_eq!(parse_e2e_switch("TRUE"), Some(true));
        assert_eq!(parse_e2e_switch("yes"), Some(true));
        assert_eq!(parse_e2e_switch("0"), Some(false));
        assert_eq!(parse_e2e_switch("false"), Some(false));
        assert_eq!(parse_e2e_switch("No"), Some(false));
        assert_eq!(parse_e2e_switch(""), None);
        assert_eq!(parse_e2e_switch("maybe"), None);
    }

    fn client(base: &str, token: Option<&str>, e2e: Option<bool>) -> MellowMeshClient {
        MellowMeshClient {
            base_url: base.to_string(),
            token: token.map(str::to_string),
            e2e,
        }
    }

    #[test]
    fn e2e_defaults_on_for_remote_hubs_with_a_token() {
        // Remote + token: on by default.
        assert!(client("https://relay.example.com/hub/x", Some("t"), None).e2e_enabled());
        // Remote without token: off, and not an error (nothing was requested).
        let c = client("https://relay.example.com/hub/x", None, None);
        assert!(!c.e2e_enabled());
        assert!(c.ensure_e2e_consistent().is_ok());
        // Local + token: off by default.
        assert!(!client("http://127.0.0.1:40000", Some("t"), None).e2e_enabled());
        // Explicit opt-out wins for remote hubs.
        assert!(!client("https://relay.example.com/hub/x", Some("t"), Some(false)).e2e_enabled());
        // Explicit opt-in works locally.
        assert!(client("http://127.0.0.1:40000", Some("t"), Some(true)).e2e_enabled());
        // Explicitly requested without a token is a hard error.
        assert!(client("http://127.0.0.1:40000", None, Some(true))
            .ensure_e2e_consistent()
            .is_err());
        // Builder overrides.
        let c = MellowMeshClient::loopback(1)
            .with_base_url("https://relay.example.com/hub/x")
            .with_token("t");
        assert!(!c.e2e_enabled(), "loopback() pins E2E off");
        assert!(c.with_e2e(true).e2e_enabled());
    }
}
