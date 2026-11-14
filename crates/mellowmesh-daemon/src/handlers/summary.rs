use crate::auth::AuthContext;
use crate::handlers::message::{check_pattern, MAX_LIMIT};
use crate::state::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
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
    Extension(ctx): Extension<AuthContext>,
    Json(payload): Json<StoreSummaryPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    check_pattern(&payload.topic)?;
    if !ctx.can_write(&payload.topic) {
        return Err((
            StatusCode::FORBIDDEN,
            format!(
                "Principal is not authorized to write topic '{}'",
                payload.topic
            ),
        ));
    }
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
    Extension(ctx): Extension<AuthContext>,
    Query(params): Query<ContextParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    check_pattern(&params.topic)?;
    let query = ContextQuery {
        topic: params.topic,
        limit: params.limit.unwrap_or(20).min(MAX_LIMIT),
    };
    match state.store.get_context(query).await {
        Ok(mut result) => {
            // Only what the caller may read: summaries, matching messages
            // and their lineage are all filtered by read scope.
            if ctx.principal.is_some() {
                result.summaries.retain(|s| ctx.can_read(&s.topic));
                result.relevant_messages.retain(|m| ctx.can_read(&m.topic));
                if let Some(lineage) = result.lineage.as_mut() {
                    lineage.retain(|m| ctx.can_read(&m.topic));
                }
            }
            Ok((StatusCode::OK, Json(result)))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to get topic context: {e}"),
        )),
    }
}
