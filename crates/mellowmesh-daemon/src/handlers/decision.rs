use crate::auth::AuthContext;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
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
        Ok(_) => {
            // Phase 2 reach layer: surface the pending decision to the human.
            crate::notify::notify_decision_requested(&decision);
            Ok((StatusCode::OK, Json(decision)))
        }
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
    Extension(ctx): Extension<AuthContext>,
    Path(decision_id): Path<String>,
    Json(payload): Json<ResponsePayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Decision integrity: an authenticated principal must be a human to
    // answer a decision — agents can never approve their own proposals.
    let responded_by = match &ctx.principal {
        Some(p) if p.kind != "human" => {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "Only human principals may respond to decisions ({} is a {})",
                    p.id, p.kind
                ),
            ));
        }
        Some(p) => p.id.clone(),
        // Open mode: localhost is trusted, but the audit trail records that
        // the response was unauthenticated.
        None => "human://local-unauthenticated".to_string(),
    };

    match state
        .store
        .respond_decision(&decision_id, &payload.option_id, Some(&responded_by))
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to respond to decision: {e}"),
        )),
    }
}
