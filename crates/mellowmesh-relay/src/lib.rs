//! MellowMesh relay: a rendezvous server that makes local hubs reachable
//! from anywhere without inbound firewall holes.
//!
//! Daemons dial *outbound* WebSocket connections to `/link` and register a
//! hub id + link key. Remote clients send plain HTTP to
//! `/hub/<hub_id>/<api-path>`; each request is framed, forwarded down the
//! hub's link, executed by the daemon against its local API (including its
//! own bearer-token authentication), and the response is relayed back.
//!
//! The relay stores only hub-id → SHA-256(link key) bindings (optionally
//! persisted to a state file) and can read only what passes through it —
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
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Notify, Semaphore};
use ulid::Ulid;

/// How long a forwarded request may wait for the daemon's answer.
const FORWARD_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum length of a hub id.
pub const MAX_HUB_ID_LEN: usize = 64;

/// Maximum number of hub-id → key bindings the relay keeps.
pub const MAX_KNOWN_HUBS: usize = 100_000;

/// Maximum number of forwarded requests in flight per linked hub.
pub const MAX_INFLIGHT_PER_HUB: usize = 256;

/// Per-stream buffer between the link reader and the remote subscriber.
const STREAM_BUFFER: usize = 256;

/// One event for a live subscription stream held open by a remote client.
enum StreamEvent {
    Data(String),
    Close,
}

/// Relay-side end of a live subscription stream.
struct StreamSlot {
    events: mpsc::Sender<StreamEvent>,
    /// Signalled when the stream must end immediately (the subscriber fell
    /// too far behind, or the hub link dropped).
    cancel: Arc<Notify>,
}

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<RelayFrame>>>>;
type StreamMap = Arc<Mutex<HashMap<String, StreamSlot>>>;

struct Hub {
    /// Frames to send down the daemon link.
    to_daemon: mpsc::Sender<RelayFrame>,
    /// In-flight forwarded requests awaiting a response frame.
    pending: PendingMap,
    /// Live subscription streams: stream id → events for the remote client.
    streams: StreamMap,
    /// Bounds the number of concurrently forwarded requests.
    inflight: Arc<Semaphore>,
}

#[derive(Clone, Default)]
pub struct RelayState {
    hubs: Arc<Mutex<HashMap<String, Arc<Hub>>>>,
    /// Durable hub-id → SHA-256(link_key) binding. Unlike `hubs`, this
    /// survives a hub disconnecting, so a hub id cannot be hijacked with a
    /// different key while the legitimate hub is briefly offline (laptop
    /// sleep, daemon restart, network blip). Trust-on-first-use: the first
    /// registration of an id binds the key; later registrations must match.
    known_keys: Arc<Mutex<HashMap<String, String>>>,
    /// Where `known_keys` is persisted, if anywhere. `None` keeps bindings
    /// in memory only (they are lost on restart).
    state_file: Option<Arc<PathBuf>>,
    /// Serializes state-file writes so snapshots land in order.
    persist_lock: Arc<Mutex<()>>,
}

/// Why a registration was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterError {
    /// The hub id is empty, too long, or uses characters outside `[A-Za-z0-9_-]`.
    InvalidHubId,
    /// The hub id is bound to a different link key.
    KeyMismatch,
    /// The relay already holds [`MAX_KNOWN_HUBS`] bindings.
    Full,
    /// The new binding could not be written to the state file.
    Persist,
}

impl RegisterError {
    fn message(self) -> &'static str {
        match self {
            RegisterError::InvalidHubId => {
                "Invalid hub id: 1-64 characters from [A-Za-z0-9_-] required"
            }
            RegisterError::KeyMismatch => "Hub id is registered with a different link key",
            RegisterError::Full => "Relay has reached its hub registration limit",
            RegisterError::Persist => "Relay could not persist the hub registration",
        }
    }
}

/// Whether `hub_id` is 1..=64 characters of `[A-Za-z0-9_-]`.
pub fn is_valid_hub_id(hub_id: &str) -> bool {
    !hub_id.is_empty()
        && hub_id.len() <= MAX_HUB_ID_LEN
        && hub_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl RelayState {
    /// A relay whose hub-id → key bindings are persisted as JSON at `path`.
    /// Existing bindings are loaded now; every new binding rewrites the file
    /// atomically (temp file + rename). A missing file starts empty; an
    /// unreadable or malformed file is an error.
    pub fn with_state_file(path: PathBuf) -> anyhow::Result<Self> {
        let known: HashMap<String, String> = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                anyhow::anyhow!("relay state file {} is not valid: {e}", path.display())
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "cannot read relay state file {}: {e}",
                    path.display()
                ))
            }
        };
        Ok(Self {
            known_keys: Arc::new(Mutex::new(known)),
            state_file: Some(Arc::new(path)),
            ..Self::default()
        })
    }

    /// Number of hub-id → key bindings currently known.
    pub fn known_hub_count(&self) -> usize {
        self.known_keys.lock().unwrap().len()
    }

    /// Check `hub_id` against its bound key hash, binding it on first use.
    fn claim(&self, hub_id: &str, key_hash: &str) -> Result<(), RegisterError> {
        if !is_valid_hub_id(hub_id) {
            return Err(RegisterError::InvalidHubId);
        }
        {
            let mut known = self.known_keys.lock().unwrap();
            match known.get(hub_id) {
                Some(stored) => {
                    return if ct_eq(stored, key_hash) {
                        Ok(())
                    } else {
                        Err(RegisterError::KeyMismatch)
                    };
                }
                None => {
                    if known.len() >= MAX_KNOWN_HUBS {
                        return Err(RegisterError::Full);
                    }
                    known.insert(hub_id.to_string(), key_hash.to_string());
                }
            }
        }
        if let Err(e) = self.persist() {
            tracing::error!("Failed to persist relay state: {e}");
            // Fail closed: a binding that is not on disk would be lost on
            // restart, so refuse it rather than accept it silently.
            let mut known = self.known_keys.lock().unwrap();
            if known.get(hub_id).map(String::as_str) == Some(key_hash) {
                known.remove(hub_id);
            }
            return Err(RegisterError::Persist);
        }
        Ok(())
    }

    /// Write the current bindings to the state file, if one is configured.
    fn persist(&self) -> std::io::Result<()> {
        let Some(path) = self.state_file.as_deref() else {
            return Ok(());
        };
        let _guard = self.persist_lock.lock().unwrap();
        let snapshot = self.known_keys.lock().unwrap().clone();
        let bytes = serde_json::to_vec(&snapshot)?;
        write_atomic(path, &bytes)
    }
}

/// Replace `path` with `bytes` via a temp file in the same directory and a
/// rename, so a crash never leaves a truncated state file behind.
fn write_atomic(path: &FsPath, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let file_name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("state file path has no file name"))?
        .to_string_lossy()
        .into_owned();
    let tmp = path.with_file_name(format!(".{file_name}.tmp.{}", std::process::id()));
    let written = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return written;
    }
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Ok(d) = std::fs::File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

/// Constant-time comparison of two hex hashes of equal length.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn error_frame_text(message: &str) -> String {
    serde_json::to_string(&RelayFrame::Error {
        message: message.to_string(),
    })
    .unwrap_or_default()
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
                .send(WsMessage::Text(error_frame_text(
                    "First frame must be a register frame",
                )))
                .await;
            return;
        }
    };

    // Claim (or re-claim) the hub id. The key is checked against a DURABLE
    // binding that outlives disconnects, so a stranger cannot grab a hub id
    // while its owner is briefly offline. Only the SHA-256 of the key is kept,
    // so the relay never holds the link key in the clear.
    let key_hash = mellowmesh_core::auth::hash_token(&link_key);
    let claim = {
        let state = state.clone();
        let hub_id = hub_id.clone();
        // Claiming may write the state file; keep that off the async workers.
        tokio::task::spawn_blocking(move || state.claim(&hub_id, &key_hash))
            .await
            .unwrap_or(Err(RegisterError::Persist))
    };
    if let Err(e) = claim {
        match e {
            // Do not echo an unvalidated id into the logs.
            RegisterError::InvalidHubId => {
                tracing::warn!("Rejected registration with an invalid hub id")
            }
            _ => tracing::warn!("Rejected registration for hub '{}': {:?}", hub_id, e),
        }
        let _ = ws_tx
            .send(WsMessage::Text(error_frame_text(e.message())))
            .await;
        return;
    }

    let (to_daemon_tx, mut to_daemon_rx) = mpsc::channel::<RelayFrame>(256);
    let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
    let streams: StreamMap = Arc::new(Mutex::new(HashMap::new()));
    let this_hub = Arc::new(Hub {
        to_daemon: to_daemon_tx,
        pending: pending.clone(),
        streams: streams.clone(),
        inflight: Arc::new(Semaphore::new(MAX_INFLIGHT_PER_HUB)),
    });
    state
        .hubs
        .lock()
        .unwrap()
        .insert(hub_id.clone(), this_hub.clone());
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
    // handlers and open subscription streams. The reader never awaits a
    // consumer, so one slow subscriber cannot stall the whole link.
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
                                let mut streams = streams_reader.lock().unwrap();
                                let overflowed = match streams.get(&stream_id) {
                                    Some(slot) => matches!(
                                        slot.events.try_send(StreamEvent::Data(text)),
                                        Err(mpsc::error::TrySendError::Full(_))
                                    ),
                                    None => false,
                                };
                                if overflowed {
                                    // The subscriber is not keeping up: end
                                    // its stream. Its handler then tells the
                                    // daemon to close the local subscription.
                                    if let Some(slot) = streams.remove(&stream_id) {
                                        slot.cancel.notify_one();
                                    }
                                    tracing::warn!(
                                        "Closing stream '{}': subscriber too slow",
                                        stream_id
                                    );
                                }
                            }
                            RelayFrame::StreamClose { stream_id } => {
                                // Removing the slot drops the sender, so the
                                // handler ends once buffered data is drained.
                                let slot = streams_reader.lock().unwrap().remove(&stream_id);
                                if let Some(slot) = slot {
                                    let _ = slot.events.try_send(StreamEvent::Close);
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

    // Wait for either half to end, then make sure the other has fully
    // stopped (so the daemon-bound receiver is dropped before cleanup).
    tokio::select! {
        _ = (&mut writer) => {
            reader.abort();
            let _ = reader.await;
        }
        _ = (&mut reader) => {
            writer.abort();
            let _ = writer.await;
        }
    }

    // Drop the hub registration only if it is still this connection's entry
    // (a reconnect may already have replaced it).
    {
        let mut hubs = state.hubs.lock().unwrap();
        if hubs
            .get(&hub_id)
            .is_some_and(|hub| Arc::ptr_eq(hub, &this_hub))
        {
            hubs.remove(&hub_id);
            tracing::info!("Hub '{}' unlinked", hub_id);
        }
    }

    // Fail every request still waiting on this link right away. Requests
    // that arrive later find the daemon channel closed and fail on send.
    let waiters: Vec<_> = pending.lock().unwrap().drain().collect();
    for (id, tx) in waiters {
        let _ = tx.send(RelayFrame::Response {
            id,
            status: StatusCode::BAD_GATEWAY.as_u16(),
            content_type: Some("text/plain; charset=utf-8".to_string()),
            body: Some("Hub link dropped while forwarding".to_string()),
        });
    }

    // End every live stream attached to this link.
    let slots: Vec<_> = streams.lock().unwrap().drain().collect();
    for (_, slot) in slots {
        slot.cancel.notify_one();
    }
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
    let (ev_tx, mut ev_rx) = mpsc::channel::<StreamEvent>(STREAM_BUFFER);
    let cancel = Arc::new(Notify::new());
    hub.streams.lock().unwrap().insert(
        stream_id.clone(),
        StreamSlot {
            events: ev_tx,
            cancel: cancel.clone(),
        },
    );

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
            _ = cancel.notified() => break,
            event = ev_rx.recv() => match event {
                Some(StreamEvent::Data(text)) => {
                    tokio::select! {
                        sent = ws_tx.send(WsMessage::Text(text)) => {
                            if sent.is_err() {
                                break;
                            }
                        }
                        _ = cancel.notified() => break,
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
    // The link protocol carries bodies as text; refuse anything that would
    // not survive that unchanged instead of corrupting it.
    let body = if body.is_empty() {
        None
    } else {
        match String::from_utf8(body.to_vec()) {
            Ok(text) => Some(text),
            Err(_) => {
                return (
                    StatusCode::UNSUPPORTED_MEDIA_TYPE,
                    "The relay forwards UTF-8 request bodies only".to_string(),
                )
                    .into_response();
            }
        }
    };

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

    let Ok(_permit) = hub.inflight.clone().try_acquire_owned() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Too many requests in flight for this hub".to_string(),
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
        body,
    };

    let (resp_tx, resp_rx) = oneshot::channel();
    hub.pending.lock().unwrap().insert(id.clone(), resp_tx);

    let outcome = tokio::time::timeout(FORWARD_TIMEOUT, async {
        if hub.to_daemon.send(frame).await.is_err() {
            return None;
        }
        Some(resp_rx.await)
    })
    .await;

    match outcome {
        Ok(Some(Ok(RelayFrame::Response {
            status,
            content_type,
            body,
            ..
        }))) => {
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
        Ok(None) | Ok(Some(Err(_))) => {
            hub.pending.lock().unwrap().remove(&id);
            (
                StatusCode::BAD_GATEWAY,
                "Hub link dropped while forwarding".to_string(),
            )
                .into_response()
        }
        Ok(Some(Ok(_))) => (
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hub_id_validation() {
        assert!(is_valid_hub_id("hub1"));
        assert!(is_valid_hub_id("01hzz-abc_DEF"));
        assert!(is_valid_hub_id(&"a".repeat(MAX_HUB_ID_LEN)));
        assert!(!is_valid_hub_id(""));
        assert!(!is_valid_hub_id(&"a".repeat(MAX_HUB_ID_LEN + 1)));
        assert!(!is_valid_hub_id("bad id"));
        assert!(!is_valid_hub_id("hub/../x"));
        assert!(!is_valid_hub_id("hüb"));
    }

    #[test]
    fn claim_binds_on_first_use_and_checks_later() {
        let state = RelayState::default();
        assert_eq!(state.claim("hub1", "aaaa"), Ok(()));
        assert_eq!(state.claim("hub1", "aaaa"), Ok(()));
        assert_eq!(state.claim("hub1", "bbbb"), Err(RegisterError::KeyMismatch));
        assert_eq!(
            state.claim("bad id", "aaaa"),
            Err(RegisterError::InvalidHubId)
        );
        assert_eq!(state.known_hub_count(), 1);
    }

    #[test]
    fn claim_rejects_when_full() {
        let state = RelayState::default();
        {
            let mut known = state.known_keys.lock().unwrap();
            for i in 0..MAX_KNOWN_HUBS {
                known.insert(format!("h{i}"), "x".to_string());
            }
        }
        assert_eq!(state.claim("newhub", "aaaa"), Err(RegisterError::Full));
        // Existing bindings keep working at capacity.
        assert_eq!(state.claim("h0", "x"), Ok(()));
    }

    #[test]
    fn state_file_round_trip() {
        let dir = std::env::temp_dir().join(format!(
            "mellowmesh-relay-unit-{}-{}",
            std::process::id(),
            Ulid::new()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("relay-state.json");

        let first = RelayState::with_state_file(path.clone()).unwrap();
        assert_eq!(first.claim("hubP", "aaaa"), Ok(()));
        drop(first);

        let second = RelayState::with_state_file(path.clone()).unwrap();
        assert_eq!(second.known_hub_count(), 1);
        assert_eq!(
            second.claim("hubP", "bbbb"),
            Err(RegisterError::KeyMismatch)
        );
        assert_eq!(second.claim("hubP", "aaaa"), Ok(()));

        // No temp files are left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() != "relay-state.json")
            .collect();
        assert!(leftovers.is_empty());

        // A corrupt state file is an error, not a silent reset.
        std::fs::write(&path, b"not json").unwrap();
        assert!(RelayState::with_state_file(path).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
