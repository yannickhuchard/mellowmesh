use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use mellowmesh_core::persistence::TopicSchema;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct AddSchemaPayload {
    pub topic_pattern: String,
    pub version: String,
    pub schema_content: String,
}

#[derive(Deserialize)]
pub struct RemoveSchemaParams {
    pub topic_pattern: String,
    pub version: String,
}

#[derive(Deserialize)]
pub struct SetSchemaStatusPayload {
    pub topic_pattern: String,
    pub version: String,
    pub status: String,
}

pub async fn add_schema(
    State(state): State<AppState>,
    Json(payload): Json<AddSchemaPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if let Err(e) = serde_json::from_str::<serde_json::Value>(&payload.schema_content) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Invalid JSON Schema content: {}", e),
        ));
    }

    let schema = TopicSchema {
        topic_pattern: payload.topic_pattern,
        version: payload.version,
        schema_content: payload.schema_content,
        status: "active".to_string(),
        created_at: Utc::now(),
    };

    match state.store.insert_schema(&schema) {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save schema: {}", e),
        )),
    }
}

pub async fn list_schemas(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.list_schemas() {
        Ok(schemas) => Ok((StatusCode::OK, Json(schemas))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list schemas: {}", e),
        )),
    }
}

pub async fn remove_schema(
    State(state): State<AppState>,
    Query(params): Query<RemoveSchemaParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state
        .store
        .remove_schema(&params.topic_pattern, &params.version)
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to remove schema: {}", e),
        )),
    }
}

pub async fn set_schema_status(
    State(state): State<AppState>,
    Json(payload): Json<SetSchemaStatusPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if payload.status != "active" && payload.status != "paused" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Status must be either 'active' or 'paused'".to_string(),
        ));
    }

    match state
        .store
        .set_schema_status(&payload.topic_pattern, &payload.version, &payload.status)
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update schema status: {}", e),
        )),
    }
}
