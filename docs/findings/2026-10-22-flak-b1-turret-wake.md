# FLAK-B1-TURRET-WAKE (#1880): turret entries, the live byte and the wake consumer

Date: 2026-10-22. Part 1 of #1820. Capabilities used: `retail` (production
readers over `ZBD/C3/M01`) and the host driven headless; nothing of the image
was read again, every law cited is in
`2026-10-21-flak-a-turret-update-law.md`. Nothing here is `verified_original`.

## What exists now

* `cs_app::mission_turrets::TurretSpec` / `read_turret_specs`: the `ai.zrd`
  `TURRET` fields the update reads (`ACTIVATED`, `TEAM`, `DETECTION_RANGE`,
  `SHOOT_UP_ONLY`, `FIRE_RATE`, `FIRE_LIMITS`, `INACCURACY`,
  `ATTACK_INTERVAL`, `BORED_INTERVAL`, `PITCH`, `YAW`, `EFFECT`, `AMMO`,
  `WEAPON`). A field the record does not state is `None`; the originals'
  defaults (live, team 2, 200 m) are accessors. Keys not interpreted are kept
  in `other_keys`.
* `TurretEntries`: one entry per world node a one-element `NODES` pattern
  matches (earliest record wins) with its composed world position, and one per
  zeppelin part chain a two-element `NODES` entry declares. The live byte
  starts as `ACTIVATED`.
* `TurretWakeLedger` (mission host step 6c1): `WAKEUP_TURRETS` sets the live
  byte of every world entry whose name matches (`*` is one digit);
  `WAKEUP_ZEP_TURRETS` sets it for every entry under the named zeppelin.
  Exactly once per execution key; a name or site that wakes nothing is a named
  `TurretWakeRefusal`.

## Measured on M01 (`accept_flak_b1_retail_…`)

`t_truck01..03` (-5459.3, 118.6, -5339.2 …) start asleep and are woken
together at host tick 6 by OBJECTIVE6's `WAKEUP_TURRETS ['t_truck**']`;
`aagun01/02/30..32` have entries, stay asleep, and no M01 program wakes them.

## Unknown, affected content, resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| `WAKEUP_ZEP_TURRETS piratezep` (OBJECTIVE1) finds no entry: the zeppelin's gun nodes (`doublecannon*`, record 37) are bound only when the mission program names their chain, and M01 names none for `piratezep`. It is refused by name, not guessed | the pirate zeppelin's guns never go live | the zeppelin model's node hierarchy as the entry source (FLAK-B-TURRET-RUNTIME #1820 / a follow-up) |
| Whether `workersvoyagezep` / `blackswanzep` wakes run (OBJECTIVE5/13 are not reached by the test's route) | their guns | same |
| Firing, target step, timers | every woken turret | FLAK-B2 #1881 |
