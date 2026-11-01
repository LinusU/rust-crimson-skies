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

Nothing writes the fields in a mission yet: the contact response that sets
`V+0xac = now + 1.0` on both aircraft of a class 0/4 pair
(`0x48d31e`..`0x48d395`, `crates/cs_app/src/physics/contacts.rs`) and whatever
sets `V+0x358 = 4` (the incapacitation status effect, #2172) do not fill them.
Until they do, an AI within 1 s of an aircraft-into-aircraft contact, and a
stunned aircraft, still run their ground blow. The `+0xb4` damping stays
unmodelled (no site assigns it a non-zero value). No constant was tuned.

## Tests

`accept_ground_blow_gate_*` in `crates/cs_sim/src/flight/original.rs`, calling
`OriginalFlightModel::step`; the existing `accept_ground_blow_` suite passes.
