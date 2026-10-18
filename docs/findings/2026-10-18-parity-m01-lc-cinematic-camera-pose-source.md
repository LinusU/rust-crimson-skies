# PARITY-M01-LC-MEASURE-CAM (#1662): the launch cinematic's camera pose is a `.zan` track in the tail of `cam_anim.zbd`

Date: 2026-10-18. Task #1662, slice (ii) of #1642 (`PARITY-M01-LAUNCH-CINEMATIC`),
owner option (a). Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) and
**static analysis** of the owner-supplied decrypted image (`$CS_ENGINE_IMAGE`,
sha-256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
image base `0x400000`, `objdump -d -M intel`). No original executable ran:
nothing here is `verified_original`. Test prefix:
`accept_parity_m01_lc_measure_cam_`. Option (b), a camera path recorded from
Drydock, was **not** used.

## Verdict

**The pose source exists, it is in the data, and it is not unknown.**
F40-D3 (`2026-10-13-f40-d3-cam-anim-camera-format.md`) concluded that no field
of `cam_anim.zbd` carries a camera pose keyframe. That was true of the *record
walk* it measured, and it named the one place it had not looked: "the payload
after the records is #650's unmeasured trailing region". That region is where
the poses are.

The launch cinematic's camera is the object `camera1`. Its path is the
compiled form of the `.zan` files `gi_cam1.zan`, `gi_cam1b.zan` … `gi_cam4b.zan`
that the container's member table already names (rows of the `.zan` kind
in `ZBD/C3/cam_anim.zbd`'s member table). The compiled tracks are the 405 135 bytes after the last
animation record. Each track is a list of time segments; each segment holds a
**cubic polynomial in x, y and z** and a **unit quaternion with a rotation
polynomial**. They are read by `cs_formats::zbd::anim_tracks` and pinned by
`crates/cs_formats/tests/zbd/t1662.rs`.

Three things stay unknown, and they gate #1665 (see the last section): the frame
the positions are in, how the four scenes follow one another on one timeline, and
the camera's field of view (no track carries one).

## Which archive

M01 "The Lost Treasure" is `ZBD/C3/M01` (`2026-10-17-m01-archive-decision.md`),
so its world group's camera carrier is **`ZBD/C3/cam_anim.zbd`**. F40-D3 and the
`m01-lc-*` findings measured `ZBD/C1C` (work order M06). The distinction does not
change the answer: the 14 intro tracks (`generic_intro/gi_*.zan`) are present in
all eight camera carriers (`C1`, `C1B`, `C1C`, `C2`, `C2B`, `C3`, `C4`, `C5`) and
are equal in all of them except one padding word per position part
(measured below). The test compares every carrier with `C3`.

## The layout of the tail (measured, `ObservedTool`)

The region after the last record of an animation container is, in 31 of the 61
containers (the same 31 #650 counted):

```text
region     := descriptor[n]  block[n]
descriptor := u32 ptr; u32 ptr; u32 type; u32 mode;
              u32 segment_count; u32 data_bytes; u32 ptr         (28 bytes)
block      := path NUL; object-name NUL; segment[segment_count]
segment    := u32 flags; f32 start; f32 end; part[popcount(flags)]
part       := f32[19]                                            (76 bytes)
```

* `n` is the number of `.zan` rows in the container's member table. Descriptor
  *k* and block *k* belong to the *k*-th such row, and the block's path repeats
  that row's path (separators normalised, because the camera carrier doubles
  them). Verified in all 31 containers.
* Blocks tile the region exactly, and every block's `data_bytes` is exactly the
  length of its walked segments: **1 090 blocks, 76 845 segments, no byte left
  over, no mismatch.** (The walk is `read_animation_tracks`; it refuses a region
  that does not tile.)
* The three `ptr` words are addresses in the memory image the writer
  serialised (`0x03e.....`, `0x045.....`, `0x047.....`). No rule maps them to a
  file position and none is read.
* `segment_count` is a segment count, not a row count of a fixed size: the
  segments of one block can have different lengths (12, 88 or 164 bytes, and up
  to 240), because `flags` says which parts follow. Flag bit 0 is a *position*
  part, bit 1 a *rotation* part, bit 2 a *scale* part; in the part order bit 0,
  bit 1, bit 2. Retail sets flags 0..=7, nothing above. Counts over all
  segments: flags 3 (position+rotation) 52 561, 2 (rotation) 9 029, 1 (position)
  6 182, 0 (neither) 8 596, 7 221, 4 168, 6 49, 5 39.
* `mode` is `1` in 1 075 blocks and `0` in 15 (all in mission carriers; none in
  the launch cinematic). `type` takes 101 distinct values (e.g. `0x82e`,
  `0x82f`, `0x731`, `0xe33`); **its meaning is unmeasured**.

### How the engine reads a part (static, `provenance = static`)

The motion executor for animation-event opcode 12 (`OBJECT_MOTION_SI_SCRIPT`, installed at
`0x4ee215` into dispatch slot `[0x727e10]`, entry `0x4ea7d0`) calls the segment
evaluator `0x4ea3b0`, which advances through a block with `0x4ea350`. The
facts below were read from those functions' instructions; none was run.

| fact | where |
| --- | --- |
| the executor looks the block up as `table[0x9fd154] + 28 * index`, where `index` is an entry of the animation instance's index-word table (`[instance+0x108]`); 28 is the descriptor size above | `0x4ea834`..`0x4ea84c` |
| the descriptor's `data_bytes` word is compared against a byte cursor on the instance (`[instance+0x20]`): the track is finished when the cursor reaches it | `0x4ea86d`..`0x4ea872` |
| the cursor advances by 12 per segment, plus 76 for each of flag bits 0, 1 and 2 set | `0x4ea35e`..`0x4ea389` |
| segment `start` and `end` are the floats at `+4` and `+8`; the local time `u` is the instance time minus `start` | `0x4ea3b7`..`0x4ea4bd` |
| position part, descriptor `mode != 0`: `x = ((a3·u + a2)·u + a1)·u + a0` with `a0..a3` at part words 7–10; `y` at words 11–14; `z` at words 15–18 | `0x4ea4d8`..`0x4ea516` |
| position part, `mode == 0`: `x = word0 + u·word4`, `y = word1 + u·word5`, `z = word2 + u·word6` | `0x4ea51e`..`0x4ea53a` |
| the position is written through the `Object3d` position setter (`0x4d1d50`) for a type-5 node, after adding the event's base triple when event flag bit 0 is set | `0x4ea53d`..`0x4ea57f` |
| rotation part: the same three cubics (or, for `mode == 0`, `u × words 4..6`) give a 3-vector; it goes through `0x53fbf0` (vector to quaternion, identity below a tolerance), `0x53f920` (quaternion product, first component the scalar) with the part's first four words, `0x53fa40` (to a 3×3 matrix) and `0x53df30` (to three Euler angles), then the `SetRotation` setter `0x4d1a30` | `0x4ea5ab`..`0x4ea6d5` |
| scale part: the same cubics, multiplied component-wise by the event's scale base when event flag bit 2 is set; setter `0x4d18d0` | `0x4ea6db`..`0x4ea79f` |
| the part's word 3 is read by none of these paths | whole of `0x4ea3b0` |

That last row is also measured in the data: **word 3 of the position part
differs between carriers while the rest of the 14 intro tracks does not** (a
handful of bytes per block, always at `segment + 24`; an uninitialised pad word).

Measured over all 76 845 segments, with the formulas above:

* every rotation part's first four words form a unit quaternion
  (largest deviation `1.3e-7`), which is why they are read as one;
* for the 14 intro tracks, the position cubic of segment *i* evaluated at that
  segment's length equals the position stored at the start of segment *i+1* to
  `3.1e-5` (positions of the order 100–800): the cubic is a continuous spline
  in local time, not a per-segment guess.

The unit of the time words is unmeasured. The tracks end at 15.9333,
12.4375, 9.95 and 1.9667, just above the stop times 15.9, 12.4, 9.9 and 1.95
that `generic_intro` and its callers state (`0x319` events, `START_TIME` words
`0x417e6666`, `0x41466666`, `0x411e6666`, `0x3ff9999a`), which is consistent with
seconds, and with the segment lengths `1/3`, `1/6`, `0.5` s, but is not a
measurement of the engine's clock.

## The join: event → index word → block (measured)

The camera cinematic's events are type `0x10c` (opcode 12, `OBJECT_MOTION_SI_SCRIPT`),
76 bytes. Their fixed words, in the notation of the walker (`event + offset`,
header included, so payload word *n* is at `+8+4n`):

| offset | meaning | evidence |
| --- | --- | --- |
| `+0x08` | `START_TIME` (0 in every camera event) | #690 |
| `+0x0c` | flag word; bit 0 = position is added to a base, bit 1 = rotation is added to a base, bit 2 = scale is multiplied. **0 in every camera event**, so every camera pose is absolute | `0x4ea53d`, `0x4ea679`, `0x4ea764` |
| `+0x10` | index into the record's `Nodes` table: the node to move | `0x4ea802`..`0x4ea819` |
| `+0x14` | index into the record's `IndexWords` table | `0x4ea829`..`0x4ea83a` |
| `+0x28` | `RUN_TIME`; 0 in every camera event | `0x4ea85d`, `0x4ea8b1` |

The `IndexWords` table is the `IndexWords` table of the record walk (#650, 4-byte
entries); its entries are the descriptor / `.zan`-row indices. For the intro
scenes of `ZBD/C3/cam_anim.zbd`, asserted by
`accept_parity_m01_lc_measure_cam_retail_c3_generic_intro_tracks`:

| scene record | `IndexWords` | tracks |
| --- | --- | --- |
| `gi_scene1` (object `camera1`) | 0, 1, 2, 3 | `gi_cam1`, `gi_cam1b`, `gi_cam1c`, `gi_cam1d` |
| `gi_scene2` (object `piratezep`) | 4, 5, 6 | `gi_cam2`, `gi_cam2b`, `gi_cam2c` |
| `gi_scene2`'s animation ref `gi_pfighter1` | — | `gi_pfighter1` (block 7) |
| `gi_1stperson` (object `player`) | 8, 9 | `gi_player1`, `gi_cam3` |
| `gi_playerdrop` (object `player`) | 10, 11, 12 | `gi_player2`, `gi_cam4`, `gi_cam4b` |
| `gi_playerdrop`'s animation ref `gi_pfighter2` | — | `gi_pfighter2` (block 13) |

In `gi_scene1` the `0x10c` event of `camera_start_script` has `+0x10 = 3`
(`Nodes[3]` is `camera1`) and `+0x14 = 0`; the same event in `gicam1b`, `gicam1c`,
`gicam1d` has `+0x14 = 1, 2, 3`.

## The fourteen tracks of the launch cinematic (`ZBD/C3/cam_anim.zbd`)

All have `mode = 1` (cubic). "Start" is the first position part's start; "end" is
its last cubic evaluated at the last segment's length. `no pos / no rot` count
segments without that part. Positions are in the unit and frame the file stores,
neither of which is measured.

| # | track | object | segments | length | type | start | end | no pos / no rot |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | `gi_cam1` | `camera1` | 48 | 15.9333 | `0x82e` | (−155.7, −62.0, −769.0) | (47.4, 46.1, −186.7) | 0 / 0 |
| 1 | `gi_cam1b` | `camera1` | 24 | 15.9333 | `0x82f` | (185.9, −62.0, −705.0) | (−62.3, 46.1, −152.5) | 0 / 0 |
| 2 | `gi_cam1c` | `camera1` | 24 | 15.9333 | `0x82f` | (94.2, 53.1, −528.6) | (−59.6, −26.3, −152.5) | 0 / 0 |
| 3 | `gi_cam1d` | `camera1` | 24 | 15.9333 | `0x82f` | (363.3, 53.1, 221.8) | (39.0, −26.3, −361.3) | 0 / 0 |
| 4 | `gi_cam2` | `camera1` | 20 | 12.4375 | `0x82e` | (69.2, −4.5, 219.8) | (64.7, −34.8, 232.8) | 0 / 0 |
| 5 | `gi_cam2b` | `camera1` | 20 | 12.4375 | `0x82f` | (107.5, 8.0, 219.8) | (103.0, −22.4, 232.8) | 0 / 0 |
| 6 | `gi_cam2c` | `camera1` | 20 | 12.4375 | `0x82f` | (110.0, −25.1, 118.5) | (78.9, −38.6, 48.3) | 0 / 0 |
| 7 | `gi_pfighter1` | `piratefighter` | 20 | 12.4375 | `0xe33` | (71.8, 3.4, 234.1) | (11.6, −47.2, 5.9) | 0 / 0 |
| 8 | `gi_player1` | `player` | 67 | 9.9500 | `0x731` | (1.2, −25.9, 32.7) | (0.8, −25.8, 15.3) | 1 / 3 |
| 9 | `gi_cam3` | `camera1` | 40 | 9.9500 | `0x82e` | (−4.5, −27.5, 44.7) | (−0.5, −26.3, 35.0) | 0 / 0 |
| 10 | `gi_player2` | `player` | 12 | 1.9667 | `0x731` | (0.7, −30.1, 2.8) | (0.6, −63.5, 2.6) | 2 / 2 |
| 11 | `gi_cam4` | `camera1` | 12 | 1.9667 | `0x82e` | (94.2, −26.7, 42.0) | (94.2, −26.7, 42.0) | 11 / 0 |
| 12 | `gi_cam4b` | `camera1` | 6 | 1.9667 | `0x82f` | (87.2, −30.6, −85.3) | (87.2, −30.6, −85.3) | 5 / 0 |
| 13 | `gi_pfighter2` | `piratefighter` | 12 | 1.9667 | `0xe33` | (0.7, −40.6, 2.8) | (0.6, −126.1, 2.5) | 0 / 0 |

`gi_cam4`/`gi_cam4b` are rotation-only after their first segment: a camera that
stays put and turns. The `gi_cam1*` family is the long scene (15.93 s); the
durations 15.9333, 12.4375, 9.95 and 1.9667 are the four scene lengths.

## The camera variants are chosen by a draw (static)

`gi_scene1`'s sequence `camera_start_script` holds an `IF` (type `0x11f`,
opcode 31) whose payload is `{ start 0, condition 1, 0, threshold 0.2, 0 }`,
then the `0x10c` for track 0, then an `ELSE` that calls the sequence `gicam1b`,
which has the same shape for track 1 and calls `gicam1c`, which calls
`gicam1d` (track 3 unconditionally). `gi_scene2` has the same chain over tracks
4–6 with threshold 0.3 (`0x3e99999a`).

The condition word's bit 0 is the engine's *random* test: the `IF` executor
(`0x4ec080`, slot `[0x727e5c]`) takes the next entry of a 200-entry float table at
`0x9fce20` (cursor `[0x72836c]`, advanced modulo 200), compares it with the event's
`+0x14` float (the threshold), and is true when the entry is **less than or equal**
to it. The table is filled at start-up by a call through a function pointer
multiplied by `1/32768` (`0x4ee41a`..`0x4ee43b`, constant `0x38000100`), i.e. a
uniform draw in `[0, 1)`. So the original picks the long camera path at random:
for scene 1 `gi_cam1` 20 %, `gi_cam1b` 16 %, `gi_cam1c` 12.8 %, `gi_cam1d`
51.2 %; for scene 2 `gi_cam2` 30 %, `gi_cam2b` 21 %, `gi_cam2c` 49 %, assuming each
`IF` consumes one draw. **Not examined: whether the generator is seeded
(then the choice is a function of the seed) and how the player's plane
(`check_balmoral` / `check_warhawk`, condition word `0x2000`) changes the scene.**

## What is still unknown, what it affects, who resolves it

| unknown | affects | notes |
| --- | --- | --- |
| the frame of the positions: `camera1`'s parent node in `gamez.zbd` (event flag bit 0 is 0, so the track is the node's own position, not a base plus offset) | where the camera sits relative to the Pandora in the world | the intro's `player`, `piratefighter` and `camera1` tracks are all of the order 0–800, so one shared frame is plausible, not measured. Resolving: **#1665-prereq** (filed below) |
| composition order of the rotation polynomial with the segment quaternion, and the Euler convention `0x53df30` produces | camera orientation inside a segment (the segment's start quaternion is exact; for `gi_cam1` the polynomial terms are < 1e-3) | read the five helpers above in full |
| how `gi_scene1`, `gi_scene2`, `gi_1stperson`, `gi_playerdrop` follow one another: `gi_scene1` and `gi_scene2` are called from `generic_intro`'s `start_script`, `gi_1stperson` and `gi_playerdrop` from `check_warhawk` and `drop_planes`, every call with `START_TIME` 0, and stopped at 15.9, 12.4, 9.9 and 1.95; `gi_scene2` opens the Pandora's launch doors at 5.0 s; the nominal sum of the four lengths is 40.3 s | the total length and cut points of the cinematic | the compiled streams do not state a chain; needs the activation/time rules of the instance scheduler, or the owner's capture timing |
| the camera's field of view | the projection during the cinematic | no track has a field-of-view channel; `CAMERA_STATE` (`0x114`) occurs in no camera record (F40-D3) |
| the time unit and the tick rate of the scheduler that drives `u` | duration in real time | consistent with seconds, not proved |
| `type`, the second `ptr`/`mode` words and the position part's word 3 | nothing the camera needs | `mode` is 1 for every intro track |
| the 15 `mode == 0` blocks | none of the launch cinematic | linear parts, only in mission carriers |
| whether the random generator behind the variant choice is seeded | which camera path the player sees | |
| the letterbox object (`letterbox` / `letterbox.zrd`) | bar geometry (the owner's task #1664 measures it from the capture) | not touched here |
| the Flight Check screen | not part of this task | #1661 |

## Superseded statement

`2026-10-13-f40-d3-cam-anim-camera-format.md` says "no measured field carries a
camera pose keyframe" and "the camera's per-tick pose … comes from data this
task does not measure". The first is true of the record walk and false of the
file: the tail holds them. The second is now measured. The F40-D3 inventory
(which records are camera cinematics, the 59 event types, the TLV grammar) stands.
`cs_app::cinematics::camera_anim::declared_camera_sequence` still declares the
cinematic with explicit unknowns; #1665 is the task that replaces those with
what is measured here.

## Reproduction

```sh
CS_GAME_DIR=<original installation> \
  cargo test --locked -p cs_formats -- accept_parity_m01_lc_measure_cam_ --include-ignored
```

The synthetic tests author every byte (layout, flag-driven part sizes, the cubic
evaluator, seven fail-closed refusals); the two retail tests re-measure the 31
tails, the 1 090 blocks, the 76 845 segments, the member-row join, the unit
quaternions, the 14 intro tracks and their equality across the eight camera
carriers.

## Files

- `crates/cs_formats/src/zbd/anim_tracks.rs` (new): `read_animation_tracks`,
  `AnimationTracks`, `TrackBlock`, `TrackSegment`, `TrackPart`, `TrackError`.
- `crates/cs_formats/src/zbd/mod.rs`: `pub mod anim_tracks;` (wiring only).
- `crates/cs_formats/tests/zbd/t1662.rs` and `tests/zbd/mod.rs` (`mod t1662;`).
