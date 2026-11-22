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

The hub listens on loopback by default. To pair a phone, `mellowmesh pair`
(below) opens the way by itself. To set it up by hand instead:

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
mellowmesh pair
```

That is all: no flags, no restart. It starts the hub if needed, prints a QR
code in the terminal, and waits until the phone connects:

```
Pair "Phone" with Yannick's mesh: scan this code with the MellowMesh app
<QR code>
Or paste this pairing link in the app:
mellowmesh-pair:?v=2&url=http%3A%2F%2F192.168.1.20%3A40000&token=…
Hub address: http://192.168.1.20:40000 (local network)
The code works for 15 min (until 10:15) and contains a secret: show it only to the device you are pairing.
Waiting for Phone to connect... (Ctrl+C to stop waiting)
✓ Phone connected
```

Without `--url`, the hub pairs over the **local network**: it finds this
computer's LAN IPv4 address (the interface of the default route; private
ranges preferred), starts listening on `<lan-ip>:<port>` right away (in
addition to `127.0.0.1`), answers to that address as a public host, and
remembers `lan_access: true` so the listener comes back after a restart, on
the then-current address. The phone must be on the same Wi-Fi. On Windows,
the first time, Windows Firewall asks whether `mellowmeshd` may accept
connections: click **Allow access** (`pair` reminds you).

Options:

* `--name Pixel` names the device (default `Phone`; it labels the token).
* `--url https://<name>.trycloudflare.com` pairs through a tunnel or reverse
  proxy instead of the LAN (when the phone is not on the same network, or no
  LAN address can be found). The url's host is added to the public hosts at
  once and remembered, so the tunnel works without restarting the hub.
* `--no-wait` prints the code and exits without waiting.

Ctrl+C stops waiting; the code stays valid until it expires. The QR code
sets its own colors (black on white) when the terminal supports it, and
otherwise draws light modules in the text color, which reads correctly on
the usual dark terminal background (`NO_COLOR` forces the latter).

What `pair` does (`POST /pair`): it mints a full-access token (`**`) for
you (`human://<owner>`), labelled with the device name and marked as a
**device** token. A device token that is not used within **15 minutes**
expires (the code on screen is short-lived); once the phone has used it, it
stays valid until you remove it. The link is:

```
mellowmesh-pair:?v=2&url=<percent-encoded base url>&token=<token>&name=<network name>&principal=<percent-encoded principal>
```

The Android and Windows apps scan or paste it. It contains a secret: show it
only to the device you are pairing.

```bash
mellowmesh devices                 # paired and pending devices
mellowmesh devices remove tok_…    # its token stops working at once
```

`pair` and `devices` must run on the hub's machine (they use `owner.token`)
or with `MELLOWMESH_TOKEN` set to an owner token. Turn the LAN listener off
with `PUT /network { "lan_access": false }`.

Security: connections on the LAN listener come from other machines, so they
are always **remote** — never trusted as local callers; a token or an open
space is required, exactly as with `--listen` and `--require-auth`. That is
why the hub may open it at runtime even without `--require-auth`. Only the
owner may pair or manage devices.

## HTTP API

All bodies are JSON. Admin = the owner's own token. A local caller without
a token may read and publish in open mode, but is never the owner (`401`;
`403` for another principal's token).

| Method & path | Who | Body / response |
| :--- | :--- | :--- |
| `GET /whoami` | anyone | `{ "principal": "human://you" \| null, "kind": "human" \| null, "is_owner": bool, "remote": bool }` |
| `GET /network` | anyone | `{ "network": Network, "spaces": [Space], "teams": [Team] }` — spaces/teams the caller may see (all for admin; open spaces and the caller's own for others; an open space the caller is not a member of comes without its `members` / `teams` lists) |
| `PUT /network` | admin | partial `Network`; `"lan_access": true / false` opens / closes the LAN listener (`503` if it cannot open) |
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
| `POST /pair` | admin | `{ "name"?: "Phone", "url"?: "https://…" }` → `201` + `PairResponse`; no `url` = LAN mode; `400` for a bad url or body; `503` + `{ "error" }` when no LAN address is found or the LAN listener cannot bind |
| `GET /pair/:token_id` | admin | `{ "token_id", "name", "connected": bool, "last_seen": ts \| null, "expired": bool }` (`connected` once the token authenticated a request; `expired` once it expired unused); `404` if unknown or removed |
| `GET /devices` | admin | `[ { "token_id", "name", "created_at", "last_seen": ts \| null, "connected": bool, "expires_at": ts \| null } ]`: active device tokens (paired or pending), newest first |
| `DELETE /devices/:token_id` | admin | revoke → `204`; `404` if unknown, not a device or already removed |
| `GET /access/check?topic=T&principal=P` | admin | `{ "read": bool, "write": bool, "space": "id" \| null, "level": "private\|closed\|open", "reason": "..." }` (`principal` omitted = a visitor; `P`'s scopes are the union of its active tokens', or `**` when it has none; `write` is also true for a visitor post, which `reason` says) |

```jsonc
// Network (lan_address is read-only: the LAN listener's ip:port, shown to
// the owner only; null when the hub does not listen on the LAN)
{ "name": "Yannick's mesh", "description": "", "default_access": "closed", "visitor_write": false,
  "lan_access": true, "lan_address": "192.168.1.20:40000" }

// PairResponse (POST /pair)
{ "link": "mellowmesh-pair:?v=2&url=…", "url": "http://192.168.1.20:40000",
  "token_id": "tok_…", "name": "Phone", "network": "Yannick's mesh",
  "principal": "human://yannick", "expires_at": "2026-11-15T10:15:00Z",
  "lan": { "enabled": true, "address": "192.168.1.20:40000", "listening": true } }

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
mellowmesh pair [--name Pixel] [--url https://<name>.trycloudflare.com] [--no-wait]
mellowmesh devices
mellowmesh devices remove tok_…
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
* **Settings → Your machines** (Android): a network can be spread over
  several computers (peered hubs). The phone keeps a key for each machine
  it was paired with; **Add a machine** pairs another one (same QR or short
  code) and never replaces the first. One machine is *in use* for the
  Plaza, boards and to-dos (switch with the *On … ▾* chip); Calls and the
  offline screen look at every machine, and a message written for a machine
  that rests leaves when that machine answers again.
* **Chats** (Android, second tab): one conversation space over every
  network. A strip of network segments (*All*, then each network with its
  colour and count) filters the page; **Your groups** are the person's own
  groups of Mellows, from any network (*+ Group*, or hold a Mellow to put it
  in a group; kept on the phone); **Everyone** lists every Mellow of every
  machine — one tap opens the conversation on the machine it lives on, with
  no machine to switch; **Conversations** are those of every machine, newest
  first. Messages that arrive rise in with a spring, and soft sounds mark
  sent, received and joined (Settings → Sounds; silent when the phone is).
  The last place of *Everyone* is **New**: finding or making a Mellow (the
  shops) lives here now. The strip ends with **+**: *Your networks*.
* **Journeys** (Android, third tab; the bottom bar is Plaza · Chats ·
  Journeys · To do): the paths Mellows help along. Every task on a
  `journey.<slug>` topic, on any machine, is a station: done (filled),
  under way (a ring), *needs you* (a question linked to the step, amber),
  next (dotted, with the Mellow who will help). The same journey on two
  networks is one path (a customer's move and the movers' step). The line
  fills up to where the work is when a journey opens; a finished step
  chimes. When the same steps come back three times with the same Mellows,
  the journey says so: *Make it a routine* keeps that round, *Run it again*
  creates the next round of steps, each given to the same Mellow and
  waiting for the one before. A conversation shows a small ribbon with the
  journey its Mellow is on. Live task events on the machine in use refresh
  the journeys within about a second.
* **Your networks** (Android, from the Chats strip or the Plaza's teams
  building): one card per network with its colour, machines and Mellows,
  and the person's role there. Administration is scoped: *Invite an AI to
  …* and *Manage …* name the network and go through a machine of that
  network where the phone holds the owner's key; a network where the
  person is a member offers no admin. The network screen's header always
  says which network is being changed. The doors are separate: *Pair a
  computer of mine*, *Join a network someone shared*, *Find or make a
  Mellow*.
* **Everywhere** (Android, from the second machine on, and the default
  there): the Plaza shows every Mellow of every machine in one list, each
  tagged with its machine (and its network when there are several), with
  *Needs you* summed over all machines and one dot per machine. A machine
  is *answering*, *resting*, *address lost* (something answers, not the hub:
  a closed tunnel) or *key refused*; the last two get a card with *Pair
  again* and *Remove*, and never empty the view. A Mellow reached through
  two addresses of the same hub is listed once, and the other address says
  *same as …*. Pick one machine in the chip to scope the Plaza to it again.
* **Settings → Notes between AIs** (Android; `mellowmesh mail`; MCP
  `set_agent_mail_policy`): who invited AIs can write to — *any helper's
  mailbox* (default: the network organizes itself), *only these mailboxes*,
  or *no mailbox* — with a different rule for one AI when needed. It applies
  at once, per machine, and the phone can set the network rule on every
  machine in one go. A helper is free to ignore a note, and no AI can write
  in another helper's outbox. See [security](security.md).
* **Settings → Email** (Android; `mellowmesh email`): the machine's hub gets
  a mailbox of its own — address, app password, *Check* — and a list of the
  people who may give it commands by email. Each machine has its own
  mailbox (two hubs never read the same inbox). See
  [connectors](connectors.md#email--the-hubs-own-mailbox).
