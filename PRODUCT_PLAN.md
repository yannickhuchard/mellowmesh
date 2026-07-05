# MellowMesh Product Plan

**From local coordination daemon to the universal fabric for piloting your agent fleet.**

This document defines the product pitch, positioning, business model, and the engineering
roadmap that transforms the current codebase into that product. It supersedes the
positioning sections of the README, which should be rewritten to match (see Phase 0).

---

## 1. The Pitch

### One-liner
> **MellowMesh: your agents, reachable from anywhere. One fabric behind every interface.**

### End-user pitch
You run AI agents everywhere — Claude Code on your workstation, Codex in a repo,
a research agent in the background, bots in Discord. Today each one is trapped in the
window that launched it. MellowMesh gives them a shared fabric: agents claim tasks,
report progress, hand work to each other, and pause for your approval — and *you* stay
in command from whatever interface you happen to be in. Kick off work from your IDE,
check progress from Slack, approve a deployment from your phone at a café. The
interfaces are interchangeable; the work is permanent; the hub is yours.

### Business pitch
Every platform vendor (OpenAI, Anthropic, Microsoft, Google) is fighting to make *their*
interface the center of the user's agent life. None of them will build the neutral layer
that treats their interface as one interchangeable ramp among many. MellowMesh owns the
structurally unoccupied position: the **vendor-neutral, user-owned coordination hub**,
with a **paid reachability service** on top. The open-source hub earns trust and
distribution (one-line MCP install into every AI-native tool); the hosted relay —
"reach your fleet from anywhere without networking setup" — is the recurring-revenue
convenience, self-hostable for those who insist. This is the proven
Tailscale / Home Assistant / Syncthing model applied to agent coordination.

### Message hierarchy
1. **Headline — Universality:** pilot your sea of agents from any interface.
2. **Trust — Sovereignty:** the hub runs on your machine; every task, decision, and
   artifact lives in a SQLite file you own. No vendor in the middle.
3. **Hook — Triviality:** `claude mcp add mellowmesh -- mellowmesh mcp` and `@agent`
   mentions that work identically in every interface. Using the fabric must feel like
   driving on a road: you don't think about it.

"Local-first" is hereby demoted from headline to trust guarantee. It is the *reason to
believe* the universality promise (a universal fabric owned by someone else's cloud is
just another walled garden) — not the promise itself.

---

## 2. Who It's For

### Beachhead (now)
**The multi-agent developer:** runs 2+ AI coding agents on one machine, coordinates them
today with markdown files and hope. Feels the pain daily, adopts via one-line MCP
install, evangelizes in public. Everything in Phases 0–2 serves this person.

### Expansion (in order)
1. **The away-from-keyboard operator** — same person, away from the desk: monitoring and
   approving from phone/Slack/Telegram. Unlocked by the relay (Phase 2).
2. **The small team** — shared fabric across 2–10 humans and their fleets, via P2P
   peering + team relay (post-Phase 4).
3. **The governed enterprise** — schema contracts, audit trails, SSO. A narrative for
   later, not a build target now.

### Explicit non-targets (for now)
Family/smart-home coordination, hobbyist content bots, travel planning, generic
enterprise message bus replacement. These dilute positioning; remove them from the
README's use-case sections.

---

## 3. North Star: "The Café Approval"

One scenario proves the entire thesis, end to end:

> An agent working overnight on your workstation hits a sensitive step and creates a
> Decision. Your phone buzzes. From Telegram (or Claude Mobile via remote MCP), you read
> the context, tap **Approve**. The agent resumes. You were never at your desk.

Work started in one interface, was governed from another, the fabric sat in the middle,
and the human stayed sovereign over both. No incumbent can demo this across vendor
boundaries. Every phase below is sequenced to make this demo real, then effortless.

**Definition of done:** a 30-second screen recording of this loop, reproducible by a new
user within 15 minutes of `winget install mellowmesh` / `brew install mellowmesh`.

---

## 4. Product Principles

1. **The hub is free and yours, forever.** MIT core, SQLite you can copy. Monetize
   reach and convenience, never data custody.
2. **Standards before connectors.** Speak MCP (already done) and A2A (bridge planned)
   to inherit whole ecosystems; hand-build only the ramps standards can't reach
   (Telegram, Slack, mobile web). Never enter the fifteen-connector treadmill.
3. **Governance must have teeth.** "Agents propose, humans approve" is enforced by
   authentication and scopes, not by convention and honor.
4. **Depth before breadth.** One moment of magic, polished, beats five half-features.
5. **Docs never outrun the code.** Every documented behavior has a test; no aspirational
   badges.

---

## 5. Roadmap

Four phases, roughly six months. Phases 2 and 3 deliberately overlap. Each phase lists
the concrete codebase changes mapped to the existing workspace.

### Phase 0 — Reposition & Harden (weeks 1–3) — ✅ SHIPPED (July 2026)
*Goal: the story matches this plan and the core loop survives real users.*

| Change | Where |
| :--- | :--- |
| **README rewrite**: lead with pitch + demo GIF; split the 1,000-line monolith into `docs/` (install, MCP, CLI, API, governance, wiki). Remove hardcoded build/perf badges; state publish vs. fan-out throughput honestly. Cut family/travel use cases. | `README.md`, new `docs/` |
| **Claim leases** — the sharpest edge in the product: add `claim_expires_at` + `lease_seconds` to `Task`; heartbeat renewal via `publish_progress`; daemon sweeper task auto-releases expired claims back to `open` and publishes `_task.<id>.reclaimed`. | `crates/mellowmesh-core/src/task.rs`, `crates/mellowmesh-store` (migration), `crates/mellowmesh-daemon/src/handlers/task.rs`, MCP tools in `crates/mellowmesh-cli/src/mcp.rs`, `skills/mellowmesh/SKILL.md` |
| **Retention policy**: configurable message TTL / archival compaction (`MELLOWMESH_RETENTION`); summaries and decisions exempt. | `crates/mellowmesh-store`, daemon config |
| **Test debt**: integration tests for full task lifecycle (incl. lease expiry), decision flow, wildcard matcher edge cases, mention routing. Target: every MCP tool has at least one test. | all crates; CI in `.github/workflows` |
| **`mellowmesh demo`**: CLI command that spawns two mock agents dividing tasks, publishing progress, and blocking on a decision — the 5-minute aha moment with zero setup. | `crates/mellowmesh-cli` |

### Phase 1 — Trust Layer (weeks 3–8) — ✅ SHIPPED (July 2026)
*Goal: governance with teeth; the prerequisite for opening the hub to the world.*
*Note: shipped with random bearer tokens (SHA-256 hashed at rest) instead of a
keypair; the owner keypair moves to Phase 2 where the relay actually needs it.*

| Change | Where |
| :--- | :--- |
| **Principals & tokens**: first-run generates the owner's `human://` identity + keypair. Agents/interfaces get issued scoped bearer tokens (`mellowmesh token create --for agent://yannick/coder --write "_agent.coder.**,_project.myapp.**"`). New tables: `principals`, `tokens`, `scopes`. | new module `crates/mellowmesh-daemon/src/auth.rs`, `crates/mellowmesh-store`, CLI commands |
| **Enforcement middleware**: Axum layer validating tokens on every REST/WS/publish/claim call; topic ACL check against the matcher. Localhost anonymous mode remains available behind an explicit `--open` flag for backwards compatibility, default off after one release. | `crates/mellowmesh-daemon/src/server.rs`, `handlers/*` |
| **Decision integrity**: only authenticated `human://` principals may call `respond_decision`; responses recorded with principal + timestamp for audit. An agent can no longer approve its own proposal. | `handlers/decision.rs`, `mcp.rs` |
| **SDK updates**: token support in Rust client, WASM client, MCP server env (`MELLOWMESH_TOKEN`). | `crates/mellowmesh-client`, `crates/mellowmesh-wasm` |

### Phase 2 — Reach Layer (weeks 8–16) — *the universality bet* — 🔨 IN PROGRESS
*Goal: the café approval works.*
*Shipped so far: desktop notification pipeline; the `mellowmesh-relay` crate
(outbound-only dial, stable hub URLs, link-key hijack protection, forced
require-auth on relayed hubs); client/CLI remote mode via `MELLOWMESH_URL`;
live subscriptions through the relay (`mellowmesh tail` works remotely);
remote MCP over Streamable HTTP (`/hub/<id>/mcp`), with the tool logic
shared between the stdio server and the daemon endpoint — the café approval
works today over REST, streaming, and MCP. Remaining: E2E encryption,
Telegram ramp with inline approve/reject, the demo video.*

| Change | Where |
| :--- | :--- |
| **`mellowmesh-relay` crate**: lightweight rendezvous server; the local daemon dials *outbound* WebSocket (no port forwarding), authenticated by the owner keypair. v1: TLS + token auth; v2: end-to-end encryption so the hosted relay never reads payloads. Self-hostable from day one. | new crate `crates/mellowmesh-relay`; daemon-side link in `crates/mellowmesh-daemon/src/peer.rs` lineage |
| **Remote MCP endpoint**: expose the MCP server over Streamable HTTP through the relay, so Claude Mobile / claude.ai / ChatGPT connectors can join the fabric as first-class interfaces. | `crates/mellowmesh-cli/src/mcp.rs` → shared MCP core, daemon route |
| **Notification pipeline**: decisions and `@mentions` targeting a `human://` trigger (a) OS desktop notifications, (b) push via the flagship mobile ramp. Without this, human-in-the-loop is a queue nobody reads. | new `crates/mellowmesh-daemon/src/notify.rs`, connectors |
| **Telegram connector (flagship mobile ramp)**: two-way — decision cards with inline Approve/Reject buttons, `@mention` replies route back into the fabric. Promote Discord/Teams/Slack connectors from monolithic `lib.rs` to modules; keep them functional but don't expand them yet. | `crates/mellowmesh-connectors` (split `lib.rs` into `discord.rs`, `telegram.rs`, `teams.rs`, `slack.rs`) |
| **Ship the demo**: record the café-approval video; it becomes the top of the README and the website. | — |

### Phase 3 — Human Surface (weeks 12–20, overlaps Phase 2)
*Goal: humans govern from a surface built for governing.*

| Change | Where |
| :--- | :--- |
| **Dashboard rebuild**: replace the 89 KB `ui.html` monolith with a small SPA (keep the glassmorphic brand kit). Hero view = **Approval Inbox**; then live task board (open/claimed/leased/done), fleet view (agents + heartbeats), forum, wiki. Mobile-responsive, served by the daemon, reachable through the relay. | `crates/mellowmesh-daemon` static assets (new `dashboard/` built artifact) |
| **Decision UX**: one-tap approve/reject with full lineage context (`parent_id` chain) rendered inline — make the lineage feature visible, it's a differentiator. | dashboard, `handlers/decision.rs` |

### Phase 4 — Ecosystem & Distribution (weeks 16–24)
*Goal: the fabric is one command away in every ecosystem that matters.*

| Change | Where |
| :--- | :--- |
| **Python SDK** (`mellowmesh` on PyPI): thin REST/WS client mirroring the Rust SDK — the agent ecosystem is Python-heavy; this likely doubles addressable integrations. | new `sdk/python/` |
| **TypeScript SDK**: publish the existing WASM pkg properly as `@mellowmesh/client` (client mode as the headline; standalone mode documented as a playground). | `crates/mellowmesh-wasm/pkg` |
| **A2A bridge**: translate A2A task/message semantics ↔ MellowMesh topics, making the hub a personal node in the A2A ecosystem rather than a competitor to it. | `crates/mellowmesh-connectors/src/a2a.rs` |
| **Distribution**: crates.io, npm, PyPI, winget, Homebrew, apt repo; MSI/DMG/DEB already exist — wire them to package managers. Submit `SKILL.md` and the MCP server to the MCP/skill registries and awesome lists. | `.github/workflows/release.yml`, packaging scripts |

### Deliberately deprioritized
WASM **standalone** mode as a product (keep as demo/playground), multi-wiki namespaces,
named-topic P2P registry expansion, additional chat connectors beyond the flagship,
enterprise SSO/audit. Revisit after the beachhead loop retains users.

---

## 6. Business Model

| Tier | What | Price signal |
| :--- | :--- | :--- |
| **Hub (OSS)** | Daemon, CLI, MCP server, SDKs, dashboard, self-hosted relay. MIT. | Free forever — this *is* the sovereignty promise and the distribution engine. |
| **MellowMesh Link** | Hosted relay: zero-config "reach your fleet from anywhere", push notifications, remote MCP endpoint. | ~$6–10 / month individual |
| **Link for Teams** (later) | Shared fabrics across humans, mesh management, roles, priority relay. | per-seat |
| **Enterprise** (narrative only, for now) | Audit export, schema governance, SSO, support. | — |

Rule: never charge for data custody or core coordination primitives. Charge for reach,
convenience, and team surface area.

## 7. Success Metrics

- **Time-to-first-coordinated-task** < 10 minutes from install (measure via `mellowmesh demo` completion).
- **Weekly active hubs** (opt-in anonymous ping) — the adoption line.
- **Remote approvals per week** — *the* universality metric; every one is a café moment.
- **Interfaces per hub** — > 1 distinct interface type per active hub validates the thesis.
- **Hub retention at week 4** — depth over breadth check.
- **Link conversion** — % of hubs with ≥1 remote approval that subscribe within 30 days.

## 8. Top Risks & Mitigations

1. **Platform absorption** (vendors ship native multi-agent orchestration + mobile
   control). *Mitigation:* neutrality — be the layer that coordinates *across* vendors;
   bridge A2A and MCP instead of competing; ship the café demo before they do.
2. **Relay security failure** — one breach kills the sovereignty brand permanently.
   *Mitigation:* outbound-only dial, scoped tokens from Phase 1, E2E encryption in relay
   v2, self-hosting option from day one, external review before hosted launch.
3. **Solo-builder breadth trap** — the historical failure mode of this codebase is
   features outrunning depth. *Mitigation:* Section 5's deprioritized list is binding;
   nothing enters a phase until the prior phase's definition of done is met.
4. **Standards drift** (MCP transport changes, A2A evolution). *Mitigation:* keep the
   MCP layer thin over the REST API; track spec releases in CI against pinned versions.

---

*Author: Yannick Huchard — plan drafted July 2026, from the product review conversation.*
