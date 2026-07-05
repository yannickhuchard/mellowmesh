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
        .route("/", get(ui_handler))
        .route("/ui", get(ui_handler))
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
    State(state): State<AppState>,
) -> impl IntoResponse {
    let pattern = params.pattern.unwrap_or_else(|| "**".to_string());
    let case_insensitive = params.case_insensitive.unwrap_or(false);
    ws.on_upgrade(move |socket| handle_socket(socket, pattern, case_insensitive, state))
}

async fn handle_socket(
    socket: WebSocket,
    pattern: String,
    case_insensitive: bool,
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
