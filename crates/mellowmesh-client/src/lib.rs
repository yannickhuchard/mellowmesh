pub mod autostart;

use futures_util::Stream;
use futures_util::StreamExt;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use mellowmesh_core::task::Task;
use mellowmesh_core::telemetry::TraceSession;
pub use mellowmesh_core::topic::NamedTopic;

#[derive(Clone)]
pub struct MellowMeshClient {
    base_url: String,
    port: u16,
}

impl MellowMeshClient {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            port,
        }
    }

    pub async fn connect() -> anyhow::Result<Self> {
        let port = 40000;
        if !autostart::is_daemon_running(port) {
            autostart::spawn_daemon(port)?;
        }
        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}"),
            port,
        })
    }

    pub async fn connect_with_port(port: u16) -> anyhow::Result<Self> {
        if !autostart::is_daemon_running(port) {
            autostart::spawn_daemon(port)?;
        }
        Ok(Self {
            base_url: format!("http://127.0.0.1:{port}"),
            port,
        })
    }

    pub async fn publish(&self, msg: &Message) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/publish", self.base_url))
            .json(msg)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Publish failed: {}", resp.text().await?));
        }
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
        let mut url = url::Url::parse(&format!("ws://127.0.0.1:{}/ws", self.port))?;
        url.query_pairs_mut().append_pair("pattern", pattern);
        if case_insensitive {
            url.query_pairs_mut()
                .append_pair("case_insensitive", "true");
        }

        let (ws_stream, _) = tokio_tungstenite::connect_async(url.as_str()).await?;

        let stream = ws_stream.map(|msg_res| match msg_res {
            Ok(msg) => {
                if msg.is_text() {
                    let text = msg.to_text().unwrap();
                    let m: Message = serde_json::from_str(text)?;
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
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/history", self.base_url))
            .query(&[("limit", limit)])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Get history failed: {}",
                resp.text().await?
            ));
        }
        let history: Vec<Message> = resp.json().await?;
        Ok(history)
    }

    pub async fn search_messages(&self, query: &str) -> anyhow::Result<Vec<Message>> {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/search", self.base_url))
            .query(&[("query", query)])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Search failed: {}", resp.text().await?));
        }
        let msgs: Vec<Message> = resp.json().await?;
        Ok(msgs)
    }

    pub async fn list_topics(&self) -> anyhow::Result<Vec<String>> {
        let resp = reqwest::get(&format!("{}/topics", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List topics failed: {}",
                resp.text().await?
            ));
        }
        let topics: Vec<String> = resp.json().await?;
        Ok(topics)
    }

    pub async fn register_agent(&self, agent: &AgentRegistration) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/agents", self.base_url))
            .json(agent)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Agent registration failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn list_agents(&self) -> anyhow::Result<Vec<AgentRegistration>> {
        let resp = reqwest::get(&format!("{}/agents", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List agents failed: {}",
                resp.text().await?
            ));
        }
        let agents: Vec<AgentRegistration> = resp.json().await?;
        Ok(agents)
    }

    pub async fn register_named_topic(&self, name: &str, topic: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let payload = NamedTopic {
            name: name.to_string(),
            topic: topic.to_string(),
        };
        let resp = client
            .post(format!("{}/named-topics", self.base_url))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Named topic registration failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn list_named_topics(&self) -> anyhow::Result<Vec<NamedTopic>> {
        let resp = reqwest::get(&format!("{}/named-topics", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List named topics failed: {}",
                resp.text().await?
            ));
        }
        let topics: Vec<NamedTopic> = resp.json().await?;
        Ok(topics)
    }

    pub async fn remove_named_topic(&self, name: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let encoded_name: String = url::form_urlencoded::byte_serialize(name.as_bytes()).collect();
        let resp = client
            .delete(format!("{}/named-topics/{}", self.base_url, encoded_name))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Remove named topic failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn create_task(&self, task: &Task) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/tasks", self.base_url))
            .json(task)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Task creation failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn list_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let resp = reqwest::get(&format!("{}/tasks", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("List tasks failed: {}", resp.text().await?));
        }
        let tasks: Vec<Task> = resp.json().await?;
        Ok(tasks)
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
        let client = reqwest::Client::new();
        let mut payload = serde_json::json!({ "claimed_by": agent_id });
        if let Some(lease) = lease_seconds {
            payload["lease_seconds"] = serde_json::json!(lease);
        }
        let resp = client
            .post(format!("{}/tasks/{}/claim", self.base_url, task_id))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Claim task failed: {}", resp.text().await?));
        }
        Ok(())
    }

    pub async fn complete_task(&self, task_id: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/tasks/{}/complete", self.base_url, task_id))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Complete task failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn create_decision(&self, decision: &Decision) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/decisions", self.base_url))
            .json(decision)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Decision creation failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn list_decisions(&self) -> anyhow::Result<Vec<Decision>> {
        let resp = reqwest::get(&format!("{}/decisions", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List decisions failed: {}",
                resp.text().await?
            ));
        }
        let decisions: Vec<Decision> = resp.json().await?;
        Ok(decisions)
    }

    pub async fn respond_decision(&self, decision_id: &str, option_id: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!(
                "{}/decisions/{}/respond",
                self.base_url, decision_id
            ))
            .json(&serde_json::json!({ "option_id": option_id }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Respond to decision failed: {}",
                resp.text().await?
            ));
        }
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
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/traces", self.base_url))
            .json(&serde_json::json!({
                "target_type": target_type,
                "target": target,
                "level": level,
                "duration": duration,
                "reason": reason,
                "enabled_by": enabled_by,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Enable trace failed: {}",
                resp.text().await?
            ));
        }
        let ts = resp.json().await?;
        Ok(ts)
    }

    pub async fn disable_trace(&self, id: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .delete(format!("{}/traces/{}", self.base_url, id))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Disable trace failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn list_traces(&self) -> anyhow::Result<Vec<TraceSession>> {
        let resp = reqwest::get(&format!("{}/traces", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List traces failed: {}",
                resp.text().await?
            ));
        }
        let sessions = resp.json().await?;
        Ok(sessions)
    }

    pub async fn get_metrics(&self) -> anyhow::Result<serde_json::Value> {
        let resp = reqwest::get(&format!("{}/metrics", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Get metrics failed: {}",
                resp.text().await?
            ));
        }
        let metrics = resp.json().await?;
        Ok(metrics)
    }

    pub async fn get_forum(&self, pattern: Option<String>) -> anyhow::Result<Vec<Message>> {
        let client = reqwest::Client::new();
        let mut req = client.get(format!("{}/forum", self.base_url));
        if let Some(pat) = pattern {
            req = req.query(&[("pattern", pat)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Get forum failed: {}", resp.text().await?));
        }
        let msgs = resp.json().await?;
        Ok(msgs)
    }

    pub async fn store_summary(&self, topic: &str, summary: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/summaries", self.base_url))
            .json(&serde_json::json!({ "topic": topic, "summary": summary }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Store summary failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn get_context(
        &self,
        topic: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<mellowmesh_core::persistence::ContextResult> {
        let client = reqwest::Client::new();
        let mut req = client
            .get(format!("{}/context", self.base_url))
            .query(&[("topic", topic)]);
        if let Some(lim) = limit {
            req = req.query(&[("limit", lim)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Get context failed: {}",
                resp.text().await?
            ));
        }
        let res = resp.json().await?;
        Ok(res)
    }

    pub async fn add_identity_mapping(&self, ext_id: &str, fm_id: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/identity-mappings", self.base_url))
            .json(&serde_json::json!({ "external_id": ext_id, "mellowmesh_id": fm_id }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Add identity mapping failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn list_identity_mappings(&self) -> anyhow::Result<Vec<(String, String)>> {
        #[derive(serde::Deserialize)]
        struct MappingItem {
            external_id: String,
            mellowmesh_id: String,
        }
        let resp = reqwest::get(&format!("{}/identity-mappings", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List identity mappings failed: {}",
                resp.text().await?
            ));
        }
        let mappings: Vec<MappingItem> = resp.json().await?;
        Ok(mappings
            .into_iter()
            .map(|m| (m.external_id, m.mellowmesh_id))
            .collect())
    }

    pub async fn shutdown_daemon(&self) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/shutdown", self.base_url))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Shutdown failed: {}", resp.text().await?));
        }
        Ok(())
    }

    pub async fn list_wiki_pages(
        &self,
        wiki: &str,
        query: Option<&str>,
        doc_type: Option<&str>,
        tag: Option<&str>,
    ) -> anyhow::Result<Vec<mellowmesh_core::okf::OKFDocument>> {
        let client = reqwest::Client::new();
        let mut req = client.get(format!("{}/wiki/{}/pages", self.base_url, wiki));
        let mut query_params = Vec::new();
        if let Some(q) = query {
            query_params.push(("query", q.to_string()));
        }
        if let Some(dt) = doc_type {
            query_params.push(("doc_type", dt.to_string()));
        }
        if let Some(t) = tag {
            query_params.push(("tag", t.to_string()));
        }
        if !query_params.is_empty() {
            req = req.query(&query_params);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List wiki pages failed: {}",
                resp.text().await?
            ));
        }
        let docs = resp.json().await?;
        Ok(docs)
    }

    pub async fn get_wiki_page(
        &self,
        wiki: &str,
        path: &str,
    ) -> anyhow::Result<mellowmesh_core::okf::OKFDocument> {
        let resp = reqwest::get(&format!("{}/wiki/{}/pages/{}", self.base_url, wiki, path)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Get wiki page failed: {}",
                resp.text().await?
            ));
        }
        let doc = resp.json().await?;
        Ok(doc)
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
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/wiki/{}/pages/{}", self.base_url, wiki, path))
            .json(&serde_json::json!({
                "doc_type": doc_type,
                "title": title,
                "description": description,
                "tags": tags,
                "resource": resource,
                "body": body,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Write wiki page failed: {}",
                resp.text().await?
            ));
        }
        let doc = resp.json().await?;
        Ok(doc)
    }

    pub async fn delete_wiki_page(&self, wiki: &str, path: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .delete(format!("{}/wiki/{}/pages/{}", self.base_url, wiki, path))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Delete wiki page failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn sync_wiki(&self, wiki: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/wiki/{}/sync", self.base_url, wiki))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Sync wiki failed: {}", resp.text().await?));
        }
        Ok(())
    }

    pub async fn get_wiki_graph(&self, wiki: &str) -> anyhow::Result<serde_json::Value> {
        let resp = reqwest::get(&format!("{}/wiki/{}/graph", self.base_url, wiki)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Get wiki graph failed: {}",
                resp.text().await?
            ));
        }
        let graph = resp.json().await?;
        Ok(graph)
    }

    pub async fn add_schema(
        &self,
        topic_pattern: &str,
        version: &str,
        schema_content: &str,
    ) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/schemas", self.base_url))
            .json(&serde_json::json!({
                "topic_pattern": topic_pattern,
                "version": version,
                "schema_content": schema_content,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Add schema failed: {}", resp.text().await?));
        }
        Ok(())
    }

    pub async fn list_schemas(
        &self,
    ) -> anyhow::Result<Vec<mellowmesh_core::persistence::TopicSchema>> {
        let resp = reqwest::get(&format!("{}/schemas", self.base_url)).await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "List schemas failed: {}",
                resp.text().await?
            ));
        }
        let schemas = resp.json().await?;
        Ok(schemas)
    }

    pub async fn set_schema_status(
        &self,
        topic_pattern: &str,
        version: &str,
        status: &str,
    ) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/schemas/status", self.base_url))
            .json(&serde_json::json!({
                "topic_pattern": topic_pattern,
                "version": version,
                "status": status,
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Set schema status failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }

    pub async fn remove_schema(&self, topic_pattern: &str, version: &str) -> anyhow::Result<()> {
        let client = reqwest::Client::new();
        let resp = client
            .delete(format!("{}/schemas", self.base_url))
            .query(&[("topic_pattern", topic_pattern), ("version", version)])
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!(
                "Remove schema failed: {}",
                resp.text().await?
            ));
        }
        Ok(())
    }
}
