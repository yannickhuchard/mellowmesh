use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityMappingRequest {
    pub external_id: String,
    pub mellowmesh_id: String,
}

pub async fn create_mapping(
    State(state): State<AppState>,
    Json(payload): Json<IdentityMappingRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if payload.external_id.is_empty() || payload.mellowmesh_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "external_id and mellowmesh_id cannot be empty".to_string(),
        ));
    }
    match state
        .store
        .insert_identity_mapping(&payload.external_id, &payload.mellowmesh_id)
    {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to insert mapping: {}", e),
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
            format!("Failed to list mappings: {}", e),
        )),
    }
}
