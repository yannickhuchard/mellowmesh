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
use serde::Deserialize;
use ulid::Ulid;

/// The authenticated principal attached to a request, if any.
#[derive(Debug, Clone)]
pub struct AuthPrincipal {
    pub id: String,
    pub kind: String,
    pub read_scopes: Vec<String>,
    pub write_scopes: Vec<String>,
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
        v: 1,
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
    if bearer.is_none() {
        if let Some(envelope) = extract_e2e_proof(&req) {
            use mellowmesh_core::e2e::{open, SealedRequest, REPLAY_WINDOW_SECS};
            let key = match state.store.find_e2e_key(&envelope.key_id) {
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
            let proof = open(&key, &envelope)
                .ok()
                .and_then(|pt| serde_json::from_slice::<SealedRequest>(&pt).ok());
            match proof {
                Some(p) if (Utc::now().timestamp() - p.ts).abs() <= REPLAY_WINDOW_SECS => {
                    bearer = p
                        .authorization
                        .as_deref()
                        .and_then(|a| a.strip_prefix("Bearer "))
                        .map(|t| t.trim().to_string());
                    stream_e2e = StreamE2e(Some((key, envelope.key_id.clone())));
                }
                _ => {
                    return (StatusCode::UNAUTHORIZED, "Invalid e2e subscription proof")
                        .into_response()
                }
            }
        }
    }

    if let Some(token) = bearer {
        match state.store.find_token_by_hash(&hash_token(&token)) {
            Ok(Some(record)) => {
                let kind = state
                    .store
                    .get_principal(&record.principal)
                    .ok()
                    .flatten()
                    .map(|p| p.kind)
                    .unwrap_or_else(|| kind_of_uri(&record.principal).to_string());
                ctx.principal = Some(AuthPrincipal {
                    id: record.principal,
                    kind,
                    read_scopes: record.read_scopes,
                    write_scopes: record.write_scopes,
                });
            }
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

/// True when the request may administer tokens: the owner principal, or an
/// unauthenticated localhost request in open mode.
fn is_admin(state: &AppState, ctx: &AuthContext) -> bool {
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
    // Register the derived end-to-end key so this token can also be used
    // for encrypted relay traffic.
    let _ = state.store.register_e2e_key(&plaintext);

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
/// full-access owner token. The plaintext is logged once and written next to
/// the database so the local user can pick it up.
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
        .and_then(|_| store.register_e2e_key(&plaintext))
        .and_then(|_| store.set_config("owner_principal", &owner_uri));
    if let Err(e) = result {
        tracing::error!("Owner bootstrap failed: {}", e);
        return owner_uri;
    }

    let token_file = db_path
        .parent()
        .map(|p| p.join("owner.token"))
        .unwrap_or_else(|| std::path::PathBuf::from("owner.token"));
    match std::fs::write(&token_file, &plaintext) {
        Ok(_) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o600));
            }
            tracing::info!(
                "Owner identity '{}' created. Owner token written to {:?} — keep it secret.",
                owner_uri,
                token_file
            );
        }
        Err(e) => {
            tracing::warn!(
                "Owner identity '{}' created but token file could not be written ({}). Token (shown once): {}",
                owner_uri,
                e,
                plaintext
            );
        }
    }
    owner_uri
}
