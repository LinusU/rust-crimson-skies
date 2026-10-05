# F36-B: moving-frame eligibility and atomic transfer

Status: designed behavior, synthetic fixtures only. Nothing here is an original-fidelity claim; the original docking, pickup, boarding and plane-swap rules remain unrecovered (see `2026-10-01-f36-a-interaction-state-machines.md`).

## What was added

- `cs_sim::interaction::transfer::InitiatorMotion` derives the initiator's velocity from the f64 **world** positions at both ends of a one-segment sweep. `evaluate_motion_eligibility` feeds that into the F36-A swept `evaluate_eligibility`.
- `cs_app::interaction::initiator_motion` builds it from a `SpatialAnchor`'s swept segment (`from_world` to `world`). The f32 local cache is not read, so an `OriginShift` between the two samples cannot show up as speed. An anchor with no sweep (fresh or teleported) yields `NoContinuousPath`: no velocity is inferred.
- `cs_sim::interaction::transfer::TransferLedger::apply` applies a completed `InteractionOutcome` to pilot and inventory bindings atomically (all preconditions first, then commit), once per `InteractionId`, bound to the session generation. `retry` restores the initial bindings under a new generation.

## Observable failure it prevents

Differencing a pre-rebase local position against a post-rebase one (a 250 km origin move in the test) gives a speed above 1e5 m/s in one tick; `accept_f36_b_local_difference_across_rebase_is_a_false_speed` shows the envelope refusing it, while the world-frame path latches at the true 1 m/s relative speed.

## Known limits (not guessed)

- The sweep is one tick of constant velocity; it does not interpolate sub-tick trajectories. A faster-than-one-tick-per-hook pass is covered by the swept closest-approach test only within that segment.
- Pilot/inventory bindings are plain ids and counts. Binding them to HUD, spyglass, sound, weapons, damage and networking is F36-C; destroyed-carrier-during-latch handling in a running session is F36-C; retail verification is F36-D.
