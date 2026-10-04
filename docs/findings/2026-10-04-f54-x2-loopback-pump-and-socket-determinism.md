# F54-X2: why the cs_net loopback suite failed on a loaded machine, and what it is worth knowing

Task: #605 (F54-X2). Found while fixing #600 (F54-X1); see
`docs/findings/2026-10-04-f54-x1-retry-hang-up-test-determinism.md` for the
one test that raced the connection layer's clock.

Machine: 11 cores (`sysctl hw.ncpu`), macOS (Darwin 25.x),
`net.inet.udp.recvspace: 786896`, `net.inet.udp.maxdgram: 9216`.
Load was produced honestly by 24 background processes each running a
`while true; do :; done` busy loop (never sleeping), started from
`private/f54x2/load.sh` and confirmed alive with `kill -0` before every
measurement below. Other agents were building on the same machine
throughout; `uptime` load average ran 25-115 against 11 cores, so the
machine was oversubscribed by 2x-10x, which is what the acceptance
criteria ask for.

## The question

`crates/cs_net/tests/accept_f54_c_lifecycle.rs` fails broadly and randomly on
a loaded machine. `Link::pump_until`'s `MAX_ROUNDS = 2_000` does not survive
it. The symptoms are `connection timed out during request step`, "the grant
never arrived", "the handshake was refused". Three candidate causes were on
the table: packet loss at `send_to` (`ENOBUFS`/`EAGAIN`), receive-queue
overflow, and plain scheduler starvation. All three are measured below.

## What was measured

### 1. The failing suite, before (origin/main `aa688133`, `3cbb1ab206a0d5c5`)

20 runs at `--test-threads=16`, 24 spinners, load average 25-40: **5 of 20
runs failed**, every failure in
`accept_f54_c_a_retry_hangs_up_the_connections_that_hold_no_peer` (which
#600 fixes on its own branch).

50 runs at `--test-threads=16`, 24 spinners, load average 25-40: **11 of 50
runs failed**, spread over 18 distinct tests. Panic messages, counted:

| count | message |
| --- | --- |
| 10 | the retry hung up on the connection that held no peer id (#600) |
| 10 | the grant never arrived (`pump_until` hit `MAX_ROUNDS`) |
| 4 | `the handshake granted a session: [Dropped { reason: Transport("disconnected: connection timed out during request step") }, ...]` |
| 2 | `the client holds the named reason: Closed(TransportLost("disconnected: connection timed out during request step"))` |
| 1 | the handshake was refused |
| 1 | `an unencodable handshake answer is reported, not swallowed` |

So the residual after #600 is: a loopback handshake that does not settle
inside the round budget, and the pinned connection layer declaring the client
lost while it happens.

### 2. Is it `send_to` failing? No.

`crates/cs_net` hands a plain `UdpSocket` to `renet2_netcode`'s
`NativeSocket`, whose `send` is `self.socket.send_to(packet, addr)?` and
whose `try_recv` is `self.socket.recv_from(buffer)`
(`renet2_netcode-0.16.1/src/native_socket.rs:41-51`). A diagnostic harness
wrapped both sockets to count calls and outcomes. Over ~16,500 handshakes:

* host `send_err: 0`, `recv_err: 0`, on every single sample;
* client `send_err: 0`, `recv_err: 0`, on every single sample.

**`send_to` never reported an error.** The `ENOBUFS`/`EAGAIN` hypothesis is
refuted.

### 3. Is it receive-queue overflow? Sometimes, but not here.

Two controls with plain `UdpSocket` pairs, no netcode:

* **Undrained receiver, stable sockets**: 200,000 datagrams of 1200 bytes
  sent at one pair, receiver never reading, then drained: **639 delivered,
  199,361 silently dropped, 0 send errors**. That is macOS dropping into a
  full receive queue without telling the sender, and it is the reason
  `SO_RCVBUF` is not a lever here — there is no error to react to.
* **Receiver draining every round, stable sockets**, 11 processes in
  parallel, 24 spinners, load average 78: **75 sent / 75 received in each
  direction, 11/11 processes, 0 errors**. So at this load, with the receiver
  draining, the plain path does not lose.

### 4. So where does the loss come from? Fresh sockets, and concurrency.

Plain UDP with the *same socket churn* the handshake loop has — a new pair
bound and dropped per iteration, one datagram every 16 rounds, receiver
draining — 11 processes in parallel:

```
pairs_ok=1120 pairs_with_loss=380   sent=88500 received=87289 lost=1211
pairs_ok=1143 pairs_with_loss=357   sent=88500 received=85058 lost=3442
...
```

**380 of 1500 socket pairs (25%) lost one or two of 59 datagrams**, always
near the start of the pair's life, with `send_to` reporting success on every
send. Across the 11 processes: 0.5%-4% of datagrams lost.

The netcode handshake is far more sensitive to that than plain UDP, because
it needs a *complete, correctly ordered* four-message exchange:

* client `ConnectionRequest` -> host `Challenge` -> client `Response` ->
  host `KeepAlive` -> client's hello -> host's grant.

A representative stuck handshake, straight from the counting harness:

```
STUCK rounds=1200 host_port=127.0.0.1:51041 client_port=127.0.0.1:55825
  host   sent 38 (12387 B)  recv 38 (40964 B)  send_err 0  recv_err 0
  client sent 59 (63602 B)  recv  0 (0 B)      send_err 0  recv_err 0  would_block 938
```

The client sent 59 datagrams and the host received 38; the host answered 38
times and the client received **none**. The client's `try_recv` was polled
938 times and returned `WouldBlock` every time. `938 rounds x 16 ms = 15.0 s`
— which is the number in the next section.

The pinned layer's own `TRACE` log, captured through a temporary `log`
implementation, shows the same shape from the other side: the host logging
`Connection request from Client 193` many times over while the client logs
`Received packet from server: Challenge` once, or not at all. No
`Discarded packet from unknown server`, no `Failed to decode packet`, no
`Failed to send packet`, no `Ignored connection response` — the datagrams
simply were not in the receiver's queue.

### 5. Why loss is fatal: the window is 15 seconds of *pump* time, not wall time

`ConnectToken::generate(current_time, protocol_id, expire_seconds,
client_id, timeout_seconds, ...)`. `renetcode2-0.16.1/src/client.rs:107-119`
calls it with `300, client_id, 15`, so:

* `expire_seconds = 300` (token lifetime),
* **`timeout_seconds = 15`** — the window in which the peer must be heard.

`NetcodeClient::update_internal_state` (`client.rs:303-304`) accumulates
`current_time += duration` and compares
`last_packet_received_time + timeout_seconds < current_time`. `last_packet_
received_time` only moves when a *state-matching* packet decodes. So the
window is measured in the `elapsed` a caller hands `update`, nothing else.
A test pumping at `STEP = 16 ms` gets **15 seconds of pump time in about four
milliseconds of wall clock**.

Once it fires the client is `ClientState::Disconnected(ConnectionRequest
TimedOut)`, the pinned layer advances `server_addr_index` past the single
address it has and returns `NetcodeError::NoMoreServers` — visible in the
captured log as `Failed to update client: client has no more servers to
connect`. There is no reconnect path in `ClientTransport`; the failure is
terminal.

And `MAX_ROUNDS = 2_000` at `STEP = 16 ms` is **32 seconds** of accumulated
time: the old round budget was more than twice the window it was trying to
outlast, so the test could always be waiting for something the connection
layer had already given up on. **A bigger round budget cannot fix this**, and
that is what the measurement shows: 435 of 1500 handshakes in the
11-concurrent-loop diagnostic never settled in 1200 rounds.

### 6. Concurrency is the multiplier

Same harness, same machine, same load, one loopback handshake at a time per
process:

| concurrency | handshakes | never settled |
| --- | --- | --- |
| 1 (single-threaded, 24 spinners, load ~25) | 600 | **0** |
| 11 processes (24 spinners, load ~40-115) | 16,500 | **555 (3.4%)**, and 435 of those at 1200 rounds |

Mean rounds for a *settled* handshake: 24.6 single-threaded, 92.1 at 11-way
concurrency, and the tail runs out to the window. Rounds are pure CPU — a
1200-round handshake takes ~10 ms of wall clock — so this is not the test
sleeping and not thread starvation inside the round loop; it is the machine's
loopback UDP path losing datagrams when many short-lived socket pairs churn
at once, and the pinned window turning each loss into a dead connection.

Starvation *is* part of the story in the ordinary sense: at
`--test-threads=1` the whole binary failed 0 of 10 runs under the same
spinners, where at `--test-threads=16` it failed 11 of 50. But starvation
alone cannot lose a datagram on loopback — it only delays delivery — so the
loss above is the mechanism and concurrency is what makes it likely.

## What was changed, and what it does not change

Production (`crates/cs_net`):

* `transport::ConnectWindow` — the connection layer's silence window in whole
  seconds, with `ConnectWindow::new` (panics on a non-positive value, because
  the pinned layer reads zero and negatives as "no timeout at all", a
  development-only setting) and `ConnectWindow::from_seconds` (an `Option`).
* `DEFAULT_CONNECT_WINDOW = ConnectWindow::new(15)` — **the pinned stack's own
  value**, asserted by a test. Every shipped session runs with it; no default
  moved.
* `ClientTransport::connect_with_window` / `ClientSession::connect_with_window`.
  `ClientTransport::connect` / `ClientSession::connect` delegate to them with
  the default, so existing callers are unchanged.
* The widened token is sealed with the same all-zero key `renetcode2` uses for
  `ClientAuthentication::Unsecure`, and the host stays in
  `ServerAuthentication::Unsecure` — which `NetcodeServer::new` configures
  with that identical key and reads `timeout_seconds` out of the token it
  decodes. No key is exchanged, no handshake behaviour changes: a widened
  client still gets the same grant and is still refused with the same named
  reason, which the new test asserts.

There is no host-side `bind_with_window`: the host has no window of its own
under unsecure authentication, and inventing one would be a lie about what
the pinned layer does.

Acceptance file (`crates/cs_net/tests/accept_f54_c_lifecycle.rs`):

* `LOOPBACK_WINDOW = ConnectWindow::new(120)`, used by every client and raw
  peer the file builds.
* `MAX_ROUNDS` is now **derived** from the window —
  `window_seconds * 1000 / 16 + 64` — so a round budget can never again
  outlast the connection layer's patience. It is still a bound on rounds: no
  test sleeps on the wall clock, and no expectation was replaced by a wait.
* `static LOOPBACK: Mutex<()>` with `fn loopback()`, held by `Link` for as
  long as its two sockets are open and taken explicitly by the tests that
  build a host or client directly. The file now runs **one live loopback pair
  at a time**.

No assertion was weakened, no test was skipped or `#[ignore]`d, and no round
budget became a sleep.

## After

Same binary path, same machine, same 24 live spinners on 11 cores, load
average ~106:

* **50 of 50 runs of `accept_f54_c_lifecycle` at `--test-threads=16` passed**
  (before: 11 of 50, with 24 spinners at load 25-40).
* 30 tests, 0 failed, whole binary in 0.02 s — serializing costs nothing.

## Independent reproduction (review, `bunny-alpha-2` reviewing its own branch)

The reviewer re-ran the acceptance measurement from scratch rather than taking
the numbers above on trust. Same machine (11 cores), its own 24 spinners
confirmed alive with `kill -0` (24/24), load average 85-99:

* **50 of 50 runs of `accept_f54_c_lifecycle` at `--test-threads=16` passed.**
* 30 passed, 0 ignored, 0 measured, 0.02 s per run — the same shape as above.
* The one configuration in the new acceptance test that is *not* generous —
  the `ConnectWindow::new(1)` client, whose handshake has only ~62 rounds of
  pump time and four `NETCODE_SEND_RATE` retransmits
  (`renetcode2-0.16.1/src/lib.rs:61`, 250 ms) to work with — was run **200
  times on its own at `--test-threads=1` under the same load: 200 passed, 0
  failed.** That sub-case was the reviewer's main worry — a one-second window is
  the one configuration in the file with no margin — and it is not a flake
  waiting to happen.

Two probes, to check that each half of the fix is load-bearing:

* **`connect_with_window` ignores its argument** (uses the default instead):
  `accept_f54_c_the_connect_window_is_a_fixture_parameter_over_the_default`
  fails, as it must —
  `after 256 silent rounds (ConnectWindow(1) window) the connection layer's
  verdict is the window's: left: true, right: false`.
* **The window stays widened but `loopback()` hands every caller its own
  private mutex**, i.e. the serialization alone is removed and nothing else
  changes: **1 failure in 30 runs** at the same load, and the single failure is
  `accept_f54_c_a_retry_hangs_up_the_connections_that_hold_no_peer` — the
  assertion **#600 (F54-X1) owns**. So the mutex is carrying this file, and the
  only residue left without it belongs to the other task, which is exactly the
  interaction recorded under "Limits".

### Reviewer corrections to this branch

* Two new rustdoc warnings, both broken intra-doc links introduced by this
  branch: `ConnectWindow`'s docs pointed at a non-existent `[`DEFAULT`]`
  (the item is `DEFAULT_CONNECT_WINDOW`) and at a non-existent
  `HostTransport::bind_with_window` — a host-side method that deliberately does
  not exist, as this finding says two sections below. Both links now name what
  actually exists, and the doc states plainly that there is no host-side
  counterpart and why. A third, stale `[`UnsecureWindow`]` link on the private
  `UNSECURE_CONNECT_KEY` named a type that never existed; rustdoc does not
  resolve links on private items, so it was silent, and it is fixed too.
* `MAX_ROUNDS` derived the round budget as `window * 1_000 / 16 + 64`, with
  `16` hardcoded. `STEP` is the thing that number means, so a change to `STEP`
  would have silently broken the one guarantee the derivation exists to give.
  It is now `window * 1_000 / STEP.as_millis() + 64`.
* `DEFAULT_CONNECT_WINDOW` was built with the tuple constructor
  `ConnectWindow(15)`, bypassing `ConnectWindow::new`'s validation, so a
  non-positive default would have installed a silent "no timeout at all". It is
  now `ConnectWindow::new(15)`, which is a compile-time const-evaluation error.
* One comment in
  `accept_f54_c_a_retry_hangs_up_the_connections_that_hold_no_peer` claimed four
  one-second rounds stay inside "the connection layer's own five-second
  timeout". Five was already wrong on `origin/main` (the pinned default is
  fifteen) and this branch widens the window again. **This correction was
  written, then dropped as obsolete** when #600 landed and replaced that prose
  with an accurate version — see the rebase note below.

### Reviewer note: rebasing onto #600 (F54-X1), which landed mid-review

#600 merged into `main` while this review was running, touching all three of
this branch's files, so the rebase needed resolving and the owner directive's
lighter-check exemption did not apply (condition (b) fails: the brought-in
commits touch this branch's files). The full four checks were re-run.

Both conflicts were in the one test #600 owns,
`accept_f54_c_a_retry_hangs_up_the_connections_that_hold_no_peer`, and both
resolved the same way: **#600's version wins.** It rebuilt that test around the
`Link` helper and a single [`STEP`] clock for both sides, and `Link::new`
already connects through `connect_with_window` with `LOOPBACK_WINDOW` and
holds the loopback mutex — so #600's structure already has everything this
branch adds, and the direct `ServerSession::bind` / `connect_with_window` code
this branch had introduced there was the older shape #600 deliberately
replaced. Nothing of #600's logic or assertions was dropped.

The reviewer's planned comment correction in that test became **obsolete and
was dropped**: #600 rewrote the surrounding prose with an accurate version of
its own, so there was no stale "five-second timeout" left to fix. The other
corrections (the intra-doc links, the `MAX_ROUNDS` derivation, the
const-validated default) applied cleanly and are unaffected.

### The rebase shipped a deadlock, and the full check set is the only thing that caught it

The first resolution kept **both** sides of that conflict: this branch's
`let _loopback = loopback();` at the top of the test *and* #600's
`Link::new(...)`, which takes the same mutex itself. `std::sync::Mutex` is not
reentrant, so that one test blocked on a lock it already held and **every other
loopback test in the binary queued behind it forever** — 13 tests reported
"running for over 60 seconds" and `cargo test -p cs_net` never finished. The
test file compiled, `fmt` was clean and `clippy` was silent: a self-deadlock on
a held `Mutex` is invisible to all three. Only running the suite found it.

Fixed by dropping the redundant `let _loopback` from that test, since
`Link::new` holds the guard in `self._loopback` for the life of the link. The
whole file was then audited for the same shape — a function that takes
`loopback()` directly *and* calls something that takes it again (`Link::new` or
`granted_client`) — and that is the only instance.

Two things are worth carrying forward:

* **The owner directive's lighter-check exemption would have merged this.** It
  applies when the rebase brings in commits that touch none of the branch's
  files; here the brought-in commits rewrote all three, so the exemption's
  condition (b) fails and the full four checks are mandatory. This is exactly
  the case the condition exists to exclude, and it is worth re-running the
  *whole* suite — not just the task prefix — after any rebase that touches the
  same test file as the change.
* **`link()`'s poison recovery is not a deadlock guard.** `loopback()` uses
  `unwrap_or_else(PoisonError::into_inner)`, which recovers from a panic
  poisoning the lock. It cannot recover from a thread that holds the lock and
  waits for it again, which is the failure that actually happened.

After the fix: 20 consecutive runs of the binary at `--test-threads=16`, each
hard-capped at 60 s so a hang would surface as a timeout: **20 passed, 0
failed, 0 hung**, 30 tests in 0.01 s.

This is the outcome Probe B predicted one commit earlier: without this
branch's mutex the only failing test was #600's, and with #600 landed that test
is now driven from one clock. #600 and this branch are complementary and touch
no common logic.

## Sensitivity

`accept_f54_c_the_connect_window_is_a_fixture_parameter_over_the_default`
(a) pins the default to the pinned stack's value, (b) pins the refusal of
zero/negative windows, (c) runs a real loopback handshake through the widened
path and checks the grant is the same epoch and the same
`UnsupportedProtocol { offered: 9, supported: 1 }` refusal still comes back,
and (d) shows the window is observable: a client that asked for one second is
dropped by the pinned layer after 256 silent rounds, a client that asked for
`LOOPBACK_WINDOW` is not.

Probing (d) by making `connect_with_window` ignore its argument and use the
default instead:

```
thread '...' panicked at crates/cs_net/tests/accept_f54_c_lifecycle.rs:476:9:
assertion `left == right` failed: after 256 silent rounds (ConnectWindow(1) window)
the connection layer's verdict is the window's
  left: true
 right: false
```

## Limits, honestly

* **The precise kernel reason for the drop is not established.** It is
  reproducible with plain `UdpSocket` pairs at the same churn rate, is not
  reported to either endpoint, and is not queue overflow on the sender
  (`send_err: 0`). It is recorded here as measured behaviour of this machine,
  not as a diagnosed Darwin bug.
* **`CS_CAPABILITIES` on this machine is `retail,gpu,audio`.** Nothing in this
  task needed retail data; every measurement is loopback-only, which is
  `network_local` per `AGENTS.md`. Nothing here is `network_real` evidence.
* **The measurement machine is shared.** Other agents' builds were running
  throughout, so the absolute failure rates in section 1 include load this
  task did not create. The before/after pair was measured under the same
  conditions with the same harness.
* **`accept_f54_b_pinned_transport.rs` has its own `MAX_ROUNDS = 2_000` and its
  own loopback pairs and was not touched** — it is a separate binary, so this
  file's mutex cannot serialize against it, and `cargo test` runs the two
  concurrently. Filed as follow-up work.
* **`HostTransport::reopen` does not reset the netcode server's client
  table.** `renetcode2` ignores a connection response whose `client_id`
  already occupies a slot, and the spent epoch's client is still in that
  slot, so a client that reconnects with the same id after a retry can never
  be told. It is a real product bug, it is not load-dependent, and it is out
  of this task's scope. Filed as follow-up work.

## Sources

`crates/cs_net/src/{transport,lifecycle}.rs`, the F54-C acceptance file,
and the pinned sources in the cargo registry:
`renet2-0.16.1`, `renet2_netcode-0.16.1/src/{native_socket,client,server}.rs`,
`renetcode2-0.16.1/src/{client,server,token,packet,lib}.rs`.
`AGENTS.md`, the project instructions, and `docs/contracts/UI-NETWORK.md`.
