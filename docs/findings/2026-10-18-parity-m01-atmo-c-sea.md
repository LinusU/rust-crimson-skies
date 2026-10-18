# #1650 PARITY-M01-ATMO-C: the M01 sea drawn with the world's own water art

Date: 2026-10-18. Implementer: `sonnet-1`; no independent review yet. Nothing
here is `verified_original`; the original executable did not run.

## What changed

The horizon sea (the plane that continues the world's water tiles to the
horizon) was a flat designed colour. It is now drawn with the **material of the
world's own water tiles**: on `ZBD/C3/M01` the original `wtr00000` image bound by
the world draw (`ATMO-A` §5: there is no `water1` in M01's archive). Three
measured or derived facts feed it, all from the geometry the world draw really
binds (`measure_water_art` in `mission_session/world_draw.rs`):

* the **material** of the water part with the most triangles (texture, tint,
  lighting, culling all the tiles' own); the sea clones it and only adds a depth
  bias so it sits behind the tiles;
* the **texture mapping** `[u, v] = J · [x, z]` solved from that part's largest
  triangle in world space, so the sea repeats the art at the tiles' own density;
* the sea follows the camera in whole texture repeats (`HorizonSpec::sea_snap`),
  so the water does not slide under the aircraft.

Origin label: `WORLD_WATER_ART_LABEL` (designed). The art is original; that the
original repeats it to the horizon is this build's choice.

## Horizon fade and coast

* **Fade:** the sea's far edge fades through the camera fog into the dome's
  colour below the horizon (the existing seam rule); the sea extent (20 km) is
  not shorter than the fog end. The fog values are still designed; binding M01's
  `ZONE1` fog is #1649 (ATMO-B).
* **Coast:** the beach is drawn by the world's own shore tiles (`shore2` etc.),
  not by the sea. No separate beach line is drawn; nothing in the archive was
  found that is one.

## Unknown, with the affected content

* The unit of the 16-frame `wtr00000…15` cycle (rate word 10.0, `ATMO-A` §5): the
  cycle is **not played**; frame 0 is shown.
* The role of the 96 `swtr01…05` meshes (unmeasured, not drawn specially).
* Where the original's water reaches: the sea plane is 40 km square.
* In the owner-command smoke, world water tiles read darker than the sea seen
  through gaps between them (`frame_00040_sea_visible.png`), though both use one
  material and near-upward normals. The cause (shadowing or tile geometry of the
  tiles) is not measured here.

## Verification

`accept_parity_m01_atmo_c_*` in `tests/campaign/parity_m01_atmo_c.rs`; the retail
member (`CS_GAME_DIR`) reads M01's art through the production stage. Owner-command
smoke: `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-ATMO-C` → 13 checks, 0 failed, 13 framebuffers
(`frame_00040_sea_visible.png`, `frame_00300_hud.png`; ignored by Git).
