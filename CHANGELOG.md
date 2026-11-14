# Changelog

## Unreleased

### Your network: spaces, teams, open and closed

- **Spaces.** Group topics into spaces that are `private` (only you),
  `closed` (chosen people, helpers and teams) or `open` (anyone who can
  reach your hub may read; visitors may post if you allow it). A closed
  space is closed to token-less local programs too, even in open mode.
- **Teams.** Name a group of people and helpers; each team gets its own
  closed team room (`_team.<id>.chat`).
- **Reach your hub from your phone.** `--listen` and `--public-host`
  (`MELLOWMESH_LISTEN`, `MELLOWMESH_PUBLIC_HOSTS`). Requests through a public
  host, from another machine or through the relay are treated as remote,
  never as trusted local callers. Listening beyond this machine requires
  `--require-auth`.
- **Pair a device in one step.** `mellowmesh pair --name Pixel --url …`
  prints a QR code and a pairing link with a token labelled for that device.
- New: `GET /whoami`, `/network`, `/spaces`, `/teams`, `/access/check`;
  CLI `network`, `space`, `team`, `access check`, `pair`; MCP tool
  `get_network`. Admin and token commands use the owner token automatically
  on the hub's machine.
- **Peers only get shared content.** A peer now receives open spaces,
  spaces shared with peers, and uncovered topics only when the network is
  open. To keep syncing the forum on a closed network, add
  `mellowmesh space add forum --pattern '_forum.**' --access closed --peers`.

### Task boards and a live roadmap

- Tasks can depend on other tasks and belong to a team; the hub records
  when they were created, claimed, started and completed.
- `PATCH /tasks/:id` (CLI `task update`, MCP `update_task`) moves tasks
  between open, claimed, in progress, blocked, completed and cancelled, with
  checked moves and permissions; dependency cycles are refused.
- `GET /tasks/graph` (CLI `task board`) shows lanes, waiting and running
  time, what each task waits on, and the critical chain.
- The hub announces task changes live on `_task.<id>.created`, `.claimed`,
  `.started`, `.completed`, `.updated` and `.cancelled`; clients can no
  longer publish those topics.

## v0.1.1 — 2026-11-14 — security & robustness release

A codebase-wide hardening pass. **Upgrading is recommended for everyone**,
especially if you use the relay, peering, chat connectors or `--require-auth`.

### Security

- **Local daemon is now protected from web pages.** Requests must be addressed
  to a loopback host, and browser requests from any origin other than the
  daemon's own dashboard are refused.
- **Decisions are answered only by the right human.** With authentication on,
  only the decision's `required_decider` (or the hub owner) can answer; agents
  can no longer answer decisions at all (the MCP `respond_decision` tool is
  removed). Chat connectors may relay an answer only for a known human.
- **Stricter permissions.** Identity mappings, schemas and traces are
  owner-only under `--require-auth`; context, summaries, wiki and named topics
  respect token scopes; agents can only register themselves.
- **Trustworthy messages.** System topics (`_decision.*`, `_system.*`,
  `_task.*.reclaimed`) can't be published by clients, and message ids,
  timestamps and senders are set by the hub.
- **Safer client.** Ids in URLs are encoded, HTTP calls have timeouts,
  end-to-end encryption is on by default for remote hubs, and text from other
  agents is cleaned of terminal escape sequences before display.
- **Safer network features.** The relay link only forwards normal API paths,
  peers authenticate with a header and sync `_forum.**` by default, the relay
  remembers hub keys across restarts, and the Teams receiver requires a
  signing key and binds to localhost.
- **Supply chain.** Dependencies updated (`cargo audit` clean), CI actions
  pinned, installers no longer import certificates or register a URL scheme.
- Added [SECURITY.md](SECURITY.md): report vulnerabilities privately via
  GitHub, not in public issues.

### Fixes

- `mellowmesh read <pattern>` and the MCP `read_history` tool now filter on the
  server, so `--limit` counts matching messages.
- `mellowmesh tail` no longer stops when a proxy sends a ping.
- `daemon stop` only stops the local daemon (owner token, then the PID in
  `mellowmeshd.pid`) and never kills unrelated processes.
- Slow live subscribers are disconnected cleanly instead of hanging; revoked
  tokens close their subscriptions.
- The dashboard's empty panels load again; the WASM SDK follows the same
  decision rules as the daemon.

### Performance

- Faster topic matching and history queries, cached registry/schema lookups,
  one shared message copy per broadcast, and a shared HTTP client.

### Behavior changes to check when upgrading

- Tools or dev servers that call the daemon from another browser origin now
  get `403`.
- Decision options need an explicit outcome to resolve to approved/rejected:
  `--option "approve:Ship it"` / `--option "reject:Hold"` in the CLI,
  `{"label", "outcome"}` in MCP.
- The MCP server exposes **27 tools** (no `respond_decision`).
- Peers without an explicit pattern sync `_forum.**` only.
- Client-supplied message ids are ignored.
- Wiki pages must be `.md`; traces are capped at 24 h.
