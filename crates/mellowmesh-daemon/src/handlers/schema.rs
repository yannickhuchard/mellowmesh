use crate::auth::AuthContext;
use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
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

/// Schema contracts gate every publish on their topics, so changing them is
/// an owner-only action.
fn require_admin(state: &AppState, ctx: &AuthContext) -> Result<(), (StatusCode, String)> {
    if crate::auth::is_admin(state, ctx) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Only the owner may manage schema contracts".to_string(),
        ))
    }
}

pub async fn add_schema(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(payload): Json<AddSchemaPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_admin(&state, &ctx)?;
    crate::handlers::message::check_pattern(&payload.topic_pattern)?;
    // Compile now so an invalid schema is rejected at registration instead
    // of failing every later publish on its topics.
    if let Err(e) = crate::cache::compile_schema(&payload.schema_content) {
        return Err((StatusCode::BAD_REQUEST, e));
    }

    let schema = TopicSchema {
        topic_pattern: payload.topic_pattern,
        version: payload.version,
        schema_content: payload.schema_content,
        status: "active".to_string(),
        created_at: Utc::now(),
    };

    let result = state.store.insert_schema(&schema);
    state.caches.schemas.invalidate();
    match result {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to save schema: {e}"),
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
            format!("Failed to list schemas: {e}"),
        )),
    }
}

pub async fn remove_schema(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Query(params): Query<RemoveSchemaParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_admin(&state, &ctx)?;
    let result = state
        .store
        .remove_schema(&params.topic_pattern, &params.version);
    state.caches.schemas.invalidate();
    match result {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to remove schema: {e}"),
        )),
    }
}

pub async fn set_schema_status(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(payload): Json<SetSchemaStatusPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_admin(&state, &ctx)?;
    if payload.status != "active" && payload.status != "paused" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Status must be either 'active' or 'paused'".to_string(),
        ));
    }

    let result =
        state
            .store
            .set_schema_status(&payload.topic_pattern, &payload.version, &payload.status);
    state.caches.schemas.invalidate();
    match result {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to update schema status: {e}"),
        )),
    }
}
