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
3. **The relay is stateless and self-hostable** — it stores nothing and
   only sees what passes through it. Run your own, and terminate TLS in
   front of it (reverse proxy) before exposing it to the internet.

## Running a relay

```bash
mellowmesh-relay --port 9443
# production: put nginx/caddy with TLS in front, forward to 127.0.0.1:9443
```

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
hijacking your hub id on the relay. If the link drops, the daemon reconnects
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

## End-to-end encryption (relay can't read your traffic)

By default the relay operator can observe the traffic it forwards (which is
why self-hosting is a first-class option). For a stronger guarantee, use the
**end-to-end encrypted transport**: your bearer token doubles as a shared
secret, and requests are sealed with ChaCha20-Poly1305 before they reach the
relay. The relay forwards opaque ciphertext and a key id that is useless
without your hub's database.

```bash
export MELLOWMESH_URL="https://relay.example.com/hub/<hub_id>"
export MELLOWMESH_TOKEN="mm_..."
export MELLOWMESH_E2E=1

mellowmesh decisions        # every command now travels sealed
mellowmesh respond decision_01k... option_1
mellowmesh tail "_task.**"  # live subscriptions too: sealed proof in,
                            # sealed deliveries out
```

With `MELLOWMESH_E2E=1`, **every** CLI command and SDK method routes through
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
