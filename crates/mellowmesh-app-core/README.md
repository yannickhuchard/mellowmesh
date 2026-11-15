# mellowmesh-app-core

The shared Rust core of the fully native MellowMesh apps:

| Platform        | UI toolkit       | How it consumes this crate                         |
|-----------------|------------------|----------------------------------------------------|
| iOS + macOS     | SwiftUI          | XCFramework (static lib) + UniFFI Swift bindings   |
| Android         | Jetpack Compose  | `.so` per ABI via `cargo-ndk` + UniFFI Kotlin (JNA)|
| Windows         | WinUI 3 / C#     | `.dll` + C# bindings from `uniffi-bindgen-cs`      |

The shells are thin: they draw. Everything else — connecting to a hub
(locally or through the relay with end-to-end sealing), pairing-URI
validation, home/agent/decision/task view models, deriving conversations from
topics, live events, MeshUI card validation and the Mesh graph layout — lives
here, written once, tested once.

Bindings are produced with [UniFFI](https://mozilla.github.io/uniffi-rs/)
**0.29** proc-macros (`uniffi::setup_scaffolding!()`, `#[uniffi::export]`,
`#[derive(uniffi::Record | Enum | Object | Error)]`). 0.29 is used because it
is the line supported by NordSecurity's `uniffi-bindgen-cs` (C#). There is no
UDL file: bindings are generated from the compiled library ("library mode").

---

## Building and generating bindings

```sh
# 1. Build the library (cdylib + staticlib + rlib)
cargo build -p mellowmesh-app-core

# 2. Generate bindings from the built library (library mode)
cargo run -p mellowmesh-app-core --features cli --bin uniffi-bindgen -- \
    generate --library target/debug/libmellowmesh_app_core.so \
    --language swift --out-dir out/swift

cargo run -p mellowmesh-app-core --features cli --bin uniffi-bindgen -- \
    generate --library target/debug/libmellowmesh_app_core.so \
    --language kotlin --out-dir out/kotlin
```

Use `.dylib` on macOS and `.dll` on Windows for `--library`. Swift output is
`mellowmesh_app_core.swift` + `mellowmesh_app_coreFFI.h` +
`mellowmesh_app_coreFFI.modulemap`; Kotlin output is
`uniffi/mellowmesh_app_core/mellowmesh_app_core.kt`. Generated code is not
committed. (If `swift-format`/`ktlint` are missing the generator prints a
warning and leaves the output unformatted; that is harmless.)

### SwiftUI (iOS + macOS) — XCFramework

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin x86_64-apple-darwin
for t in aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build -p mellowmesh-app-core --release --target $t
done
lipo -create target/{aarch64,x86_64}-apple-darwin/release/libmellowmesh_app_core.a \
     -output target/libmellowmesh_app_core_macos.a

# Headers: the generated FFI header + a module map named module.modulemap
mkdir -p out/headers && cp out/swift/mellowmesh_app_coreFFI.h out/headers/
cp out/swift/mellowmesh_app_coreFFI.modulemap out/headers/module.modulemap

xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libmellowmesh_app_core.a -headers out/headers \
  -library target/aarch64-apple-ios-sim/release/libmellowmesh_app_core.a -headers out/headers \
  -library target/libmellowmesh_app_core_macos.a -headers out/headers \
  -output MellowMeshCore.xcframework
```

Add the XCFramework and `mellowmesh_app_core.swift` to a Swift package or the
Xcode project (one shared package for the iOS and macOS apps).

### Android — cargo-ndk + JNA

```sh
cargo install cargo-ndk
rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
  -o android/app/src/main/jniLibs build -p mellowmesh-app-core --release
```

Copy the generated Kotlin file into the app module and depend on JNA:
`implementation("net.java.dev.jna:jna:5.14.0@aar")`. The bindings load
`libmellowmesh_app_core.so` from `jniLibs`.

### Windows — uniffi-bindgen-cs (C# / WinUI 3)

```sh
cargo build -p mellowmesh-app-core --release --target x86_64-pc-windows-msvc
# Install the generator at a tag built for uniffi 0.29.x (tags look like vX.Y.Z+v0.29.N)
cargo install uniffi-bindgen-cs --git https://github.com/NordSecurity/uniffi-bindgen-cs --tag <tag for 0.29>
uniffi-bindgen-cs --library target/x86_64-pc-windows-msvc/release/mellowmesh_app_core.dll --out-dir out/cs
```

Ship `mellowmesh_app_core.dll` next to the WinUI app and add the generated
`.cs` file to the project. Listener callbacks use UniFFI *callback
interfaces* (not foreign-implemented trait objects) precisely because they
are the most widely supported shape across generators, including C#.

---

## Threading / runtime model

* A `MeshSession` **owns a 2-worker multi-threaded Tokio runtime**. Every
  method is **blocking**: it runs to completion on that runtime and returns.
  This is the most portable choice (identical semantics in Swift, Kotlin and
  C#; no foreign-executor glue). Call methods from a background context —
  `Task.detached {}` / `withCheckedThrowingContinuation` in Swift,
  `withContext(Dispatchers.IO)` in Kotlin, `Task.Run` in C# — never on the UI
  thread. Calling from inside a Tokio runtime returns `InvalidInput` rather
  than deadlocking.
* `subscribe` starts a runtime task that reads the WebSocket and pushes
  events into a channel drained by **one dedicated OS thread per
  subscription**, which calls `MeshEventListener.on_event` in order. Hop to
  the main thread before touching UI. Listener code may call session methods.
  A panic/exception in the listener is contained.
* Dropping the last reference to a `MeshSession` shuts the runtime down in
  the background (never blocks the dropping thread) and ends its
  subscriptions. Dropping a `SubscriptionHandle` cancels it — keep it alive.

---

## API

### Free functions

| Function | Purpose |
|---|---|
| `parse_pairing_uri(uri) -> PairingInfo` | Strict pairing-URI validation (below). |
| `parse_ui_card(json) -> MeshUiCard` | Strict MeshUI validation (below). |
| `layout_graph(nodes, edges, width, height, seed) -> MeshGraph` | Pure, deterministic graph layout. |
| `conversation_for_topic(topic) -> ConversationRef?` | Topic → conversation classification. |
| `dm_conversation_id(a, b) -> String` | Canonical `_chat.dm.<a>.<b>` id for two humans. |
| `accent_for(id) -> Accent`, `avatar_seed_for(id) -> u64` | Stable visual identity (FNV-1a). |
| `core_version() -> String` | Crate version. |

### `MeshSession` (object)

| Method | Returns |
|---|---|
| `MeshSession.connect(MeshConfig)` (constructor) | Validates config, checks `/health`, does one authenticated read so a bad token fails here. |
| `me()` | `Principal { id, kind, display_name, device_name }` |
| `base_url()`, `e2e_enabled()` | Connection info. |
| `home()` | `HomeSnapshot { me, agents, pending_decisions, active_tasks, conversations, generated_ms }` |
| `conversation(id, limit)` | `[ChatMessage]`, oldest first (server-side pattern history; `limit` 1..=1000, 0 → 50). |
| `send_message(conversation_id, text)` | The stored `ChatMessage`. |
| `send_ui_action(conversation_id, card_message_id, action_id, label)` | The stored reply `ChatMessage`. |
| `respond_decision(decision_id, option_id)` | `()` — see *Decisions*. |
| `mark_read(conversation_id, up_to_ms)`, `read_marker(conversation_id)` | Local read state driving `unread`. |
| `subscribe(pattern, listener)` | `SubscriptionHandle { cancel(), is_active(), pattern() }` |
| `graph(width, height)` | `MeshGraph` built from `home()` and laid out with seed = hash(me). |

`MeshConfig { base_url, token?, e2e?, device_name, principal? }`:

* `base_url` — `http://127.0.0.1:40000` for a local hub or a relay hub URL
  (`PairingInfo.hub_url`). `http` is accepted only for loopback.
* `token` — bearer token. `e2e` — explicit sealing; `None` = on for a remote
  hub with a token (same default as the Rust SDK). E2E without a token is
  refused rather than silently sent in plaintext.
* `principal` — the user's `human://…` URI. The hub has no "who am I"
  endpoint yet, so the shell supplies it (from pairing/settings; default
  `human://local`). Under authentication the hub stamps the real principal on
  everything published regardless, so this only affects presentation
  (`is_mine`, `for_me`, conversation titles).

### Records and enums

* `AgentCard { id, display_name, owner, is_mine, mode, capabilities, presence, last_seen_ms?, current_task_title?, avatar_seed, accent, conversation_id? }`
* `Presence = Online | Busy | Idle | Offline`; `Accent = Indigo | Cyan | Pink | Amber | Emerald`
* `DecisionCard { id, title, question, options: [OptionCard], created_by, required_decider, status, created_ms, for_me }`,
  `OptionCard { id, label, outcome: Approve | Reject | Neutral, pros, cons }`
* `TaskCard { id, title, status, priority, created_by, claimed_by?, claim_expires_ms? }`
* `ConversationSummary { id, kind, title, participants: [ParticipantRef], last_message_preview?, last_activity_ms, unread }`
* `ParticipantRef { id, kind: PrincipalKind, display_name, avatar_seed, accent }`; `PrincipalKind = Human | Agent | Node | Interface`
* `ChatMessage { id, topic, from, from_kind, from_display, body, content_type, ts_ms, is_mine, parent_id?, ui: MeshUiCard?, ui_action: UiActionReply? }`
* `MeshEvent` (below), `MeshGraph` (below), `PairingInfo` (below)
* `MeshError = Network | Unauthorized | Forbidden | NotFound | InvalidInput | Server`, each `{ msg }`.
  Mapped from real HTTP statuses (401/403/404, 400/409/413/422 → `InvalidInput`, others → `Server`).

All fabric-originated strings (bodies, names, titles, labels) are sanitized
with `mellowmesh_client::sanitize` (escape sequences, control and bidi
characters removed) before they cross the FFI.

---

## Conversations: topic conventions

The hub has no conversation object; a conversation is a convention over topic
names. Its **id is the topic root**.

| Topic(s) | Kind | Conversation id | `send_message` publishes to |
|---|---|---|---|
| `_agent.<owner>.<name>.inbox`, `_agent.<owner>.<name>.outbox` | `Agent` (1:1 with one agent) | `_agent.<owner>.<name>` | `_agent.<owner>.<name>.inbox` |
| `_project.<name>`, `_project.<name>.**` | `Squad`, or `Group` once another human posts | `_project.<name>` | `_project.<name>` |
| `_forum.<name>`, `_forum.<name>.**` | `Squad` / `Group` (same rule) | `_forum.<name>` | `_forum.<name>` |
| `_chat.dm.<a>.<b>` (handles sorted) | `Person` (1:1 human) | `_chat.dm.<a>.<b>` | same |
| `_chat.group.<name>[.**]` | `Group` (humans + agents) | `_chat.group.<name>` | `_chat.group.<name>` |
| `_org.<org>[.**]` | `Organization` | `_org.<org>` | `_org.<org>` |

* Agent ids map to topic paths the way the hub routes mentions:
  `agent://<owner>/<name>` → `_agent.<owner>.<name>`. **Agents reply on their
  `.outbox`** (writing to their own inbox would echo back to themselves);
  `_agent.<…>.heartbeat` / `.status` belong to the agent but are not chat.
* Matching is case-insensitive; ids are lowercased.
* Everything else (`_task.*`, `_decision.*`, `_system.*`, `_wiki.*`,
  application topics) is not a conversation; it surfaces as cards/events.
* `home()` scans the most recent 500 messages; every one of *my* agents gets
  an Agent conversation even before the first message.
* `unread` counts messages from others newer than the read marker. Read state
  is per session (`mark_read`); shells persist it and replay it after
  `connect`. (A synced, server-side read state is a TODO.)

## Presence

`/agents` returns registrations only, so presence is derived:

1. an active claim (task `claimed`/`in_progress`, lease not expired) → `Busy`;
2. a fresh explicit `status: "offline"` on `_agent.<path>.status` / `.heartbeat` → `Offline`;
3. last seen (newest message from the agent or on its `_agent.<path>.*`
   topics, inbox excluded): ≤ 2 min → `Online` (or `Busy` if it said so),
   ≤ 30 min → `Idle`, else `Offline`.

## Live events

`subscribe(pattern, listener)`; `MeshEventListener.on_event(MeshEvent)`.
Patterns use the hub grammar (`*`, `**`, `>`); `**` = everything readable.

| Topic | Event |
|---|---|
| conversation topics (table above) | `MessageReceived { conversation_id, kind, message }` |
| `_decision.<id>.requested` (hub) | `DecisionCreated { decision }` |
| `_decision.<id>.responded` (hub) | `DecisionResolved { decision_id, status, option_id?, responded_by? }` |
| `_task.<id>.reclaimed` (hub sweeper) | `TaskReclaimed { task_id, agent = previous claimant }` |
| `_task.<id>.claimed` / `.completed` (client convention) | `TaskClaimed` / `TaskCompleted { task_id, agent }` |
| `_task.<id>.progress` (lease heartbeat) | `TaskProgress { task_id, agent, note }` |
| `_agent.<path>.heartbeat` / `.status`, `_system.presence.**` | `PresenceChanged { agent_id, presence }` |
| anything else | `Other { topic, from, preview, ts_ms }` |
| connection changes | `StreamStatus { connected, detail }` (retries 1 s → 30 s) |

Note: the reference hub itself only emits `_decision.*` and
`_task.<id>.reclaimed`; it does **not** announce claims/completions (the
standalone WASM engine and well-behaved agents do). Refresh `home()` on
`StreamStatus { connected: true }` to resync after a gap.

## Decisions

`respond_decision(decision_id, option_id)` answers through
`POST /decisions/<id>/respond`. The recorded status derives from the
option's `outcome` (approve → `approved`, reject → `rejected`, neutral →
`answered`).

* **Shells MUST gate this call behind OS biometrics** — Face ID / Touch ID
  (LocalAuthentication), Android `BiometricPrompt` (class 3), Windows Hello
  (`UserConsentVerifier`) — immediately before calling, showing the decision
  title and the chosen option's label.
* The session token **must be a human token**: the hub rejects agent and node
  principals, and under authentication anyone but the decision's
  `required_decider` or the hub owner (`Forbidden`). A second answer fails
  (`InvalidInput`, HTTP 409). In open (unauthenticated) mode the hub records
  the responder as unauthenticated.

## MeshUI — native cards in messages

Agents attach a declarative card to a message in `payload.mesh_ui`; each OS
renders it with native components (a deliberately small A2UI-like schema).
Cards are data, never code.

```json
{
  "version": 1,
  "title": "Nightly build",
  "blocks": [
    { "type": "text", "markdown": "**Green** on `main` — see <https://ci.example.com/42>" },
    { "type": "key_value", "pairs": [ { "key": "commit", "value": "7330242" } ] },
    { "type": "progress", "label": "Tests", "value": 0.75 },
    { "type": "metric", "label": "Coverage", "value": "82.4%", "delta": "+1.2%" },
    { "type": "list", "items": [ "lint ok", "unit ok" ] },
    { "type": "chart", "kind": "line", "series": [ 1, 2.5, 3 ], "labels": [ "Mon", "Tue", "Wed" ] },
    { "type": "image", "url": "https://example.com/graph.png", "alt": "Build times" },
    { "type": "actions", "actions": [
        { "id": "retry", "label": "Retry", "style": "primary" },
        { "id": "details", "label": "Details" },
        { "id": "drop.cache", "label": "Drop cache", "style": "destructive" } ] }
  ]
}
```

Validation (`parse_ui_card`, also applied to every incoming message; an
invalid card is dropped and the message shown without it):

* JSON ≤ 64 KiB; `version` must be `1`; unknown fields or block types are
  rejected (`deny_unknown_fields`).
* 1..=32 blocks; title ≤ 120 chars; markdown ≤ 4000; key_value 1..=32 pairs
  (key ≤ 64, value ≤ 256); list 1..=50 items (≤ 256); metric label ≤ 64,
  value/delta ≤ 32; chart 1..=500 finite points, `line`|`bar`, labels empty or
  same length (≤ 32 chars); image `https` only, no credentials, URL ≤ 2048,
  non-empty `alt` ≤ 256; at most one actions block with 1..=6 actions, unique
  ids of `[A-Za-z0-9_.-]{1,64}`, label ≤ 40, style `primary`|`secondary`
  (default)|`destructive`; `progress.value` in `0..=1`.
* No script or HTML anywhere: raw tags (`<b>`, `<img …>`, `<!-- -->`),
  `javascript:`, `vbscript:`, `data:text/html` are rejected (markdown
  autolinks `<https://…>` and prose like `a < b` are fine). Remaining
  control/bidi characters are stripped. Shells render markdown without HTML
  and must not auto-load images referenced from markdown.

**Actions never answer decisions.** Tapping an action calls
`send_ui_action(conversation_id, card_message_id, action_id, label)`, which
publishes an ordinary reply into the same conversation (agent inbox for
Agent conversations) with `content_type =
application/vnd.mellowmesh.ui-action+json`, `parent_id = card_message_id` and
`payload.ui_action = { "card_message_id", "action_id" }`. The agent that sent
the card decides what it means. Decisions are answered only via
`respond_decision`, behind biometrics.

## Pairing URIs

```text
mellowmesh-pair:?v=1&relay=<https url>&hub=<hub id>&code=<one-time code>&fp=<sha256 fingerprint>[&name=<label>]
```

| Param | Rule |
|---|---|
| `v` | optional; only `1` |
| `relay` | `https` URL (`http` only for a loopback host); no credentials, query or fragment; ≤ 512 bytes; trailing `/` trimmed |
| `hub` | 1..=64 of `[A-Za-z0-9_-]` (same as the relay's hub-id rule) |
| `code` | 6..=64 of `[A-Za-z0-9-]` |
| `fp` | SHA-256 of the hub's pairing key: 64 hex, optional `sha256:` prefix, or 32 colon-separated pairs; normalized to lowercase hex |
| `name` | optional label, 1..=64 chars (sanitized) |

Each parameter at most once; unknown parameters, fragments and URIs over
2048 bytes are rejected. Result: `PairingInfo { version, relay_url, hub_id,
hub_url = <relay_url>/hub/<hub_id>, code, fingerprint, hub_name? }`.

**TODO (server side):** the pairing exchange does not exist yet. The intended
flow: the hub shows the QR; the app calls a relay/hub pairing endpoint with
`code`, verifies the hub's key against `fp`, and receives a device-scoped
**human** token, then `connect`s with `base_url = hub_url`, `e2e = true`.
Until then `parse_pairing_uri` only validates.

## Mesh graph

`graph(width, height)` turns the home snapshot into nodes
(`Me | Agent | Human | Topic | Task | Peer`) and edges
(`Owns` me/human → agent, `Claims` agent → task, `Subscribes` agent → its
1:1 conversation, `Talks` participant → conversation, weighted) and lays them
out with `layout_graph`:

* radial force-directed layout, deterministic for a given input + seed
  (SplitMix64 jitter), 200 iterations, all-pairs repulsion (O(n²), n ≤ 500;
  extra nodes are dropped), spring attraction, linear cooling;
* rings by hop distance from `Me`: me at the center (`z = 0`), my agents on
  the first ring, then further rings outwards; unreachable nodes on the last
  ring. Nodes are constrained to their ring band, so my agents are always
  closer to me than anything two hops away;
* `x, y` always finite and inside `[0, width] × [0, height]`; `z ∈ [0, 1]` is
  a depth hint (ring / ring count) for a 2.5D/3D effect; `size` is a
  suggested diameter in canvas units; `ring` is exposed for styling.

---

## Tests

`cargo test -p mellowmesh-app-core` runs unit tests (pairing, conversation
derivation, MeshUI validation incl. rejection cases, graph layout
determinism/bounds/ordering, presence, event mapping, snapshot derivation,
error mapping) and an integration test that spawns a real in-process daemon
(fixed ports 41201–41202) and drives `MeshSession` like a shell: home
snapshot, agent conversation send/read-back, live `MessageReceived`, MeshUI
round-trip, decision approval → `DecisionResolved`, error mapping and
authentication failures.

## Known gaps / TODO

* Pairing exchange endpoint (above) and a hub "who am I" endpoint (so
  `principal` need not be supplied by the shell).
* Read state is local to the session (no cross-device sync).
* `home()` derives presence and conversations from the latest 500 messages;
  a dedicated presence/conversation index on the hub would scale better.
* TLS: the dependency chain (`reqwest`, `tokio-tungstenite`) uses
  `native-tls`. That is Security.framework on Apple and SChannel on Windows,
  but OpenSSL on Android — vendor OpenSSL for the NDK build or switch these
  crates to `rustls` for mobile.
* Async exports: methods are blocking by design (see threading model);
  UniFFI async exports can be added later where all three generators support
  them.

## Supply-chain note

`cargo audit` reports `basic-toml` as unmaintained (RUSTSEC-2026-0374). It is
pulled in only by `uniffi_bindgen` (via `askama`) when the `cli` feature is
enabled to generate bindings at build time; it is not linked into the
library shipped inside the apps. Revisit when upgrading uniffi.
