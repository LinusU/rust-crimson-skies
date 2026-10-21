# PARITY4-LENS-FLARE (#1756): what the engine image says about the lens flare

Provenance for every row: `static` (byte inspection of the owner-supplied decrypted
image `crimson.decrypted.exe`, virtual addresses below; no original run). Nothing
here is `verified_original`. No disassembly is committed; the numbers are facts.

## 1. The flare art is declared by the interpreter script

`ZBD/interp.zbd` (the campaign setup script, repeated per campaign) carries, in
this order, four commands before `LightMapTexture lightmap`:

| command | argument 0 | argument 1 |
| --- | --- | --- |
| `LensFlareTexture` | `0` | `lflare1` |
| `LensFlareTexture` | `1` | `lflare2` |
| `LensFlareTexture` | `2` (stored as the string `"2"` in the script's value list; the digits read `0`, `1`, `3` in the C-string view only because the list interleaves) | `lflare3` |
| `LensFlareTexture` | `3` | `lflare4` |

The four textures are stored in the `C4` archive (`ZBD/C4/gamez.zbd`,
`rtexture*.zbd`) as `lflare1` .. `lflare4`.

The command is dispatched at `0x5b9aa0` (command name `LensFlareTexture`,
`0x63ee1c`, 16 characters). Its body loads the texture by name (`0x531800`) and
calls `0x57c520(index, texture)`, which stores it in the four-slot table
`0x9ad27c .. 0x9ad288` (index outside 0..3 is ignored) and sets the
"flares enabled" word `0x9ad278` to 1 only when **all four** slots are non-null
(else 0).

## 2. The draw rule (`0x57ca80`)

`0x57ca80(screen_pos*, fade)` draws four screen-space quads. With `P` the light's
projected screen position, `C` the viewport centre
`((R+L)/2, (B+T)/2)` (viewport words `0x9fddcc` L, `0x9fddd0` T, `0x9fddf4` R,
`0x9fddf8` B), `W = R - L` and `f` the fade in 0..1:

| quad | texture slot | centre | half-extent |
| --- | --- | --- | --- |
| 1 | `lflare1` (`0x9ad27c`) | `P` | `W * f * 0.0625` |
| 2 | `lflare2` (`0x9ad280`) | `C + 0.5 * (P - C)` | `W * f * 0.0625` |
| 3 | `lflare3` (`0x9ad284`) | `C + 0.1 * (P - C)` | `W * f * 0.0625 * 0.5` |
| 4 | `lflare4` (`0x9ad288`) | `C - (P - C)` (mirror through the centre) | `W * f * 0.0625 * 1.5` |

Constants: `0.0625` at `0x6093d4`, `0.5` at `0x6032e0`, `0.1` at `0x6034a8`, `1.5`
at `0x603460`. Each quad goes through the sprite routine `0x57c5c0(centre*, half,
texture, 0.5, 0)`; the routine clips the quad against the viewport and submits
four vertices through the renderer's draw pointer (`0x9be78c`, or `0x9be790` when
the last argument is non-zero). Its fourth argument `0.5` and the blend state are
**not** decoded: the blend (additive or alpha) is unknown.

This is the "glow at the sun, a ring half way, a small ring near the centre, a
big ring mirrored" layout of the owner's reference frame (sun upper left, rings
along the line to the lower right).

## 3. Fade and selection (`0x57cc00`, `0x57cc80`)

Per queued flare: `t = 1.0 / rec[+0x14]` (`0x9fd5c0` instead of `1.0` when the
mode word `0x9be708` is non-zero; `rec[+0x14]` is the projected depth word of the
record). With the light definition's two floats `min = def[+0x14]`,
`max = def[+0x18]`: `t >= max` draws nothing; `t < min` gives `f = 1`; between,
`f = (max - t) / (max - min)`. The queue (`0xa0c684` count, `0xa0c688` entries,
cap `0x40`) is flushed by `0x57cc80`, which then resets the count.

The light record array (`0xa07540` count, cap `0x28a`, 32-byte records at
`0xa07544`: +0 world position, +0xc projected position (z multiplied by
`0xa07144` and offset by `0xa07140` when the mode word is zero), +0x18 intensity,
+0x1c light object whose `+0xc` must be non-zero) is filled by `0x57c230`, called
from four vertex-projection loops (`0x552815`, `0x5539e8`, `0x558d41`,
`0x558f65`), each of which drops a vertex behind the near plane (`0x9fddd4`) or
outside the viewport. **No depth-buffer or terrain/cloud occlusion test was found
on this path**: visibility is the screen-bounds and near-plane test plus the
distance fade.

The light-definition text keywords parsed at `0x4c2342`..`0x4c24ba` are `fade`
(`0x62bb6c`, two floats, defaults 1000 and 2000), `lens_flare` (`0x62bb74`, three
floats, defaults 30, 1000, 2000, flag `+0x3c` set) and the bulb kinds `flash`,
`strobe`, `solid`, `grid`, `line`, `point` (`0x62bb7c` ..).

## 4. Unknown, with the affected content

* **What makes the sun a flare source in M01.** The M01 world holds a `sun` node
  (`Sun.tif` quad, 4 vertices, node translation (1462.7, 1420.7, -1471.4) under the
  horizon, see `2026-10-18-parity-m01-atmo-a-weather-units-and-sky-consumers.md`)
  and a tag-9 `sunlight` light node, and no retail file contains the text
  `lens_flare`. Which of the four projection loops carries the sun, and from which
  node type, is not traced. Affects: the sun flare in M01 (#1756 stages B..D).
* The blend mode of the four quads and the meaning of the sprite routine's
  fourth argument (`0.5`).
* The mode word `0x9be708` (which of two projection paths, `0x53d8d0` vs
  `0x53d880`, is used).
* Whether the original's sun disc (`sun`, `Sun.tif`) is drawn by the horizon pass
  in our renderer: the owner's capture shows a bright disc with halo, ours none.
