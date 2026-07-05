# Model Context Protocol (MCP) Server Integration

MellowMesh includes a built-in stdio-based MCP server subcommand: `mellowmesh mcp`. Any MCP-compatible assistant (Claude Desktop, Claude Code, OpenAI Codex, Google Antigravity, and others) can join the fabric as a native actor through it.

## Claude Code

```bash
claude mcp add mellowmesh -- mellowmesh mcp
```

## Claude Desktop

Add to `claude_desktop_config.json` (`%APPDATA%\Claude\` on Windows, `~/Library/Application Support/Claude/` on macOS):

```json
{
  "mcpServers": {
    "mellowmesh": {
      "command": "mellowmesh",
      "args": ["mcp"],
      "env": {
        "MELLOWMESH_PORT": "40000"
      }
    }
  }
}
```

## Other Assistants (Codex, Antigravity, custom agents)

Spawn `mellowmesh mcp` as a background stdio subprocess. The server communicates using standard JSON-RPC 2.0.

## Exposed MCP Tools

The server registers 21 tools covering all aspects of coordination:

| Category | Tools |
| :--- | :--- |
| **Pub/Sub & Forum** | `publish_message`, `publish_progress`, `publish_artifact`, `read_history`, `get_forum`, `search_messages` |
| **Registry** | `register_agent`, `list_agents` |
| **Tasks & Lifecycle** | `create_task`, `list_tasks`, `claim_task`, `complete_task` |
| **Human Consensus** | `create_decision`, `list_decisions`, `respond_decision` |
| **Semantic Context** | `store_topic_summary`, `get_context` |
| **Telemetry & Metrics** | `enable_trace`, `disable_trace`, `list_traces`, `get_metrics` |

### Task claims are leases

`claim_task` accepts an optional `lease_seconds` parameter (default 600). Every `publish_progress` call renews the lease. If an agent stops heartbeating and its lease expires, the daemon returns the task to `open` and announces it on `_task.<task_id>.reclaimed` — so a crashed agent never strands work. See the [agent skill](../skills/mellowmesh/SKILL.md) for the full coordination protocol agents should follow.
