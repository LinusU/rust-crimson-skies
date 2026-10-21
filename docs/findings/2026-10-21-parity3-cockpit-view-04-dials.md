# Cockpit view 4/5: the six live panel dials are the `gauges` subtree at its stored transforms (Rally #1812)

Task `PARITY3-COCKPIT-VIEW.04`. Capabilities used: `retail`. Nothing here is
`verified_original`.

## What is drawn

`crates/cs_app/src/mission_session/cockpit_dials.rs` builds every mesh-bearing
node of `player` → `player_pfighter` → `cockpit1` → `gauges` (slot 56) in
`zbd/planes.zbd` through the same production path as `hud_art`
(`aircraft_graph`, `stored_render_mesh`, F17-B upload, `TextureBinder` — the
upload block is now the shared `hud_art::upload_gauge_part`), spawns the parts
under a `CockpitDialsRoot` child of the player body, and shows that root only
while `MissionViewMode::at_pilot_eye()` and the launch cinematic is not
playing — the same rule `cockpit_interior` follows (#1811).

Where `build_hud_art` re-places the subtree at the measured `instruments.zrd`
HUD positions, the panel dials keep each node's **composed stored transform**
(the cockpit dash space the `.03` finding noted the cockpit pass uses). The
measured stored globals under the `player` root (probe of the retail
`planes.zbd`, this task's session):

| node | stored translation | stored rotation (x quat ≈ tilt) |
|---|---|---|
| `gauges` (56) | (0, 0, 0) | identity |
| `horiz` (57) | (−0.00, −3.054, −11.006) | x 0.175 (~20° panel rake) |
| `pfhorizon` (61, under `horiz`/`horiz`) | (−0.00, −2.707, −11.951) | x 0.175 |
| `comp` (58) → `compass` (65) | (−0.00, −1.133, −13.030) | x 0.115 |
| `comp` → `g730` bezel (66) | (−0.00, −1.470, −11.589) | x 0.115 |
| `altimeter` (59): `hundreds` 69, `thousands` 70, `lowalt_on` 71, `g784` 73 | (2.0, −4.931, −10.316) | x 0.175 |
| `speedometer` (63): `speed` 75, `stallwarning_on` 76, `g784` 78 | (−2.0, −4.931, −10.316) | x 0.175 |
| `damageindicator` (68): `g843` 74, zones 79–82 | (−0.00, −4.979, −10.298) | x 0.182 |
| `missilegauge` (83): `4char_ammo` 87, `mgindicator0..7` 88–97, `mgarrow` 92, `6char_type` 93, `g819` 98 | (−2.0, −3.054, −11.006) | x 0.175 |
| `gungauge` (84): `4char_ammo` 101, `ggindicator0..3` 102–105, `ggarrow` 106, `6char_type` 107, `g815` 108 | (2.0, −3.054, −11.006) | x 0.175 |
| `nitrogauge` (85): `nitro_boost` 110, `nitro_backplate` 111, `nitro_charge` 112 | (5.793, −5.150, −11.148) | y 0.259 (angled side panel) |

So the panel reads exactly as the owner's captures: ROCKETS
(`missilegauge` = `BOOM`/`4char_ammo` + 8 pips), artificial horizon (`horiz` +
`pfhorizon` ball) and GUNS (`gungauge`) on the top row; MPH (`speedometer`),
the damage silhouette (`damageindicator`) and ALT (`altimeter`) on the bottom
row; the compass strip (`compass` tape + `g730` bezel) above the panel.
`markers`/`marker` furniture nodes bind no mesh and appear only in the report.
All 39 mesh bindings draw; none is left undrawn.

## The shared drive

The panel parts are tagged by the same `hud_art::tag_part` as the corner
cluster (`HudNeedle`, `HudCompassTape`, `HudAttitudeBall`, `HudPip`, `HudFlag`,
`HudArrow`, `HudNitroNeedle`, `HudZone`/`HudZonePiece`, `HudGlyphCell`), and
the one `update_hud_art` system drives both clusters from the same
authorities: speed needle `−0.02811017 rad/(m/s)` (engine-measured, VA
`0x6076e8`), altimeter `−2π`/1 000 ft and `−2π`/10 000 ft (designed), compass
tape one measured width per 360° (designed), pip fill order (designed), glyph
texture swaps, zone material recolours. `tag_part` now takes the spawned
transform so each cluster's recorded `base` matches its own placement.

## Findings and unknowns

- **`pfhorizon` attitude ball**: drawn only here (never in `instruments.zrd`,
  so the HUD pass never reached it). The drive counter-rotates it
  `Rz(−roll)·Rx(−pitch)` — the gyro-fixed ball is the designed inverse of the
  airframe's attitude; the original's mapping, axis, sign and range are
  **unmeasured**, recorded in the build report's `unknowns`.
- **`lowalt_on`, `stallwarning_on`, `mgarrow`, `ggarrow`, `nitro_boost` /
  `nitro_charge`** draw at their authored state (flags hidden): no measured
  driver or threshold exists; recorded unknown.
- **Chase view unchanged**: `fit_hud_viewport` now also deactivates the
  `HudArtCamera` and `fit_safe_area` hides the text layer while
  `at_pilot_eye()` — at the pilot eye the corner clusters would draw over the
  panel's own dials. The spyglass window sits outside `SafeArea` and keeps
  drawing. Whether the original still draws the `rcircle` reticle in the
  cockpit views is **unmeasured**; it stands down with the rest of the text
  layer while the gunsight glass (`inside`/`cockpit6`, .03) is the sight.
- The tape scroll, needle spins and ball counter-rotation are local-space
  compositions on the raked (~20° about x) panel transforms; the rake is about
  the tape's own scroll axis, so the drive's x-scroll stays the tape's scroll
  direction. The `nitrogauge` side panel is yaw-mounted and drawn as stored.

## Evidence

Acceptance: `accept_parity3_cockpit_view_04_*` in
`crates/cs_app/tests/accept_parity3_cockpit_view_04.rs` (via `suite_a.rs`) —
an authored-part test that fails when the spawn, the view drive or the shared
drive is removed, and a retail M01 composition test that checks all 39
bindings drawn at dash transforms, markers tagged, the pilot-eye visibility
rule, the HUD camera standing down at the pilot eye, and needles/pips
answering telemetry and the live weapon session. The on-screen smoke belongs
to `PARITY3-COCKPIT-VIEW.05` (#1813).
