use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use mellowmesh_core::persistence::{ContextQuery, MemoryStore, TopicSummary};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct StoreSummaryPayload {
    pub topic: String,
    pub summary: String,
}

#[derive(Deserialize)]
pub struct ContextParams {
    pub topic: String,
    pub limit: Option<usize>,
}

pub async fn store_summary(
    State(state): State<AppState>,
    Json(payload): Json<StoreSummaryPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let summary = TopicSummary {
        topic: payload.topic,
        summary: payload.summary,
        generated_at: Utc::now(),
    };
    match state.store.store_summary(summary).await {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to store topic summary: {e}"),
        )),
    }
}

pub async fn get_context(
    State(state): State<AppState>,
    Query(params): Query<ContextParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let query = ContextQuery {
        topic: params.topic,
        limit: params.limit.unwrap_or(20),
    };
    match state.store.get_context(query).await {
        Ok(result) => Ok((StatusCode::OK, Json(result))),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get topic context: {e}"),
        )),
    }
}
