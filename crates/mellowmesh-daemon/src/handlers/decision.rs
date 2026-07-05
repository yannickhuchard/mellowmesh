use crate::auth::AuthContext;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use mellowmesh_core::decision::Decision;
use mellowmesh_core::message::Message;
use serde::Deserialize;
use std::sync::Arc;
use ulid::Ulid;

#[derive(Deserialize)]
pub struct ResponsePayload {
    option_id: String,
    /// Optional attribution hint from interface connectors relaying a
    /// human's answer (e.g. `telegram://12345` or a mapped `human://` id).
    #[serde(default)]
    responded_by: Option<String>,
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

            // Announce on the fabric so interface connectors (Telegram,
            // Discord, ...) can offer approve/reject where the human is.
            let event = Message {
                id: String::new(),
                topic: format!("_decision.{}.requested", decision.id),
                from: decision.created_by.clone(),
                owner: Some(decision.required_decider.clone()),
                timestamp: Utc::now(),
                content_type: "application/json".to_string(),
                body: format!("Decision requested: {}", decision.title),
                headers: None,
                payload: serde_json::to_value(&decision).ok(),
                parent_id: None,
            };
            if let Err(e) =
                crate::handlers::message::handle_publish(Arc::new(state.clone()), event).await
            {
                tracing::warn!("Failed to announce decision {}: {}", decision.id, e);
            }

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
    // Decision integrity:
    // - humans answer directly;
    // - interface principals (Telegram/Discord connectors, ...) may relay a
    //   human's answer, recording who tapped through which interface;
    // - agents and nodes can NEVER answer — an agent cannot approve its own
    //   proposal.
    let responded_by = match &ctx.principal {
        Some(p) if p.kind == "human" => p.id.clone(),
        Some(p) if p.kind == "interface" => {
            let human = payload
                .responded_by
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            format!("{} (via {})", human, p.id)
        }
        Some(p) => {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "Only human principals (or interfaces relaying them) may respond to decisions ({} is a {})",
                    p.id, p.kind
                ),
            ));
        }
        // Open mode: localhost is trusted, but the audit trail records that
        // the response was unauthenticated.
        None => payload
            .responded_by
            .clone()
            .unwrap_or_else(|| "human://local-unauthenticated".to_string()),
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
