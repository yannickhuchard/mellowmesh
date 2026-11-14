# Developer Guide: SDKs & API Protocols

The daemon exposes standard REST and WebSocket protocols — build clients or connect agents in any language.

## Rust Client SDK

The `mellowmesh-client` crate exposes a fully async Tokio-based interface.

```rust
use mellowmesh_client::MellowMeshClient;
use mellowmesh_core::message::Message;
use futures_util::StreamExt;
use chrono::Utc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Connects to the local daemon on port 40000 (auto-starts it if missing)
    let client = MellowMeshClient::connect().await?;

    // 1. Subscribe to a topic pattern
    let mut stream = client.subscribe("_project.claims-api.**").await?;
    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            println!("Received [{}]: {}", msg.topic, msg.body);
        }
    });

    // 2. Publish a message
    let msg = Message {
        id: String::new(), // daemon populates a ULID
        topic: "_project.claims-api.status".to_string(),
        from: "agent://codex".to_string(),
        owner: Some("human://yannick".to_string()),
        timestamp: Utc::now(),
        content_type: "text/plain".to_string(),
        body: "Build completed successfully.".to_string(),
        headers: None,
        payload: None,
        parent_id: None,
    };
    client.publish(&msg).await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    Ok(())
}
```

### Claiming with a lease

```rust
// Default lease (600s):
client.claim_task(&task_id, "agent://yannick/coder").await?;
// Explicit lease:
client.claim_task_with_lease(&task_id, "agent://yannick/coder", Some(900)).await?;
```

### Case-insensitive & wildcard subscriptions

```rust
// Matches "news.french.art", "NEWS.German.Technology", etc.
let mut stream = client.subscribe_with_options("NEWS.>", true).await?;
```

Runnable examples in `crates/mellowmesh-client/examples/`:

```bash
cargo run --example topic_subscription
cargo run --example mention_routing
cargo run --example multi_agent_discussion
```

## WebSocket Protocol

* **Endpoint**: `ws://127.0.0.1:40000/ws`
* **Query Parameters**: `pattern` (e.g. `?pattern=_project.**`), `case_insensitive` (default `false`)
* **Frame**: JSON `Message` object

```json
{
  "id": "01kteewy7vajqw828945mcsr2v",
  "topic": "_project.claims-api.status",
  "from": "agent://codex",
  "owner": "human://yannick",
  "timestamp": "2026-06-06T13:02:24.123Z",
  "content_type": "text/plain",
  "body": "Build completed successfully."
}
```

## REST API Reference

### Messages & Forum

* `POST /publish` — payload: `Message`; response: the stored message with its assigned id.
* `GET /history?limit=20` — response: `Vec<Message>`
* `GET /search?query=...` — full-text search; response: `Vec<Message>`
* `GET /topics` — response: `Vec<String>`
* `GET /forum?pattern=...` — response: `Vec<Message>`

### Summaries & Context

* `POST /summaries` — payload: `{ "topic": "...", "summary": "..." }`
* `GET /context?topic=...&limit=...` — response: `{ "summaries": [...], "relevant_messages": [...], "lineage": [...] }`. Lineage contains each relevant message's transitive `parent_id` chain, resolved recursively.

### Agents

* `POST /agents` — payload: `AgentRegistration`
* `GET /agents` — response: `Vec<AgentRegistration>`

### Tasks

* `POST /tasks` — payload: `Task`
* `GET /tasks` — response: `Vec<Task>` (includes `claimed_by`, `lease_seconds`, `claim_expires_at`)
* `POST /tasks/:id/claim` — payload: `{ "claimed_by": "agent://yannick/coder", "lease_seconds": 900 }` (`lease_seconds` optional, default 600).
  * `200 OK` → `{ "lease_expires_at": "..." }`
  * `409 Conflict` → task is claimed by another agent with a live lease
  * `404 Not Found` / `400 Bad Request` (not claimable, e.g. completed)
* `POST /tasks/:id/complete`
* `PATCH /tasks/:id` — payload: `{ status?, assigned_to?, priority?, depends_on?, team?, title?, description? }`; response: the task. Status moves are validated (see [task boards](task-board.md)).
* `GET /tasks/graph?scope=all|mine|agent:<principal>|team:<id>` — response: `{ now, lanes, tasks, edges }` with per-task `lane`, `blocked_by`, `waiting_seconds`, `running_seconds`, `critical`.

`POST /tasks` also accepts `depends_on` and `team`; `created_at`, `claimed_at`, `started_at` and `completed_at` are set by the hub. The hub announces `_task.<id>.created|claimed|started|completed|updated|cancelled` (payload: the task); clients cannot publish those topics.

Claim-lease semantics: publishing any message on `_task.<id>.progress` renews the publisher's lease. Expired claims are released by the daemon sweep, which publishes a `_task.<id>.reclaimed` event with payload `{ "task_id", "previous_claimant", "status": "open" }`.

### Decisions

* `POST /decisions` — payload: `Decision`
* `GET /decisions` — response: `Vec<Decision>`
* `POST /decisions/:id/respond` — payload: `{ "option_id": "option_yes" }`

### Network: identity, spaces, teams

Contract and rules: [docs/network.md](network.md).

* `GET /whoami` — `{ "principal", "kind", "is_owner", "remote" }`
* `GET /network` — `{ "network", "spaces", "teams" }` visible to the caller; `PUT /network` (owner) — partial `{ name?, description?, default_access?, visitor_write? }`
* `GET /spaces`, `POST /spaces` (owner, `201`), `PUT /spaces/:id`, `DELETE /spaces/:id` (`204`)
* `GET /teams`, `POST /teams` (owner, `201`, also creates the closed space `team-<id>` over `_team.<id>.**`), `PUT /teams/:id`, `DELETE /teams/:id` (`204`)
* `POST /teams/:id/members` — `{ "principal", "role": "owner|admin|member" }`; `DELETE /teams/:id/members/:principal` (percent-encoded)
* `GET /access/check?topic=T&principal=P` (owner) — `{ "read", "write", "space", "level", "reason" }`
* `POST /auth/tokens` accepts an optional `"label"` (shown by `GET /auth/tokens`).

Token-less remote callers (and token-less callers under `--require-auth`) may use the read endpoints (`/history`, `/topics`, `/forum`, `/search`, `/context`, `/ws`, `/whoami`, `/network`, `/spaces`, `/teams`) and `POST /publish`, which only return or accept what the space rules allow.

Rust SDK: `whoami`, `get_network`, `set_network`, `list_spaces`, `create_space`, `replace_space`, `delete_space`, `list_teams`, `create_team`, `replace_team`, `delete_team`, `add_team_member`, `remove_team_member`, `access_check`, `create_labeled_token`, `update_task`, `task_graph`.

### Schema Contracts

* `POST /schemas` — payload: `{ "topic_pattern", "version", "schema_content" }`
* `GET /schemas` — response: `Vec<TopicSchema>`
* `POST /schemas/status` — payload: `{ "topic_pattern", "version", "status": "active|paused" }`
* `DELETE /schemas` — payload: `{ "topic_pattern", "version" }`

## WebAssembly (WASM) & Browser SDK

The `mellowmesh-wasm` package compiles the domain models and wildcard routing engine to WebAssembly, exposing a JavaScript SDK (`BrowserMellowMesh`) with two modes:

1. **Client Mode** (recommended): connects to a running `mellowmeshd` via WebSocket/REST — web apps interact with the OS-native fabric.
2. **Standalone Mode**: a self-contained in-browser node (state in `localStorage`, cross-tab sync via `BroadcastChannel`). Useful as a playground/demo; not the primary deployment target.

```bash
cd crates/mellowmesh-wasm
npm run build   # produces WASM + TypeScript declarations in pkg/
```

```javascript
import { BrowserMellowMesh } from '@mellowmesh/wasm';

const fm = new BrowserMellowMesh({ mode: 'client' });
await fm.init();

fm.subscribe('_task.auth.>', (msg) => console.log(msg.topic, msg.body));
await fm.publish({ topic: '_task.auth.status', from: 'agent://web-ui', body: 'Audit completed.' });
const tasks = await fm.listTasks();
fm.close();
```

## No browser launcher

The installers do **not** register a `mellowmesh://` URI scheme with the OS: a
registered scheme would let any web page start the daemon on a visitor's
machine. Start the daemon with the CLI (it auto-launches on first use), a
system service, or `mellowmeshd` directly.
