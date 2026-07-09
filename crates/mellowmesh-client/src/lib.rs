pub mod autostart;
pub mod mcp;

use futures_util::Stream;
use futures_util::StreamExt;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use mellowmesh_core::telemetry::TraceSession;
pub use mellowmesh_core::topic::NamedTopic;
use reqwest::Method;

#[derive(Clone)]
pub struct MellowMeshClient {
    base_url: String,
    token: Option<String>,
    /// Transparent end-to-end encryption: when enabled (and a token is
    /// present), every API call is sealed and sent through `/e2e/request`
    /// instead of plain HTTP — a relay in the middle sees only ciphertext.
    e2e: bool,
    http: reqwest::Client,
    /// A client with NO default Authorization header, used to POST E2E
    /// envelopes. The bearer token travels sealed *inside* the ciphertext,
    /// so it must never appear as a header the relay can read.
    plain_http: reqwest::Client,
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
            Err(anyhow::anyhow!("{context} failed: {}", self.body))
        }
    }

    fn json<T: serde::de::DeserializeOwned>(&self) -> anyhow::Result<T> {
        Ok(serde_json::from_str(&self.body)?)
    }
}

fn build_http(token: &Option<String>) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Some(t) = token {
        if let Ok(value) = reqwest::header::HeaderValue::from_str(&format!("Bearer {t}")) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap_or_default()
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

fn e2e_env_enabled() -> bool {
    matches!(
        std::env::var("MELLOWMESH_E2E").unwrap_or_default().as_str(),
        "1" | "true" | "yes"
    )
}

/// Percent-encode a query-string value.
fn enc(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

impl MellowMeshClient {
    pub fn new(port: u16) -> Self {
        // Token resolution: explicit env var wins; otherwise anonymous
        // (fine against a daemon in open mode).
        let token = std::env::var("MELLOWMESH_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty());
        let http = build_http(&token);
        let base_url = remote_base_url().unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
        Self {
            base_url,
            token,
            e2e: e2e_env_enabled(),
            http,
            plain_http: reqwest::Client::new(),
        }
    }

    /// Use an explicit bearer token instead of the `MELLOWMESH_TOKEN` env var.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self.http = build_http(&self.token);
        self
    }

    /// Point the client at an explicit base URL (e.g. a relay hub URL or a
    /// test server), bypassing the `MELLOWMESH_URL` env var.
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = url.into().trim_end_matches('/').to_string();
        self
    }

    /// Enable or disable transparent end-to-end encryption explicitly,
    /// overriding the `MELLOWMESH_E2E` env var.
    pub fn with_e2e(mut self, enabled: bool) -> Self {
        self.e2e = enabled;
        self
    }

    fn e2e_enabled(&self) -> bool {
        self.e2e && self.token.is_some()
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
        if self.e2e_enabled() {
            let (status, text) = self
                .e2e_request(method.as_str(), path_and_query, body.map(|b| b.to_string()))
                .await?;
            return Ok(ApiResponse { status, body: text });
        }
        let mut req = self
            .http
            .request(method, format!("{}{}", self.base_url, path_and_query));
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
            derive_key, derive_key_id, open, seal, Envelope, SealedRequest, SealedResponse,
        };
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("End-to-end encryption requires a bearer token"))?;
        let key = derive_key(token);
        let key_id = derive_key_id(token);

        let sealed = SealedRequest {
            ts: chrono::Utc::now().timestamp(),
            method: method.to_string(),
            path_and_query: path_and_query.to_string(),
            authorization: Some(format!("Bearer {token}")),
            body,
        };
        let envelope = seal(&key, &key_id, &serde_json::to_vec(&sealed)?)?;

        // Use the header-less client: the token is inside the ciphertext and
        // must never leak to the relay as an Authorization header.
        let resp = self
            .plain_http
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
        let opened = open(&key, &reply)?;
        let sealed_resp: SealedResponse = serde_json::from_slice(&opened)?;
        Ok((sealed_resp.status, sealed_resp.body.unwrap_or_default()))
    }

    pub async fn connect() -> anyhow::Result<Self> {
        Self::connect_with_port(40000).await
    }

    pub async fn connect_with_port(port: u16) -> anyhow::Result<Self> {
        // A remote hub URL means there is no local daemon to autostart.
        if remote_base_url().is_none() && !autostart::is_daemon_running(port) {
            autostart::spawn_daemon(port)?;
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
            derive_key, derive_key_id, open, seal, Envelope, SealedRequest,
        };

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
                method: "SUBSCRIBE".to_string(),
                path_and_query: "/ws".to_string(),
                authorization: Some(format!("Bearer {token}")),
                body: None,
            };
            let envelope = seal(&key, &key_id, &serde_json::to_vec(&proof)?)?;
            url.query_pairs_mut()
                .append_pair("e2e_kid", &envelope.key_id);
            url.query_pairs_mut()
                .append_pair("e2e_nonce", &envelope.nonce);
            url.query_pairs_mut()
                .append_pair("e2e_ct", &envelope.ciphertext);
            Some(key)
        } else {
            if let Some(token) = &self.token {
                url.query_pairs_mut().append_pair("token", token);
            }
            None
        };

        let (ws_stream, _) = tokio_tungstenite::connect_async(url.as_str()).await?;

        let stream = ws_stream.map(move |msg_res| match msg_res {
            Ok(msg) => {
                if msg.is_text() {
                    let text = msg.to_text().unwrap();
                    let payload = if let Some(key) = &e2e_key {
                        let envelope: Envelope = serde_json::from_str(text)?;
                        String::from_utf8(open(key, &envelope)?)?
                    } else {
                        text.to_string()
                    };
                    let m: Message = serde_json::from_str(&payload)?;
                    Ok(m)
                } else {
                    Err(anyhow::anyhow!("Non-text websocket message received"))
                }
            }
            Err(e) => Err(anyhow::Error::from(e)),
        });
        Ok(stream)
    }

    pub async fn get_history(&self, limit: usize) -> anyhow::Result<Vec<Message>> {
        self.send(Method::GET, &format!("/history?limit={limit}"), None)
            .await?
            .ensure("Get history")?
            .json()
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
            &format!("/named-topics/{}", enc(name)),
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
            &format!("/tasks/{task_id}/claim"),
            Some(payload),
        )
        .await?
        .ensure("Claim task")?;
        Ok(())
    }

    pub async fn complete_task(&self, task_id: &str) -> anyhow::Result<()> {
        self.send(Method::POST, &format!("/tasks/{task_id}/complete"), None)
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
            &format!("/decisions/{decision_id}/respond"),
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
        self.send(Method::DELETE, &format!("/traces/{id}"), None)
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
        let mut path = format!("/wiki/{wiki}/pages");
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
        self.send(Method::GET, &format!("/wiki/{wiki}/pages/{path}"), None)
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
            &format!("/wiki/{wiki}/pages/{path}"),
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
        self.send(Method::DELETE, &format!("/wiki/{wiki}/pages/{path}"), None)
            .await?
            .ensure("Delete wiki page")?;
        Ok(())
    }

    pub async fn sync_wiki(&self, wiki: &str) -> anyhow::Result<()> {
        self.send(Method::POST, &format!("/wiki/{wiki}/sync"), None)
            .await?
            .ensure("Sync wiki")?;
        Ok(())
    }

    pub async fn get_wiki_graph(&self, wiki: &str) -> anyhow::Result<serde_json::Value> {
        self.send(Method::GET, &format!("/wiki/{wiki}/graph"), None)
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

    pub async fn list_tokens(&self) -> anyhow::Result<Vec<serde_json::Value>> {
        self.send(Method::GET, "/auth/tokens", None)
            .await?
            .ensure("List tokens")?
            .json()
    }

    pub async fn revoke_token(&self, id: &str) -> anyhow::Result<()> {
        self.send(Method::DELETE, &format!("/auth/tokens/{id}"), None)
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
}
