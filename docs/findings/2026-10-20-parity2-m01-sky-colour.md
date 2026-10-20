# #1729 PARITY2-M01-SKY-COLOUR: why M01's sky read grey, and what is still unknown

Date: 2026-10-20. Implementer: `claude-2` (Sonnet 5.5); no independent review yet. Nothing here is
`verified_original`; the original executable was not run. Reference: the owner's Drydock launch
frame (`private/ref-drydock/m01/orig25.png`, not committed).

## What drives the sky colours (data, all from `ZBD/C3/gamez.zbd`)

The sky is not a colour, it is geometry under `horizon` (atmo-A §5). Its colour is the per-corner
stored colour times the `Sky1`/`cumulus` texture:

| corner colours (bytes) | where |
| --- | --- |
| `(200, 200, 200)` | dome base ring, Y 500 m, and the untextured skirt |
| `(199, 207, 218)` | ring at Y 744 m |
| `(167, 188, 211)` | ring at Y 1308 m |
| `(176, 209, 242)` | zenith cap, Y 1646 m (12 corners) |

`Sky1` is a 128x128 opaque (alpha 255) white texture with pale blue streaks.

## Finding: the stored bytes are display values

Drydock's launch frame, mean of the top pixel row (10-90 % of the width): **(179, 210, 245)**.
The cap's stored colour: **(176, 209, 242)**. They agree within 3 per channel. Before this change
the corner colours reached the renderer as linear light, so the sky was drawn at the wrong
brightness and the blue was lost (our top row `(176, 186, 195)`, grey-blue). The sky parts now
convert each stored channel sRGB to linear (`display_value_corner_colours`,
`mission_session/world_draw.rs`), so the output encoding returns the stored byte. Top row of our
first-control frame afterwards: `(151, 172, 193)`; mean per row 0.20 of the height
`(151, 165, 180)` against the original's `(187, 202, 226)`.

Only the `horizon` parts take this; terrain and objects keep their colour handling.

## Still unknown (affects: brightness of the whole sky, not its hue)

Ours is now blue like the original but uniformly about 1.2x darker per channel
(the ratio original/ours is 1.19-1.27 on every sampled row). The remaining factor is one of:
the texture term (`Sky1` samples about 0.86 where the original behaves like about 1.0), a
modulate-by-2 style combine, or the capture's own brightness. The sky texture decode,
modulation rule and any display gamma of the original's renderer were not measured, so no
constant is added to compensate (rule 4).

Not changed here, and visible in the same frames: the distance fog (`ZONE1.FOG_COLOR`
(0.79, 0.79, 0.79), drawn as bound by #1649) still grey-washes the terrain; cloud sprite
shading; the original's thinner haze band. Their causes are untraced.

## Evidence

`cargo test -p cs_app --test cs_app_tests_a -- accept_parity2_m01_sky_colour --include-ignored`.
Owner-command smoke: `cs --mission M01 --smoke-seconds 45 --capture-dir private/sky1729/ours`
(25 checks, no failures); frames stay in `private/`.
