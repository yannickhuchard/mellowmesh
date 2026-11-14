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
    lease_seconds: Option<u64>,
}

pub async fn create_task(
    State(state): State<AppState>,
    Json(mut task): Json<Task>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if task.id.is_empty() {
        task.id = format!("task_{}", Ulid::new().to_string().to_lowercase());
    }
    // A newly created task must start in a coordinatable state. Reject an
    // arbitrary status (e.g. a client-supplied "in_progress") that would leave
    // the task neither claimable nor sweepable.
    if !matches!(task.status.as_str(), "open" | "blocked") {
        task.status = "open".to_string();
    }
    if !matches!(
        task.priority.as_str(),
        "low" | "medium" | "high" | "critical"
    ) {
        task.priority = "medium".to_string();
    }
    match state.store.insert_task(&task) {
        Ok(_) => Ok((StatusCode::OK, Json(task))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create task: {e}"),
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
            format!("Failed to list tasks: {e}"),
        )),
    }
}

pub async fn claim_task(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Path(task_id): Path<String>,
    Json(payload): Json<ClaimPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use mellowmesh_store::task_store::ClaimOutcome;

    // An authenticated agent may only claim as itself — no impersonation.
    if let Some(p) = &ctx.principal {
        if p.kind == "agent" && p.id != payload.claimed_by {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "Agent {} cannot claim a task as {}",
                    p.id, payload.claimed_by
                ),
            ));
        }
    }
    match state
        .store
        .claim_task(&task_id, &payload.claimed_by, payload.lease_seconds)
    {
        Ok(ClaimOutcome::Claimed { lease_expires_at }) => Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "lease_expires_at": lease_expires_at })),
        )),
        Ok(ClaimOutcome::Conflict { held_by }) => Err((
            StatusCode::CONFLICT,
            format!("Task {task_id} is already claimed by {held_by} and its lease has not expired"),
        )),
        Ok(ClaimOutcome::NotFound) => {
            Err((StatusCode::NOT_FOUND, format!("Task {task_id} not found")))
        }
        Ok(ClaimOutcome::NotClaimable { status }) => Err((
            StatusCode::BAD_REQUEST,
            format!("Task {task_id} is not claimable (status: {status})"),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to claim task: {e}"),
        )),
    }
}

pub async fn complete_task(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<crate::auth::AuthContext>,
    Path(task_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let task = match state.store.get_task(&task_id) {
        Ok(Some(t)) => t,
        Ok(None) => return Err((StatusCode::NOT_FOUND, format!("Task {task_id} not found"))),
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Task lookup failed: {e}"),
            ))
        }
    };

    // An authenticated agent may only complete a task it currently holds; it
    // cannot close out another agent's work. Humans/interfaces/owners are not
    // restricted (they supervise the fleet).
    if let Some(p) = &ctx.principal {
        if p.kind == "agent" {
            match &task.claimed_by {
                Some(holder) if holder == &p.id => {}
                _ => {
                    return Err((
                        StatusCode::FORBIDDEN,
                        format!("Agent {} does not hold task {task_id}", p.id),
                    ))
                }
            }
        }
    }

    match state.store.complete_task(&task_id) {
        Ok(true) => Ok(StatusCode::OK),
        Ok(false) => Err((
            StatusCode::CONFLICT,
            format!("Task {task_id} is already in a terminal state"),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to complete task: {e}"),
        )),
    }
}
