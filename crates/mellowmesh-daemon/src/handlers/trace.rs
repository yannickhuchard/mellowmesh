use crate::auth::AuthContext;
use crate::state::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::{Duration, Utc};
use mellowmesh_core::telemetry::{TraceLevel, TraceSession};
use serde::Deserialize;
use ulid::Ulid;

/// Longest trace session that can be enabled in one request.
pub const MAX_TRACE_SECS: i64 = 24 * 3600;

#[derive(Deserialize)]
pub struct EnableTraceParams {
    pub target: String,
    pub target_type: String, // e.g. "agent", "task", "flow", "topic"
    pub level: String,
    pub duration: String, // e.g. "15m", "1h"
    pub reason: Option<String>,
    pub enabled_by: String,
}

/// Parse `<digits><unit>` (`s`, `m`, `h`, `d`) into a positive duration of
/// at most [`MAX_TRACE_SECS`]. Overflow and anything malformed is `None`.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (num, unit) = s.split_at(split);
    let num: i64 = num.parse().ok()?;
    let secs = num.checked_mul(match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86_400,
        _ => return None,
    })?;
    if secs <= 0 || secs > MAX_TRACE_SECS {
        return None;
    }
    Some(Duration::seconds(secs))
}

/// A target / target type becomes a topic segment of the session's trace
/// topics, so wildcard characters must not survive into it.
fn topic_segment(s: &str) -> String {
    s.replace("://", "_").replace(['/', '*', '>'], "_")
}

fn require_admin(state: &AppState, ctx: &AuthContext) -> Result<(), (StatusCode, String)> {
    if crate::auth::is_admin(state, ctx) {
        Ok(())
    } else {
        Err((
            crate::auth::admin_refusal(ctx),
            "Only the owner may enable or disable trace sessions".to_string(),
        ))
    }
}

pub async fn enable_trace(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(params): Json<EnableTraceParams>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_admin(&state, &ctx)?;
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
            format!(
                "Invalid duration '{}': expected <n>s|m|h|d, at most 24h",
                params.duration
            ),
        )
    })?;

    if params.target.is_empty() || params.target_type.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "target and target_type are required".to_string(),
        ));
    }

    let started_at = Utc::now();
    let expires_at = started_at
        .checked_add_signed(duration)
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "Duration overflow".to_string()))?;

    let topic_level_str = format!("{level:?}").to_lowercase();
    let target_base = format!(
        "_trace.{}.{}",
        topic_segment(&params.target_type),
        topic_segment(&params.target)
    );
    let trace_topic = format!("{target_base}.{topic_level_str}");

    // Who enabled it is the authenticated principal when there is one.
    let enabled_by = ctx
        .principal
        .as_ref()
        .map(|p| p.id.clone())
        .unwrap_or(params.enabled_by);

    let ts = TraceSession {
        id: format!("trace_{}", Ulid::new().to_string().to_lowercase()),
        target_type: params.target_type,
        target: params.target,
        level,
        enabled_by,
        reason: params.reason,
        started_at,
        expires_at,
        persistence_mode: "ephemeral".to_string(),
        retention: params.duration.clone(),
        max_messages_per_second: Some(10),
        max_bytes_per_second: Some(65536),
        // Scoped to this target: the session's own topic plus the target's
        // other levels (the level check bounds what is admitted).
        topics: vec![trace_topic, format!("{target_base}.*")],
        status: "active".to_string(),
    };

    if let Err(e) = state.trace_mgr.enable_session(ts.clone()) {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to enable trace session: {e}"),
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
            format!("Failed to list trace sessions: {e}"),
        )),
    }
}

pub async fn disable_trace(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    require_admin(&state, &ctx)?;
    match state.trace_mgr.disable_session(&id) {
        Ok(_) => Ok(StatusCode::OK),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to disable trace session: {e}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_parsing_is_bounded() {
        assert_eq!(parse_duration("15m"), Some(Duration::minutes(15)));
        assert_eq!(parse_duration("24h"), Some(Duration::hours(24)));
        assert_eq!(parse_duration("1d"), Some(Duration::days(1)));
        assert_eq!(parse_duration("25h"), None);
        assert_eq!(parse_duration("2d"), None);
        assert_eq!(parse_duration("999999999999d"), None);
        assert_eq!(parse_duration("99999999999999999999999s"), None);
        assert_eq!(parse_duration("0s"), None);
        assert_eq!(parse_duration("1m2h"), None);
        assert_eq!(parse_duration("h"), None);
        assert_eq!(parse_duration("10"), None);
    }

    #[test]
    fn trace_topic_segments_have_no_wildcards() {
        assert_eq!(topic_segment("agent://you/coder"), "agent_you_coder");
        assert_eq!(topic_segment("**"), "__");
        assert_eq!(topic_segment("a.>"), "a._");
    }
}
