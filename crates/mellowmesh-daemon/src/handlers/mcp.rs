//! Streamable HTTP MCP endpoint (`POST /mcp`).
//!
//! Lets remote MCP clients (Claude Mobile/web connectors, or anything
//! speaking the Streamable HTTP transport) join the fabric — including
//! through the relay at `https://<relay>/hub/<id>/mcp`. Each JSON-RPC
//! message is answered with a single JSON response (stateless mode).
//! Notifications (messages without an `id`) are acknowledged with `202` and
//! no body; batches are not supported.
//!
//! Tool calls are dispatched through the shared `mellowmesh_client::mcp`
//! module against the daemon's own loopback API, carrying the caller's
//! bearer token — so scopes and decision integrity apply to remote MCP
//! exactly as they do everywhere else.

use crate::state::AppState;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    Json,
};
use mellowmesh_client::mcp::{handle_tool_call, list_tools_schema};
use serde_json::{json, Value};

/// Protocol versions this server speaks, newest first.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

fn rpc_error(id: Value, code: i64, message: impl Into<String>) -> Response {
    Json(json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into() }
    }))
    .into_response()
}

fn rpc_result(id: Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

/// Echo the client's requested protocol version when supported, otherwise
/// answer with the newest version this server speaks.
pub fn negotiate_protocol_version(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|r| SUPPORTED_PROTOCOL_VERSIONS.iter().find(|v| **v == r))
        .copied()
        .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0])
}

/// The caller's bearer token, from the Authorization header only. A token
/// in the URL would end up in proxy and access logs; Streamable HTTP MCP
/// clients POST and can always set headers.
fn caller_token(headers: &HeaderMap, _uri: &Uri) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
}

pub async fn handle_mcp(
    State(state): State<AppState>,
    headers: HeaderMap,
    uri: Uri,
    body: Bytes,
) -> Response {
    let req: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return rpc_error(Value::Null, PARSE_ERROR, format!("Parse error: {e}")),
    };
    if req.is_array() {
        return rpc_error(
            Value::Null,
            INVALID_REQUEST,
            "Batch requests are not supported",
        );
    }
    let Some(obj) = req.as_object() else {
        return rpc_error(
            Value::Null,
            INVALID_REQUEST,
            "Request must be a JSON object",
        );
    };

    // A message without an id is a notification: never answered.
    let Some(id) = obj.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    if !(id.is_string() || id.is_number()) {
        return rpc_error(
            Value::Null,
            INVALID_REQUEST,
            "id must be a string or number",
        );
    }
    if obj.get("jsonrpc").and_then(|v| v.as_str()) != Some("2.0") {
        return rpc_error(id, INVALID_REQUEST, "jsonrpc must be \"2.0\"");
    }
    let Some(method) = obj.get("method").and_then(|m| m.as_str()) else {
        return rpc_error(id, INVALID_REQUEST, "method is required");
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(|v| v.as_str());
            rpc_result(
                id,
                json!({
                    "protocolVersion": negotiate_protocol_version(requested),
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "mellowmesh-mcp", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": list_tools_schema() })),
        "tools/call" => {
            let Some(tool_name) = params.get("name").and_then(|n| n.as_str()) else {
                return rpc_error(id, INVALID_PARAMS, "params.name is required");
            };
            let args = match params.get("arguments") {
                None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
                Some(a @ Value::Object(_)) => a.clone(),
                Some(_) => {
                    return rpc_error(id, INVALID_PARAMS, "params.arguments must be an object")
                }
            };

            // Dispatch against our own loopback API with the caller's token,
            // so the caller's scopes bound what the tools can do. Loopback
            // (not `new`) so a MELLOWMESH_URL in the daemon's env can't bounce
            // this back out through the relay. Clients are reused per caller.
            let token = caller_token(&headers, &uri);
            let client = state.caches.mcp_client(state.port, token.as_deref());

            match handle_tool_call(&client, tool_name, args).await {
                Ok(content) => rpc_result(id, json!({ "content": content, "isError": false })),
                Err(e) => rpc_result(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": format!("Error executing tool: {e}") }],
                        "isError": true
                    }),
                ),
            }
        }
        _ => rpc_error(id, METHOD_NOT_FOUND, format!("Method not found: {method}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_negotiation() {
        assert_eq!(negotiate_protocol_version(Some("2025-03-26")), "2025-03-26");
        assert_eq!(negotiate_protocol_version(Some("2024-11-05")), "2024-11-05");
        assert_eq!(negotiate_protocol_version(Some("1999-01-01")), "2025-06-18");
        assert_eq!(negotiate_protocol_version(None), "2025-06-18");
    }
}
