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
```

Any HTTP client works too:

```bash
curl -H "Authorization: Bearer mm_..." \
  https://relay.example.com/hub/<hub_id>/decisions
```

## Current limitations

* **No live subscriptions through the relay yet** — `mellowmesh tail` and
  WebSocket subscriptions require a local connection; poll with `read` /
  `decisions` / `tasks` remotely. Streaming pass-through is planned.
* **TLS is your reverse proxy's job** — the relay itself speaks plain HTTP.
* **End-to-end encryption** (relay cannot read payloads) is the planned v2;
  today the relay operator can observe traffic, which is why self-hosting is
  a first-class option.
