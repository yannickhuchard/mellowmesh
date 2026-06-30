use crate::state::AppState;
use axum::{extract::State, response::IntoResponse, Json};

pub async fn get_metrics(State(state): State<AppState>) -> impl IntoResponse {
    let p_q_depth = state.pipeline.persist_queue_depth();
    let idx_q_depth = state.pipeline.index_queue_depth();
    let sub_q_depth = state.registry.total_queue_depth();

    let snapshot = state.metrics.snapshot(p_q_depth, idx_q_depth, sub_q_depth);
    Json(snapshot)
}
