use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct DaemonMetrics {
    pub messages_published_total: AtomicU64,
    pub messages_routed_total: AtomicU64,
    pub messages_persisted_total: AtomicU64,
    pub messages_indexed_total: AtomicU64,
    pub persistence_write_failures_total: AtomicU64,
    pub dropped_persistence_messages_total: AtomicU64,
    pub dead_letter_messages_total: AtomicU64,

    pub trace_sessions_active: AtomicU64,
    pub trace_messages_total: AtomicU64,
    pub trace_messages_dropped_total: AtomicU64,
    pub trace_bytes_total: AtomicU64,
    pub trace_rate_limited_total: AtomicU64,
    pub trace_sessions_expired_total: AtomicU64,
    pub trace_sessions_denied_total: AtomicU64,
    pub trace_policy_violations_total: AtomicU64,
    pub raw_trace_messages_total: AtomicU64,
    pub cognitive_trace_messages_total: AtomicU64,

    pub subscriber_dropped_messages_total: AtomicU64,
    pub publisher_rejections_total: AtomicU64,
    pub overflow_events_total: AtomicU64,
    pub priority_drops_total: AtomicU64,
    pub slow_subscribers_total: AtomicU64,
}

#[derive(Serialize)]
pub struct MetricsSnapshot {
    pub messages_published_total: u64,
    pub messages_routed_total: u64,
    pub messages_persisted_total: u64,
    pub messages_indexed_total: u64,
    pub persistence_queue_depth: usize,
    pub persistence_lag_messages: usize,
    pub persistence_lag_seconds: f64,
    pub persistence_write_failures_total: u64,
    pub index_queue_depth: usize,
    pub index_lag_messages: usize,
    pub memory_builder_lag_messages: usize,
    pub dropped_persistence_messages_total: u64,
    pub dead_letter_messages_total: u64,

    pub trace_sessions_active: u64,
    pub trace_messages_total: u64,
    pub trace_messages_dropped_total: u64,
    pub trace_bytes_total: u64,
    pub trace_rate_limited_total: u64,
    pub trace_sessions_expired_total: u64,
    pub trace_sessions_denied_total: u64,
    pub trace_policy_violations_total: u64,
    pub raw_trace_messages_total: u64,
    pub cognitive_trace_messages_total: u64,

    pub subscriber_queue_depth: usize,
    pub subscriber_dropped_messages_total: u64,
    pub publisher_rejections_total: u64,
    pub overflow_events_total: u64,
    pub priority_drops_total: u64,
    pub slow_subscribers_total: u64,
}

impl DaemonMetrics {
    pub fn snapshot(
        &self,
        p_q_depth: usize,
        idx_q_depth: usize,
        sub_q_depth: usize,
    ) -> MetricsSnapshot {
        MetricsSnapshot {
            messages_published_total: self.messages_published_total.load(Ordering::Relaxed),
            messages_routed_total: self.messages_routed_total.load(Ordering::Relaxed),
            messages_persisted_total: self.messages_persisted_total.load(Ordering::Relaxed),
            messages_indexed_total: self.messages_indexed_total.load(Ordering::Relaxed),
            persistence_queue_depth: p_q_depth,
            persistence_lag_messages: p_q_depth,
            persistence_lag_seconds: p_q_depth as f64 * 0.001, // synthetic estimation
            persistence_write_failures_total: self
                .persistence_write_failures_total
                .load(Ordering::Relaxed),
            index_queue_depth: idx_q_depth,
            index_lag_messages: idx_q_depth,
            memory_builder_lag_messages: 0,
            dropped_persistence_messages_total: self
                .dropped_persistence_messages_total
                .load(Ordering::Relaxed),
            dead_letter_messages_total: self.dead_letter_messages_total.load(Ordering::Relaxed),

            trace_sessions_active: self.trace_sessions_active.load(Ordering::Relaxed),
            trace_messages_total: self.trace_messages_total.load(Ordering::Relaxed),
            trace_messages_dropped_total: self.trace_messages_dropped_total.load(Ordering::Relaxed),
            trace_bytes_total: self.trace_bytes_total.load(Ordering::Relaxed),
            trace_rate_limited_total: self.trace_rate_limited_total.load(Ordering::Relaxed),
            trace_sessions_expired_total: self.trace_sessions_expired_total.load(Ordering::Relaxed),
            trace_sessions_denied_total: self.trace_sessions_denied_total.load(Ordering::Relaxed),
            trace_policy_violations_total: self
                .trace_policy_violations_total
                .load(Ordering::Relaxed),
            raw_trace_messages_total: self.raw_trace_messages_total.load(Ordering::Relaxed),
            cognitive_trace_messages_total: self
                .cognitive_trace_messages_total
                .load(Ordering::Relaxed),

            subscriber_queue_depth: sub_q_depth,
            subscriber_dropped_messages_total: self
                .subscriber_dropped_messages_total
                .load(Ordering::Relaxed),
            publisher_rejections_total: self.publisher_rejections_total.load(Ordering::Relaxed),
            overflow_events_total: self.overflow_events_total.load(Ordering::Relaxed),
            priority_drops_total: self.priority_drops_total.load(Ordering::Relaxed),
            slow_subscribers_total: self.slow_subscribers_total.load(Ordering::Relaxed),
        }
    }
}
