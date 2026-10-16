# #1459: the drawn precipitation for M01 and the windowed scene

Date: 2026-10-16. Task: #1459 `M01-PLAY-PRECIPITATION`. Sheet:
`specs/F19-sky-atmosphere-weather-and-visibility.md` (non-negotiable 1 and 2,
AC03). Capability used: `retail` (read-only `$CS_GAME_DIR`). **No `gpu`
claim**: no windowed capture or evidence report was produced for this task;
the drawing is proven headless through the production plugin. Nothing is
`verified_original`; no original executable ran.

## Files

- `crates/cs_app/src/render/precipitation.rs` (new): `PrecipitationSpec`,
  `PrecipitationPlugin`, `lattice_origin`.
- Wiring only: `render/mod.rs`, `playtest/mod.rs` (installs `none()`: the
  original-assets playtest carries no environment record),
  `mission_session/compose.rs` (installs `from_session` on the windowed face).
- `crates/cs_app/tests/accept_m01_play_precipitation.rs`: six
  `accept_m01_play_precipitation_` tests, one retail.

## Behaviour

`weather.zrd` M01: `TYPE RAIN`, `COLOR (128,128,128)`, `PARTICLES 100`. The
bind (task #636) keeps only the kind; `COLOR` and `PARTICLES` are unbound
(unmeasured scale). The retail test shows M01 resolves to `Rain`. A clear
record draws no entity; an unknown kind draws nothing and is reported as
`Undrawn::UnknownKind`, never turned into rain or a clear sky.

## Drawn-field origin table

| field | origin | value |
| --- | --- | --- |
| kind | **bound** (record's `TYPE` claim, its provenance) | rain / snow |
| colour | designed | `DESIGNED_COLOUR`; record's `COLOR` not used |
| size | designed | rain 0.025 x 1.4 m streak, snow 0.12 m flake |
| fall speed | designed | rain 9 m/s, snow 1.5 m/s |
| count | designed | the cosmetic field's 16 particles x 27 lattice repeats; record's `PARTICLES 100` not used |
| placement | designed (cosmetic stream offsets, tiled every 120 m) | no RNG drawn here |
| impact | designed: none drawn | nothing measured a splash rule |

## Known limitations

- The authoritative wind does not advect the drawn particles yet (the fall
  is straight down). Affects every rainy mission's look only, not gameplay.
- The lattice tiling repeats the 16 cosmetic offsets every 120 m; a visible
  pattern is possible. Designed.
- No windowed (`gpu`) capture was taken; a reviewer or owner should look.
- Precipitation is read once at composition, so a timeline event that changes
  precipitation mid-mission is not redrawn.
