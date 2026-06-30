use crate::metrics::DaemonMetrics;
use chrono::Utc;
use mellowmesh_core::message::Message;
use mellowmesh_core::telemetry::{TraceLevel, TraceSession};
use mellowmesh_store::Store;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio::time::Instant;

struct SessionLimit {
    last_tick: Instant,
    count: usize,
}

pub struct TraceSessionManager {
    store: Store,
    metrics: Arc<DaemonMetrics>,
    limits: Mutex<HashMap<String, SessionLimit>>,
}

impl TraceSessionManager {
    pub fn new(store: Store, metrics: Arc<DaemonMetrics>) -> Self {
        Self {
            store,
            metrics,
            limits: Mutex::new(HashMap::new()),
        }
    }

    pub fn enable_session(&self, mut ts: TraceSession) -> anyhow::Result<()> {
        ts.status = "active".to_string();
        self.store.insert_trace_session(&ts)?;
        self.metrics
            .trace_sessions_active
            .fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn disable_session(&self, id: &str) -> anyhow::Result<()> {
        if let Some(mut ts) = self.store.get_trace_session(id)? {
            ts.status = "disabled".to_string();
            self.store.insert_trace_session(&ts)?;
            self.metrics
                .trace_sessions_active
                .fetch_sub(1, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn check_trace_allowed(&self, msg: &Message) -> bool {
        // Trace messages belong to _trace.**
        if !msg.topic.starts_with("_trace.") {
            return false;
        }

        // Retrieve active sessions from DB
        let sessions = match self.store.list_trace_sessions() {
            Ok(s) => s,
            Err(_) => return false,
        };

        let now = Utc::now();
        let mut allowed = false;

        for ts in sessions {
            if ts.status != "active" {
                continue;
            }

            if ts.expires_at <= now {
                // Auto-expire
                let _ = self.disable_session(&ts.id);
                self.metrics
                    .trace_sessions_expired_total
                    .fetch_add(1, Ordering::Relaxed);
                continue;
            }

            // Check if topic matches trace session topics
            let mut topic_match = false;
            for t_pat in &ts.topics {
                if mellowmesh_core::topic::match_topic(t_pat, &msg.topic) {
                    topic_match = true;
                    break;
                }
            }

            if !topic_match {
                continue;
            }

            // Check target match
            // e.g. msg source or topic target details
            // For MVP, if topic matches the session's topic list, we consider it target matched

            // Check rate limiting
            if let Some(max_msgs) = ts.max_messages_per_second {
                let mut limits = self.limits.lock().unwrap();
                let limit = limits.entry(ts.id.clone()).or_insert_with(|| SessionLimit {
                    last_tick: Instant::now(),
                    count: 0,
                });

                if limit.last_tick.elapsed().as_secs() >= 1 {
                    limit.last_tick = Instant::now();
                    limit.count = 0;
                }

                if limit.count >= max_msgs {
                    self.metrics
                        .trace_rate_limited_total
                        .fetch_add(1, Ordering::Relaxed);
                    self.metrics
                        .trace_messages_dropped_total
                        .fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                limit.count += 1;
            }

            // Enforce trace levels based on topic convention
            // e.g. _trace.agent.codex.cognitive -> level cognitive
            let topic_level = if msg.topic.ends_with(".raw") {
                TraceLevel::Raw
            } else if msg.topic.ends_with(".cognitive") {
                TraceLevel::Cognitive
            } else if msg.topic.ends_with(".verbose") {
                TraceLevel::Verbose
            } else if msg.topic.ends_with(".structured") {
                TraceLevel::Structured
            } else if msg.topic.ends_with(".progress") {
                TraceLevel::Progress
            } else if msg.topic.ends_with(".status") {
                TraceLevel::Status
            } else {
                TraceLevel::Off
            };

            if topic_level <= ts.level {
                allowed = true;

                // Track bytes and counts
                self.metrics
                    .trace_messages_total
                    .fetch_add(1, Ordering::Relaxed);
                self.metrics
                    .trace_bytes_total
                    .fetch_add(msg.body.len() as u64, Ordering::Relaxed);
                if topic_level == TraceLevel::Cognitive {
                    self.metrics
                        .cognitive_trace_messages_total
                        .fetch_add(1, Ordering::Relaxed);
                } else if topic_level == TraceLevel::Raw {
                    self.metrics
                        .raw_trace_messages_total
                        .fetch_add(1, Ordering::Relaxed);
                }
                break;
            }
        }

        if !allowed {
            self.metrics
                .trace_policy_violations_total
                .fetch_add(1, Ordering::Relaxed);
        }

        allowed
    }
}
