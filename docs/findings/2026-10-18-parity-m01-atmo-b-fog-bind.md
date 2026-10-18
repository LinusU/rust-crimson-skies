# #1649 PARITY-M01-ATMO-B: M01's flight-zone fog drives the drawn horizon

Date: 2026-10-18. Implementer: `claude-1` (Sonnet 5.5); no independent review yet.
Facts are those of `2026-10-18-parity-m01-atmo-a-weather-units-and-sky-consumers.md`
(all `static`, nothing `verified_original`; the original executable was not run).

## What is bound (`HorizonSpec::bind_weather`, from `ZBD/C3/M01` `weather.zrd`)

| drawn value | source | M01 value |
| --- | --- | --- |
| fog colour (and the dome's horizon band and under-horizon colour) | `ZONE1.FOG_COLOR`, normalized as the reader `0x4bc350` does | (0.79, 0.79, 0.79) |
| fog start / end, metres | `ZONE1.FOG_RANGES` times the first `VIEWING_RANGE` tier's `fog_scale` (`0x472ea0`) | 1000 / 4500 (tier `HIGH`, 1.0) |
| ambient colour | `ZONE1.SUNLIGHT_COLOR_AMBIENT` | white |

`ZONE1` is the zone below the cloud band's midpoint (10 500 m), i.e. all of M01's flight.
Each bound field carries a `Provenance` (`observed_tool`, span of `weather.zrd`).

## What stays designed, and why

* **Viewing tier choice** (`HIGH`, the first): which tier the original selects at run time is unmeasured. For M01 `HIGH` is 1.0, so the choice is only visible in `MED` (0.85) and `LOW` (0.7).
* **Colour space**: the triple is drawn as display values (as era fixed-function fog took it); atmo-A §6c leaves display-vs-linear open. The older `from_environment` path treats a bound colour as linear and is unchanged.
* `FOG_ALTITUDE` (renderer use unmeasured, band above M01's flight), `ZONE2` (above 10.5 km), `CLIP_RANGES` (the camera far plane stays the playtest's), sky zenith, sea colour, dome radius, ambient *brightness* (the engine's `SUNLIGHT_AMBIENT` has no measured relation to Bevy's scale).
* The sky art itself (`h_zone2scroll`, `g1155`, `sun`), sea tiles and clouds are #1650, #1651 and #1643.

## Smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir private/evidence/PARITY-M01-ATMO-B`: 13 checks, 0 failed (screenshots stay in `private/`). The terrain is now hazed to the grey well inside 4.5 km, which the world probe could not see through; the smoke's probe now parks the distance fog while it isolates a class (`mission_session/smoke.rs`), as it already hides the dome and weather.
