# Pause map: the player icon is drawn but falls outside the chart (#1987)

Task `PARITY3-PAUSE-MAP-GAP.04`. Provisional: a scripted smoke run, never `human_play`;
nothing here is `verified_original`.

A probe added to the M01 owner-command smoke (not committed: it fails, see below) read the
presenter at the open map. Run of 2026-10-24:

* The icon **is spawned** (one `ImageNode` with the `OWNSHIP` bitmap) and `PauseMapLive.player`
  is set. It is not a spawn-order or missing-body fault.
* The chart's drawn rectangle is (16,19)..(589,519). `world_to_chart_pixel` puts the player
  outside it, so the chart's clip container hides the icon:
  * at the smoke's Esc-map phase the player had been moved by earlier probes to
    (-1560.1, -0.6): pixel (632, 550);
  * placed back at the mission's measured start (-1426, -1815.8, the spawn of the run): pixel
    (641, 445).
* Cause: M01's `WORLD` rectangle spans x -10818..-2204, but the mission starts at x about
  -1426, east of its right edge (u = 1.09). The candidate "`WORLD` is the drawn chart" mapping
  (`2026-10-22-pause-map-live-objectives.md`, item 3) cannot place the start on the chart,
  while the original draws the icon on the island's east coast. The mapping is wrong (the
  rectangle's extent, offset or scale), not the drawing.

## Needed

One known player position and heading beside the icon's pixel in the owner's original
screenshot (or an engine decode of the `MAP` primitive, #1817) to fit the mapping. Fitting a
scale and offset from the start position alone would be a guess.
