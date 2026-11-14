//! Stdio JSON-RPC loop for `mellowmesh mcp`. The tool schema and dispatch
//! live in `mellowmesh_client::mcp`, shared with the daemon's HTTP endpoint.

use mellowmesh_client::mcp::{
    handle_tool_call, list_tools_schema, negotiate_protocol_version, SERVER_INSTRUCTIONS,
};
use mellowmesh_client::MellowMeshClient;
use serde_json::Value;
use tokio::io::AsyncBufReadExt;

const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;

/// What to do with one incoming line.
#[derive(Debug, PartialEq)]
enum Dispatch {
    /// Write this response.
    Reply(Value),
    /// A notification (or an invalid one): never answered.
    NoReply,
    /// A valid `tools/call` request: run the tool, then answer `id`.
    CallTool {
        id: Value,
        name: String,
        args: Value,
    },
}

/// Decode one line of input. Pure: everything except running a tool is
/// decided here.
fn dispatch_line(line: &str) -> Dispatch {
    match serde_json::from_str::<Value>(line) {
        Ok(req) => dispatch(&req),
        Err(e) => Dispatch::Reply(make_error_response(
            Value::Null,
            PARSE_ERROR,
            &format!("Parse error: {e}"),
        )),
    }
}

/// Decide the response to one parsed JSON-RPC message.
fn dispatch(req: &Value) -> Dispatch {
    if req.is_array() {
        return Dispatch::Reply(make_error_response(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request: batch requests are not supported",
        ));
    }
    let Some(obj) = req.as_object() else {
        return Dispatch::Reply(make_error_response(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request: expected a JSON object",
        ));
    };

    // A message without an `id` member is a notification: never answered,
    // whatever it contains (including errors).
    let Some(id) = obj.get("id").cloned() else {
        return Dispatch::NoReply;
    };
    if !(id.is_string() || id.is_number() || id.is_null()) {
        return Dispatch::Reply(make_error_response(
            Value::Null,
            INVALID_REQUEST,
            "Invalid Request: id must be a string or number",
        ));
    }
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Dispatch::Reply(make_error_response(
            id,
            INVALID_REQUEST,
            "Invalid Request: jsonrpc must be \"2.0\"",
        ));
    }
    let Some(method) = obj.get("method").and_then(Value::as_str) else {
        return Dispatch::Reply(make_error_response(
            id,
            INVALID_REQUEST,
            "Invalid Request: missing method",
        ));
    };
    let params = obj.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            Dispatch::Reply(make_result_response(
                id,
                serde_json::json!({
                    "protocolVersion": negotiate_protocol_version(requested),
                    "capabilities": { "tools": {} },
                    "serverInfo": {
                        "name": "mellowmesh-mcp",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": SERVER_INSTRUCTIONS
                }),
            ))
        }
        "ping" => Dispatch::Reply(make_result_response(id, serde_json::json!({}))),
        "tools/list" => Dispatch::Reply(make_result_response(
            id,
            serde_json::json!({ "tools": list_tools_schema() }),
        )),
        "tools/call" => {
            let name = match params.get("name").and_then(Value::as_str) {
                Some(n) if !n.is_empty() => n.to_string(),
                _ => {
                    return Dispatch::Reply(make_error_response(
                        id,
                        INVALID_PARAMS,
                        "Invalid params: missing tool name (params.name)",
                    ))
                }
            };
            let args = match params.get("arguments") {
                None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
                Some(a @ Value::Object(_)) => a.clone(),
                Some(_) => {
                    return Dispatch::Reply(make_error_response(
                        id,
                        INVALID_PARAMS,
                        "Invalid params: arguments must be an object",
                    ))
                }
            };
            Dispatch::CallTool { id, name, args }
        }
        _ => Dispatch::Reply(make_error_response(
            id,
            METHOD_NOT_FOUND,
            &format!("Method not found: {method}"),
        )),
    }
}

pub async fn run_mcp_server(port: u16) -> anyhow::Result<()> {
    let client = MellowMeshClient::connect_with_port(port).await?;
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            break; // EOF
        }

        if line.trim().is_empty() {
            continue;
        }

        let response = match dispatch_line(&line) {
            Dispatch::Reply(res) => res,
            Dispatch::NoReply => continue,
            Dispatch::CallTool { id, name, args } => {
                match handle_tool_call(&client, &name, args).await {
                    Ok(content_blocks) => make_result_response(
                        id,
                        serde_json::json!({ "content": content_blocks, "isError": false }),
                    ),
                    Err(e) => make_result_response(
                        id,
                        serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": mellowmesh_client::sanitize::sanitize_text(
                                    &format!("Error executing tool: {e}")
                                )
                            }],
                            "isError": true
                        }),
                    ),
                }
            }
        };
        let _ = write_response(&mut stdout, &response).await;
    }

    Ok(())
}

fn make_result_response(id: Value, result: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn make_error_response(id: Value, code: i32, message: &str) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

async fn write_response(stdout: &mut tokio::io::Stdout, val: &Value) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;
    let s = serde_json::to_string(val)?;
    stdout.write_all(s.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(line: &str) -> Value {
        match dispatch_line(line) {
            Dispatch::Reply(v) => v,
            other => panic!("expected a reply, got {other:?}"),
        }
    }

    #[test]
    fn test_make_error_response() {
        let err = make_error_response(json!(123), -32601, "Method not found");
        assert_eq!(err["jsonrpc"], "2.0");
        assert_eq!(err["id"], 123);
        assert_eq!(err["error"]["code"], -32601);
        assert_eq!(err["error"]["message"], "Method not found");
    }

    #[test]
    fn ping_returns_empty_result() {
        let res = reply(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#);
        assert_eq!(res["id"], 7);
        assert_eq!(res["result"], json!({}));
        assert!(res.get("error").is_none());
    }

    #[test]
    fn notifications_are_never_answered() {
        for line in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
            r#"{"jsonrpc":"2.0","method":"no/such/method"}"#,
            r#"{"jsonrpc":"2.0","method":"tools/call","params":{}}"#,
            r#"{"jsonrpc":"1.0","method":"ping"}"#,
        ] {
            assert_eq!(dispatch_line(line), Dispatch::NoReply, "{line}");
        }
    }

    #[test]
    fn batches_are_rejected() {
        let res = reply(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#);
        assert_eq!(res["id"], Value::Null);
        assert_eq!(res["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let res = reply(r#"{"jsonrpc":"2.0","id":1,"method":"#);
        assert_eq!(res["id"], Value::Null);
        assert_eq!(res["error"]["code"], PARSE_ERROR);
    }

    #[test]
    fn wrong_jsonrpc_version_is_invalid() {
        let res = reply(r#"{"jsonrpc":"1.0","id":3,"method":"ping"}"#);
        assert_eq!(res["id"], 3);
        assert_eq!(res["error"]["code"], INVALID_REQUEST);
        let res = reply(r#"{"id":4,"method":"ping"}"#);
        assert_eq!(res["error"]["code"], INVALID_REQUEST);
        let res = reply(r#"{"jsonrpc":"2.0","id":{"x":1},"method":"ping"}"#);
        assert_eq!(res["id"], Value::Null);
        assert_eq!(res["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn initialize_negotiates_protocol_version() {
        let res = reply(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        );
        assert_eq!(res["result"]["protocolVersion"], "2025-03-26");
        assert!(res["result"]["instructions"]
            .as_str()
            .unwrap()
            .contains("untrusted"));

        let res = reply(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        );
        assert_eq!(res["result"]["protocolVersion"], "2024-11-05");

        let res = reply(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2099-01-01"}}"#,
        );
        assert_eq!(res["result"]["protocolVersion"], "2025-06-18");

        let res = reply(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
        assert_eq!(res["result"]["protocolVersion"], "2025-06-18");
    }

    #[test]
    fn tools_list_has_no_respond_decision() {
        let res = reply(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#);
        let tools = res["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 27);
        assert!(tools.iter().all(|t| t["name"] != "respond_decision"));
    }

    #[test]
    fn tools_call_requires_a_name() {
        for line in [
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call"}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":""}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":3}}"#,
        ] {
            let res = reply(line);
            assert_eq!(res["id"], 5, "{line}");
            assert_eq!(res["error"]["code"], INVALID_PARAMS, "{line}");
        }
        let res = reply(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"list_tasks","arguments":[1]}}"#,
        );
        assert_eq!(res["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn tools_call_dispatches_to_the_tool() {
        assert_eq!(
            dispatch_line(
                r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"list_tasks"}}"#
            ),
            Dispatch::CallTool {
                id: json!(9),
                name: "list_tasks".to_string(),
                args: json!({}),
            }
        );
    }

    #[test]
    fn unknown_methods_are_not_found() {
        let res = reply(r#"{"jsonrpc":"2.0","id":2,"method":"resources/list"}"#);
        assert_eq!(res["error"]["code"], METHOD_NOT_FOUND);
    }
}
