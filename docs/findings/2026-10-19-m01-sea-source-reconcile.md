# #1698 M01-SEA-SOURCE-RECONCILE: what draws M01's sea

Date: 2026-10-19. Implementer: `claude-2`; no independent review yet. Nothing
here is `verified_original`; the original executable did not run.

## Measured

Owner-command smoke on retail M01 (`cs --cs-path $CS_GAME_DIR --mission M01
--smoke-seconds 45 --capture-dir private/evidence/M01-SEA-SOURCE-RECONCILE`,
21 checks, 0 failed; screenshots ignored by Git), sea phase, session paused,
dome, weather and fog parked:

* the frame with tiles and plane: 1 590 772 sea pixels (431 per mille);
* **the world's own `wtr00000` tiles draw 1 555 101 of them (421 per mille)**:
  255 drawn child parts of the world draw bind the water texture;
* the horizon plane draws the other 35 671 (about 22 in 1 000 of the sea):
  it only shows past and between the tiles, where the tiles leave gaps.

So the answer to "tiles, plane or both" is: both, but the tiles are the drawn
sea and the plane is the continuation beyond them (it clones the tiles'
material and sits behind them with a depth bias, #1650). The plane is not
drawn over where the world provides water. The 347 of #1648 (251 `world1`
terrain tiles + 96 `swtr0n` meshes) count meshes; 255 is the count of drawn
parts bound to `wtr00000`; the difference is not reconciled here.

## Why `world_sea_visible` skipped, and what it does now

The check gated on object records with a known water surface
(`known_surface() == Water`); M01's records declare none (0), so it skipped
even though 255 water parts are drawn. It now applies when the world declares
water **or** draws a part bound to the water texture (`WorldWaterPart`, marked
in `attach_world_parts`), and it passes only when those tiles draw at least
`MIN_SEA_COVERAGE_PERMILLE` of the frame on their own (probe: frame without
plane minus frame without plane and tiles) and the whole sea does too.

## Not matched to the original (unmet)

The captured sea frame does not look like `private/ref-drydock/m01/orig25.png`
(only `orig25.png` exists in that directory; there is no `orig70.png`): the
tiles read as hard-edged dark-navy and pale-teal polygons, where the original
is one continuous mid-teal sea with a soft shading. The probe frame parks the
fog and hides the dome, so it is not a player's frame; the player's frame
(`frame_00300_hud.png` in the same directory) was not compared pixel for pixel.
Cause of the two tones is **unknown** (candidates: per-tile lighting or normals,
tiles at differing altitudes, the 16-frame cycle not played). Follow-up needed.

## Unknown

* The unit of the 16-frame `wtr00000…15` cycle: not played; frame 0 is shown.
* The horizon fade form; the role of the 96 `swtr01…05` meshes.
