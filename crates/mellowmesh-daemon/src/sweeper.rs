//! Background maintenance loop: releases expired task-claim leases and
//! enforces message retention policies.

use crate::handlers::message::handle_publish;
use crate::state::AppState;
use chrono::Utc;
use mellowmesh_core::message::Message;
use mellowmesh_core::persistence::parse_retention;
use mellowmesh_core::topic::match_topic;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often retention purges run, independent of the (faster) lease sweep.
const PURGE_EVERY: Duration = Duration::from_secs(3600);

fn sweep_interval() -> Duration {
    let secs = std::env::var("MELLOWMESH_SWEEP_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(10);
    Duration::from_secs(secs)
}

pub fn start(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(sweep_interval());
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Force a retention purge on the first tick after startup.
        let mut last_purge: Option<Instant> = None;
        loop {
            ticker.tick().await;
            release_expired_leases(&state).await;
            if last_purge.is_none_or(|t| t.elapsed() >= PURGE_EVERY) {
                purge_expired_messages(&state);
                last_purge = Some(Instant::now());
            }
        }
    });
}

/// Return every expired claim to `open` and announce each release on
/// `_task.<id>.reclaimed` so waiting agents can pick the work back up.
async fn release_expired_leases(state: &AppState) {
    let released = match state.store.release_expired_claims() {
        Ok(tasks) => tasks,
        Err(e) => {
            tracing::error!("Lease sweep failed: {}", e);
            return;
        }
    };

    for task in released {
        let previous_holder = task.claimed_by.clone().unwrap_or_default();
        tracing::info!(
            "Claim lease expired on task {} (held by {}); returned to open",
            task.id,
            previous_holder
        );
        let msg = Message {
            id: String::new(),
            topic: format!("_task.{}.reclaimed", task.id),
            from: state.node_id.clone(),
            owner: None,
            timestamp: Utc::now(),
            content_type: "application/json".to_string(),
            body: format!(
                "Task '{}' returned to open: claim lease of {} expired.",
                task.title, previous_holder
            ),
            headers: None,
            payload: Some(serde_json::json!({
                "task_id": task.id,
                "previous_claimant": previous_holder,
                "status": "open",
            })),
            parent_id: None,
        };
        if let Err(e) = handle_publish(Arc::new(state.clone()), msg).await {
            tracing::warn!("Failed to announce lease release for {}: {}", task.id, e);
        }
    }
}

/// Delete messages older than their topic's resolved retention policy.
/// Topics that match no rule use the default policy's retention, which can
/// be overridden globally with `MELLOWMESH_RETENTION` (e.g. "30d").
/// Non-expiring retentions ("forever", "policy") are never purged; tasks,
/// decisions, and topic summaries live in separate tables and are untouched.
fn purge_expired_messages(state: &AppState) {
    let topics = match state.store.list_topics() {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Retention sweep failed to list topics: {}", e);
            return;
        }
    };

    let default_retention = std::env::var("MELLOWMESH_RETENTION")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| state.policy_config.default.retention.clone());

    let mut total_deleted = 0usize;
    for topic in topics {
        let retention = state
            .policy_config
            .rules
            .iter()
            .find(|(pattern, _)| match_topic(pattern, &topic))
            .map(|(_, policy)| policy.retention.clone())
            .unwrap_or_else(|| default_retention.clone());

        let Some(duration) = parse_retention(&retention) else {
            continue;
        };
        let cutoff = (Utc::now() - duration).to_rfc3339();
        match state.store.delete_messages_before(&topic, &cutoff) {
            Ok(deleted) => total_deleted += deleted,
            Err(e) => tracing::warn!("Retention purge failed for topic '{}': {}", topic, e),
        }
    }
    if total_deleted > 0 {
        tracing::info!("Retention sweep removed {} expired messages", total_deleted);
    }
}
