use crate::Store;
use mellowmesh_core::message::Message;
use mellowmesh_core::topic::{match_topic, pattern_literal_prefix};
use rusqlite::params;

/// Upper bound on the number of messages a history query returns.
pub const MAX_HISTORY_LIMIT: usize = 1000;
/// Upper bound on the number of messages a search query returns.
pub const MAX_SEARCH_RESULTS: usize = 500;

/// Columns selected for a [`Message`], in the order [`message_from_row`]
/// expects them.
pub(crate) const MESSAGE_COLUMNS: &str =
    "id, topic, from_identity, owner_identity, timestamp, content_type, body, headers, payload, parent_id";

/// Builds a [`Message`] from a row selected with [`MESSAGE_COLUMNS`].
///
/// Returns `Ok(None)` when the stored timestamp cannot be parsed, so a single
/// malformed row is skipped instead of failing the whole query.
pub(crate) fn message_from_row(row: &rusqlite::Row) -> rusqlite::Result<Option<Message>> {
    let ts_str: String = row.get(4)?;
    let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(&ts_str) else {
        return Ok(None);
    };
    let headers_str: Option<String> = row.get(7)?;
    let headers = headers_str.and_then(|s| serde_json::from_str(&s).ok());
    let payload_str: Option<String> = row.get(8)?;
    let payload = payload_str.and_then(|s| serde_json::from_str(&s).ok());

    Ok(Some(Message {
        id: row.get(0)?,
        topic: row.get(1)?,
        from: row.get(2)?,
        owner: row.get(3)?,
        timestamp: timestamp.with_timezone(&chrono::Utc),
        content_type: row.get(5)?,
        body: row.get(6)?,
        headers,
        payload,
        parent_id: row.get(9)?,
    }))
}

/// Escapes `%`, `_` and `\` so `s` is matched literally by
/// `LIKE ... ESCAPE '\'`.
pub(crate) fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A `LIKE` pattern (for use with `ESCAPE '\'`) matching every topic that
/// `pattern` can match, or `None` when the pattern has no literal prefix.
pub(crate) fn topic_prefix_like(pattern: &str) -> Option<String> {
    let prefix = pattern_literal_prefix(pattern);
    if prefix.is_empty() {
        None
    } else {
        Some(format!("{}%", escape_like(&prefix)))
    }
}

/// Quotes user input as a single FTS5 phrase so that punctuation and
/// operators in it (`'`, `+`, `-`, `"`, `*`, `AND`, ...) are never parsed as
/// query syntax.
pub(crate) fn fts5_phrase(query: &str) -> String {
    format!("\"{}\"", query.replace('"', "\"\""))
}

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
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"
        ))?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let ts_str: String = row.get(4)?;
            chrono::DateTime::parse_from_rfc3339(&ts_str)?;
            Ok(message_from_row(row)?)
        } else {
            Ok(None)
        }
    }

    /// Returns the `limit` most recent messages (at most
    /// [`MAX_HISTORY_LIMIT`]) in chronological order.
    pub fn get_history(&self, limit: usize) -> anyhow::Result<Vec<Message>> {
        let limit = limit.min(MAX_HISTORY_LIMIT);
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages ORDER BY timestamp DESC LIMIT ?1"
        ))?;
        let mut rows = stmt.query(params![limit as i64])?;

        let mut messages = Vec::new();
        while let Some(row) = rows.next()? {
            if let Some(m) = message_from_row(row)? {
                messages.push(m);
            }
        }
        // Reverse so that history is returned in chronological order
        messages.reverse();
        Ok(messages)
    }

    /// Like [`Store::get_history`], but only messages whose topic matches
    /// the topic `pattern` (`*`, `**`, `>` wildcards) are returned: the
    /// `limit` most recent matching messages (at most
    /// [`MAX_HISTORY_LIMIT`]), in the same chronological order as
    /// `get_history`.
    ///
    /// The literal prefix of the pattern is applied in SQL, and rows are
    /// read with a cursor that stops as soon as `limit` matches are found.
    pub fn get_history_matching(
        &self,
        pattern: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<Message>> {
        let limit = limit.min(MAX_HISTORY_LIMIT);
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let prefix_like = topic_prefix_like(pattern);
        let sql = if prefix_like.is_some() {
            format!(
                "SELECT {MESSAGE_COLUMNS} FROM messages WHERE topic LIKE ?1 ESCAPE '\\' ORDER BY timestamp DESC"
            )
        } else {
            format!("SELECT {MESSAGE_COLUMNS} FROM messages ORDER BY timestamp DESC")
        };
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = match &prefix_like {
            Some(like) => stmt.query(params![like])?,
            None => stmt.query([])?,
        };

        let mut messages = Vec::new();
        while let Some(row) = rows.next()? {
            let topic: String = row.get(1)?;
            if !match_topic(pattern, &topic) {
                continue;
            }
            if let Some(m) = message_from_row(row)? {
                messages.push(m);
                if messages.len() >= limit {
                    break;
                }
            }
        }
        messages.reverse();
        Ok(messages)
    }

    /// Substring search over message bodies and topics, newest first, at
    /// most [`MAX_SEARCH_RESULTS`] results. `%`, `_` and `\` in `query` are
    /// matched literally; an empty or whitespace-only query matches nothing.
    pub fn search_messages(&self, query: &str) -> anyhow::Result<Vec<Message>> {
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn()?;
        let search_pattern = format!("%{}%", escape_like(query));
        let mut stmt = conn.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM messages
             WHERE body LIKE ?1 ESCAPE '\\' OR topic LIKE ?1 ESCAPE '\\'
             ORDER BY timestamp DESC LIMIT ?2"
        ))?;
        let mut rows = stmt.query(params![search_pattern, MAX_SEARCH_RESULTS as i64])?;

        let mut messages = Vec::new();
        while let Some(row) = rows.next()? {
            if let Some(m) = message_from_row(row)? {
                messages.push(m);
            }
        }
        Ok(messages)
    }

    /// Delete all messages on `topic` older than the RFC3339 `cutoff`
    /// timestamp, including their full-text-search index entries.
    /// Returns the number of messages removed.
    pub fn delete_messages_before(&self, topic: &str, cutoff: &str) -> anyhow::Result<usize> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM messages_fts WHERE rowid IN (SELECT rowid FROM messages WHERE topic = ?1 AND timestamp < ?2)",
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
