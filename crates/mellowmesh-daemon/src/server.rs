use crate::handlers;
use crate::state::AppState;
use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
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
        .route("/e2e/request", post(handlers::e2e::handle_e2e_request))
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
        // Outermost: Host/Origin enforcement runs before authentication.
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::guard::host_origin_guard,
        ))
        .with_state(state)
}

/// How often a live subscription re-validates the token it authenticated
/// with; a revoked token closes the socket.
const WS_REVALIDATE_EVERY: std::time::Duration = std::time::Duration::from_secs(30);
/// Per-connection delivery queue bound; a subscriber that falls this far
/// behind is evicted and its socket closed.
const WS_QUEUE_CAPACITY: usize = 1000;

async fn health_handler() -> &'static str {
    "OK"
}

async fn ui_handler() -> impl IntoResponse {
    axum::response::Html(include_str!("ui.html"))
}

async fn shutdown_handler(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
) -> impl IntoResponse {
    // Shutting down the daemon is an owner-only action. A scoped agent or
    // interface token must not be able to take the whole fabric down.
    if !crate::auth::is_admin(&state, &ctx) {
        return (
            StatusCode::FORBIDDEN,
            axum::response::Json(
                serde_json::json!({ "error": "Only the owner may shut down the daemon" }),
            ),
        );
    }
    state.shutdown_trigger.notify_one();
    (
        StatusCode::OK,
        axum::response::Json(serde_json::json!({ "status": "shutting down" })),
    )
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<WsParams>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    axum::Extension(stream_e2e): axum::Extension<crate::auth::StreamE2e>,
    State(state): State<AppState>,
) -> axum::response::Response {
    let pattern = params.pattern.unwrap_or_else(|| "**".to_string());
    if let Err(e) = mellowmesh_core::topic::validate_pattern(&pattern) {
        return (StatusCode::BAD_REQUEST, format!("Invalid pattern: {e}")).into_response();
    }
    let case_insensitive = params.case_insensitive.unwrap_or(false);
    // Read scopes of the authenticated principal, if any: deliveries are
    // filtered message-by-message against these patterns.
    let token_hash = ctx.principal.as_ref().map(|p| p.token_hash.clone());
    let read_scopes = ctx.principal.map(|p| p.read_scopes);
    ws.on_upgrade(move |socket| {
        handle_socket(
            socket,
            SocketParams {
                pattern,
                case_insensitive,
                read_scopes,
                token_hash,
                stream_e2e: stream_e2e.0,
            },
            state,
        )
    })
    .into_response()
}

struct SocketParams {
    pattern: String,
    case_insensitive: bool,
    read_scopes: Option<Vec<String>>,
    /// Hash of the token the subscription authenticated with, re-validated
    /// periodically so revocation ends the stream.
    token_hash: Option<String>,
    stream_e2e: Option<([u8; 32], String)>,
}

fn close_frame(code: u16, reason: &'static str) -> WsMessage {
    WsMessage::Close(Some(axum::extract::ws::CloseFrame {
        code,
        reason: reason.into(),
    }))
}

async fn handle_socket(socket: WebSocket, params: SocketParams, state: AppState) {
    let SocketParams {
        pattern,
        case_insensitive,
        read_scopes,
        token_hash,
        stream_e2e,
    } = params;
    let conn_id = format!("conn_{}", Ulid::new().to_string().to_lowercase());
    let queue: crate::subscription::SubscriberQueue = Arc::new(Mutex::new(VecDeque::new()));
    let notify = Arc::new(Notify::new());
    let evicted = Arc::new(AtomicBool::new(false));
    let overflow_policy = mellowmesh_core::persistence::OverflowPolicy::DisconnectSlowSubscriber;

    state.registry.add(
        conn_id.clone(),
        pattern,
        queue.clone(),
        WS_QUEUE_CAPACITY,
        notify.clone(),
        overflow_policy,
        case_insensitive,
        evicted.clone(),
    );

    let (mut ws_sender, mut ws_receiver) = socket.split();

    let queue_clone = queue.clone();
    let notify_clone = notify.clone();
    let send_state = state.clone();
    let mut send_task = tokio::spawn(async move {
        let mut next_check = tokio::time::Instant::now() + WS_REVALIDATE_EVERY;
        loop {
            if evicted.load(Ordering::Acquire) {
                // Evicted as a slow consumer: tell the client instead of
                // leaving it waiting on a stream that will never deliver.
                let _ = ws_sender
                    .send(close_frame(1013, "subscriber too slow; reconnect"))
                    .await;
                break;
            }
            // Periodic re-validation of the subscription's token, whether
            // the stream is idle or busy.
            if tokio::time::Instant::now() >= next_check {
                next_check = tokio::time::Instant::now() + WS_REVALIDATE_EVERY;
                if let Some(hash) = &token_hash {
                    if !crate::auth::token_still_valid(&send_state, hash).await {
                        let _ = ws_sender.send(close_frame(1008, "token revoked")).await;
                        break;
                    }
                }
            }
            let opt_msg = {
                let mut q = queue_clone.lock().unwrap();
                q.pop_front()
            };
            let Some(msg) = opt_msg else {
                tokio::select! {
                    _ = notify_clone.notified() => {}
                    _ = tokio::time::sleep_until(next_check) => {}
                }
                continue;
            };
            {
                if let Some(ref scopes) = read_scopes {
                    if !mellowmesh_core::auth::scopes_allow(scopes, &msg.topic) {
                        continue;
                    }
                }
                if let Ok(text) = serde_json::to_string(&*msg) {
                    // Encrypted subscription: seal every delivered message so
                    // a relay in the middle sees only ciphertext.
                    let out = if let Some((key, key_id)) = &stream_e2e {
                        match mellowmesh_core::e2e::seal(
                            key,
                            key_id,
                            mellowmesh_core::e2e::CTX_STREAM,
                            text.as_bytes(),
                        )
                        .and_then(|e| Ok(serde_json::to_string(&e)?))
                        {
                            Ok(sealed) => sealed,
                            Err(_) => continue,
                        }
                    } else {
                        text
                    };
                    if ws_sender.send(WsMessage::Text(out)).await.is_err() {
                        break;
                    }
                }
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
            http: crate::state::loopback_http_client(),
            caches: Arc::new(crate::cache::Caches::default()),
        }
    }

    /// Serve `state` on `port` and return the base URL.
    async fn serve(mut state: AppState, port: u16) -> String {
        state.port = port;
        let app = create_router(state);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        format!("http://127.0.0.1:{port}")
    }

    /// Seed a principal + token directly in the store; returns the token.
    fn seed_token(store: &Store, principal: &str, read: &[&str], write: &[&str]) -> String {
        use mellowmesh_core::auth::{generate_token, hash_token, kind_of_uri, Principal};
        let token = generate_token();
        store
            .upsert_principal(&Principal {
                id: principal.to_string(),
                kind: kind_of_uri(principal).to_string(),
                display_name: None,
                created_at: chrono::Utc::now(),
            })
            .unwrap();
        store
            .insert_token(&mellowmesh_core::auth::TokenRecord {
                id: format!("tok_{}", Ulid::new().to_string().to_lowercase()),
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

    fn approve_reject_decision(id: &str, created_by: &str, decider: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id, "title": "Ship?", "question": "Ship it?",
            "created_by": created_by, "required_decider": decider,
            "status": "requested",
            "options": [
                { "id": "yes", "label": "Yes", "pros": [], "cons": [], "outcome": "approve" },
                { "id": "no", "label": "No", "pros": [], "cons": [], "outcome": "reject" }
            ]
        })
    }

    #[tokio::test]
    async fn test_host_and_origin_enforcement() {
        let store = Store::new_in_memory().unwrap();
        let port = 41060;
        let base = serve(test_state(store, false), port).await;
        let http = reqwest::Client::new();

        // Plain loopback request (no Origin): allowed.
        let resp = http.get(format!("{base}/tasks")).send().await.unwrap();
        assert_eq!(resp.status(), 200);

        // Foreign Host (DNS rebinding): rejected, even on open endpoints.
        for host in ["evil.example:41060", "127.0.0.1.evil.example", "10.1.2.3"] {
            let resp = http
                .get(format!("{base}/health"))
                .header("Host", host)
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 403, "host {host}");
        }
        // Loopback aliases are fine.
        for host in ["localhost:41060", "[::1]:41060", "127.0.0.1"] {
            let resp = http
                .get(format!("{base}/health"))
                .header("Host", host)
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 200, "host {host}");
        }

        // Cross-site Origin: rejected (CSRF to /shutdown, cross-site WS).
        let resp = http
            .post(format!("{base}/shutdown"))
            .header("Origin", "http://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403);
        let resp = http
            .get(format!("{base}/tasks"))
            .header("Origin", "http://127.0.0.1:3000")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 403);

        // Same-origin dashboard: allowed.
        for origin in [
            "http://127.0.0.1:41060",
            "http://localhost:41060",
            "http://[::1]:41060",
        ] {
            let resp = http
                .get(format!("{base}/tasks"))
                .header("Origin", origin)
                .send()
                .await
                .unwrap();
            assert_eq!(resp.status(), 200, "origin {origin}");
        }

        // Cross-site WebSocket handshake is refused.
        use tokio_tungstenite::tungstenite::client::IntoClientRequest;
        let mut req = format!("ws://127.0.0.1:{port}/ws?pattern=**")
            .into_client_request()
            .unwrap();
        req.headers_mut()
            .insert("Origin", "http://evil.example".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(req).await.is_err());
        let ok = format!("ws://127.0.0.1:{port}/ws?pattern=**");
        assert!(tokio_tungstenite::connect_async(ok).await.is_ok());

        // An invalid subscription pattern is refused, not upgraded.
        let empty = format!("ws://127.0.0.1:{port}/ws?pattern=");
        assert!(tokio_tungstenite::connect_async(empty).await.is_err());
    }

    #[tokio::test]
    async fn test_identity_mappings_are_owner_only() {
        let store = Store::new_in_memory().unwrap();
        let owner = seed_token(&store, "human://test", &["**"], &["**"]);
        let agent = seed_token(&store, "agent://test/coder", &["**"], &["**"]);
        let port = 41061;
        let base = serve(test_state(store, true), port).await;
        let http = reqwest::Client::new();
        let post = |token: String, ext: &str, id: &str| {
            http.post(format!("{base}/identity-mappings"))
                .bearer_auth(token)
                .json(&serde_json::json!({ "external_id": ext, "mellowmesh_id": id }))
                .send()
        };

        // A non-owner cannot create mappings.
        let r = post(agent.clone(), "telegram://1", "human://test")
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        // Only human:// targets.
        let r = post(owner.clone(), "telegram://1", "agent://test/coder")
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        // Under auth, the human must exist as a principal.
        let r = post(owner.clone(), "telegram://1", "human://nobody")
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        let r = post(owner.clone(), "telegram://1", "human://test")
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        // Listing stays readable.
        let r = http
            .get(format!("{base}/identity-mappings"))
            .bearer_auth(&agent)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }

    #[tokio::test]
    async fn test_required_decider_and_proposer_binding() {
        let store = Store::new_in_memory().unwrap();
        let owner = seed_token(&store, "human://test", &["**"], &["**"]);
        let you = seed_token(&store, "human://you", &["**"], &["**"]);
        let other = seed_token(&store, "human://other", &["**"], &["**"]);
        let agent = seed_token(&store, "agent://test/builder", &["**"], &["**"]);
        let port = 41062;
        let base = serve(test_state(store.clone(), true), port).await;
        let http = reqwest::Client::new();

        // The proposer is the authenticated principal, whatever the body says.
        for id in ["dec_a", "dec_b"] {
            let r = http
                .post(format!("{base}/decisions"))
                .bearer_auth(&agent)
                .json(&approve_reject_decision(
                    id,
                    "human://forged",
                    "human://you",
                ))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 200);
            let d = store.get_decision(id).unwrap().unwrap();
            assert_eq!(d.created_by, "agent://test/builder");
        }
        // Decision ids are restricted (they become topic segments).
        let r = http
            .post(format!("{base}/decisions"))
            .bearer_auth(&agent)
            .json(&approve_reject_decision("bad.id.**", "x", "human://you"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);

        let respond = |token: String, id: &str| {
            http.post(format!("{base}/decisions/{id}/respond"))
                .bearer_auth(token)
                .json(&serde_json::json!({ "option_id": "yes" }))
                .send()
        };
        // A human who is not the required decider (nor the owner): 403.
        let r = respond(other, "dec_a").await.unwrap();
        assert_eq!(r.status(), 403);
        assert_eq!(
            store.get_decision("dec_a").unwrap().unwrap().status,
            "requested"
        );
        // The required decider may answer.
        let r = respond(you, "dec_a").await.unwrap();
        assert_eq!(r.status(), 200);
        // The owner may answer any decision.
        let r = respond(owner, "dec_b").await.unwrap();
        assert_eq!(r.status(), 200);
        let d = store.get_decision("dec_b").unwrap().unwrap();
        assert_eq!(d.responded_by.as_deref(), Some("human://test"));
    }

    #[tokio::test]
    async fn test_reserved_topics_and_hub_assigned_identity() {
        let store = Store::new_in_memory().unwrap();
        let agent = seed_token(&store, "agent://test/coder", &["**"], &["**"]);
        let port = 41063;
        let base = serve(test_state(store, true), port).await;
        let http = reqwest::Client::new();
        let msg = |topic: &str| {
            serde_json::json!({
                "id": "forged-id", "topic": topic, "from": "agent://someone/else",
                "timestamp": "2001-01-01T00:00:00Z", "content_type": "text/plain",
                "body": "hello"
            })
        };

        // Clients cannot publish on hub-reserved topics, whatever their scope.
        for topic in [
            "_decision.dec_1.responded",
            "_DECISION.dec_1.requested",
            "_system.presence.x",
            "_system.registry.agent",
            "_task.t1.reclaimed",
        ] {
            let r = http
                .post(format!("{base}/publish"))
                .bearer_auth(&agent)
                .json(&msg(topic))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 403, "topic {topic}");
        }

        // Id, timestamp and (for agents) `from` are assigned by the hub.
        let before = chrono::Utc::now() - chrono::Duration::seconds(5);
        let r = http
            .post(format!("{base}/publish"))
            .bearer_auth(&agent)
            .json(&msg("_forum.general"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let m: mellowmesh_core::message::Message = r.json().await.unwrap();
        assert_ne!(m.id, "forged-id");
        assert!(m.id.starts_with("msg_"));
        assert!(m.timestamp > before, "timestamp must be server time");
        assert_eq!(m.from, "agent://test/coder");
        assert_eq!(m.owner.as_deref(), Some("agent://test/coder"));
    }

    #[tokio::test]
    async fn test_context_and_summaries_respect_scopes() {
        let store = Store::new_in_memory().unwrap();
        let owner = seed_token(&store, "human://test", &["**"], &["**"]);
        let agent = seed_token(
            &store,
            "agent://test/coder",
            &["_project.app.**"],
            &["_project.app.**"],
        );
        let port = 41064;
        let base = serve(test_state(store, true), port).await;
        let http = reqwest::Client::new();

        for topic in ["_project.app.notes", "_project.secret.notes"] {
            let r = http
                .post(format!("{base}/publish"))
                .bearer_auth(&owner)
                .json(&serde_json::json!({
                    "id": "", "topic": topic, "from": "human://test",
                    "timestamp": chrono::Utc::now(), "content_type": "text/plain",
                    "body": format!("note on {topic}")
                }))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 200);
            let r = http
                .post(format!("{base}/summaries"))
                .bearer_auth(&owner)
                .json(&serde_json::json!({ "topic": topic, "summary": format!("sum {topic}") }))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 200);
        }
        // Writing a summary outside the write scope is refused.
        let r = http
            .post(format!("{base}/summaries"))
            .bearer_auth(&agent)
            .json(&serde_json::json!({ "topic": "_project.secret.notes", "summary": "x" }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);

        // Wait for asynchronous persistence.
        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        let ctx: mellowmesh_core::persistence::ContextResult = http
            .get(format!("{base}/context?topic=_project.**"))
            .bearer_auth(&agent)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!ctx.relevant_messages.is_empty());
        assert!(ctx
            .relevant_messages
            .iter()
            .all(|m| m.topic.starts_with("_project.app.")));
        assert_eq!(ctx.summaries.len(), 1);
        assert_eq!(ctx.summaries[0].topic, "_project.app.notes");

        // History with a pattern is filtered by scope too; bad patterns 400.
        let msgs: Vec<mellowmesh_core::message::Message> = http
            .get(format!("{base}/history?pattern=_project.**&limit=5000"))
            .bearer_auth(&agent)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(msgs.len(), 1);
        let r = http
            .get(format!("{base}/context?topic="))
            .bearer_auth(&agent)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        let r = http
            .get(format!("{base}/forum?pattern="))
            .bearer_auth(&agent)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
        // Empty search is an empty list.
        let found: Vec<mellowmesh_core::message::Message> = http
            .get(format!("{base}/search?query="))
            .bearer_auth(&owner)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn test_schemas_are_owner_only_and_compiled() {
        let store = Store::new_in_memory().unwrap();
        let owner = seed_token(&store, "human://test", &["**"], &["**"]);
        let agent = seed_token(&store, "agent://test/coder", &["**"], &["**"]);
        let port = 41065;
        let base = serve(test_state(store, true), port).await;
        let http = reqwest::Client::new();
        let schema = |content: &str| {
            serde_json::json!({
                "topic_pattern": "_project.typed.**", "version": "v1",
                "schema_content": content
            })
        };
        let valid = r#"{"type":"object","required":["n"],"properties":{"n":{"type":"integer"}}}"#;

        let r = http
            .post(format!("{base}/schemas"))
            .bearer_auth(&agent)
            .json(&schema(valid))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        let r = http
            .post(format!("{base}/schemas"))
            .bearer_auth(&owner)
            .json(&schema(r#"{"type": 12}"#))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400, "an invalid schema must not be stored");
        let r = http
            .post(format!("{base}/schemas"))
            .bearer_auth(&owner)
            .json(&schema(valid))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);

        // The (cached) compiled schema is enforced on publish.
        let publish = |body: &str| {
            http.post(format!("{base}/publish"))
                .bearer_auth(&agent)
                .json(&serde_json::json!({
                    "id": "", "topic": "_project.typed.x", "from": "x",
                    "timestamp": chrono::Utc::now(), "content_type": "application/json",
                    "body": body
                }))
                .send()
        };
        assert_eq!(publish(r#"{"n": 1}"#).await.unwrap().status(), 200);
        assert_eq!(publish(r#"{"n": "x"}"#).await.unwrap().status(), 400);

        // Pausing (owner-only) invalidates the cache.
        let r = http
            .post(format!("{base}/schemas/status"))
            .bearer_auth(&agent)
            .json(&serde_json::json!({
                "topic_pattern": "_project.typed.**", "version": "v1", "status": "paused"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 403);
        let r = http
            .post(format!("{base}/schemas/status"))
            .bearer_auth(&owner)
            .json(&serde_json::json!({
                "topic_pattern": "_project.typed.**", "version": "v1", "status": "paused"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert_eq!(publish(r#"{"n": "x"}"#).await.unwrap().status(), 200);
    }

    #[tokio::test]
    async fn test_wiki_writes_validate_paths() {
        let root = std::env::temp_dir().join(format!(
            "mm-wiki-{}",
            Ulid::new().to_string().to_lowercase()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::new_in_memory().unwrap();
        let mut state = test_state(store, false);
        let mut wikis = std::collections::HashMap::new();
        wikis.insert("docs".to_string(), root.clone());
        state.wikis = Arc::new(wikis);
        let port = 41066;
        let base = serve(state, port).await;
        let http = reqwest::Client::new();
        let page = serde_json::json!({
            "doc_type": "note", "title": "T", "description": null,
            "tags": [], "resource": null, "body": "hello"
        });

        // (%2F keeps `..` from being normalized away by the HTTP client.)
        for bad in [
            "C:/x.md",
            "C:%5Cx.md",
            "x%2F..%2F..%2Fescape.md",
            "notes/x.txt",
            "%5C%5Cserver%5Cx.md",
        ] {
            let r = http
                .post(format!("{base}/wiki/docs/pages/{bad}"))
                .json(&page)
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 400, "path {bad}");
        }
        assert!(!root.parent().unwrap().join("escape.md").exists());

        let r = http
            .post(format!("{base}/wiki/docs/pages/notes/ok.md"))
            .json(&page)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert!(root.join("notes/ok.md").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn test_mcp_jsonrpc_handling() {
        let store = Store::new_in_memory().unwrap();
        let port = 41067;
        let base = serve(test_state(store, false), port).await;
        let http = reqwest::Client::new();
        let call = |body: serde_json::Value| http.post(format!("{base}/mcp")).json(&body).send();

        // ping
        let r: serde_json::Value =
            call(serde_json::json!({ "jsonrpc": "2.0", "id": 7, "method": "ping" }))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(r["id"], 7);
        assert_eq!(r["result"], serde_json::json!({}));

        // Notifications (no id) are never answered, whatever the method.
        for method in ["notifications/initialized", "ping", "tools/list"] {
            let r = call(serde_json::json!({ "jsonrpc": "2.0", "method": method }))
                .await
                .unwrap();
            assert_eq!(r.status(), 202, "{method}");
            assert!(r.text().await.unwrap().is_empty());
        }

        // Batches are rejected.
        let r: serde_json::Value = call(serde_json::json!([
            { "jsonrpc": "2.0", "id": 1, "method": "ping" }
        ]))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(r["error"]["code"], -32600);

        // Wrong jsonrpc version.
        let r: serde_json::Value =
            call(serde_json::json!({ "jsonrpc": "1.0", "id": 2, "method": "ping" }))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(r["error"]["code"], -32600);

        // tools/call without a name.
        let r: serde_json::Value = call(serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {}
        }))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(r["error"]["code"], -32602);

        // Version negotiation.
        let r: serde_json::Value = call(serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": "initialize",
            "params": { "protocolVersion": "2025-03-26" }
        }))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        let r: serde_json::Value = call(serde_json::json!({
            "jsonrpc": "2.0", "id": 5, "method": "initialize",
            "params": { "protocolVersion": "1999-01-01" }
        }))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
    }

    #[tokio::test]
    async fn test_trace_duration_cap_and_disable_accounting() {
        let store = Store::new_in_memory().unwrap();
        let port = 41068;
        let base = serve(test_state(store, false), port).await;
        let http = reqwest::Client::new();
        let enable = |duration: &str| {
            http.post(format!("{base}/traces"))
                .json(&serde_json::json!({
                    "target": "agent://test/coder", "target_type": "agent",
                    "level": "verbose", "duration": duration, "enabled_by": "human://test"
                }))
                .send()
        };
        for d in ["999999999999d", "25h", "0s", "abc"] {
            assert_eq!(enable(d).await.unwrap().status(), 400, "duration {d}");
        }
        let r = enable("1h").await.unwrap();
        assert_eq!(r.status(), 200);
        let ts: mellowmesh_core::telemetry::TraceSession = r.json().await.unwrap();
        assert!(ts.topics.iter().all(|t| t != "_trace.**"));

        let active = |http: reqwest::Client| {
            let base = base.clone();
            async move {
                let m: serde_json::Value = http
                    .get(format!("{base}/metrics"))
                    .send()
                    .await
                    .unwrap()
                    .json()
                    .await
                    .unwrap();
                m["trace_sessions_active"].as_u64().unwrap()
            }
        };
        assert_eq!(active(http.clone()).await, 1);
        for _ in 0..2 {
            let r = http
                .delete(format!("{base}/traces/{}", ts.id))
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), 200);
        }
        assert_eq!(
            active(http.clone()).await,
            0,
            "double disable must not underflow"
        );
    }

    #[tokio::test]
    async fn test_open_mode_reject_and_no_forged_human() {
        use mellowmesh_core::decision::{Decision, DecisionOption, DecisionOutcome};

        let store = Store::new_in_memory().unwrap();
        store
            .insert_decision(&Decision {
                id: "dec_gov".to_string(),
                title: "Deploy?".to_string(),
                question: "Ship to prod?".to_string(),
                created_by: "agent://you/builder".to_string(),
                required_decider: "human://you".to_string(),
                status: "requested".to_string(),
                options: vec![
                    DecisionOption {
                        id: "yes".to_string(),
                        label: "Approve".to_string(),
                        pros: vec![],
                        cons: vec![],
                        outcome: Some(DecisionOutcome::Approve),
                    },
                    DecisionOption {
                        id: "no".to_string(),
                        label: "Reject".to_string(),
                        pros: vec![],
                        cons: vec![],
                        outcome: Some(DecisionOutcome::Reject),
                    },
                ],
                response_option_id: None,
                response_timestamp: None,
                responded_by: None,
            })
            .unwrap();

        let port = 40016;
        let state = test_state(store.clone(), false); // OPEN mode
        let app = create_router(state);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Reject the decision, and try to forge a human in the body.
        let http = reqwest::Client::new();
        let resp = http
            .post(format!("http://127.0.0.1:{port}/decisions/dec_gov/respond"))
            .json(&serde_json::json!({
                "option_id": "no",
                "responded_by": "human://yannick"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let d = store.get_decision("dec_gov").unwrap().unwrap();
        // A rejection is recorded as rejected, never approved.
        assert_eq!(d.status, "rejected");
        // Open mode never trusts a caller-supplied human — no forgery.
        assert_eq!(
            d.responded_by,
            Some("human://local-unauthenticated".to_string())
        );

        // A decision is answered exactly once.
        let again = http
            .post(format!("http://127.0.0.1:{port}/decisions/dec_gov/respond"))
            .json(&serde_json::json!({ "option_id": "yes" }))
            .send()
            .await
            .unwrap();
        assert_eq!(again.status(), 409);
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
        // Pin the exact count so the documented number (README, docs/mcp.md)
        // and the code can never silently drift apart again.
        assert_eq!(
            tools.len(),
            27,
            "MCP tool count changed — update README.md and docs/mcp.md to match"
        );

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
    async fn test_e2e_encrypted_request() {
        use mellowmesh_core::auth::{generate_token, hash_token, Principal, TokenRecord};
        use mellowmesh_core::e2e::{
            derive_key, derive_key_id, new_jti, open, seal, Envelope, SealedRequest,
            SealedResponse, CTX_REQUEST, CTX_RESPONSE,
        };

        let store = Store::new_in_memory().unwrap();

        // Mint an owner token and register its e2e key (as the daemon does).
        let token = generate_token();
        store
            .upsert_principal(&Principal {
                id: "human://owner".to_string(),
                kind: "human".to_string(),
                display_name: None,
                created_at: chrono::Utc::now(),
            })
            .unwrap();
        store
            .insert_token(&TokenRecord {
                id: "tok_owner".to_string(),
                principal: "human://owner".to_string(),
                token_hash: hash_token(&token),
                read_scopes: vec!["**".to_string()],
                write_scopes: vec!["**".to_string()],
                created_at: chrono::Utc::now(),
                revoked: false,
            })
            .unwrap();
        store.register_e2e_key(&token).unwrap();

        let port = 40014;
        let mut state = test_state(store, true); // require_auth ON
        state.port = port;
        state.owner = "human://owner".to_string();
        let app = create_router(state);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let http = reqwest::Client::new();
        let key = derive_key(&token);
        let key_id = derive_key_id(&token);

        // Seal a GET /tasks and send it through /e2e/request.
        let sealed = SealedRequest {
            ts: chrono::Utc::now().timestamp(),
            jti: Some(new_jti()),
            method: "GET".to_string(),
            path_and_query: "/tasks".to_string(),
            authorization: Some(format!("Bearer {token}")),
            body: None,
        };
        let envelope = seal(
            &key,
            &key_id,
            CTX_REQUEST,
            &serde_json::to_vec(&sealed).unwrap(),
        )
        .unwrap();

        let resp = http
            .post(format!("http://127.0.0.1:{port}/e2e/request"))
            .json(&envelope)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let reply: Envelope = resp.json().await.unwrap();
        let opened = open(&key, CTX_RESPONSE, &reply).unwrap();
        let sealed_resp: SealedResponse = serde_json::from_slice(&opened).unwrap();
        assert_eq!(sealed_resp.status, 200); // inner GET /tasks succeeded under auth

        // Replaying the exact same envelope (same jti) is rejected.
        let replay = http
            .post(format!("http://127.0.0.1:{port}/e2e/request"))
            .json(&envelope)
            .send()
            .await
            .unwrap();
        assert_eq!(
            replay.status(),
            401,
            "replayed sealed request must be rejected"
        );

        // Tampered ciphertext → decryption fails at the daemon → 401.
        let mut bad = envelope.clone();
        let mut ct = bad.ciphertext.clone().into_bytes();
        ct[0] = if ct[0] == b'0' { b'1' } else { b'0' };
        bad.ciphertext = String::from_utf8(ct).unwrap();
        let resp = http
            .post(format!("http://127.0.0.1:{port}/e2e/request"))
            .json(&bad)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);

        // Replay outside the window → rejected.
        let stale = SealedRequest {
            ts: chrono::Utc::now().timestamp() - 10_000,
            jti: Some(new_jti()),
            method: "GET".to_string(),
            path_and_query: "/tasks".to_string(),
            authorization: Some(format!("Bearer {token}")),
            body: None,
        };
        let stale_env = seal(
            &key,
            &key_id,
            CTX_REQUEST,
            &serde_json::to_vec(&stale).unwrap(),
        )
        .unwrap();
        let resp = http
            .post(format!("http://127.0.0.1:{port}/e2e/request"))
            .json(&stale_env)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 401);

        // SSRF guard: a sealed path that is not an absolute path is rejected
        // before it can redirect the daemon's loopback request off-host.
        let ssrf = SealedRequest {
            ts: chrono::Utc::now().timestamp(),
            jti: Some(new_jti()),
            method: "GET".to_string(),
            path_and_query: "@attacker.example.com/x".to_string(),
            authorization: Some(format!("Bearer {token}")),
            body: None,
        };
        let ssrf_env = seal(
            &key,
            &key_id,
            CTX_REQUEST,
            &serde_json::to_vec(&ssrf).unwrap(),
        )
        .unwrap();
        let resp = http
            .post(format!("http://127.0.0.1:{port}/e2e/request"))
            .json(&ssrf_env)
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            400,
            "non-absolute sealed path must be rejected"
        );
    }

    #[tokio::test]
    async fn test_e2e_encrypted_subscription() {
        use mellowmesh_core::auth::{generate_token, hash_token, Principal, TokenRecord};
        use mellowmesh_core::e2e::{
            derive_key, derive_key_id, new_jti, open, seal, Envelope, SealedRequest, CTX_PROOF,
            CTX_STREAM,
        };

        let store = Store::new_in_memory().unwrap();
        let token = generate_token();
        store
            .upsert_principal(&Principal {
                id: "human://owner".to_string(),
                kind: "human".to_string(),
                display_name: None,
                created_at: chrono::Utc::now(),
            })
            .unwrap();
        store
            .insert_token(&TokenRecord {
                id: "tok_owner".to_string(),
                principal: "human://owner".to_string(),
                token_hash: hash_token(&token),
                read_scopes: vec!["**".to_string()],
                write_scopes: vec!["**".to_string()],
                created_at: chrono::Utc::now(),
                revoked: false,
            })
            .unwrap();
        store.register_e2e_key(&token).unwrap();

        let port = 40015;
        let state = test_state(store, true); // require_auth ON
        let app = create_router(state);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let key = derive_key(&token);
        let key_id = derive_key_id(&token);

        // Subscribe with a sealed proof instead of a raw token in the query.
        let proof = SealedRequest {
            ts: chrono::Utc::now().timestamp(),
            jti: Some(new_jti()),
            method: "SUBSCRIBE".to_string(),
            path_and_query: "/ws".to_string(),
            authorization: Some(format!("Bearer {token}")),
            body: None,
        };
        let envelope = seal(
            &key,
            &key_id,
            CTX_PROOF,
            &serde_json::to_vec(&proof).unwrap(),
        )
        .unwrap();
        let ws_url = format!(
            "ws://127.0.0.1:{port}/ws?pattern=_task.%2A%2A&e2e_kid={}&e2e_nonce={}&e2e_ct={}",
            envelope.key_id, envelope.nonce, envelope.ciphertext
        );
        let (ws, _) = tokio_tungstenite::connect_async(&ws_url).await.unwrap();
        let (mut _tx, mut rx) = ws.split();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Publish a matching message over authenticated HTTP.
        let http = reqwest::Client::new();
        let resp = http
            .post(format!("http://127.0.0.1:{port}/publish"))
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "id": "", "topic": "_task.demo.progress", "from": "human://owner",
                "timestamp": chrono::Utc::now(), "content_type": "text/plain",
                "body": "sealed delivery"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // The delivered frame must be a sealed envelope, not plaintext.
        let frame = tokio::time::timeout(tokio::time::Duration::from_secs(3), rx.next())
            .await
            .expect("should receive a frame")
            .unwrap()
            .unwrap();
        let text = frame.to_text().unwrap();
        assert!(
            !text.contains("sealed delivery"),
            "delivered frame leaked plaintext: {text}"
        );
        let delivered: Envelope = serde_json::from_str(text).unwrap();
        let opened = open(&key, CTX_STREAM, &delivered).unwrap();
        let msg: mellowmesh_core::message::Message = serde_json::from_slice(&opened).unwrap();
        assert_eq!(msg.topic, "_task.demo.progress");
        assert_eq!(msg.body, "sealed delivery");

        // A tampered proof is rejected at the handshake.
        let bad_url = format!(
            "ws://127.0.0.1:{port}/ws?pattern=**&e2e_kid={}&e2e_nonce={}&e2e_ct={}00",
            envelope.key_id, envelope.nonce, envelope.ciphertext
        );
        assert!(tokio_tungstenite::connect_async(&bad_url).await.is_err());
    }

    #[tokio::test]
    async fn test_interface_relays_human_decision_response() {
        use mellowmesh_core::auth::{generate_token, hash_token, Principal, TokenRecord};
        use mellowmesh_core::decision::{Decision, DecisionOption};

        let store = Store::new_in_memory().unwrap();

        // An interface token, as minted for the Telegram/Discord connectors.
        let interface_token = generate_token();
        store
            .upsert_principal(&Principal {
                id: "interface://local/connectors".to_string(),
                kind: "interface".to_string(),
                display_name: None,
                created_at: chrono::Utc::now(),
            })
            .unwrap();
        store
            .insert_token(&TokenRecord {
                id: "tok_iface".to_string(),
                principal: "interface://local/connectors".to_string(),
                token_hash: hash_token(&interface_token),
                read_scopes: vec!["**".to_string()],
                write_scopes: vec!["**".to_string()],
                created_at: chrono::Utc::now(),
                revoked: false,
            })
            .unwrap();
        // The relayed human must be a known human principal.
        store
            .upsert_principal(&Principal {
                id: "human://you".to_string(),
                kind: "human".to_string(),
                display_name: None,
                created_at: chrono::Utc::now(),
            })
            .unwrap();
        store
            .insert_decision(&Decision {
                id: "dec_relay".to_string(),
                title: "Ship?".to_string(),
                question: "Ship it?".to_string(),
                created_by: "agent://you/builder".to_string(),
                required_decider: "human://you".to_string(),
                status: "requested".to_string(),
                options: vec![DecisionOption {
                    id: "option_yes".to_string(),
                    label: "Yes".to_string(),
                    pros: vec![],
                    cons: vec![],
                    outcome: Some(mellowmesh_core::decision::DecisionOutcome::Approve),
                }],
                response_option_id: None,
                response_timestamp: None,
                responded_by: None,
            })
            .unwrap();

        let state = test_state(store.clone(), true);
        let app = create_router(state);
        let port = 40013;
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // The connector relays a Telegram tap on behalf of a mapped human.
        let http = reqwest::Client::new();
        let resp = http
            .post(format!(
                "http://127.0.0.1:{port}/decisions/dec_relay/respond"
            ))
            .bearer_auth(&interface_token)
            .json(&serde_json::json!({
                "option_id": "option_yes",
                "responded_by": "human://you"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let decision = store.get_decision("dec_relay").unwrap().unwrap();
        assert_eq!(decision.status, "approved");
        assert_eq!(
            decision.responded_by,
            Some("human://you (via interface://local/connectors)".to_string())
        );
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
            http: crate::state::loopback_http_client(),
            caches: Arc::new(crate::cache::Caches::default()),
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
            http: crate::state::loopback_http_client(),
            caches: Arc::new(crate::cache::Caches::default()),
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
