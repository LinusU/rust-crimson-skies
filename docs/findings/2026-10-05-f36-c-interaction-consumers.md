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

- No ECS system calls `on_actor_destroyed` yet: `cs_app::damage` has no per-actor destroyed event to subscribe to, and adding one is outside the owner paths. Needs a follow-up task that emits an actor-destroyed event from the damage consumer.
- Rebinding input, HUD, spyglass, sounds, weapons and networking to the new actor on a swap is not implemented; the session reports `TransferReport.camera_actor`/`control_actor` for those consumers to read. No such consumers consume it yet.
- Retail verification of any lifecycle rule is F36-D.
