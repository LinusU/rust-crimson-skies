# AI airframe parameter randomisation (`0x4771c6`), task #805

Provenance: `OWNER-STATIC-2026-10-08` (owner-requested static analysis of
`crimson.decrypted.exe`, sha256 `43540fc9…37d75`). Static code evidence, never
`verified_original`. This note resolves the "AI per-airframe parameter
randomisation" limitation recorded in
`2026-10-08-flight-original-fixed-wing-law.md`.

## What was read from `0x4771c6`

* **Band:** the analysis states every per-airframe flight parameter is varied
  by ±5 %. Implemented as a multiplicative factor in `[0.95, 1.05)`
  (`AI_RANDOMISATION_BAND`). Whether the original scales multiplicatively or
  adds a percentage of the base is the same thing for a symmetric ±5 %; the
  exact distribution inside the band was not recovered.
* **"Player never":** the roll is applied to AI aircraft only. The player's
  record is used with its exact imported values and draws nothing
  (`AirframeController::Player`).

## What is rolled

All fifteen `AIRFRAME_FIELDS`: `roll_torque`, `pitch_torque`, `rudder_torque`,
`level_off_rate`, `return_rate`, `ang_momentum_damp`, the three
`rec_moments_inertia_*`, `fd_speed`, `engine_factor`, `drag_factor`,
`veh_weight`, `ref_area`, `gravity`. The analysis says "each parameter"; no
exclusion was recorded, so none is made. The global (`player.zrd`) constants
are not per-airframe and are not rolled.

A positive value scaled by a factor in the band stays positive, so a rolled
record still satisfies `OriginalAirframe::from_values`. A rolled value that is
not finite is refused by name (`AirframeRollError::NonFinite`), never clamped.

## Remake decisions (not recovered behaviour)

* The RNG (`SplitMix64`, domain `AI_AIRFRAME_DOMAIN` = ASCII `"AI_AIRFR"`), the
  uniform distribution, one draw per field in `AIRFRAME_FIELDS` order and the
  seed itself are remake choices. The original's algorithm and seed were not
  recovered, so a rolled AI aircraft matches the original's *distribution
  claim*, not any particular original aircraft.
* Evidence: `RolledAirframe` carries the seed, and per field the imported
  value, factor and rolled value; `evidence_overrides()` renders them as the
  override lines of the evidence record (`docs/contracts/CLI-EVIDENCE.md`).

## Still open

No AI spawn path exists yet that calls `OriginalAirframeParameters::randomised`;
the function is the production entry point for that consumer and is exercised
by `accept_flight_ai_parameter_randomisation_*`. The consumer must choose and
record the seed. Claims that AI aircraft handle like the original's stay gated
on that wiring and on the AI control law `0x48c220`.
