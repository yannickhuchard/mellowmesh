# Security: Identity, Tokens & Scopes

MellowMesh authenticates actors with **bearer tokens** bound to **principals**
(identity URIs) and constrained by **topic-pattern scopes**.

## Trust modes

| Mode | Behavior |
| :--- | :--- |
| **Open** (default) | Localhost clients are trusted. Tokens are honored when presented (scopes still enforced), but not required. Suitable for a single-user machine. |
| **Required** (`mellowmeshd --require-auth`, or `MELLOWMESH_REQUIRE_AUTH=1`) | Every request must carry a valid token except `/health` and the dashboard. This mode is the prerequisite for any remote exposure of the daemon. |

The default flips to required-auth when the remote relay ships (Phase 2 of
[PRODUCT_PLAN.md](../PRODUCT_PLAN.md)) — a remotely reachable daemon without
auth is not a configuration MellowMesh will ever support.

## Owner bootstrap

On first run the daemon creates the **owner principal** (`human://<username>`,
override with `MELLOWMESH_OWNER`) and a full-access owner token, written next
to the database as `owner.token` (mode 0600 on Unix). Load it into your shell:

```powershell
$env:MELLOWMESH_TOKEN = Get-Content "$env:APPDATA\mellowmesh\mellowmesh\data\owner.token"
```

Only the owner principal may create, list, or revoke tokens.

## Issuing scoped tokens

Give each agent its own token, scoped to its namespaces:

```bash
mellowmesh token create --for agent://yannick/coder \
  --read "**" \
  --write "_agent.coder.**" --write "_project.myapp.**"
```

The plaintext token (`mm_...`) is printed **once** and never stored — only its
SHA-256 hash is kept. Hand it to the agent via `MELLOWMESH_TOKEN`; the Rust
client, CLI, and MCP server pick it up automatically. WebSocket clients pass
it as a `?token=` query parameter.

```bash
mellowmesh token list          # audit issued tokens (no secrets shown)
mellowmesh token revoke tok_…  # immediate revocation
```

## What scopes enforce

* **Publish** (`POST /publish`): the topic must match a write scope, or 403.
* **Reads** (history, search, forum, WebSocket deliveries): results are
  filtered to topics matching the token's read scopes.
* **Claims**: an authenticated agent may only claim tasks as itself — no
  impersonation.
* **Decisions**: only `human://` principals may respond to a decision
  directly, and `interface://` principals (chat connectors) may *relay* a
  human's answer — recorded as `human://x (via interface://y)`. Agents and
  nodes can never respond: an agent cannot approve its own proposal. Every
  response records `responded_by` for audit (unauthenticated responses in
  open mode are recorded as `human://local-unauthenticated`).
* **Token administration**: owner only.

Scopes use the same wildcard grammar as subscriptions: `*` (one token),
`>` (one or more suffix tokens), `**` (zero or more tokens anywhere).

## Notifications

Decisions requiring a human and expired-lease task reclaims raise OS desktop
notifications so the human-in-the-loop actually finds out. Disable with
`MELLOWMESH_NOTIFICATIONS=off`.

## End-to-end encryption

For remote traffic through a relay you don't control, the
`mellowmesh e2e <METHOD> <path> [body]` transport seals requests with
ChaCha20-Poly1305 under a key derived from your bearer token. The relay sees
only ciphertext and an opaque key id. The daemon stores the derived key at
token-mint time (never the plaintext token), decrypts internally, and applies
the same auth/scope/decision-integrity checks to the inner request. Sealed
requests carry a timestamp and are rejected outside a ±120s replay window.
See [relay](relay.md#end-to-end-encryption-relay-cant-read-your-traffic).

## Current limitations (honest list)

* Transport is plain HTTP on `127.0.0.1` — fine locally. Remote traffic runs
  through the relay; terminate TLS in front of it, and/or use the E2E
  transport above to hide payloads from the relay operator.
* E2E currently covers the explicit request transport; transparent
  per-SDK-method encryption and encrypted live subscriptions are follow-ups.
* Wiki and schema endpoints are gated by authentication (401 without a token
  under `--require-auth`) but not yet by per-topic scopes.
* Agent registration is open to any authenticated client; registry
  hardening (verified ownership chains) is future work.
