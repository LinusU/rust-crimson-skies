# BUG-M01-CROSSHAIR-WRONG-SPRITE: the flight reticle is `impact_point`, not `rcircle`

Date: 2026-10-24. Task: #2016. Capabilities: `retail`, `gpu`, `audio`. Nothing here is `verified_original`.

## Measured

* The owner's original M01 flight frames (`private/ref-drydock/m01/orig25.png`, `orig70.png`, not
  committed) show a small translucent grey ring with four cross ticks at screen centre, about 32 px
  at 1024×768.
* `ZBD/rimage.zbd` decodes (production `read_zbd_textures`) `impact_point` as 32×32 RGB565 with an
  alpha plane: a ring with four cross ticks and a bright centre dot. `rcircle` (32×36) is an uneven
  hand-drawn felt-marker circle, `alignment_point` (32×32) decodes black/transparent.
* Our chase frame (`frame_03421_cam_chase.png`, `--smoke-seconds 45`, private) shows the
  `impact_point` ring at screen centre, matching the original's look.

## Designed / unknown

* `impact_point` is chosen by **visual match**; the original engine call site that draws it was
  not traced in `$CS_ENGINE_IMAGE`. Whether the original composes it with other layers is unknown.
* `rcircle` is kept unused for the briefing/debrief map callouts (owner's observation, not
  source-verified). `rcircle_y` and `alignment_point` roles are unknown.
* The reticle stays a fixed 32×32 centre node; the aim ray is untouched, so targeting is unchanged.
* Chase vs pilot-eye behaviour of the reticle in the original is unmeasured.
* No pixel comparison against the original was made; the GPU comparison is by eye only.

This corrects `2026-10-20-m01-hud-art-instruments.md`, which named `rcircle` from its name alone.
