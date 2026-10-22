# FLAK-B2-TURRET-UPDATE (#1881): the turret update law as an integer-tick system

Date: 2026-10-22. Part 2 of #1820. Capabilities used: `retail` (the real
`t_truck` record and node position of M01) and a static read of
`$CS_ENGINE_IMAGE` for the constructor defaults. Nothing here is
`verified_original`.

## What exists

`cs_app::mission_turret_update`: `TurretConfig::from_spec`, `TurretSystem`
(`update` per committed tick), `TurretFireIntent` ("turret X fires from muzzle M
in direction D"). The intents are not yet projectiles (FLAK-B3, #1882). The
draws come from one `SplitMix64` stream (`TURRET_DRAW_DOMAIN`) over the mission
seed, so a seed reproduces the shots.

## New measured rows (`data/owner-static/engine-facts.toml`)

The constructor's defaults for fields a record omits, which `t_truck` needs
(it states no `FIRE_RATE`): `default.fire_rate` (1.0, 1.0),
`default.attack_interval` (10.0, 10.0), `default.bored_interval` (0, 0).
`INACCURACY` is degrees converted to radians (`field.inaccuracy`); the finding
text's "radians" describes the stored value.

## Unknowns that stay unknown

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| A world line-of-sight ray: none is wired; the update takes a callback and an `Unavailable` answer never engages (`sight_unavailable` counts it) | every turret's player ray, hence whether any turret hits the player | the wiring task that owns the physics ray (FLAK-B3 / #1820 integration) |
| Weapon flag `0x40` (lead aiming, intercept) | the fire direction of wep_29/wep_27 | descriptor trace (finding 2026-10-21, table) |
| Distribution inside the `INACCURACY` cone (two draws are measured, their mapping is not) | spread of flak fire | trace of `0x460940` |
| `EFFECT` wind-up duration source (`entry+0x158`) | turrets with `EFFECT` (none among `t_truck`) | parser trace |
| `PARTS` firepoint offsets and the mount's yaw/frame convention | muzzle position, aim sign convention | geometry binding in FLAK-B3 |
| Order of the constructor's draws | the exact first bored/fire times | not recoverable by static reading alone |
| `AMMO` consumer | turret ammunition | trace of `0x5aef40` / `0x441830` |
