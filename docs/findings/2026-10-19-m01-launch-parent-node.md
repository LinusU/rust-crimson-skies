# PARITY-M01-ATMO-E4 (#1679): the Pandora's parent node and motion during the M01 launch cinematic

Date: 2026-10-19. Task #1679, follow-up of #1660
(`docs/findings/2026-10-18-m01-launch-end-pose.md`, unknowns `parent-of-parent`
and `parent-motion`). Implementer: `sonnet-1`. Capabilities used: **`retail`**
(read-only `$CS_GAME_DIR`) and **static analysis** of the owner-supplied
decrypted image (`$CS_ENGINE_IMAGE`, sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
VA = file offset + `0x400000`, `llvm-objdump -d -M intel`). No original
executable ran and no `original-launch/` frame exists on this machine: nothing
here is `verified_original`, and **no visual comparison is claimed**. Test
prefix `accept_parity_m01_atmo_e4_`. Production reader:
`cs_app::cinematics::launch_parent`.

## Verdict

1. **`piratezep`'s parent is the world root.** In `ZBD/C3/gamez.zbd` the node
   named `piratezep` is unique, is an `Object3d` (slot 1020) that stores the
   identity transform (translation and rotation all zero), and its stored parent
   slot is 0, the `World` node `world1`, which is a root. `world1` lists
   `piratezep` among its children. So the `zeppelins.zrd` position applied
   through the Object3d setter `0x4d1d50` is a `world1` position, and #1660's
   composed drop-end position (−1399.25, 436.71, −1410.86) m is a world
   position as far as the scene graph goes. This resolves `parent-of-parent`.
2. **Nothing in the data moves the Pandora's node.** Over every animation of
   `ZBD/C3/cam_anim.zbd` and `ZBD/C3/M01/mis_anim.zbd`, the events that name
   `piratezep` are only the intro's child operations (`OBJECT_ADD_CHILD` /
   `OBJECT_DELETE_CHILD` with `piratezep` as the parent, op 15/16), one
   `OBJECT_ACTIVE_STATE` (op 6) in `callback_sequence`, and `gi_pfighter1`
   adding `piratefighter` to it. **No `OBJECT_MOTION_SI_SCRIPT` (op 12) and no
   `OBJECT_ROTATE_STATE` (op 9) names it**, and no record that lists it in its
   `Objects` table carries `AUTO_RESET_NODE_STATES` (#1678).
3. **The zeppelin's own motion code does not run during the intro.** The
   Pandora is also a zeppelin actor (`zeppelins.zrd` record, class parsed by
   `0x4bd8d0`) with a speed, a route and its own per-frame update that writes the
   node. That update is reached only from the world update `0x4897c0`, which
   skips everything from `0x4897f9` to `0x489e30` while `[0x71c50c]` (set by the
   intro's callback 20) is non-zero. So between the intro's start and its end the
   Pandora stays where `zeppelins.zrd` put it: **position (−1401.4, 500,
   −1412.5), yaw 40°**, i.e. at the 1.95 s stop of `gi_playerdrop` it has moved
   by **0 m**, up to the one-frame ordering question in `first-frame-order`
   below. This resolves `parent-motion` for the intro; it says nothing about the
   Pandora after the intro.

## The chain, link by link

### Parent (data)

| link | value | where |
| --- | --- | --- |
| node | `piratezep`, slot 1020, `Object3d`, translation (0,0,0), rotation (0,0,0); the name occurs once | `ZBD/C3/gamez.zbd` node array (`cs_formats::gamez::read_gamez_nodes`) |
| parent | slot 0 = `world1`, kind `world`, no transform field, no parent | same |
| child link | `world1`'s child list contains slot 1020 | same |
| placement | `zeppelins.zrd` `piratezep`: position (−1401.4, 500, −1412.5), yaw 40 (#1660); C3/M01 has no `placezeps.zrd` | `ZBD/C3/M01/zrdr.zbd` |

### Animation events naming `piratezep` (data)

Layouts read: op 6 node at word `+0x10`; op 9 node at word `+0x1c`; op 12 node at
word `+0x10` (#1660, #1662); ops 15/16 parent at half `+0xc`, child at half
`+0xe` (#1660). `NodeRole::writes_transform` is true for ops 9 and 12 only.

| record | events |
| --- | --- |
| `generic_intro` `RESET_SEQUENCE` | three `OBJECT_DELETE_CHILD` (parent `piratezep`; children `camera1`, `player`, `piratefighter`) |
| `generic_intro` `callback_sequence` | `OBJECT_ACTIVE_STATE` on `piratezep` |
| `generic_intro` `start_script` | two `OBJECT_ADD_CHILD` (parent `piratezep`; children `camera1`, `player`) |
| `gi_pfighter1` | `OBJECT_ADD_CHILD` (parent `piratezep`, child `piratefighter`) |
| everything else in the two carriers | none |

### Zeppelin actor update (static)

| link | address | observed |
| --- | --- | --- |
| loader | `0x4bd8d0` | `position` → `obj+0x20..0x28` (`0x4bda75`, `0x4bda7e`, `0x4bda8d`); `yaw`/`pitch` → `+0x2c`/`+0x30`; `max_speed` → `+0x9c` (`0x4bdad6`); `max_accel` → `+0xa8` (`0x4bdaf4`) |
| constructor | `0x4bd460` | bytes `+0x4`, `+0x5`, `+0x6` and the speed `+0xa4` are cleared (`0x4bd483`, `0x4bd52b`): a zeppelin starts **not enabled** with speed 0 |
| enable / disable | `0x4bd370` / `0x4bd390` | set `+0x4` to 1 / 0 on every zeppelin of the manager `0x71df80` |
| callbacks | `0x47e4b8` → `0x41f250` (id 0x391, disables); `0x47e510` → `0x41f2e0` (id 0x392, enables) | `generic_intro`'s `RESET_SEQUENCE` fires 0x392; `player_setup` (`LOAD_GAME_START`) fires 0x391. Both functions have other callers, not read |
| per-frame | `0x4bf9d0` | reads `+0x4` (`0x4bfa0f`); zero → branch over all movement (`0x4bfa16`) |
| movement | `0x4bf710`, `0x4bf930` → `0x4bf9b0` → `0x4d1d50` | `pos += speed(+0xa4) × game_dt` along the heading, then the node is written from the object. The speed changes only in `0x4bf240` (toward the target speed its callers pass, at `+0xac × game_dt`); other writers: constructor, save-game restore `0x4c0adc` |
| manager update | `0x4bd3b0` | one caller: `0x489c75`, in `0x4897c0` |
| world update gate | `0x4897dd` / `0x4897f3` | `jne 0x489e30` when `[0x71c50c]` ≠ 0, past `0x489c75`; no `call`/`jmp rel32` from outside `0x4897c0`..`0x489e30` lands in `0x4897f9`..`0x489e30` (the acceptance test scans the image); `[0x71c50c]` is set by callback 20, cleared by callback 0 at the animation's end (#1678) |

`accept_parity_m01_atmo_e4_engine_facts_are_the_bytes_the_finding_read`
re-reads every row from the image.

## What this does to #1660's composed position

Nothing changes numerically: the parent chain adds no transform and the
Pandora does not move, so (−1399.25, **436.71**, −1410.86) m stays the world
position of the drop's end under #1660's stated assumptions (`rotation-convention`
for the pitch sign, #1673). The two premises #1660 flagged are now measured,
except the one-frame ordering below.

## What stays unknown, what it affects, who resolves it

The same table is `PARENT_UNKNOWNS`; the production reader returns none of
these values and fills in none of them.

| unknown | affects | resolver |
| --- | --- | --- |
| `first-frame-order`: whether a world update runs between the reset sequence enabling the zeppelins (callback 0x392) and callback 20 setting `[0x71c50c]`; both fire when the intro starts and their order against the frame loop was not traced. At most one update, giving a speed of one ramp step times `game_dt` | the Pandora's position by at most one frame's displacement | follow-up reading `0x4ed340` against the frame loop; or the owner's capture |
| `pre-intro-frames` | nothing measured | none unless `other-enablers` runs at load |
| `other-enablers`: `0x496610` and `0x4966aa` also enable every zeppelin; not read | whether zeppelins are enabled before the intro | follow-up reading `0x4965d0`..`0x4966b0` |
| `multiplayer-net-path`: `0x4bf9d0` calls `0x470550` (interpolation, writes the node) when `0x440ad0` is non-zero | multiplayer only | multiplayer tasks |
| `save-game-restore`: `0x4c0a90` restores speed and position | a loaded game only | save-game tasks |
| `post-intro-motion`: after the intro the update runs and the Pandora can follow its route (tables `[0x64f610]`, `[0x64f614]`); not read | the Pandora's position after the intro | follow-up reading `0x4bf9d0`'s route selection |
| `no-run` | every claim | the owner's reference capture |

`LAUNCH_END_POSE_UNKNOWNS` in `launch_end_pose.rs` (#1660) still names
`parent-of-parent` and `parent-motion` with resolver #1679; this task resolves
them here and leaves that table untouched (outside its owner paths).

## Files

* `crates/cs_app/src/cinematics/launch_parent.rs` (new): `measure_ancestry`,
  `ancestry_in`, `events_naming_node`, `ZEPPELIN_ENGINE_FACTS`,
  `PARENT_UNKNOWNS`. No spawn wiring.
* `crates/cs_app/src/cinematics/mod.rs`, `crates/cs_app/tests/campaign/mod.rs`:
  module declarations (wiring).
* `crates/cs_app/tests/campaign/parity_m01_atmo_e4.rs`: two synthetic members
  (CI), two `#[ignore]` members needing `CS_GAME_DIR`, one needing
  `CS_ENGINE_IMAGE`.

## Reproduction

```sh
CS_GAME_DIR=<original installation> CS_ENGINE_IMAGE=<crimson.decrypted.exe> \
  cargo test --locked -p cs_app --test cs_app_tests_d -- accept_parity_m01_atmo_e4_ --include-ignored
```
