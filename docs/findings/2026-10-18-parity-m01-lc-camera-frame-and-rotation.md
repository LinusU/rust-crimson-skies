# PARITY-M01-LC-MEASURE-CAM-FRAME (#1673): the camera tracks are in the Pandora's frame, and the rotation is `exp(v) · q`

Date: 2026-10-18. Task #1673, follow-up to #1662
(`2026-10-18-parity-m01-lc-cinematic-camera-pose-source.md`). Capabilities used:
**`retail`** (read-only `$CS_GAME_DIR`) and **static analysis** of the
owner-supplied decrypted image (`$CS_ENGINE_IMAGE`, sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, VA = file
offset + `0x400000`, `objdump -d -M intel`). No original executable ran and no
camera path was recorded from Drydock: nothing here is `verified_original`.
Test prefix `accept_parity_m01_lc_measure_cam_frame_`.

## Verdict

1. **Frame.** The `gi_cam*` positions are in the **frame of `piratezep`, the
   Pandora**, not the world. `generic_intro`'s `start_script` takes `camera1`
   out of `world1` and adds it to `piratezep` before any scene plays; no scene
   animation re-parents it. The Pandora is a child of the world root `world1`,
   which has no parent. The track is the node's own local position (event flag
   bit 0 is clear) and the engine's re-parenting executors never rewrite a
   child's local numbers (`2026-10-18-m01-launch-end-pose.md`), so the numbers
   stay Pandora-local.
2. **Node type.** `camera1` is a **camera node (type 1)**, not an `Object3d`. The
   motion executor therefore calls the camera setters, `0x4d2710` for the
   position and `0x4d2490` for the rotation, not `0x4d1d50` / `0x4d1a30`
   (#1662's table named the `Object3d` pair for type 5, which is the
   player's and the fighters' branch). Both camera setters store local numbers
   in the camera's own record: position at `class+0x14`, the three angles at
   `class+0x20`, `+0x24`, `+0x28`.
3. **Rotation.** The node's angles are
   `euler( matrix( normalise( exp(v(u)) · q_start ) ) )`, where `v(u)` is the
   part's rotation polynomial, `q_start` its words 0..3 `(w, x, y, z)`, `exp(v) =
   (cos|v|, sin|v| · v/|v|)` **without halving the angle** (so a vector of length
   `a` turns by `2a`), the product is Hamilton's with the polynomial quaternion
   on the **left**, and the angles are `(x, y, z)` = pitch, yaw, roll of
   `R = Ry(y) · Rx(x) · Rz(z)` on column vectors. Measured, not assumed: see
   "Measurement".

## 1. The frame

### The chain, from the data

| link | value | where | status |
| --- | --- | --- | --- |
| camera node | `camera1`, index 3 in `ZBD/C3/gamez.zbd`, kind **Camera**, no parent, not in `world1`'s child list | `read_gamez_nodes` | measured |
| the Pandora | `piratezep`, index 1020, `Object3d`, parent `world1` | same | measured |
| the root | `world1`, index 0, no parent; the display and the window are separate roots (indices 1, 2) | same | measured |
| re-parent | `generic_intro` / `start_script`: `OBJECT_DELETE_CHILD` (op 16) parent `world1` child `camera1`, then `OBJECT_ADD_CHILD` (op 15) parent `piratezep` child `camera1`, both START_TIME 0, the delete first in stream order | `ZBD/C3/cam_anim.zbd` record `generic_intro` | measured |
| reset | `RESET_SEQUENCE`: `OBJECT_DELETE_CHILD` parent `piratezep` child `camera1` | same | measured |
| no other re-parent | `gi_scene1`, `gi_scene2`, `gi_1stperson`, `gi_playerdrop`, `gi_pfighter2` contain no child event; `gi_pfighter1` re-parents `piratefighter` only | all `gi_*` records | measured |
| executor | `OBJECT_ADD_CHILD` appends to both nodes' pointer lists with no check of the child's type; no call to a position or rotation setter | `0x4cd610`, `0x4d1390` | static (read in #1660, re-read here) |
| flag | event flag bit 0 is clear in every camera event: no base is added | #1662 | measured |

So the camera's pose in the scene is `Pandora_world · camera_local`, with
`camera_local` the position and angles below. (The camera node's data record in
the file has `class+0x10` = 0, so bit 1 of the flag word that selects the
position slot at `class+0xa4` instead of `+0x14` is clear; the rotation setter
clears it as well.)

### Compared with the Pandora's placement

C3/M01 has **no** `placezeps.zrd` member (its `.zrd` members are `dzones`,
`egen`, `location`, `mis_anim`, `objectives`, `startanims`, `targets`,
`weather`, `zeppelins`, `tex_drop`), so no startup placement overrides the
Pandora's `zeppelins.zrd` record: position (−1401.4, 500, −1412.5), yaw 40°,
pitch 0 (`2026-10-18-m01-launch-end-pose.md`; the M06 archive's placement
(−3584, 1360, −8704) is a different mission).

Illustration only, assuming the camera's view is the plain composition (yaw 40°
turns local +Z toward +X): the first position of `gi_cam1`, (−155.7, −62.0,
−769.0) in the Pandora's frame, is (−2015, 438, −1902) m in the world, 770 m
behind and 62 m below the Pandora, which is where a cinematic that opens on the
Pandora would stand. This arithmetic is not tested; the composition is the
unknown below.

## 2. The rotation

### The helper chain (static, `provenance = static`)

The rotation branch of `0x4ea3b0` (`0x4ea5ab`..`0x4ea6d5`):

| step | address | what it does |
| --- | --- | --- |
| vector | `0x4ea5c2`..`0x4ea623` | three cubics of local time `u` (`mode != 0`), else `u · words 4..6` with **no** word 0..3 term (the position part of the same mode adds them) |
| `0x53fbf0(v, q1)` | call at `0x4ea631` | `q1 = (cos θ, sin θ · v / θ)`, `θ = |v|`; identity `(1,0,0,0)` when `θ` is not above the float at `0x6032c8` (= 0.0). Uses `fsin`/`fcos`. **The angle is not halved.** |
| `0x53f920(q1, start, q2)` | call at `0x4ea641` | Hamilton product, first component the scalar; the **first** argument is `q1` (the polynomial), the second the part's words 0..3. `q2 = q1 · start` |
| `0x53f850(q2, q2)` | call at `0x4ea650` | in place: `q2 · (1.0 / s)` with `s = bits(|q2|²)` halved arithmetically plus `0x1fc00000` read back as a float, a bit-pattern square-root approximation exact at 1.0 and second-order near it (1.0 is the float at `0x6032dc`) |
| `0x53fa40(q2, m)` | call at `0x4ea65f` | nine floats `m[0..9]`, row-major: row 0 `(1−2y²−2z², 2xy+2wz, 2xz−2wy)`, row 1 `(2xy−2wz, 1−2x²−2z², 2yz+2wx)`, row 2 `(2xz+2wy, 2yz−2wx, 1−2x²−2y²)`. This is the **transpose** of the column-vector rotation matrix |
| `0x53df30(m, e)` | call at `0x4ea66e` | if `|m[7]| < 1.0` (the double at `0x6032e8`): `e = (asin(−m[7]), atan2(m[6], m[8]), atan2(m[1], m[4]))`; else `e = (−copysign(π/2, m[7]), atan2(−m[2], m[0]), 0)`. `asin` and `copysign` are MSVCRT imports (`_CIasin` at `0xa20198`, `_copysign` at `0xa2027c`) |
| flag bit 1 | `0x4ea679`..`0x4ea69a` | when set, the event's base triple is added to `e` component-wise (clear in every camera event) |
| setter | `0x4ea69e`..`0x4ea6d5` | node type 1: camera rotation setter `0x4d2490(node, x, y, z)`, which stores the three floats at `class+0x20/+0x24/+0x28` and clears bit 1 of `class+0x10`; type 5: `Object3d` `SetRotation` `0x4d1a30`; other types: nothing |

With `m[7] = 2yz − 2wx`, `m[6] = 2xz + 2wy`, `m[8] = 1 − 2x² − 2y²`,
`m[1] = 2xy + 2wz`, `m[4] = 1 − 2x² − 2z²`, those angles are the pitch `x`, yaw
`y`, roll `z` of `R = Ry(y) · Rx(x) · Rz(z)`: the rotation about Z is applied
first, then X, then Y. This is the same convention #770 §12.2 found for the
stored headings (`M = Ry·Rx·Rz`). The Euler triple is stored as given; its
sign convention for a positive angle (left or right handed world) is the
engine's own and is not interpreted here.

### Measurement

For the 14 `generic_intro` tracks of `ZBD/C3/cam_anim.zbd`
(`cs_formats::zbd::anim_tracks::TrackSegment::orientation_at`), at every
boundary between two contiguous segments that both have a rotation part
(more than 300 boundaries), the orientation composed at the end of segment *i*
should equal segment *i+1*'s stored start quaternion, because the spline is
continuous. The deviation `1 − |q · q_next|` is:

| composition | worst deviation |
| --- | --- |
| `exp(v) · q_start` (the engine's) | **8e-8** (float precision) |
| `q_start · exp(v)` | up to 2e-3 |
| `q_start` alone (ignoring the polynomial) | up to 1.5e-2 |

The polynomial terms reach a vector length of 0.17 (a 20° turn), so the order is
visible in the data, and the data agree with the instruction reading.
For every segment, at the start, middle and end of its span, the angles rebuild
the matrix through `Ry · Rx · Rz` within 1e-5 (the engine's approximate
normalisation leaves the quaternion a few 1e-7 off unit length), outside the
gimbal branch.

Implementation: `orientation_at` computes in `f64` where the engine uses the x87
stack, so agreement is to float precision, not bit for bit.

## 3. What stays unknown, what it affects, who resolves it

| unknown | affects | resolver |
| --- | --- | --- |
| `camera-view-composition`: how the camera's local numbers and the Pandora's world transform become the view (the renderer's matrix is not read; the child-list code does not compose anything) | the camera's world pose | #1665 reads the renderer, or the owner's capture |
| `handedness`: the world's handedness for a positive Euler angle | the sign of yaw/pitch/roll on screen | #1665 with the owner's capture; the helper chain fixes the triple, not the screen sense |
| the Pandora's motion during the cinematic (`parent-motion`) | the camera's world pose over time | #1679 |
| the camera's field of view | the projection | none found (#1662) |
| the scene order, the clock and the variant draw | cut points, total length | #1674 |
| the stored world lists no camera child although `start_script` deletes `camera1` from `world1`: whether the delete is a no-op on a node not in the list | none for the pose | not examined |

## Files

- `crates/cs_formats/src/zbd/anim_tracks.rs`: `TrackPart::rotation_vector_at`,
  `TrackSegment::orientation_at`, `SegmentOrientation`.
- `crates/cs_formats/tests/zbd/t1673.rs` (+ `mod t1673;`): synthetic tests of
  every step, the retail continuity and Euler tests, the image-byte test;
  `t1662.rs` helpers made `pub(super)`.
- `crates/cs_app/tests/campaign/parity_m01_lc_measure_cam_frame.rs`
  (+ `mod`): the re-parenting events and the C3 node graph.

## Reproduction

```sh
CS_GAME_DIR=<original installation> CS_ENGINE_IMAGE=<crimson.decrypted.exe> \
  cargo test --locked -p cs_formats -- accept_parity_m01_lc_measure_cam_frame_ --include-ignored
CS_GAME_DIR=<original installation> \
  cargo test --locked -p cs_app -- accept_parity_m01_lc_measure_cam_frame_ --include-ignored
```
