//! Stdio JSON-RPC loop for `mellowmesh mcp`. The tool schema and dispatch
//! live in `mellowmesh_client::mcp`, shared with the daemon's HTTP endpoint.

use mellowmesh_client::mcp::{handle_tool_call, list_tools_schema};
use mellowmesh_client::MellowMeshClient;
use tokio::io::AsyncBufReadExt;

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

        // Parse JSON-RPC request
        let req: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err_res = make_error_response(
                    serde_json::Value::Null,
                    -32700,
                    &format!("Parse error: {e}"),
                );
                let _ = write_response(&mut stdout, &err_res).await;
                continue;
            }
        };

        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let is_notification = id.is_null();

        match method {
            "initialize" => {
                let res = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {
                            "tools": {}
                        },
                        "serverInfo": {
                            "name": "mellowmesh-mcp",
                            "version": "0.1.0"
                        }
                    }
                });
                let _ = write_response(&mut stdout, &res).await;
            }
            "notifications/initialized" => {
                // MCP Handshake Complete
            }
            "tools/list" => {
                let tools = list_tools_schema();
                let res = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "tools": tools
                    }
                });
                let _ = write_response(&mut stdout, &res).await;
            }
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

                let result = handle_tool_call(&client, tool_name, args).await;
                match result {
                    Ok(content_blocks) => {
                        let res = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": content_blocks,
                                "isError": false
                            }
                        });
                        let _ = write_response(&mut stdout, &res).await;
                    }
                    Err(e) => {
                        let res = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": format!("Error executing tool: {}", e)
                                    }
                                ],
                                "isError": true
                            }
                        });
                        let _ = write_response(&mut stdout, &res).await;
                    }
                }
            }
            _ => {
                if !is_notification {
                    let err_res =
                        make_error_response(id, -32601, &format!("Method not found: {method}"));
                    let _ = write_response(&mut stdout, &err_res).await;
                }
            }
        }
    }

    Ok(())
}

fn make_error_response(id: serde_json::Value, code: i32, message: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

async fn write_response(
    stdout: &mut tokio::io::Stdout,
    val: &serde_json::Value,
) -> anyhow::Result<()> {
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

    #[test]
    fn test_make_error_response() {
        let err = make_error_response(serde_json::json!(123), -32601, "Method not found");
        assert_eq!(err["jsonrpc"], "2.0");
        assert_eq!(err["id"], 123);
        assert_eq!(err["error"]["code"], -32601);
        assert_eq!(err["error"]["message"], "Method not found");
    }
}
