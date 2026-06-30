# `@mellowmesh/wasm`

**Browser WebAssembly Coordination Core & JS SDK for Human-Agent Collaboration**

`@mellowmesh/wasm` compiled package brings the powerful event-driven topic publish/subscribe coordination fabric of MellowMesh directly into the web browser. Written in Rust and compiled via WebAssembly, it can run as an independent, offline-first coordination node or connect to a local native daemon.

---

## 1. Features
* **Dual Operation Modes**:
  * `standalone`: An in-browser node containing the full in-memory coordination engine (wildcard subscriber, agent registries, task Kanban state machine, decision logs) with zero backend dependencies.
  * `client`: A client SDK connecting to the native background daemon (`mellowmeshd`) via WebSocket and REST APIs.
* **Cross-Tab Synchronization**: Automatically replicates published messages, task transitions, and decision changes across multiple browser tabs using `BroadcastChannel` in `standalone` mode.
* **LocalStorage Persistence**: Auto-saves and restores database state to LocalStorage so tasks and messages persist across browser reloads.
* **Native Speed**: Core coordination routing and wildcard matches executed in WebAssembly compiled from optimized Rust.

---

## 2. Installation & Compilation
The package contains both the compiled Rust WebAssembly binary and a JavaScript class wrapper.

### Build from Source
To compile the Rust crate into a web-targeting WASM module:
```bash
cd crates/mellowmesh-wasm
npm run build
```
This runs `wasm-pack build --target web --out-dir pkg` and compiles WebAssembly exports under `pkg/`.

---

## 3. Getting Started

### Standalone Mode (In-Browser Offline Core)
```javascript
import { BrowserMellowMesh } from '@mellowmesh/wasm';

async function start() {
  const mellowmesh = new BrowserMellowMesh({
    mode: 'standalone',
    persistenceKey: 'mellowmesh_state',
    broadcastChannelName: 'mellowmesh_broadcast'
  });

  // Fetch and compile WASM, boot the internal Rust coordination engine, and restore stored state
  await mellowmesh.init();

  // 1. Subscribe to topic hierarchies using wildcard operators
  const subId = mellowmesh.subscribe('_task.auth.>', (message) => {
    console.log(`Topic match: ${message.topic} - Body: ${message.body}`);
  });

  // 2. Publish a message to the coordination fabric (auto-replicated across tabs)
  await mellowmesh.publish({
    topic: '_task.auth.status',
    from: 'agent://web-client',
    body: 'Initiating authentication configuration audit...'
  });

  // 3. Cleanup on shutdown
  mellowmesh.close();
}

start();
```

### Client Mode (Daemon-Connected Bridge)
```javascript
import { BrowserMellowMesh } from '@mellowmesh/wasm';

async function start() {
  const mellowmesh = new BrowserMellowMesh({
    mode: 'client',
    daemonUrl: 'ws://127.0.0.1:40000/ws',
    daemonHttpUrl: 'http://127.0.0.1:40000'
  });

  await mellowmesh.init();
  
  // Real-time pub/sub streams through active WebSocket connection
  mellowmesh.subscribe('_project.chat.**', (msg) => {
    console.log(`Live forum message: ${msg.body}`);
  });
}
```

---

## 4. API Reference

### `new BrowserMellowMesh(config)`
Creates the coordination manager.
* `config.mode` (`"standalone" | "client"`, default `"standalone"`): Selects whether to execute logic inside browser WASM or route calls to `mellowmeshd`.
* `config.daemonUrl` (`string`, default `"ws://127.0.0.1:40000/ws"`): Daemon WebSocket endpoint.
* `config.daemonHttpUrl` (`string`, default `"http://127.0.0.1:40000"`): Daemon HTTP API base.
* `config.broadcastChannelName` (`string`, default `"mellowmesh_broadcast"`): Cross-tab BroadcastChannel namespace.
* `config.persistenceKey` (`string`, default `"mellowmesh_state"`): Key for LocalStorage state.

### `async init(wasmModule?)`
Downloads, compiles WASM module, starts the database engine, restores state from LocalStorage (if standalone), or establishes WebSocket connections (if client).

### `async publish(message)`
Publishes a message to the fabric.
* `message.topic`: Topic string (e.g. `_project.status`).
* `message.from`: Author URI (e.g. `agent://yannick/reviewer`).
* `message.body`: Markdown body.
* `message.payload` (optional): JSON payload.
* *Returns*: The published message's unique ID (ULID).

### `subscribe(pattern, callback)`
Registers a callback to receive real-time messages.
* `pattern`: Topic filter supporting wildcards (`*`, `>`, `**`).
* `callback`: `(message) => void`
* *Returns*: Subscription ID.

### `unsubscribe(subId)`
Removes the subscription.

### `async createTask(task)`
Creates a new task. Task objects specify `id`, `title`, `description`, `priority`, `topics`, and `required_capabilities`.

### `async listTasks()`
Returns all tasks.

### `async claimTask(taskId, agentUri)`
Claims a task for a specific agent.
* *Returns*: `true` if task was claimed successfully.

### `async completeTask(taskId)`
Marks a task as completed.

### `async createDecision(decision)`
Submits a decision request for human review.
* `decision.required_decider`: Human decider URI (e.g. `human://yannick`).
* `decision.options`: Array of option objects (`{ id, label }`).

### `async listDecisions()`
Returns all decision requests.

### `async respondDecision(decisionId, optionId)`
Records a response from a human decider.

### `async readHistory(pattern, limit?)`
Queries historical messages matching a pattern.

### `clearState()`
Wipes all persisted LocalStorage state (standalone only).

### `close()`
Cleans up active WebSocket connections, reconnection timers, and BroadcastChannel instances.
