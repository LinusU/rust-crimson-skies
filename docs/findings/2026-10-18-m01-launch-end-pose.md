# PARITY-M01-ATMO-E2 (#1660): the player's pose at the end of the M01 launch cinematic

Date: 2026-10-18. Task #1660, slice of #1652 (`PARITY-M01-ATMO-E`). Implementer:
`sonnet-1`. Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) and
**static analysis** of the owner-supplied decrypted image (`$CS_ENGINE_IMAGE`,
sha-256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
VA = file offset + `0x400000`, `objdump -d -M intel`). No original executable
ran and no `original-launch/` Drydock frame exists on this machine: nothing here
is `verified_original`, and **no visual comparison is claimed**. Test prefix
`accept_parity_m01_atmo_e2_`. Production reader:
`cs_app::cinematics::launch_end_pose`.

## Verdict

The end of the launch cinematic is a **measured pose in the Pandora's frame**,
not a world pose and not the stored `aiv.zrd` start. Three of the task's
premises are corrected by the data:

1. **The chain is not `hookpoint` / `pzhookup_*`.** M01's (`ZBD/C3/M01`)
   `startanims.zrd` has `NEW_GAME_START`: `generic_intro`, `flag_state_british`,
   `cg1zep_engines_start`, `pzep_engines_start`, `fueltrlight1`, `call_add_jack`,
   `disable_dropoff`, and `LOAD_GAME_START`: `player_setup`. (`wv_hookup_state`
   belongs to the M06 archive, `ZBD/C1C/M01`.) `generic_intro`, `gi_scene1/2`,
   `gi_1stperson`, `gi_playerdrop` and `gi_pfighter1/2` (all in
   `ZBD/C3/cam_anim.zbd`) call no `pzhookup_*` animation; the only mention is the
   node `pzhookup_crane` in `gi_1stperson`'s tables, which that animation sets
   inactive (`OBJECT_ACTIVE_STATE`, state 0). The `pzhookup_player` /
   `pzhookup_playerhold` scripts in `C3/M01/mis_anim.zbd` (#1659) belong to a
   hook-up, not to this launch; nothing in the launch chain reaches them.
   `player_setup` moves nothing (it sets the player node active and fires
   callbacks `0x14 0x2 0xb 0xe 0x391`).
2. **The player is a child of the Pandora (`piratezep`) while the cinematic
   runs**, so every number in the player's tracks is in the Pandora's frame.
3. **The stored `aiv.zrd` player (−1426, 150, −1813), heading 40 is not the end
   of the drop.** Heading 40 is the Pandora's own `yaw` (`zeppelins.zrd`
   record `piratezep`: position (−1401.4, 500, −1412.5), yaw 40, pitch 0), and
   the stored position is 286.7 m below and 403.0 m away from where the drop ends.

## The chain, link by link

| link | value | where it is read | evidence |
| --- | --- | --- | --- |
| intro animation | `generic_intro` | `startanims.zrd`, first `NEW_GAME_START` row | `ZBD/C3/M01/zrdr.zbd` |
| player's parent | **`piratezep`** | `generic_intro`/`start_script`: `OBJECT_ADD_CHILD` (op 15) payload `0x10002` = child node 1 (`player`), parent node 2 (`piratezep`) in the record's `Nodes` table. The sequence first deletes `camera1` and `player` from `world1` (`0x40003`, `0x10003`), and adds `camera1` (`0x40002`) and `player` to `piratezep` | `ZBD/C3/cam_anim.zbd`, record `generic_intro`; executor `0x4eab20` reads parent at event `+0xc`, child at `+0xe` (`static`) |
| the drop | `drop_planes` calls `gi_playerdrop` (`CALL_ANIMATION`, START_TIME 0) and stops it (`STOP_ANIMATION`) at **1.95** (`0x3ff9999a`) | `generic_intro`/`drop_planes` | same record |
| the player's track | `gi_playerdrop`'s `OBJECT_MOTION_SI_SCRIPT` (op 12) for `Nodes[6]` = `player`, flag word 0 (absolute), `IndexWords[0]` = 10 → track block 10 = `gi_player2.zan` | `gi_playerdrop`, `ZBD/C3/cam_anim.zbd` tail (#1662) | executor `0x4ea7d0` (#1662) |
| frame | the track is the node's own position: the Object3d setter `0x4d1d50` takes the cubic values with no base added (event flag bit 0 clear), so they are in the parent's frame | #1662 + this chain | `static` |
| the Pandora's world pose | position (−1401.4, 500, −1412.5) metres, yaw 40°, pitch 0: **no startup placement overrides it** (C3/M01 has no `placezeps.zrd`), so the record's spawn values stand (#814) | `zeppelins.zrd`, bound by `bind_mission_world_actors` | `ParentPose::from_zeppelin` |

## The measured pose (stop time 1.95 s, segment 11 of `gi_player2`)

Segment 11 spans 1.8333 .. 1.9667 s; the stop time is 0.1167 s into it.

| quantity | value | note |
| --- | --- | --- |
| position in the Pandora's frame | (0.5918, **−63.2936**, 2.6339) m | cubics of the segment's position part at u = 0.1167 |
| orientation in the Pandora's frame | start quaternion of the segment `(w, x, y, z) = (0.78378, −0.62104, 0, 0)`: a pure X rotation of **76.78°**; the rotation polynomial adds < 0.014 | no Y or Z part: **yaw and roll relative to the Pandora are 0** |
| heading | the Pandora's yaw, **40°** | follows from the pure-X rotation under `M = Ry·Rx·Rz` (#770 §12.2); the sign of the pitch is `rotation-convention`, unknown |
| composed position (Pandora yaw 40°, right-handed) | (−1399.25, **436.71**, −1410.86) m | valid as a world position only if `parent-of-parent` resolves to the world root |
| velocity, derivative of the cubics at 1.95 s | (−0.04, **−20.02**, −0.10) m/s | but see below |
| rate over the preceding 0.1 s | (·, **−47.9**, ·) m/s | the track falls from 0 m/s at 0.5 s to ≈ −40 m/s at 1.83 s |

The velocity is **not a single number**: the last segment's spline eases to
rest (its derivative is −39.99 m/s at the segment start and ≈ 0 at its end,
yet it covers 5.7 m in 0.133 s), so the derivative at the stop time (−20.0) and
the rate the body arrives with (−40 .. −48) disagree. No field states a hand-off
velocity. It is recorded as unknown (`release-velocity`), not chosen.

## What the stored `aiv.zrd` player record is, relative to this

`ZBD/C3/M01/zrdr.zbd` `aiv.zrd` `player`: (−1426, 150, −1813), heading 40
(#1580). Relative to the Pandora (−1401.4, 500, −1412.5, yaw 40):

* world offset (−24.6, −350.0, −400.5) m: 350 m below and 401 m away;
* in the Pandora's frame: 239 m right (+X), 323 m behind (−Z), 350 m below;
* relative to the end of the drop: 286.7 m below, 403.0 m away horizontally;
* **heading equals the Pandora's yaw (40 = 40).**

So the stored record is a pose with the Pandora's heading, below and behind it.
Whether the first control frame is the drop's end (carried by physics from the
release) or the stored record is **not decided by anything read here**
(`release-rule`).

## Re-parent keeps the local numbers (static)

The intro's `RESET_SEQUENCE` (the state the record returns to) deletes
`camera1`, `player` and `piratefighter` from `piratezep` and adds `player` to
`world1`. The executors read for this finding:

| step | address | observed |
| --- | --- | --- |
| `OBJECT_ADD_CHILD` | `0x4eab20` | skips if the child is already in the parent's list (`[parent+0x5c]`, count `[parent+0x56]`), else calls `0x4cd500` |
| `OBJECT_DELETE_CHILD` | `0x4eab90` | calls `0x4cd6d0` |
| node-type dispatch | `0x4cd500` / `0x4cd6d0`; jump tables `0x4cd5e0` / `0x4cd7c8`, slot 4 | type 5 (`Object3d`) → `0x4cd561` → `0x4d1390` → list append `0x4cd610`; `0x4cd72d` → `0x4d13f0` → list removal `0x4cd850` |
| `0x4cd610`, `0x4cd850` | | append/remove in the two pointer lists (`[parent+0x5c]`, `[child+0x58]`) and set dirty flags (`[node+0x24]`, `[node+0x2c]`, `0x4cf830`); **neither calls the position setter `0x4d1d50` or the rotation setter `0x4d1a30`** |

A re-parent therefore keeps the child's local position and rotation numbers and
reinterprets them in the new parent's frame. If the cinematic's `RESET_SEQUENCE`
ran on the dropped player without anything else writing the node, the player
would sit at (0.59, −63.29, 2.63) *in the world root's frame*, which is the
floor of the world, so something else places the physics body. When the reset
runs, and what writes the body, are **not measured** (`release-rule`).
`accept_parity_m01_atmo_e2_reparent_executors_are_the_ones_the_finding_read`
re-reads these instruction bytes from the image.

## What stays unknown, what it affects, who resolves it

The same table is `LAUNCH_END_POSE_UNKNOWNS`; the production reader returns
none of these values and fills in none of them.

| unknown | affects | resolver |
| --- | --- | --- |
| `rotation-convention`: how the quaternion and rotation polynomial become the node's rotation triple; the pitch sign | the player's attitude (magnitude measured, sign not) | #1673 |
| `parent-of-parent`: `piratezep`'s own parent in the scene graph | whether the composed position is a world position | #1679 |
| `parent-motion`: whether the Pandora moves between start and 1.95 s of the drop | composed position, carried velocity | #1679 |
| `intro-clock`: when `drop_planes` starts (every call states START_TIME 0), the scheduler's tick | cinematic length; not the pose | #1674 |
| `branch-choice`: `check_balmoral` / `check_warhawk` (condition word `0x2000`, operands 5 and 6) decide whether `gi_1stperson` (9.9 s, `gi_player1`, ends at (0.81, −25.78, 15.30)) precedes the drop; both reach `drop_planes`. That 5 is the default Devastator row is inferred from #1580, not measured | a first-person shot before the drop | #1674 |
| `release-rule`: when the reset runs; what places the physics body | whether the first control frame is the drop's end or the stored record | #1678 |
| `release-velocity` | the first control frame's velocity | #1678 |
| aiv.zrd stores no speed | start speed | none: no field is measured; #1453 maps fields 0..6, 20, 21 |
| the camera of the first control frame | not this pose | #1665 / #1652 |

## Files

* `crates/cs_app/src/cinematics/launch_end_pose.rs` (new):
  `measure_launch_end_pose`, `ParentPose`, `LaunchEndPose`, `LocalPose`,
  `LAUNCH_END_POSE_UNKNOWNS`. No spawn wiring.
* `crates/cs_app/src/cinematics/mod.rs`: `pub mod launch_end_pose;` (wiring).
* `crates/cs_app/tests/campaign/parity_m01_atmo_e2.rs`, `mod.rs` (wiring):
  two synthetic members (CI) and two `#[ignore]` members (`CS_GAME_DIR`,
  `CS_ENGINE_IMAGE`).

## Reproduction

```sh
CS_GAME_DIR=<original installation> CS_ENGINE_IMAGE=<crimson.decrypted.exe> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- accept_parity_m01_atmo_e2_ --include-ignored
```
