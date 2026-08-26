# MellowMesh Conformance Kit

**"MellowMesh Compatible" should mean something.** This kit turns the protocol's
`MUST` requirements into an automated pass/fail check you can run against *any*
running hub — the reference implementation or your own reimplementation in any
language.

```bash
python3 conformance/mellowmesh_conformance.py --url http://127.0.0.1:40000
```

- **No dependencies** — Python 3 standard library only.
- **Exit code 0** only if every `MUST` passes.
- Prints a per-clause report keyed to [PROTOCOL.md](../PROTOCOL.md).

## What it verifies

The two sections of the protocol that carry the correctness- and
security-critical guarantees:

### §6 — Leased-claim task machine (crash-safe coordination)

| Check | Requirement |
| :--- | :--- |
| T1 | A new task starts in a non-terminal state. |
| T2 | A task cannot be created already-terminal. |
| T3 | Claiming an open task succeeds and records the holder. |
| T4 | A second agent cannot claim a task under a live lease (409). |
| T5 | The holder may re-claim (renew) its own lease. |
| T6 | An expired lease returns the task to `open` — no work is stranded. |
| T7 | A held task can be completed. |
| T8 | Completing an already-terminal task is refused (409). |

### §7 — Human-in-the-loop decision governance

| Check | Requirement |
| :--- | :--- |
| D1 | A new decision starts awaiting an answer (non-terminal). |
| D2 | **A rejection is recorded as `rejected`, never `approved`.** |
| D3 | A decision cannot be answered twice (409). |
| D4 | An unknown option id is refused (400), not silently recorded. |
| D5 | An authenticated **agent** principal can never answer a decision (403). |

D2 is the one that matters most: a hub that records an approval when the human
chose "reject" silently inverts the human's decision. If a hub fails only D2,
treat it as unsafe, not merely non-conformant.

## Verdict levels

- **MellowMesh Compatible (governed)** — every `MUST` passes, including the
  agent-cannot-answer governance check (D5).
- **MellowMesh Compatible (core)** — all task and decision `MUST`s pass, but D5
  was skipped (no agent token supplied).
- **Not conformant** — one or more `MUST`s failed. Exit code 1.

## Options

| Flag | Purpose |
| :--- | :--- |
| `--url` | Base URL of the hub (for a relayed hub, include `/hub/<id>`). |
| `--token` | Bearer token for setup calls, if the hub requires auth. |
| `--agent-token` | A real `agent://` bearer token the hub issued; enables D5. |
| `--reclaim-timeout` | Seconds to wait for T6 lease reclaim (default 25; raise it if the hub's sweep interval is long). |
| `--self-test` | Run the kit's internal self-check (no server needed). |

### Running against the reference hub

```bash
# start a hub with a fast sweep so the reclaim check is quick
MELLOWMESH_SWEEP_INTERVAL_SECS=2 mellowmeshd --port 40100 &
python3 conformance/mellowmesh_conformance.py --url http://127.0.0.1:40100 --reclaim-timeout 20
kill %1
```

## Scope and honesty

- The kit checks the **observable REST behavior**. The reclaim check (T6)
  observes the task returning to `open`; the reference implementation *also*
  announces this on `_task.<id>.reclaimed` over WebSocket, which the reference
  test suite covers separately (a fuller kit could add a WebSocket assertion).
- The governance MUSTs are validated against the reference implementation by the
  in-process integration tests (`crates/mellowmesh-daemon/tests/multi_agent.rs`
  and the decision-governance tests in `server.rs`); this kit runs the same
  checks over REST so *any* implementation can be certified the same way.
- Passing the kit is necessary, not sufficient, for the "MellowMesh Compatible"
  and "Certified" marks — see [../licensing/TRADEMARK.md](../licensing/TRADEMARK.md).

## Continuous integration

`.github/workflows/conformance.yml` builds the reference daemon, boots it, and
runs this kit on every push and pull request, so the reference implementation
stays conformant to its own spec.
