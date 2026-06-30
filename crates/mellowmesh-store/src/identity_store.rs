use crate::Store;
use rusqlite::params;

impl Store {
    pub fn insert_identity_mapping(&self, ext_id: &str, fm_id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO identity_mappings (external_id, mellowmesh_id)
             VALUES (?1, ?2)
             ON CONFLICT(external_id) DO UPDATE SET mellowmesh_id = excluded.mellowmesh_id",
            params![ext_id, fm_id],
        )?;
        Ok(())
    }

    pub fn get_mellowmesh_id(&self, ext_id: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT mellowmesh_id FROM identity_mappings WHERE external_id = ?1")?;
        let mut rows = stmt.query(params![ext_id])?;
        if let Some(row) = rows.next()? {
            let mellowmesh_id: String = row.get(0)?;
            Ok(Some(mellowmesh_id))
        } else {
            Ok(None)
        }
    }

    pub fn list_identity_mappings(&self) -> anyhow::Result<Vec<(String, String)>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT external_id, mellowmesh_id FROM identity_mappings")?;
        let rows = stmt.query_map([], |row| {
            let ext_id: String = row.get(0)?;
            let fm_id: String = row.get(1)?;
            Ok((ext_id, fm_id))
        })?;

        let mut mappings = Vec::new();
        for r in rows {
            mappings.push(r?);
        }
        Ok(mappings)
    }
}
