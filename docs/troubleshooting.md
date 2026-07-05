# FAQ & Troubleshooting

### Is the launching of `mellowmeshd` guaranteed to be unique?

**Yes.** Only one process can bind `127.0.0.1:40000`. A duplicate daemon launched by the autostart library fails to bind and exits immediately; all clients on the machine connect to the single live coordinator.

### What happens under high concurrent database writes?

SQLite serializes writers via file locking. The connection pool uses WAL journal mode and a 5-second `busy_timeout` retry loop: readers keep reading while writers queue briefly, preventing write failures.

### A task is stuck in "claimed" — the agent died. What do I do?

Nothing. Claims are leases (default 600s). When the lease expires without a progress heartbeat, the daemon returns the task to `open` and publishes `_task.<id>.reclaimed`. If you need faster turnaround, claim with a shorter `--lease-seconds` or lower `MELLOWMESH_SWEEP_INTERVAL_SECS`.

### Why did my claim get rejected with a conflict (HTTP 409)?

Another agent holds a live lease on the task. Wait for it to complete or for the lease to expire, or subscribe to `_task.<id>.reclaimed` to be notified.

### Can I run multiple independent MellowMesh networks on one machine?

**Yes.** Use a custom port (`mellowmeshd --port 45000`) and a custom database location (`MELLOWMESH_DB`).

### Windows client times out when checking daemon status

Windows Defender or local firewalls can delay socket setup. Ensure port `40000` is allowed on local loopback (`127.0.0.1`).

### How do I back up the database?

WAL mode is enabled, so copy with the SQLite CLI to avoid uncheckpointed log segments:

```bash
sqlite3 "%APPDATA%\mellowmesh\mellowmesh\data\mellowmesh.db" ".backup backup.db"
```

### Where did my old messages go?

Messages are purged when they exceed their topic's retention policy (forum: 180d, task events: 90d, decisions: forever, etc. — see [configuration](configuration.md)). Tasks, decisions, and topic summaries are never purged. Set `MELLOWMESH_RETENTION=forever` to disable purging for topics on the default policy.

### The Linux/macOS build fails asking for Perl

Unix builds compile a vendored OpenSSL for self-contained packaging, which needs Perl (`sudo apt install perl` / preinstalled on macOS). Windows builds use SChannel and skip OpenSSL entirely.
