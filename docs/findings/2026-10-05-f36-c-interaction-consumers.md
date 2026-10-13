# F36-C: interaction session wiring and teardown

Status: designed behavior, synthetic fixtures only. Nothing here is an original-fidelity claim; the original docking, pickup, boarding and plane-swap lifecycle rules remain unrecovered (see `2026-10-01-f36-a-interaction-state-machines.md`).

## What was added

- `cs_sim::interaction::InteractionSession` owns the transactions and the `TransferLedger` together. `complete` commits both or neither: a transaction refusal (lost authorization, destroyed target) aborts it, and a ledger refusal aborts the interaction instead of leaving a Completed transaction whose effects never happened.
- `actor_destroyed` aborts every active interaction involving the actor; `control_of` then names the initiator as the control holder again (it is `Latch(id)` only while Latching/Transferring). Pilot and inventory bindings are untouched by an abort.
- `abort_all` (pause, cinematic cancel, disconnect) and `retry` (abort, drop attempts, restore the ledger's initial bindings under a new session generation; a transaction from the old generation is refused as stale).
- One active interaction per initiator, so a second attempt cannot duplicate pilot or cargo.
- `cs_app::interaction::InteractionRuntime` is the Bevy resource wrapping the session with `on_actor_destroyed` and `on_retry`.

## Observable failure it prevents

Destroying the carrier while the transaction is Latching used to leave control with the latch controller forever (no component notified the transaction). `accept_f36_c_destroyed_carrier_during_latch_returns_control_safely` shows the abort, the initiator regaining control and the pilot/cargo unchanged.

## Known limits (not guessed)

- ~~No ECS system calls `on_actor_destroyed` yet: `cs_app::damage` has no per-actor destroyed event to subscribe to, and adding one is outside the owner paths. Needs a follow-up task that emits an actor-destroyed event from the damage consumer.~~ Closed by task #658 (F36-C.1, 2026-10-13): `cs_app::damage::DamageActorDestroyed` / `publish_destroyed_actors` emit the event from the damage consumer's resolution output, and `cs_app::interaction::InteractionSchedulePlugin`'s `forward_actor_destroyed` forwards it to the runtime; `accept_f36_c1_` tests drive the real schedule end to end. The damage *driver* that hands each tick's resolution to `publish_destroyed_actors` in a running mission is still the app's damage-loop wiring, which no stage owns yet.
- Retail verification of any lifecycle rule is F36-D.

## F36-C.2 follow-up: consumer rebinding from the applied report

The report's `camera_actor`/`control_actor` now have their consumer entries
(F36-C.2, task #659), all in `cs_app::interaction`:

- `TransferConsumers` is the one resource that publishes the input, HUD,
  spyglass, sound, weapons, damage and network-ownership bindings together,
  and `apply_transfer_consumers` is their only writer. It derives every
  binding from one applied `TransferReport` and checks the report against the
  live `InteractionRuntime` first: a report whose interaction the session
  does not hold as `Completed` (aborted, unknown, or dropped by a retry) is
  refused with `TransferConsumerError::NotCompleted`/`StaleSession` and
  changes nothing, and a record that outlives its runtime is dropped
  (`NoRuntime`).
- `teardown_transfer_consumers` is the abort/retry/disconnect path: it
  clears the bindings for their own session generation and refuses a late
  teardown from a previous one.
- `rebind_network_ownership` moves the server's peer-to-actor entry of
  `cs_net::validation::ActorOwnership`: the destination binds first and the
  old aircraft is released only after, so a refusal (a peer that is not the
  pilot, or a destination another pilot owns) leaves the peer with exactly
  the binding it had. The pilot check guards the no-control-moved path too,
  so a peer owning nothing is refused there instead of being told it is
  bound to an aircraft it does not fly.

`accept_f36_c2_consumer_rebind.rs` (13 tests) drives the real session's
applied report through these entries over a real `World` and a real
`ActorOwnership` table. The per-view mapping is checked against a report
whose two fields really differ — `accept_f36_c2_each_view_follows_its_own_report_field`
drives a real policy that moves the camera to the new actor while leaving
control with the initiator — so a view bound to the wrong field fails
instead of passing on a fixture where the fields coincide.

### What is designed here, not recovered

- **Which consumer follows which report field.** Input, HUD, sound, weapons,
  damage and network ownership follow `control_actor`; the spyglass follows
  `camera_actor`. The original game's per-consumer rebind rules (does the
  engine sound follow the camera or the airframe? does the HUD rebind at
  latch or at release? which of the original's damage views followed the
  swapped-in airframe?) are unrecovered; F36-D owns recovering them from
  reference runs.
- **The seven-view record.** Grouping exactly these seven consumers into one
  atomic record is project design following the F30-C `TargetConsumers`
  precedent; the original's swap-commit boundary is unrecovered. Spec F36
  non-negotiable 5 lists damage among the views that rebind "in one
  authoritative transition", so the damage view is in the same record rather
  than a later second writer.
- **The report's field values are trusted once the interaction is
  Completed.** `apply_transfer_consumers` binds the *interaction* (session
  generation + serial + actors) against the live session, but it does not
  re-derive `control_actor`/`camera_actor`; a caller that edits those fields
  after `complete` returned the report is trusted like the session's own
  output. Hardening that needs `cs_sim::interaction` to publish its applied
  actors per completed interaction (it keeps no report after `complete`
  returns one); resolved by an F36 stage that adds that publication — F36-D
  if a reference run shows the rebind must be session-derived, otherwise the
  F36-C follow-up below.

### Still unwired (needs follow-up tasks, outside F36 owner paths)

- No ECS schedule calls `apply_transfer_consumers` yet: nothing in the app
  completes an interaction through `InteractionRuntime` (the mission host
  does not drive a plane swap end-to-end), so the entries are production
  code with a real consumer boundary but no producer system yet. The task
  that drives a swap through the mission host should call
  `apply_transfer_consumers` and `rebind_network_ownership` with the
  session's real report.
- No consumer module reads `TransferConsumers` yet (the HUD frame, camera
  session, audio, weapons and damage read their own authorities today).
  Reading the record is wiring inside those features' owner paths (F46, F21,
  F41, F27 and the F29 damage consumers / the app's damage-loop wiring) and
  is deliberately not guessed from F36. Until the damage consumers read it,
  the damage view of spec F36 non-negotiable 5 is published but unfed, and
  that criterion stays unmet for F36-D and F63.
