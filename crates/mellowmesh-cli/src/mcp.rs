use chrono::Utc;
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;
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

fn list_tools_schema() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "publish_message",
            "description": "Publish a message to a topic on the MellowMesh fabric. Dot-separated topic hierarchy, e.g. `_forum.general` or `_project.auth`. Supports @Name (or @[Name with spaces]) mentions, which are parsed and routed to agent inboxes automatically.",

            "inputSchema": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Dot-separated topic path"
                    },
                    "body": {
                        "type": "string",
                        "description": "Message body content"
                    },
                    "from": {
                        "type": "string",
                        "description": "Sender identifier URI (e.g. agent://yannick/coder)"
                    },
                    "owner": {
                        "type": "string",
                        "description": "Owner human URI (e.g. human://yannick)"
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Optional parent message ULID for tracing lineage and causality"
                    }
                },
                "required": ["topic", "body"]
            }
        }),
        serde_json::json!({
            "name": "publish_progress",
            "description": "Publish a task progress update. Writes directly to `_task.<task_id>.progress` and renews the publishing agent's claim lease on that task (heartbeat).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task ULID identifier"
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "Agent URI publishing the progress (e.g. agent://yannick/coder)"
                    },
                    "percentage": {
                        "type": "integer",
                        "description": "Task completion percentage (0 to 100)"
                    },
                    "status_text": {
                        "type": "string",
                        "description": "Short status message detailing current progress"
                    }
                },
                "required": ["task_id", "agent_id", "status_text"]
            }
        }),
        serde_json::json!({
            "name": "publish_artifact",
            "description": "Publish a code modification, design artifact, document or output structure to `_artifact.<artifact_id>`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "artifact_id": {
                        "type": "string",
                        "description": "Desired artifact identifier (optional, auto-generated if omitted)"
                    },
                    "title": {
                        "type": "string",
                        "description": "Title of the artifact"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content of the artifact (usually markdown or code)"
                    },
                    "content_type": {
                        "type": "string",
                        "description": "Content MIME type (default: text/markdown)"
                    },
                    "task_id": {
                        "type": "string",
                        "description": "Associated task identifier (optional)"
                    },
                    "created_by": {
                        "type": "string",
                        "description": "Creator URI (e.g. agent://yannick/coder)"
                    }
                },
                "required": ["title", "content", "created_by"]
            }
        }),
        serde_json::json!({
            "name": "read_history",
            "description": "Read historical messages from a topic or pattern.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Topic name or pattern matching (e.g. `_project.**` or `_forum.*`)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum messages to return",
                        "default": 20
                    }
                },
                "required": ["topic"]
            }
        }),
        serde_json::json!({
            "name": "get_forum",
            "description": "Get chronological messages in a forum-grouped layout.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Target topic pattern (e.g. `_forum.**`)"
                    }
                }
            }
        }),
        serde_json::json!({
            "name": "search_messages",
            "description": "Perform full-text search across all message bodies.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search keyword"
                    }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "register_agent",
            "description": "Register an agent capability in MellowMesh.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Agent URI (e.g. agent://yannick/coder)"
                    },
                    "name": {
                        "type": "string",
                        "description": "Display name of agent"
                    },
                    "owner": {
                        "type": "string",
                        "description": "Owner human URI"
                    },
                    "mode": {
                        "type": "string",
                        "description": "Agent operating mode (e.g. autonomous, semi-autonomous)",
                        "default": "autonomous"
                    },
                    "capabilities": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of capability tags"
                    }
                },
                "required": ["id", "name", "owner", "capabilities"]
            }
        }),
        serde_json::json!({
            "name": "list_agents",
            "description": "List registered agents and capabilities.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "create_task",
            "description": "Create a new task on the MellowMesh fabric.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short task title"
                    },
                    "topics": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Task topics for routing"
                    },
                    "description": {
                        "type": "string",
                        "description": "Detailed description of the work needed"
                    },
                    "capabilities": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Required capabilities list"
                    },
                    "priority": {
                        "type": "string",
                        "description": "Priority: low, medium, high",
                        "default": "medium"
                    },
                    "created_by": {
                        "type": "string",
                        "description": "Creator identifier URI"
                    },
                    "parent_id": {
                        "type": "string",
                        "description": "Optional parent task or message identifier for lineage tracing"
                    }
                },
                "required": ["title", "topics", "capabilities"]
            }
        }),
        serde_json::json!({
            "name": "list_tasks",
            "description": "List all active tasks.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "claim_task",
            "description": "Claim a task for an agent. Claims carry a lease (default 600s) that is renewed every time the agent publishes progress; if the lease expires without a heartbeat the daemon releases the task back to open so other agents can pick it up.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task identifier"
                    },
                    "agent_id": {
                        "type": "string",
                        "description": "Agent URI claiming the task"
                    },
                    "lease_seconds": {
                        "type": "integer",
                        "description": "Claim lease duration in seconds (default 600). Choose a value comfortably longer than the gap between your progress updates."
                    }
                },
                "required": ["task_id", "agent_id"]
            }
        }),
        serde_json::json!({
            "name": "complete_task",
            "description": "Complete a task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": {
                        "type": "string",
                        "description": "Task identifier"
                    }
                },
                "required": ["task_id"]
            }
        }),
        serde_json::json!({
            "name": "create_decision",
            "description": "Create a proposed decision request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Decision title"
                    },
                    "question": {
                        "type": "string",
                        "description": "The decision question or choices to vote on"
                    },
                    "created_by": {
                        "type": "string",
                        "description": "Creator URI"
                    },
                    "decider": {
                        "type": "string",
                        "description": "Required decider (e.g. human://yannick)"
                    },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "A list of option labels to present"
                    }
                },
                "required": ["title", "question", "created_by", "decider", "options"]
            }
        }),
        serde_json::json!({
            "name": "list_decisions",
            "description": "List all proposed decisions.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "respond_decision",
            "description": "Respond/vote on a proposed decision request.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "decision_id": {
                        "type": "string",
                        "description": "Decision ID"
                    },
                    "option_id": {
                        "type": "string",
                        "description": "The chosen option ID"
                    }
                },
                "required": ["decision_id", "option_id"]
            }
        }),
        serde_json::json!({
            "name": "store_topic_summary",
            "description": "Store or update a topic's semantic summary.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Target topic name"
                    },
                    "summary": {
                        "type": "string",
                        "description": "Topic summary description"
                    }
                },
                "required": ["topic", "summary"]
            }
        }),
        serde_json::json!({
            "name": "get_context",
            "description": "Retrieve the coordination context (topic summary + history) for a topic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "Topic name or pattern"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum messages to retrieve",
                        "default": 20
                    }
                },
                "required": ["topic"]
            }
        }),
        serde_json::json!({
            "name": "enable_trace",
            "description": "Enable dynamic trace telemetry on a target.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target_type": {
                        "type": "string",
                        "description": "Target type, e.g. agent, task, topic, node"
                    },
                    "target": {
                        "type": "string",
                        "description": "Target identifier"
                    },
                    "level": {
                        "type": "string",
                        "description": "Trace level: status, progress, structured, verbose, cognitive, raw"
                    },
                    "duration": {
                        "type": "string",
                        "description": "Duration (e.g. 15m, 1h)"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Reasoning for enabling telemetry"
                    },
                    "enabled_by": {
                        "type": "string",
                        "description": "Issuer URI"
                    }
                },
                "required": ["target_type", "target", "level", "duration", "enabled_by"]
            }
        }),
        serde_json::json!({
            "name": "disable_trace",
            "description": "Disable dynamic trace session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "Session ID to disable"
                    }
                },
                "required": ["id"]
            }
        }),
        serde_json::json!({
            "name": "list_traces",
            "description": "List active dynamic trace sessions.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "get_metrics",
            "description": "View mellowmeshd daemon performance metrics.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        serde_json::json!({
            "name": "list_wiki_pages",
            "description": "List all pages in a wiki namespace.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "wiki": {
                        "type": "string",
                        "description": "Wiki namespace name (defaults to 'default')",
                        "default": "default"
                    },
                    "doc_type": {
                        "type": "string",
                        "description": "Optional type filter (e.g. procedure, runbook)"
                    },
                    "tag": {
                        "type": "string",
                        "description": "Optional tag filter"
                    }
                }
            }
        }),
        serde_json::json!({
            "name": "get_wiki_page",
            "description": "Get content and metadata of a specific OKF page.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative file path from wiki root (e.g., procedures/deploy.md)"
                    },
                    "wiki": {
                        "type": "string",
                        "description": "Wiki namespace name (defaults to 'default')",
                        "default": "default"
                    }
                },
                "required": ["path"]
            }
        }),
        serde_json::json!({
            "name": "write_wiki_page",
            "description": "Write or update an OKF markdown document in a wiki.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative file path from wiki root"
                    },
                    "doc_type": {
                        "type": "string",
                        "description": "YAML metadata: type of the document (e.g. procedure, schema)"
                    },
                    "title": {
                        "type": "string",
                        "description": "YAML metadata: title of the document"
                    },
                    "description": {
                        "type": "string",
                        "description": "YAML metadata: optional short description"
                    },
                    "tags": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "YAML metadata: list of tags"
                    },
                    "resource": {
                        "type": "string",
                        "description": "YAML metadata: optional linked resource"
                    },
                    "body": {
                        "type": "string",
                        "description": "Markdown body content"
                    },
                    "wiki": {
                        "type": "string",
                        "description": "Wiki namespace name (defaults to 'default')",
                        "default": "default"
                    }
                },
                "required": ["path", "doc_type", "title", "body"]
            }
        }),
        serde_json::json!({
            "name": "search_wiki",
            "description": "Search wiki pages using FTS full-text and tag/type filters.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query keywords"
                    },
                    "wiki": {
                        "type": "string",
                        "description": "Wiki namespace name (defaults to 'default')",
                        "default": "default"
                    },
                    "doc_type": {
                        "type": "string",
                        "description": "Optional type filter"
                    },
                    "tag": {
                        "type": "string",
                        "description": "Optional tag filter"
                    }
                },
                "required": ["query"]
            }
        }),
        serde_json::json!({
            "name": "register_named_topic",
            "description": "Register a human-friendly short name mapping to a topic path. The mapping will be distributed to peers.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Human-friendly short name (e.g. 'Mario Galaxy')"
                    },
                    "topic": {
                        "type": "string",
                        "description": "Dot-separated target topic path (e.g. '_forum.games.mario galaxy')"
                    }
                },
                "required": ["name", "topic"]
            }
        }),
        serde_json::json!({
            "name": "list_named_topics",
            "description": "List all registered named topic mappings.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        serde_json::json!({
            "name": "remove_named_topic",
            "description": "Remove a registered named topic mapping by name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the topic mapping to remove"
                    }
                },
                "required": ["name"]
            }
        }),
    ]
}

async fn handle_tool_call(
    client: &MellowMeshClient,
    name: &str,
    args: serde_json::Value,
) -> anyhow::Result<Vec<serde_json::Value>> {
    match name {
        "publish_message" => {
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing topic"))?;
            let body = args
                .get("body")
                .and_then(|b| b.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing body"))?;
            let from = args
                .get("from")
                .and_then(|f| f.as_str())
                .unwrap_or("agent://mcp");
            let owner = args
                .get("owner")
                .and_then(|o| o.as_str())
                .map(|s| s.to_string());
            let parent_id = args
                .get("parent_id")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string());

            let msg = Message {
                id: String::new(),
                topic: topic.to_string(),
                from: from.to_string(),
                owner,
                timestamp: Utc::now(),
                content_type: "text/plain".to_string(),
                body: body.to_string(),
                headers: None,
                payload: None,
                parent_id,
            };
            client.publish(&msg).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Message published successfully to topic: {}", topic)
            })])
        }
        "publish_progress" => {
            let task_id = args
                .get("task_id")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing task_id"))?;
            let agent_id = args
                .get("agent_id")
                .and_then(|a| a.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing agent_id"))?;
            let percentage = args.get("percentage").and_then(|p| p.as_i64());
            let status_text = args
                .get("status_text")
                .and_then(|s| s.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing status_text"))?;

            let payload = serde_json::json!({
                "task_id": task_id,
                "percentage": percentage,
                "status": status_text,
            });

            let msg = Message {
                id: String::new(),
                topic: format!("_task.{task_id}.progress"),
                from: agent_id.to_string(),
                owner: None,
                timestamp: Utc::now(),
                content_type: "application/json".to_string(),
                body: status_text.to_string(),
                headers: None,
                payload: Some(payload),
                parent_id: None,
            };
            client.publish(&msg).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Progress update published for task {}", task_id)
            })])
        }
        "publish_artifact" => {
            let artifact_id = args
                .get("artifact_id")
                .and_then(|a| a.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("art_{}", ulid::Ulid::new().to_string().to_lowercase()));
            let title = args
                .get("title")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing title"))?;
            let content = args
                .get("content")
                .and_then(|c| c.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing content"))?;
            let content_type = args
                .get("content_type")
                .and_then(|ct| ct.as_str())
                .unwrap_or("text/markdown");
            let task_id = args.get("task_id").and_then(|t| t.as_str());
            let created_by = args
                .get("created_by")
                .and_then(|c| c.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing created_by"))?;

            let mut headers = std::collections::HashMap::new();
            headers.insert("title".to_string(), title.to_string());
            if let Some(t_id) = task_id {
                headers.insert("task_id".to_string(), t_id.to_string());
            }

            let msg = Message {
                id: String::new(),
                topic: format!("_artifact.{artifact_id}"),
                from: created_by.to_string(),
                owner: None,
                timestamp: Utc::now(),
                content_type: content_type.to_string(),
                body: content.to_string(),
                headers: Some(headers),
                payload: None,
                parent_id: None,
            };
            client.publish(&msg).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Artifact published successfully with ID: {}", artifact_id)
            })])
        }
        "read_history" => {
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing topic"))?;
            let limit = args.get("limit").and_then(|l| l.as_u64()).unwrap_or(20) as usize;
            let history = client.get_history(100).await?;

            let mut filtered = Vec::new();
            for m in history {
                if mellowmesh_core::topic::match_topic(topic, &m.topic) {
                    filtered.push(m);
                    if filtered.len() >= limit {
                        break;
                    }
                }
            }
            let text = serde_json::to_string_pretty(&filtered)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "get_forum" => {
            let pattern = args
                .get("pattern")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string());
            let msgs = client.get_forum(pattern).await?;
            let text = serde_json::to_string_pretty(&msgs)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "search_messages" => {
            let query = args
                .get("query")
                .and_then(|q| q.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
            let msgs = client.search_messages(query).await?;
            let text = serde_json::to_string_pretty(&msgs)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "register_agent" => {
            let id = args
                .get("id")
                .and_then(|i| i.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
            let name = args
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
            let owner = args
                .get("owner")
                .and_then(|o| o.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing owner"))?;
            let mode = args
                .get("mode")
                .and_then(|m| m.as_str())
                .unwrap_or("autonomous");
            let capabilities = args
                .get("capabilities")
                .and_then(|c| c.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing capabilities"))?
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();

            let reg = mellowmesh_core::agent::AgentRegistration {
                id: id.to_string(),
                name: name.to_string(),
                owner: owner.to_string(),
                mode: mode.to_string(),
                capabilities,
            };
            client.register_agent(&reg).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Agent registered successfully: {}", id)
            })])
        }
        "list_agents" => {
            let agents = client.list_agents().await?;
            let text = serde_json::to_string_pretty(&agents)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "create_task" => {
            let title = args
                .get("title")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing title"))?;
            let topics = args
                .get("topics")
                .and_then(|t| t.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing topics"))?
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            let description = args
                .get("description")
                .and_then(|d| d.as_str())
                .map(|s| s.to_string());
            let capabilities = args
                .get("capabilities")
                .and_then(|c| c.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing capabilities"))?
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            let priority = args
                .get("priority")
                .and_then(|p| p.as_str())
                .unwrap_or("medium")
                .to_string();
            let created_by = args
                .get("created_by")
                .and_then(|c| c.as_str())
                .unwrap_or("agent://mcp")
                .to_string();
            let parent_id = args
                .get("parent_id")
                .and_then(|p| p.as_str())
                .map(|s| s.to_string());

            let task = mellowmesh_core::task::Task {
                id: format!("task_{}", ulid::Ulid::new().to_string().to_lowercase()),
                title: title.to_string(),
                description,
                created_from: None,
                created_by,
                status: "open".to_string(),
                priority,
                topics,
                required_capabilities: capabilities,
                assigned_to: None,
                claimed_by: None,
                deadline: None,
                artifacts: Vec::new(),
                decisions: Vec::new(),
                parent_id,
                lease_seconds: None,
                claim_expires_at: None,
            };
            client.create_task(&task).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Task created successfully with ID: {}", task.id)
            })])
        }
        "list_tasks" => {
            let tasks = client.list_tasks().await?;
            let text = serde_json::to_string_pretty(&tasks)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "claim_task" => {
            let task_id = args
                .get("task_id")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing task_id"))?;
            let agent_id = args
                .get("agent_id")
                .and_then(|a| a.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing agent_id"))?;
            let lease_seconds = args.get("lease_seconds").and_then(|l| l.as_u64());
            client
                .claim_task_with_lease(task_id, agent_id, lease_seconds)
                .await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!(
                    "Task {} successfully claimed by {}. The claim holds a lease (default 600s); publish progress with publish_progress to renew it, or the task returns to open.",
                    task_id, agent_id
                )
            })])
        }
        "complete_task" => {
            let task_id = args
                .get("task_id")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing task_id"))?;
            client.complete_task(task_id).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Task {} marked as completed", task_id)
            })])
        }
        "create_decision" => {
            let title = args
                .get("title")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing title"))?;
            let question = args
                .get("question")
                .and_then(|q| q.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing question"))?;
            let created_by = args
                .get("created_by")
                .and_then(|c| c.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing created_by"))?;
            let decider = args
                .get("decider")
                .and_then(|d| d.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing decider"))?;
            let options_raw = args
                .get("options")
                .and_then(|o| o.as_array())
                .ok_or_else(|| anyhow::anyhow!("Missing options"))?;

            let mut options = Vec::new();
            for (idx, opt) in options_raw.iter().enumerate() {
                let opt_str = opt
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Option must be a string"))?;
                options.push(mellowmesh_core::decision::DecisionOption {
                    id: format!("option_{idx}"),
                    label: opt_str.to_string(),
                    pros: Vec::new(),
                    cons: Vec::new(),
                });
            }

            let decision = mellowmesh_core::decision::Decision {
                id: format!("decision_{}", ulid::Ulid::new().to_string().to_lowercase()),
                title: title.to_string(),
                question: question.to_string(),
                created_by: created_by.to_string(),
                required_decider: decider.to_string(),
                status: "requested".to_string(),
                options,
                response_option_id: None,
                response_timestamp: None,
            };
            client.create_decision(&decision).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Decision request proposed with ID: {}", decision.id)
            })])
        }
        "list_decisions" => {
            let decisions = client.list_decisions().await?;
            let text = serde_json::to_string_pretty(&decisions)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "respond_decision" => {
            let decision_id = args
                .get("decision_id")
                .and_then(|d| d.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing decision_id"))?;
            let option_id = args
                .get("option_id")
                .and_then(|o| o.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing option_id"))?;
            client.respond_decision(decision_id, option_id).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Response choice {} registered for decision {}", option_id, decision_id)
            })])
        }
        "store_topic_summary" => {
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing topic"))?;
            let summary = args
                .get("summary")
                .and_then(|s| s.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing summary"))?;
            client.store_summary(topic, summary).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Topic summary updated for topic {}", topic)
            })])
        }
        "get_context" => {
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing topic"))?;
            let limit = args
                .get("limit")
                .and_then(|l| l.as_u64())
                .map(|u| u as usize);
            let context = client.get_context(topic, limit).await?;
            let text = serde_json::to_string_pretty(&context)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "enable_trace" => {
            let target_type = args
                .get("target_type")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing target_type"))?;
            let target = args
                .get("target")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing target"))?;
            let level = args
                .get("level")
                .and_then(|l| l.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing level"))?;
            let duration = args
                .get("duration")
                .and_then(|d| d.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing duration"))?;
            let reason = args
                .get("reason")
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());
            let enabled_by = args
                .get("enabled_by")
                .and_then(|e| e.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing enabled_by"))?;

            let ts = client
                .enable_trace(target_type, target, level, duration, reason, enabled_by)
                .await?;
            let text = serde_json::to_string_pretty(&ts)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "disable_trace" => {
            let id = args
                .get("id")
                .and_then(|i| i.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing id"))?;
            client.disable_trace(id).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Trace session {} disabled", id)
            })])
        }
        "list_traces" => {
            let traces = client.list_traces().await?;
            let text = serde_json::to_string_pretty(&traces)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "get_metrics" => {
            let metrics = client.get_metrics().await?;
            let text = serde_json::to_string_pretty(&metrics)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "list_wiki_pages" => {
            let wiki = args
                .get("wiki")
                .and_then(|w| w.as_str())
                .unwrap_or("default");
            let doc_type = args.get("doc_type").and_then(|d| d.as_str());
            let tag = args.get("tag").and_then(|t| t.as_str());

            let docs = client.list_wiki_pages(wiki, None, doc_type, tag).await?;
            let text = serde_json::to_string_pretty(&docs)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "get_wiki_page" => {
            let path = args
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
            let wiki = args
                .get("wiki")
                .and_then(|w| w.as_str())
                .unwrap_or("default");

            let doc = client.get_wiki_page(wiki, path).await?;
            let text = serde_json::to_string_pretty(&doc)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "write_wiki_page" => {
            let path = args
                .get("path")
                .and_then(|p| p.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
            let doc_type = args
                .get("doc_type")
                .and_then(|d| d.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing doc_type"))?;
            let title = args
                .get("title")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing title"))?;
            let description = args.get("description").and_then(|d| d.as_str());
            let tags: Vec<String> = args
                .get("tags")
                .and_then(|t| t.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let resource = args.get("resource").and_then(|r| r.as_str());
            let body = args
                .get("body")
                .and_then(|b| b.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing body"))?;
            let wiki = args
                .get("wiki")
                .and_then(|w| w.as_str())
                .unwrap_or("default");

            let doc = client
                .write_wiki_page(
                    wiki,
                    path,
                    doc_type,
                    title,
                    description,
                    tags,
                    resource,
                    body,
                )
                .await?;
            let text = serde_json::to_string_pretty(&doc)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Wiki page '{}' successfully saved.\n{}", path, text)
            })])
        }
        "search_wiki" => {
            let query = args
                .get("query")
                .and_then(|q| q.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
            let wiki = args
                .get("wiki")
                .and_then(|w| w.as_str())
                .unwrap_or("default");
            let doc_type = args.get("doc_type").and_then(|d| d.as_str());
            let tag = args.get("tag").and_then(|t| t.as_str());

            let docs = client
                .list_wiki_pages(wiki, Some(query), doc_type, tag)
                .await?;
            let text = serde_json::to_string_pretty(&docs)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "register_named_topic" => {
            let name = args
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
            let topic = args
                .get("topic")
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing topic"))?;

            client.register_named_topic(name, topic).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Named topic '{}' mapped to '{}' registered successfully.", name, topic)
            })])
        }
        "list_named_topics" => {
            let topics = client.list_named_topics().await?;
            let text = serde_json::to_string_pretty(&topics)?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": text
            })])
        }
        "remove_named_topic" => {
            let name = args
                .get("name")
                .and_then(|n| n.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing name"))?;

            client.remove_named_topic(name).await?;
            Ok(vec![serde_json::json!({
                "type": "text",
                "text": format!("Named topic '{}' removed successfully.", name)
            })])
        }
        _ => Err(anyhow::anyhow!("Unsupported tool call: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_tools_schema() {
        let tools = list_tools_schema();
        assert!(!tools.is_empty());
        assert_eq!(tools[0]["name"], "publish_message");
        assert_eq!(tools[1]["name"], "publish_progress");
        assert_eq!(tools[2]["name"], "publish_artifact");
    }

    #[test]
    fn test_make_error_response() {
        let err = make_error_response(serde_json::json!(123), -32601, "Method not found");
        assert_eq!(err["jsonrpc"], "2.0");
        assert_eq!(err["id"], 123);
        assert_eq!(err["error"]["code"], -32601);
        assert_eq!(err["error"]["message"], "Method not found");
    }
}
