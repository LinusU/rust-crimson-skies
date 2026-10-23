# Crash D.03: the wing fire plume at the pdp node (#1946)

Status: implemented; the threshold direction, the pools and several emitter
units stay **designed or unknown**. Nothing here is `verified_original`.

## What the code does

`crates/cs_app/src/mission_session/wing_fire.rs`:

* `read_wing_fire_definition` reads, from `ZBD/zrdr.zbd`, the `pdevastator`
  record of `vehicle.zrd` (the `leftwing` / `rightwing` `injure_anims`
  ladders, keeping the `pdpanelN` rungs), each rung's animation in
  `player.zrd`, and every effect those animations call on a `pdpN` node that
  is one of `small_fireball_follow`, `short_firetrail`, `loop_short_firetrail`,
  `large_firetrail` (`pufftrails.zrd`). The archive stores `player.zrd` twice
  (members 22 and 100); only member 100 is an animation table, so the reader
  looks at every member of that name and keeps what decodes.
* Read from retail: port `pdpanel5` / `4` / `3` at 0.50 / 0.30 / 0.15,
  starboard `pdpanel6` / `1` / `2` at 0.40 / 0.30 / 0.15. `pdpanel3` calls
  `small_fireball_follow` on `pdp3` at +0 s (two `lgpuffer` emitters, the first
  switched off by its own `INACTIVE` event at +1 s) and `loop_short_firetrail`
  on `pdp3` at +2 s; `pdpanel2` calls `small_fireball_follow` and
  `short_firetrail` (+2.01 s) on `pdp2`.
* Every pass the plugin reads each wing region node's remaining integrity from
  the session `DamageResolver` (`remaining_integrity`), over
  `DESIGNED_REGION_INTEGRITY`. When it is at or below a rung's threshold the
  rung starts (and records its tick); its effects emit from the drawn
  airframe's `pdpN` part (`WingFireNode`, named from the airframe visual's own
  part list) until the run ends. A rung whose region climbs back above its
  threshold (repair) stops.
* The emitters are the measured `PUFFER_STATE` records drawn with the damage
  trail's runtime (`plan_puff`, `trail_puff_position`, `colour_at`,
  `frame_at`), as `WingFirePuff` entities. There is no second particle model.
* After a fatal crash the airframe's parts stay where the wreck lies and the
  fire keeps emitting from them.

Wiring edits outside `wing_fire.rs`: `mission_session/mod.rs` (module and
re-exports), `compose.rs` (the stage's `wing_fire` field measured for M01's
retail stage, the error variant and the plugin install), and the one
`MissionStage` literal in `tests/campaign/vs_m01_rt_window.rs`.

## Designed (claim `crash-d-03.designed-wing-fire`)

* **Threshold direction.** The record stores the number only. A rung starts
  when remaining integrity / pool `<=` the number: the list is descending and
  named green, yellow, red, so the numbers read as the fraction of health
  remaining. Inferred, not measured.
* **The pools.** `DESIGNED_REGION_INTEGRITY` (300) is a project design; the
  original's zone armor and pools are unknown (see
  `2026-10-23-crash-d-01-terrain-contact-wing-zone.md`). *When* a given
  contact crosses a rung therefore depends on two unknowns: the zone the
  original damages and the share of the contact damage it takes. With the
  designed pools a floor-damage contact (100) leaves 0.67 and lights nothing;
  a head-on contact (600) destroys the wing and lights every rung of that
  side.
* **A static anchor.** A `DISTANCE_INTERVAL` emitter emits per metre the
  anchor travels, which is nothing for a wreck lying still. The anchor is
  credited 4 m/s of travel at least (`WING_FIRE_MIN_TRAVEL_M_S`), so the fire
  on a static wreck keeps burning. The original's behaviour for a stationary
  host is not measured.
* Units and curves of the shared runtime are the trail's: `DISTANCE_INTERVAL`
  in metres, frame times in seconds of puff age, colour keys as a fraction of
  life, velocity decay `v * exp(-friction * t)`.

## Unknown (claim `crash-d-03.wing-fire-emitter-units-unmeasured`)

Not applied, with the content they affect:

* `AT_NODE` offset words (`small_fireball_follow` emits at `INPUT_NODE 0 0 -1`
  and `0 -0.25 -1`): the frame of the offset is unmeasured; the puffs are born
  at the node. Affects the fireball's position relative to the wingtip.
* `GROWTH_FACTOR`, `FADE_RANGE`: puffs do not grow or fade by distance.
* `short_firetrail`'s four-stage hand-over (`CALL_SEQUENCE ... EVENT_OFFSET
  8/6/4/2` switch-offs): the sequence semantics are unmeasured, every stage
  runs on. Its 50 % `RANDOM_WEIGHT` `black_smoke` emits every time.
* The first-person `pcdp4` / `pcdp6` branch (`large_firetrail`): the cockpit
  view is not drawn by this module.
* `gimmeflakes` debris, `yellow_sparks_follow` sparks (contact sparks are
  another D task) and the `snd_exp_hit5` sound.
* `pdpanel1` loops `pdp1` active forever through a `LOOP`; the node's
  activation is not modelled, only its `short_firetrail`.
* An emitter whose record states no random velocity is read as zero velocity.
* The `pdpN` patch meshes drawn on the airframe are not switched by this
  task; `OBJECT_ACTIVE_STATE` is unmodelled.

## Tests

`accept_crash_d_03_` in `crates/cs_app/tests/campaign/accept_crash_d_03_wing_fire.rs`:
four synthetic tests drive the production plugin with a damage-resolver hit on
`leftwingdamage` (the definition is hand-built there; not retail evidence) and
two retail tests (`#[ignore = "requires CS_GAME_DIR"]`) read the real ladders
and run the M01 composition, finding the emitters on the drawn `pdp3` part and
none on the starboard side.

## Owner-command smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY4-CRASH-D.03` (release build, `retail,gpu,audio`):
exit 0, 69 checks, failures: none. The `damage_trail` frame
(`frame_03166_damage_trail.png`, left wing at 0.40 remaining, rung `pdpanel5`)
shows only a small orange puff at the port wingtip: the frame is taken right
after the hit, so few puffs have been born. The plume is faint there; its
look against the owner frame is not judged.
