use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::{Duration, Utc};
use mellowmesh_core::telemetry::{TraceLevel, TraceSession};
use serde::Deserialize;
use ulid::Ulid;

#[derive(Deserialize)]
pub struct EnableTraceParams {
    pub target: String,
    pub target_type: String, // e.g. "agent", "task", "flow", "topic"
    pub level: String,
    pub duration: String, // e.g. "15m", "1h"
    pub reason: Option<String>,
    pub enabled_by: String,
}

fn parse_duration(s: &str) -> Option<Duration> {
    let num_str: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    let num: i64 = num_str.parse().ok()?;
    let unit: String = s.chars().filter(|c| c.is_alphabetic()).collect();
    match unit.as_str() {
        "s" => Some(Duration::seconds(num)),
        "m" => Some(Duration::minutes(num)),
        "h" => Some(Duration::hours(num)),
        "d" => Some(Duration::days(num)),
        _ => None,
    }
}

pub async fn enable_trace(
    State(state): State<AppState>,
    Json(params): Json<EnableTraceParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let level = match params.level.as_str() {
        "off" => TraceLevel::Off,
        "status" => TraceLevel::Status,
        "progress" => TraceLevel::Progress,
        "structured" => TraceLevel::Structured,
        "verbose" => TraceLevel::Verbose,
        "cognitive" => TraceLevel::Cognitive,
        "raw" => TraceLevel::Raw,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("Invalid trace level: {}", params.level),
            ))
        }
    };

    let duration = parse_duration(&params.duration).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid duration format: {}", params.duration),
        )
    })?;

    let started_at = Utc::now();
    let expires_at = started_at + duration;

    let topic_level_str = format!("{:?}", level).to_lowercase();
    let trace_topic = format!(
        "_trace.{}.{}.{}",
        params.target_type,
        params.target.replace("://", "_").replace('/', "_"),
        topic_level_str
    );

    let ts = TraceSession {
        id: format!("trace_{}", Ulid::new().to_string().to_lowercase()),
        target_type: params.target_type,
        target: params.target,
        level,
        enabled_by: params.enabled_by,
        reason: params.reason,
        started_at,
        expires_at,
        persistence_mode: "ephemeral".to_string(),
        retention: params.duration.clone(),
        max_messages_per_second: Some(10),
        max_bytes_per_second: Some(65536),
        topics: vec![trace_topic, "_trace.**".to_string()],
        status: "active".to_string(),
    };

    if let Err(e) = state.trace_mgr.enable_session(ts.clone()) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to enable trace session: {}", e),
        ));
    }

    Ok((StatusCode::OK, Json(ts)))
}

pub async fn list_traces(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.store.list_trace_sessions() {
        Ok(s) => Ok(Json(s)),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to list trace sessions: {}", e),
        )),
    }
}

pub async fn disable_trace(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    match state.trace_mgr.disable_session(&id) {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to disable trace session: {}", e),
        )),
    }
}
