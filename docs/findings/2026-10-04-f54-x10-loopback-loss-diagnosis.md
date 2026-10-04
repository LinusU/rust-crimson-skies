# F54-X10 — diagnosis of the loopback UDP datagram loss

Date: 2026-10-04. Author: swe2-max-1. Task: Rally #618.

## Verdict

**The loss reproduces today, and it is a kernel-level fault, not an
application bug.** On this machine (Darwin 25.6.0 / 25G83, 11 cores), a
`UdpSocket::bind` call that succeeds can return a socket that the loopback
demultiplexer never routes to: every datagram addressed to its port is
dropped, and the kernel counts the drops as `dropped due to no socket`
while the socket is bound, live, owned by this process and actively
drained. The socket stays unreachable for its whole life. The trigger is
**aggregate concurrent socket churn** — many processes binding, using and
dropping short-lived UDP sockets at once. Serialized pairs were clean at
every load tried; concurrent pairs die at rates from ~5% to ~70%
depending on churn and ambient load.

The same churn also produces a transient per-datagram drop layer that
retransmission absorbs (the widened connect window healed ~84% of
failures) and, much more rarely, duplicate live ephemeral-port
assignments. F54-X2's observed signature ("client sent 59, host saw 38,
client heard 0") sits on the transient/racing edge of the same failure;
today's dominant signature is the cleaner whole-socket orphan.

Recommendation for the loopback suites: **keep serializing socket-pair
setup/use/teardown; do not spend effort on buffer sizing for this
phenomenon.** Buffer sizing fixes a different, correctly-labelled
mechanism that did not occur at the shipped `recvspace=786896`. Details in
§6.

## 1. Machine

Every number below was taken on:

| sysctl | value |
| --- | --- |
| `kern.osrelease` | 25.6.0 |
| `kern.osversion` | 25G83 |
| `hw.ncpu` | 11 |
| `net.inet.udp.recvspace` | 786896 |
| `net.inet.udp.maxdgram` | 9216 |
| `kern.ipc.maxsockbuf` | 8388608 |
| `net.inet.ip.portrange.ipport_allow_udp_port_exhaustion` | 0 |

The host is shared with other agents' builds; ambient loadavg during the
runs was ~80-114. That ambient load is part of the conditions and is
stated wherever it matters; the fleet JSON reports embed the machine
block above in every file under `private/f54x10/`.

## 2. Harness

`crates/cs_net/tests/accept_f54_x10_loopback_loss.rs` — committed, env-var
driven, no scripts thrown away. Entry points (all `#[ignore]`d except the
real test):

- `accept_f54_x10_serialized_transport_pairs_settle` — the task-prefix
  test. Runs 8 serialized `cs_net::transport` host/client handshakes and
  asserts all settle. Runs in the normal suite.
- `f54x10_probe` — one process, `F54X10_WORKERS` in-process threads,
  `F54X10_PAIRS` pairs each. `F54X10_MODE=raw` uses tagged
  `UdpSocket` datagrams; `transport` drives real
  `HostTransport`/`ClientTransport` handshakes. Prints one JSON object.
- `f54x10_fleet` — spawns `F54X10_PROCS` probe subprocesses plus
  `F54X10_SPINNERS` busy-loop processes (pgrep-verified alive, killed
  after), diffs `netstat -s -p udp` counters across the run, aggregates
  worker JSON into one fleet report under `private/f54x10/`.
- `f54x10_wildcard_probe` — explicit socket-collision probe
  (wildcard/specific bind orders, allocator check).
- `f54x10_alloc_storm` — concurrent `bind(127.0.0.1:0)`/`bind(0.0.0.0:0)`
  storm across 24 threads, counts ports held by two live sockets.

Raw pairs are tagged per worker/pair/round/direction so the report
separates `lost`, `stray`, `foreign`, `late_cross_pair`, `send_err`,
`recv_err` and port-`rebound` accounting — a send succeeding is never
taken as proof of delivery. Transport pairs are classified
settled/unsettled with the netcode reason breakdown, plus postmortem
reachability of the dead pair's host socket.

Every number in §3-§5 is reproduced by the command shown in the row's
description; the full command is always a prefix of

```sh
<env vars> cargo test -q -p cs_net --test accept_f54_x10_loopback_loss \
    --locked -- <entry point> --ignored --nocapture
```

with `f54x10_fleet` runs concurrently overlapped by launching a second
fleet binary directly:

```sh
<env vars> target/debug/deps/accept_f54_x10_loopback_loss-<hash> \
    f54x10_fleet --ignored --exact --nocapture &
```

## 3. Does the loss reproduce?

### 3a. Raw datagram pairs churning alone: **no**

`F54X10_MODE=raw` (default), `f54x10_fleet`, `F54X10_SPINNERS=24`:

| config | pairs | datagrams | pairs with loss | lost | stray | foreign |
| --- | --- | --- | --- | --- | --- | --- |
| `PROCS=1/4/8 PAIRS=300` | ≤2400 | ≤307k | **0** | 0 | 0 | 0 |
| `PROCS=11 PAIRS=300` | 3300 | 422k | **0** | 0 | 0 | 0 |
| `PROCS=16/24 PAIRS=300` | ≤7200 | ≤922k | **0** | 0 | 0 | 0 |
| `PROCS=24 PAIRS=800 ROUNDS=8` (max churn) | 19200 | 307k | **0** | 0 | 0 | 0 |
| exact F54-X2 shape: `PROCS=11 PAIRS=1500 ROUNDS=1024 SEND_EVERY=16` | 16500 | 2.1M | **0** | 0 | 0 | 0 |
| `FRESH=recv`, `FRESH=send`, `FRESH=none` at `PROCS=11` | 3300 each | 422k each | **0** | 0 | 0 | 0 |
| `BIND=client` (sender on `0.0.0.0:0`, the transport shape) | 4400 | 1.13M | **0** | 0 | 0 | 0 |

A fleet's own raw churn — up to ~16k bind+close events/s — is not enough
alone. Port rebinding happened constantly (`rebound_pairs=1737/19200` in
the max-churn run) and caused nothing.

### 3b. The same raw pairs **while a transport fleet churns**: yes, and it is exact

`f54x10_fleet` raw probe (`PROCS=6 PAIRS=500 SPINNERS=0`) run
concurrently with a transport fleet (`MODE=transport WINDOW_S=15
SETTLE_ROUNDS=2000 SPINNERS=0`):

| transport churn | raw pairs | raw pairs lost | raw datagrams lost |
| --- | --- | --- | --- |
| 4 procs × 1500 | 3000 | **329 (11%)** | 42112 = 329×128 |
| 11 procs × 900 | 3000 | **917 (30%)** | 117376 = 917×128 |

Every losing raw pair lost exactly 128 datagrams = all of both
directions. `stray=0`, `send_err=0`, `recv_err=0`. A **single serialized**
raw probe (`WORKERS=1 PAIRS=1500`, so its own churn is negligible) run
concurrently with an 11-proc transport fleet lost **222/1500 pairs
(15%)** — `lost=28416 = 222×128`, `lost_positions` uniform (444 at every
position 0-63), `rebound_pairs=262` but `rebound_pairs_with_loss=7`.
Serialization of its own pairs does not armor sockets against ambient
churn.

### 3c. Transport pairs through `cs_net::transport`: yes, strongly

`F54X10_MODE=transport WINDOW_S=15 SETTLE_ROUNDS=2000 PAIRS=600`,
`f54x10_fleet`, `SPINNERS=0`. `unsettled` = handshake did not complete
inside the shipped 15 s connect window.

| churning processes | pairs | unsettled | rate |
| --- | --- | --- | --- |
| serialized (`PROCS=1 WORKERS=1 PAIRS=400`) | 400 | **0** | 0% |
| 4 × 600 | 2400 | 0 | 0% |
| 4 × 1500 | 6000 | 2003 | 33% |
| 8 × 600 | 4800 | 274 | 5.7% |
| 11 × 600 | 6600 | 1575 / 3277 / 3657 across runs | 24-55% |
| 11 × 900 | 9900 | 4745 / 4922 / 5137 / 6795 | 48-69% |
| 16 × 400 (`BIND_DELAY_MS=5`) | 6400 | 1719 | 27% |

The threshold is a **churn rate**, not a proc count: the same 4 procs
gave 0% and 33% under different ambient load, and 11 procs ranged 24-69%.
`unsettled_connected` stayed tiny (0-20/run): ~99% of dead pairs never
finish the 4-packet netcode exchange — they die *before* `Connected`.

## 4. What the failure is

Instrumented runs tag every netcode `log` record and the fleet diffs
kernel UDP counters. Consistent across every unsettled-heavy run:

- `unsettled_host_saw` ≈ 1-6% of unsettled (e.g. 163/4745, 352/3277):
  the victim's host socket never logged `Connection request` — the
  client's requests never arrived.
- `foreign_requests = 0` always: no request was ever mis-delivered to
  another process's host socket (client ids carry a per-process tag).
- `netcode_rejected` ≈ 0-140 per run vs ~200k dropped packets: the few
  datagrams that do arrive decode fine — not garbage, not stale traffic.
- `dropped due to no socket` (netstat) rises by roughly the dead pairs'
  request count (163k-305k/run ≈ ~60 retries × thousands of dead pairs),
  and `ICMP packets for port unreachable` rises identically — each dead
  request is one kernel drop of that class.
- **Postmortem probe**: after an unsettled pair's client drops, a raw
  tagged datagram to the still-live host port is sent and the host
  pumped. `postmortem_orphan` ≈ `unsettled` in every run
  (3656/3657, 2003/2003, 597/597, 4741/4745, 5133/5137, 6795/6795):
  the host socket remains unreachable even after its peer is gone. A
  bound, live, drained socket that demux cannot find — permanently.

So the dominant mechanism is: **bind() returns a socket whose entry in
the kernel's PCB table is never reachable by the loopback demux — an
orphan socket.** When a pair's two binds both land inside a bad
interval, the whole pair dies together (hence exactly 128/128 lost).
When only the client's bind orphans, its ~60 request retries all drop as
"no socket" and the connect window times out.

### The transient layer and the window

Same churn, `WINDOW_S=120 SETTLE_ROUNDS=9000`, `PROCS=11 PAIRS=600`:
597/6600 unsettled — versus 3277-3657 at `WINDOW_S=15`. The wider window
healed ~84% of failures (a transient per-datagram drop layer
retransmission covers), but **597 permanently-orphaned pairs died
anyway** — `postmortem_orphan=597` exactly. Retransmission cannot reach
a socket demux never routes to.

### Refuted alternatives

- **Port theft by explicit bind** — `f54x10_wildcard_probe`: binding
  `127.0.0.1:P` on a port held by a wildcard socket, and `0.0.0.0:P` on
  a port held by a specific socket, refused **2000/2000** each; a
  wildcard ephemeral bind never landed on a held specific port (0/200).
  Sequential collision is impossible.
- **Allocator race** — `f54x10_alloc_storm` (300 storms × 24 threads ×
  8 binds, 57600 live sockets): `bind_errors=0`, but **19 same-shape +
  16 cross-shape duplicate live port assignments**. The kernel *can*
  double-assign a port under racing binds — real, but at ~0.06% per bind
  it cannot explain 30-70% pair death on its own; it is a second
  symptom of the same table contention.
- **Receive-queue overflow** — `F54X10_RCVBUF=8192` produced 344 lost
  datagrams and `dropped due to full socket buffers` rose by exactly
  344. The harness detects real overflow when it exists — and labels it
  differently. At the shipped `recvspace=786896` no pair's queue ever
  filled; overflow is not the observed mechanism.
- **Post-bind visibility window** — `F54X10_BIND_DELAY_MS=5` (and 25)
  gave 0/4400 unsettled at 11 procs but 1719/6400 at 16 procs: the
  delay heals by throttling aggregate churn rate, not by outliving a
  fixed per-pair window.
- **Load alone** — 24 CPU spinners + raw churn at 24 procs: clean.
  Spinners are not required; `SPINNERS=0` transport fleets reproduce at
  30-69% under ambient load.

## 5. Parameters varied

`PROCS` 1-24, `WORKERS` 1-16, `PAIRS` 150-1500/proc, `ROUNDS` 8-1024,
`SEND_EVERY` 1/16, `DGRAM_BYTES` 300-1400, `RCVBUF` 0/8192, `FRESH`
both/recv/send/none, `BIND` loopback/client, `BIND_DELAY_MS` 0/5/25,
`BOTH_DIRS` 0/1, `WINDOW_S` 15/120, `SETTLE_ROUNDS` 2000-9000,
`SPINNERS` 0-24, `MODE` raw/transport. The knobs that moved the number:
**concurrent churn** (`PROCS`×pair-lifetime, the dominant axis) and
**`WINDOW_S`** (absorbs the transient layer only). `RCVBUF=8192` moved
it into a different, labelled mechanism. Everything else was inert.

## 6. Recommendation for the loopback suites

**Keep serializing socket-pair setup/use/teardown. Do not size buffers
for this phenomenon.**

- The fault lives in the kernel's socket table under concurrent churn.
  A suite controls exactly one thing: its own contribution to that
  churn. Serialization removes it, and every serialized run of this
  harness was clean — including the shipped production path
  (`accept_f54_x10_serialized_transport_pairs_settle`).
- Buffer sizing is orthogonal: the observed drops are `no socket`, not
  `full socket buffers`, and the default `recvspace` never overflowed.
  If a future suite ever bursts >768 KB into one socket per drain
  interval it should size that socket — a different problem.
- No application-side recovery exists: an orphaned socket stays
  orphaned (postmortem), so prevention is the only tool. Retransmission
  buys back the transient layer (W15→W120 recovered ~84% of failures)
  but never the permanent one.
- Honest limit: serialization is not armor. A lone serialized probe
  lost 15% of pairs while an unrelated fleet churned the machine. On a
  dedicated CI host with nothing else churning, serialization is
  sufficient; on a shared host running other churning binaries
  concurrently, individual sockets can still be orphaned — so keep
  loopback suites serialized *and* don't co-schedule churning suites
  when determinism matters.

## 7. Honest limits

- The kernel internals are inferred from userspace evidence (counters,
  postmortem reachability, the bind-storm duplicate assignments), not
  kernel tracing — no dtrace/kdebug on this host.
- The orphan rate is a churn×load function; treat the percentages as
  ranges, not constants. The exact F54-X2-era signature (1-2 of 59
  datagrams per pair) was not reproduced today — today's machine
  produces the cleaner whole-socket orphan signature instead.
- `f54x10_alloc_storm` proves duplicate live port assignments happen
  (~0.06%/bind under storm) but its measured rate does not by itself
  account for the observed orphan rates; the two are symptoms of the
  same table contention, not one proven causal chain.
- All measurements are loopback-only (`network_local`); nothing here is
  `network_real` evidence and nothing was taken on real hardware.
