use crate::Store;
use mellowmesh_core::message::Message;
use mellowmesh_core::persistence::{
    ContextQuery, ContextResult, EventStore, IndexableMessage, MemoryStore, MessageStream,
    PersistableMessage, PersistenceMode, QueryStore, ReplayQuery, Result, SearchQuery,
    SearchResult, TopicSummary,
};
use mellowmesh_core::telemetry::{TraceLevel, TraceSession};
use rusqlite::params;

#[async_trait::async_trait]
impl EventStore for Store {
    async fn persist_batch(&self, batch: Vec<PersistableMessage>) -> Result<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = store.conn()?;
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT INTO messages (id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, persistence_mode, parent_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                     ON CONFLICT(id) DO UPDATE SET
                        topic = excluded.topic,
                        from_identity = excluded.from_identity,
                        owner_identity = excluded.owner_identity,
                        timestamp = excluded.timestamp,
                        content_type = excluded.content_type,
                        body = excluded.body,
                        headers = excluded.headers,
                        payload = excluded.payload,
                        persistence_mode = excluded.persistence_mode,
                        parent_id = excluded.parent_id"
                )?;
                for pm in batch {
                    let msg = &pm.message;
                    let body = match pm.mode {
                        PersistenceMode::Ephemeral => continue,
                        PersistenceMode::Metadata => {
                            format!("Metadata-only: size={}", msg.body.len())
                        }
                        PersistenceMode::EventLog | PersistenceMode::Queryable => {
                            msg.body.clone()
                        }
                    };
                    let headers_json = msg.headers.as_ref().map(|h| serde_json::to_string(h).unwrap());
                    let payload_json = msg.payload.as_ref().map(|p| serde_json::to_string(p).unwrap());
                    let mode_str = match pm.mode {
                        PersistenceMode::Ephemeral => "ephemeral",
                        PersistenceMode::Metadata => "metadata",
                        PersistenceMode::EventLog => "event_log",
                        PersistenceMode::Queryable => "queryable",
                    };
                    stmt.execute(params![
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
                }
            }
            tx.commit()?;
            Ok::<(), anyhow::Error>(())
        }).await??;
        Ok(())
    }

    async fn replay(&self, query: ReplayQuery) -> Result<MessageStream> {
        let store = self.clone();
        let msgs = tokio::task::spawn_blocking(move || {
            let conn = store.conn()?;
            let mut sql = "SELECT id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id FROM messages WHERE 1=1".to_string();
            let mut params_vec: Vec<rusqlite::types::Value> = Vec::new();

            if let Some(start) = query.start_time {
                sql.push_str(" AND timestamp >= ?");
                params_vec.push(rusqlite::types::Value::Text(start.to_rfc3339()));
            }
            if let Some(end) = query.end_time {
                sql.push_str(" AND timestamp <= ?");
                params_vec.push(rusqlite::types::Value::Text(end.to_rfc3339()));
            }
            sql.push_str(" ORDER BY timestamp ASC");

            let mut stmt = conn.prepare(&sql)?;
            let params_ref: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
            let rows = stmt.query_map(&*params_ref, |row| {
                let ts_str: String = row.get(4)?;
                let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
                let headers_str: Option<String> = row.get(7)?;
                let headers = headers_str.and_then(|s| serde_json::from_str(&s).ok());
                let payload_str: Option<String> = row.get(8)?;
                let payload = payload_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Message {
                    id: row.get(0)?,
                    topic: row.get(1)?,
                    from: row.get(2)?,
                    owner: row.get(3)?,
                    timestamp,
                    content_type: row.get(5)?,
                    body: row.get(6)?,
                    headers,
                    payload,
                    parent_id: row.get(9)?,
                })
            })?;

            let mut results = Vec::new();
            for r in rows {
                let m = r?;
                if mellowmesh_core::topic::match_topic(&query.topic, &m.topic) {
                    results.push(m);
                }
            }
            Ok::<Vec<Message>, anyhow::Error>(results)
        }).await??;

        let stream = futures_util::stream::iter(msgs.into_iter().map(Ok));
        Ok(Box::pin(stream))
    }
}

#[async_trait::async_trait]
impl QueryStore for Store {
    async fn index_batch(&self, batch: Vec<IndexableMessage>) -> Result<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = store.conn()?;
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "INSERT OR REPLACE INTO messages_fts (id, topic, body) VALUES (?1, ?2, ?3)",
                )?;
                for im in batch {
                    stmt.execute(params![im.message.id, im.message.topic, im.message.body])?;
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
            let conn = store.conn()?;
            let mut stmt = conn.prepare(
                "SELECT m.id, m.topic, m.from_identity, m.owner_identity, m.timestamp, m.content_type, m.body, m.headers, m.payload, m.parent_id 
                 FROM messages_fts fts 
                 JOIN messages m ON fts.id = m.id 
                 WHERE messages_fts MATCH ?1 
                 ORDER BY m.timestamp DESC"
            ).or_else(|_| {
                conn.prepare(
                    "SELECT id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id 
                     FROM messages 
                     WHERE body LIKE ?1 OR topic LIKE ?1 
                     ORDER BY timestamp DESC"
                )
            })?;

            let query_param = if stmt.column_count() == 10 {
                format!("%{}%", query.query)
            } else {
                query.query.clone()
            };

            let rows = stmt.query_map(params![query_param], |row| {
                let ts_str: String = row.get(4)?;
                let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
                let headers_str: Option<String> = row.get(7)?;
                let headers = headers_str.and_then(|s| serde_json::from_str(&s).ok());
                let payload_str: Option<String> = row.get(8)?;
                let payload = payload_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Message {
                    id: row.get(0)?,
                    topic: row.get(1)?,
                    from: row.get(2)?,
                    owner: row.get(3)?,
                    timestamp,
                    content_type: row.get(5)?,
                    body: row.get(6)?,
                    headers,
                    payload,
                    parent_id: row.get(9)?,
                })
            })?;

            let mut results = Vec::new();
            for r in rows {
                let m = r?;
                if let Some(ref pat) = query.topic_pattern {
                    if mellowmesh_core::topic::match_topic(pat, &m.topic) {
                        results.push(m);
                    }
                } else {
                    results.push(m);
                }
            }
            Ok::<Vec<Message>, anyhow::Error>(results)
        }).await??;

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

            let mut stmt = conn.prepare("SELECT topic, summary, generated_at FROM topic_summaries")?;
            let rows = stmt.query_map([], |row| {
                let ts_str: String = row.get(2)?;
                let generated_at = chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e)))?;
                Ok(TopicSummary {
                    topic: row.get(0)?,
                    summary: row.get(1)?,
                    generated_at,
                })
            })?;

            let mut matching_summaries = Vec::new();
            for r in rows {
                let s = r?;
                if mellowmesh_core::topic::match_topic(&query.topic, &s.topic) {
                    matching_summaries.push(s);
                }
            }

            let mut stmt = conn.prepare("SELECT id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id FROM messages ORDER BY timestamp DESC")?;
            let rows = stmt.query_map([], |row| {
                let ts_str: String = row.get(4)?;
                let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
                let headers_str: Option<String> = row.get(7)?;
                let headers = headers_str.and_then(|s| serde_json::from_str(&s).ok());
                let payload_str: Option<String> = row.get(8)?;
                let payload = payload_str.and_then(|s| serde_json::from_str(&s).ok());

                Ok(Message {
                    id: row.get(0)?,
                    topic: row.get(1)?,
                    from: row.get(2)?,
                    owner: row.get(3)?,
                    timestamp,
                    content_type: row.get(5)?,
                    body: row.get(6)?,
                    headers,
                    payload,
                    parent_id: row.get(9)?,
                })
            })?;

            let mut relevant_messages = Vec::new();
            for r in rows {
                let m = r?;
                if mellowmesh_core::topic::match_topic(&query.topic, &m.topic) {
                    relevant_messages.push(m);
                    if relevant_messages.len() >= query.limit {
                        break;
                    }
                }
            }

            // Resolve lineage for relevant messages
            let mut lineage_messages = Vec::new();
            let mut visited = std::collections::HashSet::new();
            let mut get_msg_stmt = conn.prepare(
                "SELECT id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id FROM messages WHERE id = ?1"
            )?;

            for msg in &relevant_messages {
                let mut current_parent = msg.parent_id.clone();
                let mut depth = 0;
                while let Some(parent_id) = current_parent {
                    if visited.contains(&parent_id) || depth >= 10 {
                        break;
                    }
                    visited.insert(parent_id.clone());

                    let mut rows = get_msg_stmt.query(params![parent_id])?;
                    if let Some(row) = rows.next()? {
                        let ts_str: String = row.get(4)?;
                        let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                            .map(|dt| dt.with_timezone(&chrono::Utc))
                            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e)))?;
                        let headers_str: Option<String> = row.get(7)?;
                        let headers = headers_str.and_then(|s| serde_json::from_str(&s).ok());
                        let payload_str: Option<String> = row.get(8)?;
                        let payload = payload_str.and_then(|s| serde_json::from_str(&s).ok());

                        let parent_msg = Message {
                            id: row.get(0)?,
                            topic: row.get(1)?,
                            from: row.get(2)?,
                            owner: row.get(3)?,
                            timestamp,
                            content_type: row.get(5)?,
                            body: row.get(6)?,
                            headers,
                            payload,
                            parent_id: row.get(9)?,
                        };
                        current_parent = parent_msg.parent_id.clone();
                        lineage_messages.push(parent_msg);
                    } else {
                        break;
                    }
                    depth += 1;
                }
            }

            Ok::<(_, _, _), anyhow::Error>((matching_summaries, relevant_messages, lineage_messages))
        }).await??;

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
