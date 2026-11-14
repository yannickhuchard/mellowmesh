use crate::auth::AuthContext;
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Extension, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityMappingRequest {
    pub external_id: String,
    pub mellowmesh_id: String,
}

/// `POST /identity-mappings` — owner-only. A mapping lets an interface relay
/// a human's decision answer, so it may only point at a `human://` identity
/// (and, under authentication, at an existing human principal).
pub async fn create_mapping(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(payload): Json<IdentityMappingRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !crate::auth::is_admin(&state, &ctx) {
        return Err((
            StatusCode::FORBIDDEN,
            "Only the owner may manage identity mappings".to_string(),
        ));
    }
    if payload.external_id.is_empty() || payload.mellowmesh_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "external_id and mellowmesh_id cannot be empty".to_string(),
        ));
    }
    if !payload.mellowmesh_id.starts_with("human://") {
        return Err((
            StatusCode::BAD_REQUEST,
            "mellowmesh_id must be a human:// identity".to_string(),
        ));
    }
    if state.require_auth {
        let known_human = state
            .store
            .get_principal(&payload.mellowmesh_id)
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Principal lookup failed: {e}"),
                )
            })?
            .is_some_and(|p| p.kind == "human");
        if !known_human {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{} is not a known human principal", payload.mellowmesh_id),
            ));
        }
    }
    match state
        .store
        .insert_identity_mapping(&payload.external_id, &payload.mellowmesh_id)
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to insert mapping: {e}"),
        )),
    }
}

pub async fn list_mappings(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.list_identity_mappings() {
        Ok(mappings) => {
            let list: Vec<IdentityMappingRequest> = mappings
                .into_iter()
                .map(|(external_id, mellowmesh_id)| IdentityMappingRequest {
                    external_id,
                    mellowmesh_id,
                })
                .collect();
            Ok(Json(list))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list mappings: {e}"),
        )),
    }
}
