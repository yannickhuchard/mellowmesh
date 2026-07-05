use serde::{Deserialize, Serialize};

/// Default claim lease duration granted when an agent claims a task
/// without specifying `lease_seconds` (10 minutes).
pub const DEFAULT_LEASE_SECONDS: u64 = 600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: String, // e.g., "task_01HY..."
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_from: Option<String>, // message ID that triggered this
    pub created_by: String, // e.g., "human://yannick"
    pub status: String,     // "open", "claimed", "in_progress", "completed", "cancelled", etc.
    pub priority: String,   // "low", "medium", "high"
    pub topics: Vec<String>,
    pub required_capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_to: Option<String>, // human or agent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>, // agent ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<String>, // ISO8601 string or date
    pub artifacts: Vec<String>, // list of artifact IDs
    pub decisions: Vec<String>, // list of decision IDs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// Lease duration in seconds granted at claim time. Renewed on progress
    /// updates; when the lease expires the daemon releases the claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_seconds: Option<u64>,
    /// RFC3339 UTC instant at which the current claim lease expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_expires_at: Option<String>,
}
