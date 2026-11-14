# Changelog

## Unreleased — security & robustness release

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
