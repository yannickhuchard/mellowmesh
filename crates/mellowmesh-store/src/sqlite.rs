use directories::ProjectDirs;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Store {
    pool: Pool<SqliteConnectionManager>,
}

impl Store {
    pub fn new<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let manager = SqliteConnectionManager::file(path_ref).with_init(|c| {
            c.pragma_update(None, "busy_timeout", &5000)?;
            c.pragma_update(None, "journal_mode", &"WAL")?;
            c.pragma_update(None, "synchronous", &"NORMAL")?;
            Ok(())
        });
        let pool = Pool::new(manager)?;
        let store = Store { pool };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn new_in_memory() -> anyhow::Result<Self> {
        let manager = SqliteConnectionManager::memory().with_init(|c| {
            c.pragma_update(None, "busy_timeout", &5000)?;
            Ok(())
        });
        let pool = Pool::new(manager)?;
        let store = Store { pool };
        store.run_migrations()?;
        Ok(store)
    }

    pub fn conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, r2d2::Error> {
        self.pool.get()
    }

    fn run_migrations(&self) -> anyhow::Result<()> {
        let conn = self.conn()?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                topic TEXT NOT NULL,
                from_identity TEXT NOT NULL,
                owner_identity TEXT,
                timestamp TEXT NOT NULL,
                content_type TEXT NOT NULL,
                body TEXT NOT NULL,
                headers TEXT,
                payload TEXT
            )",
            [],
        )?;

        // Add persistence_mode column to messages table if not exists
        let _ = conn.execute(
            "ALTER TABLE messages ADD COLUMN persistence_mode TEXT DEFAULT 'queryable'",
            [],
        );

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(id, topic, body)",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS topic_summaries (
                topic TEXT PRIMARY KEY,
                summary TEXT NOT NULL,
                generated_at TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS trace_sessions (
                id TEXT PRIMARY KEY,
                target_type TEXT NOT NULL,
                target TEXT NOT NULL,
                level TEXT NOT NULL,
                enabled_by TEXT NOT NULL,
                reason TEXT,
                started_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                persistence_mode TEXT NOT NULL,
                retention TEXT NOT NULL,
                max_messages_per_second INTEGER,
                max_bytes_per_second INTEGER,
                topics TEXT NOT NULL,
                status TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                owner TEXT NOT NULL,
                mode TEXT NOT NULL,
                capabilities TEXT NOT NULL,
                status TEXT NOT NULL,
                last_seen TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                created_from TEXT,
                created_by TEXT NOT NULL,
                status TEXT NOT NULL,
                priority TEXT NOT NULL,
                topics TEXT NOT NULL,
                required_capabilities TEXT NOT NULL,
                assigned_to TEXT,
                claimed_by TEXT,
                deadline TEXT,
                artifacts TEXT NOT NULL,
                decisions TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS decisions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                question TEXT NOT NULL,
                created_by TEXT NOT NULL,
                required_decider TEXT NOT NULL,
                status TEXT NOT NULL,
                options TEXT NOT NULL,
                response_option_id TEXT,
                response_timestamp TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS identity_mappings (
                external_id TEXT PRIMARY KEY,
                mellowmesh_id TEXT NOT NULL
            )",
            [],
        )?;

        // Wiki Tables
        conn.execute(
            "CREATE TABLE IF NOT EXISTS wiki_pages (
                wiki TEXT NOT NULL,
                path TEXT NOT NULL,
                doc_type TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                tags TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                resource TEXT,
                body TEXT NOT NULL,
                PRIMARY KEY (wiki, path)
            )",
            [],
        )?;

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS wiki_pages_fts USING fts5(
                wiki,
                path,
                title,
                description,
                body
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS wiki_links (
                wiki TEXT NOT NULL,
                source_path TEXT NOT NULL,
                target_path TEXT NOT NULL,
                PRIMARY KEY (wiki, source_path, target_path),
                FOREIGN KEY (wiki, source_path) REFERENCES wiki_pages(wiki, path) ON DELETE CASCADE
            )",
            [],
        )?;

        // Migrations for parent_id in messages and tasks
        let _ = conn.execute("ALTER TABLE messages ADD COLUMN parent_id TEXT", []);
        let _ = conn.execute("ALTER TABLE tasks ADD COLUMN parent_id TEXT", []);

        // Table for Topic Schema Contracts
        conn.execute(
            "CREATE TABLE IF NOT EXISTS topic_schemas (
                topic_pattern TEXT NOT NULL,
                version TEXT NOT NULL,
                schema_content TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (topic_pattern, version)
            )",
            [],
        )?;

        // Table for Named Topics
        conn.execute(
            "CREATE TABLE IF NOT EXISTS named_topics (
                name TEXT PRIMARY KEY,
                topic TEXT NOT NULL
            )",
            [],
        )?;

        Ok(())
    }

    // Schema Management Implementations
    pub fn insert_schema(
        &self,
        schema: &mellowmesh_core::persistence::TopicSchema,
    ) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO topic_schemas (topic_pattern, version, schema_content, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(topic_pattern, version) DO UPDATE SET
                schema_content = excluded.schema_content,
                status = excluded.status,
                created_at = excluded.created_at",
            params![
                schema.topic_pattern,
                schema.version,
                schema.schema_content,
                schema.status,
                schema.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_schema(
        &self,
        topic_pattern: &str,
        version: &str,
    ) -> anyhow::Result<Option<mellowmesh_core::persistence::TopicSchema>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT topic_pattern, version, schema_content, status, created_at 
             FROM topic_schemas 
             WHERE topic_pattern = ?1 AND version = ?2",
        )?;
        let mut rows = stmt.query(params![topic_pattern, version])?;
        if let Some(row) = rows.next()? {
            let created_at_str: String = row.get(4)?;
            let created_at =
                chrono::DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&chrono::Utc);
            Ok(Some(mellowmesh_core::persistence::TopicSchema {
                topic_pattern: row.get(0)?,
                version: row.get(1)?,
                schema_content: row.get(2)?,
                status: row.get(3)?,
                created_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_schemas(&self) -> anyhow::Result<Vec<mellowmesh_core::persistence::TopicSchema>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT topic_pattern, version, schema_content, status, created_at 
             FROM topic_schemas 
             ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let created_at_str: String = row.get(4)?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            Ok(mellowmesh_core::persistence::TopicSchema {
                topic_pattern: row.get(0)?,
                version: row.get(1)?,
                schema_content: row.get(2)?,
                status: row.get(3)?,
                created_at,
            })
        })?;
        let mut schemas = Vec::new();
        for r in rows {
            schemas.push(r?);
        }
        Ok(schemas)
    }

    pub fn remove_schema(&self, topic_pattern: &str, version: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM topic_schemas WHERE topic_pattern = ?1 AND version = ?2",
            params![topic_pattern, version],
        )?;
        Ok(())
    }

    pub fn set_schema_status(
        &self,
        topic_pattern: &str,
        version: &str,
        status: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE topic_schemas SET status = ?3 WHERE topic_pattern = ?1 AND version = ?2",
            params![topic_pattern, version, status],
        )?;
        Ok(())
    }

    pub fn get_schemas_for_topic(
        &self,
        topic: &str,
    ) -> anyhow::Result<Vec<mellowmesh_core::persistence::TopicSchema>> {
        let schemas = self.list_schemas()?;
        let matched = schemas
            .into_iter()
            .filter(|s| {
                s.status == "active" && mellowmesh_core::topic::match_topic(&s.topic_pattern, topic)
            })
            .collect();
        Ok(matched)
    }
}

pub fn default_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("MELLOWMESH_DB") {
        return PathBuf::from(path);
    }
    if let Some(proj_dirs) = ProjectDirs::from("com", "mellowmesh", "mellowmesh") {
        let data_dir = proj_dirs.data_dir();
        return data_dir.join("mellowmesh.db");
    }
    PathBuf::from("mellowmesh.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mellowmesh_core::agent::AgentRegistration;
    use mellowmesh_core::decision::{Decision, DecisionOption};
    use mellowmesh_core::message::Message;
    use mellowmesh_core::task::Task;

    #[test]
    fn test_store_init() {
        let store = Store::new_in_memory();
        assert!(store.is_ok());
        let store = store.unwrap();
        assert!(store.conn().is_ok());
    }

    #[test]
    fn test_message_store() {
        let store = Store::new_in_memory().unwrap();
        let msg = Message {
            id: "msg_1".to_string(),
            topic: "test.topic".to_string(),
            from: "human://yannick".to_string(),
            owner: Some("human://yannick".to_string()),
            timestamp: Utc::now(),
            content_type: "text/plain".to_string(),
            body: "Hello MellowMesh".to_string(),
            headers: None,
            payload: None,
            parent_id: None,
        };

        store.insert_message(&msg).unwrap();

        let retrieved = store.get_message("msg_1").unwrap().unwrap();
        assert_eq!(retrieved.body, "Hello MellowMesh");
        assert_eq!(retrieved.topic, "test.topic");

        let history = store.get_history(10).unwrap();
        assert_eq!(history.len(), 1);

        let search = store.search_messages("MellowMesh").unwrap();
        assert_eq!(search.len(), 1);

        let topics = store.list_topics().unwrap();
        assert_eq!(topics, vec!["test.topic".to_string()]);
    }

    #[test]
    fn test_agent_store() {
        let store = Store::new_in_memory().unwrap();
        let agent = AgentRegistration {
            id: "agent://codex".to_string(),
            name: "codex".to_string(),
            owner: "human://yannick".to_string(),
            mode: "human-piloted".to_string(),
            capabilities: vec!["code.write".to_string()],
        };

        store.register_agent(&agent).unwrap();

        let retrieved = store.get_agent("agent://codex").unwrap().unwrap();
        assert_eq!(retrieved.name, "codex");
        assert_eq!(retrieved.capabilities, vec!["code.write".to_string()]);

        let list = store.list_agents().unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_task_store() {
        let store = Store::new_in_memory().unwrap();
        let task = Task {
            id: "task_1".to_string(),
            title: "Test Task".to_string(),
            description: Some("Description".to_string()),
            created_from: None,
            created_by: "human://yannick".to_string(),
            status: "open".to_string(),
            priority: "high".to_string(),
            topics: vec!["_project.test".to_string()],
            required_capabilities: vec!["code.write".to_string()],
            assigned_to: None,
            claimed_by: None,
            deadline: None,
            artifacts: vec![],
            decisions: vec![],
            parent_id: None,
        };

        store.insert_task(&task).unwrap();

        let retrieved = store.get_task("task_1").unwrap().unwrap();
        assert_eq!(retrieved.title, "Test Task");
        assert_eq!(retrieved.status, "open");

        store.claim_task("task_1", "agent://codex").unwrap();
        let claimed = store.get_task("task_1").unwrap().unwrap();
        assert_eq!(claimed.status, "claimed");
        assert_eq!(claimed.claimed_by, Some("agent://codex".to_string()));

        store.complete_task("task_1").unwrap();
        let completed = store.get_task("task_1").unwrap().unwrap();
        assert_eq!(completed.status, "completed");
    }

    #[test]
    fn test_decision_store() {
        let store = Store::new_in_memory().unwrap();
        let dec = Decision {
            id: "decision_1".to_string(),
            title: "Test Decision".to_string(),
            question: "Question?".to_string(),
            created_by: "agent://codex".to_string(),
            required_decider: "human://yannick".to_string(),
            status: "requested".to_string(),
            options: vec![DecisionOption {
                id: "option_1".to_string(),
                label: "Yes".to_string(),
                pros: vec![],
                cons: vec![],
            }],
            response_option_id: None,
            response_timestamp: None,
        };

        store.insert_decision(&dec).unwrap();

        let retrieved = store.get_decision("decision_1").unwrap().unwrap();
        assert_eq!(retrieved.title, "Test Decision");

        store.respond_decision("decision_1", "option_1").unwrap();
        let responded = store.get_decision("decision_1").unwrap().unwrap();
        assert_eq!(responded.status, "approved");
        assert_eq!(responded.response_option_id, Some("option_1".to_string()));
        assert!(responded.response_timestamp.is_some());
    }

    #[test]
    fn test_identity_mappings() {
        let store = Store::new_in_memory().unwrap();
        store
            .insert_identity_mapping("discord://12345", "human://yannick")
            .unwrap();

        let retrieved = store.get_mellowmesh_id("discord://12345").unwrap();
        assert_eq!(retrieved, Some("human://yannick".to_string()));

        let not_found = store.get_mellowmesh_id("discord://unknown").unwrap();
        assert_eq!(not_found, None);

        let list = store.list_identity_mappings().unwrap();
        assert_eq!(
            list,
            vec![("discord://12345".to_string(), "human://yannick".to_string())]
        );

        // test upsert
        store
            .insert_identity_mapping("discord://12345", "human://admin")
            .unwrap();
        let retrieved_updated = store.get_mellowmesh_id("discord://12345").unwrap();
        assert_eq!(retrieved_updated, Some("human://admin".to_string()));
    }

    #[test]
    fn test_schema_store() {
        let store = Store::new_in_memory().unwrap();
        let schema = mellowmesh_core::persistence::TopicSchema {
            topic_pattern: "_artifact.invoice.**".to_string(),
            version: "v1".to_string(),
            schema_content: r#"{"type": "object"}"#.to_string(),
            status: "active".to_string(),
            created_at: Utc::now(),
        };

        // Insert
        store.insert_schema(&schema).unwrap();

        // Get
        let retrieved = store
            .get_schema("_artifact.invoice.**", "v1")
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.topic_pattern, "_artifact.invoice.**");
        assert_eq!(retrieved.version, "v1");
        assert_eq!(retrieved.schema_content, r#"{"type": "object"}"#);
        assert_eq!(retrieved.status, "active");

        // List
        let list = store.list_schemas().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].topic_pattern, "_artifact.invoice.**");

        // Matching
        let matched = store
            .get_schemas_for_topic("_artifact.invoice.123")
            .unwrap();
        assert_eq!(matched.len(), 1);

        // Status change
        store
            .set_schema_status("_artifact.invoice.**", "v1", "paused")
            .unwrap();
        let updated = store
            .get_schema("_artifact.invoice.**", "v1")
            .unwrap()
            .unwrap();
        assert_eq!(updated.status, "paused");

        // Matching should be empty now because status is paused
        let matched_paused = store
            .get_schemas_for_topic("_artifact.invoice.123")
            .unwrap();
        assert!(matched_paused.is_empty());

        // Delete
        store.remove_schema("_artifact.invoice.**", "v1").unwrap();
        let deleted = store.get_schema("_artifact.invoice.**", "v1").unwrap();
        assert!(deleted.is_none());
    }

    #[test]
    fn test_parent_id_messages_tasks() {
        let store = Store::new_in_memory().unwrap();

        // Test message parent_id
        let msg = Message {
            id: "msg_2".to_string(),
            topic: "test.topic".to_string(),
            from: "human://yannick".to_string(),
            owner: Some("human://yannick".to_string()),
            timestamp: Utc::now(),
            content_type: "text/plain".to_string(),
            body: "Hello with parent".to_string(),
            headers: None,
            payload: None,
            parent_id: Some("msg_1".to_string()),
        };
        store.insert_message(&msg).unwrap();

        let retrieved_msg = store.get_message("msg_2").unwrap().unwrap();
        assert_eq!(retrieved_msg.parent_id, Some("msg_1".to_string()));

        // Test task parent_id
        let task = Task {
            id: "task_2".to_string(),
            title: "Test Task with parent".to_string(),
            description: Some("Description".to_string()),
            created_from: None,
            created_by: "human://yannick".to_string(),
            status: "open".to_string(),
            priority: "high".to_string(),
            topics: vec!["_project.test".to_string()],
            required_capabilities: vec!["code.write".to_string()],
            assigned_to: None,
            claimed_by: None,
            deadline: None,
            artifacts: vec![],
            decisions: vec![],
            parent_id: Some("task_1".to_string()),
        };
        store.insert_task(&task).unwrap();

        let retrieved_task = store.get_task("task_2").unwrap().unwrap();
        assert_eq!(retrieved_task.parent_id, Some("task_1".to_string()));
    }

    #[test]
    fn test_named_topic_store() {
        use mellowmesh_core::topic::NamedTopic;
        let store = Store::new_in_memory().unwrap();
        let topic = NamedTopic {
            name: "Mario Galaxy".to_string(),
            topic: "_forum.games.mario galaxy".to_string(),
        };

        store.register_named_topic(&topic).unwrap();

        let list = store.list_named_topics().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Mario Galaxy");
        assert_eq!(list[0].topic, "_forum.games.mario galaxy");

        // Test update
        let topic2 = NamedTopic {
            name: "Mario Galaxy".to_string(),
            topic: "_forum.games.mario galaxy.updated".to_string(),
        };
        store.register_named_topic(&topic2).unwrap();
        let list = store.list_named_topics().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].topic, "_forum.games.mario galaxy.updated");

        store.remove_named_topic("Mario Galaxy").unwrap();
        let list = store.list_named_topics().unwrap();
        assert!(list.is_empty());
    }
}
