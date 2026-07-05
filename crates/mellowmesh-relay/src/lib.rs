//! MellowMesh relay: a rendezvous server that makes local hubs reachable
//! from anywhere without inbound firewall holes.
//!
//! Daemons dial *outbound* WebSocket connections to `/link` and register a
//! hub id + link key. Remote clients send plain HTTP to
//! `/hub/<hub_id>/<api-path>`; each request is framed, forwarded down the
//! hub's link, executed by the daemon against its local API (including its
//! own bearer-token authentication), and the response is relayed back.
//!
//! The relay stores nothing and can read only what passes through it —
//! deploy it behind TLS. Self-hosting is a first-class use case.

use axum::{
    body::Bytes,
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, RawQuery, State,
    },
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use mellowmesh_core::relay::RelayFrame;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use ulid::Ulid;

/// How long a forwarded request may wait for the daemon's answer.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(30);

/// One event for a live subscription stream held open by a remote client.
enum StreamEvent {
    Data(String),
    Close,
}

struct Hub {
    link_key: String,
    /// Frames to send down the daemon link.
    to_daemon: mpsc::Sender<RelayFrame>,
    /// In-flight forwarded requests awaiting a response frame.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<RelayFrame>>>>,
    /// Live subscription streams: stream id → events for the remote client.
    streams: Arc<Mutex<HashMap<String, mpsc::Sender<StreamEvent>>>>,
}

#[derive(Clone, Default)]
pub struct RelayState {
    hubs: Arc<Mutex<HashMap<String, Arc<Hub>>>>,
}

pub fn create_router(state: RelayState) -> Router {
    Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/link", get(link_handler))
        .route("/hub/:hub_id", any(forward_handler_root))
        .route("/hub/:hub_id/", any(forward_handler_root))
        .route("/hub/:hub_id/ws", get(hub_stream_handler))
        .route("/hub/:hub_id/*path", any(forward_handler))
        .with_state(state)
}

async fn link_handler(ws: WebSocketUpgrade, State(state): State<RelayState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_link(socket, state))
}

/// Daemon-side link lifecycle: expect a Register frame, then route frames
/// in both directions until the connection drops.
async fn handle_link(socket: WebSocket, state: RelayState) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // First frame must be a registration.
    let register = match ws_rx.next().await {
        Some(Ok(WsMessage::Text(text))) => serde_json::from_str::<RelayFrame>(&text).ok(),
        _ => None,
    };
    let (hub_id, link_key) = match register {
        Some(RelayFrame::Register { hub_id, link_key }) => (hub_id, link_key),
        _ => {
            let _ = ws_tx
                .send(WsMessage::Text(
                    serde_json::to_string(&RelayFrame::Error {
                        message: "First frame must be a register frame".to_string(),
                    })
                    .unwrap_or_default(),
                ))
                .await;
            return;
        }
    };

    let (to_daemon_tx, mut to_daemon_rx) = mpsc::channel::<RelayFrame>(256);
    let pending: Arc<Mutex<HashMap<String, oneshot::Sender<RelayFrame>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let streams: Arc<Mutex<HashMap<String, mpsc::Sender<StreamEvent>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // Claim (or re-claim) the hub id. A different link key is rejected so a
    // stranger cannot hijack an established hub id.
    let accepted = {
        let mut hubs = state.hubs.lock().unwrap();
        let key_matches = hubs
            .get(&hub_id)
            .map(|existing| existing.link_key == link_key)
            .unwrap_or(true);
        if key_matches {
            hubs.insert(
                hub_id.clone(),
                Arc::new(Hub {
                    link_key,
                    to_daemon: to_daemon_tx,
                    pending: pending.clone(),
                    streams: streams.clone(),
                }),
            );
        }
        key_matches
    };
    if !accepted {
        let _ = ws_tx
            .send(WsMessage::Text(
                serde_json::to_string(&RelayFrame::Error {
                    message: "Hub id is registered with a different link key".to_string(),
                })
                .unwrap_or_default(),
            ))
            .await;
        return;
    }
    tracing::info!("Hub '{}' linked", hub_id);

    let _ = ws_tx
        .send(WsMessage::Text(
            serde_json::to_string(&RelayFrame::Registered {
                hub_id: hub_id.clone(),
            })
            .unwrap_or_default(),
        ))
        .await;

    // Writer: frames queued for the daemon → WebSocket.
    let mut writer = tokio::spawn(async move {
        while let Some(frame) = to_daemon_rx.recv().await {
            let Ok(text) = serde_json::to_string(&frame) else {
                continue;
            };
            if ws_tx.send(WsMessage::Text(text)).await.is_err() {
                break;
            }
        }
    });

    // Reader: response and stream frames from the daemon → waiting HTTP
    // handlers and open subscription streams.
    let pending_reader = pending.clone();
    let streams_reader = streams.clone();
    let mut reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                WsMessage::Text(text) => {
                    if let Ok(frame) = serde_json::from_str::<RelayFrame>(&text) {
                        match frame {
                            RelayFrame::Response { ref id, .. } => {
                                let waiter = pending_reader.lock().unwrap().remove(id);
                                if let Some(tx) = waiter {
                                    let _ = tx.send(frame);
                                }
                            }
                            RelayFrame::StreamData { stream_id, text } => {
                                let sender = {
                                    let streams = streams_reader.lock().unwrap();
                                    streams.get(&stream_id).cloned()
                                };
                                if let Some(tx) = sender {
                                    let _ = tx.send(StreamEvent::Data(text)).await;
                                }
                            }
                            RelayFrame::StreamClose { stream_id } => {
                                let sender = streams_reader.lock().unwrap().remove(&stream_id);
                                if let Some(tx) = sender {
                                    let _ = tx.send(StreamEvent::Close).await;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                WsMessage::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = (&mut writer) => { reader.abort(); }
        _ = (&mut reader) => { writer.abort(); }
    }

    // Drop the hub registration only if it still points at this connection's
    // channel (a reconnect may already have replaced it).
    let mut hubs = state.hubs.lock().unwrap();
    if let Some(hub) = hubs.get(&hub_id) {
        if hub.to_daemon.is_closed() {
            hubs.remove(&hub_id);
            tracing::info!("Hub '{}' unlinked", hub_id);
        }
    }
    drop(hubs);
    // Dropping the senders ends every live stream attached to this link.
    streams.lock().unwrap().clear();
}

/// Remote client opening a live subscription stream through the relay.
/// The raw query (pattern, case_insensitive, token) is passed to the daemon,
/// which opens a matching local subscription under its own authentication.
async fn hub_stream_handler(
    ws: WebSocketUpgrade,
    Path(hub_id): Path<String>,
    RawQuery(query): RawQuery,
    State(state): State<RelayState>,
) -> Response {
    let hub = {
        let hubs = state.hubs.lock().unwrap();
        hubs.get(&hub_id).cloned()
    };
    let Some(hub) = hub else {
        return (
            StatusCode::BAD_GATEWAY,
            format!("Hub '{hub_id}' is not connected to this relay"),
        )
            .into_response();
    };
    ws.on_upgrade(move |socket| handle_hub_stream(socket, hub, query))
}

async fn handle_hub_stream(socket: WebSocket, hub: Arc<Hub>, query: Option<String>) {
    let stream_id = format!("str_{}", Ulid::new().to_string().to_lowercase());
    let (ev_tx, mut ev_rx) = mpsc::channel::<StreamEvent>(256);
    hub.streams.lock().unwrap().insert(stream_id.clone(), ev_tx);

    if hub
        .to_daemon
        .send(RelayFrame::StreamOpen {
            stream_id: stream_id.clone(),
            query,
        })
        .await
        .is_err()
    {
        hub.streams.lock().unwrap().remove(&stream_id);
        return;
    }

    let (mut ws_tx, mut ws_rx) = socket.split();
    loop {
        tokio::select! {
            event = ev_rx.recv() => match event {
                Some(StreamEvent::Data(text)) => {
                    if ws_tx.send(WsMessage::Text(text)).await.is_err() {
                        break;
                    }
                }
                Some(StreamEvent::Close) | None => break,
            },
            msg = ws_rx.next() => match msg {
                Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            },
        }
    }

    // Tell the daemon to tear down its local subscription (best-effort).
    hub.streams.lock().unwrap().remove(&stream_id);
    let _ = hub
        .to_daemon
        .send(RelayFrame::StreamClose { stream_id })
        .await;
}

async fn forward_handler_root(
    Path(hub_id): Path<String>,
    State(state): State<RelayState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward(state, hub_id, "".to_string(), method, uri, headers, body).await
}

async fn forward_handler(
    Path((hub_id, path)): Path<(String, String)>,
    State(state): State<RelayState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward(state, hub_id, path, method, uri, headers, body).await
}

async fn forward(
    state: RelayState,
    hub_id: String,
    path: String,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let hub = {
        let hubs = state.hubs.lock().unwrap();
        hubs.get(&hub_id).cloned()
    };
    let Some(hub) = hub else {
        return (
            StatusCode::BAD_GATEWAY,
            format!("Hub '{hub_id}' is not connected to this relay"),
        )
            .into_response();
    };

    let id = format!("req_{}", Ulid::new().to_string().to_lowercase());
    let frame = RelayFrame::Request {
        id: id.clone(),
        method: method.to_string(),
        path: format!("/{path}"),
        query: uri.query().map(|q| q.to_string()),
        authorization: headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()),
        body: if body.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&body).to_string())
        },
    };

    let (resp_tx, resp_rx) = oneshot::channel();
    hub.pending.lock().unwrap().insert(id.clone(), resp_tx);

    if hub.to_daemon.send(frame).await.is_err() {
        hub.pending.lock().unwrap().remove(&id);
        return (
            StatusCode::BAD_GATEWAY,
            "Hub link dropped while forwarding".to_string(),
        )
            .into_response();
    }

    match tokio::time::timeout(FORWARD_TIMEOUT, resp_rx).await {
        Ok(Ok(RelayFrame::Response {
            status,
            content_type,
            body,
            ..
        })) => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = (status, body.unwrap_or_default()).into_response();
            if let Some(ct) = content_type {
                if let Ok(value) = axum::http::HeaderValue::from_str(&ct) {
                    response
                        .headers_mut()
                        .insert(axum::http::header::CONTENT_TYPE, value);
                }
            }
            response
        }
        Ok(_) => (
            StatusCode::BAD_GATEWAY,
            "Malformed hub response".to_string(),
        )
            .into_response(),
        Err(_) => {
            hub.pending.lock().unwrap().remove(&id);
            (
                StatusCode::GATEWAY_TIMEOUT,
                "Hub did not answer in time".to_string(),
            )
                .into_response()
        }
    }
}
