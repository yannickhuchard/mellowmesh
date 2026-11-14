//! Network setting, spaces and teams (see `docs/network.md`).

use crate::Store;
use chrono::{DateTime, Utc};
use mellowmesh_core::network::{AccessLevel, Network, Space, Team, TeamMember, TeamRole};
use rusqlite::{params, OptionalExtension};

/// Outcome of a create that may collide with an existing id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    Created,
    /// The id (or, for a team, its team space id) is already taken.
    Conflict(String),
}

pub(crate) fn migrate(conn: &rusqlite::Connection) -> anyhow::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS network_settings (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            default_access TEXT NOT NULL DEFAULT 'closed',
            visitor_write INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS spaces (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            patterns TEXT NOT NULL,
            access TEXT NOT NULL,
            visitor_write INTEGER NOT NULL DEFAULT 0,
            share_with_peers INTEGER NOT NULL DEFAULT 0,
            members TEXT NOT NULL DEFAULT '[]',
            teams TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS teams (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            color TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS team_members (
            team_id TEXT NOT NULL,
            principal TEXT NOT NULL,
            role TEXT NOT NULL,
            PRIMARY KEY (team_id, principal)
        );",
    )?;
    Ok(())
}

fn access_from(s: &str) -> AccessLevel {
    s.parse().unwrap_or(AccessLevel::Closed)
}

fn ts(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_default()
}

fn space_from_row(row: &rusqlite::Row) -> rusqlite::Result<Space> {
    let patterns: String = row.get(3)?;
    let members: String = row.get(7)?;
    let teams: String = row.get(8)?;
    Ok(Space {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        patterns: serde_json::from_str(&patterns).unwrap_or_default(),
        access: access_from(&row.get::<_, String>(4)?),
        visitor_write: row.get::<_, i64>(5)? != 0,
        share_with_peers: row.get::<_, i64>(6)? != 0,
        members: serde_json::from_str(&members).unwrap_or_default(),
        teams: serde_json::from_str(&teams).unwrap_or_default(),
        created_at: ts(row.get(9)?),
        updated_at: ts(row.get(10)?),
    })
}

const SPACE_COLUMNS: &str = "id, name, description, patterns, access, visitor_write, share_with_peers, members, teams, created_at, updated_at";

fn write_space(conn: &rusqlite::Connection, space: &Space, upsert: bool) -> anyhow::Result<()> {
    let verb = if upsert {
        "INSERT OR REPLACE"
    } else {
        "INSERT"
    };
    conn.execute(
        &format!(
            "{verb} INTO spaces ({SPACE_COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        ),
        params![
            space.id,
            space.name,
            space.description,
            serde_json::to_string(&space.patterns)?,
            space.access.as_str(),
            space.visitor_write as i64,
            space.share_with_peers as i64,
            serde_json::to_string(&space.members)?,
            serde_json::to_string(&space.teams)?,
            space.created_at.to_rfc3339(),
            space.updated_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn exists(conn: &rusqlite::Connection, table: &str, id: &str) -> anyhow::Result<bool> {
    Ok(conn
        .prepare(&format!("SELECT 1 FROM {table} WHERE id = ?1"))?
        .exists(params![id])?)
}

fn load_members(conn: &rusqlite::Connection, team_id: &str) -> anyhow::Result<Vec<TeamMember>> {
    let mut stmt =
        conn.prepare("SELECT principal, role FROM team_members WHERE team_id = ?1 ORDER BY rowid")?;
    let rows = stmt.query_map(params![team_id], |row| {
        let role: String = row.get(1)?;
        Ok(TeamMember {
            principal: row.get(0)?,
            role: role.parse().unwrap_or(TeamRole::Member),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn write_members(
    conn: &rusqlite::Connection,
    team_id: &str,
    members: &[TeamMember],
) -> anyhow::Result<()> {
    conn.execute(
        "DELETE FROM team_members WHERE team_id = ?1",
        params![team_id],
    )?;
    for m in members {
        conn.execute(
            "INSERT OR REPLACE INTO team_members (team_id, principal, role) VALUES (?1, ?2, ?3)",
            params![team_id, m.principal, m.role.as_str()],
        )?;
    }
    Ok(())
}

impl Store {
    /// The network setting (defaults when never set).
    pub fn get_network(&self) -> anyhow::Result<Network> {
        let conn = self.conn()?;
        let found = conn
            .query_row(
                "SELECT name, description, default_access, visitor_write FROM network_settings WHERE id = 1",
                [],
                |row| {
                    Ok(Network {
                        name: row.get(0)?,
                        description: row.get(1)?,
                        default_access: access_from(&row.get::<_, String>(2)?),
                        visitor_write: row.get::<_, i64>(3)? != 0,
                    })
                },
            )
            .optional()?;
        Ok(found.unwrap_or_default())
    }

    pub fn set_network(&self, network: &Network) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO network_settings (id, name, description, default_access, visitor_write, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, description = excluded.description,
                default_access = excluded.default_access, visitor_write = excluded.visitor_write,
                updated_at = excluded.updated_at",
            params![
                network.name,
                network.description,
                network.default_access.as_str(),
                network.visitor_write as i64,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn list_spaces(&self) -> anyhow::Result<Vec<Space>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(&format!("SELECT {SPACE_COLUMNS} FROM spaces ORDER BY id"))?;
        let rows = stmt.query_map([], space_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_space(&self, id: &str) -> anyhow::Result<Option<Space>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                &format!("SELECT {SPACE_COLUMNS} FROM spaces WHERE id = ?1"),
                params![id],
                space_from_row,
            )
            .optional()?)
    }

    /// Create a space; `Conflict` when the id is taken.
    pub fn create_space(&self, space: &Space) -> anyhow::Result<CreateOutcome> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        if exists(&tx, "spaces", &space.id)? {
            return Ok(CreateOutcome::Conflict(space.id.clone()));
        }
        write_space(&tx, space, false)?;
        tx.commit()?;
        Ok(CreateOutcome::Created)
    }

    /// Replace a space, keeping its `created_at`. Returns `false` when it
    /// does not exist.
    pub fn replace_space(&self, space: &Space) -> anyhow::Result<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let created: Option<String> = tx
            .query_row(
                "SELECT created_at FROM spaces WHERE id = ?1",
                params![space.id],
                |r| r.get(0),
            )
            .optional()?;
        let Some(created) = created else {
            return Ok(false);
        };
        let mut space = space.clone();
        space.created_at = ts(created);
        write_space(&tx, &space, true)?;
        tx.commit()?;
        Ok(true)
    }

    pub fn delete_space(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn()?;
        Ok(conn.execute("DELETE FROM spaces WHERE id = ?1", params![id])? > 0)
    }

    pub fn list_teams(&self) -> anyhow::Result<Vec<Team>> {
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT id, name, description, color, created_at FROM teams ORDER BY id")?;
        let rows = stmt.query_map([], |row| {
            Ok(Team {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                color: row.get(3)?,
                members: Vec::new(),
                created_at: ts(row.get(4)?),
            })
        })?;
        let mut teams = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for t in &mut teams {
            t.members = load_members(&conn, &t.id)?;
        }
        Ok(teams)
    }

    pub fn get_team(&self, id: &str) -> anyhow::Result<Option<Team>> {
        Ok(self.list_teams()?.into_iter().find(|t| t.id == id))
    }

    /// Create a team together with its closed team space, atomically.
    pub fn create_team(&self, team: &Team, space: &Space) -> anyhow::Result<CreateOutcome> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        if exists(&tx, "teams", &team.id)? {
            return Ok(CreateOutcome::Conflict(team.id.clone()));
        }
        if exists(&tx, "spaces", &space.id)? {
            return Ok(CreateOutcome::Conflict(space.id.clone()));
        }
        tx.execute(
            "INSERT INTO teams (id, name, description, color, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                team.id,
                team.name,
                team.description,
                team.color,
                team.created_at.to_rfc3339()
            ],
        )?;
        write_members(&tx, &team.id, &team.members)?;
        write_space(&tx, space, false)?;
        tx.commit()?;
        Ok(CreateOutcome::Created)
    }

    /// Replace a team (name, description, color, members), keeping its
    /// `created_at`. Returns `false` when it does not exist.
    pub fn replace_team(&self, team: &Team) -> anyhow::Result<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let n = tx.execute(
            "UPDATE teams SET name = ?2, description = ?3, color = ?4 WHERE id = ?1",
            params![team.id, team.name, team.description, team.color],
        )?;
        if n == 0 {
            return Ok(false);
        }
        write_members(&tx, &team.id, &team.members)?;
        tx.commit()?;
        Ok(true)
    }

    /// Delete a team, its members, its team space, and its id from every
    /// other space's team list.
    pub fn delete_team(&self, id: &str, team_space_id: &str) -> anyhow::Result<bool> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        let n = tx.execute("DELETE FROM teams WHERE id = ?1", params![id])?;
        if n == 0 {
            return Ok(false);
        }
        tx.execute("DELETE FROM team_members WHERE team_id = ?1", params![id])?;
        tx.execute("DELETE FROM spaces WHERE id = ?1", params![team_space_id])?;
        let referencing: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT id, teams FROM spaces")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (space_id, teams) in referencing {
            let list: Vec<String> = serde_json::from_str(&teams).unwrap_or_default();
            if list.iter().any(|t| t == id) {
                let kept: Vec<&String> = list.iter().filter(|t| *t != id).collect();
                tx.execute(
                    "UPDATE spaces SET teams = ?2, updated_at = ?3 WHERE id = ?1",
                    params![
                        space_id,
                        serde_json::to_string(&kept)?,
                        Utc::now().to_rfc3339()
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(true)
    }

    /// Add or update a team member. Returns `false` when the team does not
    /// exist.
    pub fn upsert_team_member(&self, team_id: &str, member: &TeamMember) -> anyhow::Result<bool> {
        let conn = self.conn()?;
        if !exists(&conn, "teams", team_id)? {
            return Ok(false);
        }
        conn.execute(
            "INSERT INTO team_members (team_id, principal, role) VALUES (?1, ?2, ?3)
             ON CONFLICT(team_id, principal) DO UPDATE SET role = excluded.role",
            params![team_id, member.principal, member.role.as_str()],
        )?;
        Ok(true)
    }

    /// Remove a team member. Returns `false` when there was no such member.
    pub fn remove_team_member(&self, team_id: &str, principal: &str) -> anyhow::Result<bool> {
        let conn = self.conn()?;
        Ok(conn.execute(
            "DELETE FROM team_members WHERE team_id = ?1 AND principal = ?2",
            params![team_id, principal],
        )? > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: &str) -> Space {
        let now = Utc::now();
        Space {
            id: id.into(),
            name: "Family".into(),
            description: "d".into(),
            patterns: vec!["_chat.group.family.**".into()],
            access: AccessLevel::Closed,
            visitor_write: true,
            share_with_peers: true,
            members: vec!["human://alice".into()],
            teams: vec!["home".into()],
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn network_round_trip() {
        let store = Store::new_in_memory().unwrap();
        assert_eq!(store.get_network().unwrap(), Network::default());
        let n = Network {
            name: "Yannick's mesh".into(),
            description: "home".into(),
            default_access: AccessLevel::Open,
            visitor_write: true,
        };
        store.set_network(&n).unwrap();
        assert_eq!(store.get_network().unwrap(), n);
        let n2 = Network {
            visitor_write: false,
            ..n
        };
        store.set_network(&n2).unwrap();
        assert_eq!(store.get_network().unwrap(), n2);
    }

    #[test]
    fn space_round_trip() {
        let store = Store::new_in_memory().unwrap();
        let s = space("family");
        assert_eq!(store.create_space(&s).unwrap(), CreateOutcome::Created);
        assert_eq!(
            store.create_space(&s).unwrap(),
            CreateOutcome::Conflict("family".into())
        );
        let got = store.get_space("family").unwrap().unwrap();
        assert_eq!(got.patterns, s.patterns);
        assert_eq!(got.members, s.members);
        assert_eq!(got.teams, s.teams);
        assert!(got.visitor_write && got.share_with_peers);
        assert_eq!(got.access, AccessLevel::Closed);
        assert_eq!(got.created_at.timestamp(), s.created_at.timestamp());

        let mut changed = s.clone();
        changed.access = AccessLevel::Open;
        changed.created_at = DateTime::<Utc>::default();
        assert!(store.replace_space(&changed).unwrap());
        let got = store.get_space("family").unwrap().unwrap();
        assert_eq!(got.access, AccessLevel::Open);
        assert_eq!(
            got.created_at.timestamp(),
            s.created_at.timestamp(),
            "created_at is kept"
        );
        assert!(!store.replace_space(&space("nope")).unwrap());
        assert_eq!(store.list_spaces().unwrap().len(), 1);
        assert!(store.delete_space("family").unwrap());
        assert!(!store.delete_space("family").unwrap());
        assert!(store.list_spaces().unwrap().is_empty());
    }

    #[test]
    fn team_round_trip() {
        let store = Store::new_in_memory().unwrap();
        let mut team = Team {
            id: "home".into(),
            name: "Home".into(),
            members: vec![TeamMember {
                principal: "human://alice".into(),
                role: TeamRole::Admin,
            }],
            created_at: Utc::now(),
            ..Default::default()
        };
        let team_space = team.default_space(Utc::now());
        assert_eq!(
            store.create_team(&team, &team_space).unwrap(),
            CreateOutcome::Created
        );
        assert_eq!(
            store.create_team(&team, &team_space).unwrap(),
            CreateOutcome::Conflict("home".into())
        );
        assert!(store.get_space("team-home").is_ok_and(|s| s.is_some()));
        // Another space referencing the team.
        store.create_space(&space("family")).unwrap();

        let got = store.get_team("home").unwrap().unwrap();
        assert_eq!(got.members, team.members);
        assert_eq!(got.color, "#10B981");

        assert!(store
            .upsert_team_member(
                "home",
                &TeamMember {
                    principal: "agent://o/planner".into(),
                    role: TeamRole::Member
                }
            )
            .unwrap());
        assert!(!store
            .upsert_team_member(
                "nope",
                &TeamMember {
                    principal: "agent://o/planner".into(),
                    role: TeamRole::Member
                }
            )
            .unwrap());
        assert_eq!(store.get_team("home").unwrap().unwrap().members.len(), 2);
        assert!(store.remove_team_member("home", "human://alice").unwrap());
        assert!(!store.remove_team_member("home", "human://alice").unwrap());

        team.name = "House".into();
        team.members = vec![];
        assert!(store.replace_team(&team).unwrap());
        let got = store.get_team("home").unwrap().unwrap();
        assert_eq!(got.name, "House");
        assert!(got.members.is_empty());

        assert!(store.delete_team("home", "team-home").unwrap());
        assert!(store.get_team("home").unwrap().is_none());
        assert!(store.get_space("team-home").unwrap().is_none());
        assert!(store.get_space("family").unwrap().unwrap().teams.is_empty());
        assert!(!store.delete_team("home", "team-home").unwrap());
    }
}
