# F54-C follow-up: the work-queue refusal becomes a last resort behind a per-peer rate cap

Date: 2026-10-13. Task: #591 `F54-C-followup-work-queue-last-resort` ("Make the
host work-queue refusal a last resort behind a per-peer rate cap"), follow-up
from `docs/findings/2026-10-03-f54-c-session-lifecycle-and-bounded-processing.md`.
Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md` (the
stage this follows) and `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`
(stage F58-B, which owns the declared rate numbers). Capabilities used:
ordinary build/test only, `network_local` loopback on one machine. No
`retail` input, no `network_real`, no original multiplayer content. Nothing
here asserts anything about the original game's networking; the numbers are the
engine's own design, taken unchanged from F58-B.

## The problem this closes

`ServerSession::enqueue` refused a packet once the shared work queue held
`MAX_WORK_PER_PUMP` (64) and disconnected the offending peer as
`ThreatCase::ResourceExhaustion`. Two things were wrong with that:

1. a single client's burst was *cut off* rather than rate-limited first, and
2. the queue bound is a **shared** resource, so one peer could spend all 64
   slots while every other peer's admitted input was refused behind it.

The F54-C finding recorded this as deliberate-and-filed. This task lands the
missing per-peer, per-tick-window cap (the capability
`ThreatCase::ImpossibleRate` — "more requests per tick window than the session
designed for" — already declared) and keeps the shared bound as the backstop.

## What is implemented: a three-step ladder

`crates/cs_net/src/lifecycle.rs`, `ServerSession::enqueue`, in this order:

| Step | Bound | Where the number comes from | What happens | Connection |
| --- | --- | --- | --- | --- |
| 1. window budget | `max_intents_per_window` (96) per `window_ticks` (60) of the **session's own clock** | F58-B's `RateLimits::DESIGNED`, via `RateBudget` — no new number | refused with `DropReason::RateExceeded { limit, window }`, counted | cut off as `ThreatCase::ImpossibleRate` (the declared disposition) |
| 2. per-tick share | `max_intents_per_tick()` (2) packets per peer per pump round | `RateLimits::max_intents_per_tick()` = `max_intents_per_window.div_ceil(window_ticks)`, i.e. F58-B's own sustained rate rounded **up** to whole packets | refused with `ServerNotice::RateLimited { peer, cap }`, counted in `ServerSession::rate_refused` | **continues** — this is rate limiting |
| 3. shared queue | `MAX_WORK_PER_PUMP` (64), unchanged | F54-A's bound, unchanged | `DropReason::QueueOverflow { limit }`, counted in `overflowed` | cut off as `ThreatCase::ResourceExhaustion` (unchanged) |

The session's rate clock is `ServerSession::rate_tick`: **one tick per
`ServerSession::pump` call**, cleared/advanced from the host's own side only.
A client cannot refill a window by naming ticks. `pump`'s contract already
says `elapsed` is "the host tick's duration", so one pump per host tick is the
documented meaning of the call; `ServerSession::pump` is not yet driven from
`cs_app` (that is F58-C/F58-B's wiring, task #1221), so this is a recorded
assumption of the pump's contract, not a measured frame rate.

### Why step 2 absorbs instead of disconnecting

`ThreatCase::ImpossibleRate` is declared `Disconnect`. Applying that verdict to
*one tick's surplus* would cut off an honest peer whose two input packets
happen to land in the same host round (a frame hitch, jitter on a lossy link)
— which is exactly the "cut off rather than rate-limited first" this task was
filed against. So the two cases are separated:

* a **single tick's surplus** is the rate limiter working: the packets are
  dropped and counted, the connection continues, and the peer's contribution to
  the shared queue stops at its share;
* a peer that keeps over-running the **declared window** (every offered packet
  is charged, queued or not) is refused with `DropReason::RateExceeded` and cut
  off as `ImpossibleRate` — the declared verdict, with F58-B's declared numbers.

Both refusals are named and counted (`ServerSession::rate_refused`), so an
absorbed rate refusal is still visible from outside; that was F54-C's rule for
gate refusals and it holds here. `DropReason::threat()` stays the single
classification point: `RateExceeded -> ImpossibleRate`, `TooManyPeers ->
ResourceExhaustion`, `QueueOverflow -> ResourceExhaustion` (all pre-existing
or declared mappings; no disposition was rewritten).

One consolidation came with it: the three places that hang up on a peer (the
gate refusal in `pump`, the queue bound, the rate ladder) now go through one
`ServerSession::cut_off`, which reports `CutOff` only if the peer was *still a
member* — so a burst that keeps arriving after the verdict no longer repeats a
cut-off that has already happened. The disposition itself is unchanged; only
the duplicate notice went away.

### Why one peer can no longer spend the shared queue

With the designed numbers, `MAX_SESSION_PEERS (32) × max_intents_per_tick (2)
= 64 = MAX_WORK_PER_PUMP`: a full session's per-tick shares are *exactly* the
queue. A single peer therefore contributes at most 2 of the 64 slots per round,
and step 3 becomes reachable only through a session-wide flood (or a consumer
that stopped draining) — which is what "last resort" means here. The test
asserts that arithmetic rather than leaving it implicit.

## API additions (all in `crates/cs_net/`)

* `validation::RateLimits::window()` and `RateLimits::max_intents_per_tick()` —
  the derivation above, on F58-B's own type so that changing `DESIGNED` moves
  the cap with it.
* `validation::RateBudget::forget(peer)` — a departing peer's charge is
  dropped, so the budget describes the members the session holds rather than
  every peer the epoch has seen (`PeerJoined` also begins that id with no
  charge). Without it, a departed peer's entry would stay until
  `close`/`reopen`: peer ids are never reissued inside an epoch
  (`compat::PeerAllocator` does not wrap) and the table is capped at
  `MAX_SESSION_PEERS` keys, so that lingering entry is a hygiene problem and
  not a reachable `TooManyPeers` — but the tables now track membership
  directly.
* `transport::DropReason::RateExceeded { limit, window }` and
  `DropReason::TooManyPeers { max }` — the two named rate/resource refusals.
* `lifecycle::ServerNotice::RateLimited { peer, cap }` — the absorbed
  per-tick refusal, plus `ServerSession::{rate_refused, rate_limits,
  rate_tick}` and the epoch resets in `close`/`reopen`.

`RateViolation::TooManyPeers` cannot fire in practice (peer ids are allocated
by `compat::PeerAllocator`, bounded by `MAX_SESSION_PEERS`, so the budget map
is bounded by 32 keys); it is still handled with a named `ResourceExhaustion`
refusal rather than an `unreachable!()` or a silent drop.

## Behaviour change to an existing test, and why it is not a weakening

`accept_f54_c_the_work_queue_refuses_the_surplus_and_cuts_the_abusive_peer`
(flood 65 packets from **one** peer, expect the queue to fill and that peer to
be cut off) asserted precisely the behaviour this task removes: one peer
spending the whole shared queue. It is replaced by
`accept_f54_c_the_shared_work_queue_bound_holds_under_a_multi_peer_flood`,
whose flood comes from four peers each sending *exactly its per-tick share* —
the scenario the shared bound is now reachable from. **Every assertion of the
old test is kept**: `queued() == MAX_WORK_PER_PUMP`, `overflowed() > 0`, the
named `DropReason::QueueOverflow { limit: MAX_WORK_PER_PUMP }`, the
`CutOff { threat: ResourceExhaustion }`, the cut-off peer no longer being a
member, and `drain_work().len() == MAX_WORK_PER_PUMP`. Two assertions were
added: the arithmetic relation above, and that this flood is *not* answered by
`ImpossibleRate` (so the backstop, not the rate cap, is what fired).

Two new acceptance tests cover the steps that replaced it:

* `accept_f54_c_a_flooding_peer_is_rate_limited_before_it_can_spend_the_shared_queue`
  — a peer offering `max_intents_per_tick() + 1` a round is refused with a
  named `rate` reason and counted, **stays a member** (rate-limited, not cut
  off), never drives `overflowed` above zero, and the honest peer's input is
  admitted throughout. Its round bound is chosen so "still a member" is
  arithmetic: at most `3 × 24 = 72 < 96` offers can have been charged inside
  it, so a cut-off there would have to come from something other than the
  declared budget.
* `accept_f54_c_a_peer_that_keeps_over_the_declared_rate_is_cut_off_with_the_named_reason`
  — the escalation: `DropReason::RateExceeded { limit: 96, .. }`,
  `CutOff { threat: ImpossibleRate }`, `rate_refused() > 0`,
  `overflowed() == 0`, the peer gone from `members`, and the honest peer's
  input admitted **after** the verdict (the wait is over the notices recorded
  from that point, so an earlier admission cannot answer it).

## Test sensitivity

Each probe was applied, run, and reverted (the restored file passes 37/37):

| Implementation removed | Test that failed | Exit |
| --- | --- | --- |
| step 2, the per-tick share | `..._a_flooding_peer_is_rate_limited_before_it_can_spend_the_shared_queue` | 101 |
| step 1, the window-budget charge | `..._a_peer_that_keeps_over_the_declared_rate_is_cut_off_with_the_named_reason` | 101 |
| step 3, the `MAX_WORK_PER_PUMP` check | `..._the_shared_work_queue_bound_holds_under_a_multi_peer_flood` | 101 |

The helper `peer_arrivals` (used to decide whether a fixture carried anything
from a peer) now counts `ServerNotice::RateLimited` as a classification, so a
host that answered a peer only with rate refusals is still seen as having
heard it.

## Commands run (2026-10-13)

```sh
cargo fmt --all -- --check                                             # 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # 0
cargo test --workspace --locked                                        # 0 (exit 0, 537 `test result: ok`)
cargo test --workspace --locked -- accept_f54_c_ --include-ignored     # 0 (37 passed, 0 failed)
```

The task-test prefix for this task is `accept_f54_c_` (the follow-up's own
lineage, in `crates/cs_net/tests/accept_f54_c_lifecycle.rs`).

## Not claimed here

* No original-data, `human_play` or `network_real` evidence. Loopback on one
  machine is `network_local`.
* The intent layer (`validation::IntentValidator`) is still **not** on the wire
  path; this ladder is the session's queue admission, not a second validator.
  Passing a `MatchStage` through the pump so F58-B's caps run for every packet
  is task #1221 (`F58-C-followup-wire-match-stage-into-pump`), and the two must
  not grow separate numbers — this stage deliberately reused `RateLimits`.
* Whether one pump really equals one host tick once `ServerSession::pump` is
  driven from the app is unverified until that wiring lands; if the app pumps
  more often than it ticks, the window refills faster than 96 per second of
  simulation and the cap weakens accordingly. Recorded for #1221.
* A code/test pass awards at most **checked**.
