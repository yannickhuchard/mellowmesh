use crate::Store;
use mellowmesh_core::auth::{Principal, TokenRecord};
use rusqlite::params;

impl Store {
    pub fn upsert_principal(&self, principal: &Principal) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO principals (id, kind, display_name, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET
                kind = excluded.kind,
                display_name = excluded.display_name",
            params![
                principal.id,
                principal.kind,
                principal.display_name,
                principal.created_at.to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_principal(&self, id: &str) -> anyhow::Result<Option<Principal>> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare("SELECT id, kind, display_name, created_at FROM principals WHERE id = ?1")?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let created_str: String = row.get(3)?;
            Ok(Some(Principal {
                id: row.get(0)?,
                kind: row.get(1)?,
                display_name: row.get(2)?,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_str)?
                    .with_timezone(&chrono::Utc),
            }))
        } else {
            Ok(None)
        }
    }

    pub fn insert_token(&self, token: &TokenRecord) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO tokens (id, principal, token_hash, read_scopes, write_scopes, created_at, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                token.id,
                token.principal,
                token.token_hash,
                serde_json::to_string(&token.read_scopes)?,
                serde_json::to_string(&token.write_scopes)?,
                token.created_at.to_rfc3339(),
                token.revoked as i64
            ],
        )?;
        Ok(())
    }

    /// Look up a non-revoked token by the SHA-256 hash of its plaintext.
    pub fn find_token_by_hash(&self, token_hash: &str) -> anyhow::Result<Option<TokenRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, principal, token_hash, read_scopes, write_scopes, created_at, revoked
             FROM tokens WHERE token_hash = ?1 AND revoked = 0",
        )?;
        let mut rows = stmt.query(params![token_hash])?;
        if let Some(row) = rows.next()? {
            Ok(Some(token_from_row(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn list_tokens(&self) -> anyhow::Result<Vec<TokenRecord>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, principal, token_hash, read_scopes, write_scopes, created_at, revoked
             FROM tokens ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| token_from_row(row).map_err(row_err))?;
        let mut tokens = Vec::new();
        for r in rows {
            tokens.push(r?);
        }
        Ok(tokens)
    }

    pub fn revoke_token(&self, id: &str) -> anyhow::Result<bool> {
        let conn = self.conn()?;
        let updated = conn.execute("UPDATE tokens SET revoked = 1 WHERE id = ?1", params![id])?;
        Ok(updated > 0)
    }

    /// Persist the end-to-end encryption key for a freshly minted token.
    /// Called at mint time, the only moment the plaintext token exists.
    pub fn register_e2e_key(&self, token: &str) -> anyhow::Result<()> {
        use mellowmesh_core::e2e::{derive_key, derive_key_id, hex_encode};
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO e2e_keys (key_id, e2e_key) VALUES (?1, ?2)
             ON CONFLICT(key_id) DO UPDATE SET e2e_key = excluded.e2e_key",
            params![derive_key_id(token), hex_encode(&derive_key(token))],
        )?;
        Ok(())
    }

    /// Look up an end-to-end key by its public key id (from an envelope).
    pub fn find_e2e_key(&self, key_id: &str) -> anyhow::Result<Option<[u8; 32]>> {
        use mellowmesh_core::e2e::hex_decode;
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT e2e_key FROM e2e_keys WHERE key_id = ?1")?;
        let mut rows = stmt.query(params![key_id])?;
        if let Some(row) = rows.next()? {
            let hex: String = row.get(0)?;
            if let Some(bytes) = hex_decode(&hex) {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    return Ok(Some(key));
                }
            }
        }
        Ok(None)
    }

    pub fn get_config(&self, key: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT value FROM app_config WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_config(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn token_from_row(row: &rusqlite::Row) -> anyhow::Result<TokenRecord> {
    let read_str: String = row.get(3)?;
    let write_str: String = row.get(4)?;
    let created_str: String = row.get(5)?;
    let revoked: i64 = row.get(6)?;
    Ok(TokenRecord {
        id: row.get(0)?,
        principal: row.get(1)?,
        token_hash: row.get(2)?,
        read_scopes: serde_json::from_str(&read_str).unwrap_or_default(),
        write_scopes: serde_json::from_str(&write_str).unwrap_or_default(),
        created_at: chrono::DateTime::parse_from_rfc3339(&created_str)?.with_timezone(&chrono::Utc),
        revoked: revoked != 0,
    })
}

fn row_err(e: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, e.into())
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use chrono::Utc;
    use mellowmesh_core::auth::{generate_token, hash_token, Principal, TokenRecord};

    #[test]
    fn test_principal_and_token_lifecycle() {
        let store = Store::new_in_memory().unwrap();

        let principal = Principal {
            id: "human://yannick".to_string(),
            kind: "human".to_string(),
            display_name: Some("Yannick".to_string()),
            created_at: Utc::now(),
        };
        store.upsert_principal(&principal).unwrap();
        let fetched = store.get_principal("human://yannick").unwrap().unwrap();
        assert_eq!(fetched.kind, "human");

        let plaintext = generate_token();
        let record = TokenRecord {
            id: "tok_1".to_string(),
            principal: "human://yannick".to_string(),
            token_hash: hash_token(&plaintext),
            read_scopes: vec!["**".to_string()],
            write_scopes: vec!["**".to_string()],
            created_at: Utc::now(),
            revoked: false,
        };
        store.insert_token(&record).unwrap();

        // Lookup by hash of the plaintext succeeds; wrong token fails
        let found = store
            .find_token_by_hash(&hash_token(&plaintext))
            .unwrap()
            .unwrap();
        assert_eq!(found.principal, "human://yannick");
        assert!(store
            .find_token_by_hash(&hash_token("mm_wrong"))
            .unwrap()
            .is_none());

        // Revocation kills the lookup
        assert!(store.revoke_token("tok_1").unwrap());
        assert!(store
            .find_token_by_hash(&hash_token(&plaintext))
            .unwrap()
            .is_none());
        // Listing still shows it, marked revoked
        let all = store.list_tokens().unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].revoked);
    }

    #[test]
    fn test_app_config() {
        let store = Store::new_in_memory().unwrap();
        assert!(store.get_config("owner_principal").unwrap().is_none());
        store
            .set_config("owner_principal", "human://yannick")
            .unwrap();
        assert_eq!(
            store.get_config("owner_principal").unwrap().unwrap(),
            "human://yannick"
        );
        store
            .set_config("owner_principal", "human://other")
            .unwrap();
        assert_eq!(
            store.get_config("owner_principal").unwrap().unwrap(),
            "human://other"
        );
    }
}
