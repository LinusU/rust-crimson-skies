# T452-REG-BEEPER-SEEKER (#2178): the BEEPER_SEEKER retargeting in the runtime guidance vocabulary

Date: 2026-10-28. Follow-up of #2171 (`T452-RUNTIME-REGISTER`). Capabilities:
ordinary build/test only. No original executable ran; nothing here is
`verified_original`.

## The gap

`wep_11` declares `BEEPER_SEEKER`. The engine installs callback `0x441780` into
the projectile (`0x441877`) and calls it once per step before steering
(`0x5af745`, T1601); it writes the found beeper's two values into the target
slot, or zero (T2116, T452-R.02 §1). `GuidanceRule::Targeted` named one target
with a terminal loss, so the measured behaviour was kept as
`t452_r02.limit.beeper_seeker_guidance`.

## What changed

- `cs_sim::weapons::GuidanceRule::MarkerSeeker` (declared as
  `DeclaredGuidanceRule::MarkerSeeker`, catalogued as
  `CataloguedGuidance::MarkerSeeker`). It is targeted but has no lost-target
  behaviour: loss is not terminal (T2116).
- `StatusEffectLedger::newest_live_marker`: the newest live `Marker` effect
  (latest start tick, ties by the later instance serial).
- `OrdnanceRuntime::retarget_marker_seekers`, run by `step_ordnance_session`
  every step and by `guidance_tick`: each seeker's target becomes the newest
  live marker's actor, or none. A seeker with none flies unguided
  (`GuidanceUpdate::Unguided`), not as a lost target.
- A seeker's launch target is dropped at launch (the launch tail zeroes the
  slot, `0x5af610`); the first step's callback writes the real one.
- F28 AC02: an observation that the tracked actor was destroyed clears the
  seeker's target for the step; a session change loses it for good and a spent
  seeker adopts no marker.
- The claim leaves `wep_11`'s measured behaviour and the registration report's
  `claims_kept`.

## Unknown, recorded

"Best live beeper" is the engine search `0x4b8b50` on the projectile's position
and heading. Which beeper it returns when several are live is **not** measured
here; the runtime picks the newest marker, the rule the task names. The
search's ordering is not established, so this is a design choice, not a
measurement.

`docs/findings/evidence/T452-RUNTIME-REGISTER.json` still lists the claim among
its `unknowns`; it was derived by #2171's harness and is not hand-edited. It
needs regenerating with that harness.

Tests: `accept_t452_beeper_*` in `crates/cs_app/tests/accept_t452_beeper.rs`.
