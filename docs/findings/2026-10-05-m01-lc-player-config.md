# M01-LC-PLAYER-CONFIG: the player and wingmate records, and what stays unknown

Date: 2026-10-05. Task: #634. Capability used: **`retail`** (read-only
`$CS_GAME_DIR`) and ordinary build/test. No original run happened; nothing here
is `verified_original`.

## Where the configuration lives

`missions/bindings/M01.json` records "initial player and wingmate
configurations" as an unknown. Measured in `ZBD/C1C/M01/zrdr.zbd`: its `aiv.zrd`
member (9869 bytes) decodes as a header record followed by 12 aircraft records
`[name, [~90 positional fields]]`. Names include `player` (record 1),
`wingman_3` (4), `wingman_2` (6) and the scripted AI aircraft
(`devastator_3`, `devastator_2`, `bsfury_1`, `rusdevastator_<n>`, ...). Other
members mention the player (`startanims.zrd`'s `LOAD_GAME_START` calls
`player_setup`; `objectives.zrd` has `PLAYER_INIT`), but name no airframe.

## What was built

`cs_app::mission_start` (new; wiring: `crates/cs_app/src/lib.rs`):
`MissionStartConfiguration::read` and `recover_retail_start_configuration`.

* **Bound, `ObservedTool`, with the member's byte span:** the one record named
  `player`, and every record named `wingman_<digits>`. M01: player at index 1,
  wingmates `wingman_3` and `wingman_2`.
* **Bound as a reference only:** field 6 of a wingmate record is text naming
  another record of the table (`devastator_3`, `devastator_2`); it is reported
  only when it resolves. What it means (leader? flight? template?) is not
  claimed.
* **`Resolved::Unknown`, with reasons:** the player's airframe, each wingmate's
  airframe, and the initial pose. A `Known` pose or airframe is unconstructed
  today.

## Why the airframe and pose stay unknown

The record's layout is about ninety opaque fields (also recorded in
`2026-10-02-t465-ai-stunt-earning.md`). The player record has a three-float
vector and a bearing-like float at fixed positions, but no unit, frame or
origin is measured, and the mission program may move or replace the aircraft
before launch; no field is measured to name an airframe. The statements that
may assign one are mission-language code (F13-B/C, F38). Resolving: the
mission-program decoding (F37/F38) or a measurement of the aircraft record
layout. `missions/bindings/M01.json` is protected and unchanged; its
"initial player and wingmate configurations" unknown therefore **still stands**
for the airframe and pose, and VS-M01-RUNTIME (#359) stays blocked on them.

## Tests

`accept_m01_lc_player_config_*` in
`crates/cs_app/tests/campaign/m01_lc_player_config.rs` (2 synthetic, 1 retail
`#[ignore = "requires CS_GAME_DIR"]`), in the existing campaign test binary.
