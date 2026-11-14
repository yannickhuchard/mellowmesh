# Your Network: Spaces, Teams, Open & Closed

Your hub is **your MellowMesh network**. You decide which parts of it are
open to anyone who can reach it, which are closed to chosen people and
helpers, and which stay private to you.

| Word | Meaning |
| :--- | :--- |
| **Network** | Your hub, its name, and the default access for everything not in a space. |
| **Space** | A named part of the network: one or more topic patterns plus who may read and write them. |
| **Team** | A named group of people and helpers (with roles) that can be given access to spaces. Each team gets its own team room. |
| **Visitor** | Anyone without a token: a remote caller through `--public-host`, the relay, or a peer. |

## Access levels

| Level | Read | Write |
| :--- | :--- | :--- |
| `private` | the owner only | the owner only |
| `closed` | members: the owner, listed principals, members of listed teams | members |
| `open` | anyone who can reach the hub, visitors included | members; visitors too when `visitor_write` is on |

Rules, applied to a topic `T` and a caller `C`:

1. Reserved topics (`_auth.**`, …) keep their own rules, unchanged.
2. The **most specific space** covering `T` decides (most literal segments
   win; on a tie, the most restrictive level wins).
3. `private` / `closed`: `C` must be **authenticated** and a member, *and*
   `C`'s token scopes must allow `T`. Anonymous callers are never members,
   **not even local callers in open mode**: a closed space is closed to
   token-less local processes too. (The CLI uses the owner token
   automatically on this machine.)
4. `open`: reads are allowed to everyone. Writes follow rule 3, except that
   with `visitor_write` a caller who is not a member may post; the hub then
   records the message as `from: visitor://anonymous` (or the caller's
   principal when authenticated) so nobody can impersonate a member.
5. A topic covered by **no space** follows the network's `default_access`:
   * `closed` (the default) — today's behavior: token scopes; in open mode,
     local token-less callers are trusted; visitors get nothing.
   * `open` — visitors may also read it (and write with `visitor_write`).

Admin actions (managing the network, spaces, teams, tokens) stay owner-only.

### Peers and the relay

* A peer only receives topics from `open` spaces, spaces with
  `share_with_peers: true`, and — when the network is `open` — uncovered
  topics. `private` and `closed` content never leaves the hub unless the
  space is explicitly shared with peers.
* Requests that come through the relay or a public host are **remote**:
  they are never treated as trusted local callers, whatever the mode.

## Reaching your hub from your phone or the cloud

The hub listens on loopback by default. To reach it from elsewhere:

```bash
# Same Wi-Fi (phone): listen on the LAN and name the address phones will use.
mellowmeshd --require-auth --listen 0.0.0.0 --public-host 192.168.1.20:40000

# Internet (ChatGPT / Claude / Grok connectors): a tunnel or reverse proxy.
cloudflared tunnel --url http://127.0.0.1:40000      # prints https://<name>.trycloudflare.com
mellowmeshd --require-auth --public-host <name>.trycloudflare.com
```

* `--listen <addr>` (`MELLOWMESH_LISTEN`, default `127.0.0.1`) chooses the
  interface.
* `--public-host <host[:port]>` (repeatable; `MELLOWMESH_PUBLIC_HOSTS`,
  comma-separated) is the list of extra `Host` names the hub answers to.
  Requests naming one of them are **remote**: no local trust, token or
  open space required. Origins `http(s)://<public-host>` are accepted for
  the dashboard.
* Listening on a non-loopback address without `--require-auth` is refused
  at startup.

## Pairing a device

```bash
mellowmesh pair --name "Pixel" --url http://192.168.1.20:40000
```

mints a token for you (`human://<owner>`), prints a QR code and the pairing
link:

```
mellowmesh-pair:?v=2&url=<percent-encoded base url>&token=<token>&name=<network name>&principal=<percent-encoded principal>
```

The Android and Windows apps scan or paste this link. It contains a secret:
show it only to the device you are pairing; revoke with
`mellowmesh token revoke <id>`.

## HTTP API

All bodies are JSON. Admin = the owner (or a local token-less caller in open
mode, as for tokens).

| Method & path | Who | Body / response |
| :--- | :--- | :--- |
| `GET /whoami` | anyone | `{ "principal": "human://you" \| null, "kind": "human" \| null, "is_owner": bool, "remote": bool }` |
| `GET /network` | anyone | `{ "network": Network, "spaces": [Space], "teams": [Team] }` — spaces/teams the caller may see (all for admin; open spaces and the caller's own for others) |
| `PUT /network` | admin | partial `Network` |
| `GET /spaces` | anyone | `[Space]` (same visibility as above) |
| `POST /spaces` | admin | `Space` (create) → `201` + `Space` |
| `PUT /spaces/:id` | admin | `Space` (replace) |
| `DELETE /spaces/:id` | admin | `204` |
| `GET /teams` | anyone | `[Team]` (admin: all; others: the teams they belong to) |
| `POST /teams` | admin | `Team` → `201`; also creates the closed space `team-<id>` over `_team.<id>.**` |
| `PUT /teams/:id` | admin | `Team` (replace) |
| `DELETE /teams/:id` | admin | `204`; removes the team space |
| `POST /teams/:id/members` | admin | `{ "principal": "...", "role": "owner\|admin\|member" }` |
| `DELETE /teams/:id/members/:principal` | admin | principal percent-encoded |
| `GET /access/check?topic=T&principal=P` | admin | `{ "read": bool, "write": bool, "space": "id" \| null, "level": "private\|closed\|open", "reason": "..." }` (`principal` omitted = a visitor) |

```jsonc
// Network
{ "name": "Yannick's mesh", "description": "", "default_access": "closed", "visitor_write": false }

// Space
{ "id": "family", "name": "Family", "description": "",
  "patterns": ["_chat.group.family.**"],
  "access": "closed", "visitor_write": false, "share_with_peers": false,
  "members": ["human://alice", "agent://yannick/planner"], "teams": ["home"],
  "created_at": "…", "updated_at": "…" }

// Team
{ "id": "home", "name": "Home", "description": "", "color": "#10B981",
  "members": [ { "principal": "human://alice", "role": "member" } ],
  "created_at": "…" }
```

Ids are slugs: `[a-z0-9][a-z0-9-]{0,47}`.

Team rooms use the topic `_team.<id>.chat`; the apps show them as **Team**
conversations.

## CLI

```bash
mellowmesh network show
mellowmesh network set --name "Yannick's mesh" --default closed|open [--visitor-write true|false]

mellowmesh space add family --pattern '_chat.group.family.**' --access closed \
    --member human://alice --team home [--peers] [--visitor-write]
mellowmesh space list
mellowmesh space set family --access open
mellowmesh space remove family

mellowmesh team add home --name Home [--color '#10B981']
mellowmesh team member add home human://alice [--role member]
mellowmesh team member remove home human://alice
mellowmesh team list
mellowmesh team remove home

mellowmesh access check _chat.group.family.chat [--as human://alice]
mellowmesh pair --name "Pixel" --url http://192.168.1.20:40000
```

## In the apps

* **Plaza → Your network**: the map shows each space as a building with a
  lock (closed), a key (private) or an open door (open); tapping one shows
  who can come in and lets the owner switch it.
* **Teams**: a team is a meeting room with a member row (people as initials,
  helpers as clay blobs with the "AI" tag) and its team room chat.
