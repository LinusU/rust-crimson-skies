# F14-D.10: the engine and gun collections, and where the original states armor, ammunition and hardpoints

Date: 2026-10-11. Task: #563 "Locate where the original states airframe engines,
armor, guns, ammunition and hardpoints" (follow-up of #489 / F14-D.6). Required
capability: **`retail`**, used read-only. `gpu` and `audio` were available and
not used; no original run happened and nothing here claims `verified_original`.

## Measurement

All of it is in `ZBD/zrdr.zbd`, read through the production chain
(`cs_formats::zbd` + `cs_content::stunts::decode_zrd`, with byte spans from
`cs_content::original_airframe::SpannedDocument`).

| family | where it is stated | shape | row? |
| --- | --- | --- | --- |
| engine | `engines.zrd` | one `(id "name" factor)` list per engine; ids 0-4 generic, 10-42 `<Airframe> Lvl-N` | **yes**, `engine/engine-<id>` |
| gun | `weapons.zrd`, section `BALLISTICS` | flat key/record pairs `wep_NN (DESC NAME AMMO_LIMIT ARMOR_DAMAGE FIRE_RATE RANGE VELOCITY CALIBER CANNON …)` | **yes**, `gun/wep_NN` for records that set `CANNON` |
| ammunition | `AMMO_LIMIT` inside a weapon record, and the count in a vehicle's `weapons` loadout `(wep_NN count …)` in `vehicle.zrd` | a number per weapon and per vehicle fit, no entry list of its own | no |
| armor | `vehicle.zrd`, key `armor` (a single float per vehicle record, beside `health`) | per-vehicle value, no list of armor items; the four zones are only named in `ARMOR.SCRIPT` | no |
| hardpoint equipment | none located. `vehicle.zrd` has `turrets` and `gun_pitch`/`gun_yaw` per vehicle; `HARDPOINTS.SCRIPT` fills its rows by native callback | no list | no |

Records of `weapons.zrd` without `CANNON` are rockets, a torpedo and two records
carrying no weapon-type key. They are weapons, but no `ContentKind` names them;
they are counted in the `gun` collection's `gaps.weapon_record_without_cannon`
and have no row. Whether a `wep_NN` record is a "gun" in the original's sense
beyond the file's own `CANNON` key was not established.

## What the rows do not carry

No stored number is copied: thrust factors, damage, rate, range and ammunition
limits have no measured unit (`IDENTITY-CONTENT.md`, numeric contract), and the
hangar field that selects an engine (`loadout+0x10c`, owner note
`OWNER-STATIC-2026-10-08`) is not traced. No engine is tied to an airframe row:
the airframe collection is keyed by loading-script root names (`player_bhawk`)
and `engines.zrd` by `<Airframe> Lvl-N` display text, and matching them by name
would be a guess. Every row has an `Unknown` reason, `NotNormalized` and
`MissingRuntimeConsumer`.

## Coverage

The coverage denominator did not move (both kinds are non-launchable). The new
rows are counted in `coverage.unreachable_by_kind.engine` and `.gun`.

## Resolving work

* armor, ammunition and hardpoint rows: the native-callback decode of the
  `callback($$E$$, 5029/5061/5067/5010/5015, …)` and `callback($$A$$, 2507, …)`
  family (F38/F13), which is where the values the UI scripts show come from;
* engine to airframe/vehicle binding: a measured mapping between the loading
  script roots and `vehicle.zrd` records;
* units: the F16 calibration stage.

## Review record

Implemented by sonnet-1 (Sonnet 5.5). No review yet; the evidence report
`docs/findings/evidence/F14-D.10.json` says so.
