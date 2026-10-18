# F17-F.3: back-face elimination edge cases (zero normal, edge-on tolerance)

Date: 2026-10-18. Task: #1408 (`F17-F.3`), follow-up of F10-D.2 (#396;
`2026-10-14-f10-d-2-front-face-winding-and-culling.md`). Claim class: static
code evidence `OWNER-STATIC-2026-10-05`, at most **checked**, never
`verified_original`. This document commits no executable bytes.

## The original's rule (from the F10-D.2 finding)

In camera space, `N = (P1−P0)×(P2−P0)` of the first three stored corners and
`d = N·P0` (2·area·distance, so view-dependent). With `tol = 0.00005`:
`d < −tol` front; `d > tol` back, drawn only if polygon bit `0x400`;
otherwise eliminated — including `|d| ≤ tol` for two-sided polygons, and a
polygon whose normal is exactly zero is never drawn.

## Decision

| Case | Replicated? | Where |
| --- | --- | --- |
| one-sided back face (`d > tol`) | yes | GPU `cull_mode: Back` (F10-D.2, unchanged) |
| two-sided back face | yes | GPU, no cull (unchanged) |
| edge-on band `\|d\| ≤ tol` | **no, not in the GPU pipeline** | classified and counted by `render::elimination::classify` |
| exactly-zero first-three-corner normal | **no, not removed** | classified and counted |

Reasons:

* F17's non-negotiable is that culling must not remove gameplay geometry; the
  band is a per-frame, per-polygon decision that would need a CPU pre-pass over
  every polygon each frame, against polygons that the F10-B reader has already
  triangulated (the original fans from corner 0 and never triangulates).
* A polygon eliminated here is, by construction, edge-on or degenerate on its
  first three corners, so its rasterised area from that triangle is ~0; the GPU
  difference is bounded to later fan triangles, which is **unmeasured**.
* Nothing is silent: `EliminationReport` counts `front`, `back_drawn`,
  `back_culled`, `edge_on` and `zero_normal`; `eliminated()` and
  `unreplicated()` (edge-on + zero-normal) state how many polygons the original
  would not have drawn that the GPU still draws.

`BFE_TOLERANCE = 0.00005` is the effective `SetBFETolerance` value; the engine
default `0.005` is overridden on every world load.

## Still unknown

* Visual effect of the unreplicated band on retail content (needs per-polygon
  camera-space data and a runtime consumer; none exists yet).
* Whether later-corner geometry of an eliminated polygon is visible in our
  triangulation.
* Per-polygon `0x400` data (F10-F.1, #1406) is still unplumbed, so a caller
  must declare `two_sided`; no production path calls `classify` yet.
