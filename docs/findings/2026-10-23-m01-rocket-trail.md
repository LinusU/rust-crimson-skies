# M01 rocket launch puffs and smoke trail (#1827): the trail is measured, the launch puffs are not

Date: 2026-10-23. Capabilities: `retail`, `gpu`, `audio`. Nothing here is `verified_original`: no original executable ran
under an agent; the owner's Drydock capture (task description) is the only observation of the original. Implementer:
`sonnet-2` (Sonnet 5.5); no independent review yet. Static reads used `$CS_ENGINE_IMAGE` through `llvm-objdump`
(output in `private/`, not committed).

## What the task assumed, and what the engine strings are

The task named the engine strings `RocketFire_large`, `RocketFire_small`, `RocketFire_small_rear`, `RocketHit` and
`missile_impact` as candidate launch effects. **The first four are force-feedback effect names, not visual effects.**
Static evidence:

* They sit in the string pool at `0x6287b0` (`RocketFire_large`), `0x6287cc` (`RocketHit`), `0x628830`
  (`RocketFire_small`) and `0x628844` (`RocketFire_small_rear`), next to `CrimsonFF.ifr`, `FireGun_large`,
  `FireGun_small`, `FireGun_medium`, `GunHit`, `Collision_large`, `Collision_small`, `NitroStart`, `ExcessiveSpeed` and
  `TurretFire`. The installation holds `crimsonff.ifr`.
* One init routine (`0x480840`..`0x480c08`) looks each name up through an interface vtable call and stores the handle
  in an object field: `RocketFire_large` -> `+0x158`, `RocketHit` -> `+0x164`, `RocketFire_small` -> `+0x150`
  (falling back to `+0x158` when absent), `RocketFire_small_rear` -> `+0x154` (same fallback). The routine reads no
  animation, texture or particle table.

So they bind rumble, not puffs. They are not used for drawing. (`missile_impact` at `0x6215c0` is a separate string,
used at `0x42bca0`; it is an impact name and was not traced here, see the unknowns.)

## What the data states for the BOOM rocket

`weapons.zrd` `wep_06` (`NAME BOOM`, the record `SELECTED_ROCKET_RECORD` points at):

* `FIRE` block: `EFFECT`, `SOUND snd_missile_bg` and **no `ANIMATION` key**. The records that do draw something at
  launch state it there (`wep_13` `generate_smokescreen`, `wep_15` `call_small_flash`, `wep_27` `small_fireball`).
  So the BOOM record binds **no launch animation**.
* `FLYOUT`: `MODEL he_rocket`, `MODEL_ANIMATION he_rocket`.

`missile_puffers.zrd` (`ZBD/zrdr.zbd`) defines the animation `he_rocket`: `ACTIVATION ON_CALL`, a sequence that sets
`OBJECT_ACTIVE_STATE he_rocket ACTIVE` and calls the sequence `he_rocketpuffer1`, which holds one `PUFFER_STATE
he_rocket_trail`:

| field | value | applied |
| --- | --- | --- |
| `AT_NODE` | `he_rocket` (the flyout model's node) | yes: the puff is born at the rocket |
| `DISTANCE_INTERVAL` | 1.5 | yes: one puff per 1.5 m of travel, read as metres |
| `SIZE_RANGE` | 0.3 .. 0.9 | yes |
| `LIFETIME_RANGE` | 3.0 .. 4.5 | yes, seconds |
| `MIN_`/`MAX_RANDOM_VELOCITY` | -0.8 .. 0.8 per axis | yes |
| `FRICTION` | 1.0 | yes (designed `exp(-friction t)` decay, as the other emitters) |
| `TEXTURES` | `splashbase` (the `c3` world archive) | yes |
| `COLORS` | `[0, 255,180,0, 1]`, `[0.1, 100,100,100, 1]` | yes: orange to grey over the first tenth of the life (key time read as fraction of life, as `damage_trail`) |
| `LOCAL_`/`WORLD_VELOCITY` | 0 | no effect |
| `GROWTH_FACTOR` | 0.25 | **not applied** (unit/curve unmeasured) |
| `DEVIATION_DISTANCE` | 0.05 | **not applied** |
| `FAR_FADE` (`FADE_RANGE`) | 400 .. 600 | **not applied** (unit unmeasured) |
| second state, `ACTIVE FALSE`, `ANIMATION_OFFSET 10.0` | | **not applied**: read as a possible switch-off offset and kept in `RocketTrailDefinition::switch_off_offset_s`; meaning unmeasured |

This is the smoke trail ahead of the plane the owner saw. Reader: `read_rocket_trail_definition`
(`crates/cs_app/src/mission_session/weapon_present.rs`), checked by the ignored test
`accept_m01_rocket_trail_retail_he_rocket_states_the_trail_emitter`.

## What changed

* `weapon_present.rs`: the definition reader, `measure_rocket_trail` (definition + the `splashbase` texture from the
  `c3` archive), and the runtime consumer: every rocket mirror launched by the player (`shooter ==
  MissionWeapons::player()`) drops a camera-facing puff per `DISTANCE_INTERVAL` of its authoritative travel; puffs
  fade through the stored colour keys and are despawned at the end of their life. It reuses `TrailEmitter`,
  `plan_emissions`, `plan_puff` and `colour_at` of the damage trail (the same `PUFFER_STATE` fields).
* Wiring: `compose.rs` measures the trail for `zbd/c3/m01` only and installs `RocketTrailSourceResource`;
  `mod.rs` re-exports; `damage_trail.rs::emitter_of` became `pub(super)`.
* Smoke: check `weapon_rocket_trail_drawn` (rocket phase) counts the trail puffs.

## Unknown, recorded (rule 4)

* **Launch puffs around the nose.** No launch animation is bound in `wep_06` (`FIRE` has no `ANIMATION`), and no
  `PUFFER_STATE` found so far is called at the moment of launch for the BOOM rocket. The burst of grey puffs the owner
  saw around the nose may be the first trail puffs (a trail puff is born at the rocket, which leaves the nose, lives 3 to
  4.5 s and is grey after 0.1 of its life), a hardpoint-side effect in the player airframe record, or an engine path not
  traced. **Not drawn separately; open.** Resolving it needs either a trace of what the engine does when the
  secondary-fire row runs (`missile_impact` `0x42bca0` is the first lead) or a Drydock frame sequence of the first
  half second after the tap.
* `RocketHit` and `missile_impact` as impact visuals: `wep_06` `IMPACT` states `he_ground_effect` and `large_fireball`
  for the surfaces; this task did not touch impacts.
* The `GROWTH_FACTOR`, `FAR_FADE`, `DEVIATION_DISTANCE` meanings above, the blend mode and whether the 10 s offset is a
  switch-off.
* The designed rocket body still draws instead of the `he_rocket` model.

## Tests

`accept_m01_rocket_trail_*` in `crates/cs_app/tests/campaign/m01_rocket_trail.rs` (two synthetic, two ignored retail
tests).
