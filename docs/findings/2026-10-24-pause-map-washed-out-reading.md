# Pause map: is the screen drawn washed out? (#1985)

Task `PARITY3-PAUSE-MAP-GAP.02`. Provisional: a scripted run, never `human_play`,
nothing here is `verified_original`.

## Question

#1818 compared the smoke frame with the owner's original screenshot and saw our
chart, note and portrait as faded where the original's are saturated. Is there
an alpha, tint or dimming overlay in our pause screen?

## What was checked (run of 2026-10-24, `--mission M01 --smoke-seconds 45`)

* **No overlay in the code.** `spawn_pause_map` spawns an opaque black backdrop,
  then the dialog's own bitmaps. No `BackgroundColor` with alpha, no `ImageNode`
  colour tint, no dimming node (`crates/cs_app/src/mission_session/pause_map.rs`).
* **The chart is opaque.** `HA-m1MAP` (800x600): mean alpha 255, 0 non-opaque
  texels. The shared LOADINGDIALOG parts do carry partial alpha, as stored:
  `parchment` (240x312) mean alpha 204, `momento_temp` (242x305) mean alpha 228
  with 8 816 non-opaque texels (the photo's torn border), `momento_shad` mean
  alpha 18 (the drop shadow). Those are the stored coverage, not a wash.
* **The framebuffer equals the decoded texture.** Sampling the chart area of
  `frame_*_pause_map.png` (2560x1440, 4:3 fit at scale 2.4, left 320) against
  the texture at the same chart pixels gives a mean RGB of (69.6, 138.4, 134.7)
  in the frame and (64.6, 141.0, 137.7) in the texture: equal within the
  filtering of the up-scale. The drawn portrait region averages (71, 65, 52),
  the texture's opaque part is darker than its 8 816 translucent border texels
  suggest. Nothing between the decode and the framebuffer lightens or greys the
  colours.

## Reading

Our pause screen is not dimmed by an alpha, a tint or an overlay: it draws the
decoded bitmaps unchanged. The "washed out" look of #1818 therefore cannot come
from this layer. Candidates left, none decidable here:

1. The grey-white impression is the flat white left panel and the offset chart
   (#1984), which dominate the frame.
2. The decode of the stored 16-bit bitmaps (F17-B RGB565 expansion policy) or a
   display gamma the original applies is lighter/darker than ours. The retail
   dialog data names no gamma, brightness or tint for the screen.

Deciding between them needs the owner's original screenshot pixels (not in the
repository, never committed), compared region by region with this frame. That is
`human_review` evidence. No code was changed: guessing a tone curve without it
would invent a rule.
