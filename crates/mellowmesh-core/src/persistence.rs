use crate::message::Message;
use crate::topic::{match_topic, Topic};
use chrono::{DateTime, Utc};
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceMode {
    Ephemeral,
    Metadata,
    EventLog,
    Queryable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistencePolicy {
    pub mode: PersistenceMode,
    pub retention: String, // e.g., "7d", "5m", "forever", "policy"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_message_size: Option<String>,
    #[serde(default)]
    pub sync: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistableMessage {
    pub message: Message,
    pub mode: PersistenceMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexableMessage {
    pub message: Message,
}

pub type Result<T> = std::result::Result<T, anyhow::Error>;
pub type MessageStream = Pin<Box<dyn Stream<Item = Result<Message>> + Send>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct HotOffset(pub u64);

pub trait HotBuffer: Send + Sync {
    fn append_transient(&self, msg: &Message) -> Result<HotOffset>;
    fn read_recent(&self, topic: &Topic, from: Option<HotOffset>) -> Result<MessageStream>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayQuery {
    pub topic: String,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

#[async_trait::async_trait]
pub trait EventStore: Send + Sync {
    async fn persist_batch(&self, batch: Vec<PersistableMessage>) -> Result<()>;
    async fn replay(&self, query: ReplayQuery) -> Result<MessageStream>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub topic_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub messages: Vec<Message>,
}

#[async_trait::async_trait]
pub trait QueryStore: Send + Sync {
    async fn index_batch(&self, batch: Vec<IndexableMessage>) -> Result<()>;
    async fn search(&self, query: SearchQuery) -> Result<SearchResult>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSummary {
    pub topic: String,
    pub summary: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextQuery {
    pub topic: String,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextResult {
    pub summaries: Vec<TopicSummary>,
    pub relevant_messages: Vec<Message>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lineage: Option<Vec<Message>>,
}

#[async_trait::async_trait]
pub trait MemoryStore: Send + Sync {
    async fn store_summary(&self, summary: TopicSummary) -> Result<()>;
    async fn get_context(&self, query: ContextQuery) -> Result<ContextResult>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistenceConfig {
    pub default: PersistencePolicy,
    pub rules: Vec<(String, PersistencePolicy)>,
}

impl PersistenceConfig {
    pub fn resolve(&self, topic: &str) -> &PersistencePolicy {
        for (pattern, policy) in &self.rules {
            if match_topic(pattern, topic) {
                return policy;
            }
        }
        &self.default
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicy {
    DropOldest,
    DropNewest,
    Sample,
    MetadataOnly,
    Summarize,
    BlockPublisher,
    DeadLetter,
    DisconnectSlowSubscriber,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TopicSchema {
    pub topic_pattern: String,
    pub version: String,
    pub schema_content: String, // Stringified JSON Schema
    pub status: String,         // "active" or "paused"
    pub created_at: chrono::DateTime<chrono::Utc>,
}
