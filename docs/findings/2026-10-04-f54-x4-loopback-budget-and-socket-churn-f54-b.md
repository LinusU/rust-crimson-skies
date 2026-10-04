# F54-X4: the same treatment for accept_f54_b_pinned_transport, and what it did and did not show

Task: #607 (F54-X4). Found while fixing #605 (F54-X2); see
`docs/findings/2026-10-04-f54-x2-loopback-pump-and-socket-determinism.md`, whose
"Limits, honestly" section recorded this file as untouched follow-up work.

Machine: 11 cores (`sysctl -n hw.ncpu`), macOS (Darwin 25.x),
`net.inet.udp.recvspace: 786896`, `net.inet.udp.maxdgram: 9216` — the same
machine and the same UDP parameters the F54-X2 finding measured.

Load was produced honestly by 24 background processes each running
`while :; do :; done` (never sleeping), from `private/f54x4/load.sh`, and
confirmed alive with `kill -0` (**24/24**) before and after every measurement
below. 24 spinners on 11 cores is oversubscribed more than twofold on its own;
the machine was additionally carrying other agents' builds throughout, so
`uptime` load average ran 86-121. `private/f54x4/unload.sh` stopped all 24 and
re-verified 0 alive.

## What was changed

One file: `crates/cs_net/tests/accept_f54_b_pinned_transport.rs`. **No
production code was touched**, and no assertion was weakened, no round budget
replaced by a sleep, and no test skipped or `#[ignore]`d.

* `LOOPBACK_WINDOW = ConnectWindow::new(120)`, used by every client the file
  builds — including the second client inside
  `accept_f54_b_handshake_admits_and_returns_a_grant_over_udp`, which was the
  one site that had been missed by construction rather than by oversight.
* `MAX_ROUNDS` is now **derived**: `window * 1000 / STEP.as_millis() + 64`, the
  same relation F54-C uses, so neither number can drift out from under the
  other. It is still a bound on rounds; nothing sleeps on the wall clock.
* `static LOOPBACK: Mutex<()>` with `fn loopback()`, held by `Link` in
  `self._loopback` for as long as its two sockets are open. The file now runs
  **one live loopback pair at a time**.
* New test `accept_f54_b_the_connect_window_is_a_fixture_parameter_over_the_default`,
  which pins the fixture window as a real production-path parameter rather than
  a comment: it asserts the derived budget, asserts a client that asked for
  [`DEFAULT_CONNECT_WINDOW`] **is** dropped by the pinned layer after
  `SILENT_ROUNDS` silent rounds while a client that asked for `LOOPBACK_WINDOW`
  **is not**, and asserts the widened client still gets the same grant a default
  client gets.

`cargo test` runs test binaries concurrently — measured, not assumed: polling
for the two harness processes during a run saw **2 at once**. So F54-C's own
`LOOPBACK` mutex genuinely cannot serialize against this file, exactly as the
task description says, and each file holds its own lock.

## The measurement, and the part that did not go as expected

Acceptance criterion 1 asks for 50 consecutive passing runs at
`--test-threads=16` under deliberate oversubscription. That was met, in two
configurations, both with 24/24 spinners confirmed alive:

| configuration | 50 runs | result |
| --- | --- | --- |
| F54-B alone, this branch | 50 | **50 pass, 0 fail, 0 hung** (load ~98) |
| F54-B + F54-C concurrent, this branch | 50 | **50 pass, 0 fail, 0 hung** (load ~102) |

Each run was hard-capped at 120 s so a self-deadlock would surface as a timeout
rather than as a hang. The whole binary is 17 tests in ~0.03 s, so serializing
its loopback tests costs nothing measurable.

**But the same measurement run against `origin/main`'s version of this file, on
the same machine with the same 24 spinners and the same harness, also passed
50/50 — both alone (50/50) and concurrent with F54-C (50/50).** A third probe,
keeping this branch's mutex but reverting `Link::new` to the default fifteen-
second window, also passed 50/50 concurrent.

So on this machine, today, **this task's fix is not demonstrated by a
before/after failure contrast**, and it would be dishonest to present it as
though it were. The criterion is met; the flake it was written against did not
reproduce here.

### Why the pre-fix file did not reproduce, stated as a hypothesis

F54-X2 measured **11 of 50** failing runs for F54-C before its fix, at load
85-99. The most likely reason the pre-fix F54-B file is quiet now is that the
pair churn that triggers the loss is much lower than it was during that
measurement: F54-C has been serialized by its own `LOOPBACK` mutex since #605
landed, so the "before" runs here churn roughly F54-B's ~9 loopback tests plus
F54-C's single one, whereas F54-X2's "before" had F54-C's ~9 unserialized *and*
F54-B's ~9 — about twice the number of short-lived socket pairs in flight. The
F54-X2 finding's own measurement puts the threshold well above what this file
reaches alone: 25% of pairs lost datagrams at 11-way concurrent churn, while a
single loopback loop at the same load settled 600 of 600.

This is a hypothesis about a kernel behaviour nobody has diagnosed yet, offered
as the best available explanation for a null result. It was **not** tested by
reverting F54-C's mutex to raise the churn, and it should not be read as
settled.

### Reviewer follow-up: the hypothesis was then tested, and still did not reproduce

The review of #607 ran that missing experiment. `origin/main`'s versions of
*both* acceptance files were built as separate harness binaries — that is the
highest-churn configuration this pair can reach, `F54-B`'s ~9 unserialized
loopback tests plus `F54-C`'s unserialized ones, the state F54-X2 measured at
11 of 50 failing — and both binaries were then run **concurrently** at
`--test-threads=16`, 30 rounds, each round hard-capped at 120 s, with 24/24
spinners confirmed alive on 11 cores:

| configuration | rounds | result |
| --- | --- | --- |
| pre-fix F54-B + pre-fix F54-C, both unserialized, concurrent | 30 | **30 pass, 0 fail, 0 hung** (load 95-114) |
| this branch's F54-B + F54-C, concurrent | 50 | **50 pass, 0 fail, 0 hung** (load 116-129) |

So the churn hypothesis, which would have said "raise the churn and the pre-fix
file flakes again", **does not reproduce even when the churn is maximised**.
Either the threshold moved for another reason, or something else about the
earlier session differed, or the loss needs more than these tests alone
generate. The implementer's characterisation above stands unchanged and is now
supported from two directions: the after column is not evidence the pre-fix file
was flaky *here*, and neither is the before column evidence that raising churn
brings it back. Treat the mutex as a defensible reduction in concurrent socket
pressure justified by F54-X2's measurement, not as a fix for a failure anyone
can currently reproduce. The reviewer ran the second row of the table as the
acceptance check for criterion 1 independently; harness scripts and binaries are
in gitignored `private/f54x4-review/`.

One reviewer-side observation belongs here because it is about this task's own
verification rather than the loopback path: the first `cargo test --workspace
--locked` on the review machine exited 101 with
`could not execute process …/cs_inspect-e4bc649912dd2984 (never executed)` /
`No such file or directory (os error 2)`. That is the case `AGENTS.md` names as
*not* a test failure (the host's external pruner of old harness binaries, task
F54-X8); the identical command rerun once was green. Both runs are reported, in
that order. Nothing in this branch touches `cs_inspect`.

### A correction to the cleanup verification above, including this review's own

The section above says `unload.sh` "stopped all 24 and re-verified 0 alive".
The reviewer hit a defect in exactly that verification method and cannot
confirm the original claim, so it should not be relied on as stated.

What happened: the review's own unload script counted the spinners still alive
by looping over the pid files *its own kill step had just emptied*, so once
every file was gone the count was trivially `0` and the script reported success
— **while all 24 spinners kept running** for the next ~50 minutes, across the
whole of the 50-round acceptance measurement. Two further traps sit in the same
spot: a pid file is not proof of a live spinner, because a reused pid answers
`kill -0` just as well; and the report cannot be trusted from a glob the same
loop emptied. The authoritative check is `ps` on the spinner's own command line,
which is what the corrected script now does.

What this does and does not change:

* **The measurements in the tables stand.** The load averages prove the spinners
  were running during both of them: the 1-minute load was ~116 immediately after
  the unload script claimed success, and load only decays once the CPU work
  stops. So both the 30-round before column and the 50-round after column were
  taken with at least the 24 spinners alive on 11 cores, as stated.
* **The claim that they were stopped at the end does need re-checking** by
  whatever runs next, with `ps` and not with a pid-file count.
* A reviewer reading "re-verified 0 alive" in this or any sibling finding should
  assume it may be a vacuous zero until it has been reproduced with `ps`.

## What justifies the change without a before/after contrast

The defect the old budget contained is provable by inspection and by a
deterministic test, independently of whether it flakes on a given afternoon.

`MAX_ROUNDS = 2_000` at `STEP = 16 ms` is **32 seconds of accumulated pump
time**, against a connection-layer window of **15 seconds**. The pinned client
gives up at `last_packet_received_time + timeout_seconds < current_time`, and
`current_time` advances only by the `elapsed` a caller hands `update` — so the
old budget let `client_until` and `host_until` keep pumping for **17 seconds of
pump time after the connection layer had already declared the connection dead**.
That is F54-X2's section 5, and it is why it wrote "a bigger round budget cannot
fix this".

The new test demonstrates exactly that gap deterministically rather than
probabilistically: `SILENT_ROUNDS` is `15 * 1000 / 16 + 64 = 1001` rounds, and at
that point a default-window client **is** gone while a `LOOPBACK_WINDOW` client
**is not**. The old budget would have spent another ~1000 rounds past the
default window's verdict waiting for an event that can never arrive.

### Sensitivity: each half is load-bearing for what it claims

* **`connect_with_window` ignores its argument** (uses
  `DEFAULT_CONNECT_WINDOW` instead) — the new test fails, as it must:
  `after 1001 silent rounds (ConnectWindow(120) window) the connection layer's
  verdict is the window's  left: false  right: true`.
* **`MAX_ROUNDS` reverted to a bare `2_000`** — the derived-budget assertion in
  the new test fails. That assertion is the regression tripwire for this task.
* **The mutex removed** — *not* demonstrated on this machine today. The
  mechanism is measured in F54-X2 sections 4 and 6, not re-measured here.
* **The second assertion on the budget is a close-tracking check, not a tight
  one.** `budget_ms < window_ms * 2` passes for any budget under twice the
  window; the old `2_000`-round value (32 s against a 15 s window, 2.1x) would
  also have passed *if it were measured against `LOOPBACK_WINDOW`*, which is why
  the regression tripwire for a bare constant is the preceding `assert_eq!` on
  the derivation, not this bound.

## Limits, honestly

* **No failure reproduced before or after.** Criterion 1 is satisfied as
  written (50/50 under >2x deliberate oversubscription), but this branch's
  value on this hardware today rests on the mechanism and the deterministic
  budget argument above, not on a before/after contrast. A reviewer should not
  read the after column as evidence the pre-fix file was flaky *here*. The
  reviewer's follow-up above repeats the before column at the highest churn
  this pair can reach and still sees no failure, so neither direction
  reproduces.
* **The kernel reason for the datagram loss is still not established.** It is
  reproducible with plain `UdpSocket` pairs at the same churn rate, is not
  reported to either endpoint, and is not sender-side queue overflow
  (`send_err: 0`). Recorded as measured behaviour of this machine, not a
  diagnosed Darwin bug — unchanged from F54-X2.
* **The measurement machine is shared.** Other agents' builds ran throughout and
  contributed to the absolute load figures. The before and after runs were
  measured under the same conditions with the same harness, and my own 24
  spinners were verified alive for every one of them.
* **Two files, two locks, one machine.** F54-B and F54-C each hold their own
  `LOOPBACK` mutex, so a `cargo test -p cs_net` run still has at most one live
  loopback pair *per binary*, not one per crate. Making the lock crate-wide
  would need a lock shared across two integration-test binaries, which Rust
  cannot express without a library or a file lock; that is a design question
  for the crate, not this task.
* **`accept_f54_b_handshake_admits_and_returns_a_grant_over_udp` deliberately
  holds three sockets at once** — the first `Link`'s host, its client, and a
  second client connecting to the same host to take the next peer id. So the
  lock guarantees one `Link` at a time, not one socket at a time, and the
  scenario needs the second peer, so it was not serialized further.
* **`CS_CAPABILITIES` on this machine is `retail,gpu,audio`.** Nothing in this
  task needed retail data; every measurement is loopback-only, which is
  `network_local` per `AGENTS.md`. Nothing here is `network_real` evidence.
* **Deadlock hazard carried forward.** `Link::new` takes `loopback()` itself, so
  any test in this file that takes `loopback()` directly **and** builds a
  `Link` will self-deadlock: `std::sync::Mutex` is not reentrant, and a
  self-deadlock on a held lock passes `fmt`, `clippy` and compilation. The new
  test builds its own host and client directly for this reason, and this file
  was audited for the shape — a function taking `loopback()` and also reaching
  `Link::new` — and the new test is the only such function, deliberately.

## Sources

`crates/cs_net/src/transport.rs`, the F54-B and F54-C acceptance files, and the
pinned sources in the cargo registry:
`renet2-0.16.1`, `renet2_netcode-0.16.1/src/{native_socket,client,server}.rs`,
`renetcode2-0.16.1/src/{client,server,token,packet,lib}.rs`.
`AGENTS.md`, the project instructions, and `docs/contracts/UI-NETWORK.md`.