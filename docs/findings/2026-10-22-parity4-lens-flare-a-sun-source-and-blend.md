# PARITY4-LENS-FLARE-A (#1858): what makes the sun a flare source in M01, and how the flare is blended

Date: 2026-10-22. Implementer: `claude-2` (Sonnet 5.5); no independent review yet.
Static work only: the owner-supplied decrypted image `crimson.decrypted.exe` (virtual
addresses below) and the installation's `ZBD/C3/gamez.zbd` read through the production
GameZ readers. The original executable was not run, so every claim is `static` and
nothing is `verified_original`. No disassembly is committed; the numbers are facts.
It extends `2026-10-21-parity4-lens-flare-engine-rule.md` and corrects three of its
statements (section 6).

Tests: `accept_parity4_lens_flare_a_c3_gamez_sun_mesh_light_carries_the_flare_word`
(needs `CS_GAME_DIR`) and `accept_parity4_lens_flare_a_image_bytes_match_the_finding`
(needs `CS_ENGINE_IMAGE`), both `#[ignore]`d and failing loudly without their input, in
`crates/cs_formats/tests/accept_parity4_lens_flare_a.rs`.

## 1. The answer: the sun is a stored mesh light on the `sun` node's mesh

The flare source is **not** the weather `SUNLIGHT_*` zone light (tag-9 `sunlight`
node) and **not** a hard-coded horizon quad. It is a **mesh light record** in the
GameZ mesh data: `ZBD/C3/gamez.zbd` mesh 491, the 4-vertex 1-polygon `Sun.tif` quad
owned by the node `sun` (child of `zone1` under `horizon`), stores one 76-byte light
(`light_count = 1`, the record `cs_formats` reads as `RawMeshLightHeader`). The mesh
data is binary, so the keyword `lens_flare` never occurs as text in any retail file
(the engine's text parser for the same layout, below, is not what retail uses).

Of the 26 stored mesh lights in the archive exactly **three** carry the flare word:

| mesh | owner node | verts / polys | `unk00` kind, `unk04` mode | `flags` | fade (`unk48`, `unk52`) | `unk56` | flare word `unk60` | flare numbers (`unk64`, `unk68`, `unk72`) | bulb position (`extra[0]`, node space) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 491 | `sun` | 4 / 1 | 0, 0 | `0x3df` | 0, 0 | 0.17 | 1 | **5000, 5, 10000** | (6.18489, 18.294209, 0) |
| 128 | `lens_flash` | 0 / 0 | 0, 0 | `0x211` | 0, 0 | 0.17 | 1 | 30, 4000, 6000 | (0, 0, 0) |
| 130 | `lens_flash` | 0 / 0 | 0, 0 | `0x211` | 0, 0 | 0.17 | 1 | 30, 4000, 6000 | (0, 0, 0) |

The `lens_flash` meshes are light-only meshes (no geometry) with the same layout; they
are another flare source (their placement and use are not traced here).

### Layout of the 76-byte light = the engine's `0x4c`-byte light-sprite entry

The mesh-render function `0x5524d0` walks `[block+0x20]` entries of `[block+0x40]`,
stride **`0x4c`**, and that is the stored 76-byte light. Mapping of the reader's raw
fields to entry offsets (the offsets are read from the instruction stream, the field
names are the reader's):

| entry offset | reader field | consumer (VA) |
| --- | --- | --- |
| +0x00 | `unk00` (0/1) | kind switch at `0x552c29` (0: static bulb, 1: frame-cycled) |
| +0x04 | `unk04` | sub-mode, `0x552cb0` |
| +0x08 | `unk08` | frame interval (added to the timer at `+0x10` by `0x9ad744`) |
| +0x0c | `extra_count` | frame count |
| +0x10 / +0x14 | `unk16`, `unk20` | frame timer / frame index (runtime) |
| +0x18 | `unk24` (word) | brightness step: `step * 0xa06fb0 + 1.0` multiplies the projected z (`0x558c5b`) |
| +0x28 | `pad40` (word) | the intensity word handed to the record filler |
| +0x2c | pointer | the `extra` `Vec3` array (bulb positions per frame) |
| +0x30 / +0x34 / +0x38 | `unk48`, `unk52`, `unk56` | bulb fade min / max / scale (`0x558d53`: sprite fade `(max - z) * scale * 255`) |
| **+0x3c** | `unk60` | **lens-flare flag** (`0x558cb7`: `mov eax, [ebx+0x3c]`, skip the record when zero) |
| +0x40 / +0x44 / +0x48 | `unk64`, `unk68`, `unk72` | `lens_flare` three floats; **+0x44 = flare fade min, +0x48 = flare fade max** |

The text parser `0x4c2250` (keyword `pointlight` at `0x62c028`, then `fade` at `0x62bb6c`
into `+0x30/+0x34`, `lens_flare` at `0x62bb74` into `+0x3c` and `+0x40/+0x44/+0x48`, defaults
30, 1000, 2000) fills the same entry, which fixes the meaning of `+0x30..+0x48`.
`0x4c2610` (the node-text parser that calls it) is a build-style path; retail meshes
store the numbers in binary. The flare quad size word (`+0x40`, 5000 for the sun) is
**not read** by any instruction on the flare path found (`0x57cc00` reads only
`light+0x14` / `+0x18`, `0x57ca80` sizes from the viewport); its use is unknown.

### Which projection loop

The four loops that call the record filler `0x57c230(world*, proj*, intensity, light*)`:

| site | in function | `light*` argument | can feed a flare |
| --- | --- | --- | --- |
| `0x552815` | `0x5524d0`, geometry type 2 (vertex sprites) | `push 0` (`0x55280a`) | no |
| `0x5539e8` | `0x553700` | `push 0` (`0x5539dd`) | no |
| **`0x558d41`** | `0x558b30`, mode word non-zero path | `entry + 0x30` (`add edx, 0x30`, `0x558d37`) | **yes** |
| **`0x558f65`** | `0x558b30`, mode word zero path | `entry + 0x30` (`0x558f53`) | **yes** |

`0x558b30(position*, intensity word, entry*)` is called from the light loop of
`0x5524d0` (`0x552c83`, `0x552d02`, geometry types 0 and 1: the sun's mesh has
`unk04 = 1`, the `lens_flash` meshes `unk04 = 0`; the loop at `0x552ab2` is reached
from both) and from three more call sites in the function around `0x554600` (`0x55467d`, `0x5546b1`, `0x554711`). The record queue
builder `0x57c490` only keeps a record whose light object is non-null **and whose word
at `light+0xc` (= `entry+0x3c`) is non-zero** and whose projected depth `rec[+0x14] > 0`
(cap `0x40`, `0xa0c684`/`0xa0c688`). With `light = entry + 0x30` the flare gate is the
stored `unk60` and the fade pair is `unk68`/`unk72`.

For the sun: `t = 1 / depth` against min 5 and max 10000. Projected depth is at least
the near clip (M01 `CLIP_RANGES` near 5, floor 6), so `t <= 0.2 < 5` and **the fade is
always 1** (`0x57cc45` path); the flare never fades with distance in M01. The `lens_flash`
pair fades from full at `t < 4000` to nothing at `t >= 6000`, i.e. practically never
either (the numbers would only matter for depths below 1/4000).

## 2. Visibility: a terrain ray test gates every flare

The earlier note's "no occlusion test was found" is wrong. The queue is drawn by
`0x4d6a10` (`0x57c490` build, then per queued record a **ray cast `0x4c8f70`** from the
camera position (view struct `+0x2c..+0x34`) to the record's `+0` position, with the
collision mask word `0x71e8b4` set to `0x40000` and the hit-recording word `0x71e8a8` set
to 1 around it). `0x4c8f70` is a cell-grid walk over the world's terrain cell array
(`world+0x38`, cell size `+0x84/+0x88`). Its return value is `1` when the hit byte
stayed zero (no hit) and `0` otherwise (`0x4c9727..0x4c9730`); `0x4d6a89..0x4d6a91`
draws the flare when the return is non-zero and skips it otherwise. So the flare is drawn
only when the segment from the camera to the light meets no hit under mask `0x40000`.
What objects carry that mask bit (terrain only, or also buildings or clouds) is **not
determined**; the sun at about 2.5 km elevation 34 degrees is above any terrain, so
for M01 the practical gate is the screen-bounds and near-plane tests below.

The record filler's callers drop the vertex when its depth is not beyond the near plane
(`0x9fddd4`) or when its projected x/y lies outside the viewport words
(`0x9fddec/f0/f4/f8`) (`0x558be8..0x558c52`).

## 3. The mode word `0x9be708`

* It is set by `0x59ded0` (stores to `0xa10300` and `0x9be708`), reached through
  `0x5a8c00(mode, backend)`; callers pass `0` (`0x59e8fc`), `1` (`0x59f7f4`) or the
  caller's own argument (`0x5a888c`, `0x5a8a10`). Comparisons with `2` exist at 20+
  sites (`0x44b2cd`, `0x4cc610`, `0x531c65`, `0x55b054`, ...).
* **The flare is drawn only when it is non-zero**: both callers of the flare draw
  `0x4d6a10` (`0x42e708` after the test at `0x42e6d0`, and `0x49efb9` after the test at
  `0x49ee6d`) sit on the non-zero branch; the zero branch (`0x42e720`, `0x49f033`) never
  reaches `0x57cc00` (single caller `0x4d6a94`). `0x57cc80` (flush-all variant) has no caller.
* The same word picks the **software** weather zone array (`0x71dd24`, the `SW_ZONE*`
  records, sun off in M01) over the hardware one (`0x71de2c`) at `0x472eb8`. So non-zero
  matches the `SW*` family. Which renderer the owner's reference capture ran in is not
  stated by the image; the capture showing flare rings is consistent with the non-zero
  branch. **Unknown:** the names of the mode values 0/1/2.
* In the non-zero branch the record projection uses `0x53d8d0`, in the zero branch
  `0x53d880`, and the depth numerator is `0x9fd5c0` instead of `1.0`; `0x9fd5c0` is a
  runtime word (not constant data), so the exact numerator in that mode is unknown. In
  the sun's case the result is `f = 1` regardless (section 1).

## 4. Blend state and the meaning of `0.5`

Each flare quad is `0x57c5c0(centre*, half_extent, texture, 0.5, 0)` (arguments as in the
earlier note). The routine returns without drawing when `half_extent < 1.0` pixel,
clips the quad to the viewport (words `0x9fddcc/d0/e4/e8`), writes four vertices at
`0x9be7f4` (z = the centre's third word, which `0x57ca80` sets to **0.99**,
`0x3f7d70a4`, in the non-zero mode, 10.0 in zero mode) and calls the draw pointer
**`0x9be78c(4, texture, 0.5, 0)`** (`0x9be790` when the last argument is non-zero). The
pointers are installed in one place, `0x5a8df8`/`0x5a8e02`, as `0x5a4580` / `0x5a4b70`,
Direct3D (device vtable `0xa17120`: `SetRenderState +0x58`, `SetTextureStageState +0xa0`,
`DrawPrimitive +0x70`) draw functions.

* **The fourth argument `0.5` is the quad's opacity.** `0x5a4580` takes it as its third
  parameter (a float): `>= 1.0` goes to the opaque path (`0x5a45ac` compares with
  `1.0`, `0x6032dc`), anything below goes to `0x5a4882`. With `0.5` and the flag word
  `0`, the quad is stored (`0x5a49cf..0x5a4b61`) in the **depth-sorted translucent list**
  (`0xa11168` count, cap `0x1770`; vertices `0xa1116c`, cap `0x5dc0`; primitive type
  `0x9be7d4 = 6`, a triangle fan) with every vertex's diffuse
  `(int(0.5 * 255) << 24) | 0xffffff` = `0x7fffffff` (alpha 127) and specular `0xff000000`.
  The sort key carries z (0.99) and the texture's blend bit (below).
* **Blend factors** are set per material, not by the flare code. The device defaults
  are `SRCBLEND = 5` (source alpha) and `DESTBLEND = 6` (inverse source alpha)
  (`0x5a13fd`, `0x5a142f`). The translucent draw (`0x5a4380..0x5a43d7`) replaces them from
  the texture descriptor's flag word (`[[tex]+0xc]`): bit `0x4` set gives
  **`SRCBLEND = 2`, `DESTBLEND = 2` (additive: one, one)**, clear gives source
  alpha / inverse source alpha. Alpha blending is enabled (`0x1b`) and z-write is
  disabled (`0xe`) for these draws.
* **Not determined:** whether the four `lflare` texture descriptors carry bit `0x4`
  (the flag is filled by the texture loader chain `0x531800 -> 0x5318a0 -> 0x531f30`,
  whose first word's `+0xc` field was not traced). Until that is traced the flare is
  either additive or ordinary alpha at 50 percent; the owner's frame (bright rings
  and a glow) is consistent with additive but does not prove it.

## 5. What the renderer task needs (for #1859 / #1756)

* Source: read the mesh lights (`RawMeshLight`) of the mission's GameZ meshes and
  treat a light with `header.unk60 != 0` as a flare source at `extra[frame]` (node
  space) of the node that owns the mesh; for M01 that is the `sun` node (translation
  (1462.7, 1420.7, -1471.4) under `zone1`), fade pair (5, 10000) = always 1, and the
  two `lens_flash` nodes.
* Draw order, positions and sizes: as the earlier note section 2 (four quads
  `lflare1..4`, half-extent `W * f * 0.0625` etc., viewport-clipped, opacity 0.5,
  z 0.99, depth-sorted among translucent draws).
* Gate: only in the engine's non-zero mode-word configuration; screen bounds and near
  plane; terrain ray cast as in section 2.
* The sun disc itself is the `Sun.tif` quad drawn by the horizon pass (mesh 491); the
  flare adds the glow and rings. The disc is not produced by the flare code.

## 6. Corrections to `2026-10-21-parity4-lens-flare-engine-rule.md`

1. "No depth-buffer or terrain/cloud occlusion test was found": there is a terrain
   cell-grid ray cast per queued flare (section 2).
2. The queue `0xa0c688` is not filled by the four loops directly: they fill the record
   array `0xa07544`; the queue is built from it by `0x57c490` and only keeps records with
   a non-null light object whose `+0xc` word is non-zero. Two of the four loops pass a
   null light object and can never produce a flare (section 1).
3. The light definition `def` of that note is the entry's embedded `+0x30` block:
   `def[+0x14]/[+0x18]` are entry `+0x44/+0x48` (the stored `unk68`/`unk72`), the flag is
   entry `+0x3c` (`unk60`), and the values come from the GameZ mesh light, not from text.

## 7. Unknown, with the affected content

* Whether the `lflare1..4` textures are additive (section 4). Affects: the look of the
  flare in #1859.
* The collision mask `0x40000` (which world objects can block a flare).
* The use of the stored size word (`unk64`, 5000 for the sun) and of `unk56` (0.17).
* The names of the mode word's values and the zero-mode numerator `0x9fd5c0`.
* The placement of the two `lens_flash` nodes and what raises them.
