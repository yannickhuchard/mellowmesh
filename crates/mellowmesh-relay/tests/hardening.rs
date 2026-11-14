//! Relay hardening: registration validation, persisted bindings, body
//! handling, link-drop cleanup and slow-subscriber isolation.

use futures_util::{SinkExt, StreamExt};
use mellowmesh_core::relay::RelayFrame;
use mellowmesh_relay::{create_router, RelayState};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message as WsMessage;

async fn start_relay_with(port: u16, state: RelayState) {
    let app = create_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// Connect a link and register; returns the reply frame and the socket.
async fn register(
    port: u16,
    hub_id: &str,
    link_key: &str,
) -> (
    RelayFrame,
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/link"))
        .await
        .unwrap();
    ws.send(WsMessage::Text(
        serde_json::to_string(&RelayFrame::Register {
            hub_id: hub_id.to_string(),
            link_key: link_key.to_string(),
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    let reply = ws.next().await.unwrap().unwrap();
    let reply: RelayFrame = serde_json::from_str(reply.to_text().unwrap()).unwrap();
    (reply, ws)
}

#[tokio::test]
async fn test_invalid_hub_id_rejected() {
    let port = 40024;
    start_relay_with(port, RelayState::default()).await;

    let long_id = "a".repeat(65);
    for bad in ["", "bad id", "../etc", "hub/1", long_id.as_str()] {
        let (reply, _ws) = register(port, bad, "key").await;
        assert!(
            matches!(reply, RelayFrame::Error { .. }),
            "hub id {bad:?} must be rejected"
        );
    }

    let ok_id = "a".repeat(64);
    let (reply, _ws) = register(port, &ok_id, "key").await;
    assert!(matches!(reply, RelayFrame::Registered { .. }));
}

#[tokio::test]
async fn test_state_file_persists_binding_across_restarts() {
    let dir = std::env::temp_dir().join(format!(
        "mellowmesh-relay-it-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("relay-state.json");

    // First relay instance binds the hub id.
    let first_port = 40025;
    start_relay_with(
        first_port,
        RelayState::with_state_file(path.clone()).unwrap(),
    )
    .await;
    let (reply, ws) = register(first_port, "hubP", "goodkey").await;
    assert!(matches!(reply, RelayFrame::Registered { .. }));
    drop(ws);
    assert!(path.exists(), "state file must be written");

    // A fresh relay instance loading the same file keeps the binding.
    let second_port = 40026;
    let state = RelayState::with_state_file(path.clone()).unwrap();
    assert_eq!(state.known_hub_count(), 1);
    start_relay_with(second_port, state).await;
    let (reply, _ws) = register(second_port, "hubP", "other-key").await;
    assert!(
        matches!(reply, RelayFrame::Error { .. }),
        "a different key must be rejected after a restart"
    );
    let (reply, _ws) = register(second_port, "hubP", "goodkey").await;
    assert!(matches!(reply, RelayFrame::Registered { .. }));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn test_non_utf8_body_rejected_with_415() {
    let port = 40027;
    start_relay_with(port, RelayState::default()).await;
    let (reply, _ws) = register(port, "hubB", "key").await;
    assert!(matches!(reply, RelayFrame::Registered { .. }));

    let resp = reqwest::Client::new()
        .post(format!("http://127.0.0.1:{port}/hub/hubB/upload"))
        .body(vec![0xff, 0xfe, 0x00, 0x80])
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 415);
}

#[tokio::test]
async fn test_link_drop_fails_pending_requests_fast() {
    let port = 40028;
    start_relay_with(port, RelayState::default()).await;
    let (reply, mut ws) = register(port, "hubD", "key").await;
    assert!(matches!(reply, RelayFrame::Registered { .. }));

    // The request reaches the hub, which never answers and then drops.
    let request = tokio::spawn(async move {
        reqwest::Client::new()
            .get(format!("http://127.0.0.1:{port}/hub/hubD/tasks"))
            .send()
            .await
            .unwrap()
            .status()
    });
    let forwarded = ws.next().await.unwrap().unwrap();
    assert!(forwarded.to_text().unwrap().contains("\"request\""));
    drop(ws);

    let status = tokio::time::timeout(Duration::from_secs(5), request)
        .await
        .expect("pending request must fail promptly when the link drops")
        .unwrap();
    assert_eq!(status, 502);

    // The stale hub entry is gone: new requests fail fast too.
    let resp = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/hub/hubD/tasks"))
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 502);
}

#[tokio::test]
async fn test_slow_subscriber_does_not_stall_link() {
    let port = 40029;
    start_relay_with(port, RelayState::default()).await;
    let (reply, ws) = register(port, "hubS", "key").await;
    assert!(matches!(reply, RelayFrame::Registered { .. }));
    let (mut link_tx, mut link_rx) = ws.split();

    // Single writer for the fake daemon's side of the link.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::channel::<RelayFrame>(64);
    tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            if link_tx
                .send(WsMessage::Text(serde_json::to_string(&frame).unwrap()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Fake daemon: flood any opened stream until told to close it; answer
    // forwarded requests immediately.
    let closed = Arc::new(AtomicBool::new(false));
    let (closed_tx, closed_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let closed = closed.clone();
        let out_tx = out_tx.clone();
        tokio::spawn(async move {
            let mut closed_tx = Some(closed_tx);
            while let Some(Ok(msg)) = link_rx.next().await {
                let WsMessage::Text(text) = msg else { continue };
                match serde_json::from_str::<RelayFrame>(&text) {
                    Ok(RelayFrame::StreamOpen { stream_id, .. }) => {
                        let out_tx = out_tx.clone();
                        let closed = closed.clone();
                        tokio::spawn(async move {
                            let chunk = "x".repeat(16 * 1024);
                            for _ in 0..4000 {
                                if closed.load(Ordering::SeqCst) {
                                    break;
                                }
                                if out_tx
                                    .send(RelayFrame::StreamData {
                                        stream_id: stream_id.clone(),
                                        text: chunk.clone(),
                                    })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        });
                    }
                    Ok(RelayFrame::StreamClose { .. }) => {
                        closed.store(true, Ordering::SeqCst);
                        if let Some(tx) = closed_tx.take() {
                            let _ = tx.send(());
                        }
                    }
                    Ok(RelayFrame::Request { id, .. }) => {
                        let _ = out_tx
                            .send(RelayFrame::Response {
                                id,
                                status: 200,
                                content_type: Some("text/plain".to_string()),
                                body: Some("pong".to_string()),
                            })
                            .await;
                    }
                    _ => {}
                }
            }
        });
    }

    // A subscriber that connects and then never reads.
    let (_sub_ws, _) =
        tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/hub/hubS/ws?pattern=x"))
            .await
            .unwrap();

    // The relay gives up on the slow subscriber and tells the daemon.
    tokio::time::timeout(Duration::from_secs(20), closed_rx)
        .await
        .expect("relay must close a stream whose subscriber stopped reading")
        .unwrap();

    // Ordinary forwarded requests for the same hub still go through.
    let resp = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/hub/hubS/ping"))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "pong");
}
