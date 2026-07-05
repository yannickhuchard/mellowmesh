use crate::Store;
use chrono::{SecondsFormat, Utc};
use mellowmesh_core::task::{Task, DEFAULT_LEASE_SECONDS};
use rusqlite::params;

/// Outcome of a claim attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimOutcome {
    /// The claim (or re-claim by the same agent) succeeded.
    Claimed { lease_expires_at: String },
    /// The task is already claimed by another agent whose lease has not expired.
    Conflict { held_by: String },
    /// No task exists with the given id.
    NotFound,
    /// The task is not claimable (e.g. already completed or cancelled).
    NotClaimable { status: String },
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

const TASK_COLUMNS: &str = "id, title, description, created_from, created_by, status, priority, topics, required_capabilities, assigned_to, claimed_by, deadline, artifacts, decisions, parent_id, lease_seconds, claim_expires_at";

fn task_from_row(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    let topics_str: String = row.get(7)?;
    let caps_str: String = row.get(8)?;
    let artifacts_str: String = row.get(12)?;
    let decisions_str: String = row.get(13)?;
    let lease_seconds: Option<i64> = row.get(15)?;

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
        lease_seconds: lease_seconds.map(|v| v as u64),
        claim_expires_at: row.get(16)?,
    })
}

impl Store {
    pub fn insert_task(&self, task: &Task) -> anyhow::Result<()> {
        let conn = self.conn()?;
        let topics_json = serde_json::to_string(&task.topics)?;
        let caps_json = serde_json::to_string(&task.required_capabilities)?;
        let artifacts_json = serde_json::to_string(&task.artifacts)?;
        let decisions_json = serde_json::to_string(&task.decisions)?;

        conn.execute(
            "INSERT INTO tasks (id, title, description, created_from, created_by, status, priority, topics, required_capabilities, assigned_to, claimed_by, deadline, artifacts, decisions, parent_id, lease_seconds, claim_expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
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
                task.parent_id,
                task.lease_seconds.map(|v| v as i64),
                task.claim_expires_at
            ],
        )?;
        Ok(())
    }

    pub fn get_task(&self, id: &str) -> anyhow::Result<Option<Task>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"))?;
        let mut rows = stmt.query(params![id])?;

        if let Some(row) = rows.next()? {
            Ok(Some(task_from_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_tasks(&self) -> anyhow::Result<Vec<Task>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks"))?;
        let rows = stmt.query_map([], task_from_row)?;

        let mut tasks = Vec::new();
        for r in rows {
            tasks.push(r?);
        }
        Ok(tasks)
    }

    /// Attempt to claim a task atomically.
    ///
    /// A claim succeeds when the task is `open`, when its previous claim lease
    /// has expired, or when the same agent re-claims (which renews the lease).
    /// A task claimed by another agent with an unexpired lease is a conflict.
    pub fn claim_task(
        &self,
        id: &str,
        claimed_by: &str,
        lease_seconds: Option<u64>,
    ) -> anyhow::Result<ClaimOutcome> {
        let conn = self.conn()?;
        let lease = lease_seconds.unwrap_or(DEFAULT_LEASE_SECONDS);
        let now = now_rfc3339();
        let expires_at = (Utc::now() + chrono::Duration::seconds(lease as i64))
            .to_rfc3339_opts(SecondsFormat::Secs, true);

        let updated = conn.execute(
            "UPDATE tasks SET claimed_by = ?2, status = 'claimed', lease_seconds = ?3, claim_expires_at = ?4
             WHERE id = ?1 AND (
                 status = 'open'
                 OR claimed_by = ?2
                 OR (status = 'claimed' AND claim_expires_at IS NOT NULL AND claim_expires_at < ?5)
             ) AND status NOT IN ('completed', 'cancelled', 'failed')",
            params![id, claimed_by, lease as i64, expires_at, now],
        )?;

        if updated > 0 {
            return Ok(ClaimOutcome::Claimed {
                lease_expires_at: expires_at,
            });
        }

        drop(conn);
        match self.get_task(id)? {
            None => Ok(ClaimOutcome::NotFound),
            Some(task) => {
                if task.status == "claimed" || task.status == "in_progress" {
                    Ok(ClaimOutcome::Conflict {
                        held_by: task.claimed_by.unwrap_or_default(),
                    })
                } else {
                    Ok(ClaimOutcome::NotClaimable {
                        status: task.status,
                    })
                }
            }
        }
    }

    /// Extend the claim lease of a task held by `agent_id`. Returns `true` if
    /// a lease was renewed. Used as the heartbeat path: publishing progress
    /// on `_task.<id>.progress` renews the publisher's lease.
    pub fn renew_claim(&self, id: &str, agent_id: &str) -> anyhow::Result<bool> {
        let conn = self.conn()?;
        let updated = conn.execute(
            "UPDATE tasks
             SET claim_expires_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+' || COALESCE(lease_seconds, ?3) || ' seconds')
             WHERE id = ?1 AND claimed_by = ?2 AND status IN ('claimed', 'in_progress')",
            params![id, agent_id, DEFAULT_LEASE_SECONDS as i64],
        )?;
        Ok(updated > 0)
    }

    /// Release every claimed task whose lease expired before `now`. Each
    /// released task returns to `open` with its claim cleared. Returns the
    /// released tasks so the caller can announce them on the fabric.
    pub fn release_expired_claims(&self) -> anyhow::Result<Vec<Task>> {
        let now = now_rfc3339();
        let conn = self.conn()?;

        let expired: Vec<Task> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks
                 WHERE status = 'claimed' AND claim_expires_at IS NOT NULL AND claim_expires_at < ?1"
            ))?;
            let rows = stmt.query_map(params![now], task_from_row)?;
            let mut tasks = Vec::new();
            for r in rows {
                tasks.push(r?);
            }
            tasks
        };

        for task in &expired {
            conn.execute(
                "UPDATE tasks SET status = 'open', claimed_by = NULL, claim_expires_at = NULL
                 WHERE id = ?1 AND status = 'claimed' AND claim_expires_at IS NOT NULL AND claim_expires_at < ?2",
                params![task.id, now],
            )?;
        }
        Ok(expired)
    }

    pub fn complete_task(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE tasks SET status = 'completed', claim_expires_at = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }
}
