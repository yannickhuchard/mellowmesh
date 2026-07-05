use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use mellowmesh_core::decision::Decision;
use serde::Deserialize;
use ulid::Ulid;

#[derive(Deserialize)]
pub struct ResponsePayload {
    option_id: String,
}

pub async fn create_decision(
    State(state): State<AppState>,
    Json(mut decision): Json<Decision>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if decision.id.is_empty() {
        decision.id = format!("decision_{}", Ulid::new().to_string().to_lowercase());
    }
    if decision.status.is_empty() {
        decision.status = "requested".to_string();
    }
    match state.store.insert_decision(&decision) {
        Ok(_) => Ok((StatusCode::OK, Json(decision))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create decision: {e}"),
        )),
    }
}

pub async fn list_decisions(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.list_decisions() {
        Ok(decisions) => Ok(Json(decisions)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list decisions: {e}"),
        )),
    }
}

pub async fn respond_decision(
    State(state): State<AppState>,
    Path(decision_id): Path<String>,
    Json(payload): Json<ResponsePayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state
        .store
        .respond_decision(&decision_id, &payload.option_id)
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to respond to decision: {e}"),
        )),
    }
}
