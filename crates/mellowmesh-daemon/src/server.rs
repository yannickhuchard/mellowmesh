use crate::handlers;
use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use ulid::Ulid;

#[derive(Deserialize)]
pub struct WsParams {
    pattern: Option<String>,
    case_insensitive: Option<bool>,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/publish", post(handlers::message::publish_message))
        .route("/history", get(handlers::message::get_history))
        .route("/search", get(handlers::message::search_messages))
        .route("/topics", get(handlers::message::list_topics))
        .route(
            "/agents",
            post(handlers::agent::register_agent).get(handlers::agent::list_agents),
        )
        .route(
            "/named-topics",
            post(handlers::named_topic::register_named_topic)
                .get(handlers::named_topic::list_named_topics),
        )
        .route(
            "/named-topics/:name",
            delete(handlers::named_topic::remove_named_topic),
        )
        .route(
            "/tasks",
            post(handlers::task::create_task).get(handlers::task::list_tasks),
        )
        .route("/tasks/:id/claim", post(handlers::task::claim_task))
        .route("/tasks/:id/complete", post(handlers::task::complete_task))
        .route(
            "/decisions",
            post(handlers::decision::create_decision).get(handlers::decision::list_decisions),
        )
        .route(
            "/decisions/:id/respond",
            post(handlers::decision::respond_decision),
        )
        .route(
            "/identity-mappings",
            post(handlers::identity::create_mapping).get(handlers::identity::list_mappings),
        )
        .route(
            "/schemas",
            post(handlers::schema::add_schema)
                .get(handlers::schema::list_schemas)
                .delete(handlers::schema::remove_schema),
        )
        .route("/schemas/status", post(handlers::schema::set_schema_status))
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .route("/metrics", get(handlers::metrics::get_metrics))
        .route(
            "/traces",
            post(handlers::trace::enable_trace).get(handlers::trace::list_traces),
        )
        .route("/traces/:id", delete(handlers::trace::disable_trace))
        .route("/forum", get(handlers::message::get_forum))
        .route("/summaries", post(handlers::summary::store_summary))
        .route("/context", get(handlers::summary::get_context))
        .route(
            "/wiki/:wiki/pages",
            get(handlers::wiki::list_or_search_pages),
        )
        .route(
            "/wiki/:wiki/pages/*path",
            get(handlers::wiki::get_page)
                .post(handlers::wiki::write_page)
                .delete(handlers::wiki::delete_page),
        )
        .route("/wiki/:wiki/sync", post(handlers::wiki::sync_wiki_endpoint))
        .route("/wiki/:wiki/graph", get(handlers::wiki::get_wiki_graph))
        .route("/shutdown", post(shutdown_handler))
        .route("/mcp", post(handlers::mcp::handle_mcp))
        .route(
            "/auth/tokens",
            post(crate::auth::create_token).get(crate::auth::list_tokens),
        )
        .route("/auth/tokens/:id", delete(crate::auth::revoke_token))
        .route("/", get(ui_handler))
        .route("/ui", get(ui_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::auth_middleware,
        ))
        .with_state(state)
}

async fn health_handler() -> &'static str {
    "OK"
}

async fn ui_handler() -> impl IntoResponse {
    axum::response::Html(include_str!("ui.html"))
}

async fn shutdown_handler(State(state): State<AppState>) -> impl IntoResponse {
    state.shutdown_trigger.notify_one();
    axum::response::Json(serde_json::json!({ "status": "shutting down" }))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let pattern = params.pattern.unwrap_or_else(|| "**".to_string());
    let case_insensitive = params.case_insensitive.unwrap_or(false);
    // Read scopes of the authenticated principal, if any: deliveries are
    // filtered message-by-message against these patterns.
    let read_scopes = ctx.principal.map(|p| p.read_scopes);
    ws.on_upgrade(move |socket| {
        handle_socket(socket, pattern, case_insensitive, read_scopes, state)
    })
}

async fn handle_socket(
    socket: WebSocket,
    pattern: String,
    case_insensitive: bool,
    read_scopes: Option<Vec<String>>,
    state: AppState,
) {
    let conn_id = format!("conn_{}", Ulid::new().to_string().to_lowercase());
    let queue = Arc::new(Mutex::new(VecDeque::new()));
    let notify = Arc::new(Notify::new());
    let capacity = 1000;
    let overflow_policy = mellowmesh_core::persistence::OverflowPolicy::DisconnectSlowSubscriber;

    state.registry.add(
        conn_id.clone(),
        pattern,
        queue.clone(),
        capacity,
        notify.clone(),
        overflow_policy,
        case_insensitive,
    );

    let (mut ws_sender, mut ws_receiver) = socket.split();

    let queue_clone = queue.clone();
    let notify_clone = notify.clone();
    let mut send_task = tokio::spawn(async move {
        loop {
            let opt_msg = {
                let mut q = queue_clone.lock().unwrap();
                q.pop_front()
            };
            if let Some(msg) = opt_msg {
                if let Some(ref scopes) = read_scopes {
                    if !mellowmesh_core::auth::scopes_allow(scopes, &msg.topic) {
                        continue;
                    }
                }
                if let Ok(text) = serde_json::to_string(&msg) {
                    if ws_sender.send(WsMessage::Text(text)).await.is_err() {
                        break;
                    }
                }
            } else {
                notify_clone.notified().await;
            }
        }
    });

    let registry_clone = state.registry.clone();
    let conn_id_clone2 = conn_id.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            if let WsMessage::Close(_) = msg {
                break;
            }
        }
        registry_clone.remove(&conn_id_clone2);
    });

    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
        }
        _ = (&mut recv_task) => {
            send_task.abort();
        }
    }

    state.registry.remove(&conn_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::DaemonMetrics;
    use crate::pipeline::PersistencePipeline;
    use crate::subscription::SubscriptionRegistry;
    use crate::trace_mgr::TraceSessionManager;
    use mellowmesh_client::MellowMeshClient;
    use mellowmesh_core::persistence::{PersistenceConfig, PersistenceMode, PersistencePolicy};
    use mellowmesh_store::Store;
    use std::net::SocketAddr;

    fn test_state(store: Store, require_auth: bool) -> AppState {
        let metrics = Arc::new(DaemonMetrics::default());
        let pipeline = Arc::new(PersistencePipeline::new(store.clone(), metrics.clone()));
        pipeline.start();
        let trace_mgr = Arc::new(TraceSessionManager::new(store.clone(), metrics.clone()));
        let registry = SubscriptionRegistry::new(metrics.clone());
        let policy_config = Arc::new(PersistenceConfig {
            default: PersistencePolicy {
                mode: PersistenceMode::Queryable,
                retention: "7d".to_string(),
                max_message_size: None,
                sync: false,
            },
            rules: vec![],
        });
        AppState {
            store,
            registry,
            metrics,
            pipeline,
            trace_mgr,
            policy_config,
            wikis: Arc::new(std::collections::HashMap::new()),
            node_id: "test-node".to_string(),
            shutdown_trigger: Arc::new(tokio::sync::Notify::new()),
            require_auth,
            owner: "human://test".to_string(),
            port: 0,
        }
    }

    #[tokio::test]
    async fn test_http_mcp_endpoint() {
        let store = Store::new_in_memory().unwrap();
        let port = 40012;
        let mut state = test_state(store, false);
        state.port = port; // loopback dispatch for tools/call
        let app = create_router(state);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let base = format!("http://127.0.0.1:{port}");
        let http = reqwest::Client::new();

        // initialize
        let resp: serde_json::Value = http
            .post(format!("{base}/mcp"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "mellowmesh-mcp");

        // tools/list exposes the full toolset
        let resp: serde_json::Value = http
            .post(format!("{base}/mcp"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/list"
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 20);

        // tools/call round-trips through the daemon's own API
        let resp: serde_json::Value = http
            .post(format!("{base}/mcp"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": {
                    "name": "create_task",
                    "arguments": {
                        "title": "MCP-created task",
                        "topics": ["_task.mcp"],
                        "capabilities": ["demo"]
                    }
                }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(resp["result"]["isError"], false);

        let resp: serde_json::Value = http
            .post(format!("{base}/mcp"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "id": 4, "method": "tools/call",
                "params": { "name": "list_tasks", "arguments": {} }
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("MCP-created task"));

        // notifications are acknowledged without a body
        let resp = http
            .post(format!("{base}/mcp"))
            .json(&serde_json::json!({
                "jsonrpc": "2.0", "method": "notifications/initialized"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 202);
    }

    #[tokio::test]
    async fn test_require_auth_enforcement() {
        use mellowmesh_core::auth::{generate_token, hash_token, Principal, TokenRecord};

        let store = Store::new_in_memory().unwrap();

        // Seed a scoped agent token and a human token directly in the store.
        let agent_token = generate_token();
        store
            .upsert_principal(&Principal {
                id: "agent://test/coder".to_string(),
                kind: "agent".to_string(),
                display_name: None,
                created_at: chrono::Utc::now(),
            })
            .unwrap();
        store
            .insert_token(&TokenRecord {
                id: "tok_agent".to_string(),
                principal: "agent://test/coder".to_string(),
                token_hash: hash_token(&agent_token),
                read_scopes: vec!["_agent.coder.**".to_string()],
                write_scopes: vec!["_agent.coder.**".to_string()],
                created_at: chrono::Utc::now(),
                revoked: false,
            })
            .unwrap();

        let state = test_state(store, true);
        let app = create_router(state);
        let port = 40011;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let base = format!("http://127.0.0.1:{port}");
        let http = reqwest::Client::new();

        // No token → 401 (except open endpoints)
        let resp = http.get(format!("{base}/tasks")).send().await.unwrap();
        assert_eq!(resp.status(), 401);
        let resp = http.get(format!("{base}/health")).send().await.unwrap();
        assert_eq!(resp.status(), 200);

        // Wrong token → 401
        let resp = http
            .get(format!("{base}/tasks"))
            .bearer_auth("mm_bogus")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);

        // Valid token → 200
        let resp = http
            .get(format!("{base}/tasks"))
            .bearer_auth(&agent_token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // Publishing outside the token's write scope → 403
        let msg = serde_json::json!({
            "id": "", "topic": "_forum.general", "from": "agent://test/coder",
            "timestamp": chrono::Utc::now(), "content_type": "text/plain",
            "body": "should be rejected"
        });
        let resp = http
            .post(format!("{base}/publish"))
            .bearer_auth(&agent_token)
            .json(&msg)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403);

        // Publishing inside the scope → 200
        let msg = serde_json::json!({
            "id": "", "topic": "_agent.coder.status", "from": "agent://test/coder",
            "timestamp": chrono::Utc::now(), "content_type": "text/plain",
            "body": "allowed"
        });
        let resp = http
            .post(format!("{base}/publish"))
            .bearer_auth(&agent_token)
            .json(&msg)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // Agents may not respond to decisions → 403
        let resp = http
            .post(format!("{base}/decisions/dec_x/respond"))
            .bearer_auth(&agent_token)
            .json(&serde_json::json!({ "option_id": "yes" }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403);
    }

    #[tokio::test]
    async fn test_identity_mapping_rest_api() {
        let store = Store::new_in_memory().unwrap();
        let metrics = Arc::new(DaemonMetrics::default());
        let pipeline = Arc::new(PersistencePipeline::new(store.clone(), metrics.clone()));
        pipeline.start();
        let trace_mgr = Arc::new(TraceSessionManager::new(store.clone(), metrics.clone()));
        let registry = SubscriptionRegistry::new(metrics.clone());
        let default_policy = PersistencePolicy {
            mode: PersistenceMode::Ephemeral,
            retention: "7d".to_string(),
            max_message_size: None,
            sync: false,
        };
        let policy_config = Arc::new(PersistenceConfig {
            default: default_policy,
            rules: vec![],
        });

        let state = AppState {
            store,
            registry,
            metrics,
            pipeline,
            trace_mgr,
            policy_config,
            wikis: Arc::new(std::collections::HashMap::new()),
            node_id: "test-node".to_string(),
            shutdown_trigger: Arc::new(tokio::sync::Notify::new()),
            require_auth: false,
            owner: "human://test".to_string(),
            port: 0,
        };

        let app = create_router(state);
        let port = 40009;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give a short time for server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = MellowMeshClient::new(port);

        // Verify initially empty
        let initial_mappings = client.list_identity_mappings().await.unwrap();
        assert!(initial_mappings.is_empty());

        // Add mapping
        client
            .add_identity_mapping("discord://12345", "human://yannick")
            .await
            .unwrap();

        // Verify populated
        let mappings = client.list_identity_mappings().await.unwrap();
        assert_eq!(mappings.len(), 1);
        assert_eq!(
            mappings[0],
            ("discord://12345".to_string(), "human://yannick".to_string())
        );
    }

    #[tokio::test]
    async fn test_case_insensitive_subscription() {
        let store = Store::new_in_memory().unwrap();
        let metrics = Arc::new(DaemonMetrics::default());
        let pipeline = Arc::new(PersistencePipeline::new(store.clone(), metrics.clone()));
        pipeline.start();
        let trace_mgr = Arc::new(TraceSessionManager::new(store.clone(), metrics.clone()));
        let registry = SubscriptionRegistry::new(metrics.clone());
        let default_policy = PersistencePolicy {
            mode: PersistenceMode::Ephemeral,
            retention: "7d".to_string(),
            max_message_size: None,
            sync: false,
        };
        let policy_config = Arc::new(PersistenceConfig {
            default: default_policy,
            rules: vec![],
        });

        let state = AppState {
            store,
            registry: registry.clone(),
            metrics,
            pipeline,
            trace_mgr,
            policy_config,
            wikis: Arc::new(std::collections::HashMap::new()),
            node_id: "test-node".to_string(),
            shutdown_trigger: Arc::new(tokio::sync::Notify::new()),
            require_auth: false,
            owner: "human://test".to_string(),
            port: 0,
        };

        let app = create_router(state);
        let port = 40010;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        // Give a short time for server to start
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = MellowMeshClient::new(port);

        // Subscribe to a topic with case-insensitivity enabled
        let mut stream = client.subscribe_with_options("NEWS.>", true).await.unwrap();

        // Publish a message with a lowercase topic
        let msg = mellowmesh_core::message::Message {
            id: "msg1".to_string(),
            topic: "news.french.technology".to_string(),
            from: "test".to_string(),
            owner: None,
            timestamp: chrono::Utc::now(),
            content_type: "text/plain".to_string(),
            body: "hello".to_string(),
            headers: None,
            payload: None,
            parent_id: None,
        };
        client.publish(&msg).await.unwrap();

        // Receive the message
        let received = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            futures_util::StreamExt::next(&mut stream),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();

        assert_eq!(received.topic, "news.french.technology");
    }
}
