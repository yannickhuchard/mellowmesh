//! Shared test harness: a real daemon on an ephemeral loopback port, plus
//! small HTTP helpers.
#![allow(dead_code)]

use mellowmesh_core::auth::{generate_token, hash_token, kind_of_uri, Principal, TokenRecord};
use mellowmesh_core::persistence::{PersistenceConfig, PersistenceMode, PersistencePolicy};
use mellowmesh_daemon::access::AccessState;
use mellowmesh_daemon::metrics::DaemonMetrics;
use mellowmesh_daemon::pipeline::PersistencePipeline;
use mellowmesh_daemon::server::create_router;
use mellowmesh_daemon::state::AppState;
use mellowmesh_daemon::subscription::SubscriptionRegistry;
use mellowmesh_daemon::trace_mgr::TraceSessionManager;
use mellowmesh_store::Store;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

pub const OWNER: &str = "human://owner";
pub const PUBLIC_HOST: &str = "mesh.example.test";

pub struct Hub {
    pub base: String,
    pub port: u16,
    pub store: Store,
    pub marker: String,
    pub http: reqwest::Client,
    pub state: AppState,
}

/// Serve a real daemon on an ephemeral loopback port.
pub async fn spawn_hub(require_auth: bool) -> Hub {
    spawn_hub_with(require_auth, Default::default()).await
}

/// Like [`spawn_hub`], with explicit pairing settings (device-token
/// lifetime, LAN address source).
pub async fn spawn_hub_with(
    require_auth: bool,
    pairing: mellowmesh_daemon::pairing::PairingState,
) -> Hub {
    let store = Store::new_in_memory().unwrap();
    let metrics = Arc::new(DaemonMetrics::default());
    let pipeline = Arc::new(PersistencePipeline::new(store.clone(), metrics.clone()));
    pipeline.start();
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let access = Arc::new(AccessState::new(vec![PUBLIC_HOST.to_string()]).with_pairing(pairing));
    access
        .pairing
        .set_primary_listen(std::net::IpAddr::from([127, 0, 0, 1]));
    let state = AppState {
        store: store.clone(),
        registry: SubscriptionRegistry::new(metrics.clone()),
        trace_mgr: Arc::new(TraceSessionManager::new(store.clone(), metrics.clone())),
        metrics,
        pipeline,
        policy_config: Arc::new(PersistenceConfig {
            default: PersistencePolicy {
                mode: PersistenceMode::Queryable,
                retention: "7d".to_string(),
                max_message_size: None,
                sync: true,
            },
            rules: vec![],
        }),
        wikis: Arc::new(std::collections::HashMap::new()),
        node_id: "node://test".to_string(),
        shutdown_trigger: Arc::new(tokio::sync::Notify::new()),
        require_auth,
        owner: OWNER.to_string(),
        port,
        http: mellowmesh_daemon::state::loopback_http_client(),
        caches: Arc::new(mellowmesh_daemon::cache::Caches::default()),
        access: access.clone(),
    };
    let app = create_router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    Hub {
        base: format!("http://127.0.0.1:{port}"),
        port,
        store,
        marker: access.remote_marker.clone(),
        http: reqwest::Client::new(),
        state,
    }
}

impl Hub {
    pub fn token(&self, principal: &str, read: &[&str], write: &[&str]) -> String {
        let token = generate_token();
        self.store
            .upsert_principal(&Principal {
                id: principal.to_string(),
                kind: kind_of_uri(principal).to_string(),
                display_name: None,
                created_at: chrono::Utc::now(),
            })
            .unwrap();
        self.store
            .insert_token(&TokenRecord {
                id: format!("tok_{}", ulid::Ulid::new().to_string().to_lowercase()),
                principal: principal.to_string(),
                token_hash: hash_token(&token),
                read_scopes: read.iter().map(|s| s.to_string()).collect(),
                write_scopes: write.iter().map(|s| s.to_string()).collect(),
                created_at: chrono::Utc::now(),
                revoked: false,
            })
            .unwrap();
        token
    }

    pub fn req(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        remote: bool,
    ) -> reqwest::RequestBuilder {
        let mut r = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        if remote {
            r = r.header("Host", PUBLIC_HOST);
        }
        r
    }

    pub async fn get(&self, path: &str, token: Option<&str>, remote: bool) -> (u16, Value) {
        let r = self
            .req(reqwest::Method::GET, path, token, remote)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        let body = r.text().await.unwrap();
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    pub async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        token: Option<&str>,
        remote: bool,
        body: Value,
    ) -> (u16, Value) {
        let r = self
            .req(method, path, token, remote)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        let text = r.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn publish(&self, topic: &str, token: Option<&str>, remote: bool) -> (u16, Value) {
        self.send(
            reqwest::Method::POST,
            "/publish",
            token,
            remote,
            json!({
                "id": "", "topic": topic, "from": "human://forged",
                "timestamp": chrono::Utc::now(), "content_type": "text/plain",
                "body": format!("hello {topic}")
            }),
        )
        .await
    }
}

pub fn topics_of(v: &Value) -> Vec<String> {
    let mut t: Vec<String> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["topic"].as_str().unwrap().to_string())
        .collect();
    t.sort();
    t
}
