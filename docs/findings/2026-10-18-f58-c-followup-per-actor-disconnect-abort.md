# F58-C follow-up (#1222): the per-actor disconnect abort

Closes limitation 2 of
`docs/findings/2026-10-10-f58-c-disconnect-reconnect-and-host-loss.md`: a
single peer disconnecting mid-interaction while the match continues now
settles precisely, instead of the attempt staying non-terminal.

## What changed

* `cs_sim::interaction::InteractionSession::disconnect(actor)` aborts every
  non-terminal interaction whose initiator **or target** is `actor`, with
  `AbortReason::Disconnect` — `actor_destroyed`'s reach with the right
  reason, `abort_all`'s effect without touching honest pilots' attempts.
  Control of each surviving initiator returns to it; no binding changes.
* `RecoveryFlow::depart(peer, cause, tick, match_state, interactions)` takes
  the mission's `InteractionSession` as its last parameter (`None` for a mode
  that holds none, exactly like `match_state`). After the fallible objective
  drops and before the departure is recorded, the aircraft the departed peer
  was bound to calls `disconnect`, and `Departure.aborted` reports what
  aborted. A duplicate report still moves nothing.
* `RecoveryFlow::receive_and_depart` takes the same parameter and passes it
  through, so an admitted farewell settles the peer's dock in the same step.
* `InteractionAbort` is now `Eq` so `Departure` keeps its `Eq` derive.

## Ordering note

The abort runs after the objective-drop loop: a `FlowError::Settlement`
refusal there records nothing and leaves the interactions in flight, so a
corrected report still resolves — and reports — them. `DepartureLedger`'s
bound is paid by `check` before anything moves, so `record` cannot fail once
the abort has run.

## Not done

* Nothing outside tests calls `depart` yet: the flow is a boundary API and
  the runtime consumer is still #592 (no app pumps a `ServerSession`).
* #1223 (`DisconnectPolicy`) is untouched: a departure still leaves the
  departed peer's standing where it was.

## Test sensitivity

Probe applied to the `depart` call site (the `disconnect` call stubbed to an
empty `Vec`, the tree restored afterwards):
`accept_t1222_a_departing_peers_interactions_abort_and_an_honest_pilots_does_not`
failed — `aborted` came back empty. The fixture also pins the two ways this
could be done wrong: the `inbound` attempt matches on the *target* side (an
initiator-only matcher would leave it `Latching`), and the `honest` attempt
staying `Latching` rejects an over-broad `abort_all`. The second test pins
the unbound-aircraft case: with no actor, no interaction can be attributed
to the peer and nothing aborts.

Tests: `accept_t1222_*` in
`crates/cs_app/tests/accept_t1222_per_actor_disconnect_abort.rs` (2 tests).
