//! End-to-end encrypted request endpoint (`POST /e2e/request`).
//!
//! Accepts an opaque [`Envelope`] whose key id identifies a stored e2e key.
//! The daemon decrypts the sealed request, replays it against its own
//! loopback API (so the inner bearer token drives the same authentication
//! and scope enforcement as any other request), then seals the response
//! under the same key. A relay in the middle only ever sees ciphertext and
//! a key id that is useless without the hub's database.
//!
//! This path is exempt from the bearer-auth middleware: possession of the
//! key (proven by successful decryption) plus the sealed inner Authorization
//! header together provide authentication.

use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use mellowmesh_core::e2e::{
    open, seal, Envelope, SealedRequest, SealedResponse, REPLAY_WINDOW_SECS,
};

pub async fn handle_e2e_request(
    State(state): State<AppState>,
    Json(envelope): Json<Envelope>,
) -> impl IntoResponse {
    // Resolve the key by the envelope's key id.
    let key = match state.store.find_e2e_key(&envelope.key_id) {
        Ok(Some(k)) => k,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "Unknown e2e key id".to_string()).into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Key lookup failed: {e}"),
            )
                .into_response()
        }
    };

    // Decrypt. Failure means tampering or a wrong key.
    let plaintext = match open(&key, &envelope) {
        Ok(p) => p,
        Err(_) => {
            return (StatusCode::UNAUTHORIZED, "Decryption failed".to_string()).into_response()
        }
    };
    let sealed: SealedRequest = match serde_json::from_slice(&plaintext) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("Malformed sealed request: {e}"),
            )
                .into_response()
        }
    };

    // Replay window.
    if (Utc::now().timestamp() - sealed.ts).abs() > REPLAY_WINDOW_SECS {
        return (
            StatusCode::UNAUTHORIZED,
            "Sealed request outside replay window".to_string(),
        )
            .into_response();
    }

    // Dispatch against our own loopback API. The inner Authorization header
    // carries the caller's token, so auth/scopes apply exactly as usual.
    let dispatched = dispatch_local(state.port, &sealed).await;
    let sealed_response = match dispatched {
        Ok(r) => r,
        Err(e) => SealedResponse {
            status: 502,
            content_type: Some("text/plain".to_string()),
            body: Some(format!("Local dispatch failed: {e}")),
        },
    };

    // Seal the response under the same key.
    let payload = match serde_json::to_vec(&sealed_response) {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Response serialization failed: {e}"),
            )
                .into_response()
        }
    };
    match seal(&key, &envelope.key_id, &payload) {
        Ok(reply) => Json(reply).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Response sealing failed: {e}"),
        )
            .into_response(),
    }
}

async fn dispatch_local(port: u16, sealed: &SealedRequest) -> anyhow::Result<SealedResponse> {
    let url = format!("http://127.0.0.1:{port}{}", sealed.path_and_query);
    let method = reqwest::Method::from_bytes(sealed.method.as_bytes())?;
    let http = reqwest::Client::new();
    let mut req = http.request(method, &url);
    if let Some(auth) = &sealed.authorization {
        req = req.header(reqwest::header::AUTHORIZATION, auth);
    }
    if let Some(body) = &sealed.body {
        req = req
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.clone());
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let body = resp.text().await.ok();
    Ok(SealedResponse {
        status,
        content_type,
        body,
    })
}
