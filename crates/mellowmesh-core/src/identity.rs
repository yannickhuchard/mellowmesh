use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanIdentity {
    pub id: String, // e.g., "human://yannick"
    pub display_name: String,
    pub interfaces: Vec<String>, // e.g., ["teams://...", "telegram://..."]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: String, // e.g., "agent://yannick/codex"
    pub name: String,
    pub owner: String,             // human owner ID: "human://yannick"
    pub mode: String,              // "human-piloted", "semi-autonomous", "autonomous", etc.
    pub capabilities: Vec<String>, // ["code.write", "code.review"]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeIdentity {
    pub id: String, // e.g., "node://laptop-yannick"
    pub hostname: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
}
