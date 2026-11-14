# Your Network: Spaces, Teams, Open & Closed

Your hub is **your MellowMesh network**. You decide which parts of it are
open to anyone who can reach it, which are closed to chosen people and
helpers, and which stay private to you.

| Word | Meaning |
| :--- | :--- |
| **Network** | Your hub, its name, and the default access for everything not in a space. |
| **Space** | A named part of the network: one or more topic patterns plus who may read and write them. |
| **Team** | A named group of people and helpers (with roles) that can be given access to spaces. Each team gets its own team room. |
| **Visitor** | Anyone without a token: a remote caller through `--public-host`, the relay, or a peer — and, under `--require-auth`, any token-less caller. |

## Access levels

| Level | Read | Write |
| :--- | :--- | :--- |
| `private` | the owner only | the owner only |
| `closed` | members: the owner, listed principals, members of listed teams | members |
| `open` | anyone who can reach the hub, visitors included | members; visitors too when `visitor_write` is on |

Rules, applied to a topic `T` and a caller `C`:

1. Reserved topics (`_auth.**`, `_decision.**`, `_system.**`) keep their own
   rules, unchanged: token scopes, local token-less callers trusted in open
   mode, nothing for visitors. Spaces never apply to them.
2. The **most specific space** covering `T` decides (most literal segments
   win; on a tie, the most restrictive level wins). Patterns match
   case-insensitively, like topic names.
3. `private` / `closed`: `C` must be **authenticated** and a member, *and*
   `C`'s token scopes must allow `T`. Anonymous callers are never members,
   **not even local callers in open mode**: a closed space is closed to
   token-less local processes too. (The CLI uses the owner token
   automatically on this machine.)
4. `open`: reads are allowed to everyone. Writes follow rule 3, except that
   with `visitor_write` a caller who is not a member may post; the hub then
   records the message as `from: visitor://anonymous` (or the caller's
   principal when authenticated) so nobody can impersonate a member. Visitor
   posts carry the hub-set header `x-mellowmesh-visitor: true`, are never
   routed into agents' inboxes, and are only accepted by `POST /publish`
   (no tasks, decisions, summaries, wiki pages or named topics).
5. A topic covered by **no space** follows the network's `default_access`:
   * `closed` (the default) — today's behavior: token scopes; in open mode,
     local token-less callers are trusted; visitors get nothing.
   * `open` — visitors may also read it (and write with `visitor_write`).

Admin actions (managing the network, spaces, teams, tokens) stay owner-only.

Hub task events (`_task.<id>.created`, `.claimed`, …, see
[task boards](task-board.md)) follow the visibility of their task: a caller
receives them when it may read one of the task's topics (or `_task.<id>`
for a task without topics).

### What a visitor can reach

A token-less caller that is not trusted (remote, or any caller under
`--require-auth`) may call `GET /history`, `/topics`, `/forum`, `/search`,
`/context`, `/ws`, `/whoami`, `/network`, `/spaces`, `/teams` and
`POST /publish`; the answers only contain what the rules above allow. Every
other endpoint answers `401`, as before. `/health`, the dashboard and
`/e2e/request` stay open as before.

### Peers and the relay

* A peer (a subscriber authenticated as a `node://` principal, e.g. with
  `MELLOWMESH_PEER_TOKEN`) only receives topics from `open` spaces, spaces
  with `share_with_peers: true`, and — when the network is `open` — uncovered
  topics. `private` and `closed` content never leaves the hub unless the
  space is explicitly shared with peers. Reserved topics (registry sync on
  `_system.registry.*`) keep their own rules. Its token scopes still apply.
  With the default closed network, keep syncing the forum with
  `mellowmesh space add forum --pattern '_forum.**' --access closed --peers`.
* Requests that come through the relay or a public host, or from another
  machine, are **remote**: they are never treated as trusted local callers,
  whatever the mode.

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
  open space required. An entry without a port matches any port. Origins
  `http(s)://<public-host>` are accepted for the dashboard.
* A connection from a non-loopback address is remote whatever `Host` it
  names. When `--listen` is one specific non-loopback address, the hub also
  listens on `127.0.0.1` for its own loopback traffic (relay link, MCP,
  connectors).
* Listening on a non-loopback address without `--require-auth` is refused
  at startup.

## Pairing a device

```bash
mellowmesh pair --name "Pixel" --url http://192.168.1.20:40000
```

mints a full-access token (`**`) for you (`human://<owner>`), labelled with
the device name, prints a QR code and the pairing link (`--url` is the
address the device will use, `http://` or `https://`):

```
mellowmesh-pair:?v=2&url=<percent-encoded base url>&token=<token>&name=<network name>&principal=<percent-encoded principal>
```

The Android and Windows apps scan or paste this link. It contains a secret:
show it only to the device you are pairing; `pair` prints the token id, and
`mellowmesh token list` shows it with its label. Revoke with
`mellowmesh token revoke <id>`. `pair` must run on the hub's machine (it
uses `owner.token`) or with `MELLOWMESH_TOKEN` set to an owner token. The
token comes from `POST /auth/tokens`, which accepts an optional `"label"`.

## HTTP API

All bodies are JSON. Admin = the owner (or a local token-less caller in open
mode, as for tokens).

| Method & path | Who | Body / response |
| :--- | :--- | :--- |
| `GET /whoami` | anyone | `{ "principal": "human://you" \| null, "kind": "human" \| null, "is_owner": bool, "remote": bool }` |
| `GET /network` | anyone | `{ "network": Network, "spaces": [Space], "teams": [Team] }` — spaces/teams the caller may see (all for admin; open spaces and the caller's own for others; an open space the caller is not a member of comes without its `members` / `teams` lists) |
| `PUT /network` | admin | partial `Network` |
| `GET /spaces` | anyone | `[Space]` (same visibility as above) |
| `POST /spaces` | admin | `Space` (create) → `201` + `Space`; `409` if the id exists; `400` for an invalid body or an unknown team |
| `PUT /spaces/:id` | admin | `Space` (replace; `id` may be omitted) → `Space`; `404` if missing |
| `DELETE /spaces/:id` | admin | `204` |
| `GET /teams` | anyone | `[Team]` (admin: all; others: the teams they belong to) |
| `POST /teams` | admin | `Team` → `201`; also creates the closed space `team-<id>` over `_team.<id>.**`; `409` if the team or that space exists |
| `PUT /teams/:id` | admin | `Team` (replace name, description, color, members; `id` may be omitted) → `Team` |
| `DELETE /teams/:id` | admin | `204`; removes the team space |
| `POST /teams/:id/members` | admin | `{ "principal": "...", "role": "owner\|admin\|member" }` (role defaults to `member`; adding an existing member changes its role) → `Team` |
| `DELETE /teams/:id/members/:principal` | admin | principal percent-encoded → `204` |
| `GET /access/check?topic=T&principal=P` | admin | `{ "read": bool, "write": bool, "space": "id" \| null, "level": "private\|closed\|open", "reason": "..." }` (`principal` omitted = a visitor; `P`'s scopes are the union of its active tokens', or `**` when it has none; `write` is also true for a visitor post, which `reason` says) |

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

Ids are slugs: `[a-z0-9][a-z0-9-]{0,47}`. A team id is at most 43
characters, so that its space id `team-<id>` is a slug too. A space without
a `name` is named after its id; a team without a `color` gets `#10B981`.

Team rooms use the topic `_team.<id>.chat`; the apps show them as **Team**
conversations.

## CLI

```bash
mellowmesh network show
mellowmesh network set --name "Yannick's mesh" --default closed|open [--visitor-write true|false]

mellowmesh space add family --pattern '_chat.group.family.**' --access closed \
    --member human://alice --team home [--peers] [--visitor-write] [--name Family]
mellowmesh space list
mellowmesh space set family --access open      # only the given fields; lists replace
mellowmesh space set family --peers true --visitor-write false
mellowmesh space remove family

mellowmesh team add home --name Home [--color '#10B981'] [--description ...]
mellowmesh team member add home human://alice [--role member]
mellowmesh team member remove home human://alice
mellowmesh team list
mellowmesh team remove home

mellowmesh access check _chat.group.family.chat [--as human://alice]
mellowmesh pair --name "Pixel" --url http://192.168.1.20:40000
```

On this machine these commands (and `mellowmesh token …`) use the owner
token from `owner.token` automatically unless `MELLOWMESH_TOKEN` is set.

MCP clients get one read-only tool, `get_network`, returning
`GET /network` for the caller.

## In the apps

* **Plaza → Your network**: the map shows each space as a building with a
  lock (closed), a key (private) or an open door (open); tapping one shows
  who can come in and lets the owner switch it.
* **Teams**: a team is a meeting room with a member row (people as initials,
  helpers as clay blobs with the "AI" tag) and its team room chat.
