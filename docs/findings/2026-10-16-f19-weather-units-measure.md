# F19-WEATHER-UNITS-MEASURE: `CLOUD_COVER` altitudes are metres; `FOG_COLOR` is normalized 0–1

Date: 2026-10-16. Task: #1460 / F19-WEATHER-UNITS-MEASURE "Measure
weather.zrd's CLOUD_COVER altitude unit and fog colour scale, then bind them"
(context: `docs/findings/2026-10-05-m01-lc-weather-bind.md`,
`docs/findings/2026-10-15-m01-play-weather-clouds-haze.md`). Inputs:
`$CS_ENGINE_IMAGE` (read-only, never committed) and `$CS_GAME_DIR`
(read-only). The original executable was not run; every claim below is
`static` — read from the owner-supplied decrypted image — never
`verified_original`. Implementer: `swe2-max-1`; no independent review yet.

## Measured claims

Each row names a claim and its virtual address(es) in the image.

| # | Claim | Address(es) | Provenance |
|---|-------|-------------|------------|
| 1 | `CLOUD_COVER`'s three scalars are parsed into the zone record **raw**, no unit scale: `TOP`→offset `+4` (default `280.0f`), `BOTTOM`→`+8` (default `240.0f`), `THICKNESS`→`+0xc`. | parser `0x4bc4c3`–`0x4bc7ba`; string xrefs `CLOUD_COVER` `0x62b4f8` | `static` |
| 2 | The per-frame cloud updater compares the caller-supplied altitude **directly** against stored `BOTTOM`/`TOP` with no conversion. The altitude is one component of a 3-float world position copied in by getter `0x4cf200` (camera position). | compare `0x42ee8e`, `0x42eef1`–`0x42ef1c` inside the updater rooted at `0x42e840`; getter `0x4cf200` | `static` |
| 3 | The compared component is world-space **Y**, and the world unit is the measured **metre** (`metres_per_unit = 1.0`, +Y up — engine-facts row at `0x48fc40`). Therefore `CLOUD_COVER.TOP`/`BOTTOM`/`THICKNESS` are **canonical world metres**. M01's `BOTTOM 1055` is a 1 055 m cloud floor. | engine-facts `0x48fc40` | `static` |
| 4 | A second consumer tests the same camera-Y against `BOTTOM` and `BOTTOM + 0.8·(TOP−BOTTOM)` — again raw file units vs world-Y. | `0x42b7cb`+; `0.8f` constant `0x6034b0` | `static` |
| 5 | The parser registers `center=(top+bottom)/2` and `center ∓ thickness/2` into cloud globals, later named `zone2_cloud_floor`. | `0x71dcd0`–`0x71dcdc`, `0x75d1b8` | `static` |
| 6 | The draw path keeps `bottom + (top−bottom)/2` (= the band midpoint, 1082.5 for M01 — exactly `ZONE1.FOG_ALTITUDE`'s high edge) and `midpoint ∓ thickness/2`: `THICKNESS` is the band's colour-transition half-width, not a second altitude. | `0x71c2cc`–`0x71c2d4` | `static` |
| 7 | `FOG_COLOR`'s triple reader takes each channel **raw**, compares it to `1.0f`: a channel **above** 1.0 is multiplied by `0x3b808081` (the constant at `0x6081b8`, `1/255`); a channel **at or below** 1.0 passes verbatim. So `FOG_COLOR` is a **normalized 0–1** float triple that also accepts byte spellings. | reader `0x4bc350`; constant `0x6081b8`; defaults `1.0f` in the parse `0x4bc4c3`–`0x4bc7a2` | `static` |
| 8 | The active fog colour lives in view-state floats that are blended in 0–1 space and clamped to `[0,1]` — float colours end-to-end until the renderer packs them for the device. | blend `0x64efc8` region; view-state `0x64ef78`/`0x64efc0`; clamp in the updater `0x42e840` | `static` |
| 9 | Adjacent parse code scales `SHADOW_ANGLES` by `π/180` (constant `0x6040e8`, shared with heading reads): angles are degrees in file, radians in memory. Recorded here for completeness; nothing binds it. | `0x6040e8` use in `0x4bc4c3`–`0x4bc7a2` | `static` |

## What the bind does with it

`cs_content::weather::bind_weather` now binds, both with the record's own
`ObservedTool` provenance over the `weather.zrd` source span:

- **`CLOUD_COVER.BOTTOM` → the one `CloudLayer`'s `altitude_m`** (claim 3).
  `TOP` and `THICKNESS` have no contract fields (`CloudLayer` carries an
  altitude and a coverage, nothing else) and stay named in the binding's
  unbound list as `CLOUD_COVER.TOP/THICKNESS`. The record authors no coverage
  fraction, so coverage stays `Unknown` — the drawn deck falls back to the
  labelled designed coverage rather than inventing one.
- **`ZONEn.FOG_COLOR` → `FogDefinition::color_linear`**, normalized per claim
  7 (`>1 → /255`, `≤1` verbatim), **only when every hardware-path zone**
  (`ZONEn`, never the `SW_ZONEn` software-render twin) **spells the same
  normalized triple**. M01's seven zones all spell `0.69`, so it binds
  `(0.69, 0.69, 0.69)`. When the zones disagree or none exist the colour
  stays `Unknown` and `ZONEn.FOG_COLOR` appears in the unbound list — the
  contract holds one fog colour, so a per-zone answer cannot be smuggled in
  as one.

`FOG_RANGES` and `FOG_ALTITUDE` stay unbound (`ZONEn.FOG_RANGES/FOG_ALTITUDE`
in the unbound list): they are near/far distances and altitude bands with no
per-metre density and no measured band consumer. `TOP_COLOR`/`BOTTOM_COLOR`
stay unbound: integer colours of unmeasured scale with no measured consumer.
Wind, sun, `CLIP_RANGES` and the rest are unchanged from #636.

## Residuals — recorded, not bound

- **Colour domain.** Claims 7–8 prove the record's channels are normalized
  0–1 floats, clamped and blended in float space. The eventual consumer is
  the fixed-function D3D fog colour (a byte-packed `D3DCOLOR`), which makes
  the spelled domain most plausibly display/sRGB; the environment contract's
  `color_linear` is linear RGB. No disassembly site in this measurement
  proves the pack step, so the bind preserves the file's own normalized
  values **verbatim** — auditable as "what the record said" — and this
  domain question stays a documented residual rather than an invented
  conversion. A later task that measures the pack site can decide whether a
  display-domain→linear conversion belongs.
- **`TOP`/`THICKNESS` consumers** (claims 5–6) are measured far enough to
  say `BOTTOM` is the floor and `THICKNESS` a transition half-width, but the
  environment contract has no fields for either; they stay named unbound
  rather than bound to a guessed meaning.
- **How the original renderer drew** `cloudlayer`/`cloud1`/`cloud2` remains
  unmeasured (the F19-B/#1440 limit); the drawn deck is the engine's own
  presentation, only its altitude — and now the haze's colour — is the
  record's.

## Verification

`accept_m01_play_weather_retail_m01_weather_is_bound_to_its_record_and_labelled`
(updated for this task) drives M01's own `weather.zrd` through the production
bind and asserts: one `CloudLayer` at `altitude_m == 1055.0`, the fog
`color_linear == [f64::from(0.69f32); 3]` with record provenance, density
still unknown, `TOP/THICKNESS` and the fog ranges still named unbound, the
drawn deck's altitude `Bound` to the record, its coverage designed, and the
haze `Bound` to the record's colour. The synthetic bind tests
(`accept_m01_lc_weather_bind_…`) pin the same rules on authored documents:
disagreeing zones bind no colour, a `SW_ZONE`-only colour binds no colour,
and byte-spelled channels divide by 255.
