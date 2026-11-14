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
    // A freshly created decision must be awaiting an answer. Reject a terminal
    // status at birth so a proposer can't ship a pre-"approved" decision.
    if !matches!(decision.status.as_str(), "requested" | "discussed") {
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
    // Decision integrity gate, applied BEFORE any decision lookup so an agent
    // learns nothing about which decisions exist:
    // - agents and nodes can NEVER answer — an agent cannot approve its own
    //   proposal.
    if let Some(p) = &ctx.principal {
        if p.kind != "human" && p.kind != "interface" {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "Only human principals (or interfaces relaying them) may respond to decisions ({} is a {})",
                    p.id, p.kind
                ),
            ));
        }
    }

    // Load the decision so we can validate the option, resolve the
    // approve/reject outcome, and enforce answer-once + no-self-approval.
    let decision = match state.store.get_decision(&decision_id) {
        Ok(Some(d)) => d,
        Ok(None) => {
            return Err((
                StatusCode::NOT_FOUND,
                format!("No decision with id {decision_id}"),
            ))
        }
        Err(e) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Decision lookup failed: {e}"),
            ))
        }
    };

    // Resolve who is answering:
    // - humans answer directly;
    // - interface principals (Telegram/Discord connectors, ...) may relay a
    //   human's answer, but ONLY for an external id that maps to a real
    //   `human://` principal — the human identity is never taken at face value
    //   from the request body;
    // - in open mode the response is recorded as unauthenticated and is never
    //   attributed to a caller-supplied human, so an agent cannot forge one.
    let responded_by = match &ctx.principal {
        Some(p) if p.kind == "human" => p.id.clone(),
        Some(p) if p.kind == "interface" => {
            let ext = payload.responded_by.clone().unwrap_or_default();
            let human = state
                .store
                .get_mellowmesh_id(&ext)
                .ok()
                .flatten()
                .filter(|h| h.starts_with("human://"))
                .or_else(|| ext.starts_with("human://").then(|| ext.clone()));
            match human {
                Some(h) => format!("{} (via {})", h, p.id),
                None => {
                    return Err((
                        StatusCode::FORBIDDEN,
                        format!(
                            "Interface {} may only relay a human:// identity that maps to a known principal (got {:?})",
                            p.id, payload.responded_by
                        ),
                    ))
                }
            }
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
        None => "human://local-unauthenticated".to_string(),
    };

    // No principal may answer its own proposal, in any auth mode.
    let responder_core = responded_by.split(" (via ").next().unwrap_or(&responded_by);
    if responder_core == decision.created_by {
        return Err((
            StatusCode::FORBIDDEN,
            "A principal cannot answer its own decision proposal".to_string(),
        ));
    }

    // Resolve the terminal status from the chosen option. An unknown option is
    // rejected rather than recorded, and a "reject" option resolves to
    // `rejected` — never silently to `approved`.
    let status = match decision.status_for_option(&payload.option_id) {
        Some(s) => s,
        None => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Unknown option '{}' for decision {}",
                    payload.option_id, decision_id
                ),
            ))
        }
    };

    match state.store.respond_decision(
        &decision_id,
        &payload.option_id,
        status,
        Some(&responded_by),
    ) {
        Ok(true) => {
            announce_decision_result(&state, &decision, &payload.option_id, status, &responded_by)
                .await;
            Ok(StatusCode::OK)
        }
        Ok(false) => Err((
            StatusCode::CONFLICT,
            format!("Decision {decision_id} has already been answered"),
        )),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to respond to decision: {e}"),
        )),
    }
}

/// Announce a decision's resolved outcome on `_decision.<id>.responded` so
/// agents waiting on the answer learn the status (approved/rejected/answered),
/// the chosen option, and who decided.
async fn announce_decision_result(
    state: &AppState,
    decision: &Decision,
    option_id: &str,
    status: &str,
    responded_by: &str,
) {
    let event = Message {
        id: String::new(),
        topic: format!("_decision.{}.responded", decision.id),
        from: responded_by.to_string(),
        owner: Some(decision.created_by.clone()),
        timestamp: Utc::now(),
        content_type: "application/json".to_string(),
        body: format!("Decision {} {}", decision.id, status),
        headers: None,
        payload: Some(serde_json::json!({
            "decision_id": decision.id,
            "status": status,
            "option_id": option_id,
            "responded_by": responded_by,
        })),
        parent_id: None,
    };
    if let Err(e) = crate::handlers::message::handle_publish(Arc::new(state.clone()), event).await {
        tracing::warn!("Failed to announce decision result {}: {}", decision.id, e);
    }
}
