# MellowMesh — Codebase Review Against the Product Vision

*Review date: 2026-08-23. Scope: the whole Cargo workspace (9 crates), docs, skills,
examples, presentation, packaging, and CI, assessed against `PRODUCT_PLAN.md` and
`README.md`. Method summarized at the end.*

---

## Verdict

MellowMesh has a **real, working core** and a **clean architecture** — and a **story that
has run well ahead of the code**. The central coordination loop the whole product rests on
(tasks → crash-safe leases → human decisions → persistent, queryable history) genuinely
works end to end: `mellowmesh demo` runs the full loop, an agent "crash" is reclaimed by the
sweeper after its lease expires, and everything lands as inspectable rows in SQLite. The
build is green and all 65 tests pass.

But the plan declares Phase 0 and Phase 1 **shipped** and Phase 2 **engineering-complete**,
and on close inspection those claims do not hold. The gaps are not cosmetic — they land
squarely on the two things the plan itself names as make-or-break:

- **Principle 3 — "Governance must have teeth… enforced by authentication and scopes, not
  by convention."** In the *shipped default configuration* it has no teeth at all, and a
  correctness bug means a human's **"Reject" is recorded as "approved."**
- **Risk #2 — "Relay security failure… one breach kills the sovereignty brand
  permanently,"** gated on "external review before hosted launch." Acting as a stand-in for
  that review, this pass found **four independently launch-blocking issues** in which the
  *hosted relay operator is the attacker the design names* — and in three of them the
  operator need only read its own logs.

Two further meta-findings matter because the plan raised them as its own guardrails:

- **Principle 5 — "Docs never outrun the code."** They do, in many places (tool counts,
  crate counts, a keypair, a `--open` flag, a `scopes` table, Slack, an auto-discovery
  feature — all documented, none real).
- **Risk #3 — the "solo-builder breadth trap," with a "binding" deprioritization list.**
  The list is not being honored: ~2,000 lines of deprioritized/non-target features and dead
  code ship with zero tests, while the Phase 0 test debt is unpaid.

None of this means the project is off track. It means **the codebase is ahead on breadth
and behind on the depth, safety, and honesty the plan explicitly prioritizes** — exactly the
failure mode the plan was written to prevent. The fixes are mostly well-contained. The rest
of this document is the evidence, ranked by severity, and a prioritized punch list.

---

## What is genuinely there (credit where due)

This is not a hollow demo. The following are real, and several are well-tested:

- **The core loop works.** Verified by running `mellowmesh demo`: two agents register, split
  two tasks, one claims a task under a 5-second lease and "crashes," the daemon sweeper
  reclaims it (`_task.<id>.reclaimed`), the other picks it up and blocks on a human decision.
  `mellowmesh tasks` / `mellowmesh decisions` show the persisted result.
- **Claim-lease core is correct and atomic.** A single conditional `UPDATE` handles
  open-claim / same-agent-renewal / expired-takeover in one statement
  (`crates/mellowmesh-store/src/task_store.rs:116-159`), with store-level tests for conflict,
  expiry, takeover, and renewal (`crates/mellowmesh-store/src/sqlite.rs:546-668`).
- **The topic matcher is solid** — `*` / `**` / `>` with mid-pattern backtracking, Unicode
  case-folding and emoji topics, 7 dedicated tests (`crates/mellowmesh-core/src/topic.rs`).
- **Advertised primitives that actually exist and work:** FTS5 full-text search
  (`sqlite.rs:72`, query at `persistence_impl.rs:160`), per-topic JSON-Schema validation
  with versioning (`handlers/message.rs:210-227`, table at `sqlite.rs:252`), the wiki link
  graph + backlinks + change events (`wiki_store.rs`, `handlers/wiki.rs`), retention
  exemptions for decisions/summaries (`main.rs:172-180`, `sweeper.rs`).
- **MCP is done right.** The stdio server and the daemon's HTTP endpoint share one
  implementation — both import `handle_tool_call` / `list_tools_schema` from
  `crates/mellowmesh-client/src/mcp.rs` (`cli/src/mcp.rs:4`, `daemon/src/handlers/mcp.rs:20`).
  No duplication. Generic JSON-RPC 2.0, so the "any stdio MCP client" claim is credible.
- **E2E transport primitives are correctly built** — ChaCha20-Poly1305 with the key id
  bound as AAD (`core/e2e.rs:78-122`), a genuine single dispatch choke point in the SDK
  (`client/src/lib.rs:135-158`), and the `afdfeba` "no bearer token in HTTP headers" fix is
  real and regression-tested (`client/tests/e2e_no_header_leak.rs`). The problems below are
  in the *protocol design around* these primitives, not the crypto calls themselves.
- **The docs are, in places, more honest than the plan** — `docs/relay.md` never claims a
  keypair; `docs/security.md` documents the true open-by-default behavior.
- **Green CI with teeth:** `cargo fmt --check`, `cargo clippy -D warnings`, and
  `cargo test` all run on every push/PR.

Hold this list in mind while reading what follows — the point is not that the project is bad,
it's that the *claims* are ahead of a genuinely good foundation.

---

## Severity legend

- **S1 Critical** — breaks the product thesis or is launch-blocking for the hosted relay.
- **S2 High** — a documented guarantee is false, or a real security/correctness hole.
- **S3 Medium** — meaningful gap, misleading claim, or latent defect.
- **S4 Low** — polish, hygiene, drift.

---

## Theme A — Governance has no teeth in the shipped default (thesis-breaker)

The North Star is the café approval: *agents propose, the human disposes, and that boundary
is enforced.* This is the one thing no incumbent can copy and the reason the whole product
exists. In the configuration that ships by default, it is not enforced — and one bug makes it
actively unsafe.

### A1 (S1) — A human's "Reject" is persisted as "approved"

`crates/mellowmesh-store/src/decision_store.rs:82-96` hardcodes the status on **every**
response:

```rust
"UPDATE decisions SET response_option_id = ?2, response_timestamp = ?3,
   status = 'approved', responded_by = ?4 WHERE id = ?1"
```

The domain model defines `status` as `"requested" | "approved" | "rejected" | "deferred" | …`
(`crates/mellowmesh-core/src/decision.rs:21`), and the agent skill instructs agents:
*"Once the decision status is `approved`, proceed"* (`skills/mellowmesh/SKILL.md:133`). So a
Telegram **Reject** tap, or any rejection, writes `status = 'approved'`, and a
protocol-following agent proceeds with the action the human tried to block. The chosen
`option_id` is stored correctly, but the *status field the safety guarantee keys on* is
always "approved." This is the single most dangerous finding in the review: it silently
inverts the human's decision. **Fix:** derive status from the selected option's semantics (or
add an explicit approve/reject flag) and write it through.

### A2 (S1) — Default is open mode, and open mode lets an agent approve its own proposal and forge the audit trail

The daemon defaults to **open mode** (`crates/mellowmesh-daemon/src/main.rs:34`, banner at
`:74-76`). The plan describes the opposite — "anonymous mode behind an explicit `--open`
flag, default off after one release" (`PRODUCT_PLAN.md:128`) — but **no `--open` flag exists**
anywhere in the code (the only match for `--open` is in the plan itself).

In open mode, `respond_decision` takes the responder identity **verbatim from the request
body** (`crates/mellowmesh-daemon/src/handlers/decision.rs:113-116`):

```rust
None => payload.responded_by.clone()
    .unwrap_or_else(|| "human://local-unauthenticated".to_string()),
```

There is no principal, so no kind check runs. The `respond_decision` MCP tool
(`client/src/mcp.rs:960`) is exposed to every connected assistant, so the proposing agent can
call it, approve its own decision, **and set `responded_by` to `"human://yannick"`**, writing
a false human into the audit record. In the shipped default, "an agent can never approve its
own proposal" (`README.md:50`) is false. The enforcement is real *only* under
`--require-auth`, which nothing turns on for a local user.

### A3 (S2) — Even under `--require-auth`, enforcement is far narrower than advertised

The plan says the middleware performs a "topic ACL check… on every REST/WS/publish/claim
call" (`PRODUCT_PLAN.md:128`). Token *validation* is universal under `--require-auth`, but
**topic ACL is enforced on 4 of ~37 handlers**: `publish`, `history`, `search`, `forum`, and
WS delivery. The rest — `create_task`, `complete_task` (no claimant check,
`handlers/task.rs:95-106`), `create_decision`, `register_agent`, all schema/wiki/trace/
named-topic/identity-mapping handlers, and `GET /topics` (enumerates the whole namespace,
`handlers/message.rs:392`) — do no scope check. `POST /shutdown` (`server.rs:117`) has **no
admin check**: any authenticated principal, or anyone on localhost in open mode, can kill the
daemon. A token scoped to `_agent.coder.**` can still create tasks, complete anyone's task,
create decisions, and write wiki pages.

### A4 (S2) — Interface-relayed approvals are unverified; the Telegram ramp accepts anyone

Under `--require-auth`, an `interface://` principal may relay a human's answer, but the code
accepts whatever `responded_by` string the interface sends without consulting the
`identity_mappings` table that exists for exactly this purpose
(`handlers/decision.rs:96-101`). Any interface token can attribute an approval to any human.

Worse, the Telegram connector performs **no authorization on inbound messages or button
taps**. The inbound handler destructures the chat object into an unused `_chat`
(`crates/mellowmesh-connectors/src/lib.rs:543`) and never compares it against the configured
`TELEGRAM_CHAT_ID` (which is used only for *outbound* sends). Any Telegram user who finds the
bot can tap **Approve** on a decision card and the daemon records it. The connectors principal
is also minted with `read_scopes: ["**"]` / `write_scopes: ["**"]`
(`main.rs:301-302`) — the one principal that ignores the scoped-token model the README sells.

### A5 (S2) — Peer links bypass the trust layer wholesale

Machine-to-machine peering dials a plain `ws://{addr}/ws` with **no token**
(`crates/mellowmesh-daemon/src/peer.rs:72`) and feeds everything received straight into
`handle_publish` with a fabricated state and no `AuthContext`
(`peer.rs:124-131`). No principal, no scope check, no topic filter. It is wired
unconditionally (`main.rs:272-277`). Any reachable peer can publish to any topic.

### A6 (S3) — Notifications miss human @mentions; and `msg.from` is forgeable

The desktop-notification pipeline is real for decisions and reclaims (`notify.rs`, called
from `decision.rs:38` and `sweeper.rs:82`), but the plan's other trigger — "@mentions
targeting a `human://`" (`PRODUCT_PLAN.md:155`) — fires nothing: the mention fan-out only
handles `agent://` URIs (`handlers/message.rs:112`). Separately, the persisted `from` field
of every message is caller-supplied and never bound to the authenticated principal (true even
under `--require-auth`), so provenance is forgeable across the board — including the
heartbeat-renewal identity (see D-series).

> **Bottom line for Theme A:** the governance loop that justifies the entire product is
> unenforced in the default configuration, forgeable under authentication in several paths,
> and — via A1 — capable of turning a rejection into an approval. This is the highest-leverage
> area to fix and the one most central to the vision.

---

## Theme B — The reach layer is not launch-safe (Risk #2, realized)

The plan gates the hosted relay on "external review before hosted launch" and calls a relay
breach existentially brand-ending. Standing in for that review, these are the issues a
security reviewer would block launch on. In each, **the hosted relay operator is the very
adversary the E2E design was built to defeat.**

### B1 (S1) — The sealed E2E subscription proof is an unbound, replayable, full-privilege credential

The auth middleware opens the sealed proof and then checks **only the timestamp**
(`crates/mellowmesh-daemon/src/auth.rs:149-157`): it never compares the proof's `method` or
`path_and_query` against the actual request, and there is no nonce cache. The proof travels
as three plaintext query parameters (`e2e_kid`, `e2e_nonce`, `e2e_ct`) that the relay reads
and can log (`client/src/lib.rs:275-280`; relay forwards the raw query,
`relay/src/lib.rs:332`). So within the ±120 s window of any `MELLOWMESH_E2E=1 mellowmesh
tail`, a **passive** relay operator can replay that one captured proof against **any**
endpoint:

- `GET …/history?…&e2e_kid=…` → full message history returned as ordinary plaintext JSON
  (sealed mode only affects `/ws` deliveries, `server.rs:189`).
- `POST …/decisions/<id>/respond?…&e2e_kid=…` → **the operator approves the café decision.**
- `POST …/auth/tokens?…&e2e_kid=…` → if the proof came from the owner token, mints a
  permanent full-scope token. Persistence achieved; the window stops mattering.

This *inverts* the guarantee: turning on E2E and running `tail` **hands the operator a
credential** they otherwise would not get. **Fix:** bind method + full path + hub id + a
random `jti` (with a seen-cache) into the sealed proof, verify all of it, and accept proofs
only on the `/ws` route.

### B2 (S1) — Hub-id hijack whenever a hub is briefly disconnected

The relay's link-key check is `hubs.get(&hub_id).map(|h| h.link_key == link_key)
.unwrap_or(true)` (`crates/mellowmesh-relay/src/lib.rs:106-109`). `hubs` holds only
*currently connected* hubs, and the entry is deleted on disconnect (`:205-211`). So **any hub
id that is not connected right now can be claimed by anyone with any link key** — despite the
adjacent comment asserting the opposite. The hub id is in the URL every remote client uses.
On a laptop sleep, network blip, daemon restart, or a relay restart (which drops every hub at
once), an attacker registers the id and every subsequent remote request — **including the
client's `Authorization` header, forwarded verbatim** (`lib.rs:333-336`) — flows to them.
That is direct bearer-token harvesting. **Fix:** a durable relay-side registry binding hub id
→ hashed link key.

### B3 (S1) — The relay learns every link key in plaintext and can impersonate any hub

The link key is generated and stored **in plaintext** in the daemon's SQLite
(`relay_link.rs:41-42`), sent **in plaintext** in the first frame (`relay_link.rs:94-101`),
held **in plaintext** in relay memory (`Hub { link_key: String }`, `lib.rs:41-49`), and
compared with non-constant-time `==` (`lib.rs:108`). The relay is both the verifier and a
holder of the credential, so it can impersonate any hub at will — there is no handshake to
MITM because there is no handshake. This is the "relay never reads your secrets" promise
failing at the link layer.

### B4 (S1) — E2E silently downgrades to plaintext when no token is set

`e2e_enabled()` returns `self.e2e && self.token.is_some()`
(`client/src/lib.rs:128-130`). With `MELLOWMESH_E2E=1` and no token, `send()` takes the plain
branch and the full request crosses the relay in cleartext — no warning, no error — before
the daemon rejects it 401. `mellowmesh publish _forum.general "<secret>"` leaks the secret
first. This contradicts three written "cannot fall back to plaintext" guarantees
(`client/src/lib.rs:132-134`, `docs/relay.md:99`, `PRODUCT_PLAN.md:145`). **Fix:** hard error
at construction when E2E is requested without a token.

### B5 (S2) — SSRF: a scoped agent token can drive the daemon's outbound HTTP

The E2E dispatcher builds `format!("http://127.0.0.1:{port}{}", sealed.path_and_query)`
with no check that the path starts with `/` (`handlers/e2e.rs:100-101`). A crafted
`path_and_query = "@attacker.example.com/x"` resolves to host `attacker.example.com`. **Every
minted token gets an e2e key** (`auth.rs:271`), including narrowly-scoped agent tokens, so a
`_agent.coder.**` agent can make the daemon issue arbitrary outbound requests — an escape from
the Phase-1 scope model. The CLI happens to prepend `/`, but `e2e_request()` is public SDK
surface. **Fix:** validate `path_and_query` begins with `/`.

### B6 (S2) — Non-E2E remote `tail` puts the raw bearer token in the relay URL

On the default (non-E2E) remote subscription, the client appends `?token=<raw>` to the WS URL
(`client/src/lib.rs:283-286`); the relay sees it and any fronting proxy logs it. A test even
**asserts** this passthrough (`relay/tests/forwarding.rs:197`). `docs/relay.md:81` says only
that the operator "can observe the traffic it forwards" — never that it receives your
*credential*.

### B7 (S3) — Crypto-design issues an external reviewer will flag

Key derivation is a bare `SHA256("mellowmesh-e2e-key-v1:" || token)` — no HKDF, no salt, no
directional separation (`core/e2e.rs:30-33`); the same key seals requests, responses, and
every streamed delivery. 96-bit random nonces on a long-lived key across three generators
approach the birthday bound on a busy `tail`; XChaCha20 or a per-connection counter is the
standard remedy. No forward secrecy (key is a pure function of the token, forever). The
`key_id` is a stable public tag the relay sees on every request — a cross-session linkability
leak the docs don't list. Also: `revoke_token` leaves the `e2e_keys` row behind
(`auth_store.rs:90-94`); in-window replay is accepted with no nonce cache, including
`respond_decision` and `claim_task`.

### B8 (S2) — The "owner keypair" that authenticates the relay does not exist

`PRODUCT_PLAN.md:153` says the relay link is "authenticated by the owner keypair. v1: TLS +
token auth." A repo-wide search for any asymmetric primitive
(`ed25519|x25519|keypair|signature|public_key|…`) finds **nothing** but HMAC for Teams
webhooks. There is no keypair, the relay serves plain HTTP (TLS delegated to a reverse proxy,
which is fine but is not "v1: TLS"), and the relay authenticates neither daemons nor clients —
it is an open forwarding proxy for anyone who knows a hub id, with no rate limit or connection
cap. The Phase-1 note deferred the keypair to Phase 2 (`PRODUCT_PLAN.md:122`); Phase 2 is now
"engineering complete" and it still does not exist and is not in the remaining-work list.

> **Bottom line for Theme B:** B1, B2, B3, and B5 are each independently launch-blocking for a
> hosted relay. The E2E *plumbing* is good; the *protocol* around it (proof binding, key
> ownership, downgrade, SSRF) is not yet safe against the operator it names as the adversary.
> This is Risk #2 arriving on schedule — and it is fixable before any hosted launch, which is
> exactly what the plan's external-review gate is for.

---

## Theme C — The story outruns the code (Principle 5 violated)

The plan's Principle 5 is "Docs never outrun the code. Every documented behavior has a test;
no aspirational badges." These are the places the code cannot back the words:

- **"21 tools"** (`README.md:45`, `docs/mcp.md:36` & `:46`) — the code ships **28**
  (`client/src/mcp.rs`, 28 schemas and 28 dispatch arms; two independent audits counted 28).
  The 7 undocumented tools are exactly the wiki + named-topic families the README markets as
  features — advertised as capabilities, invisible as tools. The only guard is
  `assert!(tools.len() >= 20)` (`server.rs:319`), which can't catch drift either way.
- **"eight crates"** (`README.md:83`) — there are **nine**; `mellowmesh-relay`, the Phase-2
  centerpiece, is omitted from the enumeration.
- **Slack** is marketed in four places (`README.md:70` & `:83`, `DESIGN.md:50`,
  `presentation/index.html:320`) and **does not exist** — `ConnectorsManager` constructs
  Discord, Telegram, Teams only (`connectors/src/lib.rs:859`). `docs/connectors.md` correctly
  omits it; the marketing surfaces don't.
- **A `scopes` table** (`PRODUCT_PLAN.md:127`) — not created; scopes are JSON TEXT columns in
  `tokens` (not normalized, no per-scope revocation).
- **`docs/connectors.md:80`** documents `mellowmesh identity add …`; **no such CLI command
  exists** (only the REST route). `docs/cli.md` — billed as "Every command" — omits `token`
  (the Phase-1 headline), `e2e`, and `mcp`.
- **The presentation deck is pre-plan and wrong.** `presentation/` sells the demoted
  "local-first nervous system" positioning with **zero** mention of café/Telegram/mobile/
  universality, and two features that don't exist: office-LAN **auto-discovery** ("auto-
  discovers other nodes on your office network" — no mDNS/zeroconf anywhere) and an install
  flow `npx mellowmesh init` / `mellowmesh start` (neither command exists). It also quotes
  "<1.0 ms" latency against the measured ~20 ms in `docs/performance.md`.
- **`data-model/…md:9`** restricts topics to ASCII `^[a-z0-9._-]+$`, contradicting the
  README's (real, tested) Unicode + emoji topics.

Credit: `docs/performance.md` *does* satisfy the "honest throughput" requirement — it lists
publish (~364/s) and fan-out delivery (~18,200/s) as distinct rows and explains the
difference (`:22-26`), matching the bench harness. That is the model the other docs should
follow.

---

## Theme D — The crown-jewel feature has depth defects; test debt is unpaid

The plan calls claim leases "the sharpest edge in the product." The core UPDATE is correct
(credited above), but the surrounding daemon logic has three real defects:

- **D1 (S2) — False-reclaim race.** The sweeper SELECTs expired tasks, then issues per-row
  UPDATEs whose result is **discarded** (`task_store.rs:195-201`). If a heartbeat renews
  between SELECT and UPDATE, the UPDATE matches 0 rows but the sweeper still publishes
  `_task.<id>.reclaimed` and fires a toast (`sweeper.rs:60-82`). A live-leased task is
  announced open; a second agent can claim on top of the first.
- **D2 (S2) — Heartbeat identity is unauthenticated.** Lease renewal keys on caller-supplied
  `msg.from` (`handlers/message.rs:150`; MCP `agent_id` from tool args,
  `client/src/mcp.rs:622`), never bound to the principal. Anyone with `_task.**` write scope —
  or anyone at all in open mode — can keep another agent's lease alive forever. (`claim_task`,
  by contrast, *does* check impersonation.)
- **D3 (S3) — `in_progress` is a permanent dead-end.** Takeover and the sweeper require
  `status = 'claimed'` (`task_store.rs:133,185`) while renewal treats `in_progress` as live
  (`:148,169`); nothing writes `in_progress`, but `POST /tasks` accepts arbitrary `status`
  with no validation (`handlers/task.rs:18-35`), so a client can create a task that is never
  claimable and never swept.

**Test debt (Phase 0's "every MCP tool has at least one test" — not met):** 65 tests run;
**26 of 28 MCP tools have no dispatch-level test**; there are **no integration-test
directories** in core/store/daemon/cli. Nothing tests the daemon sweeper, the
`_task.<id>.reclaimed` publication, the progress→renewal path, `MELLOWMESH_RETENTION`, or
mention→inbox routing — several of these are headline behaviors. The WASM `test_task_lifecycle`
/ `test_decision_consensus` are `#[wasm_bindgen_test]` and **never run in CI** (which only
does `cargo test`). Latent risk: `Store::new_in_memory` opens a 10-connection pool but
migrates only one connection (`sqlite.rs:32-41`); tests pass only because connections are
checked out serially today — concurrent checkout (which the daemon's pipeline sets up) would
see an empty schema.

Also worth a pass: `std::sync::Mutex::lock().unwrap()` pervades the broadcast/delivery hot
path (`subscription.rs`, `pipeline.rs`, `server.rs:177`), so a single panic under a held lock
poisons it and every later publish panics — the daemon stays up but silently stops delivering.

---

## Theme E — The breadth trap (Risk #3) is real and unmanaged

The plan's Risk #3 is "features outrunning depth," with a deprioritization list it calls
**binding**. It is not being honored. Present in the tree, **all with zero tests**:

| Feature | Plan status | Footprint |
| :--- | :--- | :--- |
| Multi-wiki namespaces | deprioritized (`:178`) | ~1,070 lines (`wiki_store`, `handlers/wiki`, `wiki_sync`, `okf`, tables, 6 REST + 4 MCP) |
| Telemetry / traces | not in any phase | ~430 lines + a 14-column table + a branch in the publish hot path |
| Named-topic P2P registry | deprioritized (`:179`) | ~300 lines + hot-path interception |
| M2M peering (`peer.rs`) | relay "lineage" only | 163 lines, unconditional, **unauthenticated** (A5) |
| Priority lanes / hot buffer / legacy identity | — | ~136 lines of **dead code** (`priority.rs`, `hot_buffer.rs`, `identity.rs` — zero references) |

And the **non-target use cases still ship in the file that configures agents**:
`skills/mellowmesh/SKILL.md:194-208` leads with "Smart Home & Family Coordination,"
"Hobbyist & Content Creation," and "Multi-Agent Travel & Event Planning" — three of the four
named non-targets (`PRODUCT_PLAN.md:66`) — while `:160` still tells agents *"Bind exclusively
to the local port 40000. Never attempt to route messages to external networks"* (the pre-relay
positioning). The README was cleaned in Phase 0; SKILL.md — the artifact actually shipped into
every agent — was not.

The WASM story is also **inverted** vs the plan ("client mode as headline, standalone as
playground"): the compiled crate is 100% standalone (`wasm/src/lib.rs`, 390 lines of
in-memory engine), the demo defaults to standalone, and **the WASM client has zero token
support** — so the browser SDK cannot reach a `--require-auth` or relayed hub at all, despite
Phase 1 claiming "token support in… WASM client."

The `examples/` directory reinforces the drift: the only top-level example (`examples/llmwiki`,
One Piece Devil Fruits + quantum particles) demos the deprioritized multi-wiki feature with
**zero agents, tasks, or decisions**, while the genuinely on-beachhead examples
(`crates/mellowmesh-client/examples/multi_agent_discussion.rs` et al.) are unreferenced.

---

## Prioritized punch list

**Do before anything else (safety / thesis):**
1. **A1** — Stop recording rejections as approvals (`decision_store.rs:92`). One-line class of
   bug, safety-critical. Add a test that a reject persists as `rejected`.
2. **A2** — Make decision integrity hold in the default mode: reject `respond_decision` from
   non-human principals regardless of auth mode, and never take `responded_by` from the body.
   Decide whether "open by default" is still the intended posture; if so, the North Star
   guarantee needs a floor that survives it.
3. **A4/A5** — Enforce `TELEGRAM_CHAT_ID` on inbound; drop the connectors principal from
   `**`/`**` to real scopes; put a token (and scope check) on peer links or gate peering off
   by default.

**Do before any hosted relay launch (Risk #2 gate):**
4. **B1** — Bind method/path/hub-id/jti into the sealed proof; restrict to `/ws`.
5. **B2/B3** — Durable relay-side hub-id → hashed-link-key registry; stop trusting
   `.unwrap_or(true)`.
6. **B4** — Hard-fail E2E-without-token instead of silently downgrading.
7. **B5** — Validate `path_and_query` starts with `/`.
8. Then commission the external review the plan already calls for.

**Depth / correctness:**
9. **D1** — Act on the sweeper UPDATE's row count before announcing a reclaim.
10. **D2** — Bind heartbeat renewal to the authenticated principal.
11. **D3/F7** — Validate `status`/`priority` on task and decision creation.
12. Pay down the Phase-0 test debt: a test per MCP tool, and integration tests for the
    sweeper-reclaim and progress→renewal paths.

**Honesty / focus (cheap, high-trust-per-line):**
13. Fix the counts ("21"→"28", "eight"→"nine"), remove Slack from marketing surfaces, or
    build it — pick one.
14. Purge the non-targets and the "never route externally" line from `SKILL.md`; it's the
    highest-leverage doc fix because it ships into agents.
15. Re-shoot or shelve `presentation/` (it sells the demoted positioning and two nonexistent
    features).
16. Delete the dead code (`priority.rs`, `hot_buffer.rs`, `identity.rs`); decide in or out on
    telemetry/traces/multi-wiki per the "binding" list, and either test them or gate them off.

---

## Method & confidence

Ground truth established directly: `cargo build` (green) and `cargo test --workspace` (65
tests, 0 failures); a full run of `mellowmesh demo` with post-hoc inspection via
`mellowmesh tasks` / `mellowmesh decisions`. The codebase was then audited by three
independently-scoped passes (core/trust, relay/E2E, interfaces/docs/breadth). Every **S1**
finding and the most consequential **S2** findings were re-verified by reading the cited
source directly: A1 (`decision_store.rs:92` + `decision.rs:21`), A2 (`main.rs:34,74-76` +
`decision.rs:113-116`), A5 (`peer.rs:124-131`), B1 (`auth.rs:149-157`), B2
(`relay/src/lib.rs:106-109`), B5 (`e2e.rs:100-101`), and the Telegram gap
(`connectors/src/lib.rs:543`). Findings surfaced by only one pass and not personally
re-read (mostly S3/S4 line references) are flagged by their file:line so they can be checked;
the S1/S2 set is corroborated. Line numbers reflect the state of branch
`claude/codebase-product-vision-review-1qrdi1` on the review date.
