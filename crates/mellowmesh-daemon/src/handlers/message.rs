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

/// Hard cap on any history-style listing.
pub const MAX_LIMIT: usize = 1000;

#[derive(Deserialize)]
pub struct HistoryParams {
    limit: Option<usize>,
    /// Optional topic pattern (`*`, `**`, `>`) to filter history by.
    pattern: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    query: String,
}

#[derive(Deserialize)]
pub struct ForumParams {
    pattern: Option<String>,
}

/// Topics that only the hub itself publishes on. Clients may not forge
/// decision outcomes, system/registry events, or task lifecycle events
/// (`_task.<id>.progress` stays open: it is the claim-lease heartbeat).
const RESERVED_PATTERNS: &[&str] = &[
    "_decision.**",
    "_system.**",
    "_task.*.reclaimed",
    "_task.*.created",
    "_task.*.claimed",
    "_task.*.started",
    "_task.*.completed",
    "_task.*.updated",
    "_task.*.cancelled",
];

/// True when `topic` is in a hub-reserved namespace (case-insensitively,
/// since topic names are case-insensitive).
pub fn is_reserved_topic(topic: &str) -> bool {
    RESERVED_PATTERNS
        .iter()
        .any(|p| mellowmesh_core::topic::match_topic_with_options(p, topic, true))
}

/// Validate a user-supplied topic pattern, mapping failure to 400.
pub fn check_pattern(pattern: &str) -> Result<(), (StatusCode, String)> {
    mellowmesh_core::topic::validate_pattern(pattern)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid pattern: {e}")))
}

/// Parse a size such as `4KB`, `512B`, `1MB` or `2048` into bytes
/// (binary multiples). Returns `None` when unparseable.
pub fn parse_size(s: &str) -> Option<usize> {
    let s = s.trim().to_ascii_uppercase();
    let split = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let num: usize = num.parse().ok()?;
    let mult: usize = match unit.trim() {
        "" | "B" => 1,
        "K" | "KB" | "KIB" => 1 << 10,
        "M" | "MB" | "MIB" => 1 << 20,
        "G" | "GB" | "GIB" => 1 << 30,
        _ => return None,
    };
    num.checked_mul(mult)
}

/// Size of a message's content for `max_message_size` enforcement: the
/// body plus the serialized structured payload.
fn message_size(msg: &Message) -> usize {
    let payload = msg
        .payload
        .as_ref()
        .map(|p| serde_json::to_vec(p).map(|v| v.len()).unwrap_or(0))
        .unwrap_or(0);
    msg.body.len().saturating_add(payload)
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

        if let Err(e) = mellowmesh_core::topic::Topic::new(&msg.topic) {
            return Err(format!("Invalid topic: {e}"));
        }

        // Resolve persistence policy and enforce its size limit before any
        // side effect (registry sync, mention routing, broadcast).
        let policy = state.policy_config.resolve(&msg.topic).clone();
        if let Some(max) = policy.max_message_size.as_deref() {
            match parse_size(max) {
                Some(limit) => {
                    let size = message_size(&msg);
                    if size > limit {
                        state
                            .metrics
                            .publisher_rejections_total
                            .fetch_add(1, Ordering::Relaxed);
                        return Err(format!(
                            "Message too large for topic '{}': {size} bytes (max {max})",
                            msg.topic
                        ));
                    }
                }
                None => tracing::warn!(
                    "Ignoring unparseable max_message_size '{}' for topic '{}'",
                    max,
                    msg.topic
                ),
            }
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
                        state.caches.named_topics.invalidate();
                    }
                }
                "remove" => {
                    let name = msg.body.trim();
                    if !name.is_empty() {
                        let _ = state.store.remove_named_topic(name);
                        state.caches.named_topics.invalidate();
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
                    state.caches.agents.invalidate();
                }
            }
        }

        // Parse mentions and route directed inbox copies. Visitor posts are
        // never routed into anyone's inbox.
        let header_is_true = |name: &str| {
            msg.headers
                .as_ref()
                .and_then(|h| h.get(name))
                .is_some_and(|v| v == "true")
        };
        let is_routed =
            header_is_true("x-mellowmesh-routed") || header_is_true(crate::access::VISITOR_HEADER);

        if !is_routed
            && !msg.topic.starts_with("_agent.")
            && (msg.topic.starts_with("_forum.") || msg.topic.starts_with("_project."))
        {
            let store = &state.store;
            if let Ok(agents) = state.caches.agents.get_or_load(|| store.list_agents()) {
                let named_topics = state
                    .caches
                    .named_topics
                    .get_or_load(|| store.list_named_topics())
                    .unwrap_or_default();
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
                            // An invited AI's mention reaches a mailbox only
                            // where the owner's mail policy lets it write.
                            if !crate::mail_policy::may_route(&state, &msg.from, &inbox_topic) {
                                continue;
                            }

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
                        } else if mention_uri.starts_with("human://") {
                            // A human-directed mention triggers a desktop
                            // notification so the human-in-the-loop actually
                            // sees it, instead of it dead-ending in a header.
                            crate::notify::notify_human_mention(
                                &mention_uri,
                                &msg.from,
                                &msg.topic,
                                &msg.body,
                            );
                        }
                    }
                }
            }
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

        // JSON Schema Contract Validation (compiled schemas are cached).
        let all_schemas = match state.caches.load_schemas(&state.store) {
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
        let matched_schemas: Vec<&crate::cache::CompiledSchema> = all_schemas
            .iter()
            .filter(|s| {
                s.schema.status == "active"
                    && mellowmesh_core::topic::match_topic(&s.schema.topic_pattern, &msg.topic)
            })
            .collect();

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
                matched_schemas.iter().find(|s| &s.schema.version == ver)
            } else {
                matched_schemas.first()
            };

            match schema_to_validate {
                Some(compiled) => {
                    let schema = &compiled.schema;
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

                    let compiled_schema = match &compiled.validator {
                        Ok(s) => s.clone(),
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

        // Broadcast in-memory (Hot path)
        state.registry.broadcast(&msg);
        // Answers, decisions and mentions that go out by email.
        crate::email::observe(&state, &msg);
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

/// True for `_task.<id>.progress` (a claim-lease heartbeat).
fn is_task_progress(topic: &str) -> bool {
    topic
        .strip_prefix("_task.")
        .and_then(|rest| rest.strip_suffix(".progress"))
        .map(|id| !id.is_empty() && !id.contains('.'))
        .unwrap_or(false)
}

/// `POST /publish` — the client entry point. Internal publishers call
/// [`handle_publish`] directly and are not subject to these checks.
pub async fn publish_message(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Json(mut msg): Json<Message>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Hub-reserved namespaces: decision outcomes, system/registry events and
    // lease reclaims are only ever published by the hub itself.
    if is_reserved_topic(&msg.topic) {
        state
            .metrics
            .publisher_rejections_total
            .fetch_add(1, Ordering::Relaxed);
        return Err((
            StatusCode::FORBIDDEN,
            format!("Topic '{}' is reserved for the hub", msg.topic),
        ));
    }

    // A bound Mellow's outbox is its voice: only the hub publishes there
    // (after a hosting session's mellow_reply), so nobody can speak as it.
    if crate::mellows::is_bound_outbox(&msg.topic) {
        return Err((
            StatusCode::FORBIDDEN,
            "Only the session hosting this Mellow speaks for it (mellow_reply)".to_string(),
        ));
    }

    use mellowmesh_core::network::WriteAccess;
    let write = ctx.access(&state, &msg.topic).write;
    if write == WriteAccess::Denied {
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Principal is not authorized to publish on topic '{}'",
                msg.topic
            ),
        ));
    }

    // Identity and time are assigned by the hub, never by the publisher.
    msg.id = format!("msg_{}", Ulid::new().to_string().to_lowercase());
    msg.timestamp = Utc::now();

    // Routing headers are set by the daemon, never by a publisher: a client
    // must not be able to suppress mention routing or forge a "routed" copy
    // into another agent's inbox.
    if let Some(h) = msg.headers.as_mut() {
        h.remove("x-mellowmesh-routed");
        h.remove("x-mentions");
        h.remove(crate::access::VISITOR_HEADER);
    }

    if write == WriteAccess::Visitor {
        // A visitor post (open content with visitor_write): recorded as the
        // visitor, never as a member, and never routed to inboxes.
        let who = ctx.principal.as_ref().map(|p| p.id.clone());
        msg.from = who
            .clone()
            .unwrap_or_else(|| mellowmesh_core::network::VISITOR_PRINCIPAL.to_string());
        msg.owner = who;
        msg.headers.get_or_insert_with(Default::default).insert(
            crate::access::VISITOR_HEADER.to_string(),
            "true".to_string(),
        );
    } else if let Some(p) = &ctx.principal {
        // Bind provenance to the authenticated principal so a message's
        // origin can be trusted for audit.
        msg.owner = Some(p.id.clone());
        // Agents, humans and nodes publish as themselves. Interfaces relay
        // external identities (e.g. `telegram://123`) and keep `from`.
        if matches!(p.kind.as_str(), "agent" | "human" | "node") {
            msg.from = p.id.clone();
        }

        // A claim-lease heartbeat (publishing on `_task.<id>.progress`) may
        // only renew the lease of the principal that is actually
        // authenticated — no non-human principal can keep another's lease
        // alive by spoofing `from`.
        if p.kind != "human" && is_task_progress(&msg.topic) && msg.from != p.id {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "{} {} cannot publish a task heartbeat as {}",
                    p.kind, p.id, msg.from
                ),
            ));
        }
    }

    // Only the authenticated owner may set a Mellow in motion: a turn spends
    // the owner's own AI session, so no other publisher (a visitor, another
    // agent, a spoofed `from`) can trigger one.
    let owner_speaks = crate::auth::is_admin(&state, &ctx);
    let state = std::sync::Arc::new(state);
    match handle_publish(state.clone(), msg).await {
        Ok(m) => {
            if owner_speaks {
                crate::mellows::maybe_dispatch(&state, &m);
            }
            Ok((StatusCode::OK, Json(m)))
        }
        Err(e) => Err((StatusCode::BAD_REQUEST, e)),
    }
}

/// Run a blocking store query on the blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f).await?
}

pub async fn get_history(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Query(params): Query<HistoryParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let limit = params.limit.unwrap_or(100).min(MAX_LIMIT);
    if let Some(p) = &params.pattern {
        check_pattern(p)?;
    }
    let store = state.store.clone();
    let result = blocking(move || match params.pattern {
        Some(p) => store.get_history_matching(&p, limit),
        None => store.get_history(limit),
    })
    .await;
    match result {
        Ok(msgs) => Ok(Json(crate::access::filter_messages(&state, &ctx, msgs))),
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
    if params.query.trim().is_empty() {
        return Ok(Json(Vec::new()));
    }
    let store = state.store.clone();
    match blocking(move || store.search_messages(&params.query)).await {
        Ok(msgs) => Ok(Json(crate::access::filter_messages(&state, &ctx, msgs))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to search messages: {e}"),
        )),
    }
}

pub async fn list_topics(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = state.store.clone();
    match blocking(move || store.list_topics()).await {
        Ok(topics) => {
            // A caller only sees topics it is allowed to read, so topic
            // enumeration can't be used to map the whole namespace.
            let snap = crate::access::snapshot(&state);
            let caller = ctx.caller();
            let visible: Vec<String> = topics
                .into_iter()
                .filter(|t| snap.can_read(&caller, t))
                .collect();
            Ok(Json(visible))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list topics: {e}"),
        )),
    }
}

/// Messages shown in the forum view.
const FORUM_LIMIT: usize = 200;

pub async fn get_forum(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Query(params): Query<ForumParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let pat = params.pattern.unwrap_or_else(|| "**".to_string());
    check_pattern(&pat)?;
    let store = state.store.clone();
    // Over-fetch matching messages, since telemetry/system topics are
    // filtered out below.
    let result = blocking(move || store.get_history_matching(&pat, MAX_LIMIT)).await;
    match result {
        Ok(msgs) => {
            let snap = crate::access::snapshot(&state);
            let caller = ctx.caller();
            let mut filtered: Vec<Message> = msgs
                .into_iter()
                .filter(|m| {
                    let is_eligible = !m.topic.starts_with("_trace.")
                        && !m.topic.starts_with("_system.")
                        && !m.topic.contains(".scratch.")
                        && !m.topic.contains(".heartbeat")
                        && !m.topic.contains(".stream");
                    is_eligible && crate::access::can_read_message(&snap, &caller, m)
                })
                .collect();
            // Keep the most recent FORUM_LIMIT, oldest first.
            if filtered.len() > FORUM_LIMIT {
                filtered.drain(..filtered.len() - FORUM_LIMIT);
            }
            Ok(Json(filtered))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to fetch history for forum: {e}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_topics() {
        assert!(is_reserved_topic("_decision.d1.responded"));
        assert!(is_reserved_topic("_DECISION.d1.responded"));
        assert!(is_reserved_topic("_system.presence.agent"));
        assert!(is_reserved_topic("_system.registry.agent"));
        assert!(is_reserved_topic("_task.t1.reclaimed"));
        assert!(!is_reserved_topic("_task.t1.progress"));
        assert!(!is_reserved_topic("_forum.general"));
        assert!(!is_reserved_topic("_decisions.x"));
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("4KB"), Some(4096));
        assert_eq!(parse_size("512"), Some(512));
        assert_eq!(parse_size("1 MB"), Some(1 << 20));
        assert_eq!(parse_size("10b"), Some(10));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(parse_size("99999999999999999999GB"), None);
    }
}
