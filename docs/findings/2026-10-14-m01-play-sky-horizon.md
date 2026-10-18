# #1434: the drawn sky, sea and fog for M01 and the retail playtest

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

Date: 2026-10-14. Task: #1434 `M01-PLAY-SKY`. Feature sheet:
`specs/F19-sky-atmosphere-weather-and-visibility.md` (non-negotiable 3 and 5).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used:
`retail` (read-only `$CS_GAME_DIR`) and ordinary build/test. Nothing was
rendered for a human, no original executable ran and nothing here is
`verified_original`.

## Files

- `crates/cs_app/src/render/horizon.rs` (new): `HorizonSpec` (the drawn
  values plus one [`HorizonOrigin`] per field), `sky_colour_at`, `dome_mesh`,
  `sea_mesh` and `HorizonPlugin` (spawn plus the per-frame placement, the
  camera's `DistanceFog`, the ambient colour and the clear colour).
- Wiring only: `crates/cs_app/src/render/mod.rs` (module declaration and
  re-export), `crates/cs_app/src/playtest/mod.rs` (`windowed_app` installs it
  when `retail` content was requested), `crates/cs_app/src/mission_session/
  compose.rs` (`add_composition` installs it on the windowed face from the
  stage's own `EnvironmentSession`).
- `crates/cs_app/tests/accept_m01_play_sky_horizon.rs`: seven
  `accept_m01_play_sky_` tests, one retail (`#[ignore = "requires
  CS_GAME_DIR"]`).

## What the original data holds (measured, read-only, from the owner's bytes)

| observation | source |
| --- | --- |
| `ZBD/C1C/gamez.zbd`'s own texture table (header `texture_count` = 279, table at `0x28`..`0x301c`) lists, in this order, `moon1`, `Sky1`, `cloud1`, `cloud2`, `water1`, `cloudlayer` among its 279 names | the container's own header and texture-name table (`textures_offset..materials_offset`, the table `cs_formats::gamez`'s reader walks), read with a throwaway script over the installation |
| the same container's node array holds 5 644 slots; slot 789 is named `horizon` with `node_type` 5 and **no** mesh index (`-1`); `cloudparent` appears in ~40 slots, `cloudsprite1`/`cloudsprite2` once each; **no** node is named `Sky1` | the node info array at `nodes_offset` (`0x1dfa8c`), slot 212 bytes, name in the first 36 |
| `ZBD/C1C/texture.zbd`'s strings include `horizonindicator`, `cloudlayer`, `water1`, `watersquirt` | `strings` over the archive (a lead only: the archive's index is what the production readers parse) |
| `ZBD/C1C/M01/zrdr.zbd`'s `weather.zrd` is 3 429 bytes, four zones, precipitation `RAIN`, and binds **no** fog colour, no ambient colour, no sky texture, no sea level and no distance (`SkyArt::missing`; fog/ambient are explicit unknowns) | `cs_app::environment::read_mission_weather`, asserted by `accept_m01_play_sky_retail_m01_horizon_is_bound_to_its_weather_and_labelled`; the unbound list is task #636's `docs/findings/2026-10-05-m01-lc-weather-bind.md` |
| the decrypted image holds `D:\zipper\Crimson\weather.cpp`, `CLOUD_COVER`, `zone2_cloud_floor`, `no_cloud`, `pfhorizon`, `CameraSetHorizon`, `ZBT_CAMDYN_CLOUDHACK`, `weather.zrd` and `fogvol.zrd` | `strings` over `$CS_ENGINE_IMAGE` (a lead for whoever binds the original's sky consumer) |

**Unknown, and therefore not drawn as if known:** how the original renderer
consumes the `Sky1`/`cloudlayer`/`water1` textures and the `horizon` node
(dome, quad, or per-frame gradient — unmeasured); the original sea level of
`c1c`; the colour space and unit convention of `weather.zrd`'s `FOG_COLOR`,
`FOG_RANGES` and `SUNLIGHT_*` (task #636 leaves them unbound for exactly that
reason); whether `FOG_ALTITUDE` is metres or world units.

## What is drawn, and where each value came from

Every value carries its origin in code (`HorizonOrigin`), and every field of
`HorizonSpec` can be asked:

| field | origin for M01 and `--playtest --world c1c` | value |
| --- | --- | --- |
| sky zenith colour | **designed** `M01-PLAY-SKY designed horizon, 2026-10-14` | sRGB `(0.21, 0.42, 0.72)` |
| sky colour at the horizon | **designed** (follows the fog colour by construction) | sRGB `(0.66, 0.75, 0.84)` |
| dome colour below the horizon | **designed** | the fog colour, exactly — the seam rule |
| sea colour | **designed** | sRGB `(0.07, 0.20, 0.36)` |
| sea level | **designed** (the original's water level is unmeasured) | −1 000 m |
| sea half extent | **designed** | 20 000 m |
| fog colour | **designed** (M01's `FOG_COLOR` is unbound, #636) | sRGB `(0.66, 0.75, 0.84)` |
| fog start / saturate | **designed** | 6 000 m / 20 000 m |
| dome radius | **designed** (inside the play camera's 40 km far plane) | 24 000 m |
| ambient colour | **designed** (M01's `SUNLIGHT_*` is unbound, #636) | sRGB `(0.72, 0.79, 0.87)` |
| ambient brightness | carried from the playtest's own presentation | 400.0 |

**Original, in the sense this task can claim:** the *structure* comes from the
spec (F19 behaviour 3: a dome centred on the camera's world translation that
no camera roll and no origin rebase may rotate), and the values a composed
mission draws are read through the production `weather.zrd` bind of task #636
— `HorizonSpec::from_environment(stage.host.environment.definition())`. For
M01 that record leaves every colour unknown, so the retail test asserts that
the horizon is labelled designed **end to end**: no drawn number can be
mistaken for an original measurement.

The designed palette is a development choice (it is allowed by the task:
"Where a value is unknown, use a designed value labelled as such"), it is
labelled in code, in this file and in every `HorizonOrigin::Designed`
payload, and it is **not** a claim that the original sky is this colour.

## How it is drawn

- **Sky**: one unlit, unfogged, unculled unit sphere scaled to 24 km, whose
  per-vertex colour is `sky_colour_at` of the vertex's own elevation: fog
  colour at and below the horizon, blending to the zenith colour above. It is
  re-centred on the camera every frame with an identity rotation.
- **Sea**: one unlit flat quad, 40 km across, at −1 000 m, kept horizontally
  under the camera. It reaches past the fog's far distance, so its edge is
  never an edge.
- **Fog**: `DistanceFog { Linear { 6 000 m, 20 000 m } }` on the camera, in
  the same colour the dome paints below the horizon, so the sea, the world
  and the sky meet in one haze. Ambient light takes the designed haze colour
  at the playtest's existing brightness.

The three distances are ordered on purpose (`accept_m01_play_sky_the_fog_
saturates_inside_the_dome_and_the_sea_reaches_past_it`): fog saturates inside
the dome, and the sea reaches past the fog, so neither the dome's edge nor the
sea's edge can be seen.

## What is not claimed

- No original sky art is drawn: `SkyArt::missing` for M01 stays a diagnostic
  (F19 behaviour 5) and this stage paints a labelled design instead. Binding
  the authored `Sky1`/`water1` textures needs the mapping the original
  renderer used, which is unmeasured — follow-up below.
- No original fog, ambient, sea or cloud value is drawn, for the reason in
  the unknowns table above.
- Nothing here was shown to a human; `human_play`/`human_review` remain the
  owner's, and a Rally merge awards at most `checked`.

## Follow-ups

- #1440 `M01-PLAY-WEATHER` (already queued, depends on this task): clouds and
  haze from `weather.zrd`.
- Binding the authored sky and sea art (`Sky1`, `water1`, `cloudlayer` in
  `ZBD/C1C/gamez.zbd`) once the original's consumption of them — and the
  `horizon` node — is measured from the image or from an owner-supplied
  original run.
