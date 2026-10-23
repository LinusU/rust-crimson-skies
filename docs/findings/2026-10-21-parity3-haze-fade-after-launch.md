# #1744 PARITY3-HAZE-FADE-AFTER-LAUNCH: no static driver for a fog fade found; task blocked

Date: 2026-10-21. Implementer: `claude-2` (Sonnet 5.5); no independent review. Nothing here is
`verified_original`; the original executable was not run. Reference: the owner's Drydock frames
c03 (+15 s, thick haze) and c04 (+20 s, clear), not committed.

## What was searched, and what it shows

| candidate driver | evidence | result |
| --- | --- | --- |
| `weather.zrd` | `ZBD/C3/M01/zrdr.zbd`; atmo-A §1 | One constant `ZONE1` fog record (colour 0.79 grey, range 1000/4500) and `CLOUD_COVER` at 10-11 km. No timeline, ramp, delay or second record for the same zone; a flight under ~10.5 km stays in `ZONE1`. |
| mission script fog natives | `strings` over `ZBD/interp.zbd`; engine native table near image offset `0x23e1bc` | The engine registers `WorldSetFogState` (`exponential`/`linear`), `WorldGetFogRange`, `WorldSetFogRange[Near/Far]`, `WorldSetFogDensity` and a fog colour setter. Of these, interp.zbd spells only `WorldSetFogState`, in the world-setup preamble (`FindNode %worldName%`, `WorldSetFogState`, `linear`), at world load. No script spells a range, density or colour call, so no script ramps the fog. `ZBD/C3/M01/mis_anim.zbd` and `ZBD/C3/M01/zrdr.zbd` spell no fog call either. |
| fog volumes (`fvol*`, `fogvol.zrd`) | `2026-10-07-m01-lc-fvol-roles-and-axis-convention.md` census | Group `c3` holds **0** `fvol*` records, and `ZBD/C3/M01/zrdr.zbd` has no `fogvol.zrd`. So the player cannot be flying out of a fog volume in M01. |
| zone switch by camera altitude | atmo-A §4 | Applied "when it changes", but both M01 flight zones near the carrier are `ZONE1`. The launch cinematic's camera zone is untraced. |

## Not found, so not implemented (rule 4)

The original thins its haze within about 5 s of control. Nothing in the data or scripts above
changes any fog number over time, so the cause is one of: a per-frame engine interpolation of the fog object (setter
`0x4da870`, fields `+0x20`/`+0x24`), a camera-side effect when the cinematic camera hands over to the
player camera (clip/horizon state, `CameraSetHorizon`), or a render-side distance fade that
depends on state the capture does not show. None is traced. Disassembly needs a disassembler this
host does not have (no `capstone`; installing global software is out of bounds), and an invented
ramp (time constant, start range) would be a fabricated tuning table.

## What is needed

* a disassembly of the fog setter `0x4da870` users, the per-frame fog update and the camera
  handover, or
* an original-run capture of fog range against time after the launch (`human_play`/owner capture),
  at least two frames with the same pose and a known tier (`VIEWING_RANGE`).

Affected content: M01 first ~20 s of free flight (island haze). Resolving tasks: this one, after the
above exist.
