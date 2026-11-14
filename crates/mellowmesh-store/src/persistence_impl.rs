use crate::message_store::{
    escape_like, fts5_phrase, message_from_row, topic_prefix_like, MAX_HISTORY_LIMIT,
    MAX_SEARCH_RESULTS, MESSAGE_COLUMNS,
};
use crate::Store;
use mellowmesh_core::message::Message;
use mellowmesh_core::persistence::{
    ContextQuery, ContextResult, EventStore, IndexableMessage, MemoryStore, MessageStream,
    PersistableMessage, PersistenceMode, QueryStore, ReplayQuery, Result, SearchQuery,
    SearchResult, TopicSummary,
};
use mellowmesh_core::telemetry::{TraceLevel, TraceSession};
use mellowmesh_core::topic::match_topic;
use rusqlite::params;

/// Maximum number of messages a single replay returns.
pub const MAX_REPLAY_MESSAGES: usize = 10_000;
/// Maximum number of candidate rows a single replay reads.
const MAX_REPLAY_SCAN_ROWS: i64 = 100_000;
/// Maximum number of candidate rows `get_context` reads when looking for the
/// most recent messages on a topic pattern.
const MAX_CONTEXT_SCAN_ROWS: i64 = 10_000;
/// Maximum number of candidate rows a topic-filtered search reads.
const MAX_SEARCH_SCAN_ROWS: i64 = 5_000;

/// Collects rows into messages, skipping rows whose timestamp cannot be
/// parsed and rows whose topic does not match `pattern`, stopping once
/// `limit` messages have been collected.
fn collect_matching(
    rows: &mut rusqlite::Rows<'_>,
    pattern: Option<&str>,
    limit: usize,
) -> rusqlite::Result<Vec<Message>> {
    let mut out = Vec::new();
    if limit == 0 {
        return Ok(out);
    }
    while let Some(row) = rows.next()? {
        if let Some(pat) = pattern {
            let topic: String = row.get(1)?;
            if !match_topic(pat, &topic) {
                continue;
            }
        }
        if let Some(m) = message_from_row(row)? {
            out.push(m);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

#[async_trait::async_trait]
impl EventStore for Store {
    async fn persist_batch(&self, batch: Vec<PersistableMessage>) -> Result<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = store.conn()?;
            let tx = conn.transaction()?;
            {
                // First write wins: a message id that already exists is never
                // overwritten (and so can never be moved to another topic).
                let mut stmt = tx.prepare(
                    "INSERT INTO messages (id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, persistence_mode, parent_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                     ON CONFLICT(id) DO NOTHING",
                )?;
                let mut fts_delete = tx.prepare("DELETE FROM messages_fts WHERE rowid = ?1")?;
                let mut fts_insert = tx.prepare(
                    "INSERT INTO messages_fts (rowid, id, topic, body) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for pm in batch {
                    let msg = &pm.message;
                    let (body, headers_json, payload_json) = match pm.mode {
                        PersistenceMode::Ephemeral => continue,
                        // Metadata mode keeps no content: body, headers and
                        // payload are all redacted.
                        PersistenceMode::Metadata => (
                            format!("Metadata-only: size={}", msg.body.len()),
                            None,
                            None,
                        ),
                        PersistenceMode::EventLog | PersistenceMode::Queryable => (
                            msg.body.clone(),
                            msg.headers
                                .as_ref()
                                .map(serde_json::to_string)
                                .transpose()?,
                            msg.payload
                                .as_ref()
                                .map(serde_json::to_string)
                                .transpose()?,
                        ),
                    };
                    let mode_str = match pm.mode {
                        PersistenceMode::Ephemeral => "ephemeral",
                        PersistenceMode::Metadata => "metadata",
                        PersistenceMode::EventLog => "event_log",
                        PersistenceMode::Queryable => "queryable",
                    };
                    let inserted = stmt.execute(params![
                        msg.id,
                        msg.topic,
                        msg.from,
                        msg.owner,
                        msg.timestamp.to_rfc3339(),
                        msg.content_type,
                        body,
                        headers_json,
                        payload_json,
                        mode_str,
                        msg.parent_id
                    ])?;
                    // Index only rows this call actually inserted. The FTS
                    // row shares the message's rowid, so it can never be
                    // duplicated.
                    if inserted == 1 && pm.mode == PersistenceMode::Queryable {
                        let rowid = tx.last_insert_rowid();
                        fts_delete.execute(params![rowid])?;
                        fts_insert.execute(params![rowid, msg.id, msg.topic, body])?;
                    }
                }
            }
            tx.commit()?;
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        Ok(())
    }

    async fn replay(&self, query: ReplayQuery) -> Result<MessageStream> {
        let store = self.clone();
        let msgs = tokio::task::spawn_blocking(move || {
            let conn = store.conn()?;
            let mut sql = format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE 1=1");
            let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();

            if let Some(like) = topic_prefix_like(&query.topic) {
                sql.push_str(" AND topic LIKE ? ESCAPE '\\'");
                params_vec.push(rusqlite::types::Value::Text(like));
            }
            if let Some(start) = query.start_time {
                sql.push_str(" AND timestamp >= ?");
                params_vec.push(rusqlite::types::Value::Text(start.to_rfc3339()));
            }
            if let Some(end) = query.end_time {
                sql.push_str(" AND timestamp <= ?");
                params_vec.push(rusqlite::types::Value::Text(end.to_rfc3339()));
            }
            sql.push_str(" ORDER BY timestamp ASC LIMIT ?");
            params_vec.push(rusqlite::types::Value::Integer(MAX_REPLAY_SCAN_ROWS));

            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query(rusqlite::params_from_iter(params_vec.iter()))?;
            let results = collect_matching(&mut rows, Some(&query.topic), MAX_REPLAY_MESSAGES)?;
            Ok::<Vec<Message>, anyhow::Error>(results)
        })
        .await??;

        let stream = futures_util::stream::iter(msgs.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }
}

/// Looks up messages through the FTS index (`fts = true`) or with an escaped
/// `LIKE` substring scan, newest first, optionally restricted to a topic
/// pattern.
fn search_rows(
    conn: &rusqlite::Connection,
    query: &str,
    topic_pattern: Option<&str>,
    fts: bool,
) -> rusqlite::Result<Vec<Message>> {
    let mut sql = format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE ");
    let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();
    if fts {
        sql.push_str("id IN (SELECT id FROM messages_fts WHERE messages_fts MATCH ?1)");
        params_vec.push(rusqlite::types::Value::Text(fts5_phrase(query)));
    } else {
        sql.push_str("(body LIKE ?1 ESCAPE '\\' OR topic LIKE ?1 ESCAPE '\\')");
        params_vec.push(rusqlite::types::Value::Text(format!(
            "%{}%",
            escape_like(query)
        )));
    }
    let scan_limit = match topic_pattern {
        Some(pat) => {
            if let Some(like) = topic_prefix_like(pat) {
                sql.push_str(&format!(
                    " AND topic LIKE ?{} ESCAPE '\\'",
                    params_vec.len() + 1
                ));
                params_vec.push(rusqlite::types::Value::Text(like));
            }
            MAX_SEARCH_SCAN_ROWS
        }
        None => MAX_SEARCH_RESULTS as i64,
    };
    sql.push_str(&format!(
        " ORDER BY timestamp DESC LIMIT ?{}",
        params_vec.len() + 1
    ));
    params_vec.push(rusqlite::types::Value::Integer(scan_limit));

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(params_vec.iter()))?;
    collect_matching(&mut rows, topic_pattern, MAX_SEARCH_RESULTS)
}

#[async_trait::async_trait]
impl QueryStore for Store {
    /// Ensures each message is present in the full-text index.
    ///
    /// Messages persisted in queryable mode are indexed by `persist_batch`
    /// when their row is inserted, so this is idempotent: a message is
    /// indexed only if its stored row exists, has the same topic and body,
    /// and is not yet indexed. Content that differs from the stored row
    /// (e.g. a later message reusing an existing id) is never indexed.
    async fn index_batch(&self, batch: Vec<IndexableMessage>) -> Result<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = store.conn()?;
            let tx = conn.transaction()?;
            {
                let mut lookup = tx.prepare(
                    "SELECT rowid, topic, body FROM messages WHERE id = ?1 AND persistence_mode = 'queryable'",
                )?;
                let mut fts_exists = tx.prepare("SELECT 1 FROM messages_fts WHERE rowid = ?1")?;
                let mut fts_insert = tx.prepare(
                    "INSERT INTO messages_fts (rowid, id, topic, body) VALUES (?1, ?2, ?3, ?4)",
                )?;
                for im in batch {
                    let msg = &im.message;
                    let stored = {
                        let mut rows = lookup.query(params![msg.id])?;
                        match rows.next()? {
                            Some(row) => Some((
                                row.get::<_, i64>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                            )),
                            None => None,
                        }
                    };
                    let Some((rowid, topic, body)) = stored else {
                        continue;
                    };
                    if topic != msg.topic || body != msg.body {
                        continue;
                    }
                    if fts_exists.exists(params![rowid])? {
                        continue;
                    }
                    fts_insert.execute(params![rowid, msg.id, topic, body])?;
                }
            }
            tx.commit()?;
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        Ok(())
    }

    async fn search(&self, query: SearchQuery) -> Result<SearchResult> {
        let store = self.clone();
        let msgs = tokio::task::spawn_blocking(move || {
            if query.query.trim().is_empty() {
                return Ok::<Vec<Message>, anyhow::Error>(Vec::new());
            }
            let conn = store.conn()?;
            let pattern = query.topic_pattern.as_deref();
            // The FTS query is a quoted phrase and should not fail; fall back
            // to an escaped substring scan if it does anyway.
            let results = match search_rows(&conn, &query.query, pattern, true) {
                Ok(r) => r,
                Err(_) => search_rows(&conn, &query.query, pattern, false)?,
            };
            Ok(results)
        })
        .await??;

        Ok(SearchResult { messages: msgs })
    }
}

#[async_trait::async_trait]
impl MemoryStore for Store {
    async fn store_summary(&self, summary: TopicSummary) -> Result<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let conn = store.conn()?;
            conn.execute(
                "INSERT INTO topic_summaries (topic, summary, generated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(topic) DO UPDATE SET
                    summary = excluded.summary,
                    generated_at = excluded.generated_at",
                params![
                    summary.topic,
                    summary.summary,
                    summary.generated_at.to_rfc3339()
                ],
            )?;
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        Ok(())
    }

    async fn get_context(&self, query: ContextQuery) -> Result<ContextResult> {
        let store = self.clone();
        let (summaries, relevant_messages, lineage) = tokio::task::spawn_blocking(move || {
            let conn = store.conn()?;
            let prefix_like = topic_prefix_like(&query.topic);

            let mut matching_summaries = Vec::new();
            {
                let mut stmt = match &prefix_like {
                    Some(_) => conn.prepare(
                        "SELECT topic, summary, generated_at FROM topic_summaries WHERE topic LIKE ?1 ESCAPE '\\'",
                    )?,
                    None => conn.prepare("SELECT topic, summary, generated_at FROM topic_summaries")?,
                };
                let mut rows = match &prefix_like {
                    Some(like) => stmt.query(params![like])?,
                    None => stmt.query([])?,
                };
                while let Some(row) = rows.next()? {
                    let topic: String = row.get(0)?;
                    if !match_topic(&query.topic, &topic) {
                        continue;
                    }
                    let ts_str: String = row.get(2)?;
                    let Ok(generated_at) = chrono::DateTime::parse_from_rfc3339(&ts_str) else {
                        continue;
                    };
                    matching_summaries.push(TopicSummary {
                        topic,
                        summary: row.get(1)?,
                        generated_at: generated_at.with_timezone(&chrono::Utc),
                    });
                }
            }

            let limit = query.limit.min(MAX_HISTORY_LIMIT);
            let relevant_messages = {
                let mut stmt = match &prefix_like {
                    Some(_) => conn.prepare(&format!(
                        "SELECT {MESSAGE_COLUMNS} FROM messages WHERE topic LIKE ?1 ESCAPE '\\' ORDER BY timestamp DESC LIMIT ?2"
                    ))?,
                    None => conn.prepare(&format!(
                        "SELECT {MESSAGE_COLUMNS} FROM messages ORDER BY timestamp DESC LIMIT ?1"
                    ))?,
                };
                let mut rows = match &prefix_like {
                    Some(like) => stmt.query(params![like, MAX_CONTEXT_SCAN_ROWS])?,
                    None => stmt.query(params![MAX_CONTEXT_SCAN_ROWS])?,
                };
                collect_matching(&mut rows, Some(&query.topic), limit)?
            };

            // Resolve lineage for relevant messages
            let mut lineage_messages = Vec::new();
            let mut visited = std::collections::HashSet::new();
            let mut get_msg_stmt = conn.prepare(&format!(
                "SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"
            ))?;

            for msg in &relevant_messages {
                let mut current_parent = msg.parent_id.clone();
                let mut depth = 0;
                while let Some(parent_id) = current_parent {
                    if visited.contains(&parent_id) || depth >= 10 {
                        break;
                    }
                    visited.insert(parent_id.clone());

                    let mut rows = get_msg_stmt.query(params![parent_id])?;
                    let Some(row) = rows.next()? else {
                        break;
                    };
                    let Some(parent_msg) = message_from_row(row)? else {
                        break;
                    };
                    current_parent = parent_msg.parent_id.clone();
                    lineage_messages.push(parent_msg);
                    depth += 1;
                }
            }

            Ok::<(_, _, _), anyhow::Error>((matching_summaries, relevant_messages, lineage_messages))
        })
        .await??;

        Ok(ContextResult {
            summaries,
            relevant_messages,
            lineage: Some(lineage),
        })
    }
}

impl Store {
    pub fn insert_trace_session(&self, ts: &TraceSession) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let topics_json = serde_json::to_string(&ts.topics)?;
        conn.execute(
            "INSERT INTO trace_sessions (id, target_type, target, level, enabled_by, reason, started_at, expires_at, persistence_mode, retention, max_messages_per_second, max_bytes_per_second, topics, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                expires_at = excluded.expires_at,
                level = excluded.level",
            params![
                ts.id,
                ts.target_type,
                ts.target,
                format!("{:?}", ts.level).to_lowercase(),
                ts.enabled_by,
                ts.reason,
                ts.started_at.to_rfc3339(),
                ts.expires_at.to_rfc3339(),
                ts.persistence_mode,
                ts.retention,
                ts.max_messages_per_second,
                ts.max_bytes_per_second,
                topics_json,
                ts.status
            ],
        )?;
        Ok(())
    }

    pub fn get_trace_session(&self, id: &str) -> anyhow::Result<Option<TraceSession>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, target_type, target, level, enabled_by, reason, started_at, expires_at, persistence_mode, retention, max_messages_per_second, max_bytes_per_second, topics, status FROM trace_sessions WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let level_str: String = row.get(3)?;
            let level = match level_str.as_str() {
                "off" => TraceLevel::Off,
                "status" => TraceLevel::Status,
                "progress" => TraceLevel::Progress,
                "structured" => TraceLevel::Structured,
                "verbose" => TraceLevel::Verbose,
                "cognitive" => TraceLevel::Cognitive,
                "raw" => TraceLevel::Raw,
                _ => TraceLevel::Off,
            };
            let start_str: String = row.get(6)?;
            let started_at =
                chrono::DateTime::parse_from_rfc3339(&start_str)?.with_timezone(&chrono::Utc);
            let expire_str: String = row.get(7)?;
            let expires_at =
                chrono::DateTime::parse_from_rfc3339(&expire_str)?.with_timezone(&chrono::Utc);
            let topics_str: String = row.get(12)?;
            let topics = serde_json::from_str(&topics_str)?;

            Ok(Some(TraceSession {
                id: row.get(0)?,
                target_type: row.get(1)?,
                target: row.get(2)?,
                level,
                enabled_by: row.get(4)?,
                reason: row.get(5)?,
                started_at,
                expires_at,
                persistence_mode: row.get(8)?,
                retention: row.get(9)?,
                max_messages_per_second: row.get(10)?,
                max_bytes_per_second: row.get(11)?,
                topics,
                status: row.get(13)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_trace_sessions(&self) -> anyhow::Result<Vec<TraceSession>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, target_type, target, level, enabled_by, reason, started_at, expires_at, persistence_mode, retention, max_messages_per_second, max_bytes_per_second, topics, status FROM trace_sessions")?;
        let rows = stmt.query_map([], |row| {
            let level_str: String = row.get(3).unwrap_or_default();
            let level = match level_str.as_str() {
                "off" => TraceLevel::Off,
                "status" => TraceLevel::Status,
                "progress" => TraceLevel::Progress,
                "structured" => TraceLevel::Structured,
                "verbose" => TraceLevel::Verbose,
                "cognitive" => TraceLevel::Cognitive,
                "raw" => TraceLevel::Raw,
                _ => TraceLevel::Off,
            };
            let start_str: String = row.get(6)?;
            let started_at = chrono::DateTime::parse_from_rfc3339(&start_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            let expire_str: String = row.get(7)?;
            let expires_at = chrono::DateTime::parse_from_rfc3339(&expire_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            let topics_str: String = row.get(12)?;
            let topics = serde_json::from_str(&topics_str).unwrap_or_default();

            Ok(TraceSession {
                id: row.get(0)?,
                target_type: row.get(1)?,
                target: row.get(2)?,
                level,
                enabled_by: row.get(4)?,
                reason: row.get(5)?,
                started_at,
                expires_at,
                persistence_mode: row.get(8)?,
                retention: row.get(9)?,
                max_messages_per_second: row.get(10)?,
                max_bytes_per_second: row.get(11)?,
                topics,
                status: row.get(13)?,
            })
        })?;

        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use futures_util::StreamExt;
    use std::collections::HashMap;

    fn msg(id: &str, topic: &str, body: &str, age_secs: i64) -> Message {
        Message {
            id: id.to_string(),
            topic: topic.to_string(),
            from: "agent://test/a".to_string(),
            owner: None,
            timestamp: Utc::now() - Duration::seconds(age_secs),
            content_type: "text/plain".to_string(),
            body: body.to_string(),
            headers: None,
            payload: None,
            parent_id: None,
        }
    }

    fn pm(m: Message, mode: PersistenceMode) -> PersistableMessage {
        PersistableMessage { message: m, mode }
    }

    fn fts_rows_for(store: &Store, id: &str) -> i64 {
        store
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM messages_fts WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn insert_bad_timestamp_row(store: &Store) {
        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO messages (id, topic, from_identity, timestamp, content_type, body, persistence_mode)
                 VALUES ('msg_bad', 'a.b.c', 'agent://x', '+10000-01-01T00:00:00+00:00', 'text/plain', 'needle bad', 'queryable')",
                [],
            )
            .unwrap();
    }

    #[tokio::test]
    async fn test_duplicate_id_first_write_wins() {
        let store = Store::new_in_memory().unwrap();
        let first = msg("msg_dup", "team.a", "original body", 10);
        let second = msg("msg_dup", "team.b", "replacement body", 0);
        store
            .persist_batch(vec![pm(first.clone(), PersistenceMode::Queryable)])
            .await
            .unwrap();
        store
            .persist_batch(vec![pm(second.clone(), PersistenceMode::Queryable)])
            .await
            .unwrap();
        // Also within a single batch.
        store
            .persist_batch(vec![
                pm(
                    msg("msg_dup2", "team.a", "one", 5),
                    PersistenceMode::Queryable,
                ),
                pm(
                    msg("msg_dup2", "team.b", "two", 4),
                    PersistenceMode::Queryable,
                ),
            ])
            .await
            .unwrap();

        let stored = store.get_message("msg_dup").unwrap().unwrap();
        assert_eq!(stored.topic, "team.a");
        assert_eq!(stored.body, "original body");
        assert_eq!(stored.timestamp.timestamp(), first.timestamp.timestamp());
        let stored2 = store.get_message("msg_dup2").unwrap().unwrap();
        assert_eq!(
            (stored2.topic.as_str(), stored2.body.as_str()),
            ("team.a", "one")
        );

        // The rejected content is never indexed, even via index_batch.
        store
            .index_batch(vec![
                IndexableMessage { message: first },
                IndexableMessage { message: second },
            ])
            .await
            .unwrap();
        assert_eq!(fts_rows_for(&store, "msg_dup"), 1);
        assert_eq!(fts_rows_for(&store, "msg_dup2"), 1);
        let found = |q: &str| {
            let store = store.clone();
            let q = q.to_string();
            async move {
                store
                    .search(SearchQuery {
                        query: q,
                        topic_pattern: None,
                    })
                    .await
                    .unwrap()
                    .messages
            }
        };
        assert_eq!(found("original").await.len(), 1);
        assert!(found("replacement").await.is_empty());
    }

    #[tokio::test]
    async fn test_index_batch_is_idempotent() {
        let store = Store::new_in_memory().unwrap();
        let m = msg("msg_idx", "t.x", "indexed text", 0);
        // Indexing before the row exists is a no-op; persisting indexes it.
        store
            .index_batch(vec![IndexableMessage { message: m.clone() }])
            .await
            .unwrap();
        assert_eq!(fts_rows_for(&store, "msg_idx"), 0);
        store
            .persist_batch(vec![pm(m.clone(), PersistenceMode::Queryable)])
            .await
            .unwrap();
        for _ in 0..3 {
            store
                .index_batch(vec![IndexableMessage { message: m.clone() }])
                .await
                .unwrap();
        }
        assert_eq!(fts_rows_for(&store, "msg_idx"), 1);

        // Non-queryable modes are never indexed.
        let e = msg("msg_ev", "t.x", "event text", 0);
        store
            .persist_batch(vec![pm(e.clone(), PersistenceMode::EventLog)])
            .await
            .unwrap();
        store
            .index_batch(vec![IndexableMessage { message: e }])
            .await
            .unwrap();
        assert_eq!(fts_rows_for(&store, "msg_ev"), 0);
    }

    #[tokio::test]
    async fn test_metadata_mode_redacts_headers_and_payload() {
        let store = Store::new_in_memory().unwrap();
        let mut m = msg("msg_meta", "t.meta", "secret body", 0);
        m.headers = Some(HashMap::from([("k".to_string(), "secret".to_string())]));
        m.payload = Some(serde_json::json!({"secret": true}));
        let mut full = m.clone();
        full.id = "msg_full".to_string();
        store
            .persist_batch(vec![
                pm(m, PersistenceMode::Metadata),
                pm(full, PersistenceMode::EventLog),
            ])
            .await
            .unwrap();

        let meta = store.get_message("msg_meta").unwrap().unwrap();
        assert_eq!(meta.body, "Metadata-only: size=11");
        assert!(meta.headers.is_none());
        assert!(meta.payload.is_none());
        let (h, p): (Option<String>, Option<String>) = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT headers, payload FROM messages WHERE id = 'msg_meta'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(h.is_none() && p.is_none());

        let full = store.get_message("msg_full").unwrap().unwrap();
        assert!(full.headers.is_some());
        assert!(full.payload.is_some());
    }

    #[tokio::test]
    async fn test_fts_search_handles_punctuation() {
        let store = Store::new_in_memory().unwrap();
        store
            .persist_batch(vec![
                pm(
                    msg(
                        "m1",
                        "dev.lang",
                        "We don't ship C++ in the foo-bar service",
                        1,
                    ),
                    PersistenceMode::Queryable,
                ),
                pm(
                    msg("m2", "ops.x", "unrelated", 0),
                    PersistenceMode::Queryable,
                ),
            ])
            .await
            .unwrap();
        for q in ["don't", "C++", "foo-bar"] {
            // The FTS path itself must accept the input (no silent fallback).
            let direct = search_rows(&store.conn().unwrap(), q, None, true).unwrap();
            assert_eq!(direct.len(), 1, "fts query {q:?}");
            let res = store
                .search(SearchQuery {
                    query: q.to_string(),
                    topic_pattern: None,
                })
                .await
                .unwrap();
            assert_eq!(res.messages.len(), 1, "query {q:?}");
            assert_eq!(res.messages[0].id, "m1");
        }
        for q in ["\"", "*", "a OR", "NEAR(", "%", ""] {
            assert!(store
                .search(SearchQuery {
                    query: q.to_string(),
                    topic_pattern: None,
                })
                .await
                .is_ok());
        }
        let filtered = store
            .search(SearchQuery {
                query: "foo-bar".to_string(),
                topic_pattern: Some("ops.**".to_string()),
            })
            .await
            .unwrap();
        assert!(filtered.messages.is_empty());
        let filtered = store
            .search(SearchQuery {
                query: "foo-bar".to_string(),
                topic_pattern: Some("dev.*".to_string()),
            })
            .await
            .unwrap();
        assert_eq!(filtered.messages.len(), 1);
    }

    #[tokio::test]
    async fn test_bad_timestamp_rows_are_skipped() {
        let store = Store::new_in_memory().unwrap();
        store
            .persist_batch(vec![pm(
                msg("msg_ok", "a.b.c", "needle ok", 0),
                PersistenceMode::Queryable,
            )])
            .await
            .unwrap();
        insert_bad_timestamp_row(&store);

        let ids = |v: Vec<Message>| v.into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids(store.get_history(10).unwrap()), vec!["msg_ok"]);
        assert_eq!(
            ids(store.search_messages("needle").unwrap()),
            vec!["msg_ok"]
        );
        assert_eq!(
            ids(store.get_history_matching("a.**", 10).unwrap()),
            vec!["msg_ok"]
        );
        let ctx = store
            .get_context(ContextQuery {
                topic: "a.b.*".to_string(),
                limit: 10,
            })
            .await
            .unwrap();
        assert_eq!(ids(ctx.relevant_messages), vec!["msg_ok"]);
        let replayed: Vec<Message> = store
            .replay(ReplayQuery {
                topic: "a.>".to_string(),
                start_time: None,
                end_time: None,
            })
            .await
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(ids(replayed), vec!["msg_ok"]);
    }

    #[tokio::test]
    async fn test_get_context_and_replay_filter_by_pattern() {
        let store = Store::new_in_memory().unwrap();
        let mut batch = Vec::new();
        for i in 0..20 {
            let topic = match i % 4 {
                0 => "_forum.general",
                1 => "_forum.games.mario",
                2 => "xforum.general",
                _ => "_agent.codex.status",
            };
            batch.push(pm(
                msg(&format!("m{i:02}"), topic, &format!("body {i}"), 100 - i),
                PersistenceMode::EventLog,
            ));
        }
        store.persist_batch(batch).await.unwrap();
        store
            .store_summary(TopicSummary {
                topic: "_forum.general".to_string(),
                summary: "s".to_string(),
                generated_at: Utc::now(),
            })
            .await
            .unwrap();
        store
            .store_summary(TopicSummary {
                topic: "xforum.general".to_string(),
                summary: "s".to_string(),
                generated_at: Utc::now(),
            })
            .await
            .unwrap();

        let ctx = store
            .get_context(ContextQuery {
                topic: "_forum.**".to_string(),
                limit: 3,
            })
            .await
            .unwrap();
        let got: Vec<_> = ctx
            .relevant_messages
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        // Newest first.
        assert_eq!(got, vec!["m17", "m16", "m13"]);
        assert_eq!(ctx.summaries.len(), 1);
        assert_eq!(ctx.summaries[0].topic, "_forum.general");

        let ctx = store
            .get_context(ContextQuery {
                topic: "*.general".to_string(),
                limit: usize::MAX,
            })
            .await
            .unwrap();
        assert_eq!(ctx.relevant_messages.len(), 10);
        assert_eq!(ctx.summaries.len(), 2);

        let replayed: Vec<String> = store
            .replay(ReplayQuery {
                topic: "_forum.*".to_string(),
                start_time: None,
                end_time: None,
            })
            .await
            .unwrap()
            .map(|r| r.unwrap().id)
            .collect()
            .await;
        assert_eq!(replayed, vec!["m00", "m04", "m08", "m12", "m16"]);
    }

    #[test]
    fn test_get_history_clamps_limit() {
        let store = Store::new_in_memory().unwrap();
        for i in 0..5 {
            store
                .insert_message(&msg(&format!("h{i}"), "t", "b", 10 - i))
                .unwrap();
        }
        let all = store.get_history(usize::MAX).unwrap();
        assert_eq!(all.len(), 5);
        // Chronological order.
        assert_eq!(all[0].id, "h0");
        assert_eq!(all[4].id, "h4");
        assert_eq!(store.get_history(2).unwrap().len(), 2);
        assert!(store.get_history(0).unwrap().is_empty());
    }

    #[test]
    fn test_get_history_limit_is_capped_at_max() {
        let store = Store::new_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let tx = conn.transaction().unwrap();
        let now = Utc::now();
        for i in 0..(MAX_HISTORY_LIMIT + 50) {
            tx.execute(
                "INSERT INTO messages (id, topic, from_identity, timestamp, content_type, body)
                 VALUES (?1, 't.x', 'a', ?2, 'text/plain', 'b')",
                params![
                    format!("c{i:05}"),
                    (now - Duration::seconds(i as i64)).to_rfc3339()
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        drop(conn);
        assert_eq!(
            store.get_history(usize::MAX).unwrap().len(),
            MAX_HISTORY_LIMIT
        );
        assert_eq!(
            store.get_history_matching("t.*", usize::MAX).unwrap().len(),
            MAX_HISTORY_LIMIT
        );
    }

    #[test]
    fn test_search_messages_escapes_and_bounds() {
        let store = Store::new_in_memory().unwrap();
        store
            .insert_message(&msg("s1", "t.a", "100% done", 2))
            .unwrap();
        store
            .insert_message(&msg("s2", "t.b", "plain text", 1))
            .unwrap();
        store
            .insert_message(&msg("s3", "t_c", "under_score", 0))
            .unwrap();
        assert!(store.search_messages("").unwrap().is_empty());
        assert!(store.search_messages("   ").unwrap().is_empty());
        let pct = store.search_messages("%").unwrap();
        assert_eq!(pct.len(), 1);
        assert_eq!(pct[0].id, "s1");
        let us: Vec<_> = store
            .search_messages("_")
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(us, vec!["s3"]);
        assert!(store.search_messages("\\").unwrap().is_empty());
        assert_eq!(store.search_messages("plain").unwrap().len(), 1);
    }

    #[test]
    fn test_search_messages_result_cap() {
        let store = Store::new_in_memory().unwrap();
        let mut conn = store.conn().unwrap();
        let tx = conn.transaction().unwrap();
        for i in 0..(MAX_SEARCH_RESULTS + 10) {
            tx.execute(
                "INSERT INTO messages (id, topic, from_identity, timestamp, content_type, body)
                 VALUES (?1, 't.x', 'a', ?2, 'text/plain', 'needle')",
                params![format!("n{i:05}"), Utc::now().to_rfc3339()],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        drop(conn);
        assert_eq!(
            store.search_messages("needle").unwrap().len(),
            MAX_SEARCH_RESULTS
        );
    }

    #[test]
    fn test_get_history_matching() {
        let store = Store::new_in_memory().unwrap();
        let topics = [
            "_forum.general",
            "_forum.games.mario",
            "xforum.general",
            "_forum",
            "_agent.codex.status",
            "_forum.general",
        ];
        for (i, t) in topics.iter().enumerate() {
            store
                .insert_message(&msg(&format!("g{i}"), t, "b", 100 - i as i64))
                .unwrap();
        }
        let ids = |p: &str, n: usize| -> Vec<String> {
            store
                .get_history_matching(p, n)
                .unwrap()
                .into_iter()
                .map(|m| m.id)
                .collect()
        };
        // Same chronological order as get_history; newest `limit` matches.
        assert_eq!(ids("_forum.**", 10), vec!["g0", "g1", "g3", "g5"]);
        assert_eq!(ids("_forum.**", 2), vec!["g3", "g5"]);
        assert_eq!(ids("_forum.*", 10), vec!["g0", "g5"]);
        assert_eq!(ids("_forum.>", 10), vec!["g0", "g1", "g5"]);
        assert_eq!(ids("*.general", 10), vec!["g0", "g2", "g5"]);
        assert_eq!(ids("**", 1), vec!["g5"]);
        assert_eq!(ids("_forum", 10), vec!["g3"]);
        assert!(ids("nothing.**", 10).is_empty());
        assert!(ids("**", 0).is_empty());
        // `_` and `%` in the literal prefix are matched literally.
        assert!(ids("%.**", 10).is_empty());
        assert_eq!(ids("_forum.general", 10), vec!["g0", "g5"]);
    }
}
