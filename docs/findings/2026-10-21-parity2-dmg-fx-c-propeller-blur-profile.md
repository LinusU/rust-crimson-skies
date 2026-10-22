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

## Compared with the owner frame (smoke on a display-attached host)

`cs --cs-path <retail> --mission M01 --smoke-seconds 60 --capture-dir
private/evidence/PARITY2-DMG-FX-C` passed (59 checks, failures: none); the
frames `frame_03298_cam_f10_back.png` and `frame_03148_cam_chase.png` show the
disc from behind (not committed). Against the owner's `orig70.png` (+70 s):

* Ours: a translucent grey-blue disc of the player's `prop1` (radius 1.666 m),
  the static blades hidden, the ring only a faint tint. It is not opaque.
* Original: the disc reads darker and more opaque, with pinkish ring arcs and
  ghost copies offset from one another. Those copies could be `prop1b` drawn
  at its own z and turn, or the frame's own blur; the frame does not tell.

The ring colour (red annulus near 0.6 radius) is present in the texture and
drawn. The darker opacity and the offset copies are **not** reproduced and are
not explained by any measured number, so no alpha or ring count was added.
Affected content: the player's prop in every M01 frame. Resolving work: a
measurement of which prop states the original draws at which engine state.
