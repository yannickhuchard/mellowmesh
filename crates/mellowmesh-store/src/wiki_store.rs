use crate::Store;
use mellowmesh_core::okf::OKFDocument;
use rusqlite::params;

impl Store {
    pub fn save_wiki_page(&self, doc: &OKFDocument) -> anyhow::Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        // Save metadata and content
        let tags_json = serde_json::to_string(&doc.tags)?;
        tx.execute(
            "INSERT INTO wiki_pages (wiki, path, doc_type, title, description, tags, timestamp, resource, body)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(wiki, path) DO UPDATE SET
                doc_type = excluded.doc_type,
                title = excluded.title,
                description = excluded.description,
                tags = excluded.tags,
                timestamp = excluded.timestamp,
                resource = excluded.resource,
                body = excluded.body",
            params![
                doc.wiki,
                doc.path,
                doc.doc_type,
                doc.title,
                doc.description,
                tags_json,
                doc.timestamp.to_rfc3339(),
                doc.resource,
                doc.body
            ],
        )?;

        // Update FTS index: delete first, then insert
        tx.execute(
            "DELETE FROM wiki_pages_fts WHERE wiki = ?1 AND path = ?2",
            params![doc.wiki, doc.path],
        )?;

        tx.execute(
            "INSERT INTO wiki_pages_fts (wiki, path, title, description, body)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![doc.wiki, doc.path, doc.title, doc.description, doc.body],
        )?;

        // Rebuild links (delete existing and insert new)
        tx.execute(
            "DELETE FROM wiki_links WHERE wiki = ?1 AND source_path = ?2",
            params![doc.wiki, doc.path],
        )?;

        for link in &doc.links {
            tx.execute(
                "INSERT OR IGNORE INTO wiki_links (wiki, source_path, target_path)
                 VALUES (?1, ?2, ?3)",
                params![doc.wiki, doc.path, link],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    pub fn delete_wiki_page(&self, wiki: &str, path: &str) -> anyhow::Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;

        tx.execute(
            "DELETE FROM wiki_pages WHERE wiki = ?1 AND path = ?2",
            params![wiki, path],
        )?;

        tx.execute(
            "DELETE FROM wiki_pages_fts WHERE wiki = ?1 AND path = ?2",
            params![wiki, path],
        )?;

        tx.execute(
            "DELETE FROM wiki_links WHERE wiki = ?1 AND source_path = ?2",
            params![wiki, path],
        )?;

        tx.commit()?;
        Ok(())
    }

    pub fn get_wiki_page(&self, wiki: &str, path: &str) -> anyhow::Result<Option<OKFDocument>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT wiki, path, doc_type, title, description, tags, timestamp, resource, body 
             FROM wiki_pages 
             WHERE wiki = ?1 AND path = ?2",
        )?;
        let mut rows = stmt.query(params![wiki, path])?;

        if let Some(row) = rows.next()? {
            let tags_str: String = row.get(5)?;
            let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
            let ts_str: String = row.get(6)?;
            let timestamp =
                chrono::DateTime::parse_from_rfc3339(&ts_str)?.with_timezone(&chrono::Utc);

            // Fetch links
            let mut link_stmt = conn.prepare(
                "SELECT target_path FROM wiki_links WHERE wiki = ?1 AND source_path = ?2",
            )?;
            let link_rows = link_stmt.query_map(params![wiki, path], |r| r.get::<_, String>(0))?;
            let mut links = Vec::new();
            for link in link_rows {
                links.push(link?);
            }

            Ok(Some(OKFDocument {
                wiki: row.get(0)?,
                path: row.get(1)?,
                doc_type: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                tags,
                timestamp,
                resource: row.get(7)?,
                body: row.get(8)?,
                links,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn search_wiki(
        &self,
        wiki: &str,
        query: &str,
        doc_type: Option<&str>,
        tag: Option<&str>,
    ) -> anyhow::Result<Vec<OKFDocument>> {
        let conn = self.conn()?;
        let mut results = Vec::new();

        let docs = if !query.trim().is_empty() {
            // Using FTS search
            let mut stmt = conn.prepare(
                "SELECT w.wiki, w.path, w.doc_type, w.title, w.description, w.tags, w.timestamp, w.resource, w.body
                 FROM wiki_pages w
                 JOIN wiki_pages_fts f ON w.wiki = f.wiki AND w.path = f.path
                 WHERE w.wiki = ?1 AND f.body MATCH ?2"
            )?;
            let rows = stmt.query_map(params![wiki, query], |row| self.row_to_doc(row))?;
            let mut list = Vec::new();
            for r in rows {
                list.push(r?);
            }
            list
        } else {
            // Fetch all in wiki
            let mut stmt = conn.prepare(
                "SELECT wiki, path, doc_type, title, description, tags, timestamp, resource, body
                 FROM wiki_pages
                 WHERE wiki = ?1",
            )?;
            let rows = stmt.query_map(params![wiki], |row| self.row_to_doc(row))?;
            let mut list = Vec::new();
            for r in rows {
                list.push(r?);
            }
            list
        };

        for doc in docs {
            // Filter by doc_type if specified
            if let Some(dt) = doc_type {
                if doc.doc_type != dt {
                    continue;
                }
            }
            // Filter by tag if specified
            if let Some(t) = tag {
                if !doc.tags.contains(&t.to_string()) {
                    continue;
                }
            }
            results.push(doc);
        }

        // Fetch links for each matching document
        for doc in &mut results {
            let mut link_stmt = conn.prepare(
                "SELECT target_path FROM wiki_links WHERE wiki = ?1 AND source_path = ?2",
            )?;
            let link_rows =
                link_stmt.query_map(params![doc.wiki, doc.path], |r| r.get::<_, String>(0))?;
            for link in link_rows {
                doc.links.push(link?);
            }
        }

        Ok(results)
    }

    pub fn list_wiki_pages(&self, wiki: &str) -> anyhow::Result<Vec<OKFDocument>> {
        self.search_wiki(wiki, "", None, None)
    }

    pub fn get_wiki_incoming_links(&self, wiki: &str, path: &str) -> anyhow::Result<Vec<String>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT source_path FROM wiki_links WHERE wiki = ?1 AND target_path = ?2")?;
        let rows = stmt.query_map(params![wiki, path], |row| row.get::<_, String>(0))?;
        let mut incoming = Vec::new();
        for r in rows {
            incoming.push(r?);
        }
        Ok(incoming)
    }

    fn row_to_doc(&self, row: &rusqlite::Row) -> Result<OKFDocument, rusqlite::Error> {
        let tags_str: String = row.get(5)?;
        let tags: Vec<String> = serde_json::from_str(&tags_str).unwrap_or_default();
        let ts_str: String = row.get(6)?;
        let timestamp = chrono::DateTime::parse_from_rfc3339(&ts_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;

        Ok(OKFDocument {
            wiki: row.get(0)?,
            path: row.get(1)?,
            doc_type: row.get(2)?,
            title: row.get(3)?,
            description: row.get(4)?,
            tags,
            timestamp,
            resource: row.get(7)?,
            body: row.get(8)?,
            links: Vec::new(), // populated later
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_wiki_store_crud_and_links() {
        let store = Store::new_in_memory().unwrap();

        let doc1 = OKFDocument {
            wiki: "default".to_string(),
            path: "procedures/deploy.md".to_string(),
            doc_type: "procedure".to_string(),
            title: "Deployment Guide".to_string(),
            description: Some("How to deploy".to_string()),
            tags: vec!["deploy".to_string(), "prod".to_string()],
            timestamp: Utc::now(),
            resource: Some("deploy-pipeline".to_string()),
            body: "Refer to [credentials](auth/keys.md) first.".to_string(),
            links: vec!["auth/keys.md".to_string()],
        };

        let doc2 = OKFDocument {
            wiki: "default".to_string(),
            path: "auth/keys.md".to_string(),
            doc_type: "resource".to_string(),
            title: "Access Keys".to_string(),
            description: Some("System keys".to_string()),
            tags: vec!["auth".to_string(), "security".to_string()],
            timestamp: Utc::now(),
            resource: None,
            body: "Keys are stored in vaults.".to_string(),
            links: vec![],
        };

        // Test Save
        store.save_wiki_page(&doc1).unwrap();
        store.save_wiki_page(&doc2).unwrap();

        // Test Get
        let retrieved = store
            .get_wiki_page("default", "procedures/deploy.md")
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.title, "Deployment Guide");
        assert_eq!(retrieved.tags, vec!["deploy", "prod"]);
        assert_eq!(retrieved.links, vec!["auth/keys.md"]);

        // Test Search
        let search_results = store.search_wiki("default", "vaults", None, None).unwrap();
        assert_eq!(search_results.len(), 1);
        assert_eq!(search_results[0].path, "auth/keys.md");

        // Test filter by tag
        let prod_results = store
            .search_wiki("default", "", None, Some("prod"))
            .unwrap();
        assert_eq!(prod_results.len(), 1);
        assert_eq!(prod_results[0].path, "procedures/deploy.md");

        // Test Backlinks (Incoming links)
        let backlinks = store
            .get_wiki_incoming_links("default", "auth/keys.md")
            .unwrap();
        assert_eq!(backlinks, vec!["procedures/deploy.md".to_string()]);

        // Test Delete
        store
            .delete_wiki_page("default", "procedures/deploy.md")
            .unwrap();
        assert!(store
            .get_wiki_page("default", "procedures/deploy.md")
            .unwrap()
            .is_none());
        assert!(store
            .get_wiki_incoming_links("default", "auth/keys.md")
            .unwrap()
            .is_empty());
    }
}
