# F58-C: reconnect/disconnect and clean host-loss flows — what landed and what stays open

Date: 2026-10-10. Task: F58-C "Add reconnect/disconnect and clean host-loss flows"
(`specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`, stage `### F58-C`).
Capabilities used: **ordinary build/test only** — no `retail` (nothing here reads the
installation), no `network_real` (loopback/synthetic fixtures only), no human evidence.
Test prefix: `accept_f58_c_`.

## The declared slice: functions/files and one observable failure

Listed before editing, as the sheet requires:

- `crates/cs_net/src/recovery.rs` (owner path) — the *rules*: `DisconnectCause`
  (voluntary / timeout / abuse naming the refusal / host loss, with
  `wire_reason()`), `DepartureRecord`, `Settlement::{Applied, Duplicate}`,
  `DepartureLedger::{get, check, record}` and its own `DepartureError::LedgerFull`.
- `crates/cs_app/src/network/recovery.rs` (owner path) — the *wiring*:
  `RecoveryFlow::{departure_cause, receive_and_depart, depart, host_lost, recover}`,
  the result types `Departure`, `HostLossOutcome`, `HostLossAction`, `SettledInbound`,
  `FlowError`, and `SessionReceiver::release_actor` (the teardown half of `bind_actor`).
- Wiring-only doc edits in `crates/cs_net/src/lib.rs` and
  `crates/cs_app/src/network/mod.rs` (both named F58-C as "not here yet").
- Tests: `crates/cs_net/tests/accept_f58_c_departure_ledger.rs` (3),
  `crates/cs_app/tests/accept_f58_c_disconnect_settles_carried_objectives.rs` (3),
  `crates/cs_app/tests/accept_f58_c_host_loss_ends_the_match_once.rs` (2).

**Observable failure without the implementation:** no production code owns a
departure. A peer that disconnects while carrying the match's objective leaves the
board holding `ObjectiveState::Held(dead_peer)` forever — nobody queues the drop —
and because two producers can report the same departure (the client's farewell and
the transport's dead-link notice) there is no guard that could keep a second report
from applying a second settlement. With F58-C the first report queues the drop and
the board applies `Transition::Dropped` once at the closing tick; the second report
is `Settlement::Duplicate`, queues nothing, and the next tick has no ruling at all
(`accept_f58_c_a_disconnect_while_carrying_resolves_the_objective_once`). The second
observable failure is the host loss: nothing ended the match or resolved the pilot's
mid-dock latch, so pose/control stayed with a latch controller whose peer was gone;
`RecoveryFlow::host_lost` now aborts it exactly once with `AbortReason::Disconnect`
and returns control to the aircraft
(`accept_f58_c_host_loss_ends_the_match_once_and_sends_every_client_to_menu`).

## The rules this stage adds

1. **A departure is settled once per peer.** `DepartureLedger::record` answers the
   first report with `Settlement::Applied { tick }` and every later report of that
   peer with `Settlement::Duplicate { tick }`, keeping the *first* cause of record.
   The duplicate short-circuits before the teardown runs, so two producers of the
   same departure cannot apply anything twice.
2. **The bound is paid before any state moves.** `DepartureLedger::check` runs first
   in `RecoveryFlow::depart`; a full ledger refuses with
   `DepartureError::LedgerFull { max: MAX_SESSION_PEERS }` while the session is
   unchanged. The cap is exactly the number of peer ids `PeerAllocator` will ever
   issue in a session (ids are never recycled), so the ledger cannot outgrow the
   session, and a flood of departure notices for unknown peers is refused by name.
3. **The teardown is the receive boundary's own.** `depart` releases the aircraft
   binding (`SessionReceiver::release_actor`, so a respawn or a reconnect reclaims it
   through the match's `PilotBindings`, never through the client's word) and forgets
   the peer's membership and replay window. A later packet from that peer is
   `UnauthenticatedPeer`, which the declared dispositions already classify as a
   disconnect.
4. **The settlement runs against the server's own clock.** The drop event is stamped
   with the tick the *caller* names (`MatchStage::server_tick` on the packet path,
   never anything the client sent) and with a reserved producer serial
   (`u32::MAX`), so it can never collide with a peer-produced event of the same tick
   and is judged after that tick's own events.
5. **A refusal is propagated, not swallowed.** `FlowError::Settlement(SessionError)`
   names the layer and the field; nothing is recorded when the match refuses the
   settlement, so a corrected report still settles, and the failure never looks like
   a success (`accept_f58_c_a_refused_settlement_is_named_and_recorded_nothing`).
6. **Host loss ends the match once.** `host_lost` records the loss, aborts the
   mission's in-flight interactions with `AbortReason::Disconnect` on the first
   report only (an aborted interaction is terminal, so `abort_all` itself resolves
   once), and hands back `HostLossAction::ReturnToMenu` — the contract's
   `match -> results -> lobby` path. It forces `RecoveryPolicy::match_running` false
   for every later `recover`, so nothing resumes into a match that is over.
7. **Reconnect is a retry.** `recover` runs the F58-A `decide_recovery` and, on a
   `Resume`, reopens the boundary on the fresh epoch (the old gate's membership and
   replay windows die, so every packet of the prior connection is stale) and admits
   the newly allocated peer id. Every client claim is still refused in order and the
   only state source stays `ResumeState::FullAuthoritativeSnapshot`. The departure
   ledger deliberately survives the epoch change: it is match-scoped, like
   `PilotBindings` and `RewardLedger`.
8. **The abuse reason reaches the client only as far as the wire allows.**
   `DisconnectCause::wire_reason()` maps voluntary/timeout/host-loss onto
   `message::DisconnectReason` and returns `None` for abuse, because that enum still
   has no abuse arm (task #815). Nothing maps abuse onto `SessionEnded`.

All values are newly authored engine design. No original disconnect, timeout, abuse
or host-loss behavior has been measured, and none is asserted.

## The acceptance scenario (spec AC03)

"Disconnect during docking or objective carry; authoritative state resolves once."
Both halves are driven through production code:

- **Objective carry** — `accept_f58_c_disconnect_settles_carried_objectives.rs`:
  a peer claims the flag, disconnects on the wire, and the authoritative
  `cs_sim::multiplayer::MatchSession` drops it once (one ruling, one transition,
  `ObjectiveState::Dropped`, no capture recorded for a drop), with the boundary torn
  down in the same step. The abuse producer (`…_an_abusive_packet_settles_a_cut_off_with_a_bounded_reason`)
  drives the same flow from an oversized packet, and `…_a_refused_settlement_is_named_and_recorded_nothing`
  covers the error path.
- **Docking** — `accept_f58_c_host_loss_ends_the_match_once.rs`: a latched synthetic
  docking attempt (the state a pilot is in when the link dies) reaches
  `InteractionState::Aborted` exactly once with `AbortReason::Disconnect` and control
  back to the aircraft, and the second report of the same loss aborts nothing.

The *mid-match, single-peer* docking case is **not** covered here; see limitation 2
below.

## Not done here / open limitations (each filed, none guessed)

1. **The pinned transport's pump still has no `MatchStage`.**
   `cs_net::lifecycle::ServerSession::pump` and `cs_net::transport` admit packets
   straight through `SessionGate::admit`, so on the wire path only the F54-A caps and
   F54-C's bounded queue are live; the F58-B rate budget and tick window are not
   charged there. `lifecycle.rs`/`transport.rs` are outside F58-C's owner paths and
   the change is logic, not lib.rs wiring, so it was not made. Filed as
   **#1221** (`F58-C-followup-wire-match-stage-into-pump`), on top of F58-B finding 2.
2. **A single peer's mid-dock disconnect cannot be settled precisely.**
   `cs_sim::interaction::InteractionSession` exposes `actor_destroyed(actor)` (wrong
   reason) and `abort_all(reason)` (aborts *every* pilot's in-flight interaction) but
   no per-actor abort, and `cs_sim` is not an F58-C owner path. Rather than abort
   honest pilots' docking because one peer vanished, the flow settles docking only on
   a match-ending departure (`host_lost`, where `abort_all` is correct) and records
   the gap. Affected content: co-op mission interactions (docking, passenger pickup,
   boarding, aircraft swap) of a pilot that disconnects while the match continues —
   the attempt stays in a non-terminal stage until **#1222**
   (`F58-C-followup-per-actor-disconnect-abort`).
3. **`DisconnectPolicy` (KeepScore / StrikeScore / EndMatch) has no consumer.**
   `cs_sim` has no API to strike a score from the standings or to force-end a running
   match, and `cs_net::rules` is not an owner path, so F58-C did not fake one. Until
   **#1223** (`F58-C-followup-apply-disconnect-policy`) lands, a departure settles the
   objective and the boundary the peer held but leaves its standing untouched — which
   is `KeepScore` behaviour for every mode, whether or not that mode asked for it.
4. **The wire has no abuse arm** (`message::DisconnectReason`), so an abusive peer is
   hung up without a reason packet rather than told a different one; task #815 owns
   that arm. `DisconnectCause::wire_reason()` returns `None` for abuse today.
5. **`DisconnectCause::Timeout` has no in-packet producer.** No decoded packet can
   carry it: the transport's dead-connection report is the producer, and no app code
   pumps a `ServerSession` yet (task #592), so a caller reports it to `depart`
   directly. The flow settles it identically to a farewell.
6. **The flow holds no roster.** Membership belongs to the lobby, so a host reports
   each member's departure through `depart`; `SessionGate` has no member iterator and
   `host_lost` therefore cannot tear every member down in one call.
7. **A departed peer's rate-budget entry is kept.** `RateBudget` has no per-peer
   removal and its state is bounded by `MAX_SESSION_PEERS`; peer ids are never
   recycled, so a stale charge can never be attributed to another pilot. Recorded so
   no one claims the budget is per-*live*-peer.
8. **The intent-layer loadout check still does not re-run the F44 budget judge**
   (F58-B finding 3, unchanged here), and `ClientIntent` remains a server-side
   vocabulary the wire cannot express (F58-B finding 4).

## Evidence boundary

No original data, no original executable, no network beyond synthetic fixtures on
one machine. Nothing here is `retail`, `network_real`, `human_play` or
`human_review` evidence, and a code/test pass awards at most **checked**. F58-D
(capability `network_real`) is what would exercise the adversarial and
interrupted-match matrix against real traffic; it stays blocked on the owner.

## Sources

- `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md` (stage
  `### F58-C`, non-negotiables 1/3/4, AC03).
- `docs/contracts/UI-NETWORK.md` (ownership table, epoch/sequence, reliable delivery
  vs application idempotency, `match -> results -> lobby`).
- `docs/findings/2026-10-02-f58-a-threat-cases-and-session-identity.md`,
  `docs/findings/2026-10-09-f58-b-intent-validation-and-rate-caps.md` (what F58-C was
  declared to wire, and what it may not reach),
  `docs/findings/2026-10-03-f54-c-session-lifecycle-and-bounded-processing.md`
  (the `SessionReceiver` vs `HostTransport` gate split and the reconnect-policy
  pointer left to F58-C).
- Owner-path code: `crates/cs_net/src/{recovery,validation,lobby,rules,message,bounds,compat}.rs`,
  `crates/cs_app/src/network/recovery.rs`, `crates/cs_sim/src/multiplayer/{session,objective,result}.rs`,
  `crates/cs_sim/src/interaction/{session,state,transaction,synthetic}.rs`.
