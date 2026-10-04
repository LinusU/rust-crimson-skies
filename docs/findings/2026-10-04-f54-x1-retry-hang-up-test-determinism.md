# F54-X1: why the retry hang-up test was timing-sensitive, and what makes it deterministic

Date: 2026-10-04. Task: #600 F54-X1 "Make the cs_net retry test deterministic
(`accept_f54_c_a_retry_hangs_up_the_connections_that_hold_no_peer`)", opened by
bunny-alpha-2 from an observed `cargo test --workspace` failure on
`rally/596-measure-the-precedence-between-wake-nap` (origin/main `fc8022bb`).
Capabilities used: ordinary build/test, `network_local` loopback. No original
data, no `network_real`, no human play. This is engine test/tooling design on
the F54-B pinned transport; nothing here claims anything about the original
game's networking.

## The observation

```
thread 'accept_f54_c_a_retry_hangs_up_the_connections_that_hold_no_peer' panicked at
crates/cs_net/tests/accept_f54_c_lifecycle.rs:2010:5:
the retry hung up on the connection that held no peer id
```

It passed when that test binary ran alone and failed inside
`cargo test --workspace`. Reproduced on this machine by running the binary with
`--test-threads=16` under CPU load: 2 failures in 30 runs with 8 `yes` spinners,
6 in 40 runs with 24 spinners (11 cores, so >2x oversubscribed).

## Measured cause: the test raced the pinned connection layer's own clock

The pinned stack keeps **no wall clock**: `NetcodeClient`/`NetcodeServer`
accumulate `current_time` from the `Duration` each `update` is handed, and every
timeout and pacing decision reads that accumulator. From the pinned sources:

| Fact | Where |
| --- | --- |
| `NETCODE_SEND_RATE = 250 ms`: a client sends at most one protocol packet per 250 ms **of its own accumulated time** | `renetcode2-0.16.1/src/lib.rs:61`, `client.rs:354-359` |
| The connect token's `timeout_seconds` (5 s) is compared against the same accumulator, in `SendingConnectionRequest`, `SendingConnectionResponse` **and `Connected`** | `renetcode2-0.16.1/src/client.rs:303-304`, `342-345` |
| `timeout_seconds` is the pinned unsecure token's fixed 5, so every loopback client here gets a 5 s window with nothing in `cs_net` setting it | `renetcode2-0.16.1/src/server.rs:912` |
| `Packet::Disconnect` is honored **only** in `ClientState::Connected`; every other state falls through the `_ => {}` arm | `renetcode2-0.16.1/src/client.rs:256-259` |
| The server generates and sends that hang-up **exactly once**, in the update where the reliable layer no longer holds the connection, and drops the netcode client | `renet2_netcode-0.16.1/src/server.rs:185-193`, `renetcode2-0.16.1/src/server.rs:829` |

The old test pumped the **host at `STEP` (16 ms) and the client at one second per
round**:

```rust
for _ in 0..4 {
    host.pump(STEP);
    client.pump(Duration::from_secs(1));
    if !client.transport().is_connected() { break; }
}
```

so the client's accumulator ran 62x ahead of the host's, and four of those
rounds are four of the five seconds its own disconnect window allows. The
comment claimed "four one-second rounds stay inside the connection layer's own
five-second timeout", but the window that matters is measured from
`last_packet_received_time`, not from the start of the loop, so the margin the
argument rested on was zero: the client's own timeout and the host's hang-up were
racing each other, and the hang-up is sent once and never retransmitted.

Instrumenting the test (temporary, not committed) showed both sides' verdicts
arriving late and unevenly under load — the client's refusal at up to round 4 of
that loop and the hang-up at round 6 — while the same test with **both sides at
`STEP`** observed the hang-up in 1-2 rounds in every one of 40+ runs.

## What changed

Production (`crates/cs_net`), all of it making the retry's decision observable
instead of inferable from a round budget:

- `ServerSession::reopen` now returns `Result<usize, ServerFault>`: how many
  netcode connections it hung up beyond the peers `close` already reached — in
  practice the clients that hold no peer id. `HostTransport::reopen` already
  computed that count and `ServerSession` discarded it.
- `ServerSession::reopen` drains the deferred hang-up queue. The host hangs up on
  a refused client one round after the refusal (F54-C, on purpose, so renet can
  flush the refusal first), and the retry subsumes every such condemned
  hang-up; reporting it again inside the fresh epoch would describe the spent
  epoch's connection as the new epoch's business.
- `HostTransport::connected_clients` / `ServerSession::connected_clients`: how
  many netcode connections are held, peers or not. A refused client is not a
  member and not a peer, so this is the only count that says whether it still
  occupies one of the `MAX_SESSION_PEERS` slots.
- `ClientTransport::disconnect_reason`: the pinned layer's own reason, so a caller
  can tell a host hang-up (`DisconnectedByServer`) from a window the client missed
  by itself (`ConnectionTimedOut`). `is_connected` alone cannot tell them apart,
  which is precisely what made the old assertion ambiguous.

The test now:

1. drives both sides from the same `STEP`, the clock every pump takes;
2. waits for the client's verdict with client-only rounds, because the host must
   not run another update before the retry — the retry has to be what hangs this
   connection up, so it has to start from one the host still holds;
3. asserts the retry's decision with no clock in it at all: `reopen` hung up on
   exactly the one connection that held no peer id, and the fresh epoch holds no
   connection for it;
4. gives the hang-up `HANGUP_ROUNDS = 64` rounds (about a second of
   connection-layer time, while the client's own 5 s window is ~312 rounds away)
   and then asserts the reason is `DisconnectedByServer` by name.

The assertion is stronger than before, not weaker: it still proves the refused
client loses its connection to the retry, and it now also proves the host made
that decision, that it released the slot, that the client named the server as
the cause, and that the fresh epoch does not report the hang-up twice.

## Sensitivity

Both production behaviors the test names were removed in turn and the test fails
on each:

| Removed | Failure |
| --- | --- |
| `HostTransport::reopen`'s `self.server.disconnect_all()` | `the retry hung up on the connection that held no peer id within 64 rounds` |
| `ServerSession::reopen`'s `self.hung_up.clear()` | `the retry hung up on that connection as one decision, so the fresh epoch does not report it again …` |

## Measurements

Retry test alone, `--exact … --test-threads=1`, 120 runs of each binary
interleaved, machine load average ~98 on 11 cores (other agents building):

| Binary | Failures |
| --- | --- |
| before (origin/main `3ead0b9c`) | **3** — all at the hang-up assertion (line 2010) |
| after | **0** |

Whole binary, `--test-threads=16`, no artificial load, machine load average
80-124 from other agents: 10 of 60 runs failing before, 4 of 60 after.

## Review verification (2026-10-04)

The two probes above were repeated independently on the reviewed head, and a
third was added that stubs the new return value. All three fail the test:

| Removed | Failure |
| --- | --- |
| `HostTransport::reopen`'s `self.server.disconnect_all()` | `the retry hung up on the connection that held no peer id within 64 rounds` (line 2077) |
| `ServerSession::reopen`'s `self.hung_up.clear()` | `… does not report it again …` (line 2088) |
| `ServerSession::reopen`'s returned count, replaced by `0` | `assertion 'left == right' failed: the retry hung up on the connection that held no peer id` (line 2056) |

Both binaries were then built once each and run **interleaved** in the same time
window, so the comparison is not two different machine conditions. With load
average 120 on 11 cores (other agents linking), `--test-threads=16`, 30 runs each:

| Binary | Runs with a failure | Retry-test failures |
| --- | --- | --- |
| before (origin/main `aa688133`) | **23 / 30** | 8 |
| after | **7 / 30** | 3 |

At load average 38-43 the same two binaries gave 0 failures in 100 runs each, and
64 CPU spinners on top of the existing load added none in a further 40 runs of
the retry test. The failure is therefore a property of the load, not of either
version — which is what the residual limit below says.

## Residual limit (not this task)

Under >2x CPU oversubscription **both** versions fail broadly and randomly
across the file — before 16 failures over 40 runs, after 24 over 40 runs — with
symptoms like `connection timed out during request step` and a host that never
sees a hello at all. That is loopback UDP losing packets under load, not a
property of this test: every test in this file needs a real handshake, and
`pump_until`'s `MAX_ROUNDS` budget does not survive it either. Filed as a
separate task; this finding records it so the numbers above are not read as a
claim that `cs_net`'s suite is load-immune.

One single-shot dependency survives the fix and belongs to that task too: the
pinned layer sends `Packet::Disconnect` exactly once and never retransmits it,
so a lost hang-up packet fails `HANGUP_ROUNDS` instead of being retried. The old
test had that exposure plus a round budget that raced the client's own window.
