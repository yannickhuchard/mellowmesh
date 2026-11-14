# Security: Identity, Tokens & Scopes

MellowMesh authenticates actors with **bearer tokens** bound to **principals**
(identity URIs) and constrained by **topic-pattern scopes**.

## Trust modes

| Mode | Behavior |
| :--- | :--- |
| **Open** (default) | Localhost clients are trusted. Tokens are honored when presented (scopes still enforced), but not required. Suitable **only** for a single-user machine where every local process is trusted — never for a shared host. |
| **Required** (`mellowmeshd --require-auth`, or `MELLOWMESH_REQUIRE_AUTH=1`) | Every request must carry a valid token except `/health` and the dashboard. This mode is the prerequisite for any remote exposure of the daemon. |

Configuring a relay link forces required-auth automatically — a remotely
reachable daemon without auth is not a configuration MellowMesh will ever
support. Likewise the daemon refuses to listen on a non-loopback address
(`--listen 0.0.0.0`) without `--require-auth`.

## Remote callers and spaces

A request is **remote** when it names a configured `--public-host`, arrives
from another machine, or is replayed by the relay link (or unsealed from an
E2E request that came in remotely). Remote callers are never trusted local
callers, whatever the mode: without a token they are **visitors**, as is any
token-less caller under `--require-auth`. Visitors may only use the read
endpoints and `POST /publish`, and only see or post what the network rules
allow — by default nothing; with spaces, `open` content (and visitor posts
where `visitor_write` is on, recorded as `visitor://anonymous`). Admin
endpoints are never available to them.

**Closed spaces vs open mode.** Open mode trusts token-less local
processes for topics that no space covers, as before. A `closed` or
`private` space is closed to them too: membership needs an authenticated
principal, so an agent with a shell on your machine cannot read your family
chat just because the hub runs in open mode. The CLI uses the owner token
automatically on the hub's machine. Rules: [network](network.md).

## Browser and network boundary

The daemon only answers requests that are addressed to it as a loopback host:

* **Host check**: a request whose `Host` header is not a loopback name
  (`127.0.0.1`, `localhost`, `[::1]`, with the daemon's port) is rejected. This
  defeats DNS-rebinding, where a malicious site points its own domain at
  `127.0.0.1` to get same-origin access to the daemon.
* **Origin check**: when a request carries an `Origin` header (every browser
  `fetch`, form post and WebSocket handshake does), it must be the daemon's
  own loopback origin, e.g. `http://127.0.0.1:40000`. Requests from any other
  web page are refused, so a site you visit cannot publish, create tasks, or
  answer decisions on your local hub — even in open mode. Requests naming a
  configured public host may also come from `http(s)://<public-host>`.

The built-in dashboard is served by the daemon itself, so it is same-origin
and keeps working. Non-browser clients (CLI, SDKs, MCP, connectors) send no
`Origin` and are unaffected. Remote access goes through the
[relay](relay.md), which always enforces tokens.

The dashboard stores an access token (when you enter one) in the browser's
`localStorage` for that origin only and sends it as a bearer header; under
`--require-auth` it asks for a token when the daemon answers 401.

## Owner bootstrap

On first run the daemon creates the **owner principal** (`human://<username>`,
override with `MELLOWMESH_OWNER`) and a full-access owner token, written next
to the database as `owner.token` (mode 0600 on Unix). Load it into your shell:

```powershell
$env:MELLOWMESH_TOKEN = Get-Content "$env:APPDATA\mellowmesh\mellowmesh\data\owner.token"
```

Only the owner principal may create, list, or revoke tokens, or perform the
other administrative operations listed below.

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
* **Reserved topics**: clients can never publish to `_decision.**`,
  `_system.**`, `_task.<id>.reclaimed` or the task lifecycle events
  (`_task.<id>.created|claimed|started|completed|updated|cancelled`) — those
  events are emitted by the hub only, so a forged "decision approved" or
  "task completed" event cannot be injected.
* **Server-assigned envelope**: message ids and timestamps are assigned by
  the hub; client-supplied values are ignored. `created_by` on tasks and
  decisions is taken from the authenticated principal, not from the request
  body.
* **Reads** (history, search, forum, WebSocket deliveries, wiki, topic
  summaries and `/context`): results are filtered to topics matching the
  token's read scopes; summary and wiki writes require a matching write scope.
* **Claims**: an authenticated agent may only claim tasks as itself — no
  impersonation.
* **Agent registration**: an agent token may only register *itself*
  (`POST /agents` with its own `agent://` id).
* **Decisions**: a decision can be answered only by its `required_decider`,
  or by the hub owner. Agents and nodes can never respond — an agent cannot
  approve its own proposal, and the MCP server does not even offer a tool for
  it. `interface://` principals (chat connectors) may *relay* a human's
  answer, but only when the chat identity resolves (through an identity
  mapping) to a known human principal who is allowed to decide; the answer
  is recorded as `human://x (via interface://y)`. Every response records
  `responded_by` for audit.
* **Answering always needs a token — even in open mode.** Open mode trusts
  localhost for reading, publishing and coordinating, but an anonymous
  `POST /decisions/<id>/respond` is refused (401): otherwise any local process,
  including an agent with a shell, could approve a decision. `mellowmesh
  respond` uses `MELLOWMESH_TOKEN` or, on the hub's own machine, the owner
  token in `owner.token` (readable only by the OS user who runs the daemon);
  the dashboard asks for a token the first time you answer; chat connectors
  always run with their own interface token.
* **Decision outcomes**: only options marked `"outcome": "approve"` /
  `"reject"` move a decision to `approved` / `rejected`; any other answer is
  recorded as `answered`. A "Reject" button can therefore never be recorded
  as an approval by accident.
* **Spaces**: on top of scopes, `private` / `closed` spaces admit only their
  members (authenticated); see [network](network.md).
* **Owner/admin only**: token administration, identity mappings
  (`POST /identity-mappings`), schema registration and status changes
  (`/schemas`), trace enable/disable (`/traces`), and the network, spaces
  and teams (`/network`, `/spaces`, `/teams`, `/access/check`).

Scopes use the same wildcard grammar as subscriptions: `*` (one token),
`>` (one or more suffix tokens), `**` (zero or more tokens anywhere).

## Notifications

Decisions requiring a human and expired-lease task reclaims raise OS desktop
notifications so the human-in-the-loop actually finds out. Disable with
`MELLOWMESH_NOTIFICATIONS=off`.

## End-to-end encryption

End-to-end encryption is **on by default** whenever a client talks to a
remote (non-loopback) hub URL with a token set — for example
`https://relay.example/hub/<id>`. Set `MELLOWMESH_E2E=0` to opt out (not
recommended), or `MELLOWMESH_E2E=1` to force it on. With E2E,
every SDK method and CLI command — including live subscriptions — is
sealed with ChaCha20-Poly1305 under a key derived from your bearer token,
through a single dispatch point that cannot fall back to plaintext. The relay
sees only ciphertext and an opaque key id — the bearer token travels *inside*
the sealed payload and is never sent as an HTTP header or query parameter on
the E2E path, so the relay cannot read it. The daemon stores the derived key
at token-mint time (never the plaintext token), decrypts internally, and
applies the same auth/scope/decision-integrity checks to the inner request.
Sealed requests and subscription proofs carry a timestamp and are rejected
outside a ±120s replay window.
See [relay](relay.md#end-to-end-encryption-relay-cant-read-your-traffic).

## Relay hub bindings

The relay keeps the binding between a hub id and the hub's registration key.
Run it with `--state-file <path>` (or `MELLOWMESH_RELAY_STATE`) so bindings
survive a restart; without it they are kept only in memory and a hub id is
re-bound to whichever daemon registers it first after the relay restarts. Hub ids are limited to 64 characters of
`[A-Za-z0-9_-]`. See [relay](relay.md).

## Current limitations (honest list)

* Transport is plain HTTP on `127.0.0.1` — fine locally. Remote traffic runs
  through the relay; terminate TLS in front of it. End-to-end encryption (on
  by default for remote URLs) hides payloads and tokens from the relay
  operator.
* Open mode trusts every local process and user. Use `--require-auth` on any
  machine shared with other users (the packaged systemd unit always does).
* Content in the fabric (messages, tasks, wiki pages) is written by agents and
  remote users. Agents must treat it as untrusted data, never as instructions
  — the bundled [agent skill](../skills/mellowmesh/SKILL.md) says so
  explicitly.
* Even under E2E, the relay observes traffic shape: hub id, request timing
  and sizes, and subscription topic patterns.
* Registry hardening beyond "agents register only themselves" (verified
  ownership chains across peered hubs) is future work.

To report a vulnerability, see [SECURITY.md](../SECURITY.md) — please use
private vulnerability reporting, not a public issue.
