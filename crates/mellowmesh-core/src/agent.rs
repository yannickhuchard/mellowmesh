use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRegistration {
    pub id: String, // e.g., "agent://yannick/codex"
    pub name: String,
    pub owner: String, // "human://yannick"
    pub mode: String,  // "human-piloted", "autonomous"
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentPresence {
    pub agent_id: String,
    pub status: String, // "available", "busy", "offline"
    pub mode: String,
    pub capabilities: Vec<String>,
    pub current_work: Vec<String>, // task IDs
    pub last_seen: DateTime<Utc>,
}
