# M01-C3-PLAYER-CONFIG (#1580): C3/M01's player airframe and start pose

Date: 2026-10-18. Task #1580. Implementer: Devin SWE-2 (`swe2-max-1`).
Capabilities used: `retail` (reads of `ZBD/C3/M01/zrdr.zbd` member `aiv.zrd`
and the campaign scan) and the owner's decrypted image
(`crimson.decrypted.exe`, sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA =
file offset + `0x400000`). Every claim is `static` / `ObservedTool`; nothing
here is `verified_original` and no original executable ran.

Builds on `2026-10-17-m01-archive-decision.md` (#1565, M01 is `ZBD/C3/M01`),
`2026-10-08-m01-lc-campaign-airframe-engine-state.md` (#770, the measured
engine state), `2026-10-06-m01-lc-player-airframe-source.md` (#715, the
airframe table), `2026-10-15-m01-aiv-layout.md` (#1453, the record layout)
and `2026-10-18-m01-m06-archive-finding-prose-audit.md` (#1568).

## Verdict

The launch plan for M01 "The Lost Treasure" (`mission/ch1-m01`,
`world/c3`, `script/c3-m01-zrdr`, `zbd/c3/m01`) reports
`player_configuration: Satisfied` **both** with `CS_ENGINE_IMAGE` named and
with it unset. Printed from the production binary:

```
$ env -u CS_ENGINE_IMAGE ./target/debug/cs --cs-path $CS_GAME_DIR --mission M01
cs: M01 cannot launch: 1 of 11 launch surfaces are not satisfied:
  world_geometry: unknown: the container imports to a WorldDefinition
  (256 partition cells, 458 objects) ... what no stage has answered is what
  the 47 grid records that store no mesh index drew ...
```

and identically with `CS_ENGINE_IMAGE` set. `player_configuration` is not
among the gaps in either run; the sole remaining gap is `world_geometry`,
which is task #1578's surface — once it lands, the same command opens the
window on `zbd/c3/m01` (the task's acceptance).

## The two measured facts the surface binds

### Airframe — the fresh-profile campaign default, row 5

No field of any `aiv.zrd` aircraft record names an airframe (the player's
field 0 is the none value `0xFFFFFFFF` in all 53 retail missions), and no
campaign `zrdr.zbd` carries the `player_plane` key — that key is read only
by the instant-action setup from `ia.zrd` (#715). What assigns the player
airframe on a fresh, profile-less campaign launch is the engine's own
`.data` plane-roster record, measured by #770:

- roster record at image VA `0x61a81c` (file offset `0x21a81c`), whose
  `+0x2c` dword is **5**;
- row 5 of the eleven-row airframe table (`0x620cfc`): display
  `Devastator`, scene root **`player_pfighter`**, model `piratefighter`,
  vehicle record **`pdevastator`**;
- measured chain: roster init `0x4113b0`, selection index `0x411477` = 0,
  `roster[0]+0x2c` = 5, campaign-start setter `0x41712c`, spawn `0x474d48`.

With the image named, `bind_engine_state` binds this from the image span
(`crimson.decrypted.exe` offset `0x21a81c`). Without it,
`bind_engine_facts` binds the same row from the committed
`data/owner-static/engine-facts.toml` `[[mission]]` row — which #1580
re-keyed from `zbd/c1c/m01` to **`zbd/c3/m01`** (the row was committed under
the pre-correction layout that called C1C/M01 "M01"). Both legs cite the
same measured bytes, so the surface names its sources either way.

### Start pose — the mission's own `aiv.zrd` `player` record

`ZBD/C3/M01/zrdr.zbd` member `aiv.zrd`, record `player` (document index 1,
the only `player` record; the table has four records total — `player`,
`medkestrel_1..3` asleep, `wingman_1`, per #1571):

- field 1 (`position_field`): **`(-1426, 150, -1813)`** metres — scale
  `metres_per_unit = 1.0` (#436's owner note), +Y up, right-handed,
  identity axis map;
- field 2 (`heading_field`): **`40.0`** stored degrees →
  `stored_heading_radians(40.0)` = **0.6981317007976 rad**, converted by
  the sequence at VA `0x47c4e5` multiplying the image's own π/180 f64 at
  `0x6040e8` (`0.01745329251994`) — the measured convention, never an
  invented yaw;
- field 0: `0xFFFFFFFF` (none), as #715 measured for every retail player
  record.

The pose's provenance names the `aiv.zrd` member span of
`ZBD/C3/M01/zrdr.zbd`; the conversion's provenance names the image span at
offset `0x7c4e5`. #1571 independently measured the same player record:
`wingman_1` (Betty) sits at `(+48, 0, +107.08)` from `(-1426, 150, -1813)`
heading 40.

## Corrections made by this task

`data/owner-static/engine-facts.toml` `[[mission]]` rows were keyed to the
pre-#1565 directory-digit attribution. Corrected with provenance (the
`OWNER-STATIC-2026-10-18` claim), labels now naming the work order each
committed `missions/bindings/*.json` gives the directory:

- `zbd/c3/m01` — the known row (`va 0x61a81c`, `airframe_index 5`,
  `record "pdevastator"`), label `M01`;
- `zbd/c1c/m01` — label `M06`, `status = "unknown"`: a progression-reached
  mission carries no per-mission claim (whether progression or undecoded
  mission language changes the measured default is unmeasured);
- stale labels removed or corrected to the committed bindings:
  `zbd/c1b/m03` → `M08`, `zbd/c1/m04` → unlabeled (no M09 binding),
  `zbd/c1/m05` → `M10`, `zbd/c2/m01` → `M12`, `zbd/c2/m03` → `M13`,
  `zbd/c3/m02..m05` → `M02..M05`.

Tests referring to the old attribution were reframed as what they measure —
the `zbd/c1c/m01` archive is M06's: `retail_m01_*` test names and `M01`
prose on c1c bytes became `retail_m06_*`/`M06` in `m01_lc_player_config.rs`,
`m01_lc_campaign_airframe_pose.rs`, `m01_lc_player_airframe_source.rs`,
`m01_aiv_layout.rs` and the `m01_lc_campaign_airframe_pose_evidence.rs`
harness. `owner_facts_table.rs` re-points its M01 assertions at
`zbd/c3/m01`; its launch-gate test keeps exercising a launchable plan on
M06 **with the image** (`bind_engine_state` binds the default for any dir).
`vs_m01_runtime.rs` already framed the archive as M06 and is untouched.

Consequence filed as #1585: ~20 campaign test files compose the
`zbd/c1c/m01` stage through `stage_for`/`MissionContent::prepare` while
declaring only `#[ignore = "requires CS_GAME_DIR"]`. Image-less they now
fail loudly at `StartUnresolved("the player's airframe")` — the honest
explicit unknown — where they previously passed only through the
misattributed row. Their real capability is `CS_GAME_DIR` +
`CS_ENGINE_IMAGE`; the audit is tracked separately rather than ballooning
this slice. Verified failing:
`m01_lc_player_config::accept_m01_window_world_start_pose_is_the_aiv_zrd_player_record`
(its attribute is corrected here because the test lives in this task's
subject file).

## Observation only — the walkthrough's cave

The owner's walkthrough of the original game notes that M01's start is a
low-altitude approach **facing a cave** the player flies into. The stored
record is consistent with that reading (`150` m altitude, heading `40`
stored degrees), but this is an **observation**, not a measured causal or
gameplay claim: nothing here ran the original, the heading convention only
says which way the nose points in the canonical frame, and what the pose
"faces" in the world is a world-geometry question (#1578's surface is the
one still open). Recorded so the pose is checkable against play footage
once an original reference capture exists.

## Acceptance tests

`crates/cs_app/tests/campaign/accept_m01_c3_player_config.rs`:

- `accept_m01_c3_player_config_retail_plan_satisfied_and_names_sources`
  (`requires CS_GAME_DIR`) — the production plan's `player_configuration`
  verdict is `Satisfied`, names `mission_start::MissionStartConfiguration`
  as its consumer, and `recover_retail_start_configuration("zbd/c3/m01")`
  binds `player_pfighter` citing the image span and the pose citing the
  `aiv.zrd` member span;
- `accept_m01_c3_player_config_committed_table_binds_m01`
  (`requires CS_GAME_DIR`) — the committed-table leg binds the same values
  on real retail bytes without naming the image.
