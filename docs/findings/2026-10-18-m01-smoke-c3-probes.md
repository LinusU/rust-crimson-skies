# M01 owner smoke: the two probes that measured the wrong thing on C3

Task: Rally #1591 (`M01-SMOKE-C3-PROBES`), 2026-10-18. Implementer: sonnet-1.

Before: `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45` on
ZBD/C3/M01 failed 2 of 13 checks for harness reasons.

## `hud_objective_matches` — three faults, all in the probe

The HUD itself was right (the objective line is in `frame_*_hud.png`). The probe
read `drawn None, region text pixels None` because:

1. It queried `(&Text, &Visibility, &GlobalTransform, &ComputedNode)`. A Bevy UI
   node carries `UiGlobalTransform`, not `GlobalTransform`, so the query matched
   **no entity at all** (confirmed by dumping every `Text` entity: all ten had
   `ComputedNode` and `Visibility`, none `GlobalTransform`). Fixed: the probe
   reads `UiGlobalTransform` (a physical-pixel centre).
2. It tested `Visibility::Visible`, but the probe's own `Restored` pass writes
   `Visibility::Inherited` on every UI node, so the widget's own value says
   nothing. Fixed: it reads `InheritedVisibility`, what the renderer draws.
3. The widget text is `"OBJECTIVE - DESIGNED LAYOUT\n<line>"` while the read
   model's line is bare, so the two could never be equal. Fixed:
   `objective_line_of` strips the caption before comparing.
4. The HUD frame's `text_pixels` was never copied into
   `measurements.hud.region_text_pixels`. Fixed in `finish_measurements`.

After: drawn `"1) Find the main treasure site."`, visible, 9475 text pixels
(floor 64).

## `world_terrain_lit` — the region was the sea

The check read the frame's bottom third; at C3's start that is open water
(spread 5, floor 24). It now derives the region from the draw:
`near_terrain_luminance` cuts the probe frame into 32 px tiles, calls a tile
terrain when at least 10 % of its pixels differ from a neighbouring world pixel
(a texture does; flat water, a flat fill and a lone coastline edge do not),
takes the lowest unbroken run of rows holding terrain tiles (not a textured
cloud bank higher up), and measures the luminance range of the detail pixels in
the lowest third of that run. The floor (24) is unchanged. It is stricter than
before in one way: a frame with no terrain tile at all now reports 0..0 and
fails instead of reading whatever the bottom third held.

Limit, stated plainly: the measurement is shading **plus albedo texture**; a
framebuffer cannot separate the two without a differential probe, which this
task does not add. On C3 it reads 15..223 (spread 208).

## Proof each check fails when its subject is broken

Synthetic, in `accept_m01_smoke_c3_probes_*`: flat unlit terrain and sea-only
frames fail the lit measurement (`None` / spread below floor); shaded terrain far
above a flat sea passes; a missing objective line (`drawn None`) fails the
judge. I did **not** break the HUD or the renderer in a real run.

## Run on this Mac (gpu, audio, `CS_ENGINE_IMAGE`), C3/M01

`target/release/cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45
--capture-dir <dir>`: 13 checks, 0 failed (`world_sea_visible` skipped by name:
no water objects declared), 440 frames, 794 ticks. Framebuffers stay in
`private/` (not committed). Not produced: a committed
`docs/findings/evidence/M01-SMOKE-C3-PROBES.json` (no harness for this task).
