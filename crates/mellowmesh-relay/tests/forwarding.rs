//! End-to-end relay test: a fake hub links up over WebSocket and answers
//! forwarded HTTP requests.

use futures_util::{SinkExt, StreamExt};
use mellowmesh_core::relay::RelayFrame;
use mellowmesh_relay::{create_router, RelayState};
use std::net::SocketAddr;
use tokio_tungstenite::tungstenite::Message as WsMessage;

async fn start_relay(port: u16) {
    let app = create_router(RelayState::default());
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_forwarding_roundtrip_and_hub_security() {
    let port = 40021;
    start_relay(port).await;
    let http = reqwest::Client::new();

    // Requests for an unlinked hub fail fast with 502.
    let resp = http
        .get(format!("http://127.0.0.1:{port}/hub/nohub/tasks"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 502);

    // Link a fake hub that echoes request details back as JSON.
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/link"))
        .await
        .unwrap();
    let (mut tx, mut rx) = ws.split();
    tx.send(WsMessage::Text(
        serde_json::to_string(&RelayFrame::Register {
            hub_id: "hub1".to_string(),
            link_key: "key1".to_string(),
        })
        .unwrap(),
    ))
    .await
    .unwrap();

    // Registration ack
    let ack = rx.next().await.unwrap().unwrap();
    let ack: RelayFrame = serde_json::from_str(ack.to_text().unwrap()).unwrap();
    assert!(matches!(ack, RelayFrame::Registered { .. }));

    // Fake daemon: answer every forwarded request with an echo.
    tokio::spawn(async move {
        while let Some(Ok(msg)) = rx.next().await {
            if let WsMessage::Text(text) = msg {
                if let Ok(RelayFrame::Request {
                    id,
                    method,
                    path,
                    authorization,
                    body,
                    ..
                }) = serde_json::from_str::<RelayFrame>(&text)
                {
                    let echo = serde_json::json!({
                        "method": method,
                        "path": path,
                        "authorization": authorization,
                        "body": body,
                    });
                    let frame = RelayFrame::Response {
                        id,
                        status: 200,
                        content_type: Some("application/json".to_string()),
                        body: Some(echo.to_string()),
                    };
                    tx.send(WsMessage::Text(serde_json::to_string(&frame).unwrap()))
                        .await
                        .unwrap();
                }
            }
        }
    });

    // Forward a POST through the relay and verify the hub saw everything.
    let resp = http
        .post(format!(
            "http://127.0.0.1:{port}/hub/hub1/decisions/dec_1/respond"
        ))
        .bearer_auth("mm_token123")
        .json(&serde_json::json!({ "option_id": "yes" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let echo: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(echo["method"], "POST");
    assert_eq!(echo["path"], "/decisions/dec_1/respond");
    assert_eq!(echo["authorization"], "Bearer mm_token123");
    assert!(echo["body"].as_str().unwrap().contains("option_id"));

    // A second daemon with the wrong link key cannot hijack the hub id.
    let (ws2, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/link"))
        .await
        .unwrap();
    let (mut tx2, mut rx2) = ws2.split();
    tx2.send(WsMessage::Text(
        serde_json::to_string(&RelayFrame::Register {
            hub_id: "hub1".to_string(),
            link_key: "wrong-key".to_string(),
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    let reply = rx2.next().await.unwrap().unwrap();
    let reply: RelayFrame = serde_json::from_str(reply.to_text().unwrap()).unwrap();
    assert!(matches!(reply, RelayFrame::Error { .. }));

    // Original hub still serves traffic after the hijack attempt.
    let resp = http
        .get(format!("http://127.0.0.1:{port}/hub/hub1/tasks"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn test_live_stream_through_relay() {
    let port = 40022;
    start_relay(port).await;

    // Link a fake hub that answers StreamOpen with two data frames.
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}/link"))
        .await
        .unwrap();
    let (mut tx, mut rx) = ws.split();
    tx.send(WsMessage::Text(
        serde_json::to_string(&RelayFrame::Register {
            hub_id: "hub2".to_string(),
            link_key: "key2".to_string(),
        })
        .unwrap(),
    ))
    .await
    .unwrap();
    let _ack = rx.next().await.unwrap().unwrap();

    let (seen_close_tx, seen_close_rx) = tokio::sync::oneshot::channel::<String>();
    tokio::spawn(async move {
        let mut close_tx = Some(seen_close_tx);
        while let Some(Ok(msg)) = rx.next().await {
            if let WsMessage::Text(text) = msg {
                match serde_json::from_str::<RelayFrame>(&text) {
                    Ok(RelayFrame::StreamOpen { stream_id, query }) => {
                        // Echo the query back, then a payload frame.
                        for body in [
                            format!("query:{}", query.unwrap_or_default()),
                            "payload".to_string(),
                        ] {
                            tx.send(WsMessage::Text(
                                serde_json::to_string(&RelayFrame::StreamData {
                                    stream_id: stream_id.clone(),
                                    text: body,
                                })
                                .unwrap(),
                            ))
                            .await
                            .unwrap();
                        }
                    }
                    Ok(RelayFrame::StreamClose { stream_id }) => {
                        if let Some(tx) = close_tx.take() {
                            let _ = tx.send(stream_id);
                        }
                    }
                    _ => {}
                }
            }
        }
    });

    // Remote subscriber connects through the relay with a pattern + token.
    let (sub_ws, _) = tokio_tungstenite::connect_async(format!(
        "ws://127.0.0.1:{port}/hub/hub2/ws?pattern=_task.%2A%2A&token=mm_sub"
    ))
    .await
    .unwrap();
    let (mut sub_tx, mut sub_rx) = sub_ws.split();

    let first = sub_rx.next().await.unwrap().unwrap();
    let first = first.to_text().unwrap();
    assert!(first.contains("pattern="), "query passthrough: {first}");
    assert!(first.contains("token=mm_sub"), "token passthrough: {first}");
    let second = sub_rx.next().await.unwrap().unwrap();
    assert_eq!(second.to_text().unwrap(), "payload");

    // Closing the remote subscription tells the hub to tear down.
    sub_tx.send(WsMessage::Close(None)).await.unwrap();
    let closed_stream = tokio::time::timeout(tokio::time::Duration::from_secs(3), seen_close_rx)
        .await
        .expect("hub should receive StreamClose")
        .unwrap();
    assert!(closed_stream.starts_with("str_"));
}
