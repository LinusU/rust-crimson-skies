# ROCKET-SPLASH-01 (#1927): a `wep_27` burst's splash lands on the player

Date: 2026-10-24. Closes the "rocket splash falloff, the `entity+0x678` factor
and the HEALTH unit" row of `2026-10-22-flak-b3-turret-fire.md`. Capabilities:
`retail` and the owner's decrypted image (static reading only). Claim level
`checked` (static); nothing here is `verified_original`.

## Measured (static, `$CS_ENGINE_IMAGE`)

The burst calls the area query `0x5aca30` and the apply loop `0x5acac0` when
`IMPACT_PROXIMITY > 0` (`0x5b0718`). Cited rows: `splash.*` in
`data/owner-static/engine-facts.toml`.

* **Falloff.** `0x5acb37`: `1 - entry / weapon+0x40`, the entry the squared
  distance from the burst to the recipient's bounding surface (T1318 Q4) and
  `weapon+0x40` `IMPACT_PROXIMITY` squared: `1 - d^2/r^2`. Already carried by
  `cs_sim::AreaEffect::blast_fraction`.
* **The `entity+0x678` factor.** The rocket's blast-and-damage scale (T1318
  Q2): copied from `0xa1e1b0` at spawn, which is 1.0 and is reset to 1.0, and
  scaled afterwards only through a `RANGE_MINIMUM` window (`0x5b06c6`). A
  turret's `wep_27` declares no `RANGE_MINIMUM`, so the scale is **1.0**
  (`TURRET_ROCKET_BLAST_SCALE`). Both damage channels are multiplied by it
  (`0x5acb56`, `0x5acb60`); it also scales the query radius (1.0 here).
* **The HEALTH unit.** None: `0x5abcf0` subtracts the scaled `ARMOR_DAMAGE`
  (`weapon+0x4c`, parser `0x5adb69`) from the victim's pool at `+0x670` and the
  scaled `HEALTH_DAMAGE` (`weapon+0x50`, parser `0x5adb78`) from `+0x674`,
  clamped at 0, with no conversion (`0x5abd51`, `0x5abdab`). The amounts keep
  the weapon record's unit, exactly as the gun path's two channels do.
* Hits go straight to `0x5abcf0` while fewer than 31 are queued
  (`0x5acb70`); the overflow list is not reached by one player victim.

## What the runtime does

`mission_session::turret_fire`:

* `hull_surface_distance_m` takes the burst into the player's frame and returns
  the distance to the hull box (zero inside).
* `rocket_splash_hits` builds one `HitEvent` per channel (armor, then
  internal; `WEAPON_DAMAGE_CHANNELS`) with `declared x scale x (1 - d^2/r^2)`,
  producer `TURRET_SPLASH_PRODUCER` (207), no attacker, node
  `MISSION_TARGET_NODE`, and nothing when `IMPACT_PROXIMITY` is 0 or the burst
  is at or beyond the radius.
* The turret system resolves the tick's splash hits into the session's one
  `SessionDamageResolver`, and records each as a `RocketSplash` in
  `TurretFireReport::rocket_splashes`.

Acceptance: `accept_rocket_splash_*` (cs_app lib tests, and
`tests/accept_rocket_splash_player.rs`). The retail leg test-wakes `aagun99`
in M01, parks the player in its field and reads the hull pool fall through the
damage authority after the first burst.

## Unknown, affected content, resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| The victim's bounding extent (`0x4d8bd0`) | the surface distance `d`: it is measured to the designed hull box, not the original's bound | the player's collision shape / bound trace |
| Per-airframe armor and health pools | how much one burst costs the player: the hull pool is designed (`player_victim`) | the airframe damage table |
| Splash on actors other than the player | zeppelin and aircraft victims of turret rockets | the zeppelin turret work (#1820) |
| The impact-path exclusion key (`+0xbc`) | not reached: a flak round never impacts | none needed for flak |
