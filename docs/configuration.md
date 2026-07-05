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
