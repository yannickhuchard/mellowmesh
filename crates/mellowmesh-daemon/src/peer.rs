use crate::state::AppState;
use futures_util::StreamExt;
use mellowmesh_core::message::Message;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

#[derive(Clone, Debug)]
pub struct PeerConfig {
    pub addr: String,
    pub pattern: String,
}

pub struct PeerManager {
    node_id: String,
    peers: Vec<PeerConfig>,
    state: Arc<Mutex<Option<AppState>>>,
}

impl PeerManager {
    pub fn new(node_id: String, peers: Vec<PeerConfig>) -> Self {
        Self {
            node_id,
            peers,
            state: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn set_state(&self, app_state: AppState) {
        *self.state.lock().await = Some(app_state);
    }

    pub fn start(&self) {
        for peer in &self.peers {
            let peer = peer.clone();
            let node_id = self.node_id.clone();
            let state_clone = self.state.clone();
            tokio::spawn(async move {
                // Wait for AppState to be initialized
                let mut app_state = None;
                for _ in 0..100 {
                    if let Some(ref s) = *state_clone.lock().await {
                        app_state = Some(s.clone());
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }

                let app_state = match app_state {
                    Some(s) => s,
                    None => {
                        tracing::error!(
                            "PeerManager failed to retrieve AppState for peer {}",
                            peer.addr
                        );
                        return;
                    }
                };

                run_peer_link(node_id, peer, app_state).await;
            });
        }
    }
}

async fn run_peer_link(node_id: String, peer: PeerConfig, state: AppState) {
    let mut backoff = Duration::from_secs(1);

    // Present a token to the peer so a peer running with --require-auth can
    // authenticate this link instead of accepting anonymous subscribers. It
    // travels in the Authorization header, never in the URL (which ends up
    // in logs).
    let peer_token = std::env::var("MELLOWMESH_PEER_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    let pattern_q: String = url::form_urlencoded::byte_serialize(peer.pattern.as_bytes()).collect();
    let ws_url = format!("ws://{}/ws?pattern={}", peer.addr, pattern_q);

    loop {
        tracing::info!("Connecting to peer at {}", ws_url);
        let request = match build_peer_request(&ws_url, peer_token.as_deref()) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Invalid peer address {}: {}", peer.addr, e);
                return;
            }
        };
        match connect_async(request).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Successfully connected to peer at {}", peer.addr);
                backoff = Duration::from_secs(1); // Reset backoff

                let (_, mut read) = ws_stream.split();

                while let Some(msg_res) = read.next().await {
                    match msg_res {
                        Ok(WsMessage::Text(text)) => {
                            match serde_json::from_str::<Message>(&text) {
                                Ok(mut msg) => {
                                    // Check for loop
                                    let mut loop_detected = false;
                                    if let Some(ref h) = msg.headers {
                                        if let Some(forwarded_by) = h.get("forwarded_by") {
                                            if forwarded_by.split(',').any(|n| n.trim() == node_id)
                                            {
                                                loop_detected = true;
                                            }
                                        }
                                    }

                                    if loop_detected {
                                        tracing::trace!(
                                            "Dropped looped message {} from peer {}",
                                            msg.id,
                                            peer.addr
                                        );
                                        continue;
                                    }

                                    // A peer may only inject topics inside the
                                    // pattern this link explicitly subscribed
                                    // to — it cannot push arbitrary topics
                                    // (e.g. `_decision.**`) into our fabric.
                                    if !mellowmesh_core::topic::match_topic(
                                        &peer.pattern,
                                        &msg.topic,
                                    ) {
                                        tracing::warn!(
                                            "Dropped out-of-pattern topic '{}' from peer {}",
                                            msg.topic,
                                            peer.addr
                                        );
                                        continue;
                                    }

                                    // Hub-local authority: a peer can never
                                    // inject decision outcomes, lease
                                    // reclaims or presence, whatever pattern
                                    // the link was configured with.
                                    if !peer_topic_allowed(&msg.topic) {
                                        tracing::warn!(
                                            "Dropped reserved topic '{}' from peer {}",
                                            msg.topic,
                                            peer.addr
                                        );
                                        continue;
                                    }

                                    // Append our node_id to forwarded_by
                                    let mut headers = msg.headers.clone().unwrap_or_default();
                                    let new_forwarded_by =
                                        if let Some(old) = headers.get("forwarded_by") {
                                            format!("{old},{node_id}")
                                        } else {
                                            node_id.clone()
                                        };
                                    headers.insert("forwarded_by".to_string(), new_forwarded_by);
                                    msg.headers = Some(headers);

                                    tracing::debug!(
                                        "Received message {} from peer {}, forwarding locally",
                                        msg.id,
                                        peer.addr
                                    );
                                    if let Err(e) = crate::handlers::message::handle_publish(
                                        std::sync::Arc::new(state.clone()),
                                        msg,
                                    )
                                    .await
                                    {
                                        tracing::error!("Failed to publish peer message: {:?}", e);
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "Failed to deserialize message from peer {}: {:?}",
                                        peer.addr,
                                        e
                                    );
                                }
                            }
                        }
                        Ok(WsMessage::Close(_)) => {
                            tracing::warn!("Peer {} closed connection", peer.addr);
                            break;
                        }
                        Err(e) => {
                            tracing::error!("Error reading from peer {}: {:?}", peer.addr, e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to connect to peer at {}: {:?}", peer.addr, e);
            }
        }

        tracing::warn!("Reconnecting to peer at {} in {:?}", peer.addr, backoff);
        tokio::time::sleep(backoff).await;
        backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
    }
}

/// Default pattern a peer link subscribes to when none is configured.
pub const DEFAULT_PEER_PATTERN: &str = "_forum.**";

/// Whether a topic received from a peer may be injected locally. Registry
/// sync (`_system.registry.*`) is the point of peering and is allowed;
/// decision events, lease reclaims and any other `_system.*` topic
/// (presence included) are authoritative only on the hub that owns them.
pub fn peer_topic_allowed(topic: &str) -> bool {
    use mellowmesh_core::topic::match_topic_with_options as m;
    if m("_system.registry.*", topic, true) {
        return true;
    }
    !(m("_decision.**", topic, true)
        || m("_task.*.reclaimed", topic, true)
        || m("_system.**", topic, true))
}

/// Build the WebSocket handshake request, carrying the peer token (if any)
/// as an `Authorization: Bearer` header.
fn build_peer_request(
    ws_url: &str,
    token: Option<&str>,
) -> anyhow::Result<tokio_tungstenite::tungstenite::handshake::client::Request> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{header::AUTHORIZATION, HeaderValue};
    let mut request = ws_url.into_client_request()?;
    if let Some(t) = token {
        request.headers_mut().insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {t}"))?,
        );
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_topics_are_not_accepted_from_peers() {
        assert!(peer_topic_allowed("_forum.general"));
        assert!(peer_topic_allowed("_system.registry.agent"));
        assert!(peer_topic_allowed("_system.registry.named_topic"));
        assert!(!peer_topic_allowed("_decision.d1.responded"));
        assert!(!peer_topic_allowed("_task.t1.reclaimed"));
        assert!(!peer_topic_allowed("_system.presence"));
        assert!(!peer_topic_allowed("_system.presence.node1"));
        assert!(!peer_topic_allowed("_SYSTEM.presence.node1"));
    }

    #[test]
    fn peer_token_goes_in_the_header() {
        let req =
            build_peer_request("ws://127.0.0.1:1/ws?pattern=_forum.%2A%2A", Some("mm_x")).unwrap();
        assert_eq!(req.headers()["authorization"], "Bearer mm_x");
        assert!(!req.uri().to_string().contains("mm_x"));
    }
}
