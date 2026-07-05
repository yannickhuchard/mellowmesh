//! Outbound link to a MellowMesh relay.
//!
//! When `MELLOWMESH_RELAY_URL` is set, the daemon dials the relay's `/link`
//! WebSocket endpoint, registers its hub id, and then serves forwarded HTTP
//! requests by replaying them against its own local API — so every remote
//! request passes through the exact same authentication and scope
//! enforcement as a local one. No inbound port is ever opened.

use crate::state::AppState;
use futures_util::{SinkExt, StreamExt};
use mellowmesh_core::relay::RelayFrame;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as WsMessage;

pub struct RelayConfig {
    pub relay_url: String,
    pub hub_id: String,
    pub link_key: String,
}

/// Resolve the relay configuration. Hub id and link key are generated on
/// first use and persisted so the hub URL stays stable across restarts.
pub fn load_config(store: &mellowmesh_store::Store) -> Option<RelayConfig> {
    let relay_url = std::env::var("MELLOWMESH_RELAY_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())?;

    let hub_id = match store.get_config("relay_hub_id") {
        Ok(Some(id)) => id,
        _ => {
            let id = ulid::Ulid::new().to_string().to_lowercase();
            let _ = store.set_config("relay_hub_id", &id);
            id
        }
    };
    let link_key = match store.get_config("relay_link_key") {
        Ok(Some(key)) => key,
        _ => {
            let key = mellowmesh_core::auth::generate_token();
            let _ = store.set_config("relay_link_key", &key);
            key
        }
    };
    Some(RelayConfig {
        relay_url: relay_url.trim_end_matches('/').to_string(),
        hub_id,
        link_key,
    })
}

pub fn start(state: AppState, config: RelayConfig, local_port: u16) {
    let ws_url = format!(
        "{}/link",
        config
            .relay_url
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1)
    );
    tracing::info!(
        "Relay link enabled. Your hub will be reachable at: {}/hub/{}",
        config.relay_url,
        config.hub_id
    );

    tokio::spawn(async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            match run_link(&state, &ws_url, &config, local_port).await {
                Ok(_) => {
                    tracing::warn!("Relay link closed; reconnecting...");
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    tracing::warn!("Relay link error ({}); retrying in {:?}", e, backoff);
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    });
}

async fn run_link(
    _state: &AppState,
    ws_url: &str,
    config: &RelayConfig,
    local_port: u16,
) -> anyhow::Result<()> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await?;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    ws_tx
        .send(WsMessage::Text(serde_json::to_string(
            &RelayFrame::Register {
                hub_id: config.hub_id.clone(),
                link_key: config.link_key.clone(),
            },
        )?))
        .await?;

    // Single writer task fed by the per-request executors.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<RelayFrame>(256);
    let mut writer = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let Ok(text) = serde_json::to_string(&frame) else {
                continue;
            };
            if ws_tx.send(WsMessage::Text(text)).await.is_err() {
                break;
            }
        }
    });

    let http = reqwest::Client::new();
    let base = format!("http://127.0.0.1:{local_port}");

    let reader_result: anyhow::Result<()> = async {
        while let Some(msg) = ws_rx.next().await {
            match msg? {
                WsMessage::Text(text) => {
                    let Ok(frame) = serde_json::from_str::<RelayFrame>(&text) else {
                        continue;
                    };
                    match frame {
                        RelayFrame::Registered { hub_id } => {
                            tracing::info!("Relay accepted hub '{}'", hub_id);
                        }
                        RelayFrame::Error { message } => {
                            anyhow::bail!("Relay rejected link: {message}");
                        }
                        RelayFrame::Request {
                            id,
                            method,
                            path,
                            query,
                            authorization,
                            body,
                        } => {
                            let http = http.clone();
                            let base = base.clone();
                            let out = out_tx.clone();
                            tokio::spawn(async move {
                                let response = execute_local(
                                    &http,
                                    &base,
                                    &method,
                                    &path,
                                    query.as_deref(),
                                    authorization.as_deref(),
                                    body,
                                )
                                .await;
                                let frame = match response {
                                    Ok((status, content_type, body)) => RelayFrame::Response {
                                        id,
                                        status,
                                        content_type,
                                        body,
                                    },
                                    Err(e) => RelayFrame::Response {
                                        id,
                                        status: 502,
                                        content_type: Some("text/plain".to_string()),
                                        body: Some(format!("Local forward failed: {e}")),
                                    },
                                };
                                let _ = out.send(frame).await;
                            });
                        }
                        _ => {}
                    }
                }
                WsMessage::Close(_) => break,
                _ => {}
            }
        }
        Ok(())
    }
    .await;

    writer.abort();
    let _ = (&mut writer).await;
    reader_result
}

async fn execute_local(
    http: &reqwest::Client,
    base: &str,
    method: &str,
    path: &str,
    query: Option<&str>,
    authorization: Option<&str>,
    body: Option<String>,
) -> anyhow::Result<(u16, Option<String>, Option<String>)> {
    let mut url = format!("{base}{path}");
    if let Some(q) = query {
        url = format!("{url}?{q}");
    }
    let method = reqwest::Method::from_bytes(method.as_bytes())?;
    let mut req = http.request(method, &url);
    if let Some(auth) = authorization {
        req = req.header(reqwest::header::AUTHORIZATION, auth);
    }
    if let Some(b) = body {
        req = req
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(b);
    }
    let resp = req.send().await?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let text = resp.text().await.ok();
    Ok((status, content_type, text))
}
