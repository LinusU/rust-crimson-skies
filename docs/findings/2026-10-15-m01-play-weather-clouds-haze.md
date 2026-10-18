# #1440: the drawn cloud deck and haze for M01 and the retail playtest

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

Date: 2026-10-15. Task: #1440 `M01-PLAY-WEATHER`. Feature sheet:
`specs/F19-sky-atmosphere-weather-and-visibility.md` (non-negotiable 1 and 3).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used:
`retail` (read-only `$CS_GAME_DIR`) and `gpu` (two real windowed runs on this
machine). Nothing was rendered for a human, no original executable ran and
nothing here is `verified_original`.

## Files

- `crates/cs_app/src/render/weather.rs` (new): `WeatherSpec` (the drawn layers
  plus one `WeatherOrigin` per field), `WeatherLayerSpec` (`mesh` and the
  alpha profile it is built from), `designed_cloud_field` and `WeatherPlugin`
  (spawn plus the per-frame placement under the camera).
- Wiring only: `crates/cs_app/src/render/mod.rs` (module declaration and
  re-export), `crates/cs_app/src/playtest/mod.rs` (`windowed_app` installs it
  beside the horizon when `retail` content was requested),
  `crates/cs_app/src/mission_session/compose.rs` (`add_composition` installs it
  on the windowed face from the stage's own `EnvironmentSession`).
- `crates/cs_app/tests/accept_m01_play_weather.rs`: seven
  `accept_m01_play_weather_` tests, one retail (`#[ignore = "requires
  CS_GAME_DIR"]`).
- `crates/cs_app/tests/evidence_report_m01_play_weather.rs`: the
  evidence-report harness (`docs/contracts/CLI-EVIDENCE.md`), not an
  acceptance test.

## What the original data holds (measured, read-only, from the owner's bytes)

M01 is `zbd/c1c/m01`; its `zrdr.zbd` member `weather.zrd` is 3 429 bytes and
parses through the production reader of task #636
(`cs_app::environment::read_mission_weather`). Read on 2026-10-15:

| observation | value |
| --- | --- |
| `CLOUD_COVER.TOP` / `.BOTTOM` / `.THICKNESS` | `1110.0`, `1055.0`, `30.0` (file units; **no** `TOP_COLOR`/`BOTTOM_COLOR`) |
| `ZONE1.FOG_COLOR` / `.FOG_RANGES` / `.FOG_ALTITUDE` | `(0.69, 0.69, 0.69)`, `[1000, 1750]`, `[1055, 1082.5]` |
| `ZONE2.FOG_COLOR` / `.FOG_RANGES` / `.FOG_ALTITUDE` | `(0.69, 0.69, 0.69)`, `[1000, 4000]`, `[4000, 5000]` |
| precipitation | `TYPE RAIN`, `COLOR (128, 128, 128)`, `PARTICLES 100` |
| zones | four (`ZONE1`, `SW_ZONE1`, `ZONE2`, `SW_ZONE2`); `ZONE1`'s altitude band is the cloud band |

`ZONE1`'s `FOG_ALTITUDE` `[1055, 1082.5]` sits inside `CLOUD_COVER`'s
`BOTTOM 1055 … TOP 1110`, which is a **lead** for whoever measures the unit —
not a unit this task may assume.

**Unknown, and therefore not drawn as if known:** the unit of those altitudes
and the colour scale of those numbers (task #636 leaves the whole record
unbound for exactly that reason); how the original renderer consumed the
`cloudlayer`, `cloud1` and `cloud2` textures and the `horizon` node of
`ZBD/C1C/gamez.zbd`; and how the 2000 engine turned `CLOUD_COVER` into a drawn
layer. The decrypted image holds `weather.cpp`, `CLOUD_COVER`,
`zone2_cloud_floor`, `no_cloud` and `ZBT_CAMDYN_CLOUDHACK` — leads for that
work, not a layout.

The bind itself is unchanged: `bind_weather` still emits **no** `CloudLayer`,
because `CloudLayer` takes a metric altitude and nobody measured the record's
unit. `M01-PLAY-WEATHER` does not change that; it names the fields in the
unbound list, as before.

## What is drawn, and where each value came from

Every value carries its origin in code (`WeatherOrigin`), and every field of
every drawn layer can be asked. For M01 the whole record is unbound, so the
drawn weather is labelled designed end to end:

| field | origin for M01 and `--playtest --world c1c` | value |
| --- | --- | --- |
| deck altitude | **designed** `M01-PLAY-WEATHER designed weather, 2026-10-15` | 2 400 m |
| deck coverage | **designed** | 0.55 |
| deck colour | **designed** | sRGB `(0.86, 0.88, 0.92)` |
| layer half extent | **designed** | 20 000 m |
| haze altitude | **designed** (M01's `FOG_ALTITUDE` bands are unbound) | 300 m |
| haze coverage | **designed** | 0.35 (× 0.35 overhead) |
| haze colour | **designed** (M01's `FOG_COLOR` is unbound) | sRGB `(0.66, 0.75, 0.84)` |
| mesh resolution | **designed** | 64 cells per axis (625 m each) |

**Original, in the sense this task can claim:** the *structure* is the record's
own — `weather.zrd` authors a `CLOUD_COVER` band and per-zone fog — and the
production path binds a record's own layer when a record authors one:

- an environment whose `EnvironmentDefinition` carries cloud layers has **one
  drawn deck per layer**, at that layer's own altitude (`Bound`) and with its
  own coverage (`Bound`), the latter falling back to the labelled design when
  the record leaves the coverage unknown;
- the haze layer takes the record's `FOG_COLOR` verbatim (`Bound` with its own
  provenance) whenever the record carries one, so the veil, the fog and the
  dome's colour below the horizon stay one value.

Fixtures exercise both halves; the acceptance suite asserts them
(`accept_m01_play_weather_an_authored_cloud_layer_reaches_the_drawn_deck`,
`..._a_record_with_no_layer_falls_back_to_the_labelled_design`), and for M01
the retail test asserts that the drawn weather is labelled designed **end to
end** (`..._retail_m01_weather_is_bound_to_its_record_and_labelled`).

The designed palette is a development choice (the task allows it: "Use
designed visuals where the original rendering is unknown, labelled as such"),
it is labelled in code, in this file and in every `WeatherOrigin::Designed`
payload, and it is **not** a claim that the original's clouds are this shape,
this altitude or this colour. In particular the 2 400 m has **not** been copied
from the record's `TOP 1110`: copying a number whose unit is unknown would be
the guess rule 4 forbids.

## How it is drawn

- **A deck is a broken layer, not a sheet.** Its mesh is one grid whose every
  vertex carries the layer's colour in linear space and its own alpha: the
  designed field `designed_cloud_field` (three sine lobes whose coefficient
  pairs are `(3, 2)`, `(2, 5)` and `(7, 3)`), painted wherever it is above a
  threshold, with a soft edge (`CLOUD_PATTERN_EDGE` 0.06). The threshold is the
  field's own quantile at `1 - coverage` **over that layer's own grid**, so the
  share of the deck the pattern paints — measured over that same grid — *is*
  the layer's coverage: a 0.55 deck paints a little over half of its own grid
  and leaves the rest of the `M01-PLAY-SKY` sky visible, and the same code
  draws a 0.25 record layer as a thin broken layer.
- **The haze is a profile, not a sheet.** Its alpha runs from
  `HAZE_OVERHEAD_FRACTION` (0.35) of the coverage directly over the camera to
  the coverage itself at the layer's edge, where the line of sight through it
  is longest — the shape a real haze has, and the reason it does not flatten
  the sky above the aircraft.
- Both layers are unlit, translucent (`AlphaMode::Blend`), drawn from both
  sides, and fog-enabled, so their far parts fade into the same haze the
  horizon draws. Each reaches past the fog's far distance, so a layer's edge is
  never a visible line.
- Both are re-placed every frame at the camera's horizontal position and their
  own world altitude, with no rotation: rolling or climbing cannot tilt or drag
  a layer (F19 non-negotiable 3).

Ordering is asserted, not assumed
(`accept_m01_play_weather_the_layers_sit_above_the_water_and_inside_the_haze`):
the haze is above the sea the horizon draws and under the deck, the deck is
inside the dome's radius, every layer reaches past the fog's far distance, and
the designed haze is exactly the colour the designed dome paints below the
horizon.

## The captures

Both runs are real windowed GPU runs on this machine (`Apple M3 Pro`, Metal);
they are automated, are never `human_play`, and no original executable ran.
Their framebuffers and traces stay under `private/evidence/M01-PLAY-WEATHER/`
(Git-ignored); only `docs/findings/evidence/M01-PLAY-WEATHER.json` — paths and
hashes — is committed.

- `cs --playtest --cs-path $CS_GAME_DIR --world c1c --smoke-seconds 20
  --capture-dir private/evidence/M01-PLAY-WEATHER/playtest` — six frames of the
  original-assets scene. `frame_00150_pitch_up.png` (39.5° nose-up) shows the
  cloud masses with clear sky between them; `frame_00030_initial.png` shows the
  same deck over the horizon band and the sea, with the airship in frame.
- `CS_MISSION_CAPTURE=1 CS_EVIDENCE_DIR=private/evidence/M01-PLAY-WEATHER cs
  --cs-path $CS_GAME_DIR --mission M01` — `frame_00180_player.png`, the composed
  mission climbing through ~1 300 m: the deck is above the aircraft and the haze
  is below it, so the mission draws its weather where the layer altitudes say
  it is. (The composed run is real-time, so the altitude and the frame bytes
  differ slightly between runs; the playtest capture above is deterministic.)

## What is not claimed

- No original cloud, haze, fog or sky value is drawn: the record's own numbers
  stay unbound and every drawn value is labelled designed.
- No original art is drawn. Binding `cloudlayer`/`cloud1`/`cloud2` needs a
  measured mapping the 2000 engine used, which does not exist yet.
- The pattern is this build's design. Nothing about its shape, its 625 m cells
  or its soft edge is a measurement.
- `M01-PRESENTATION` is not satisfied by any of this: the frames show a
  development presentation, not the original's, and no human looked at them.
- Nothing here was shown to a human; `human_play`/`human_review` remain the
  owner's, and a Rally merge awards at most `checked`.

## Follow-ups

- Binding the authored cloud art (`cloudlayer`, `cloud1`, `cloud2`) and the
  record's own `CLOUD_COVER` band, once the original's consumption of them — and
  the file's altitude and colour units — are measured from the image or from an
  owner-supplied original run. That work would replace
  `WeatherLayerSpec::designed_cloud`'s labelled values with `Bound` ones on the
  very same path.
- The whole `weather.zrd` unbound list (task #636) remains the work list for
  binding M01's fog colour and ranges (the haze layer's colour and coverage).
