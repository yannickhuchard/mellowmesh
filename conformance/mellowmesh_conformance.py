#!/usr/bin/env python3
"""
MellowMesh Protocol Conformance Kit
===================================

Checks a running MellowMesh hub against the `MUST` requirements of the
coordination protocol (PROTOCOL.md), sections 6 (the leased-claim task machine)
and 7 (human-in-the-loop decision governance). These are the security- and
correctness-critical guarantees that make an implementation trustworthy.

Dependency-free: standard library only, so anyone reimplementing MellowMesh in
any language can verify their hub with:

    python3 mellowmesh_conformance.py --url http://127.0.0.1:40000

Exit code is 0 only if every MUST passes. An implementation that passes may
describe itself as "MellowMesh Compatible" (see licensing/TRADEMARK.md).

Notes
-----
* Lease reclaim is checked twice: T6 observes the REST state transition (the
  task returns to `open`), and T9 subscribes over WebSocket (a minimal stdlib
  RFC 6455 client is built in) and asserts the `_task.<id>.reclaimed`
  announcement with its payload.
* D6 checks that a client cannot publish a forged decision outcome on the
  hub-reserved `_decision.<id>.responded` topic.
* A check whose setup call fails (e.g. the hub cannot create a task) is
  reported as ERROR rather than FAIL, and a harness problem in the WebSocket
  check (T9) is reported instead of aborting the run.
* Governance checks that require an authenticated *agent* principal (D5) run
  only when you pass `--agent-token` (a real bearer token the hub issued to an
  `agent://` principal). Without it they are reported as SKIPPED, and the
  verdict is "core" rather than "governed".
"""

import argparse
import base64
import json
import os
import socket
import ssl
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

PASS, FAIL, SKIP, ERROR = "PASS", "FAIL", "SKIP", "ERROR"


class Report:
    def __init__(self):
        self.rows = []

    def add(self, clause, ref, name, status, detail=""):
        self.rows.append((clause, ref, name, status, detail))

    def counts(self):
        c = {PASS: 0, FAIL: 0, SKIP: 0, ERROR: 0}
        for _, _, _, s, _ in self.rows:
            c[s] = c.get(s, 0) + 1
        return c

    def print(self):
        icon = {PASS: "PASS", FAIL: "FAIL", SKIP: "skip", ERROR: "ERR "}
        print("\n  MellowMesh Protocol Conformance\n  " + "-" * 62)
        for clause, ref, name, status, detail in self.rows:
            line = f"  [{icon[status]}] {clause:<4} {name}"
            print(line)
            if detail and status in (FAIL, ERROR):
                print(f"         └─ {ref}: {detail}")
        c = self.counts()
        print("  " + "-" * 62)
        print(
            f"  {c[PASS]} passed · {c[FAIL]} failed · {c[ERROR]} errored · "
            f"{c[SKIP]} skipped"
        )


class Hub:
    """Thin REST client for a MellowMesh hub."""

    def __init__(self, base, token=None, timeout=10):
        self.base = base.rstrip("/")
        self.token = token
        self.timeout = timeout

    def call(self, method, path, body=None, token=None):
        url = f"{self.base}{path}"
        data = json.dumps(body).encode() if body is not None else None
        headers = {"Content-Type": "application/json"}
        tok = token if token is not None else self.token
        if tok:
            headers["Authorization"] = f"Bearer {tok}"
        req = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as r:
                return r.status, _parse(r.read())
        except urllib.error.HTTPError as e:
            body = e.read() if e.fp else b""
            return e.code, _parse(body)
        except Exception as e:  # connection refused, DNS, timeout…
            return None, str(e)

    # -- protocol operations -------------------------------------------------
    def create_task(self, title, created_by="human://conformance", status=None,
                    priority="medium"):
        body = {"id": "", "title": title, "created_by": created_by,
                "status": status or "open", "priority": priority,
                "topics": [], "required_capabilities": [], "artifacts": [],
                "decisions": []}
        return self.call("POST", "/tasks", body)

    def list_tasks(self):
        return self.call("GET", "/tasks")

    def get_task(self, task_id):
        status, data = self.list_tasks()
        if status != 200 or not isinstance(data, list):
            return None
        for t in data:
            if t.get("id") == task_id:
                return t
        return None

    def claim(self, task_id, claimed_by, lease_seconds=None, token=None):
        body = {"claimed_by": claimed_by}
        if lease_seconds is not None:
            body["lease_seconds"] = lease_seconds
        return self.call("POST", f"/tasks/{task_id}/claim", body, token=token)

    def complete(self, task_id, token=None):
        return self.call("POST", f"/tasks/{task_id}/complete", None, token=token)

    def create_decision(self, title, created_by="agent://conformance/proposer",
                        decider="human://conformance", status="requested",
                        options=None):
        body = {"id": "", "title": title, "question": title,
                "created_by": created_by, "required_decider": decider,
                "status": status, "options": options or []}
        return self.call("POST", "/decisions", body)

    def list_decisions(self):
        return self.call("GET", "/decisions")

    def get_decision(self, decision_id):
        status, data = self.list_decisions()
        if status != 200 or not isinstance(data, list):
            return None
        for d in data:
            if d.get("id") == decision_id:
                return d
        return None

    def respond(self, decision_id, option_id, responded_by=None, token=None):
        body = {"option_id": option_id}
        if responded_by is not None:
            body["responded_by"] = responded_by
        return self.call("POST", f"/decisions/{decision_id}/respond", body,
                         token=token)


class Subscription:
    """Minimal RFC 6455 WebSocket client (stdlib only) for `GET /ws?pattern=`.

    Enough for conformance: client handshake, masked client frames, unmasked
    server text frames (with fragmentation), ping/pong, close.
    """

    def __init__(self, base, pattern, token=None, timeout=10):
        u = urllib.parse.urlsplit(base.rstrip("/"))
        secure = u.scheme in ("https", "wss")
        host = u.hostname
        port = u.port or (443 if secure else 80)
        query = {"pattern": pattern}
        if token:
            query["token"] = token
        path = f"{u.path}/ws?{urllib.parse.urlencode(query)}"

        sock = socket.create_connection((host, port), timeout=timeout)
        if secure:
            sock = ssl.create_default_context().wrap_socket(sock, server_hostname=host)
        self.sock = sock
        key = base64.b64encode(os.urandom(16)).decode()
        handshake = (
            f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
            "Upgrade: websocket\r\nConnection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        sock.sendall(handshake.encode())
        head = b""
        while b"\r\n\r\n" not in head:
            chunk = sock.recv(4096)
            if not chunk:
                raise ConnectionError("connection closed during WebSocket handshake")
            head += chunk
        head, self.buf = head.split(b"\r\n\r\n", 1)
        status_line = head.split(b"\r\n", 1)[0].decode("latin-1")
        if " 101 " not in status_line + " ":
            raise ConnectionError(f"WebSocket upgrade refused: {status_line}")

    def _read_exact(self, n):
        while len(self.buf) < n:
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError("WebSocket closed")
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out

    def _send(self, opcode, payload=b""):
        mask = os.urandom(4)
        header = bytes([0x80 | opcode])
        n = len(payload)
        if n < 126:
            header += bytes([0x80 | n])
        elif n < 65536:
            header += bytes([0x80 | 126]) + struct.pack("!H", n)
        else:
            header += bytes([0x80 | 127]) + struct.pack("!Q", n)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.sock.sendall(header + mask + masked)

    def _read_frame(self):
        """Read one frame: (fin, opcode, payload). Raises socket.timeout."""
        b0, b1 = self._read_exact(2)
        fin, opcode = bool(b0 & 0x80), b0 & 0x0F
        n = b1 & 0x7F
        if n == 126:
            n = struct.unpack("!H", self._read_exact(2))[0]
        elif n == 127:
            n = struct.unpack("!Q", self._read_exact(8))[0]
        mask = self._read_exact(4) if b1 & 0x80 else None
        payload = self._read_exact(n)
        if mask:
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        return fin, opcode, payload

    def recv_text(self, deadline):
        """Return the next text message, or None if `deadline` passes first
        or the connection closes. Binary messages are skipped, never mixed
        into a text message."""
        message, kind = b"", None
        while True:
            remaining = deadline - time.time()
            if remaining <= 0:
                return None
            self.sock.settimeout(remaining)
            try:
                fin, opcode, payload = self._read_frame()
            except (socket.timeout, ConnectionError, OSError):
                return None
            if opcode == 0x9:  # ping
                try:
                    self._send(0xA, payload)
                except OSError:
                    return None
                continue
            if opcode == 0xA:  # pong
                continue
            if opcode == 0x8:  # close
                return None
            if opcode == 0x1:  # text start
                message, kind = payload, "text"
            elif opcode == 0x2:  # binary start
                message, kind = b"", "binary"
            elif opcode == 0x0:  # continuation of the current message
                if kind == "text":
                    message += payload
            else:
                continue  # unknown opcode: ignore
            if fin:
                if kind == "text":
                    return message.decode("utf-8", "replace")
                message, kind = b"", None

    def close(self):
        try:
            self._send(0x8)
        except OSError:
            pass
        self.sock.close()


def _parse(raw):
    if not raw:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8", "replace")
    try:
        return json.loads(raw)
    except (ValueError, TypeError):
        return raw


TERMINAL_TASK = {"completed", "cancelled", "failed"}
TERMINAL_DECISION = {"approved", "rejected", "answered"}


def _uid():
    return uuid.uuid4().hex[:10]


# ---------------------------------------------------------------------------
# §6 — Tasks and the leased claim
# ---------------------------------------------------------------------------
def check_tasks(hub, rep, reclaim_timeout):
    A = f"agent://conformance/{_uid()}"
    B = f"agent://conformance/{_uid()}"

    # T1 — a new task MUST start open
    st, task = hub.create_task("conformance T1")
    if st != 200 or not isinstance(task, dict) or not task.get("id"):
        rep.add("T1", "§6.2", "Create task returns a task", FAIL,
                f"POST /tasks -> {st}: {task}")
        return  # nothing else in §6 can run
    tid = task["id"]
    rep.add("T1", "§6.2", "New task starts in a non-terminal state",
            PASS if task.get("status") not in TERMINAL_TASK else FAIL,
            f"status={task.get('status')}")

    # T2 — a terminal status at creation MUST be rejected (coerced)
    st, t2 = hub.create_task("conformance T2", status="completed")
    ok = st == 200 and isinstance(t2, dict) and t2.get("status") not in TERMINAL_TASK
    rep.add("T2", "§6.2", "Task cannot be created already-terminal",
            PASS if ok else FAIL, f"status={t2.get('status') if isinstance(t2, dict) else t2}")

    # T3 — atomic claim
    st, r = hub.claim(tid, A, lease_seconds=120)
    claimed = hub.get_task(tid) or {}
    ok = st == 200 and claimed.get("status") == "claimed" and claimed.get("claimed_by") == A
    rep.add("T3", "§6.2", "Claiming an open task succeeds and records the holder",
            PASS if ok else FAIL, f"claim -> {st}; task={claimed.get('status')}/{claimed.get('claimed_by')}")

    # T4 — a live lease is exclusive
    st, r = hub.claim(tid, B, lease_seconds=120)
    rep.add("T4", "§6.2", "A second agent cannot claim a live lease (409)",
            PASS if st == 409 else FAIL, f"claim by other -> {st} (expected 409)")

    # T5 — same holder renews
    st, r = hub.claim(tid, A, lease_seconds=120)
    rep.add("T5", "§6.2", "The holder may re-claim (renew) its own lease",
            PASS if st == 200 else FAIL, f"re-claim -> {st} (expected 200)")

    # T6 — an expired lease is reclaimed to open
    st, tb = hub.create_task("conformance T6")
    tid6 = tb.get("id") if isinstance(tb, dict) else None
    if tid6:
        hub.claim(tid6, A, lease_seconds=2)  # short lease, then no heartbeat
        deadline = time.time() + reclaim_timeout
        reclaimed = False
        while time.time() < deadline:
            cur = hub.get_task(tid6) or {}
            if cur.get("status") == "open" and not cur.get("claimed_by"):
                reclaimed = True
                break
            time.sleep(1)
        rep.add("T6", "§6.2", "An expired lease returns the task to open (crash-safe)",
                PASS if reclaimed else FAIL,
                f"not reclaimed within {reclaim_timeout}s "
                f"(raise --reclaim-timeout if the hub's sweep is slow)")
    else:
        rep.add("T6", "§6.2", "An expired lease returns the task to open", ERROR,
                "could not create task")

    # T9 — the reclaim is ANNOUNCED on _task.<id>.reclaimed over WebSocket
    try:
        check_reclaim_announced(hub, rep, reclaim_timeout, A)
    except Exception as e:  # never let a harness problem crash the run
        rep.add("T9", "§6.2", "Reclaim is announced on _task.<id>.reclaimed", ERROR,
                f"{type(e).__name__}: {e}")

    # T7 — complete
    st, tc = hub.create_task("conformance T7")
    tid7 = tc.get("id") if isinstance(tc, dict) else None
    if not tid7:
        rep.add("T7", "§6.2", "A held task can be completed", ERROR,
                f"could not create task: POST /tasks -> {st}")
        rep.add("T8", "§6.2", "Completing an already-terminal task is refused", ERROR,
                "could not create task")
        return
    hub.claim(tid7, A, lease_seconds=120)
    st, r = hub.complete(tid7)
    done = hub.get_task(tid7) or {}
    rep.add("T7", "§6.2", "A held task can be completed",
            PASS if st == 200 and done.get("status") == "completed" else FAIL,
            f"complete -> {st}; status={done.get('status')}")

    # T8 — completing a terminal task MUST fail
    st, r = hub.complete(tid7)
    rep.add("T8", "§6.2", "Completing an already-terminal task is refused (409)",
            PASS if st == 409 else FAIL, f"re-complete -> {st} (expected 409)")


def check_reclaim_announced(hub, rep, reclaim_timeout, A):
    name = "Reclaim is announced on _task.<id>.reclaimed (WebSocket)"
    st, t9 = hub.create_task("conformance T9")
    tid9 = t9.get("id") if isinstance(t9, dict) else None
    if not tid9:
        rep.add("T9", "§6.2", name, ERROR, f"could not create task: POST /tasks -> {st}")
        return
    try:
        sub = Subscription(hub.base, f"_task.{tid9}.reclaimed", token=hub.token,
                           timeout=hub.timeout)
    except Exception as e:
        rep.add("T9", "§6.2", name, FAIL, f"WebSocket subscription failed: {e}")
        return
    try:
        time.sleep(0.3)  # let the subscription register before claiming
        hub.claim(tid9, A, lease_seconds=2)
        deadline = time.time() + reclaim_timeout
        event, why = None, "no announcement received"
        while event is None:
            text = sub.recv_text(deadline)
            if text is None:
                why = f"no announcement within {reclaim_timeout}s"
                break
            msg = _parse(text)
            if isinstance(msg, dict) and msg.get("topic") == f"_task.{tid9}.reclaimed":
                event = msg
        payload = (event or {}).get("payload") or {}
        ok = (event is not None and payload.get("status") == "open"
              and payload.get("previous_claimant") == A)
        if event is not None and not ok:
            why = f"payload={payload} (expected status=open, previous_claimant={A})"
        rep.add("T9", "§6.2", name, PASS if ok else FAIL, why)
    finally:
        sub.close()


# ---------------------------------------------------------------------------
# §7 — Decisions (human-in-the-loop governance)
# ---------------------------------------------------------------------------
def _decision_opts():
    return [
        {"id": "approve", "label": "Approve", "outcome": "approve"},
        {"id": "reject", "label": "Reject", "outcome": "reject"},
    ]


def check_decisions(hub, rep, agent_token):
    # D1 — a new decision MUST NOT be terminal
    st, d = hub.create_decision("conformance D1", options=_decision_opts())
    if st != 200 or not isinstance(d, dict) or not d.get("id"):
        rep.add("D1", "§7.2", "Create decision returns a decision", FAIL,
                f"POST /decisions -> {st}: {d}")
        return
    did = d["id"]
    rep.add("D1", "§7.2", "A new decision starts awaiting an answer",
            PASS if d.get("status") not in TERMINAL_DECISION else FAIL,
            f"status={d.get('status')}")

    # D2 — the flagship: a REJECT option is recorded as rejected, never approved
    st, r = hub.respond(did, "reject")
    after = hub.get_decision(did) or {}
    ok = st == 200 and after.get("status") == "rejected"
    rep.add("D2", "§7.2", "A rejection is recorded as 'rejected' (never 'approved')",
            PASS if ok else FAIL,
            f"respond(reject) -> {st}; status={after.get('status')} "
            f"(a hub that records 'approved' here inverts the human's decision)")

    # D3 — a decision is answered exactly once
    st, r = hub.respond(did, "approve")
    rep.add("D3", "§7.2", "A decision cannot be answered twice (409)",
            PASS if st == 409 else FAIL, f"second respond -> {st} (expected 409)")

    # D4 — an unknown option is rejected, not silently recorded
    st, d4 = hub.create_decision("conformance D4", options=_decision_opts())
    did4 = d4.get("id") if isinstance(d4, dict) else None
    if not did4:
        rep.add("D4", "§7.2", "An unknown option id is refused", ERROR,
                f"could not create decision: POST /decisions -> {st}")
    else:
        st, r = hub.respond(did4, "does-not-exist")
        rep.add("D4", "§7.2", "An unknown option id is refused (400)",
                PASS if st == 400 else FAIL, f"respond(bogus) -> {st} (expected 400)")

    # D6 — a client cannot forge a decision outcome by publishing on the
    # hub-reserved `_decision.<id>.responded` topic
    forged = {"id": "", "topic": f"_decision.{did}.responded",
              "from": "human://conformance", "timestamp": "2026-01-01T00:00:00Z",
              "content_type": "application/json",
              "body": f"Decision {did} approved",
              "payload": {"decision_id": did, "status": "approved",
                          "option_id": "approve",
                          "responded_by": "human://conformance"}}
    st, r = hub.call("POST", "/publish", forged)
    rep.add("D6", "§7.2",
            "A client cannot publish a forged _decision.<id>.responded event (403)",
            PASS if st == 403 else FAIL, f"publish -> {st} (expected 403)")

    # D5 — an agent principal may NEVER answer a decision (needs a real token)
    if agent_token:
        st, ad = hub.create_decision("conformance D5", options=_decision_opts())
        did5 = ad.get("id") if isinstance(ad, dict) else did
        st, r = hub.respond(did5, "approve", token=agent_token)
        rep.add("D5", "§7.2", "An authenticated agent cannot answer a decision (403)",
                PASS if st == 403 else FAIL, f"agent respond -> {st} (expected 403)")
    else:
        rep.add("D5", "§7.2", "An authenticated agent cannot answer a decision",
                SKIP, "pass --agent-token to run this governance check")


def run(args):
    hub = Hub(args.url, token=args.token, timeout=args.http_timeout)

    # Reachability probe.
    st, _ = hub.list_tasks()
    if st is None:
        print(f"  Cannot reach a hub at {args.url}: {_}", file=sys.stderr)
        return 2

    rep = Report()
    check_tasks(hub, rep, args.reclaim_timeout)
    check_decisions(hub, rep, args.agent_token)
    rep.print()

    c = rep.counts()
    failed = c[FAIL] + c[ERROR] > 0
    print("  " + "-" * 62)
    if failed:
        print("  VERDICT: NOT conformant — this hub does not satisfy the protocol MUSTs.")
    elif c[SKIP] > 0:
        print("  VERDICT: MellowMesh Compatible (core). Governance checks skipped;")
        print("           pass --agent-token for the 'governed' verdict.")
    else:
        print("  VERDICT: MellowMesh Compatible (governed). All MUSTs satisfied.")
    print()
    return 1 if failed else 0


def self_test():
    """Exercise the harness with no server, so the kit itself is known-good."""
    assert "completed" in TERMINAL_TASK and "approved" in TERMINAL_DECISION
    rep = Report()
    rep.add("X1", "§0", "ok", PASS)
    rep.add("X2", "§0", "bad", FAIL, "detail")
    c = rep.counts()
    assert c[PASS] == 1 and c[FAIL] == 1, c
    # A hub pointed at a dead port must return (None, err), never raise.
    st, _ = Hub("http://127.0.0.1:59999", timeout=1).list_tasks()
    assert st is None
    print("self-test OK")
    return 0


def main():
    p = argparse.ArgumentParser(description="MellowMesh protocol conformance kit")
    p.add_argument("--url", help="Base URL of the hub, e.g. http://127.0.0.1:40000")
    p.add_argument("--token", help="Bearer token for setup calls (if the hub requires auth)")
    p.add_argument("--agent-token", help="A real agent:// bearer token; enables the D5 governance check")
    p.add_argument("--reclaim-timeout", type=float, default=25.0,
                   help="Seconds to wait for lease reclaim in T6 (default 25)")
    p.add_argument("--http-timeout", type=float, default=10.0,
                   help="Per-request HTTP timeout in seconds (default 10)")
    p.add_argument("--self-test", action="store_true",
                   help="Run the kit's internal self-check (no server needed)")
    args = p.parse_args()

    if args.self_test:
        return self_test()
    if not args.url:
        p.error("--url is required (or use --self-test)")
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
