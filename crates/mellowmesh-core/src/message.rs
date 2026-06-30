use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub id: String, // e.g., "msg_01HX..."
    pub topic: String,
    pub from: String, // e.g., "agent://yannick/codex" or "human://yannick"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>, // e.g., "human://yannick"
    pub timestamp: DateTime<Utc>,
    pub content_type: String, // e.g., "text/plain", "text/markdown", "application/json"
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}
