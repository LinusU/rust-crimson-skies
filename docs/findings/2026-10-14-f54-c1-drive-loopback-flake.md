# F54-C1 — why `accept_f54_c_drive`'s mailbox freeze flaked (Tick 14 vs 13)

Date: 2026-10-14. Author: bunny-alpha-2. Task: Rally #1386.

Capabilities used: ordinary build/test only, `network_local` loopback on one
machine. No original data, no `network_real`, no human evidence.

## Verdict

**The test asserted something the wire never promised; production behaviour is
correct.** `remove_resource::<NetHost>()` stops the host *publishing* — it
cannot recall a datagram the host had already written to the loopback socket.
A snapshot published on the last live run is queued by `renet2` and written to
the socket by the *next* host pump, which normally happens before the client's
recv in that same drive run; when the kernel's loopback visibility window
outlasts that in-run gap, the client reads the datagram one run late. If that
late read is the run in which the fixture captures `last_fed` and then removes
the host, the client's next pump — the first run *after* teardown — reads it
and moves `snapshot_tick()` by exactly one. That is the reported
`left: Some(Tick(14)) / right: Some(Tick(13))`.

The fix drains the wire before `last_fed` is captured: the fixture ends the
match (`ServerSession::finish`), the transport flushes its queue once, and the
fixture waits for the snapshot published on the final live tick to land. After
that, nothing can be written or delivered, so the original exact freeze
assertion (`snapshot_tick() == last_fed`) is sound again — and the staging now
also asserts `published == None` and `ingested == None` on the teardown run
itself.

## 1. Reproduction and traces

On the task branch (fresh from `origin/main`, no changes yet):

```
$ cargo test -p cs_app --locked --test accept_f54_c_drive
thread 'accept_f54_c_drive_the_session_round_trip_binds_every_boundary' panicked at
  crates/cs_app/tests/accept_f54_c_drive.rs:473:5:
assertion `left == right` failed: the mailbox still holds the last snapshot
  left: Some(Tick(13))
 right: Some(Tick(12))
```

Failed on the **first** run after checkout; a temporary `F54_TRACE` build then
caught it again on attempt 5 of a 60-attempt loop. The whole five-test file
runs in 0.04 s, so one attempt is ~0.01 s per round trip.

Instrumented traces (temporary, removed before commit):

| | stop run (first ingest) | `last_fed` | teardown run | final |
| --- | --- | --- | --- | --- |
| passing | `at=14`, `published=Sent at 14`, `ingested=Applied tick 13` | 13 | `ingested=None`, mailbox 13, `published=None` | 13 |
| failing | `at=15`, `published=Sent at 15`, `ingested=Applied tick 13` | 13 | **`ingested=Applied tick 14`**, mailbox 14, `published=None` | 14 ✗ |

Signature: a passing run has `report.at == last_fed + 1` (the client read the
datagram written during *this* run's host pump); a failing run has
`report.at == last_fed + 2` (the client read the datagram written during the
*previous* run's pump, and this run's datagram — header `last_fed + 1` — is
still unread when `last_fed` is captured). Nothing else differs, which is why
the assertion at `accept_f54_c_drive.rs:472` (`report.ingested == None` on the
*final* of the three post-teardown runs) still passed while line 473 failed.

## 2. Where the datagram is written, and when it is read

All in the production path the fixture drives (`drive_net_session`):

1. `crates/cs_app/src/network/session.rs:1096` — the host block pumps first
   (`ServerSession::pump`), and that pump ends with
   `self.transport.send_packets(&mut self.server)`
   (`crates/cs_net/src/transport.rs:820`).
2. `crates/cs_app/src/network/session.rs:1127-1142` — the committed tick's
   snapshot is published *after* the pump, but `ServerTransport::broadcast`
   only queues: it calls `RenetServer::send_message`
   (`crates/cs_net/src/transport.rs:665-673`), which parks the payload in the
   channel (`renet2-0.16.1/src/server.rs:201`). Packets are only produced by
   `get_packets_to_send` (`renet2-0.16.1/src/remote_connection.rs:590`), which
   the pinned layer calls from `send_packets`
   (`renet2_netcode-0.16.1/src/server.rs:207`).
3. `crates/cs_app/src/network/session.rs:1149` — the client block pumps later
   in the same run: `ClientTransport::update` drains the socket in a
   `try_recv` loop until `WouldBlock` (`crates/cs_net/src/transport.rs:1185-1191`
   and the pinned recv loop in `renet2_netcode-0.16.1/src/client.rs:110-155`).

So the snapshot published on tick *T* is written to the socket on run *T+1*
and is normally read by the client pump of run *T+1*. The write is a kernel
`sendto`; the read is a separate `recvfrom` microseconds later — the visibility
of one to the other is the kernel's business, not the drive's.

## 3. Measurement: loopback `sendto` is never immediately readable here

Machine: Darwin 25.6.0 / 25G83, `hw.ncpu=11`, `net.inet.udp.recvspace=786896`
— the same host as `2026-10-04-f54-x10-loopback-loss-diagnosis.md`, which
diagnosed a *different* kernel fault (whole sockets orphaned by concurrent
socket churn). This measurement is about per-datagram visibility delay.

Harness (one thread, non-blocking recv, both sockets on `127.0.0.1`):

```python
srv = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); srv.bind(("127.0.0.1", 0))
cli = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); cli.bind(("127.0.0.1", 0))
cli.setblocking(False)
for _ in range(n):
    srv.sendto(b"x" * 64, cli.getsockname())
    t0 = time.perf_counter_ns()
    while True:
        try:
            cli.recv(2048); break
        except BlockingIOError:
            if time.perf_counter_ns() - t0 > 5_000_000_000: raise SystemExit("never readable")
    delays.append(time.perf_counter_ns() - t0)
```

| condition | n | p50 | p99 | max | >100 µs |
| --- | --- | --- | --- | --- | --- |
| idle (loadavg ~2.4) | 200 000 | 2.0 µs | 7.0 µs | 82.6 µs | 0 |
| 8 `yes` generators, started and killed by PID | 30 000 | 7.3 µs | 16.4 µs | **580 µs** | 5 |

A datagram therefore sits invisible for ~2 µs typically and for well over
half a millisecond in the load tail — while a whole fixture run (physics +
both pumps) is tens of microseconds. Whenever the window spans the gap between
`send_packets` and the client's `try_recv`, the read misses that run's
datagram and picks it up on the next one. When the window spans the run in
which the fixture stops, the pickup lands after teardown.

The defect is not confined to a single run: while the host is Live it writes
one datagram per run, so a window of *k* runs leaves up to *k* datagrams
unread at any moment. That is why "tolerate one in-flight snapshot" is not a
proof — a bounded tolerance cannot bound the window — and why the fix drains
instead.

## 4. Why production is correct

- The datagram that moved the mailbox was written **before**
  `remove_resource::<NetHost>()`; nothing is delivered from a resource that is
  gone. The teardown run reports `published: None` (the host block does not
  run), which the fixed test now asserts.
- The drive's client half deliberately does not consult the host resource:
  the mirror feeds from `ClientSession::snapshot()`, the session owner that
  still exists (`crates/cs_app/src/network/session.rs:1148-1255`).
- The mailbox is strictly newest-wins: `cs_net` refuses any frame whose tick
  is not strictly greater (`crates/cs_net/src/lifecycle.rs:1623-1639`), so a
  straggler can only ever arrive once and never moves the tick backwards.
- Each tick is fed at most once (`state.fed` gate,
  `crates/cs_app/src/network/session.rs:1241`).
- The contract makes no delivery promise for motion snapshots: "Motion
  snapshots are sequenced and may be dropped" (`docs/contracts/UI-NETWORK.md`,
  §Reliability and prediction). A test may not infer "frozen forever" from
  "nothing new was published *yet*" while the socket still holds a datagram.

## 5. The fix

`crates/cs_app/tests/accept_f54_c_drive.rs`, stage "the mailbox is
latest-wins …" of `accept_f54_c_drive_the_session_round_trip_binds_every_boundary`:

1. **Drain.** After the ingest wait and the publish assertion, call
   `NetHost::session_mut().finish(last_published, FinishReason::Completed)`.
   The host stops publishing (`report.published == Some(Skipped("finished"))`,
   asserted on every drain run), the transport flushes its queue on the next
   pump, and the snapshot published on that final live tick
   (`last_published = report.at`) is the last datagram the wire will carry.
   Step until `snapshot_tick() == Some(last_published)`.
2. **Capture `last_fed` only after that** — asserted equal to
   `Some(last_published)`, i.e. "the drained wire left its final snapshot in
   the mailbox".
3. **Teardown, with the original exact assertion kept** — plus two new
   assertions that tighten the boundary: the teardown run reports
   `published == None` ("a host resource that is gone publishes nothing") and
   `ingested == None` ("teardown itself feeds nothing new").

Nothing was weakened: the exact `snapshot_tick() == last_fed` equality and the
`ingested == None` check survive verbatim; the staging around them now matches
what the system actually guarantees, and two assertions were added.

## 6. Verification

- 550 consecutive passes of the whole five-test file after the change: 400
  attempts idle, then 150 attempts with 8 CPU load generators running
  (generators started by recorded PID and killed in a `trap`; `pgrep -x yes`
  was 0 afterwards). Before the change the same loop failed on attempt 1 and
  attempt 5.
- Mutation check (temporary, reverted): removing the `state.fed` once-each
  gate in `drive_net_session` fails the new assertion —
  `teardown itself feeds nothing new: left Some(Applied … tick 11)` — so the
  strengthened staging still exercises the production path and still bites.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings`, `cargo test --workspace --locked`
  and `cargo test --workspace --locked -- accept_f54_c_drive_ --include-ignored`
  all run for this task; results are in the handover.

## 7. Affected content and follow-ups

Affected content: the F54-C drive acceptance suite only; no production file,
contract or spec changed. Resolving task: Rally #1386.

No follow-up filed: the drive, `cs_net` and the pinned transport all behave as
documented. Related earlier work: `2026-10-04-f54-x10-loopback-loss-diagnosis.md`
(a different kernel fault — orphaned sockets under socket churn, which the
serialized `LOOPBACK` mutex in this file already answers).
