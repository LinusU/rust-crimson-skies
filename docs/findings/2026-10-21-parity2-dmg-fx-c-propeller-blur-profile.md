# Propeller blur disc: the texture's radial profile (#1807 `PARITY2-DMG-FX-C`)

* **Status:** measured from `rotorblur.tif` as the player's `prop1` draws it
  (64x64, alpha plane). Nothing here is `verified_original`.

## Measured

Mean RGBA of the drawn texture by radius (fraction of the half-width):

| r | alpha | red |
| --- | --- | --- |
| 0.0 | 52 | 0 |
| 0.125 | 56 (max) | 0 |
| 0.4 | 39 | 0 |
| 0.6 | 21 | 22 (max) |
| 0.8 | 15 | 8 |
| 0.97 | 3 | 0 |

The coverage is a smooth falloff (at most 22% opaque anywhere) and the only
ring in the texture is one faint red annulus near 0.6 of the radius. Green and
blue stay near 0. There is no repeated concentric structure in the texture.

The disc was therefore already drawn translucent (`AlphaMode::Blend` over the
texture's own coverage, `player_visual.rs`) with the static blades hidden while
the engine runs. Its rings, if more are seen in the original, would come from
the two stacked discs (`prop1` at z 3.865 turning −220 and `prop1b` at z 4.350
turning +60, `2026-10-21-parity2-dmg-fx-a-effect-definitions.md`), which
`prop1b` not being drawn leaves untested.

## Unmet

The owner-command smoke run on this host (Apple M5 Max, Metal, unattended
session) produced all-black framebuffers (10 of 37 checks failed, including
`world_terrain_textured`), so no frame of the disc could be seen or cited, and
no original frame with the blur is in `private/ref-drydock/m01/` to compare.
