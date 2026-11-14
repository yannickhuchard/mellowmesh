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

/// Parse a retention duration string (e.g. "30s", "5m", "24h", "7d") into a
/// [`chrono::Duration`]. Returns `None` for non-expiring values ("forever",
/// "policy") and for unparseable inputs, meaning: never purge.
pub fn parse_retention(retention: &str) -> Option<chrono::Duration> {
    let s = retention.trim().to_lowercase();
    if s.is_empty() || s == "forever" || s == "policy" {
        return None;
    }
    let unit = s.chars().last()?;
    let digits = &s[..s.len() - unit.len_utf8()];
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: i64 = digits.parse().ok()?;
    let multiplier: i64 = match unit {
        's' => 1,
        'm' => 60,
        'h' => 60 * 60,
        'd' => 24 * 60 * 60,
        _ => return None,
    };
    let secs = value.checked_mul(multiplier)?;
    if secs > MAX_RETENTION_SECS {
        return None;
    }
    chrono::Duration::try_seconds(secs)
}

/// Upper bound accepted by [`parse_retention`] (about 100 years). Longer
/// values are treated as invalid, i.e. never purge, so that subtracting the
/// duration from the current time can never overflow.
pub const MAX_RETENTION_SECS: i64 = 100 * 366 * 24 * 60 * 60;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_retention_units() {
        assert_eq!(parse_retention("30s"), Some(chrono::Duration::seconds(30)));
        assert_eq!(parse_retention("5m"), Some(chrono::Duration::minutes(5)));
        assert_eq!(parse_retention("24h"), Some(chrono::Duration::hours(24)));
        assert_eq!(parse_retention("90d"), Some(chrono::Duration::days(90)));
        assert_eq!(parse_retention(" 7D "), Some(chrono::Duration::days(7)));
    }

    #[test]
    fn test_parse_retention_non_expiring() {
        assert_eq!(parse_retention("forever"), None);
        assert_eq!(parse_retention("policy"), None);
        assert_eq!(parse_retention(""), None);
    }

    #[test]
    fn test_parse_retention_invalid() {
        assert_eq!(parse_retention("abc"), None);
        assert_eq!(parse_retention("10x"), None);
        assert_eq!(parse_retention("d"), None);
        assert_eq!(parse_retention("-5d"), None);
        assert_eq!(parse_retention("+5d"), None);
    }

    #[test]
    fn test_parse_retention_multibyte_does_not_panic() {
        assert_eq!(parse_retention("5é"), None);
        assert_eq!(parse_retention("é"), None);
        assert_eq!(parse_retention("5日"), None);
        assert_eq!(parse_retention("é5d"), None);
    }

    #[test]
    fn test_parse_retention_huge_values_are_rejected() {
        assert_eq!(parse_retention("99999999999d"), None);
        assert_eq!(parse_retention("9223372036854775807s"), None);
        assert_eq!(parse_retention("99999999999999999999999d"), None);
        assert_eq!(
            parse_retention("36500d"),
            Some(chrono::Duration::days(36500))
        );
        assert_eq!(parse_retention("40000d"), None);
        // Whatever is accepted can be subtracted from now without panicking.
        for r in ["36500d", "876000h", "52560000m"] {
            let d = parse_retention(r).unwrap();
            let _ = chrono::Utc::now() - d;
        }
    }

    #[test]
    fn test_resolve_falls_back_to_default() {
        let config = PersistenceConfig {
            default: PersistencePolicy {
                mode: PersistenceMode::Metadata,
                retention: "7d".to_string(),
                max_message_size: None,
                sync: false,
            },
            rules: vec![(
                "_forum.**".to_string(),
                PersistencePolicy {
                    mode: PersistenceMode::Queryable,
                    retention: "180d".to_string(),
                    max_message_size: None,
                    sync: false,
                },
            )],
        };
        assert_eq!(config.resolve("_forum.general").retention, "180d");
        assert_eq!(config.resolve("random.topic").retention, "7d");
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
