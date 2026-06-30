use crate::state::AppState;
use mellowmesh_core::message::Message;
use futures_util::StreamExt;
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

    // Construct WebSocket URL
    let ws_url = format!("ws://{}/ws?pattern={}", peer.addr, peer.pattern);

    loop {
        tracing::info!("Connecting to peer at {}", ws_url);
        match connect_async(&ws_url).await {
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

                                    // Append our node_id to forwarded_by
                                    let mut headers = msg.headers.clone().unwrap_or_default();
                                    let new_forwarded_by =
                                        if let Some(old) = headers.get("forwarded_by") {
                                            format!("{},{}", old, node_id)
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
                                    if let Err(e) =
                                        crate::handlers::message::handle_publish(std::sync::Arc::new(state.clone()), msg).await
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
