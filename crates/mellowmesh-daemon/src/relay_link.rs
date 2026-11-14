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
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
    state: &AppState,
    ws_url: &str,
    config: &RelayConfig,
    local_port: u16,
) -> anyhow::Result<()> {
    let (ws_stream, _) =
        tokio::time::timeout(LINK_IDLE_TIMEOUT, tokio_tungstenite::connect_async(ws_url)).await??;
    let (mut ws_tx, mut ws_rx) = ws_stream.split();

    ws_tx
        .send(WsMessage::Text(serde_json::to_string(
            &RelayFrame::Register {
                hub_id: config.hub_id.clone(),
                link_key: config.link_key.clone(),
            },
        )?))
        .await?;

    // Single writer task fed by the per-request executors. It also pings the
    // relay periodically so a half-open link is noticed.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<RelayFrame>(256);
    let mut writer = tokio::spawn(async move {
        let mut ping = tokio::time::interval_at(
            tokio::time::Instant::now() + LINK_PING_EVERY,
            LINK_PING_EVERY,
        );
        loop {
            tokio::select! {
                frame = out_rx.recv() => {
                    let Some(frame) = frame else { break };
                    let Ok(text) = serde_json::to_string(&frame) else {
                        continue;
                    };
                    if ws_tx.send(WsMessage::Text(text)).await.is_err() {
                        break;
                    }
                }
                _ = ping.tick() => {
                    if ws_tx.send(WsMessage::Ping(Vec::new())).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Every request replayed for a relay client is marked remote, so it is
    // never treated as a trusted local caller.
    let marker = state.access.remote_marker.clone();
    let http = remote_dispatch_client(&marker);
    let base = format!("http://127.0.0.1:{local_port}");
    // Live subscription bridges opened on behalf of remote clients, keyed by
    // stream id, with a generation so a finished bridge only removes itself.
    let streams: Arc<Mutex<HashMap<String, (u64, tokio::task::AbortHandle)>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let mut next_generation: u64 = 0;

    let reader_result: anyhow::Result<()> = async {
        loop {
            // Nothing at all (not even a pong) for LINK_IDLE_TIMEOUT means
            // the link is dead: reconnect.
            let msg = match tokio::time::timeout(LINK_IDLE_TIMEOUT, ws_rx.next()).await {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(_) => anyhow::bail!("relay link idle for {:?}", LINK_IDLE_TIMEOUT),
            };
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
                            let out = out_tx.clone();
                            // Only an absolute path on this hub may be
                            // forwarded; anything else could redirect the
                            // loopback request to another host.
                            if !is_forwardable_path(&path) {
                                let _ = out
                                    .send(RelayFrame::Response {
                                        id,
                                        status: 400,
                                        content_type: Some("text/plain".to_string()),
                                        body: Some("Invalid request path".to_string()),
                                    })
                                    .await;
                                continue;
                            }
                            let http = http.clone();
                            let base = base.clone();
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
                        RelayFrame::StreamOpen { stream_id, query } => {
                            let out = out_tx.clone();
                            let streams_map = streams.clone();
                            let sid = stream_id.clone();
                            let marker = marker.clone();
                            next_generation += 1;
                            let generation = next_generation;
                            // Hold the lock across spawn + insert so the
                            // bridge's own cleanup cannot run before it is
                            // registered.
                            let mut map = streams.lock().unwrap();
                            let handle = tokio::spawn(async move {
                                run_stream_bridge(
                                    local_port,
                                    &marker,
                                    sid.clone(),
                                    query,
                                    out.clone(),
                                )
                                .await;
                                {
                                    let mut map = streams_map.lock().unwrap();
                                    if map.get(&sid).is_some_and(|(g, _)| *g == generation) {
                                        map.remove(&sid);
                                    }
                                }
                                let _ = out.send(RelayFrame::StreamClose { stream_id: sid }).await;
                            });
                            // A reused stream id replaces (and stops) the
                            // previous bridge instead of leaking it.
                            if let Some((_, old)) =
                                map.insert(stream_id, (generation, handle.abort_handle()))
                            {
                                old.abort();
                            }
                        }
                        RelayFrame::StreamClose { stream_id } => {
                            if let Some((_, handle)) = streams.lock().unwrap().remove(&stream_id) {
                                handle.abort();
                            }
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
    // The relay link is gone: tear down every local subscription bridge.
    for (_, (_, handle)) in streams.lock().unwrap().drain() {
        handle.abort();
    }
    reader_result
}

/// The loopback client used to replay relayed requests: like
/// [`crate::state::loopback_http_client`], plus the remote marker header.
fn remote_dispatch_client(marker: &str) -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(marker) {
        headers.insert(crate::access::REMOTE_MARKER_HEADER, v);
    }
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(headers)
        .build()
        .unwrap_or_default()
}

/// Ping the relay this often.
const LINK_PING_EVERY: Duration = Duration::from_secs(30);
/// Treat the link as dead after this long without any inbound frame.
const LINK_IDLE_TIMEOUT: Duration = Duration::from_secs(90);

/// A forwarded request path must be an absolute path on this hub: it starts
/// with a single `/` (not `//`, which would be read as an authority) and
/// contains no backslash or control characters.
pub fn is_forwardable_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.starts_with("//")
        && !path.contains('\\')
        && !path.chars().any(|c| c.is_control() || c == ' ')
}

/// Bridge one remote subscription: open a local WebSocket (the query carries
/// the pattern and the remote client's token, so daemon auth applies) and
/// forward every delivered message up the relay link.
async fn run_stream_bridge(
    local_port: u16,
    marker: &str,
    stream_id: String,
    query: Option<String>,
    out: tokio::sync::mpsc::Sender<RelayFrame>,
) {
    let url = format!(
        "ws://127.0.0.1:{local_port}/ws{}",
        query.map(|q| format!("?{q}")).unwrap_or_default()
    );
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let (mut request, marker) = match (url.into_client_request(), marker.parse()) {
        (Ok(r), Ok(m)) => (r, m),
        _ => {
            tracing::debug!("Stream bridge {} has an invalid query", stream_id);
            return;
        }
    };
    request
        .headers_mut()
        .insert(crate::access::REMOTE_MARKER_HEADER, marker);
    let ws = match tokio_tungstenite::connect_async(request).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            tracing::debug!("Stream bridge {} failed to open: {}", stream_id, e);
            return;
        }
    };
    let (_ws_tx, mut ws_rx) = ws.split();
    while let Some(Ok(msg)) = ws_rx.next().await {
        match msg {
            WsMessage::Text(text) => {
                if out
                    .send(RelayFrame::StreamData {
                        stream_id: stream_id.clone(),
                        text,
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }
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
    anyhow::ensure!(is_forwardable_path(path), "invalid request path");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_paths_must_stay_on_this_hub() {
        assert!(is_forwardable_path("/tasks"));
        assert!(is_forwardable_path("/wiki/w/pages/a/b.md"));
        assert!(!is_forwardable_path("@169.254.169.254/latest"));
        assert!(!is_forwardable_path("//169.254.169.254/latest"));
        assert!(!is_forwardable_path(""));
        assert!(!is_forwardable_path("tasks"));
        assert!(!is_forwardable_path("/\\evil"));
        assert!(!is_forwardable_path("/a b"));
        assert!(!is_forwardable_path("/a\r\nHost: x"));
    }
}
