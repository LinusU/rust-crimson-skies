# #636: `weather.zrd` bound to `EnvironmentSession`

Date: 2026-10-05. Task: #636 `M01-LC-WEATHER-BIND`, the step `VS-M01-RUNTIME`
(#359) waits on. Feature sheet: `specs/F19-sky-atmosphere-weather-and-visibility.md`
(non-negotiable behavior 1). Capabilities used: `retail` (read-only
`$CS_GAME_DIR`) and ordinary build/test. Nothing was rendered or run, so nothing
here is `verified_original`.

## Files

- `crates/cs_content/src/weather.rs` (new): `WeatherDocument::from_zrd` (strict
  reader), `bind_weather`, `UnboundField`.
- `crates/cs_app/src/environment/retail.rs` (new): `read_mission_weather`,
  `RetailWeather::session`.
- Wiring only: `crates/cs_content/src/lib.rs`, `crates/cs_app/src/environment/mod.rs`.
- `crates/cs_app/tests/accept_m01_lc_weather_bind.rs`: five
  `accept_m01_lc_weather_bind_` tests, two retail.

## The measurement

Every mission scope (53: `zbd/<group>/<mission>/zrdr.zbd`) carries one
`weather.zrd`, 3218-3442 bytes. All 53 decode through the production `.zrd`
grammar reader and parse under the strict reader; no key outside the table below
occurs.

| record | keys | notes measured over the 53 |
| --- | --- | --- |
| `VIEWING_RANGE` | `HIGH`/`SWHIGH`/`MED`/`SWMED`/`LOW`/`SWLOW`, each `CLIP_SCALE [x]`, `FOG_SCALE [x]` | all 53 |
| `WIND` | `STATIC_VELOCITY [x y z]`, `RANDOM_MAX_SPEED`, `RANDOM_ACCEL`, `RANDOM_ANG_VEL` | `STATIC_VELOCITY` is `[0.0 2.0 0.0]` in **all 53** |
| `CLOUD_COVER` | `TOP`, `BOTTOM`, `THICKNESS`, optional `TOP_COLOR`/`BOTTOM_COLOR` (integer rgb, 30 docs) | |
| `ZONE1..3`, `SW_ZONE1..3` | `FOG_COLOR`, `FOG_RANGES`, optional `FOG_ALTITUDE`, `CLIP_RANGES`, `SUNLIGHT_*` (8 keys) | `ZONE2` in 45, `ZONE3` in 8; `FOG_ALTITUDE` absent in 18 zones |
| top level | `SHADOW_ANGLES`, optional `TYPE` + `COLOR`, `WIND_DIR`, `WIND_VEL`, `GRAVITY`, `ALPHA_GRADIENT`, and `PARTICLES` | `TYPE` in 14: 9 `RAIN` (with `PARTICLES`), 5 `SNOW` (without) |

The same field is spelled both as integers and floats: `FOG_ALTITUDE` as ints in
36 documents, `FOG_COLOR` as ints (`[192 192 192]`) in 17, and
`SUNLIGHT_ORIENTATION` as wrapped `u32` (`4294967271` = `-25` as `i32`) in 8.
The reader keeps the spelling (`WeatherScalar`). Strictness found three variants
a first draft got wrong (optional `FOG_ALTITUDE`, `PARTICLES` absent for snow),
which is why it refuses unknown, duplicate, missing and misshapen keys by name.

## What is bound

| input | binding |
| --- | --- |
| precipitation kind | `TYPE` `RAIN`/`SNOW`: **known**, `observed_tool`, source span = the member. No `TYPE`: **known clear**, `inferred` (absence is read as none) |
| environment identity / origin / profile | `<mission>.weather`, `Origin::Installation`, `Retail` |

M01 (`zbd/c1c/m01`, 3429 bytes) is a rain mission with four zones.

## What stays unknown, and why (every one named by `UnboundField`)

The original's world-unit scale and its angle, axis and color conventions are
unmeasured (`2026-09-30-f18-b-world-import-and-static-collision.md`,
`2026-09-30-f19-wind-conversion-ownership.md`), so:

- **Wind** (`WIND.STATIC_VELOCITY`): unknown in the definition. `EnvironmentSession::wind`
  returns `WindUnavailable::Unknown`; it is never still air. The raw `[0, 2, 0]`
  stays in the document. The three `RANDOM_*` fields have no consumer.
- **Gameplay visibility**: unknown. `FOG_RANGES`/`CLIP_RANGES` are renderer
  values and are never converted (F19 behavior 1).
- **Sun direction, ambient, sky orientation, fog density/color**: unknown
  (angle order/axes, intensity scale, color space, near/far versus per-meter).
- **Cloud layers**: none emitted (`CloudLayer` takes a metric altitude).
- **Sky texture**: `weather.zrd` names none; `SkyArt::missing`.
- **`SW_ZONEn`, `VIEWING_RANGE`, `SUNLIGHT_ACTIVE/STATIC/BICOLORED`, particle
  tuning**: no consumer; meaning unmeasured. `SW_` as "software render" is a
  guess and is not relied on.

## Follow-ups

Resolving the unit scale and angle conventions (an original capture or a
measured motion record) is what lets the wind and sun bind known; that is
outside this task and the unbound list is the work list for it.
