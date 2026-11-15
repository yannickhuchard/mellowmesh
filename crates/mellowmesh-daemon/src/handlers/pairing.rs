//! One-step pairing endpoints: `POST /pair`, `GET /pair/:token_id`,
//! `GET /devices`, `DELETE /devices/:token_id` (owner only; contract:
//! `docs/network.md`, "Pairing a device"). Errors are `{ "error": "…" }`.

use crate::auth::{is_admin, AuthContext};
use crate::state::AppState;
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Extension, Json,
};
use chrono::Utc;
use mellowmesh_core::auth::{generate_token, hash_token, Principal, TokenRecord};
use mellowmesh_core::pairing::{
    pairing_link, sanitize_device_name, DeviceInfo, PairRequest, PairResponse, PairStatus,
};
use mellowmesh_store::auth_store::DeviceToken;
use serde_json::json;
use ulid::Ulid;

type ApiError = (StatusCode, Json<serde_json::Value>);

fn error(status: StatusCode, msg: impl Into<String>) -> ApiError {
    (status, Json(json!({ "error": msg.into() })))
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn require_owner(state: &AppState, ctx: &AuthContext) -> Result<(), ApiError> {
    if is_admin(state, ctx) {
        Ok(())
    } else {
        Err(error(
            StatusCode::FORBIDDEN,
            "Only the owner may pair and manage devices",
        ))
    }
}

/// Run a blocking store call on the blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(internal)?
        .map_err(internal)
}

/// `POST /pair`: open the way in (LAN listener, or the given url's host),
/// mint a short-lived device token for the owner and return the link.
pub async fn pair(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    body: Bytes,
) -> Result<impl IntoResponse, ApiError> {
    require_owner(&state, &ctx)?;
    let request: PairRequest = if body.iter().all(u8::is_ascii_whitespace) {
        PairRequest::default()
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| error(StatusCode::BAD_REQUEST, format!("Invalid body: {e}")))?
    };
    let name = sanitize_device_name(request.name.as_deref());
    let mut network = state.store.get_network().map_err(internal)?;

    let url = match request
        .url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
    {
        Some(url) => {
            let host = crate::pairing::url_public_host(url)
                .map_err(|e| error(StatusCode::BAD_REQUEST, e))?;
            if state.access.add_public_host(&host) {
                tracing::info!("Public host added by pairing: {}", host);
            }
            let store = state.store.clone();
            blocking(move || crate::pairing::persist_public_host(&store, &host)).await?;
            url.trim_end_matches('/').to_string()
        }
        None => {
            let addr = crate::pairing::open_lan(&state)
                .await
                .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, e))?;
            if !network.lan_access {
                network.lan_access = true;
                let store = state.store.clone();
                let saved = network.clone();
                blocking(move || store.set_network(&saved)).await?;
                crate::access::refresh(&state);
            }
            format!("http://{addr}")
        }
    };

    // The device token: full access for the owner, pending until used.
    let owner = state.owner.clone();
    let plaintext = generate_token();
    let now = Utc::now();
    let ttl = chrono::Duration::from_std(state.access.pairing.device_ttl)
        .unwrap_or_else(|_| chrono::Duration::minutes(15));
    let expires_at = now + ttl;
    let record = TokenRecord {
        id: format!("tok_{}", Ulid::new().to_string().to_lowercase()),
        principal: owner.clone(),
        token_hash: hash_token(&plaintext),
        read_scopes: vec!["**".to_string()],
        write_scopes: vec!["**".to_string()],
        created_at: now,
        revoked: false,
    };
    let store = state.store.clone();
    let (saved, device_name, secret) = (record.clone(), name.clone(), plaintext.clone());
    blocking(move || {
        if store.get_principal(&saved.principal)?.is_none() {
            store.upsert_principal(&Principal {
                id: saved.principal.clone(),
                kind: mellowmesh_core::auth::kind_of_uri(&saved.principal).to_string(),
                display_name: None,
                created_at: now,
            })?;
        }
        store.insert_device_token(&saved, &device_name, expires_at)?;
        // Linked to the token, so revoking it deletes the key.
        store.register_e2e_key_for_token(&saved.id, &secret)
    })
    .await?;
    tracing::info!("Pairing '{}' (token {}) via {}", name, record.id, url);

    let response = PairResponse {
        link: pairing_link(&url, &plaintext, &network.name, &owner),
        url,
        token_id: record.id,
        name,
        network: network.name.clone(),
        principal: owner,
        expires_at,
        lan: crate::pairing::lan_status(&state, network.lan_access),
    };
    Ok((StatusCode::CREATED, Json(response)))
}

async fn device(state: &AppState, id: String) -> Result<DeviceToken, ApiError> {
    let store = state.store.clone();
    let key = id.clone();
    blocking(move || store.get_device_token(&key))
        .await?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, format!("Device {id} not found")))
}

/// `GET /pair/:token_id`: has the device connected yet? `404` for an
/// unknown or removed device.
pub async fn pair_status(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    require_owner(&state, &ctx)?;
    let d = device(&state, id).await?;
    // A removed (revoked) device is gone; one that expired unused is still
    // reported, as expired, until the sweep revokes it.
    if d.revoked {
        return Err(error(
            StatusCode::NOT_FOUND,
            format!("Device {} not found", d.id),
        ));
    }
    Ok(Json(PairStatus {
        connected: d.connected(),
        expired: d.expired(Utc::now()),
        last_seen: d.last_seen_at,
        token_id: d.id,
        name: d.name,
    }))
}

/// `GET /devices`: active device tokens (paired or pending), newest first.
pub async fn list_devices(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<impl IntoResponse, ApiError> {
    require_owner(&state, &ctx)?;
    let store = state.store.clone();
    let devices = blocking(move || store.list_device_tokens(Utc::now())).await?;
    Ok(Json(
        devices
            .into_iter()
            .map(|d| DeviceInfo {
                connected: d.connected(),
                token_id: d.id,
                name: d.name,
                created_at: d.created_at,
                last_seen: d.last_seen_at,
                expires_at: d.expires_at,
            })
            .collect::<Vec<_>>(),
    ))
}

/// `DELETE /devices/:token_id`: revoke a device token.
pub async fn remove_device(
    State(state): State<AppState>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    require_owner(&state, &ctx)?;
    let d = device(&state, id).await?;
    if d.revoked {
        return Err(error(
            StatusCode::NOT_FOUND,
            format!("Device {} not found", d.id),
        ));
    }
    let store = state.store.clone();
    let key = d.id.clone();
    blocking(move || store.revoke_token(&key)).await?;
    // Revocation takes effect immediately, not after the cache TTL.
    state.caches.clear_tokens();
    tracing::info!("Device '{}' (token {}) removed", d.name, d.id);
    Ok(StatusCode::NO_CONTENT)
}
