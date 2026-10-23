# BUG-M01-SEA-EDGE-FLICKER (#2000): the horizon plane tied the water tiles in the depth buffer

Date: 2026-10-24. Implementer: `claude-1` (Sonnet 5.5); no independent review yet.
Nothing here is `verified_original`; the original executable did not run.

## Isolated (retail M01, windowed GPU, `retail,gpu`)

The owner-command smoke's sea probe (`sea_visible` = plane + world water tiles,
`sea_absent` = tiles only, same paused pose) was repeated with the player
placed above the shore at 1 500 m and 4 000 m (a throwaway local placement in
`smoke.rs`, not committed). In a 700x400 crop of pure tile-covered sea (no plane
edge, no terrain) the two frames must be identical, because the plane lies
behind the tiles:

| camera | differing pixels (fuzz 1%) |
| --- | --- |
| 1 500 m, before | 124 of 320 000 |
| 4 000 m, before | 324 of 280 000, scattered in bands along tile rows |
| 4 000 m, after | **0** |

The scattered one-pixel speckle, banded along tile rows, with the plane's
colour winning individual pixels, is depth-test ties between two coplanar
surfaces: the plane (mean corner shade, #1730) against the tiles' own stored
corner colours. The speckle moves with every camera move: the flicker.

Cause: the plane sat at the tiles' altitude and relied on
`StandardMaterial::depth_bias = -4`, which `playtest_textures.rs` already
records as inert on this path (wgpu's constant bias on a `Depth32Float` buffer).
So nothing separated the two surfaces except interpolation rounding. Not
colour, alpha, UV, fog or winding: those are static and were not touched.

## Changed

The bias is removed. The plane hangs below the tiles by
`max(0.25 m, 2e-4 x camera height above the level)` (`HorizonSpec::sea_drop_m`,
applied in `place_around_camera`). A view ray reaches a surface `h` lower
`h / height` further whatever the distance, so the relative depth margin is a
constant 2e-4 (about 1 700 steps of a 2^-23 buffer) at the shore and at the
edge of the world, and the plane loses every contact with the tiles. Through a
gap in the tiles the plane is at most centimetres to a few metres lower, below a
pixel. Designed numbers (not measured from the original); the tile geometry,
art and colours are unchanged.

Tests: `accept_bug_m01_sea_edge_flicker_*` (`render::horizon::tests`) assert the
margin in depth steps over camera heights 0.5 m to 12 km, above and below the
level; `accept_m01_play_sky_horizon` pins the plane's placed altitude.

## Not done / unknown

* The speckle is shown in static paired frames at two altitudes; a moving-camera
  video was not recorded (the speckle is a per-pixel tie, so its removal in
  static frames removes it under motion).
* No comparison with the owner's original reference frames (not available here).
* The coast tiles' differing corner colours (faint steps, #1730) and the unplayed
  16-frame `wtr` cycle (#1981) are unchanged and unrelated to the tie.
