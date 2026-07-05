# Configuration

MellowMesh works out-of-the-box with zero configuration, binding to `127.0.0.1:40000`.

## CLI Arguments

### Daemon (`mellowmeshd`)

```text
Usage: mellowmeshd [OPTIONS]

Options:
  -p, --port <PORT>      The port to bind to [default: 40000]
      --db <DB>          SQLite database path override
      --peer <PEER>      Peer daemon address (optionally addr=pattern)
      --node-id <ID>     Node identity override
  -h, --help             Print help
```

### Client CLI (`mellowmesh`)

Use the global `-p` / `--port` argument to target specific daemon instances:

```bash
mellowmesh --port 45000 status
```

## Environment Variables

| Variable | Description | Default |
| :--- | :--- | :--- |
| `MELLOWMESH_PORT` | Port for the daemon to bind or for the client/MCP server to target. | `40000` |
| `MELLOWMESH_DB` | Absolute path to the SQLite storage database. | OS AppData folder (below) |
| `MELLOWMESH_WIKIS` | Wiki namespace-to-directory mappings (see [CLI guide](cli.md)). | `default:./wiki` |
| `MELLOWMESH_RETENTION` | Global retention override for topics that match no per-topic policy rule (e.g. `30d`, `12h`, `forever`). | default policy (`7d`) |
| `MELLOWMESH_SWEEP_INTERVAL_SECS` | Interval of the background sweep that releases expired task-claim leases. Retention purges run on the same loop, at most hourly. | `10` |
| `MELLOWMESH_TOKEN` | Bearer token used by the CLI, Rust client, and MCP server to authenticate against the daemon. See [security](security.md). | unset (anonymous) |
| `MELLOWMESH_REQUIRE_AUTH` | Set to `1` to require a valid token on every request (same as `mellowmeshd --require-auth`). | unset (open mode) |
| `MELLOWMESH_OWNER` | Owner identity created on first run (e.g. `human://yannick`). | `human://<os-username>` |
| `MELLOWMESH_NOTIFICATIONS` | Set to `off` to disable desktop notifications for decisions and task reclaims. | enabled |
| `MELLOWMESH_RELAY_URL` | Relay server to dial for remote reachability (e.g. `https://relay.example.com`). Enabling this **forces `--require-auth`**. See [relay](relay.md). | unset |
| `MELLOWMESH_URL` | Client-side: full base URL of a remote hub (`https://relay.example.com/hub/<id>`), used by the CLI/SDK instead of `127.0.0.1`. | unset (local) |
| `TELEGRAM_TOKEN`, `TELEGRAM_CHAT_ID` | Telegram connector: decision cards with inline approve/reject buttons + message bridging. See [connectors](connectors.md). | unset (idle) |
| `DISCORD_TOKEN`, `DISCORD_CHANNEL_ID` | Discord connector: decision announcements with `!approve` + message bridging. | unset (idle) |
| `TEAMS_WEBHOOK_URL`, `TEAMS_OUTGOING_WEBHOOK_KEY` | Teams webhook bridge. | unset (idle) |
| `MELLOWMESH_CONNECTOR_MOCKS` | Set to `1` to run connectors in demo simulation mode when no credentials are configured. | off |
| `RUST_LOG` | Logging filter level (`trace`, `debug`, `info`, `warn`, `error`). | `info` |

### Default Database Locations

* **Windows**: `%APPDATA%\mellowmesh\mellowmesh\data\mellowmesh.db`
* **Linux**: `~/.local/share/mellowmesh/mellowmesh.db`
* **macOS**: `~/Library/Application Support/mellowmesh/mellowmesh.db`

## Task Claim Leases

Every task claim carries a lease (default **600 seconds**, settable per claim via `lease_seconds` in the REST/MCP/CLI claim calls). Publishing progress on `_task.<id>.progress` renews the publisher's lease. The daemon's sweep loop releases expired claims back to `open` and publishes a `_task.<id>.reclaimed` event. This guarantees that a crashed or hung agent cannot strand a task.

## Message Retention

Messages are subject to per-topic retention policies (e.g. forum messages 180 days, agent heartbeats ephemeral, decision messages forever). An hourly purge deletes messages older than their topic's retention, including their full-text-search index entries. Tasks, decisions, and topic summaries are stored in separate tables and are **never** purged by message retention.

Retention strings use the format `<number><unit>` with units `s`, `m`, `h`, `d` — or `forever` to disable purging.

## Storage Engine Pragmas

MellowMesh configures SQLite connection pragmas at launch to resolve concurrency write conflicts:

```sql
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
```

* **WAL (Write-Ahead Logging)**: concurrent readers see consistent snapshots while a writer commits.
* **Busy Timeout**: queries retry for up to 5 seconds before returning a lock error.
* **Synchronous Normal**: disk syncs restricted to checkpoints — fast throughput without compromising ACID compliance.
