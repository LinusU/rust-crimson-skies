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

## #676 M01-LC-PLAYER-AIRFRAME-POSE: what the measurement found

Date: 2026-10-05. Capability: **`retail`**. Nothing is `verified_original`. The
measurement dumped the `player` record of `aiv.zrd` in every retail
`zbd/*/*/zrdr.zbd` that has one (**53** missions, all decoding) and M01's
whole table.

**Airframe: still unknown, now with evidence.**
* Field 0 of the player record is `0xFFFFFFFF` in all 53 missions, and of every
  `wingman_<n>` record in M01. Scripted AI aircraft carry other values there
  (M01: `rusdevastator_*` 24, `devastator_2` 28, `devastator_3` 29,
  `bsfury_1` 34). The header's `(id, name)` table has entries for 24, 25, 28
  and 29 (`Scout1`..`Scout4`) but not 34, so even the AI values are unmeasured.
* Field 4 is `5` on M01's player and wingmates but `0` in most other missions
  and `4` in `c1b` and `c3/m02`; it is the same for the player and the AI in a
  mission, so it is not a per-aircraft airframe field.
* No member of M01's `zrdr.zbd` contains the text `bloodhawk` (searched all 12).
  `player_setup.zrd` toggles the player's active state and names no airframe.
  The airframe is not stored in the mission data read so far; it may be a
  profile, hangar or mission-program choice.

**Pose: the stored values are bound; the metric pose stays unknown.**
* Field 1 is a three-float vector, field 2 a float, on every player record.
  M01: position `(-3694, 1318, -12482)`, heading `170`; `wingman_3`
  `(-4531, 1280, -13065)`, `wingman_2` `(-3421, 1350, -13114)`, both `180`.
* Axis 1 is vertical (110..1400 against 1363..13257 on the other axes). The
  heading is not radians (|value| reaches 330; all 53 are multiples of 5).
* M01's start lies outside `c1c`'s grid `[-12288, 0]^2` on the third axis
  (by 194 stored units for the player), so the frame relation to the world is
  not simply "inside the grid".
* Unmeasured: the position unit (#436, blocked), the heading's zero direction
  and handedness.

New in `cs_app::mission_start`: `StoredStartPose`, `StartRecord::field_zero`,
`StartRecord::stored_pose` and `MissionStartConfiguration::stored_pose()`
(`Known`, `ObservedTool`, with the member's span). `airframe()` and
`initial_pose()` stay `Unknown` with updated reasons.

**Unmet:** the airframe and the metric pose. Affected: VS-M01-RUNTIME (#359)
player spawn. Resolving: #436 (unit), F13-B/C and F38 (statements that may assign
an airframe or move the aircraft). `missions/bindings/M01.json` is protected and
unchanged, so its unknown still stands.
