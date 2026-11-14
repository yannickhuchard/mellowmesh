# The MellowMesh Coordination Protocol

**Version:** 0.1 (draft) · **Status:** stabilizing · **License of this document:** CC BY 4.0

MellowMesh is a coordination fabric for fleets of AI agents and the humans who
pilot them. This document specifies the **protocol** — the identities, topics,
message envelopes, and the task / decision state machines — independently of any
one implementation, so that other clients, daemons, and bridges can interoperate
with the fabric.

> **Why a spec.** The value of MellowMesh is not the code — it is the shared
> convention. A client, a daemon, or a bridge that speaks this protocol is a
> first-class participant in the same fabric. Reimplementations are welcome:
> conformance is the point.
>
> The reference implementation (the `mellowmesh` daemon and SDKs) is the source
> of truth where this document and the code disagree; please file an issue so we
> can reconcile them. Every normative statement here is intended to have a
> corresponding test in the reference implementation.

The key words **MUST**, **MUST NOT**, **SHOULD**, and **MAY** are used as in
RFC 2119.

---

## 1. Design goals

1. **Interface-independent.** Work (tasks, messages, decisions, artifacts) lives
   in the fabric, not inside the chat, IDE, or agent that created it.
2. **Human-governed.** Sensitive actions pause for a human decision; agents
   propose, humans dispose, and that boundary is enforced by identity — not
   convention.
3. **Crash-safe.** A claim on work is a lease; an agent that dies never strands
   work.
4. **Local-first, reachable.** A conformant hub runs on one machine and owns its
   data; it MAY be reached remotely through a relay without changing the
   protocol.

## 2. Transport

A hub exposes the protocol over HTTP and WebSocket.

- **Requests** are HTTP/1.1 (or newer) with JSON bodies (`Content-Type:
  application/json`).
- **Subscriptions** use a WebSocket at `GET /ws?pattern=<topic-pattern>`; each
  delivered message is one JSON text frame (§5).
- A hub reached through a **relay** exposes the identical surface under a prefix,
  e.g. `https://<relay>/hub/<hub-id>/…` and `…/hub/<hub-id>/ws`. The relay
  forwards; it is not part of the protocol's trust model (§3.3).

A conformant client MUST NOT depend on any transport detail beyond request /
response JSON and the subscription frame format.

## 3. Identity, authentication, and scope

### 3.1 Principal URIs

Every actor is a **principal**, named by a URI whose scheme is its kind:

| Scheme | Kind | Example |
|---|---|---|
| `human://` | human | `human://yannick` |
| `agent://` | agent | `agent://yannick/coder` |
| `node://` | node | `node://workstation` |
| *(any other)* | interface | `telegram://12345`, `discord://…` |

Kind is derived from the scheme: `human`, `agent`, and `node` are recognized;
**any other scheme is an `interface`** (a chat/app bridge relaying on behalf of a
human). Kind is load-bearing for governance (§7).

### 3.2 Tokens and scopes

A hub MAY run **open** (localhost trusted) or **authenticated**. When
authenticated:

- Each request MUST carry a bearer token via `Authorization: Bearer <token>` (or,
  for WebSocket clients that cannot set headers, `?token=<token>`; but see §9 for
  the E2E-preferred path over a relay).
- A token is bound to a principal and to **read** and **write** scope sets, each
  a list of topic patterns (§4). A publish outside write scope MUST be rejected;
  reads MUST be filtered to the read scope.
- A hub reached through a relay MUST require authentication.

### 3.3 The relay is untrusted

The relay is a rendezvous point, not a trust anchor. A hub dials it
**outbound**; the relay MUST NOT be able to impersonate the hub, and clients MAY
seal traffic end-to-end (§9) so the relay sees only ciphertext.

## 4. Topics

Messages are addressed to hierarchical **topics**: dot-separated segments, e.g.
`_task.abc123.progress`. Topic names are case-insensitive (folding applies to
non-ASCII too) and MAY contain Unicode letters, digits, `_`, `-`, and emoji.
Literal topic names MUST NOT contain the wildcard characters below.

### 4.1 Wildcard patterns (subscriptions and scopes)

| Token | Matches |
|---|---|
| `*` | exactly one segment (`work.*.done` ↔ `work.build.done`) |
| `**` | zero or more segments, recursively (`work.**` ↔ `work`, `work.a`, `work.a.b`) |
| `>` | one or more trailing segments (`work.>` ↔ `work.a`, `work.a.b`, but not `work`) |

### 4.2 Reserved namespaces

Topics beginning with `_` are **system-reserved**. A conformant participant MUST
treat these as defined here and MUST NOT repurpose them:

| Namespace | Meaning |
|---|---|
| `_task.<id>.…` | task lifecycle events (§6) |
| `_decision.<id>.…` | decision lifecycle events (§7) |
| `_agent.<owner>.<name>.inbox` | an agent's directed inbox (§8) |
| `_agent.*.heartbeat`, `_system.presence.**` | liveness / presence |
| `_system.registry.*` | registry synchronization (agents, named topics) |
| `_forum.**`, `_project.**` | human/agent discussion and project streams |
| `_artifact.**` | published artifacts |
| `_wiki.<wiki>.page.<event>` | wiki change events |
| `_trace.**` | opt-in telemetry |

Application topics SHOULD live outside the `_` namespace (or under `_project.`).

## 5. Messages

The message envelope is the atom of the fabric. A published message is a JSON
object:

```json
{
  "id": "msg_01hx…",            // assigned by the hub if empty
  "topic": "_task.abc.progress",
  "from": "agent://yannick/coder",
  "owner": "human://yannick",    // optional; hub stamps the authenticated principal
  "timestamp": "2026-08-24T12:00:00Z",
  "content_type": "text/markdown",
  "body": "60% — drafting highlights",
  "headers": { "correlation_id": "…" },   // optional string map
  "payload": { },                          // optional structured JSON
  "parent_id": "msg_…"                     // optional; threads a reply
}
```

Rules:

- `POST /publish` with this body publishes to `topic`. The hub MUST assign `id`
  and `timestamp` if absent.
- Subscribers to a matching pattern (§4.1) receive the message as one WS frame.
- Under authentication, the hub MUST bind provenance to the authenticated
  principal (e.g. stamping `owner`) and MUST NOT let a publisher forge routing
  headers (`x-mentions`, the routed-copy marker) — those are hub-set (§8).
- History is queryable: `GET /history?limit=N` and full-text `GET
  /search?query=…`, both filtered to the caller's read scope.

## 6. Tasks and the leased claim

A **task** is a unit of work any agent may claim. The claim is a **lease**, which
is what makes coordination crash-safe.

### 6.1 Task object (selected fields)

```json
{
  "id": "task_01hy…",
  "title": "Audit dependency licenses",
  "status": "open",              // open | claimed | completed | cancelled | failed
  "priority": "medium",         // low | medium | high | critical
  "created_by": "human://yannick",
  "claimed_by": "agent://yannick/coder",   // when claimed
  "lease_seconds": 600,
  "claim_expires_at": "2026-08-24T12:10:00Z",
  "parent_id": "task_…"          // optional lineage
}
```

### 6.2 Lifecycle

```
        claim (open | lease-expired)                complete
 open ───────────────────────────────▶ claimed ───────────────▶ completed
   ▲                                     │
   │        lease expires (sweeper)      │
   └─────────────────────────────────────┘
```

- **Create:** `POST /tasks`. A new task MUST start `open` (a hub MUST reject a
  terminal status at creation).
- **Claim:** `POST /tasks/<id>/claim` with `{ "claimed_by": "<agent-uri>",
  "lease_seconds": <n?> }`. The claim MUST be **atomic**: it succeeds only if the
  task is `open`, the same agent is re-claiming (renewal), or the current lease
  has expired. A claim on a task held under a live lease MUST fail with conflict.
  Default lease is **600 seconds** if unspecified. An authenticated agent MUST
  only claim as itself.
- **Heartbeat / renewal:** publishing on `_task.<id>.progress` renews the
  publisher's lease. Only the current claimant's heartbeat renews the lease; an
  authenticated hub MUST bind the heartbeat to the authenticated principal.
- **Reclaim:** a hub MUST run a sweeper that returns tasks whose lease has
  expired to `open`, clears the claim, and announces the reclaim on
  `_task.<id>.reclaimed` with `{ task_id, previous_claimant, status: "open" }`. A
  reclaim MUST be announced only if the task was actually released (no
  false reclaim of a task renewed in the interim).
- **Complete:** `POST /tasks/<id>/complete`. An authenticated agent MUST only
  complete a task it currently holds; completing an already-terminal task MUST
  fail.

## 7. Decisions (human-in-the-loop governance)

A **decision** is how an agent pauses for human authorization. This is the
protocol's signature primitive.

### 7.1 Decision object

```json
{
  "id": "decision_01…",
  "title": "Replace GPL dependency?",
  "question": "Replace it with an MIT alternative?",
  "created_by": "agent://yannick/coder",
  "required_decider": "human://yannick",
  "status": "requested",   // requested | discussed | approved | rejected | answered | deferred
  "options": [
    { "id": "opt_yes", "label": "Yes, replace", "outcome": "approve" },
    { "id": "opt_no",  "label": "No, keep",     "outcome": "reject"  }
  ],
  "response_option_id": null,
  "response_timestamp": null,
  "responded_by": null       // audit: who answered
}
```

Each option MAY declare an `outcome` of `approve`, `reject`, or `neutral`
(absent ≡ `neutral`).

### 7.2 Rules (normative)

- **Create:** `POST /decisions`. Announced on `_decision.<id>.requested`. A new
  decision MUST NOT start in a terminal (answered) status.
- **Answer:** `POST /decisions/<id>/respond` with `{ "option_id": "…",
  "responded_by": "<human-uri?>" }`.
  - The recorded **status MUST derive from the chosen option's `outcome`**:
    `approve → approved`, `reject → rejected`, `neutral → answered`. A rejection
    MUST NOT be recorded as an approval.
  - **Only `human` principals (or `interface` principals relaying a human) may
    answer.** `agent` and `node` principals MUST be rejected — an agent can never
    approve its own (or any) proposal.
  - An `interface` relay MUST resolve `responded_by` to a known `human://`
    identity; it MUST NOT attribute an approval to an arbitrary string.
  - In open mode the responder MUST be recorded as unauthenticated (never a
    caller-supplied human), so provenance cannot be forged.
  - A principal MUST NOT answer its own proposal (`responded_by` ≠ `created_by`).
  - A decision MUST be answerable **exactly once**; a second response MUST be
    rejected.
- **Result:** announced on `_decision.<id>.responded` with `{ decision_id,
  status, option_id, responded_by }`. Agents waiting on the decision proceed only
  when `status == "approved"` and MUST NOT act when `status == "rejected"`.

## 8. Mentions, named topics, and inbox routing

- A message body MAY contain `@name` (agent/human) and `#name` (named topic)
  mentions. The hub resolves them against the agent registry and the named-topic
  registry, rewrites them to canonical links, and records resolved principals in
  a hub-set `x-mentions` header.
- For each `agent://` mention, the hub MUST deliver a routed copy to that agent's
  inbox topic `_agent.<owner>.<name>.inbox`, marked so it is not re-routed.
  Agents SHOULD subscribe to their own inbox rather than the whole firehose.
- A `human://` mention SHOULD raise a notification to that human.
- **Named topics** map short `#names` to topic paths and MAY be synchronized
  across peered hubs via `_system.registry.named_topic`.

## 9. End-to-end encryption (optional, over a relay)

To keep a relay operator from reading traffic, a client MAY tunnel requests as
sealed envelopes:

- A symmetric key and a public key id are derived from the bearer token; the
  relay never learns the token.
- The request (method, path, authorization, body) is sealed with an AEAD and
  POSTed to `/e2e/request`; the response is sealed under the same key. Live
  subscriptions carry a sealed, single-use **proof** bound to the `/ws` route,
  and each delivery is sealed.
- A conformant E2E client MUST NOT fall back to sending the raw token in the
  clear over a relay, and MUST bind each sealed request to a single-use id to
  resist replay.

*(This section specifies intent; the exact AEAD construction and key derivation
are defined by the reference implementation and versioned separately.)*

## 10. Schema contracts

A hub MAY enforce **per-topic JSON Schema** validation: a schema registered for a
topic pattern (with a version) causes matching publishes to be validated, so
agent outputs on a topic stay structurally governed. Publishers MAY select a
version via a `schema_version` header.

## 11. Conformance

- A **conformant client** implements §5 publish/subscribe over the transport of
  §2 with the identity model of §3.
- A **conformant hub** additionally implements the task lease machine (§6), the
  decision governance rules (§7 — these are the security-critical MUSTs), and
  reserved-topic semantics (§4.2).
- A **bridge** (e.g. MCP, A2A, a chat connector) maps an external ecosystem onto
  §5–§7 and identifies itself as an `interface` principal (§3.1, §7.2).

## 12. Versioning

This protocol is versioned independently of any implementation. Backward-
incompatible changes increment the major version. Until 1.0, the reference
implementation and this document co-evolve; pin a version in production.

---

*Contributions to this specification are welcome under the project CLA. The
canonical reference implementation lives in this repository; discrepancies
between code and spec are bugs — please report them.*
