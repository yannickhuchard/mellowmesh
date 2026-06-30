use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use mellowmesh_core::task::Task;
use serde::Deserialize;
use ulid::Ulid;

#[derive(Deserialize)]
pub struct ClaimPayload {
    claimed_by: String,
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(mut task): Json<Task>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if task.id.is_empty() {
        task.id = format!("task_{}", Ulid::new().to_string().to_lowercase());
    }
    if task.status.is_empty() {
        task.status = "open".to_string();
    }
    match state.store.insert_task(&task) {
        Ok(_) => Ok((StatusCode::OK, Json(task))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create task: {}", e),
        )),
    }
}

pub async fn list_tasks(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.list_tasks() {
        Ok(tasks) => Ok(Json(tasks)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list tasks: {}", e),
        )),
    }
}

pub async fn claim_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(payload): Json<ClaimPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.claim_task(&task_id, &payload.claimed_by) {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to claim task: {}", e),
        )),
    }
}

pub async fn complete_task(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.complete_task(&task_id) {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to complete task: {}", e),
        )),
    }
}
