# FLAK-B3-TURRET-FIRE (#1882): woken turrets fire real rounds at the player

Date: 2026-10-22. Part 3 of #1820. Capabilities used: `retail` (the real
`weapons.zrd` records and the M01 stage, composed headlessly) and `gpu`/`audio`
for the owner-command smoke. Nothing here is `verified_original`.

## What exists

`cs_app::mission_session::turret_fire`:

* `read_turret_weapons` reads every `WEAPON` the `ai.zrd` `TURRET` records name
  from `weapons.zrd` and classifies it by its own keys: `CANNON` (`wep_29`) is a
  gun (`FIRE_RATE`, `VELOCITY`, `RANGE`, `ARMOR_DAMAGE`, `HEALTH_DAMAGE`,
  `AMMO_LIMIT`, `CANNON_SPREAD`), `ROCKET` (`wep_27`) is a `FlakRound`. A record
  that is neither (`wep_28`, `8igun01`) is refused by name.
* `build_mission_turrets` / `step_mission_turrets` (fixed tick, after the physics
  step, before the weapon publication). Each cannon turret becomes a weapon
  actor of its own in the session's `WeaponSession` with a one-gun loadout from
  its record (the same builder the player's `wep_50` uses), a hostile-faction
  record in the `TargetingSession` (unrevealed, so the player's target list does
  not change) and a body at the mount node. The turret system of FLAK-B2 runs
  over the live bytes the FLAK-B1 wake ledger holds, with the player as the only
  candidate. Each fire intent aims the body at the intent's direction and
  queues a fire order that rides the mission's own weapon publication
  (`append_turret_fire_orders`, producer 206). The round is the F27 step's, and
  a hit on the player's swept hull box resolves through the session's one
  `SessionDamageResolver`.
* The line of sight is a physics ray from the muzzle to the player against
  static bodies, excluding the player and the static colliders enclosing the
  muzzle.
* Rocket turrets fly a `FlakRound` and record its burst.

## Measured on M01 (`accept_flak_b3_retail_…`)

After OBJECTIVE6's `WAKEUP_TURRETS ['t_truck**']`, with the player parked 100 m
east and 40 m above `t_truck01` for 15 s, `t_truck01..03` fire `wep_29` rounds
(`cannon_orders > 0`, every shot named by a `t_truck` entry), the player ray is
asked and the player's hull pool falls through the damage authority. `aagun**`
stay asleep and nothing fires from them. The hull pool and its box are the
mission's designed ones (`player_victim`), so the size of the drop is not a
claim about the original.

## Owner-command smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 60 --capture-dir
private/evidence/FLAK-B3-TURRET-FIRE`: 58 checks, 3810 frames, no failures
(exit 0). The scripted smoke does not drive the objective chain to OBJECTIVE6,
so the trucks stay asleep in it; it proves the wiring does not disturb the
shipped M01 run, not that the trucks were seen firing. A smoke mode that reaches
OBJECTIVE6 is a follow-up.

## Unknown, affected content, resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| `PARTS` firepoint offsets | the muzzle: it is the mount node's origin | geometry binding of the turret parts |
| The mount's yaw/frame convention and the `PITCH`/`YAW` travel | the aim: the body looks along the intent direction, unclamped | descriptor/mount trace |
| `CANNON_SPREAD` unit and shape (read, `-cos` of an angle in the engine) | the cone of every cannon round: the gun keeps the mission's designed spread | trace of the spread consumer |
| Weapon flag `0x40` (lead aiming) | the fire direction | FLAK-C1 / descriptor trace |
| `HEALTH` unit; rocket splash falloff and the `entity+0x678` factor | rocket bursts apply no damage to the player (none is woken in M01) | burst-damage task |
| `AMMO` consumer | the gun starts with the record's `AMMO_LIMIT`, `AMMO` is not read | trace of `0x5aef40` / `0x441830` |
| Zeppelin turrets (`piratezep` guns) | no mount position is bound, so they are not built | #1820 / the zeppelin node source |
| Static world colliders as the sight ray's occluders | whether the terrain blocks the ray is only as good as the colliders the stage spawns | world collider coverage |
