//! Bearer-token authentication middleware and token administration endpoints.
//!
//! Every request passes through [`auth_middleware`], which resolves an
//! optional `Authorization: Bearer <token>` header (or `?token=` query
//! parameter, for WebSocket clients that cannot set headers) into an
//! [`AuthContext`] request extension. When the daemon runs with
//! `--require-auth`, requests without a valid token are rejected with 401
//! except for a small set of open endpoints (health, dashboard).

use crate::state::AppState;
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::Utc;
use mellowmesh_core::auth::{
    generate_token, hash_token, kind_of_uri, scopes_allow, Principal, TokenRecord,
};
use mellowmesh_core::e2e::REPLAY_WINDOW_SECS;
use serde::Deserialize;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Mutex, OnceLock};
use ulid::Ulid;

/// How far in the future a sealed request's `ts` may be (clock skew).
pub(crate) const MAX_FUTURE_SKEW_SECS: i64 = 30;

/// True when a sealed request timestamp is inside the acceptance window:
/// not older than [`REPLAY_WINDOW_SECS`] and not more than
/// [`MAX_FUTURE_SKEW_SECS`] in the future.
pub(crate) fn ts_acceptable(ts: i64, now: i64) -> bool {
    now.saturating_sub(ts) <= REPLAY_WINDOW_SECS && ts.saturating_sub(now) <= MAX_FUTURE_SKEW_SECS
}

/// Recently-seen sealed-request ids with their expiry, plus a min-heap of
/// expiries so pruning only touches entries that actually expired.
#[derive(Default)]
struct ReplayCache {
    expiry: HashMap<String, i64>,
    order: BinaryHeap<Reverse<(i64, String)>>,
}

/// Process-wide cache of recently-seen sealed-request ids, so a captured E2E
/// envelope (request or subscription proof) cannot be replayed within the
/// window. Ids are random 128-bit values; a global cache is sufficient.
fn replay_cache() -> &'static Mutex<ReplayCache> {
    static CACHE: OnceLock<Mutex<ReplayCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ReplayCache::default()))
}

/// Record a single-use id carried by a request stamped `ts`. Returns `true`
/// if it is fresh, `false` if it was already seen (a replay) or `ts` is
/// outside the acceptance window. An id is remembered until
/// `max(now, ts) + REPLAY_WINDOW_SECS`, i.e. for as long as a request
/// carrying it could still be accepted.
pub(crate) fn register_jti(jti: &str, ts: i64, now: i64) -> bool {
    if !ts_acceptable(ts, now) {
        return false;
    }
    let mut cache = match replay_cache().lock() {
        Ok(c) => c,
        Err(poisoned) => poisoned.into_inner(),
    };
    while let Some(Reverse((exp, _))) = cache.order.peek() {
        if *exp >= now {
            break;
        }
        if let Some(Reverse((exp, id))) = cache.order.pop() {
            if cache.expiry.get(&id) == Some(&exp) {
                cache.expiry.remove(&id);
            }
        }
    }
    if cache.expiry.contains_key(jti) {
        return false;
    }
    let exp = now.max(ts) + REPLAY_WINDOW_SECS;
    cache.expiry.insert(jti.to_string(), exp);
    cache.order.push(Reverse((exp, jti.to_string())));
    true
}

/// The authenticated principal attached to a request, if any.
#[derive(Debug, Clone)]
pub struct AuthPrincipal {
    pub id: String,
    pub kind: String,
    pub read_scopes: Vec<String>,
    pub write_scopes: Vec<String>,
    /// SHA-256 of the bearer token the request authenticated with, so a
    /// long-lived subscription can re-validate it.
    pub token_hash: String,
}

#[derive(Debug, Clone, Default)]
pub struct AuthContext {
    pub principal: Option<AuthPrincipal>,
}

impl AuthContext {
    pub fn can_write(&self, topic: &str) -> bool {
        match &self.principal {
            Some(p) => scopes_allow(&p.write_scopes, topic),
            // Unauthenticated requests only reach handlers in open mode,
            // where localhost is trusted.
            None => true,
        }
    }

    pub fn can_read(&self, topic: &str) -> bool {
        match &self.principal {
            Some(p) => scopes_allow(&p.read_scopes, topic),
            None => true,
        }
    }
}

/// Endpoints reachable without a bearer token even under `--require-auth`.
/// `/e2e/request` authenticates via the sealed envelope (key possession
/// proven by decryption + the inner Authorization header), not the outer
/// header, so the middleware lets it through to its own handler.
fn is_open_path(path: &str) -> bool {
    matches!(
        path,
        "/health" | "/" | "/ui" | "/favicon.ico" | "/e2e/request"
    )
}

fn extract_bearer(req: &Request<Body>) -> Option<String> {
    if let Some(header) = req.headers().get(axum::http::header::AUTHORIZATION) {
        if let Ok(value) = header.to_str() {
            if let Some(token) = value.strip_prefix("Bearer ") {
                return Some(token.trim().to_string());
            }
        }
    }
    // WebSocket clients (browsers) cannot set headers on the upgrade request.
    if let Some(query) = req.uri().query() {
        for pair in query.split('&') {
            if let Some(value) = pair.strip_prefix("token=") {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// End-to-end encryption context for a live subscription: when a WebSocket
/// client authenticated with a sealed proof, this carries the key (and its
/// id, bound as AEAD associated data) under which every delivered message
/// must be sealed. Inserted for every request; `None` on plain connections.
#[derive(Clone)]
pub struct StreamE2e(pub Option<([u8; 32], String)>);

/// Sealed subscription proof carried as query parameters (WebSocket clients
/// cannot send bodies with the upgrade request).
fn extract_e2e_proof(req: &Request<Body>) -> Option<mellowmesh_core::e2e::Envelope> {
    let query = req.uri().query()?;
    let mut key_id = None;
    let mut nonce = None;
    let mut ciphertext = None;
    for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
        match k.as_ref() {
            "e2e_kid" => key_id = Some(v.into_owned()),
            "e2e_nonce" => nonce = Some(v.into_owned()),
            "e2e_ct" => ciphertext = Some(v.into_owned()),
            _ => {}
        }
    }
    Some(mellowmesh_core::e2e::Envelope {
        v: 2,
        key_id: key_id?,
        nonce: nonce?,
        ciphertext: ciphertext?,
    })
}

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let mut ctx = AuthContext::default();

    // Sealed subscription proof: authenticates the subscriber without the
    // raw token ever crossing the relay, and switches the connection into
    // sealed-delivery mode.
    let mut bearer = extract_bearer(&req);
    let mut stream_e2e = StreamE2e(None);
    // A sealed subscription proof authenticates a WebSocket subscriber without
    // the raw token crossing the relay. It is accepted ONLY on the `/ws`
    // route, and is bound to that method+path and to a single-use id, so a
    // relay operator cannot lift the proof out of the query string and replay
    // it against another endpoint (e.g. /auth/tokens) to escalate.
    if bearer.is_none() && req.uri().path() == "/ws" {
        if let Some(envelope) = extract_e2e_proof(&req) {
            use mellowmesh_core::e2e::{open, SealedRequest, CTX_PROOF};
            let key_store = state.store.clone();
            let key_id = envelope.key_id.clone();
            let key_lookup =
                tokio::task::spawn_blocking(move || key_store.find_e2e_key(&key_id)).await;
            let key = match key_lookup.unwrap_or_else(|e| Err(anyhow::anyhow!(e))) {
                Ok(Some(k)) => k,
                Ok(None) => {
                    return (StatusCode::UNAUTHORIZED, "Unknown e2e key id").into_response()
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Key lookup failed: {e}"),
                    )
                        .into_response()
                }
            };
            let now = Utc::now().timestamp();
            let proof = open(&key, CTX_PROOF, &envelope)
                .ok()
                .and_then(|pt| serde_json::from_slice::<SealedRequest>(&pt).ok());
            let valid = match &proof {
                Some(p) => {
                    p.method == "SUBSCRIBE"
                        && p.path_and_query.starts_with("/ws")
                        && p.jti
                            .as_deref()
                            .map(|j| register_jti(j, p.ts, now))
                            .unwrap_or(false)
                }
                None => false,
            };
            if valid {
                let p = proof.unwrap();
                bearer = p
                    .authorization
                    .as_deref()
                    .and_then(|a| a.strip_prefix("Bearer "))
                    .map(|t| t.trim().to_string());
                stream_e2e = StreamE2e(Some((key, envelope.key_id.clone())));
            } else {
                return (StatusCode::UNAUTHORIZED, "Invalid e2e subscription proof")
                    .into_response();
            }
        }
    }

    if let Some(token) = bearer {
        match resolve_token(&state, &token).await {
            Ok(Some(principal)) => ctx.principal = Some(principal),
            Ok(None) => {
                return (StatusCode::UNAUTHORIZED, "Invalid or revoked token").into_response();
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Token lookup failed: {e}"),
                )
                    .into_response();
            }
        }
    }

    if state.require_auth && ctx.principal.is_none() && !is_open_path(req.uri().path()) {
        return (
            StatusCode::UNAUTHORIZED,
            "Authentication required: pass a bearer token (Authorization header or ?token=)",
        )
            .into_response();
    }

    req.extensions_mut().insert(ctx);
    req.extensions_mut().insert(stream_e2e);
    next.run(req).await
}

/// Resolve a bearer token to its principal, via the short-lived token cache
/// or (on a miss) SQLite on the blocking pool. `Ok(None)` means unknown or
/// revoked.
async fn resolve_token(state: &AppState, token: &str) -> anyhow::Result<Option<AuthPrincipal>> {
    let token_hash = hash_token(token);
    if let Some(p) = state.caches.get_token(&token_hash) {
        return Ok(Some(p));
    }
    let store = state.store.clone();
    let hash = token_hash.clone();
    let resolved = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<AuthPrincipal>> {
        let Some(record) = store.find_token_by_hash(&hash)? else {
            return Ok(None);
        };
        let kind = store
            .get_principal(&record.principal)
            .ok()
            .flatten()
            .map(|p| p.kind)
            .unwrap_or_else(|| kind_of_uri(&record.principal).to_string());
        Ok(Some(AuthPrincipal {
            id: record.principal,
            kind,
            read_scopes: record.read_scopes,
            write_scopes: record.write_scopes,
            token_hash: hash,
        }))
    })
    .await??;
    if let Some(p) = &resolved {
        state.caches.put_token(token_hash, p.clone());
    }
    Ok(resolved)
}

/// True when the token behind `token_hash` is still valid (not revoked).
/// Bypasses the token cache.
pub(crate) async fn token_still_valid(state: &AppState, token_hash: &str) -> bool {
    let store = state.store.clone();
    let hash = token_hash.to_string();
    match tokio::task::spawn_blocking(move || store.find_token_by_hash(&hash)).await {
        Ok(Ok(found)) => found.is_some(),
        // A transient lookup failure does not tear the subscription down.
        _ => true,
    }
}

/// True when the request may administer tokens: the owner principal, or an
/// unauthenticated localhost request in open mode.
pub(crate) fn is_admin(state: &AppState, ctx: &AuthContext) -> bool {
    match &ctx.principal {
        Some(p) => p.id == state.owner,
        None => !state.require_auth,
    }
}

// ---------------------------------------------------------------------------
// Token administration endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateTokenPayload {
    /// Principal URI the token authenticates as, e.g. `agent://yannick/coder`.
    pub principal: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Topic patterns the token may read. Defaults to `["**"]`.
    #[serde(default)]
    pub read_scopes: Option<Vec<String>>,
    /// Topic patterns the token may write. Defaults to `["**"]`.
    #[serde(default)]
    pub write_scopes: Option<Vec<String>>,
}

pub async fn create_token(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Json(payload): Json<CreateTokenPayload>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !is_admin(&state, &ctx) {
        return Err((
            StatusCode::FORBIDDEN,
            "Only the owner may manage tokens".to_string(),
        ));
    }

    let principal = Principal {
        id: payload.principal.clone(),
        kind: kind_of_uri(&payload.principal).to_string(),
        display_name: payload.display_name,
        created_at: Utc::now(),
    };
    state.store.upsert_principal(&principal).map_err(internal)?;

    let plaintext = generate_token();
    let record = TokenRecord {
        id: format!("tok_{}", Ulid::new().to_string().to_lowercase()),
        principal: payload.principal,
        token_hash: hash_token(&plaintext),
        read_scopes: payload
            .read_scopes
            .unwrap_or_else(|| vec!["**".to_string()]),
        write_scopes: payload
            .write_scopes
            .unwrap_or_else(|| vec!["**".to_string()]),
        created_at: Utc::now(),
        revoked: false,
    };
    state.store.insert_token(&record).map_err(internal)?;
    // Register the derived end-to-end key (linked to this token, so revoking
    // the token also deletes the key) for encrypted relay traffic.
    let _ = state
        .store
        .register_e2e_key_for_token(&record.id, &plaintext);

    // The plaintext token is returned exactly once and never stored.
    Ok(Json(serde_json::json!({
        "id": record.id,
        "principal": record.principal,
        "token": plaintext,
        "read_scopes": record.read_scopes,
        "write_scopes": record.write_scopes,
    })))
}

pub async fn list_tokens(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !is_admin(&state, &ctx) {
        return Err((
            StatusCode::FORBIDDEN,
            "Only the owner may manage tokens".to_string(),
        ));
    }
    let tokens = state.store.list_tokens().map_err(internal)?;
    // Strip hashes from the listing.
    let sanitized: Vec<serde_json::Value> = tokens
        .into_iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "principal": t.principal,
                "read_scopes": t.read_scopes,
                "write_scopes": t.write_scopes,
                "created_at": t.created_at,
                "revoked": t.revoked,
            })
        })
        .collect();
    Ok(Json(sanitized))
}

pub async fn revoke_token(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    if !is_admin(&state, &ctx) {
        return Err((
            StatusCode::FORBIDDEN,
            "Only the owner may manage tokens".to_string(),
        ));
    }
    let revoked = state.store.revoke_token(&id).map_err(internal)?;
    // Revocation takes effect immediately, not after the cache TTL.
    state.caches.clear_tokens();
    if revoked {
        Ok(StatusCode::OK)
    } else {
        Err((StatusCode::NOT_FOUND, format!("Token {id} not found")))
    }
}

fn internal(e: anyhow::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// First-run bootstrap: if no tokens exist, create the owner principal and a
/// full-access owner token. The plaintext is written (mode 0600 on unix) to
/// `owner.token` next to the database so the local user can pick it up; it
/// is never logged.
pub fn bootstrap_owner(store: &mellowmesh_store::Store, db_path: &std::path::Path) -> String {
    if let Ok(Some(owner)) = store.get_config("owner_principal") {
        return owner;
    }

    let username = std::env::var("MELLOWMESH_OWNER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "owner".to_string())
        .to_lowercase();
    let owner_uri = if username.contains("://") {
        username
    } else {
        format!("human://{username}")
    };

    let principal = Principal {
        id: owner_uri.clone(),
        kind: "human".to_string(),
        display_name: None,
        created_at: Utc::now(),
    };
    let plaintext = generate_token();
    let record = TokenRecord {
        id: format!("tok_{}", Ulid::new().to_string().to_lowercase()),
        principal: owner_uri.clone(),
        token_hash: hash_token(&plaintext),
        read_scopes: vec!["**".to_string()],
        write_scopes: vec!["**".to_string()],
        created_at: Utc::now(),
        revoked: false,
    };

    let result = store
        .upsert_principal(&principal)
        .and_then(|_| store.insert_token(&record))
        // Linked to the token row so revoking the token deletes the key.
        .and_then(|_| store.register_e2e_key_for_token(&record.id, &plaintext))
        .and_then(|_| store.set_config("owner_principal", &owner_uri));
    if let Err(e) = result {
        tracing::error!("Owner bootstrap failed: {}", e);
        return owner_uri;
    }

    let token_file = owner_token_path(db_path);
    match write_secret_file(&token_file, plaintext.as_bytes()) {
        Ok(_) => {
            tracing::info!(
                "Owner identity '{}' created. Owner token written to {:?} — keep it secret.",
                owner_uri,
                token_file
            );
        }
        Err(e) => {
            // Never log the plaintext token.
            tracing::error!(
                "Owner identity '{}' created but the token file {:?} could not be written: {}",
                owner_uri,
                token_file,
                e
            );
        }
    }
    owner_uri
}

/// `owner.token` lives next to the SQLite database.
pub fn owner_token_path(db_path: &std::path::Path) -> std::path::PathBuf {
    match db_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join("owner.token"),
        _ => std::path::PathBuf::from("owner.token"),
    }
}

/// Write a secret atomically: a fresh temporary file created with mode 0600
/// on unix from the start (never readable by others, not even briefly), then
/// renamed over `path`.
fn write_secret_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension(format!("tmp.{}", Ulid::new().to_string().to_lowercase()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&tmp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_cache_rejects_reuse_and_out_of_window_timestamps() {
        let now = Utc::now().timestamp();
        let jti = format!("jti-{}", Ulid::new());
        assert!(register_jti(&jti, now, now));
        assert!(!register_jti(&jti, now, now), "a replay must be rejected");
        // Too far in the future, or too old.
        let future = now + MAX_FUTURE_SKEW_SECS + 1;
        assert!(!register_jti(&format!("f-{}", Ulid::new()), future, now));
        let stale = now - REPLAY_WINDOW_SECS - 1;
        assert!(!register_jti(&format!("o-{}", Ulid::new()), stale, now));
        // A slightly future-dated id is remembered until ts + window, not
        // merely receipt time + window.
        let fut = format!("s-{}", Ulid::new());
        let ts = now + 20;
        assert!(register_jti(&fut, ts, now));
        let exp = *replay_cache()
            .lock()
            .unwrap()
            .expiry
            .get(&fut)
            .expect("id is cached");
        assert_eq!(exp, ts + REPLAY_WINDOW_SECS);
    }

    #[test]
    fn owner_token_file_is_private_and_linked() {
        let dir = std::env::temp_dir().join(format!(
            "mm-owner-{}",
            Ulid::new().to_string().to_lowercase()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("mellowmesh.db");
        let store = mellowmesh_store::Store::new(&db).unwrap();
        bootstrap_owner(&store, &db);
        let token_file = dir.join("owner.token");
        let token = std::fs::read_to_string(&token_file).unwrap();
        assert!(token.starts_with("mm_"));
        let record = store
            .find_token_by_hash(&hash_token(&token))
            .unwrap()
            .expect("owner token stored");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&token_file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "owner.token must be mode 0600");
        }
        // No temporary files are left behind.
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .count();
        assert_eq!(leftovers, 0);
        // The e2e key is linked to the token: revoking deletes it.
        let key_id = mellowmesh_core::e2e::derive_key_id(&token);
        assert!(store.find_e2e_key(&key_id).unwrap().is_some());
        store.revoke_token(&record.id).unwrap();
        assert!(store.find_e2e_key(&key_id).unwrap().is_none());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
