# Pause map: `CLIP` is a source rectangle of the chart bitmap (#1984)

Task `PARITY3-PAUSE-MAP-GAP.01`. Provisional: from a scripted smoke run beside the
owner's original screenshot, never `human_play`; nothing here is `verified_original`.

`escape.zrd` M01 `MAP`: `BITMAP HA-m1MAP` (800x600), `POSITION [16 19]`, `CLIP [211 51]..[784 551]`.
The bitmap's left 207 px are the paper's label panel (title, compass, logo); the island art
runs from there to the right edge.

* Reading `CLIP` as a *screen* clip with the whole bitmap at `POSITION` (the earlier
  presenter) drew a flat blank frame for screen x 16..211, a strip of the label panel at
  211, and the island starting at 0.32 of the screen, cut off under the note.
* Reading `CLIP` as the *source* rectangle (bitmap pixels 211..784 x 51..551), drawn with its
  corner at `POSITION`, gives screen 16..589 x 19..519: the island fills the frame from its
  left edge to the note, and the authored pin positions (`[506 149]`, `[396 254]`, `[181 244]`,
  `[106 319]`) fall on and around the island as in the owner's screenshot.

The second reading is the one implemented. It is a fit to one screenshot, not an engine decode
(`static`-level at best). Consequence for the candidate `WORLD` mapping: it now targets the
drawn rectangle (16,19)..(589,519), so Lighthouse maps to about (210, 216) rather than
(405, 248). That mapping stays unmeasured (#1817).
