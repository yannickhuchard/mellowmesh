use crate::Store;
use mellowmesh_core::topic::NamedTopic;
use rusqlite::params;

impl Store {
    pub fn register_named_topic(&self, named_topic: &NamedTopic) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO named_topics (name, topic)
             VALUES (?1, ?2)
             ON CONFLICT(name) DO UPDATE SET
                topic = excluded.topic",
            params![named_topic.name, named_topic.topic],
        )?;
        Ok(())
    }

    pub fn list_named_topics(&self) -> anyhow::Result<Vec<NamedTopic>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT name, topic FROM named_topics")?;
        let rows = stmt.query_map([], |row| {
            Ok(NamedTopic {
                name: row.get(0)?,
                topic: row.get(1)?,
            })
        })?;

        let mut topics = Vec::new();
        for r in rows {
            topics.push(r?);
        }
        Ok(topics)
    }

    pub fn remove_named_topic(&self, name: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute("DELETE FROM named_topics WHERE name = ?1", params![name])?;
        Ok(())
    }
}
