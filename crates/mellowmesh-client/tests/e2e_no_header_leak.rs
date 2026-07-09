//! Regression test: the E2E envelope POST must NOT carry the bearer token
//! as an `Authorization` header — the token travels sealed inside the
//! ciphertext, so a relay in the middle can never read it.

use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::e2e::{derive_key, open, seal, Envelope, SealedResponse};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Captured {
    auth_header: Arc<Mutex<Option<String>>>,
    token: String,
}

async fn e2e_handler(
    State(cap): State<Captured>,
    headers: HeaderMap,
    Json(envelope): Json<Envelope>,
) -> Json<Envelope> {
    // Record whatever Authorization header (if any) reached this "relay".
    *cap.auth_header.lock().unwrap() = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Prove the request really was sealed under the token, and reply sealed.
    let key = derive_key(&cap.token);
    open(&key, &envelope).expect("request should decrypt under the token key");
    let resp = SealedResponse {
        status: 200,
        content_type: Some("application/json".to_string()),
        body: Some("[]".to_string()),
    };
    let reply = seal(&key, &envelope.key_id, &serde_json::to_vec(&resp).unwrap()).unwrap();
    Json(reply)
}

#[tokio::test]
async fn e2e_envelope_carries_no_authorization_header() {
    let token = "mm_leak_check_token".to_string();
    let captured = Captured {
        auth_header: Arc::new(Mutex::new(None)),
        token: token.clone(),
    };

    let app = Router::new()
        .route("/e2e/request", post(e2e_handler))
        .with_state(captured.clone());
    let addr = SocketAddr::from(([127, 0, 0, 1], 40031));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let client = MellowMeshClient::new(40031)
        .with_base_url("http://127.0.0.1:40031")
        .with_token(token);

    let (status, body) = client
        .e2e_request("GET", "/decisions", None)
        .await
        .expect("e2e request should succeed");
    assert_eq!(status, 200);
    assert_eq!(body, "[]");

    // The whole point: the token never appeared as a header on the wire.
    assert_eq!(
        *captured.auth_header.lock().unwrap(),
        None,
        "E2E envelope POST leaked an Authorization header to the relay"
    );
}

#[tokio::test]
async fn transparent_e2e_routes_sdk_methods_through_sealed_envelopes() {
    let token = "mm_transparent_mode_token".to_string();
    let captured = Captured {
        auth_header: Arc::new(Mutex::new(None)),
        token: token.clone(),
    };

    let app = Router::new()
        .route("/e2e/request", post(e2e_handler))
        .with_state(captured.clone());
    let addr = SocketAddr::from(([127, 0, 0, 1], 40032));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // With transparent E2E on, an ordinary SDK call must arrive at the
    // /e2e/request endpoint as a sealed envelope with no auth header —
    // the server above ONLY serves /e2e/request, so reaching it at all
    // proves the call did not go out as plain HTTP to /decisions.
    let client = MellowMeshClient::new(40032)
        .with_base_url("http://127.0.0.1:40032")
        .with_token(token)
        .with_e2e(true);

    let decisions = client
        .list_decisions()
        .await
        .expect("list_decisions should tunnel through the sealed transport");
    assert!(decisions.is_empty()); // the capture server replies sealed "[]"

    assert_eq!(
        *captured.auth_header.lock().unwrap(),
        None,
        "Transparent E2E leaked an Authorization header to the relay"
    );
}
