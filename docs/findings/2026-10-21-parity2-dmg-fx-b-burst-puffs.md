# PARITY2-DMG-FX-B: smoke puffs at ordnance bursts

Date: 2026-10-21. Task: #1806. Builds on
`docs/findings/2026-10-21-parity2-dmg-fx-a-effect-definitions.md`.

## What is drawn

`crates/cs_app/src/mission_session/burst_puffs.rs` reads `large_black_smokeball`
(`flame_ball.zrd`, emitter `trailpuffer2`) from `ZBD/zrdr.zbd` and the three
`thickblksmoke01..03` textures from the c3 texture archive, at stage time (M01
only, `MissionStage::burst_puffs`). Each ordnance detonation or guidance-loss
blast of the mission session (`drive_mission_ordnance`) publishes an
`OrdnanceBurst`; the plugin spawns `NUMBER` (5) camera-facing puffs there.
Applied as stored: count, `SIZE_RANGE` 4-7, `LIFETIME_RANGE` 4-5 s, per-axis
`MIN/MAX_RANDOM_VELOCITY` (+-65), one of the three textures per puff.

## Designed (claim `parity2-dmg-fx-b.designed-puff-motion`)

Velocity decays as `v*exp(-FRICTION*t)`; opacity fades linearly over the life;
the quad faces the camera; alpha blending with the texture as decoded.

## Not applied (meaning unmeasured, not tuned)

`GROWTH_FACTOR`, `FADE_RANGE`, `TIME_INTERVAL` and the repeat emissions up to
the stored switch-off (+2.1 s; one emission is drawn), `COLORS`, blend mode,
the units of size/velocity (read as metres), the flak trails, the `flak_light1`
flash and the screen flash of `flak_effect`.

## Source events

Only the player's rocket detonations exist in M01 today. The flak shooters are
#1741's: when they publish an `OrdnanceBurst` the puffs draw unchanged.

## Not shown in the owner-command smoke

On the machine of this session the smoke ran (retail, `CS_CAPABILITIES` with
`gpu`) but every framebuffer PNG, including the briefing and world frames this
change does not touch, was uniformly black (10 of 37 checks failed for that
reason). No screenshot of the puffs exists; the task is **not** closed on its
"seen in the smoke" criterion. Whether the smoke's rocket tap detonates inside
its 90-frame wait is also unmeasured.
