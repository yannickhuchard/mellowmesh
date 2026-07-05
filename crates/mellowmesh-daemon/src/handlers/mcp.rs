//! Streamable HTTP MCP endpoint (`POST /mcp`).
//!
//! Lets remote MCP clients (Claude Mobile/web connectors, or anything
//! speaking the Streamable HTTP transport) join the fabric — including
//! through the relay at `https://<relay>/hub/<id>/mcp`. Each JSON-RPC
//! message is answered with a single JSON response (stateless mode).
//!
//! Tool calls are dispatched through the shared `mellowmesh_client::mcp`
//! module against the daemon's own loopback API, carrying the caller's
//! bearer token — so scopes and decision integrity apply to remote MCP
//! exactly as they do everywhere else.

use crate::state::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use mellowmesh_client::mcp::{handle_tool_call, list_tools_schema};
use mellowmesh_client::MellowMeshClient;

pub async fn handle_mcp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);

    // Notifications get acknowledged without a body.
    if id.is_null() && method.starts_with("notifications/") {
        return StatusCode::ACCEPTED.into_response();
    }

    let response = match method {
        "initialize" => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "mellowmesh-mcp", "version": "0.1.0" }
            }
        }),
        "tools/list" => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": list_tools_schema() }
        }),
        "tools/call" => {
            let params = req
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

            // Dispatch against our own API with the caller's token, so the
            // caller's scopes bound what the tools can do.
            let mut client = MellowMeshClient::new(state.port);
            if let Some(token) = headers
                .get(axum::http::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
            {
                client = client.with_token(token.trim());
            }

            match handle_tool_call(&client, tool_name, args).await {
                Ok(content) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": content, "isError": false }
                }),
                Err(e) => serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": format!("Error executing tool: {e}") }],
                        "isError": true
                    }
                }),
            }
        }
        _ => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") }
        }),
    };

    Json(response).into_response()
}
