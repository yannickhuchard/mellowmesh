use crate::Store;
use mellowmesh_core::task::Task;
use rusqlite::params;

impl Store {
    pub fn insert_task(&self, task: &Task) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let topics_json = serde_json::to_string(&task.topics)?;
        let caps_json = serde_json::to_string(&task.required_capabilities)?;
        let artifacts_json = serde_json::to_string(&task.artifacts)?;
        let decisions_json = serde_json::to_string(&task.decisions)?;

        conn.execute(
            "INSERT INTO tasks (id, title, description, created_from, created_by, status, priority, topics, required_capabilities, assigned_to, claimed_by, deadline, artifacts, decisions, parent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                task.id,
                task.title,
                task.description,
                task.created_from,
                task.created_by,
                task.status,
                task.priority,
                topics_json,
                caps_json,
                task.assigned_to,
                task.claimed_by,
                task.deadline,
                artifacts_json,
                decisions_json,
                task.parent_id
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> anyhow::Result<Option<Task>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, title, description, created_from, created_by, status, priority, topics, required_capabilities, assigned_to, claimed_by, deadline, artifacts, decisions, parent_id FROM tasks WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let topics_str: String = row.get(7)?;
            let caps_str: String = row.get(8)?;
            let artifacts_str: String = row.get(12)?;
            let decisions_str: String = row.get(13)?;

            Ok(Some(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                created_from: row.get(3)?,
                created_by: row.get(4)?,
                status: row.get(5)?,
                priority: row.get(6)?,
                topics: serde_json::from_str(&topics_str)?,
                required_capabilities: serde_json::from_str(&caps_str)?,
                assigned_to: row.get(9)?,
                claimed_by: row.get(10)?,
                deadline: row.get(11)?,
                artifacts: serde_json::from_str(&artifacts_str)?,
                decisions: serde_json::from_str(&decisions_str)?,
                parent_id: row.get(14)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, title, description, created_from, created_by, status, priority, topics, required_capabilities, assigned_to, claimed_by, deadline, artifacts, decisions, parent_id FROM tasks")?;
        let rows = stmt.query_map([], |row| {
            let topics_str: String = row.get(7)?;
            let caps_str: String = row.get(8)?;
            let artifacts_str: String = row.get(12)?;
            let decisions_str: String = row.get(13)?;

            Ok(Task {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                created_from: row.get(3)?,
                created_by: row.get(4)?,
                status: row.get(5)?,
                priority: row.get(6)?,
                topics: serde_json::from_str(&topics_str).unwrap_or_default(),
                required_capabilities: serde_json::from_str(&caps_str).unwrap_or_default(),
                assigned_to: row.get(9)?,
                claimed_by: row.get(10)?,
                deadline: row.get(11)?,
                artifacts: serde_json::from_str(&artifacts_str).unwrap_or_default(),
                decisions: serde_json::from_str(&decisions_str).unwrap_or_default(),
                parent_id: row.get(14)?,
            })
        })?;

        let mut tasks = Vec::new();
        for r in rows {
            tasks.push(r?);
        }
        Ok(tasks)
    }

    pub fn claim_task(&self, id: &str, claimed_by: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE tasks SET claimed_by = ?2, status = 'claimed' WHERE id = ?1",
            params![id, claimed_by],
        )?;
        Ok(())
    }

    pub fn complete_task(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE tasks SET status = 'completed' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}
