//! Transport behavior of the shared HTTP client and the WebSocket stream.

use axum::{http::HeaderMap, routing::get, Json, Router};
use futures_util::{SinkExt, StreamExt};
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;
use std::sync::{Arc, Mutex};
use tokio_tungstenite::tungstenite::Message as WsMessage;

#[tokio::test]
async fn plain_requests_carry_the_bearer_token_per_request() {
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    let seen_handler = seen.clone();
    let app = Router::new().route(
        "/tasks",
        get(move |headers: HeaderMap| {
            let seen = seen_handler.clone();
            async move {
                seen.lock().unwrap().push(
                    headers
                        .get(axum::http::header::AUTHORIZATION)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string),
                );
                Json(serde_json::json!([]))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Two clients share one HTTP connection pool but must each send their
    // own credentials (or none).
    let with_token = MellowMeshClient::loopback(1)
        .with_base_url(base.clone())
        .with_token("tok_a");
    let anonymous = MellowMeshClient::loopback(1).with_base_url(base.clone());
    with_token.list_tasks().await.unwrap();
    anonymous.list_tasks().await.unwrap();

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen, vec![Some("Bearer tok_a".to_string()), None]);
}

#[tokio::test]
async fn crafted_ids_are_rejected_or_stay_in_their_route() {
    let paths: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let paths_handler = paths.clone();
    let app = Router::new().fallback(move |uri: axum::http::Uri| {
        let paths = paths_handler.clone();
        async move {
            paths.lock().unwrap().push(uri.to_string());
            "{}"
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = MellowMeshClient::loopback(1).with_base_url(base);
    client.complete_task("../shutdown?").await.unwrap();
    client.revoke_token("../../shutdown").await.unwrap();
    assert_eq!(
        *paths.lock().unwrap(),
        vec![
            "/tasks/..%2Fshutdown%3F/complete".to_string(),
            "/auth/tokens/..%2F..%2Fshutdown".to_string(),
        ]
    );
    assert!(client.complete_task("..").await.is_err());
    assert!(client.revoke_token("").await.is_err());
    assert!(client.get_wiki_page("default", "../x").await.is_err());
}

fn sample_message() -> Message {
    Message {
        id: "msg_1".to_string(),
        topic: "_forum.general".to_string(),
        from: "agent://a/b".to_string(),
        owner: None,
        timestamp: chrono::Utc::now(),
        content_type: "text/plain".to_string(),
        body: "hello".to_string(),
        headers: None,
        payload: None,
        parent_id: None,
    }
}

#[tokio::test]
async fn ws_stream_skips_control_frames_and_ends_on_close() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let msg_json = serde_json::to_string(&sample_message()).unwrap();
    tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        ws.send(WsMessage::Ping(b"keepalive".to_vec()))
            .await
            .unwrap();
        ws.send(WsMessage::Pong(Vec::new())).await.unwrap();
        ws.send(WsMessage::Binary(vec![1, 2, 3])).await.unwrap();
        ws.send(WsMessage::Text(msg_json)).await.unwrap();
        ws.send(WsMessage::Close(None)).await.unwrap();
        // Drain until the client acknowledges the close.
        while let Some(Ok(_)) = ws.next().await {}
    });

    let client = MellowMeshClient::loopback(1).with_base_url(base);
    let stream = client.subscribe("_forum.**").await.unwrap();
    let items: Vec<_> = tokio::time::timeout(std::time::Duration::from_secs(5), stream.collect())
        .await
        .expect("stream should end after the Close frame");
    assert_eq!(items.len(), 1, "only the text frame is a message");
    let m = items.into_iter().next().unwrap().unwrap();
    assert_eq!(m.body, "hello");
}
