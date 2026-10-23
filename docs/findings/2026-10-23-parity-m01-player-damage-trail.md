# PARITY-M01-PLAYER-DAMAGE-TRAIL: the damaged airframe's sprite trail

Date: 2026-10-23. Task: #1700. Capabilities used: `retail`, `gpu`.

## What is drawn

`crates/cs_app/src/mission_session/damage_trail.rs` reads `dense_firetrail`
(`pufftrails.zrd` in `ZBD/zrdr.zbd`; the animation `player_smoketrail` calls it
`WITH_NODE prop1`) and decodes its nine textures (`smoke101..103`,
`fire_f01..f06`) from the world's c3 texture archive at stage time (M01 only,
`MissionStage::damage_trail`). While a wing region of the damage authority
reports `Damaged` or `Destroyed`, the player's `prop1` node
(the `PropellerBlur` entity) leaves camera-facing puffs: `smokepuffer` every
1 m of travel, `firepuffer` every 0.25 m. Applied as stored: `DISTANCE_INTERVAL`
(read as metres of anchor travel), `SIZE_RANGE`, `LIFETIME_RANGE`, the
constant `MIN_/MAX_RANDOM_VELOCITY` (-0.1 on each axis), `FRICTION`, the
smoke `TEXTURES` cycle and the fire `TEXTURE_SEQUENCE`.

## The open measurements of the task

- **`fire_lt` is not a scene node**: it is a light the animation creates and
  places at `prop1` (`2026-10-21-parity2-dmg-fx-a-effect-definitions.md`). The
  trail's anchor is therefore `prop1`, a measured `planes.zbd` node.
- Port vs starboard: the records bind `wing_flare2` (set INACTIVE) and `prop1`;
  no record places the trail at the port tip. Not resolved.
- Whether the anims move nodes: not decoded; no node is moved.

## Designed (claim `parity-m01-player-damage-trail.designed-trail`)

- Gate: any wing region `Damaged`/`Destroyed`. Which `injure_anims`
  threshold calls `player_smoketrail` (0.10) and its direction are unmeasured.
- `TEXTURE_SEQUENCE` frame times read as seconds of puff age; `COLORS` key
  times as a fraction of the puff's life (rgb /255, linear interpolation);
  velocity decays as `v*exp(-FRICTION*t)`; alpha blend, camera-facing quad.

## Not applied (unmeasured meaning)

`GROWTH_FACTOR`, `FADE_RANGE`, `DEVIATION_DISTANCE`, the `fire_lt` light
flicker, `player_fuelleak`'s `fuel_trail` at `pdp1..3`, the `pdpanel*` fires
and which trail plays at which damage level.

## Owner-command smoke

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-PLAYER-DAMAGE-TRAIL`: 67 checks, no failures. The
new phase `DamageTrail` (a test hit of a quarter of the designed region pool on
`leftwingdamage`, listed as `damage_trail_drawn`) drew 155 smoke and 621 fire
puffs; framebuffer `frame_03166_damage_trail.png`. In that frame the port
wingtip flare quad is visible; **the puffs are not clearly legible** in the
chase view (they are left behind the airframe, towards the camera, at the
stored 0.1-0.8 m sizes), so the frame is evidence that the pass ran with the
wing burning, not that the plume matches `orig70.png`. Comparison against the
original is #1808.
