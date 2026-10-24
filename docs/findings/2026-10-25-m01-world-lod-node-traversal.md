# M01 world: the per-frame LOD range test and the camera LOD multiplier (#2058)

Task #2058, follow-up of `2026-10-24-m01-world-multiplayer-root-gating.md` (#1815,
"Per-frame LOD pick"). Capability used: `retail` (static reading of `$CS_ENGINE_IMAGE`,
image sha256 as in `data/owner-static/engine-facts.toml`, plus plain-text searches of
the read-only `$CS_GAME_DIR/ZBD` tree). Provenance of every row is `static`; no original
run happened and nothing is `verified_original`. No disassembly is committed:
addresses are virtual addresses in the image (file offset = VA - 0x400000), and the
byte strings are short cited excerpts that let a reader re-find the site.

Note on #1815's addresses: `0x5b8547` and `0x5b8688` in that finding are the *operand
bytes of a string push* inside the handlers, not entry points. The handlers that
compare the names `CameraSetDynamicLOD` and `CameraSetLODMultiplier` contain them at
0x5b8546 and 0x5b8687; the string pushed is at 0x63f558 / 0x63f508.

## Result in one paragraph

LOD is **not a separate per-frame walk over LOD nodes with children chosen by the
parent**. A LOD node is an ordinary scene node of kind 6 that carries its own squared
distance band `[min², max²)`; the render dispatch visits each node and the kind-6
handler decides, for that node alone, whether it is drawn. The camera multiplier enters
as one multiplication of the camera-to-node squared distance by
`inverse_lod_multiplier_squared` (camera +0xd4) before both comparisons. The band is
**inclusive below, exclusive above**. With a multiplier of 1 the test is exactly the
plain `min <= distance < max` the scenery already models; a multiplier `L` scales both
bounds by `L` (distance / L is what is compared). `CameraSetDynamicLOD` is off unless a
script calls it, and no readable retail script does.

## Rows

| # | Claim | VA (image) | Evidence |
|---|---|---|---|
| 1 | Scene-node dispatch: a visit routine switches on the node kind `[node+0x34]` (1..11) through the jump table at 0x4d4b0c; kind **6** jumps to the call of the LOD range handler `0x4d4170` at 0x4d4a78. | 0x4d4a58 (`ff2485 0c4b4d00`), 0x4d4a78 | table entry 6 = 0x4d4a72; entry 1 = 0x4d4aa2, 5 = 0x4d4a5f, 9 = 0x4d4a82, 10 = 0x4d4a92 are other handlers |
| 2 | Kind 6 is the LOD node: its constructor `0x4e05e0` stores kind 6 at `[node+0x34]` (0x4e05ec) and allocates a 0x5c-byte block at `[node+0x38]`. | 0x4e05e8 (`6a5c 6a01 c7463406000000`) | the only store of kind 6 into a node found by a linear sweep of the code section |
| 3 | LOD block layout recovered: `+0x00` = 1 (constructor), `+0x04` = **min²**, `+0x08` = 1000.0f (0x447a0000), `+0x0c` = **max²**, default 1.0e6f (0x49742400 = 1000²). So an unconfigured LOD node covers `[0, 1000²)`. | 0x4e0601..0x4e0611 | `c70001000000 894804 c740080000 7a44 c7400c00247449` |
| 4 | `LODSetRange a b` stores `a*a` at `[block+4]` and `b*b` at `[block+0xc]`: the script arguments are *distances*, the block holds squares. | 0x5ba540..0x5ba565 (handler for the name at 0x63ec3c) | `d9442448 d84c2448 ... d95904 ... d958 0c` |
| 5 | `LODAddChild` (name at 0x63ec48, check at 0x5ba4d2) only parents a node; the handler touches no range. | 0x5ba4d2 | |
| 6 | **Multiplier consumer.** In the kind-6 handler the camera-to-node squared distance (camera position `[cam+0x2c]` minus node world position `[node+0x60]`) is cached in a 16-byte per-slot table at 0x727964 and **multiplied by `[camera+0xd4]`** (camera data pointer in the global 0x71ebd8) before it is compared. This is the only instruction in the image that multiplies by camera +0xd4. | 0x4d42f3 (`8b15d8eb7100 d910 d88ad4000000`), 0x4d42fb | |
| 7 | **Inclusivity.** Scaled d² is out of range when `d² < [block+4]` (0x4d4303 `fcom [esi+4]`, then `test ah,1; jne` to the reject at 0x4d4319) and in range when `d² < [block+0xc]` (0x4d430d `fcomp [esi+0xc]`, `jne` to the accept at 0x4d4335); anything else rejects. Range is therefore **[min², max²)**: lower bound inclusive, upper bound exclusive. A node rejected here returns 0 from the handler (no draw, no children). | 0x4d4303..0x4d4317 | `d85604 dfe0 f6c401 750c d85e0c dfe0 f6c401 751e` |
| 8 | The same inclusive/exclusive pair guards a cached path: when `[block+0]` is 0 the handler reuses the already-scaled distance of the current slot (global slot index 0x62d1a0) and applies the identical two comparisons against `+4` / `+0xc`. The constructor's `+0 = 1` means ordinary LOD nodes always take the computing path. | 0x4d41c0..0x4d41f9 | `d980647972 00 8d90... d85e04 ... d85e0c` |
| 9 | After the range test the handler computes fade factors (alpha-style ramps) from further block fields (`+0x10..+0x30`, `+0x40..+0x4c`) using a square-root approximation `(bits >> 1) + 0x1fc00000`. Their meaning is **not recovered**. | 0x4d4335..0x4d4522 | see Unknown |
| 10 | **Camera setter.** `0x4d2e30(camera, L)` requires a camera node (`[node+0x34] == 1`, else error 3), stores `L` raw at `[cam+0xd0]` and `1.0 / (L*L)` at `[cam+0xd4]`. `CameraSetLODMultiplier` (0x5b8688 region) calls it; so does the `CAMERA_STATE` apply path (0x4e8664, when the state's flag bit 4 is set, value at `[state+0x1c]`), a camera re-apply (0x4d80b4, value `[cam+0xd0]`) and a per-frame interpolator (0x42b616). `inverse_lod_multiplier_squared` is therefore `1/L²` (effect: row 11). | 0x4d2e9a (`d9442408 8b442408 d84c2408 8981d0000000 33c0 d83ddc326000 d999d4000000`) | 0x6032dc holds 1.0f |
| 11 | Effect of `L`: the test is `d²/L² in [min², max²)`, i.e. `d in [L*min, L*max)`. `L > 1` makes LODs hold longer (higher detail further out), `L < 1` switches earlier. | 0x4d42f3 + 0x4d2e9a | derived from rows 6 and 10 |
| 12 | **CameraSetDynamicLOD** (`0x5b8546`, name at 0x63f558) calls `0x4d2140(1, v)`, which stores `v`'s reciprocal as a **frame-time threshold** at 0x62d188 (`v == 0` stores 0.01) and sets the **enable flag** 0x62d184. It takes no camera: the state is global. | 0x4d2140 (`d9442408 d81dc8326000 dfe0 f6c440 7414`) | callers of 0x4d2140: only 0x5b8568 |
| 13 | **Dynamic LOD loop.** Three sibling per-frame render routines (updates at 0x4d6470..0x4d64f1, 0x4d670e..0x4d6792, 0x4d6b1e..0x4d6b9f) each, when 0x62d184 is non-zero, compare the frame delta time `[0x9ad744]` with the threshold: `dt <= threshold` raises a global multiplier at 0x62d18c by the step at 0x62d190, otherwise lowers it by the step; the result is clamped to `[floor, 1.0]` with the floor at 0x62d194; then `0x4d2e30(camera, multiplier)` is called with it. Constants stored in the image: multiplier starts **1.0**, step **0.05**, floor **0.6**. So dynamic LOD can only *shrink* bands, down to 0.6 times, and only when frames are slower than the target. | 0x4d647f, 0x4d6498, 0x62d18c (`0000803f cdcc4c3d 9a99193f`) | `d90544d79a00 d81d88d16200 ... d82590d16200 eb06 d80590d16200` |
| 14 | The enable flag 0x62d184 is **zero in the image** and has no writer except `0x4d2140`; so dynamic LOD never runs unless a script executes `CameraSetDynamicLOD`. | 0x62d184 | writers: 0x4d215f, 0x4d2173 only |
| 15 | **Camera creation does not set +0xd0/+0xd4.** The camera constructor `0x4d2180` allocates 0x1e8 bytes through the function pointer at 0xa201b4 (called with 1, 0x1e8) and writes only +0x14..+0x28, +0xb0, +0xb4, +0xd8, +0xdc, +0xf8, +0x138, +0x184, +0x1e0; neither +0xd0 nor +0xd4 is written. Its two callers are the script command that creates a camera (0x5baacd) and a clone path (0x4d7fd3); neither calls 0x4d2e30. | 0x4d21b0..0x4d221b | `57bf01000000 68e8010000 57 897e34` ... |

## Retail data side

* Plain-text search of every `.zbd` under `$CS_GAME_DIR/ZBD` finds **no** occurrence of
  `CameraSetDynamicLOD`, `CameraSetLODMultiplier`, nor `LODMultiplier`. The INTERP
  container `ZBD/interp.zbd` does spell its camera commands in plain text
  (`CameraSetHorizon` occurs there), so a loading script calling the two LOD commands
  would be visible; it does not. Dynamic LOD is therefore **not enabled by any
  readable retail loading script** (row 14 then fixes it off).
* The animation keyword `LOD_MULTIPLIER` (a field of `CAMERA_STATE`, next to
  `NEAR_CLIP`) occurs exactly **once** in the shared `ZBD/zrdr.zbd` (offset 1849478),
  inside the `gi_scene*` intro-scene animation group that drives `camera1` with the
  `gi_cam*.zan` splines. It occurs nowhere in `ZBD/C3/M01/zrdr.zbd`,
  `ZBD/C3/M01/mis_anim.zbd`, `ZBD/C3/zrdr.zbd` or `ZBD/C3/cam_anim.zbd`
  (`C3/M01/zrdr.zbd` has no `CAMERA_STATE` at all, only `camera1` spline scripts).
  `ZBD/C1/M02/zrdr.zbd` and `ZBD/C1/M04/zrdr.zbd` hold the same shared text.

## What this changes for the scenery

1. The scenery's single band chosen "at the player's distance" is the wrong shape: each
   LOD node has its own `[min, max)` band tested **per node against the camera**, with
   the lower bound inclusive and the upper bound exclusive, and a node outside its band
   draws nothing (children included). An LOD group is several sibling LOD nodes with
   disjoint bands, not one node choosing a child.
2. With `L = 1` the plain distance comparison is exact. Slot 1969's gun turrets behind a
   0-500 m band are visible only while the camera is within 500 m of them (500 m
   itself excluded), measured from the **camera** position, not the player's.
3. `L` scales both bounds. If the camera's `+0xd4` were left at zero (row 15), every
   scaled distance would be 0 and only bands containing 0 (those with `min = 0`)
   would pass: see Unknown.

## Still unknown (rule 4)

* **The camera's initial `lod_multiplier`.** Row 15 shows the constructor never writes
  +0xd0/+0xd4; whether the 0xa201b4 allocator returns zeroed memory, and whether some
  later initialisation (the `CAMERA_STATE` apply, row 10) runs for M01's gameplay
  camera, was not recovered. The pointer 0xa201b4 is a runtime-filled function-pointer
  slot with no writer in the code section. Affected content: every LOD group in M01
  (and any mission), in particular the gun-turret band of slot 1969. Only a retail run
  (or recovering the allocator's identity) settles it; until then the scenery keeping
  `L = 1` is a **designed** choice, not a measured one.
* **M01's multiplier from data.** No M01-specific file names `LOD_MULTIPLIER` (above);
  whether M01's flow reaches the shared `gi_scene*` group (which does) is not
  recovered. Affected content: the intro/cinematic camera of M01 only.
* The meaning of the fade fields (row 9): whether a LOD node fades alpha in a border
  zone near each bound, and over what distances. Affected content: pop-in at band edges.
* The object options `ObjectLOD_SW` / `ObjectLOD_HW` and how they alter the stored
  bands were not traced.
* The `[block+0] = 0` shared-distance case (row 8) is not produced by anything recovered
  here; whether any world data creates it is unknown.
