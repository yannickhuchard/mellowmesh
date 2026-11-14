use crate::auth::AuthContext;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::message::Message;
use std::collections::HashMap;

pub async fn register_agent(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(agent): Json<AgentRegistration>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if agent.id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Agent ID cannot be empty".to_string(),
        ));
    }
    // A non-human principal may only register itself; humans (and the
    // owner) may register any agent.
    if let Some(p) = &ctx.principal {
        if p.kind != "human" && p.id != agent.id {
            return Err((
                StatusCode::FORBIDDEN,
                format!("{} {} may only register itself", p.kind, p.id),
            ));
        }
    }
    match state.store.register_agent(&agent) {
        Ok(_) => {
            state.caches.agents.invalidate();
            // Publish registry sync event message
            let mut headers = HashMap::new();
            headers.insert("x-registry-action".to_string(), "register".to_string());

            let msg = Message {
                id: format!("msg_{}", ulid::Ulid::new().to_string().to_lowercase()),
                topic: "_system.registry.agent".to_string(),
                from: format!("agent://{}/daemon", state.node_id),
                owner: None,
                timestamp: chrono::Utc::now(),
                content_type: "application/json".to_string(),
                body: serde_json::to_string(&agent).unwrap_or_default(),
                headers: Some(headers),
                payload: None,
                parent_id: None,
            };
            let _ = crate::handlers::message::handle_publish(std::sync::Arc::new(state), msg).await;

            Ok(StatusCode::OK)
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to register agent: {e}"),
        )),
    }
}

pub async fn list_agents(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let store = state.store.clone();
    match state.caches.agents.get_or_load(|| store.list_agents()) {
        Ok(agents) => Ok(Json(agents.as_ref().clone())),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list agents: {e}"),
        )),
    }
}
