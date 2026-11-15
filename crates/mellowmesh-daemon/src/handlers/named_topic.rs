use crate::auth::AuthContext;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use mellowmesh_core::message::Message;
use mellowmesh_core::topic::NamedTopic;
use std::collections::HashMap;

fn registry_event(state: &AppState, action: &str, content_type: &str, body: String) -> Message {
    let mut headers = HashMap::new();
    headers.insert("x-registry-action".to_string(), action.to_string());
    Message {
        id: format!("msg_{}", ulid::Ulid::new().to_string().to_lowercase()),
        topic: "_system.registry.named_topic".to_string(),
        from: format!("agent://{}/daemon", state.node_id),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: content_type.to_string(),
        body,
        headers: Some(headers),
        payload: None,
        parent_id: None,
    }
}

pub async fn register_named_topic(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(named_topic): Json<NamedTopic>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if named_topic.name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Named topic name cannot be empty".to_string(),
        ));
    }
    if named_topic.topic.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Named topic target path cannot be empty".to_string(),
        ));
    }
    crate::handlers::message::check_pattern(&named_topic.topic)?;
    // Mapping a name onto a topic routes messages there, so it requires
    // write access to the mapped topic — and to the topic it replaces.
    if !ctx.can_write(&state, &named_topic.topic) {
        return Err((
            StatusCode::FORBIDDEN,
            format!("Not authorized to write topic '{}'", named_topic.topic),
        ));
    }
    if let Some(existing) = find_named_topic(&state, &named_topic.name)? {
        if !ctx.can_write(&state, &existing.topic) {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Not authorized to write topic '{}'", existing.topic),
            ));
        }
    }

    match state.store.register_named_topic(&named_topic) {
        Ok(_) => {
            state.caches.named_topics.invalidate();
            let msg = registry_event(
                &state,
                "register",
                "application/json",
                serde_json::to_string(&named_topic).unwrap_or_default(),
            );
            let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state), msg).await;

            Ok(StatusCode::OK)
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to register named topic: {e}"),
        )),
    }
}

fn find_named_topic(
    state: &AppState,
    name: &str,
) -> Result<Option<NamedTopic>, (StatusCode, String)> {
    let store = state.store.clone();
    let all = state
        .caches
        .named_topics
        .get_or_load(|| store.list_named_topics())
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to list named topics: {e}"),
            )
        })?;
    Ok(all.iter().find(|t| t.name == name).cloned())
}

pub async fn list_named_topics(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = state.store.clone();
    match state
        .caches
        .named_topics
        .get_or_load(|| store.list_named_topics())
    {
        Ok(topics) => Ok(Json(topics.as_ref().clone())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list named topics: {e}"),
        )),
    }
}

pub async fn remove_named_topic(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Name cannot be empty".to_string()));
    }
    if let Some(existing) = find_named_topic(&state, &name)? {
        if !ctx.can_write(&state, &existing.topic) {
            return Err((
                StatusCode::FORBIDDEN,
                format!("Not authorized to write topic '{}'", existing.topic),
            ));
        }
    }

    match state.store.remove_named_topic(&name) {
        Ok(_) => {
            state.caches.named_topics.invalidate();
            let msg = registry_event(&state, "remove", "text/plain", name);
            let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state), msg).await;

            Ok(StatusCode::OK)
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to remove named topic: {e}"),
        )),
    }
}
