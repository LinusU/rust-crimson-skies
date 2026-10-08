# PLAYTEST-DECAL-ZFIGHT (#794): coplanar decal layers draw 1 cm off their base

Measured with `accept_playtest_decal_*` (retail, GPU). Frames are under
`private/evidence/PLAYTEST-DECAL-ZFIGHT/` and are not committed; the machine
report is `docs/findings/evidence/PLAYTEST-DECAL-ZFIGHT.json`.

## What the flicker is

#753 left one measured residual: the grid view `t4d20z0` (20 m off the `-x`
face, 0.4 of the height up, mid-hull) flipped `0.081` of its pixels at 1 cm
camera steps, where "the hull's skull decal visibly changes". The owner also
saw an insignia flicker on each Bloodhawk wing.

## Attribution

A per-material census (hide each drawn material, count how many of the view's
flips it owns) attributes `10 103` of `10 150` flip pixels in `t4d20z0` to one
material — the emblem layer — and a handful each to rigging shimmer. Through
the production report the layer is:

| flicker | container | mesh | group | material | texture | form |
| --- | --- | --- | --- | --- | --- | --- |
| hull emblem | `zbd/c1c/gamez.zbd` | 750, 755 | 1 | 194 | `fhunter_logo2.tif` | second material group of the hull quads |
| hull emblem | `zbd/c1c/gamez.zbd` | 697, 700, 701, 704, 705, 707 | 1 | 190 | `fhunter_logo4.tif` | second material group of the hull quads |
| wing insignia | `zbd/planes.zbd` | 1368, 1415 | 1 | 95 | `blo_winglogo.tif` | own quads, ~1 mm off the surface |
| wing insignia | `zbd/planes.zbd` | 1414, 1416 | 2 | 95 | `blo_winglogo.tif` | own quads, ~1 mm off the surface |
| tail logo | `zbd/planes.zbd` | 1363 | 8 | 94 | `blo_taillogo.tif` | own quads |
| nose logo | `zbd/planes.zbd` | 1365 | 6 | 89 | `blo_noselogo.tif` | own quads |
| cockpit glass | `zbd/planes.zbd` | 1360, 1365 | 0, 2 | 84 | `bld_gencockpit.tif` | keyed but nothing coplanar behind it |

Both forms are the same draw: the decal's triangles cover its base surface at
depth ~equal, and a 1 cm camera step is enough for float rounding to swap the
winner per pixel.

## What the bytes do and do not mark

* Every one of those decal textures stores an **alpha plane**
  (`alpha_source = Plane`), which `keyed` reads as carried coverage.
* Polygon `unk04` correlates (`2` on most logo polygons) but does not separate
  them (one wing-logo quad per wing stores `1`): recorded as unknown, not a
  flag.
* Polygon flags differ (`0x10` skull, `0x00`/`0x30` aircraft logos): no stored
  marker reliably names a decal.

So the rule is designed, not recovered: **a part bound to a coverage-carrying
texture is a decal layer**, and its vertices move `DECAL_OFFSET_M` = 1 cm
along their normals before upload. Shared through `TextureBinder`, so the area
and the aircraft get the identical rule; `PlaytestConfig::decal_offset = false`
reproduces the coplanar baseline the tests measure against.

## Why a geometric offset and not `StandardMaterial::depth_bias`

Bevy 0.19 lands `depth_bias` in wgpu's `DepthBiasState::constant`. On the
measured path (Metal, `Depth32Float`) wgpu scales that constant by the
smallest representable depth increment — which for a float buffer is ≈ 0 — so
the bias is inert here: setting ±10⁶ on every material changed not one pixel.
The equivalent polygon-offset rule expressed in geometry works on every
backend. 1 cm is ten times the stored ~1 mm decal gap and far sub-pixel at the
distances the decals are viewed from.

## Measurements (lost share of decal interior, 1 cm camera step)

The measure: mask the target layer, diff shown vs hidden to get the decal's
own pixels (its footprint), erode one pixel (coverage-edge shimmer cannot
reach the interior), then count interior pixels that fall back to the base
colour across the step — the z-fight signature. `#753`'s bound is `0.002`.

| view | decal pixels | coplanar (`decal_offset=false`) | offset |
| --- | --- | --- | --- |
| `t4d20z0` skull emblem | 13 744 | **0.717** | **0.000** |
| chase `l_aileron1` | 2 338 | 0.366 | 0.000 |
| chase `r_aileron1` | 2 334 | 0.818 | 0.0005 |

The same skull view on #753's whole-region metric: `0.081` coplanar vs
`0.0009` with the offset — the emblem owned ~99.5% of that view's
flips. `leftwing`/`rightwing` insignia views draw fine and did not fight in
baseline here; they are measured too and stay at 0. #753's three flank views
were re-run and still pass.

Not measured / open: the original engine's own decal rule (draw order, stored
flag, or its own offset) is still unknown; two *keyed* layers coplanar with
each other would both take the offset and could still tie — none was measured
to; `unk04`'s meaning stays unknown.
