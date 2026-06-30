use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TraceLevel {
    Off,
    Status,
    Progress,
    Structured,
    Verbose,
    Cognitive,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TraceSession {
    pub id: String,
    pub target_type: String, // "agent", "task", "flow", "topic", "connector", "node"
    pub target: String,
    pub level: TraceLevel,
    pub enabled_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub started_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub persistence_mode: String, // "ephemeral", "metadata", "event_log"
    pub retention: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_messages_per_second: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_bytes_per_second: Option<usize>,
    #[serde(default)]
    pub topics: Vec<String>,
    pub status: String, // "requested", "approved", "active", "paused", "expired", "disabled", "archived"
}
