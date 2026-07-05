use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use mellowmesh_core::message::Message;
use mellowmesh_core::topic::NamedTopic;
use std::collections::HashMap;

pub async fn register_named_topic(
    State(state): State<AppState>,
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

    match state.store.register_named_topic(&named_topic) {
        Ok(_) => {
            // Publish registry sync event message
            let mut headers = HashMap::new();
            headers.insert("x-registry-action".to_string(), "register".to_string());

            let msg = Message {
                id: format!("msg_{}", ulid::Ulid::new().to_string().to_lowercase()),
                topic: "_system.registry.named_topic".to_string(),
                from: format!("agent://{}/daemon", state.node_id),
                owner: None,
                timestamp: chrono::Utc::now(),
                content_type: "application/json".to_string(),
                body: serde_json::to_string(&named_topic).unwrap_or_default(),
                headers: Some(headers),
                payload: None,
                parent_id: None,
            };
            let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state), msg).await;

            Ok(StatusCode::OK)
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to register named topic: {e}"),
        )),
    }
}

pub async fn list_named_topics(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.list_named_topics() {
        Ok(topics) => Ok(Json(topics)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list named topics: {e}"),
        )),
    }
}

pub async fn remove_named_topic(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Name cannot be empty".to_string()));
    }

    match state.store.remove_named_topic(&name) {
        Ok(_) => {
            // Publish registry sync event message
            let mut headers = HashMap::new();
            headers.insert("x-registry-action".to_string(), "remove".to_string());

            let msg = Message {
                id: format!("msg_{}", ulid::Ulid::new().to_string().to_lowercase()),
                topic: "_system.registry.named_topic".to_string(),
                from: format!("agent://{}/daemon", state.node_id),
                owner: None,
                timestamp: chrono::Utc::now(),
                content_type: "text/plain".to_string(),
                body: name,
                headers: Some(headers),
                payload: None,
                parent_id: None,
            };
            let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state), msg).await;

            Ok(StatusCode::OK)
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to remove named topic: {e}"),
        )),
    }
}
