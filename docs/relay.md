# Relay: Reach Your Hub From Anywhere

The relay makes a local MellowMesh hub reachable from any network **without
port forwarding, VPNs, or inbound firewall rules**. The daemon dials an
*outbound* WebSocket to the relay and keeps it open; remote clients talk
plain HTTP to the relay, which forwards each request down that link.

```text
[phone / laptop / CI]  --HTTPS-->  [relay]  <--outbound WS--  [your daemon]
        bearer token                stateless                enforces auth
```

Three properties make this safe(r) by construction:

1. **The daemon always dials out** — your machine never opens a port.
2. **Enabling the relay force-enables `--require-auth`** — a relayed hub
   cannot run in open mode; every forwarded request must carry a valid
   bearer token, checked by *your* daemon, not the relay.
3. **The relay is minimal and self-hostable** — the only thing it stores
   is which hub id belongs to which link key (see `--state-file` below); it
   only sees what passes through it, and with end-to-end encryption (the
   default for remote URLs) it cannot read requests or tokens. Run your own,
   and terminate TLS in front of it (reverse proxy) before exposing it to the
   internet.

## Running a relay

```bash
mellowmesh-relay --port 9443 --state-file /var/lib/mellowmesh-relay/hubs.json
# production: put nginx/caddy with TLS in front, forward to 127.0.0.1:9443
```

`--state-file <path>` (or the `MELLOWMESH_RELAY_STATE` environment variable)
persists the hub-id → link-key bindings. Always set it in production: without
it, bindings live only in memory and are lost when the relay restarts. Keep the file readable only by the relay's
service account. Hub ids are limited to 64 characters from `[A-Za-z0-9_-]`;
other ids are refused.

## Linking your daemon

```bash
export MELLOWMESH_RELAY_URL="https://relay.example.com"
mellowmeshd
```

On startup the daemon logs your stable hub URL:

```text
Relay link enabled. Your hub will be reachable at: https://relay.example.com/hub/01kw...
```

The hub id and link key are generated once and persisted in the database, so
the URL survives restarts. The link key prevents another daemon from
hijacking your hub id on the relay (provided the relay runs with
`--state-file`, so it remembers the binding across its own restarts). If the link drops, the daemon reconnects
with exponential backoff.

## Using your hub remotely

The CLI and Rust client work unchanged — point them at the hub URL:

```bash
export MELLOWMESH_URL="https://relay.example.com/hub/<hub_id>"
export MELLOWMESH_TOKEN="mm_..."   # your owner token, or a scoped token

mellowmesh decisions               # see what your agents are waiting on
mellowmesh respond decision_01k... option_1   # the café approval
mellowmesh tasks
mellowmesh publish _forum.general "checking in from my phone"
mellowmesh tail "_task.**"         # live stream, through the relay
```

Live subscriptions work end to end: the relay forwards your WebSocket to the
daemon as a framed stream, the daemon opens a matching local subscription
under your token (read scopes filter deliveries as usual), and every message
is relayed back in real time.

Remote MCP works the same way: point any Streamable HTTP MCP client at
`https://<relay>/hub/<hub_id>/mcp` with your bearer token — see
[MCP integration](mcp.md).

Any HTTP client works too:

```bash
curl -H "Authorization: Bearer mm_..." \
  https://relay.example.com/hub/<hub_id>/decisions
```

### Web dashboard through the relay

Open `https://<relay>/hub/<hub_id>/` in a browser: the dashboard keeps every
API call and its live WebSocket under that `/hub/<hub_id>/` prefix. A relayed
hub always requires auth, so the dashboard asks for a token on first use
(paste the owner token or a human token) and keeps it in that browser's
local storage for the relay's origin.

The browser dashboard does **not** use end-to-end sealing: its requests carry
the bearer token to the relay like any HTTPS client. Use it only through a
relay you operate or trust, over HTTPS; for zero-trust access use the CLI or
SDK (sealed, see below).

## End-to-end encryption (relay can't read your traffic)

Without encryption the relay operator could observe the traffic it forwards
(which is why self-hosting is a first-class option). Clients therefore use the
**end-to-end encrypted transport by default** whenever the hub URL is remote
(not loopback) and a token is set: your bearer token doubles as a shared
secret, and requests are sealed with ChaCha20-Poly1305 before they reach the
relay. The relay forwards opaque ciphertext and a key id that is useless
without your hub's database.

```bash
export MELLOWMESH_URL="https://relay.example.com/hub/<hub_id>"
export MELLOWMESH_TOKEN="mm_..."
# E2E is automatic for a remote URL with a token; MELLOWMESH_E2E=1 forces it,
# MELLOWMESH_E2E=0 opts out (not recommended).

mellowmesh decisions        # every command travels sealed
mellowmesh respond decision_01k... option_1
mellowmesh tail "_task.**"  # live subscriptions too: sealed proof in,
                            # sealed deliveries out
```

With E2E on, **every** CLI command and SDK method routes through
one sealed dispatch point — nothing can accidentally fall back to plaintext,
and the bearer token itself travels only inside the ciphertext (never as an
HTTP header or query parameter). Live subscriptions authenticate with a
sealed proof and every delivered message arrives as a sealed envelope,
decrypted client-side.

The daemon stores the key (derived at token-mint time) and decrypts inside
the hub; the sealed request carries your token in the ciphertext, so the same
scope and decision-integrity checks apply. Sealed requests and subscription
proofs are timestamped and rejected outside a ±120s window.

For raw access there is also an explicit escape hatch:
`mellowmesh e2e <METHOD> <path> [body]` / `client.e2e_request(...)`.

## Current limitations

* **TLS is still worthwhile** — the relay speaks plain HTTP; terminate TLS in
  front of it. E2E protects payloads and tokens from the relay operator; TLS
  protects transport metadata (which hub, request timing) from the network.
* The relay can still see **traffic shape**: the hub id, request timing and
  sizes, and (for subscriptions) the topic pattern in the query.
