# PARITY2-M01-MISSING-TERRAIN (#1735): the island's terrain groups were never drawn

Task #1735. Owner report: in the smoke's `world` frame the two hills were joined
by a flat hole through which the sky showed, its lower edge a straight cut at the
waterline; in the cinematic the island was a hollow shell with a gap. Capabilities
used: `retail`, `gpu` (the owner-command smoke, Apple M5 Max, Metal). Nothing here
is `verified_original`; no original run happened.

## Cause (measured)

`import_world_container` owns the world record's **direct children** plus the
`horizon` subtree (`2026-10-17-m01-window-farimport.md`). On `ZBD/C3/gamez.zbd` the
island's terrain is not only direct children: 28 of the world record's 453
direct children are grouping nodes (`g28515`, `g28414`, `g28548`, `g28707`, ...)
with `mesh_index = -1`, and the terrain tiles, buildings, boats, guns and
landing pads hang below them (for example `g28515` slot 548 owns `g28607`
slot 554, a world-space tile at (-11264, 0, -12288)..(-10240, 213, -11264), beside
75 mesh-bearing descendants). Their descendants were never imported, so they
were never drawn. Everything else the task listed was checked and is **not** the
cause: the face census of the container is complete (16 087 declared = stored =
decoded faces, none invalid, culled or missing), every one of the 377 meshes the
definition names is uploaded with exactly the triangles its topology declares
(8 110 = 8 110), the draw culls no face (`cull_mode: None`), and 81 definition
objects bind no mesh by the store's own statement.

## The fix

`playtest_retail::build_world_scenery` cuts each direct child's subtree out
through the area's own `area_graph` and selects it with the area's own
`select_area_parts` (one LOD band at the player's designed distance, the intact
variant of every part, no flat-colour cards), at each node's composed canonical
transform, uploading and binding every distinct mesh once. `world_draw::build_scenery`
applies the stored-shading materials (#1706) and the world draw spawns the result
under one `WorldSceneryRoot` entity. The mission's actor-owned nodes are left to
the actors. On M01: 819 nodes from 443 groups (420 meshes), 17 bindings left
undrawn by the intact-variant rule, none unbuildable.

## Left undrawn on purpose (rule 4)

* **The `multiplayer*` zeppelins, `ctf_*` and `cs_flag*` roots**: multiplayer-mode
  content whose role in a single-player mission nothing measured states
  (`multiplayer1zep` slot 931, `multiplayer2zep` 1455, `ctf_1` 3090, `cs_flag_1`
  3091, `ctf_2` 3092, `cs_flag_2` 3096). Resolving task: none filed; needs a
  measurement of the original's mode gating.
* **`horizon`, `dzpath*`** (destroyed at load) and the zeppelin actors' nodes
  (`piratezep`, `cargozep1`) are other paths'.
* **The LOD band and the damage state are designed**: one band at the player's
  distance for the whole world, `healthy` over `destroyed`. The original's
  per-frame LOD pick is unmeasured; gun turrets behind a 0-500 m band (for example
  slot 1969) are therefore not drawn.
* **The scenery has no collider**: it is drawn only. Flying into a terrain tile
  below a group does not collide. Affected content: every mesh listed above.

## Checks

* `accept_parity2_m01_missing_terrain_every_terrain_tile_of_the_world_is_drawn`
  (retail, ignored without `CS_GAME_DIR`): the 7 terrain tiles (`g<digits>` meshes
  directly under a `g<digits>` group of the world record) each draw exactly the
  triangles their mesh declares; fails when the scenery is removed.
* `accept_parity2_m01_missing_terrain_clear_colour_below_the_horizon_is_counted`:
  the pure probe behind the new smoke check.
* Smoke check `world_terrain_closed` (`docs/PLAYTEST.md`): in the `sea_absent`
  frame the clear colour must not show below the water tiles' far edge. Measured
  on the same frame before and after: 35 649 pixels before (fail), 0 after.

## Evidence (private, never committed)

Owner-command smoke before (main, `private/evidence/1735-before`) and after
(`private/evidence/1735-after`): `frame_02450_world.png` (hole before; closed island
after), `frame_02410_first_control.png`, `frame_02461_sea_absent.png`.
