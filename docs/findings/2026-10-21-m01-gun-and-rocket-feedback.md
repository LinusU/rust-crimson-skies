# PARITY3-GUN-AND-ROCKET-FEEDBACK (#1742): the M01 gun record, one gun, no tracers; rocket effects still open

Date: 2026-10-21. Capabilities: `retail`, `build`, `test`, `gpu`. Nothing here
is `verified_original`: no original executable ran under an agent. The owner's
Drydock captures (task description) are the only observations of the original.

## What the owner saw (original, M01)

* Holding `Space` for about 1.5 s took the GUNS dial from 2000 to 1980
  (about 20 rounds) and drew no tracer streaks.
* Tapping `X` fired one rocket: ROCKETS 3 -> 2, a burst of grey puffs around
  the nose and a smoke trail ahead of the plane.

## What the data states (`weapons.zrd`, `ZBD/zrdr.zbd`, read by the production reader)

`wep_50` is the gun the fresh-campaign pilot fires (slot 0 of the Flight
Check record; `docs/findings/2026-10-19-m01-window-fire-gun-loadout.md`).
Its record states `AMMO_LIMIT 2000`, `FIRE_RATE 7.5`, `VELOCITY 850`,
`RANGE 1000`, `ARMOR_DAMAGE 6.25`, `HEALTH_DAMAGE 6.25`, `CANNON_SPREAD 6`,
`CALIBER 50`, `CLUSTER_SIZE 2000`, `FLYOUT MODEL slug.flt` (`wep_40`:
2400 / 8 / 900; `wep_30`: 2800 / 8 / 1000). The dial value 2000 equals
`AMMO_LIMIT`.

## What changed

* `selected_gun_loadout` declares **one** gun (the selected `wep_50`), not the
  designed three. Its rate, muzzle velocity, lifetime (`RANGE / VELOCITY`) and
  both damage channels come from the record, labelled `ObservedTool` under
  `m01-guns-and-rockets.selected-gun-record`; mount, caliber text and spread
  stay designed. At 120 ticks a second `FIRE_RATE 7.5` is 16 ticks between
  shots (before: three guns, 4 ticks, 250 rounds each of a designed 12-damage
  round: 90 rounds in one second).
* A live round draws nothing (the tracer streak is removed); the smoke check
  `weapon_rounds_drawn` became `weapon_rounds_not_drawn`.

## Unknown, recorded (rule 4)

* **Unit of `FIRE_RATE`.** Read as rounds per second; `weapons.zrd` states no
  unit. One gun at 7.5 per second would put about 12 rounds through a 1.5 s
  hold, while the capture shows about 20. Either the hold was longer than
  estimated, a shot costs or fires more than one round, or the unit differs.
  Resolving it needs a Drydock capture with a timed hold (for example 3 s) and
  the dial read before and after.
* **Whether the other two guns (`wep_40`, `wep_30`) ever fire together** with
  `wep_50`: the dial counting 20 rounds is compatible with the selected gun
  only. No gun-switch model exists.
* **`FLYOUT MODEL slug.flt`** is declared but the capture shows no streak;
  whether the model is drawn at all (size, culling) is unmeasured.
* **Rocket launch puffs and the smoke trail.** The records name
  `RocketFire_large`, `RocketFire_small`, `RocketFire_small_rear` and
  `RocketHit` (engine strings, see the task's owner note); their binding to
  the BOOM rocket (`wep_06`), puff count, lifetime and trail length are not
  measured and not drawn. This is **not done** in this change and is filed as
  a follow-up task.

## Tests

`accept_parity3_gun_and_rocket_feedback_*` in
`crates/cs_app/src/mission_session/weapon_counts.rs`;
`accept_m01_window_fire_a_live_round_draws_no_tracer` in
`crates/cs_app/tests/campaign/m01_window_fire.rs`.
