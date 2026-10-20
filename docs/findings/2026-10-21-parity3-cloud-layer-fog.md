# #1745 PARITY3-CLOUD-LAYER-FOG: the static data places no cloud layer or in-layer fog in a sea-level climb; task blocked

Date: 2026-10-21. Implementer: `claude-2` (Sonnet 5.5); no independent review. Nothing here is
`verified_original`; the original executable was not run. Reference: the owner's two Drydock
frames (translucent cloud blobs about 15 s into the climb, then uniform pale grey fog inside the
layer); the images are not committed and were not re-measured here.

## What the static record says (all from earlier findings, no new measurement)

| fact | source |
| --- | --- |
| M01 (`ZBD/C3/M01`) `CLOUD_COVER`: `TOP 11000`, `BOTTOM 10000`, `THICKNESS 50`, colours bytes (32, 56, 72); a trapezoid colour-and-intensity volume for a camera or vertex inside it, no drawn texture | atmo-A §3 (`0x473636`, `0x42ee60`) |
| `ZONE1.FOG_ALTITUDE 9000 10000`, same constant fog for all of `ZONE1` (grey 0.79, 1000/4500); renderer use of the altitude pair unmeasured | atmo-A §2, §6 |
| `zone2_cloud_floor` node absent from `ZBD/C3/gamez.zbd`; no sprite clouds in the archive; `cloud1.tif`/`cloud2.tif` named by the `g1155` quads are in no `ZBD/C3` texture archive | atmo-A §7, atmo-D |
| no fog volumes (`fvol*`, `fogvol.zrd`) in M01; no script ramps a fog range, density or colour | #1744 finding on the haze fade |

## Why this does not match the owner's frames

The recorded cloud band starts at 10 000 m. A sea-level pull-up reaches roughly 1 300 m after
about 15 s (#1440 composed run), so the player is nowhere near `BOTTOM`. Nothing in the data puts
a cloud volume, a fog colour change or a sprite fade in the first 20 s of the climb. Either the
frames' effect comes from an unmeasured consumer (the `FOG_ALTITUDE` renderer use at fog object
`+0x28`/`+0x2c`, whose M01 values 9000/10000 are also far too high, the missing `cloud1/cloud2`
quads, or a state the capture does not show), or the units of the band are not metres, which
atmo-A argues for but did not prove by a draw.

## Not implemented (rule 4)

The altitude band, in-layer fog colour and density, and sprite fade rules for the "below / in /
above layer" states are not recoverable from what is committed. A designed layer would be a
fabricated tuning table and the task forbids it ("Unmeasured -> unknown"). The smoke comparison
beside the original frames cannot be produced either.

## What is needed

* a disassembly (or an original-run capture) of the fog object `+0x28`/`+0x2c` consumer and the
  `cloud_inside_fraction` caller `0x42b7c0`, to show what changes at 1-2 km, or
* an owner capture with a known altitude readout at both frames (state which of the three states
  begins at which altitude) so the band can be measured rather than guessed.
  This host has no disassembler (no `capstone`; global installs are out of bounds).

Affected content: M01 climb from sea level (cloud sprites and in-layer fog). Resolving task: this
one, after the above.
