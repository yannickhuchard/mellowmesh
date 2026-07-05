use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use mellowmesh_core::message::Message;
use mellowmesh_core::persistence::{
    IndexableMessage, OverflowPolicy, PersistableMessage, PersistenceMode,
};
use serde::Deserialize;
use std::sync::atomic::Ordering;
use ulid::Ulid;

#[derive(Deserialize)]
pub struct HistoryParams {
    limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct SearchParams {
    query: String,
}

#[derive(Deserialize)]
pub struct ForumParams {
    pattern: Option<String>,
}

async fn route_to_inbox(state: std::sync::Arc<AppState>, msg: Message) {
    let _ = handle_publish(state, msg).await;
}

pub fn handle_publish(
    state: std::sync::Arc<AppState>,
    mut msg: Message,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Message, String>> + Send + 'static>>
{
    Box::pin(async move {
        if msg.id.is_empty() {
            msg.id = format!("msg_{}", Ulid::new().to_string().to_lowercase());
        }

        // Intercept registry synchronization events
        if msg.topic == "_system.registry.named_topic" {
            let action = msg
                .headers
                .as_ref()
                .and_then(|h| h.get("x-registry-action"))
                .map(|v| v.as_str())
                .unwrap_or("");
            match action {
                "register" => {
                    if let Ok(named_topic) =
                        serde_json::from_str::<mellowmesh_core::topic::NamedTopic>(&msg.body)
                    {
                        let _ = state.store.register_named_topic(&named_topic);
                    }
                }
                "remove" => {
                    let name = msg.body.trim();
                    if !name.is_empty() {
                        let _ = state.store.remove_named_topic(name);
                    }
                }
                _ => {}
            }
        } else if msg.topic == "_system.registry.agent" {
            let action = msg
                .headers
                .as_ref()
                .and_then(|h| h.get("x-registry-action"))
                .map(|v| v.as_str())
                .unwrap_or("");
            if action == "register" {
                if let Ok(agent) =
                    serde_json::from_str::<mellowmesh_core::agent::AgentRegistration>(&msg.body)
                {
                    let _ = state.store.register_agent(&agent);
                }
            }
        }

        // Parse mentions and route directed inbox copies
        let is_routed = msg
            .headers
            .as_ref()
            .and_then(|h| h.get("x-mellowmesh-routed"))
            .map(|v| v == "true")
            .unwrap_or(false);

        if !is_routed
            && !msg.topic.starts_with("_agent.")
            && (msg.topic.starts_with("_forum.") || msg.topic.starts_with("_project."))
        {
            if let Ok(agents) = state.store.list_agents() {
                let named_topics = state.store.list_named_topics().unwrap_or_default();
                let (new_body, mentions) =
                    mellowmesh_core::mentions::parse_mentions(&msg.body, &agents, &named_topics);
                msg.body = new_body;
                if !mentions.is_empty() {
                    let mut headers = msg.headers.clone().unwrap_or_default();
                    headers.insert(
                        "x-mentions".to_string(),
                        serde_json::to_string(&mentions).unwrap_or_default(),
                    );
                    msg.headers = Some(headers);

                    for mention_uri in mentions {
                        if mention_uri.starts_with("agent://") {
                            let path = mention_uri.replace("agent://", "").replace("/", ".");
                            let inbox_topic = format!("_agent.{path}.inbox");

                            let mut routed_msg = msg.clone();
                            routed_msg.id =
                                format!("msg_{}", Ulid::new().to_string().to_lowercase());
                            routed_msg.topic = inbox_topic;

                            let mut copy_headers = routed_msg.headers.clone().unwrap_or_default();
                            copy_headers
                                .insert("x-mellowmesh-routed".to_string(), "true".to_string());
                            routed_msg.headers = Some(copy_headers);

                            let state_clone = state.clone();
                            tokio::spawn(route_to_inbox(state_clone, routed_msg));
                        }
                    }
                }
            }
        }

        if let Err(e) = mellowmesh_core::topic::Topic::new(&msg.topic) {
            return Err(format!("Invalid topic: {e}"));
        }

        if msg.timestamp.timestamp() <= 0 {
            msg.timestamp = Utc::now();
        }

        // Progress updates double as claim-lease heartbeats: publishing on
        // `_task.<id>.progress` renews the publisher's lease on that task.
        if let Some(task_id) = msg
            .topic
            .strip_prefix("_task.")
            .and_then(|rest| rest.strip_suffix(".progress"))
        {
            if !task_id.is_empty() && !task_id.contains('.') {
                match state.store.renew_claim(task_id, &msg.from) {
                    Ok(true) => {
                        tracing::debug!("Renewed claim lease on {} for {}", task_id, msg.from)
                    }
                    Ok(false) => {}
                    Err(e) => tracing::warn!("Lease renewal failed for {}: {}", task_id, e),
                }
            }
        }

        // JSON Schema Contract Validation
        let matched_schemas = match state.store.get_schemas_for_topic(&msg.topic) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    "Failed to retrieve schemas for topic '{}': {}",
                    msg.topic,
                    e
                );
                return Err(format!("Internal schema lookup failed: {e}"));
            }
        };

        if !matched_schemas.is_empty() {
            let requested_version = msg
                .headers
                .as_ref()
                .and_then(|h| {
                    h.get("schema_version")
                        .or_else(|| h.get("x-schema-version"))
                })
                .cloned();

            let schema_to_validate = if let Some(ref ver) = requested_version {
                matched_schemas.iter().find(|s| &s.version == ver)
            } else {
                matched_schemas.first()
            };

            match schema_to_validate {
                Some(schema) => {
                    let json_value: serde_json::Value = if let Some(ref payload) = msg.payload {
                        payload.clone()
                    } else {
                        match serde_json::from_str(&msg.body) {
                            Ok(v) => v,
                            Err(e) => {
                                return Err(format!("Message body is not valid JSON, but topic '{}' requires a schema contract: {}", msg.topic, e));
                            }
                        }
                    };

                    let schema_json: serde_json::Value =
                        match serde_json::from_str(&schema.schema_content) {
                            Ok(v) => v,
                            Err(e) => {
                                return Err(format!("Invalid stored schema JSON: {e}"));
                            }
                        };

                    let compiled_schema = match jsonschema::JSONSchema::compile(&schema_json) {
                        Ok(s) => s,
                        Err(e) => {
                            return Err(format!("Failed to compile JSON Schema: {e}"));
                        }
                    };

                    let err_msgs: Option<Vec<String>> = {
                        match compiled_schema.validate(&json_value) {
                            Ok(_) => None,
                            Err(errors) => Some(errors.map(|e| e.to_string()).collect()),
                        }
                    };

                    if let Some(msgs) = err_msgs {
                        return Err(format!(
                            "Schema validation failed for version '{}': {}",
                            schema.version,
                            msgs.join("; ")
                        ));
                    }
                }
                None => {
                    if let Some(ref ver) = requested_version {
                        return Err(format!(
                            "Schema version '{}' not found for topic '{}'",
                            ver, msg.topic
                        ));
                    }
                }
            }
        }

        let mut headers = msg.headers.clone().unwrap_or_default();
        let contains_self = if let Some(old) = headers.get("forwarded_by") {
            old.split(',').any(|n| n.trim() == state.node_id)
        } else {
            false
        };
        if !contains_self {
            let new_forwarded_by = if let Some(old) = headers.get("forwarded_by") {
                format!("{},{}", old, state.node_id)
            } else {
                state.node_id.clone()
            };
            headers.insert("forwarded_by".to_string(), new_forwarded_by);
            msg.headers = Some(headers);
        }

        state
            .metrics
            .messages_published_total
            .fetch_add(1, Ordering::Relaxed);

        // Check Trace Authorization if it is a trace topic
        if msg.topic.starts_with("_trace.") && !state.trace_mgr.check_trace_allowed(&msg) {
            // Drop silently, return OK as per telemetry policy
            return Ok(msg);
        }

        // Resolve persistence policy
        let policy = state.policy_config.resolve(&msg.topic);

        // Broadcast in-memory (Hot path)
        state.registry.broadcast(&msg);
        state
            .metrics
            .messages_routed_total
            .fetch_add(1, Ordering::Relaxed);

        if policy.mode != PersistenceMode::Ephemeral {
            let pm = PersistableMessage {
                message: msg.clone(),
                mode: policy.mode,
            };

            if policy.sync {
                // Synchronous persistence
                use mellowmesh_core::persistence::EventStore;
                if let Err(e) = state.store.persist_batch(vec![pm]).await {
                    state
                        .metrics
                        .persistence_write_failures_total
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(format!("Failed to store message synchronously: {e}"));
                }
                state
                    .metrics
                    .messages_persisted_total
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                // Asynchronous persistence
                let overflow =
                    if msg.topic.starts_with("_control.") || msg.topic.starts_with("_decision.") {
                        OverflowPolicy::BlockPublisher
                    } else {
                        OverflowPolicy::DropOldest
                    };

                if let Err(e) = state.pipeline.queue_message(pm, overflow).await {
                    state
                        .metrics
                        .dropped_persistence_messages_total
                        .fetch_add(1, Ordering::Relaxed);
                    state
                        .metrics
                        .overflow_events_total
                        .fetch_add(1, Ordering::Relaxed);
                    tracing::warn!("Persistence queue full: {}", e);
                }
            }
        }

        if policy.mode == PersistenceMode::Queryable {
            let im = IndexableMessage {
                message: msg.clone(),
            };
            let _ = state.pipeline.queue_index(im).await;
        }

        Ok(msg)
    })
}

pub async fn publish_message(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Json(msg): Json<Message>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !ctx.can_write(&msg.topic) {
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Principal is not authorized to publish on topic '{}'",
                msg.topic
            ),
        ));
    }
    match handle_publish(std::sync::Arc::new(state), msg).await {
        Ok(m) => Ok((StatusCode::OK, Json(m))),
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

fn filter_readable(msgs: Vec<Message>, ctx: &crate::auth::AuthContext) -> Vec<Message> {
    if ctx.principal.is_none() {
        return msgs;
    }
    msgs.into_iter()
        .filter(|m| ctx.can_read(&m.topic))
        .collect()
}

pub async fn get_history(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Query(params): Query<HistoryParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(100);
    match state.store.get_history(limit) {
        Ok(msgs) => Ok(Json(filter_readable(msgs, &ctx))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fetch history: {e}"),
        )),
    }
}

pub async fn search_messages(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Query(params): Query<SearchParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.search_messages(&params.query) {
        Ok(msgs) => Ok(Json(filter_readable(msgs, &ctx))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to search messages: {e}"),
        )),
    }
}

pub async fn list_topics(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.list_topics() {
        Ok(topics) => Ok(Json(topics)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list topics: {e}"),
        )),
    }
}

pub async fn get_forum(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Query(params): Query<ForumParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let pat = params.pattern.unwrap_or_else(|| "**".to_string());
    match state.store.get_history(200) {
        Ok(msgs) => {
            let filtered: Vec<Message> = msgs
                .into_iter()
                .filter(|m| {
                    let is_eligible = !m.topic.starts_with("_trace.")
                        && !m.topic.starts_with("_system.")
                        && !m.topic.contains(".scratch.")
                        && !m.topic.contains(".heartbeat")
                        && !m.topic.contains(".stream");
                    is_eligible
                        && mellowmesh_core::topic::match_topic(&pat, &m.topic)
                        && ctx.can_read(&m.topic)
                })
                .collect();
            Ok(Json(filtered))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fetch history for forum: {e}"),
        )),
    }
}
