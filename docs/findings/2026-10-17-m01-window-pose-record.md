# M01-WINDOW-POSE: where M01's start pose comes from

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

Date: 2026-10-17. Task: #1555 (owner #1537, bullet 3). Capability: **`retail`**
(read-only `$CS_GAME_DIR`) and ordinary build/test. No original run happened;
nothing here is `verified_original`.

## The record

| Part | Value |
| --- | --- |
| Container | `ZBD/C1C/M01/zrdr.zbd` |
| Member | `aiv.zrd` (9869 bytes; header record plus 12 aircraft records) |
| Record | the one named `player`, document index 1 |
| Position | field `POSITION_FIELD = 1`, three floats `[-3694.0, 1318.0, -12482.0]`; unit the metre, axis 1 vertical (#436) |
| Heading | field `HEADING_FIELD = 2`, float `170.0`, degrees-like, converted by `stored_heading_radians` (image-measured constant, `STORED_HEADING_DEGREES_TO_RADIANS`) to `2.9670596` rad |

The record's byte range inside the member is the `SourceSpan` carried by
`MissionStartConfiguration::player()` and `stored_pose()` (same span for both).
The code reads it in `crates/cs_app/src/mission_start.rs`
(`recover_retail_start_configuration`); `stage_for` copies
`initial_pose()` into `MissionStage::start`, `build_headless` copies that into
the `MissionPlayerStart` spawn recipe and spawns the body from it.

## The regression test

`accept_m01_window_world_start_pose_is_the_aiv_zrd_player_record` in
`crates/cs_app/tests/campaign/m01_lc_player_config.rs` (ignored without
`CS_GAME_DIR`; the committed engine-facts table, #1432, stands in for
`CS_ENGINE_IMAGE`). It reads the record through production code, asserts the
container, member, record name and index and a non-empty span, then asserts
that `stage.start`, `MissionPlayerStart::position_m` and the live
`MissionPlayerBody` `Position` equal the record's values. Replacing the binding
with any other source changes one of those numbers.

## Observation: the walkthrough (labelled, not a source of values)

The community walkthrough of "The Lost Treasure"
(https://crimsonskies.fandom.com/wiki/The_Lost_Treasure) describes the player
starting at low altitude facing a cave in a cliff. The record stores
y = 1318 m. **This is an observation only.** Nothing was adjusted to match it.
The difference is unexplained, and is open as a record/frame question:

* The stored pose might be relative to a mission origin or terrain frame that
  has not been measured (the relation between a stored start and the world grid
  is still unmeasured, see `POSE_UNKNOWN_REASON`).
* The mission program (`startanims.zrd`'s `LOAD_GAME_START` calls
  `player_setup`) may move the aircraft before launch; its statements are
  undecoded (F13-B/C, F38).
* The prose may describe a later point than the launch frame.

No evidence available distinguishes these. Resolving it needs the
mission-program decoding or an original run, which only the owner can supply.
The record is not "corrected" to prose (AGENTS rule 4).

## Observation: the flight law's ceiling

The recovered flight law (`2026-10-08-flight-original-fixed-wing-law.md`) has an
atmosphere switch at 2000 m (`6561.68 ft`, image `0x463640`). The stored start
altitude, 1318 m, is **below** it, so the start is inside the lower-atmosphere
regime. (The climb to about 1900 m the task mentions is also below 2000 m;
that figure is a report from the task description, not something measured here.)

## Unmet

The frame question above remains open; it is not claimed resolved.
