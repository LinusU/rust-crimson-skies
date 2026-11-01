# #2222 the ground blow's two per-vehicle gates

Date: 2026-11-01. Task #2222, follow-up of #2107
(`2026-10-29-groundblow-assist-model.md`, section 2). Static claims only
(`OWNER-STATIC-2026-10-29`, bytes in the #2107 finding); the original
executable was not run, nothing here is `verified_original`. Implementer:
`sonnet-2`; not independently reviewed.

## Wiring

Both gates are inputs to the law, not state it owns:

* `OriginalInput::status_word` (`V+0x358`): `INCAPACITATED_STATUS` (4) runs no
  ground blow for either seat (`0x48c22b`).
* `OriginalInput::collision_grace_until_s` (`V+0xac`) with
  `OriginalInput::sim_time_s` (`[0x71c470]`): a **non-player's** blow is skipped
  while `sim_time_s < collision_grace_until_s` (`0x48c241`). A player is never
  gated by it. The default `0.0` never gates.

`OriginalFlightModel::rotation_increment` applies `ground_blow_gated` before
the block runs, so a gated vehicle gets neither the torque term nor the
velocity steering. Input fields were chosen over `OriginalState` because the
law keeps no clock and the only exhaustive `OriginalInput` literal outside
tests is `playtest::scene::PlaytestScene::input`, which sets all three to the
disarmed values.

## Still not wired (affected content, resolving task)

Both fields are written now (below). What remains is not a writer but
coverage: `+0xb4` damping stays unmodelled (no site assigns it a non-zero
value), and no retail mission yet reaches either gate, because the mission
ordnance step supplies no blast recipients and no bound loadout carries a
`SONIC`/`FLASH` record (see the #2252 limit in
`2026-10-28-t452-r05-slot-map-and-incapacitation.md`). No constant was tuned.

## Wired by #2251 (`V+0xac`)

Static claims only, not `verified_original`; the bytes are the ones above
(`0x48d31e`..`0x48d395`, `now + 1.0`, both vehicles), no new image read.

* `crates/cs_app/src/playtest/collision_grace.rs` keeps the deadline in the
  app's own `CollisionGrace` component (absent means `0.0`, never gates).
  `write_collision_grace` runs in `FixedLast` and, for every solid contact
  report of the tick whose two bodies are both on `CollisionLayer::Aircraft`
  and both carry a `PlaytestOriginalFlight`, writes
  `session_time_s(report.tick) + COLLISION_GRACE_S (1.0)` on **both**.
  Sensor overlaps and contacts with anything else write nothing. "Class 0/4
  vehicle" is read as the port's aircraft records; the finding does not say
  whether a player-into-AI contact takes the same branch as AI-into-AI (the
  `0x48d375` text says the grace is set on the non-player path), and the port
  writes it for every aircraft pair.
* `drive_original_flight` states `sim_time_s` (fixed ticks times the fixed
  timestep, converted once) and the record's deadline in every record's
  `OriginalInput`; the law alone decides who is gated, so a player stays
  ungated by construction.

## Wired by #2252 (`V+0x358`)

Static claims only, not `verified_original`; the bytes are the ones in
`2026-10-28-t452-r05-slot-map-and-incapacitation.md`, no new image read.

* `playtest::scene::overlay_incapacitation` reads the mission ordnance
  session's status ledger (the one writer of
  `StatusEffectKind::Incapacitation`) and, while
  `incapacitation::incapacitated_mode` reports the mode for the record's
  `DamageActorBinding` actor, states `status_word = INCAPACITATED_STATUS` (4)
  and passes the six control words through
  `incapacitation::effective_controls` (all zero). Otherwise the word is `0`
  and the record's own controls stand.
* `drive_original_flight` applies it after the ground-blow probe overlay, so a
  stunned aircraft runs neither its controls nor its ground blow until the
  measured deadline tick, on which the ledger's expiry pass clears the mode and
  both come back. A playtest with no ordnance session or no
  `DamageActorBinding` (the synthetic and free-flight scenes) overlays nothing.
* The player seat never reads an incapacitated mode: the measured exposure
  hands `0x71c298` a screen/audio effect and no control loss, so
  `overlay_incapacitation` leaves a player's word at `0` and its controls
  intact whatever the ledger holds. The screen and the sound themselves still
  have no presentation consumer.

## Tests

`accept_groundblow_grace_*` in `crates/cs_app/tests/campaign/accept_groundblow_grace.rs`
(two synthetic aircraft in contact through the production headless app, lockstep
against an unprobed run); `accept_ground_blow_gate_*` in `crates/cs_sim/src/flight/original.rs`, calling
`OriginalFlightModel::step`; the existing `accept_ground_blow_` suite passes.
