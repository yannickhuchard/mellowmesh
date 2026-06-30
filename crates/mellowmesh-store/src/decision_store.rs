use crate::Store;
use chrono::Utc;
use mellowmesh_core::decision::Decision;
use rusqlite::params;

impl Store {
    pub fn insert_decision(&self, dec: &Decision) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let options_json = serde_json::to_string(&dec.options)?;
        let ts_str = dec.response_timestamp.map(|t| t.to_rfc3339());

        conn.execute(
            "INSERT INTO decisions (id, title, question, created_by, required_decider, status, options, response_option_id, response_timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                dec.id,
                dec.title,
                dec.question,
                dec.created_by,
                dec.required_decider,
                dec.status,
                options_json,
                dec.response_option_id,
                ts_str
            ],
        )?;
        Ok(())
    }

    pub fn get_decision(&self, id: &str) -> anyhow::Result<Option<Decision>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, title, question, created_by, required_decider, status, options, response_option_id, response_timestamp FROM decisions WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            let options_str: String = row.get(6)?;
            let ts_str: Option<String> = row.get(8)?;
            let response_timestamp = ts_str.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .ok()
            });

            Ok(Some(Decision {
                id: row.get(0)?,
                title: row.get(1)?,
                question: row.get(2)?,
                created_by: row.get(3)?,
                required_decider: row.get(4)?,
                status: row.get(5)?,
                options: serde_json::from_str(&options_str)?,
                response_option_id: row.get(7)?,
                response_timestamp,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_decisions(&self) -> anyhow::Result<Vec<Decision>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT id, title, question, created_by, required_decider, status, options, response_option_id, response_timestamp FROM decisions")?;
        let rows = stmt.query_map([], |row| {
            let options_str: String = row.get(6)?;
            let ts_str: Option<String> = row.get(8)?;
            let response_timestamp = ts_str.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .ok()
            });

            Ok(Decision {
                id: row.get(0)?,
                title: row.get(1)?,
                question: row.get(2)?,
                created_by: row.get(3)?,
                required_decider: row.get(4)?,
                status: row.get(5)?,
                options: serde_json::from_str(&options_str).unwrap_or_default(),
                response_option_id: row.get(7)?,
                response_timestamp,
            })
        })?;

        let mut decisions = Vec::new();
        for r in rows {
            decisions.push(r?);
        }
        Ok(decisions)
    }

    pub fn respond_decision(&self, id: &str, option_id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE decisions SET response_option_id = ?2, response_timestamp = ?3, status = 'approved' WHERE id = ?1",
            params![id, option_id, now],
        )?;
        Ok(())
    }
}
