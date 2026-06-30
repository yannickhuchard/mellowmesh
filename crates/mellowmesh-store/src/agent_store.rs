use crate::Store;
use chrono::Utc;
use mellowmesh_core::agent::{AgentPresence, AgentRegistration};
use rusqlite::params;

impl Store {
    pub fn register_agent(&self, agent: &AgentRegistration) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let caps_json = serde_json::to_string(&agent.capabilities)?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO agents (id, name, owner, mode, capabilities, status, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                owner = excluded.owner,
                mode = excluded.mode,
                capabilities = excluded.capabilities,
                last_seen = excluded.last_seen",
            params![
                agent.id,
                agent.name,
                agent.owner,
                agent.mode,
                caps_json,
                "available", // Default status
                now
            ],
        )?;
        Ok(())
    }

    pub fn update_agent_status(&self, agent_id: &str, status: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE agents SET status = ?2, last_seen = ?3 WHERE id = ?1",
            params![agent_id, status, now],
        )?;
        Ok(())
    }

    pub fn get_agent(&self, id: &str) -> anyhow::Result<Option<AgentRegistration>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT id, name, owner, mode, capabilities FROM agents WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let caps_str: String = row.get(4)?;
            let capabilities = serde_json::from_str(&caps_str)?;
            Ok(Some(AgentRegistration {
                id: row.get(0)?,
                name: row.get(1)?,
                owner: row.get(2)?,
                mode: row.get(3)?,
                capabilities,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_agent_presence(&self, id: &str) -> anyhow::Result<Option<AgentPresence>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, status, mode, capabilities, last_seen FROM agents WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let caps_str: String = row.get(3)?;
            let capabilities = serde_json::from_str(&caps_str)?;
            let ts_str: String = row.get(4)?;
            let last_seen =
                chrono::DateTime::parse_from_rfc3339(&ts_str)?.with_timezone(&chrono::Utc);

            Ok(Some(AgentPresence {
                agent_id: row.get(0)?,
                status: row.get(1)?,
                mode: row.get(2)?,
                capabilities,
                current_work: vec![], // Populate manually if needed, or leave empty for MVP 1
                last_seen,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_agents(&self) -> anyhow::Result<Vec<AgentRegistration>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, name, owner, mode, capabilities FROM agents")?;
        let rows = stmt.query_map([], |row| {
            let caps_str: String = row.get(4)?;
            let capabilities = serde_json::from_str(&caps_str).unwrap_or_default();
            Ok(AgentRegistration {
                id: row.get(0)?,
                name: row.get(1)?,
                owner: row.get(2)?,
                mode: row.get(3)?,
                capabilities,
            })
        })?;

        let mut agents = Vec::new();
        for r in rows {
            agents.push(r?);
        }
        Ok(agents)
    }
}
