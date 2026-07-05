# Performance

## How to reproduce

The numbers below come from the `mellowmesh-bench` crate on a Windows development machine over the local loopback interface. They are indicative, not certified; rerun on your own hardware:

```bash
cargo run --release -p mellowmesh-bench
```

## Benchmark: concurrent publish + fan-out

**Load**: 10 concurrent publishers × 100 messages (1,000 published messages) with 50 concurrent WebSocket subscribers, producing 50,000 total deliveries.

| Metric | Result | What it measures |
| :--- | :--- | :--- |
| Total deliveries | 50,000 (100% success, 0 lock contentions) | End-to-end correctness under load |
| **Publish throughput** | **~364 messages/sec** | Concurrent HTTP writes through schema validation + persistence |
| **Delivery throughput** | **~18,200 deliveries/sec** | In-memory WebSocket fan-out (each publish reaches 50 subscribers) |
| Mean latency | ~20 ms | Publish timestamp → WebSocket receipt |

Read these numbers together: the fan-out figure is high because each published message is delivered to 50 subscribers from memory; the sustained *ingest* rate is the publish throughput. For the intended workload — a handful of humans and agents coordinating on one machine — both are orders of magnitude above what is needed.

## Storage characteristics

* SQLite in WAL mode with a 5s busy timeout: concurrent readers never block on the single writer; writers queue briefly instead of failing.
* An hourly retention sweep bounds database growth by purging messages past their per-topic retention (see [configuration](configuration.md)).
