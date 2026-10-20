# #1730 PARITY2-M01-SEA-SHADING: the dark patches were the tiles' stored corner colours

Date: 2026-10-20. Implementer: `claude-1` (Sonnet 5.5); no independent review yet.
Nothing here is `verified_original`; the original executable did not run.

## Measured (retail M01, water parts of the world draw)

Every part bound to `wtr00000` carries stored corner colours (no stored normals,
altitude 0 except one part). 180 of the 251 tiles are uniformly 170/255 (0.667);
the coast tiles range 0.38–1.0. The tiles are drawn `texture × corner colour`
(#1706), so tiles read darker than the horizon plane, which cloned the tile
material but carried no corner colours (1.0). Where tiles leave gaps or end, the
plane showed as lighter polygons with straight edges: the "patches" of #1730.
Not z-fighting, not missing filtering (the texture is sampled filtered).

## Changed

The plane's corners carry the mean stored corner colour of all water-part
vertices (`WorldWaterArt::corner_shade`, `HorizonSpec::bind_world_water_shade`).
Designed choice: the mean; the original's treatment of the far sea is unmeasured.

## Smoke

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY2-M01-SEA-SHADING`: 25 checks, 0 failed. Before/after
frames (`frame_01206_cinematic_mid`, `frame_01621_scene_27s`,
`frame_02410_first_control`): the large dark wedges and angular shapes are gone;
faint tile seams remain where coast tiles' corner colours differ.

## Unmet / unknown

* Horizon fade: the sea still meets the grey fog at a hard line; the fog form
  (linear, 1000/4500 m) is measured (#1649) but the far sea ends at the fog
  colour without the soft gradient the original frame shows. Unmeasured.
* Shore surf line: a thin white line is visible from the shore tiles' own art;
  no separate beach geometry was found.
* The 16-frame `wtr` cycle is still not played; the original reference image was
  not available locally, so no pixel comparison was made.
