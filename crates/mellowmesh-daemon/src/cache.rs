//! Small in-process caches for data read on every request or publish.
//!
//! The agent registry, named topics and compiled JSON schemas are consulted
//! on the publish hot path; bearer tokens are resolved on every request.
//! Each cache is invalidated explicitly by the handlers that write the
//! underlying table, and the token cache additionally expires entries after
//! [`TOKEN_CACHE_TTL`] so out-of-band changes are picked up quickly.

use crate::auth::AuthPrincipal;
use mellowmesh_core::agent::AgentRegistration;
use mellowmesh_core::persistence::TopicSchema;
use mellowmesh_core::topic::NamedTopic;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// How long a resolved token stays cached before it is re-read from SQLite.
pub const TOKEN_CACHE_TTL: Duration = Duration::from_secs(30);
/// Upper bound on cached tokens; the cache is cleared when it is exceeded.
const TOKEN_CACHE_MAX: usize = 1024;
/// A token's last use is recorded at most this often.
pub const TOKEN_TOUCH_EVERY: Duration = Duration::from_secs(60);
/// Upper bound on remembered token touches; cleared when exceeded.
const TOKEN_TOUCH_MAX: usize = 4096;
/// How long an MCP dispatch client is reused.
const MCP_CLIENT_TTL: Duration = Duration::from_secs(300);
const MCP_CLIENT_MAX: usize = 64;

/// A lazily loaded, explicitly invalidated snapshot of a table.
pub struct Cached<T> {
    value: RwLock<Option<Arc<T>>>,
    generation: AtomicU64,
}

impl<T> Default for Cached<T> {
    fn default() -> Self {
        Self {
            value: RwLock::new(None),
            generation: AtomicU64::new(0),
        }
    }
}

impl<T> Cached<T> {
    /// Return the cached value, loading it with `load` on a miss. A value
    /// loaded concurrently with an invalidation is returned but not stored.
    pub fn get_or_load(&self, load: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<Arc<T>> {
        if let Some(v) = self
            .value
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            return Ok(v.clone());
        }
        let generation = self.generation.load(Ordering::Acquire);
        let loaded = Arc::new(load()?);
        let mut slot = self.value.write().unwrap_or_else(|p| p.into_inner());
        if self.generation.load(Ordering::Acquire) == generation {
            *slot = Some(loaded.clone());
        }
        Ok(loaded)
    }

    pub fn invalidate(&self) {
        let mut slot = self.value.write().unwrap_or_else(|p| p.into_inner());
        self.generation.fetch_add(1, Ordering::AcqRel);
        *slot = None;
    }
}

/// A stored schema together with its compiled validator (or the compile
/// error, so a schema that stopped compiling fails publishes loudly).
pub struct CompiledSchema {
    pub schema: TopicSchema,
    pub validator: Result<Arc<jsonschema::JSONSchema>, String>,
}

/// Compile a schema document. Remote and file `$ref` resolution is disabled
/// at build level, so this never performs I/O.
pub fn compile_schema(content: &str) -> Result<jsonschema::JSONSchema, String> {
    let json: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("Invalid schema JSON: {e}"))?;
    jsonschema::JSONSchema::compile(&json).map_err(|e| format!("Invalid JSON Schema: {e}"))
}

#[derive(Default)]
pub struct Caches {
    pub agents: Cached<Vec<AgentRegistration>>,
    pub named_topics: Cached<Vec<NamedTopic>>,
    pub schemas: Cached<Vec<CompiledSchema>>,
    tokens: Mutex<HashMap<String, (AuthPrincipal, Instant)>>,
    /// When each token's use was last recorded (by token id).
    token_touches: Mutex<HashMap<String, Instant>>,
    mcp_clients: Mutex<HashMap<String, (mellowmesh_client::MellowMeshClient, Instant)>>,
}

impl Caches {
    /// All compiled schemas, loaded from the store on a miss.
    pub fn load_schemas(
        &self,
        store: &mellowmesh_store::Store,
    ) -> anyhow::Result<Arc<Vec<CompiledSchema>>> {
        self.schemas.get_or_load(|| {
            Ok(store
                .list_schemas()?
                .into_iter()
                .map(|schema| {
                    let validator = compile_schema(&schema.schema_content).map(Arc::new);
                    CompiledSchema { schema, validator }
                })
                .collect())
        })
    }

    pub fn get_token(&self, token_hash: &str) -> Option<AuthPrincipal> {
        let mut map = self.tokens.lock().unwrap_or_else(|p| p.into_inner());
        match map.get(token_hash) {
            Some((p, at)) if at.elapsed() < TOKEN_CACHE_TTL => Some(p.clone()),
            Some(_) => {
                map.remove(token_hash);
                None
            }
            None => None,
        }
    }

    pub fn put_token(&self, token_hash: String, principal: AuthPrincipal) {
        let mut map = self.tokens.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= TOKEN_CACHE_MAX {
            map.clear();
        }
        map.insert(token_hash, (principal, Instant::now()));
    }

    /// Whether the use of `token_id` should be recorded now: on its first
    /// use since the daemon started, then at most every
    /// [`TOKEN_TOUCH_EVERY`]. Returns `true` at most once per interval.
    pub fn token_touch_due(&self, token_id: &str) -> bool {
        let mut map = self.token_touches.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(at) = map.get(token_id) {
            if at.elapsed() < TOKEN_TOUCH_EVERY {
                return false;
            }
        }
        if map.len() >= TOKEN_TOUCH_MAX {
            map.clear();
        }
        map.insert(token_id.to_string(), Instant::now());
        true
    }

    /// Drop every cached token and dispatch client (called on revocation).
    pub fn clear_tokens(&self) {
        self.tokens
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.mcp_clients
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// A loopback client for MCP tool dispatch, reused across requests from
    /// the same caller so its connection pool is shared.
    pub fn mcp_client(
        &self,
        port: u16,
        token: Option<&str>,
    ) -> mellowmesh_client::MellowMeshClient {
        let key = token
            .map(mellowmesh_core::auth::hash_token)
            .unwrap_or_default();
        let mut map = self.mcp_clients.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((client, at)) = map.get(&key) {
            if at.elapsed() < MCP_CLIENT_TTL {
                return client.clone();
            }
        }
        let mut client = mellowmesh_client::MellowMeshClient::loopback(port);
        if let Some(t) = token {
            client = client.with_token(t);
        }
        if map.len() >= MCP_CLIENT_MAX {
            map.retain(|_, (_, at)| at.elapsed() < MCP_CLIENT_TTL);
            if map.len() >= MCP_CLIENT_MAX {
                map.clear();
            }
        }
        map.insert(key, (client.clone(), Instant::now()));
        client
    }
}
