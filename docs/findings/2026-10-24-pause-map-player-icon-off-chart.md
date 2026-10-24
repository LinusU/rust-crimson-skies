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

## Fit from the objective pins (owner route, 2026-10-24): no fit

The owner suggested fitting scale and offset from the four `OBJPIN` pixels ((506,149),
(396,254), (181,244), (106,319)) against measured world target points. Only three measured
world points exist (`2026-10-21-m01-objective-target-points.md`: Site #1 (-4278.5, -4760.9),
Site #2 (-6311.5, -4610.9), Site #3 (-8888.5, -4686.8), x and z), and no data links a pin to
a site: `OBJPINn` names objective index n-1 (`PRIMARY n`), while the sites are `Examine`
target nodes. The fourth pin has no measured point at all. A separate least-squares line
(`pixel = s * world + o`, per axis) over every choice of three pins in every order gives:

* x: the best slope is 0.0711 px/m (pins 1,2,3 in site order) with a worst residual of 22 px;
  the chart implied by M01's `WORLD` rectangle is 0.0665 px/m. Other orders are worse
  (35 to 245 px) or have the wrong sign.
* y: the three sites span only 150 m in z, so the slope is unconstrained and every order
  gives a physically impossible one (|s| 0.06 to 1.13 px/m against 0.058 expected), worst
  residual 6 to 92 px.

No assignment fits one affine map within a few pixels, so the pins are not drawn at the
examine sites (as `2026-10-21-pause-map-screen-data.md` item (h) already inferred), and this
data cannot fix the mapping. Not done: no change to `world_to_chart_pixel`, since a fit
would be a guess.

Still needed: the world positions of the four primary objectives' chart pins (an engine decode
of the `Objective`/`Pict OBJPIN` binding, or the owner reading the player's position while
capturing the Esc screen).
