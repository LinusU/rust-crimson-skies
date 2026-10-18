# M01-WINDOW-WORLD (#1537): the windowed M01 world on ZBD/C3, re-checked

Date: 2026-10-18. Task #1537, integration step. Implementer: sonnet-1 (Claude
Sonnet 5.5). Capabilities used: `retail`, `gpu` (the owner-command smoke on an
Apple M3 Pro, Metal). Nothing here is `verified_original`; no original
executable ran. The owner's 2026-10-17 screenshots were taken on `ZBD/C1C`
(M06's archive, see `2026-10-17-m01-archive-decision.md`); every number below is
re-measured on `ZBD/C3/M01` after #1567, #1578 and #1580 landed.

## Command and run of record

```sh
cargo build --release --locked -p cs_app --bin cs
CS_CAPABILITIES=retail,gpu,audio CS_ENGINE_IMAGE=$CS_ENGINE_IMAGE \
target/release/cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 \
  --capture-dir private/evidence/M01-WINDOW-WORLD
```

The console states `world world/c3`, `mission world draw: 377 meshes bound,
643 drawn parts (640 textured)`. 11 of 13 checks pass; the two that fail are
named under "Unmet" below. Screenshots (ignored by Git, in
`private/evidence/M01-WINDOW-WORLD/`): `frame_00033_world.png` (probe: horizon,
weather, player and UI hidden — the grey band between the water tiles' edge and
the sky there is the probe's clear colour, not a drawn surface),
`frame_00040_sea_visible.png`, `frame_00276_hud.png` (the ordinary view).

## Answers to the four bullets

1. **Untextured, unlit terrain.** On C3 the terrain is textured: 640 of 643
   parts bind a texture from `ZBD/C3/rtexture12.zbd` (forest1, cliff1, shore2,
   sand128 ...). The C1C census (288 water/cloud tiles plus 21 untextured fog
   boxes) described M06, not M01.
2. **Square edge and sea.** C3's world water is bound to `wtr00000`, not
   C1C's `water1`. The sea-level measurement only knew `water1`, so on C3 it
   found no water and the horizon sea stayed at its designed -1000 m, a 1000 m
   step below the world's own water tiles. It also demanded every water
   vertex flat, but C3 water spans y = 0..13.2 m (4 477 vertices: the sea
   plus raised pools and falls). Now `wtr00000` counts as water and the level
   is the lowest altitude when more than half of the water vertices share it:
   **y = 0**. The sea meets the world's water at the same altitude and the
   ordinary frame (`frame_00276_hud.png`) shows sea to the horizon with no
   visible square edge. The designed half extent and fog are unchanged.
3. **Start pose.** The record is `ZBD/C3/M01/zrdr.zbd` member `aiv.zrd`,
   `player`: position (-1426, 150, -1813) m, stored heading 40 deg
   (0.698 rad), per `2026-10-18-m01-c3-player-config.md`. So the start is
   **150 m**, low, not the 1 318 m of the C1C record the owner saw, and
   consistent with the walkthrough's low start. That the heading faces the
   cave is not verified here (no cave geometry was located).
4. **Smoke.** See the run above.

## Unmet

* `world_terrain_lit` fails: the probe frame's bottom third is open water, so
  its spread is 5 levels (floor 24). The islands do show hillside shading;
  the check measures the wrong region at this start pose. Not weakened here.
* `hud_objective_matches` fails (model has an objective, the probe reads none
  drawn) although `frame_00276_hud.png` shows the objective line. Different
  surface; not touched.
* `world_sea_visible` is skipped by name: the object records declare no known
  water surface under the check's rule.
* Raised-pool water shows the horizon sea through gaps as a brighter blue.
