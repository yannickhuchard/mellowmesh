use crate::Store;
use mellowmesh_core::message::Message;
use rusqlite::params;

impl Store {
    pub fn insert_message(&self, msg: &Message) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let headers_json = msg
            .headers
            .as_ref()
            .map(|h| serde_json::to_string(h).unwrap());
        let payload_json = msg
            .payload
            .as_ref()
            .map(|p| serde_json::to_string(p).unwrap());

        conn.execute(
            "INSERT INTO messages (id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                msg.id,
                msg.topic,
                msg.from,
                msg.owner,
                msg.timestamp.to_rfc3339(),
                msg.content_type,
                msg.body,
                headers_json,
                payload_json,
                msg.parent_id
            ],
        )?;
        Ok(())
    }

    pub fn get_message(&self, id: &str) -> anyhow::Result<Option<Message>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id FROM messages WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let ts_str: String = row.get(4)?;
            let timestamp =
                chrono::DateTime::parse_from_rfc3339(&ts_str)?.with_timezone(&chrono::Utc);
            let headers_str: Option<String> = row.get(7)?;
            let headers = headers_str.and_then(|s| serde_json::from_str(&s).ok());
            let payload_str: Option<String> = row.get(8)?;
            let payload = payload_str.and_then(|s| serde_json::from_str(&s).ok());

            Ok(Some(Message {
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
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_history(&self, limit: usize) -> anyhow::Result<Vec<Message>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id FROM messages ORDER BY timestamp DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let ts_str: String = row.get(4)?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
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

        let mut messages = Vec::new();
        for r in rows {
            messages.push(r?);
        }
        // Reverse so that history is returned in chronological order
        messages.reverse();
        Ok(messages)
    }

    pub fn search_messages(&self, query: &str) -> anyhow::Result<Vec<Message>> {
        let conn = self.conn()?;
        let search_pattern = format!("%{query}%");
        let mut stmt = conn.prepare("SELECT id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id FROM messages WHERE body LIKE ?1 OR topic LIKE ?1 ORDER BY timestamp DESC")?;
        let rows = stmt.query_map(params![search_pattern], |row| {
            let ts_str: String = row.get(4)?;
            let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
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

        let mut messages = Vec::new();
        for r in rows {
            messages.push(r?);
        }
        Ok(messages)
    }

    /// Delete all messages on `topic` older than the RFC3339 `cutoff`
    /// timestamp, including their full-text-search index entries.
    /// Returns the number of messages removed.
    pub fn delete_messages_before(&self, topic: &str, cutoff: &str) -> anyhow::Result<usize> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM messages_fts WHERE id IN (SELECT id FROM messages WHERE topic = ?1 AND timestamp < ?2)",
            params![topic, cutoff],
        )?;
        let deleted = conn.execute(
            "DELETE FROM messages WHERE topic = ?1 AND timestamp < ?2",
            params![topic, cutoff],
        )?;
        Ok(deleted)
    }

    pub fn list_topics(&self) -> anyhow::Result<Vec<String>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT DISTINCT topic FROM messages ORDER BY topic")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        let mut topics = Vec::new();
        for r in rows {
            topics.push(r?);
        }
        Ok(topics)
    }
}
