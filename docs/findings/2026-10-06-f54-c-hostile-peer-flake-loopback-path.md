# F54-C hostile-peer test: why the cut-off was unobservable, and what makes it deterministic

Date: 2026-10-06. Author: bunny-2. Task: Rally #695.

Capabilities used: ordinary build/test, `network_local` loopback. No original
data, no `network_real`, no human play. This is engine test design on the
F54-B pinned transport; nothing here claims anything about the original
game's networking.

## The observation

Reported by claude-2 on 2026-10-06 during task #691, in a full
`cargo test --workspace --locked` run on this shared macOS host (load average
~20) at a tree whose only differences from `main` were outside `cs_net`:

```
thread 'accept_f54_c_a_hostile_peer_is_cut_off_without_disturbing_the_others' panicked at
crates/cs_net/tests/accept_f54_c_lifecycle.rs:2103:5:
the abusive peer was cut off; the host saw [PeerJoined { peer: PeerId(1) }]
```

The notice list is the whole diagnosis in one line: after the honest peer
joined, **the host recorded nothing at all** — no refusal, no admission, no
cut-off — for the entire lifetime of the abusive peer.

## Baseline: it is not reproducible when the machine is quiet

| run | result |
| --- | --- |
| 50 consecutive single-test runs, idle (`load ~2-5`) | **0 failures** |
| 200 consecutive single-test runs while a full workspace suite ran | **0 failures** |
| full `cargo test --workspace --locked`, warm target | green (404 test binaries) |

So the timing assumption involved only shows when something else churns the
machine — which is how the original report happened (a workspace run, in
which hundreds of other test binaries bind and drop sockets).

## Reproducing it: cross-process socket churn

F54-X2 and F54-X10 already measured this machine's loopback:

* `docs/findings/2026-10-04-f54-x2-loopback-pump-and-socket-determinism.md`
  — loss under concurrency, the widened connect window, the file's
  `LOOPBACK` mutex that runs one live pair at a time;
* `docs/findings/2026-10-04-f54-x10-loopback-loss-diagnosis.md` — the
  dominant mechanism is the kernel's socket table under *concurrent churn*:
  a bound, live, drained socket that the loopback demultiplexer never routes
  to ("dropped due to no socket"), including sockets that routed traffic and
  went unreachable **mid-life**, plus a transient per-datagram drop layer
  that only retransmission covers.

A churn driver built from the committed F54-X10 harness reproduces the
original failure on demand — a driver of the kind that exists in the
repository already, run beside a loop of the hostile-peer test:

```sh
# background, looping for the duration of the measurement:
F54X10_MODE=transport F54X10_PROCS=6 F54X10_PAIRS=250 F54X10_SPINNERS=0 \
  timeout 25 target/debug/deps/accept_f54_x10_loopback_loss-* \
  f54x10_fleet --ignored --exact --nocapture   # plus parallel f54_b runs and 4 CPU spinners

# foreground: N consecutive runs of the test under test
target/debug/deps/accept_f54_c_lifecycle-* \
  accept_f54_c_a_hostile_peer_is_cut_off_without_disturbing_the_others --exact --nocapture
```

Results (600 runs per attempt, same machine, same driver):

| test under | failures | of which the reported signature | of which handshake-class |
| --- | --- | --- | --- |
| original test (`main`'s logic) | 127/600 | **11** | 104 |
| re-fire while waiting (see below) | 86/600 | **7** | 76 |
| this task's change | 18/600 | **1** | 17 |

The churn driver is far harsher than the original condition: its own fleet
report shows `dropped due to no socket` rising by 138 345–177 137 datagrams
per fleet run, and F54-X10 measured 1–69% of transport pairs dying under
exactly this shape. The two failure classes are the same fault seen at two
points: **the loopback path of the fixture stops carrying anything** — before
the handshake completes (handshake-class: `Link::joined`, `granted_client`
and the raw peer's handshake all report "the grant never arrived"), or after
it, when the fixture's abusive peer can no longer reach the host.

## The timing assumption

The scenario was written as if the handshake proved the *path*, not just the
*moment*:

1. it handed the whole corpus to the abusive peer in **one burst** on
   `CHANNEL_SEQUENCED` — the droppable channel, which never retransmits;
2. it then waited a fixed 64 rounds **without sending anything more**, so if
   the disconnect-triggering datagram was one of the lost ones, nothing could
   ever produce the verdict again;
3. it never asked whether *any* hostile payload had arrived, so a path that
   carried nothing was reported as a **session** decision ("the abusive peer
   was cut off") rather than as a fixture that never worked.

F54-X10's own recommendation is the third point's consequence: on a shared
host, serialization of socket setup/use/teardown is necessary but not armor —
an unrelated churning fleet can still orphan an individual socket.

## What changed

All of it in `crates/cs_net/tests/accept_f54_c_lifecycle.rs`; no production
code changed, and every assertion kept its exact meaning.

1. **The abusive peer keeps abusing while the verdict is awaited.** After the
   one burst of the corpus, the wait loop fires one hostile payload per round
   (cycling through the corpus) until the peer is cut off or
   [`HOSTILE_ROUNDS`] is spent. The expectation now rests on the session's
   answer instead of on which datagrams survived the burst.
2. **A fixture that carried nothing is rebuilt, bounded by
   [`DEAD_PATH_ATTEMPTS`] = 6.** `abusive_peer_attempt()` reports whether the
   host processed *anything* from the abusive peer (`hostile_traffic`:
   an admission, a refusal or the cut-off). An attempt in which it did not —
   or whose handshake never completed (`Link::try_joined`,
   `RawPeer::try_handshake`) — is proof that the fixture, not the session,
   failed, so the whole fixture is rebuilt. **The verdict is never retried**:
   as soon as the host has seen hostile traffic, that attempt's outcome is
   reported exactly as it happened.
3. **Failure says what both ends could see.** `diagnose_silence()` runs only
   when no cut-off came, and reports the notices the host collected, the
   abusive peer's own transport state and event stream, how many connections
   the host still holds, whether hostile traffic ever reached it, and — by
   letting the honest peer submit an edge — whether the host still hears
   *anybody*. The per-attempt history is appended. Nothing is pumped on the
   passing path.
4. `Link::try_joined` and `RawPeer::try_handshake` are options over the
   existing `joined`/`handshake`, which keep their behaviour and messages for
   the other 30 tests in the file.

### The diagnosis the new message produces

The one remaining target-signature failure in the 600-run measurement above:

```
the abusive peer was cut off; the host saw [PeerJoined { peer: PeerId(1) }];
the abusive peer's transport: connected true, reason None, events [("connected", 1), ("granted", 1)];
the host holds 2 client(s);
the abusive peer's traffic reached the host: false;
the honest peer still reaches the host: true
```

That separates the two possible stories: the host socket is **alive and
serving the honest peer** (the probe's input is queued), while the abusive
peer — which completed its handshake moments earlier and still believes it
is connected — never gets a single payload into the host's receive path.
This is the F54-X10 mid-life unreachable-socket signature, at the *peer*
end of the pair rather than the host end.

### A deadlock worth recording

The first implementation of the rebuild kept the previous `Attempt` alive
while building the next one. `Link` holds the file's `LOOPBACK` mutex for as
long as its sockets live, so the second fixture blocked forever in
`loopback()`; the test hung after 118 runs instead of failing. `sample` on
the hung process showed `abusive_peer_attempt → Link::joined → Link::new →
loopback → Mutex::lock → __psynch_mutexwait`. The retry now drops the
previous fixture before binding a new one, and says why in a comment.

## After

Same machine, same churn driver:

* **18/600** failures, none of them the reported signature except **one**,
  which now prints the full diagnosis above instead of a bare notice list.
  Handshake-class failures dropped from 104/600 to 17/600 (six attempts per
  run instead of one) and are reported as "the loopback never carried this
  fixture" with the last attempt's reason — never as a session verdict.
* **1500/1500 consecutive runs of the test while
  `cargo test --workspace --locked` ran concurrently** (0 failures; the task
  asked for 50), plus an earlier 60/60 the same way.
* Full check set green: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`,
  `cargo test --workspace --locked` (exit 0, 404 test binaries),
  `cargo test --workspace --locked -- accept_f54_c_ --include-ignored`
  (31 passed).

The churn driver itself demonstrates the remaining exposure honestly: while
it ran, the *task-selection* run
`cargo test --workspace --locked -- accept_f54_c_ --include-ignored` failed
6–7 tests of the same file that this task does not own (a grant that never
arrives, `pump_until`'s budget, `granted_client`'s panic). With the driver
stopped, the same command is green. That class is filed as a follow-up task,
not fixed here.

## Honest limits

* The kernel mechanism is *inferred* from userspace evidence, as F54-X10's
  own limits say: no dtrace/kdebug on this host. What is measured here is
  that the host still serves one peer while another peer's traffic
  vanishes, that the churn driver's UDP counters show massive
  `dropped due to no socket`, and that rebuilding the fixture usually
  restores delivery.
* One residual target-signature failure in 600 runs under a churn
  generator that is far more aggressive than any suite this file runs in.
  On a host without a concurrent churning fleet the measurement is 0/1500.
  A test cannot make a dead path live; what it can do is refuse to report
  a session verdict that was never observable, and this one now does.
* `ClientTransport::update` discards the result of its send path
  (`let _ = self.transport.send_packets(...)`), so a peer that *cannot* send
  is indistinguishable from a peer that is not trying. That was not proven to
  be part of this failure (the receiving end shows no traffic either way), so
  it is recorded here as an observation for a follow-up task rather than
  changed in this one.
* Loopback on one machine is `network_local` per `AGENTS.md`; nothing here
  is `network_real` evidence, and no original data was read.

## Sources

* Task Rally #695 (observation and acceptance criteria).
* `docs/findings/2026-10-04-f54-x1-retry-hang-up-test-determinism.md` — the
  pinned stack keeps no wall clock; round budgets, not sleeps.
* `docs/findings/2026-10-04-f54-x2-loopback-pump-and-socket-determinism.md`
  — the connect window, the `LOOPBACK` mutex, what serialization does and
  does not change.
* `docs/findings/2026-10-04-f54-x4-loopback-budget-and-socket-churn-f54-b.md`
  — churn during F54-B.
* `docs/findings/2026-10-04-f54-x10-loopback-loss-diagnosis.md` — the
  kernel-level orphan socket, mid-life loss, transient drop layer and the
  recommendation this task follows.
* `crates/cs_net/tests/accept_f54_x10_loopback_loss.rs` — the committed
  churn harness used to reproduce (`f54x10_fleet`), run only under `timeout`.
