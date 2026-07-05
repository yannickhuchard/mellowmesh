use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub pros: Vec<String>,
    #[serde(default)]
    pub cons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Decision {
    pub id: String, // e.g., "decision_01HZ..."
    pub title: String,
    pub question: String,
    pub created_by: String,       // e.g., "agent://architecture-reviewer"
    pub required_decider: String, // e.g., "human://yannick"
    pub status: String, // "requested", "discussed", "approved", "rejected", "deferred", etc.
    pub options: Vec<DecisionOption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_option_id: Option<String>, // Chosen option ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_timestamp: Option<DateTime<Utc>>,
    /// Authenticated principal that answered the decision (audit trail).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responded_by: Option<String>,
}
