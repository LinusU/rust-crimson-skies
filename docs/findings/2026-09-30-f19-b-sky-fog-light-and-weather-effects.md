# F19-B: sky, fog, light and weather effects

Task: #89, stage `### F19-B` of
`specs/F19-sky-atmosphere-weather-and-visibility.md`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`; the air-relative sign convention comes
from `docs/contracts/FLIGHT-PHYSICS.md`, "Coordinate convention".

Stage F19-A defined *what an environment says*. This stage implements *what
that record does* — the smallest production path that exercises the sheet's
declared behavior, with no ECS, no Bevy types and no renderer.

## Files and the one observable failure (the slice plan)

Everything below is inside the task's owner paths.

| File | What it owns |
| --- | --- |
| `crates/cs_app/src/environment/air.rs` | `AuthoritativeWind`, `ProjectileMotion` — the one wind field, the `v_air = v_world - wind_world` conversion, the refusal of an unknown wind |
| `crates/cs_app/src/environment/effects.rs` | `SkyEffect`, `FogEffect`, `LightEffect`, `CloudLayerEffect`, `EnvironmentEffects` — what a frame may be drawn from |
| `crates/cs_app/src/environment/cosmetic.rs` | `CosmeticField`, `PrecipitationEffect` — the decorative particle field and its own stream |
| `crates/cs_app/src/environment/mod.rs` | module declarations and re-exports (wiring only) |
| `crates/cs_app/src/lib.rs` | the crate-level doc paragraph for `environment` (doc comment only) |
| `crates/cs_app/tests/environment/{air,effects,cosmetic,common,main}.rs` | the `accept_f19_b_` tests and the F19-B test fixtures |
| `docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md` | this document |

The one observable failure the slice had to fix: with F19-A's records in
place there was **no path from an environment state to anything a frame or an
aircraft could use**. `EnvironmentState::wind` was a field nothing read;
`FlightEnvironment::wind_velocity_mps` was a field nothing wrote; and a
renderer had no value for the sky, fog or sun. A wind change on the storm
fixture's timeline changed a record and nothing else.

## What the stage implements

### The authoritative wind (AC02, non-negotiable behavior 2)

`AuthoritativeWind::from_state` lifts the single `WindField` out of the state
the timeline replaced and **refuses to exist** while that field is an
explicit unknown (`WindUnavailable::Unknown { claim_id, reason }`). There is
no still-air default anywhere in the chain, because "unknown wind" and "zero
wind" are different records and only the second one is a wind.

`air_relative` is the one conversion, and it is exactly the
`FLIGHT-PHYSICS` convention component-wise. `flight_environment` hands the
field to the real `cs_sim::flight::FlightModel` through `FlightEnvironment`,
so the model subtracts the environment's own wind rather than a second
approximation of it. The acceptance test measures the model's
`InstrumentState::airspeed_mps` and compares it to `AuthoritativeWind::airspeed_m_s`
for three different winds; they agree exactly and the value changes with the
wind.

`ProjectileMotion` carries a **constant air-relative velocity** and nothing
else — no drag, no gravity, no wind shear. What it owns is the coupling
AC02 asks about:

* `world_velocity_m_s` is `air_velocity + wind`, so two winds move a projectile
  by exactly their own difference;
* `closing_speed_m_s` is a relative quantity *inside one air* and takes no
  wind at all: `closing_speed == -closing_speed(-Δwind, target_air)`;
* `world_closing_speed_m_s` is the world-space closure and **does** depend on
  the wind, because a target whose world velocity is held fixed while the air
  speeds up is genuinely being blown at a different rate. The distinction is
  documented in both directions, because collapsing them is the classic
  inconsistency.

### Sky, fog and light

`SkyEffect::resolve(sky, run_profile)` is the renderer decision, and it is
`SkyArt::allows_generated_sky` asked with the **run's** profile. Both halves
matter and the tests pin both: a run label alone does not license a generated
sky (the storm record asks for a diagnostic, so a synthetic run still gets
one), and a generated sky under the retail profile is unreachable from
`SkyEffect`. The `Generated` payload carries the profile, so a renderer cannot
lose track of what it is drawing.

`FogEffect` keeps each field's own resolution state and offers two products:
`transmittance_at` (exponential, `1.0` at the camera) and `fade_toward`
(`surface * T + colour * (1 - T)`). It has **no** gameplay-visibility accessor
and no use for one (behavior 1): a known density with an unknown colour can
fade but cannot tint, an unknown density is refused by claim rather than read
as "no fog", and the test asserts that the screen is already fogged at the
storm's authored 400 m sight range while the sight range stays its own
authored value.

`LightEffect::rig()` returns `Some` only when **both** the sun direction and
the ambient term are known. An unknown sun, an unknown ambient, and half a rig
are all `None`, because a default sun or a default ambient would be a renderer
choice masquerading as an authored one.

`CloudLayerEffect::coverage()` is `None` for an unknown coverage rather than
`1.0`: a layer nobody measured is drawn at no coverage, not at full opacity.

`EnvironmentEffects` gathers one frame from a definition plus a **state** and
a cosmetic seed. The state is a separate parameter on purpose — sky art, fog
and lighting are authored once, precipitation is what a `WeatherEvent`
replaces — and `from_clock` reads it from the clock the timeline runs on, so
the frame cannot draw weather the timeline has already moved past.

### Cosmetic precipitation

`CosmeticField` draws three particles' worth of components per particle from
`CosmeticWeatherSeed`'s stream and nothing else; `COSMETIC_WEATHER_DOMAIN` is
that domain, distinct from `SYNTHETIC_BODY_DOMAIN`. Particles are then
advected **only** by the authoritative wind. That split is what makes "the
decoration follows the weather, the decoration's randomness does not depend on
it" observable: a gust shifts every drifted particle by exactly
`Δwind * elapsed`, while the drawn field is bit-identical in both weathers.
Only authored precipitation decorates: `Clear` draws nothing, an unknown kind
draws nothing and reports its claim.

## Test sensitivity

Twelve `accept_f19_b_*` tests, all in `crates/cs_app/tests/environment/`. They
drive production code only — `cs_content::environment`'s records,
`cs_sim::flight`'s real `FlightModel`, and the three new
`cs_app::environment` modules. No test carries its own environment builder,
wind conversion, fog curve or particle draw.

Eight defects were injected one at a time and reverted; each was caught:

| injected defect | test that failed |
| --- | --- |
| `flight_environment` kept the base wind instead of this field | `accept_f19_b_wind_changes_aircraft_airspeed_and_projectile_velocity_consistently`, `..._air_relative_velocity_is_one_lossless_field_conversion`, `..._the_wind_the_timeline_installed_is_the_wind_consumers_read` |
| `air_relative` dropped the wind's x component | the same three |
| `from_state` turned an unknown wind into still air | `accept_f19_b_an_unknown_wind_is_refused_and_never_becomes_still_air` |
| `SkyEffect::resolve` generated under the retail profile | `accept_f19_b_a_missing_sky_texture_is_a_diagnostic_and_only_a_labelled_run_generates_one` |
| `EnvironmentEffects::from_clock` read the definition's initial state | `accept_f19_b_frame_effects_follow_the_weather_timeline_and_not_the_definition` |
| an unknown cloud coverage became `1.0` | `accept_f19_b_cloud_layers_keep_their_own_coverage_and_never_invent_one` |
| `CosmeticField` drew from `SYNTHETIC_BODY_DOMAIN` | `accept_f19_b_cosmetic_particles_are_drawn_only_from_the_cosmetic_weather_stream` |
| the decoration was advected by a decorative wind of its own | `accept_f19_b_a_gust_advects_the_particles_without_moving_their_draws` |
| a clear state decorated anyway | `accept_f19_b_only_authored_precipitation_is_ever_drawn`, `..._frame_effects_follow_the_weather_timeline_...` |
| an unknown fog density read as zero | `accept_f19_b_fog_fades_the_frame_and_never_becomes_a_sight_range` |
| a half-known rig was completed from defaults | `accept_f19_b_light_uses_the_authored_sun_and_never_invents_one` |

The first cosmetic defect needed a stronger test: "the field is deterministic
and differs from another seed's" does **not** discriminate the domain. The test
now replays `CosmeticWeatherSeed::stream()` itself and asserts the field is
exactly those draws — which is what pins the domain.

## Commands run (exit codes)

```text
cargo fmt --all -- --check                                       → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                  → 0
cargo test --workspace --locked -- accept_f19_b_ --include-ignored → 0 (12 tests, all passed)
```

Nothing here is `#[ignore]`d and nothing reads `CS_GAME_DIR`: every value is
authored development data, so the task selection runs in CI unchanged.

## Sheet acceptance cases

| case | status here |
| --- | --- |
| AC01 rebase under a fixed horizon, sky and sun stable | covered by the F19-A tests (`sky.rs`); this stage consumes `SkyFrame` and asserts the light rig carries the same sun the frame does |
| **AC02 wind changes affect aircraft airspeed and projectile-relative velocity consistently** | this stage's minimum scenario, in `air.rs` |
| AC03 weather seeds do not change mission AI RNG sequences | **not claimed here.** It is F19-C's minimum scenario and needs the AI consumer that does not exist yet. What this stage does establish is the half it owns: the cosmetic stream is domain-separated and a wind change moves no decorative draw |
| AC04 the original environment states actually present in each world/scenario | **not claimed here.** F19-D, `gpu` + `retail` |

## Designed vs measured: the unknowns this stage met

Everything in this stage is **designed** engine contract. The following are
`unknown`, recorded rather than guessed:

| unknown | evidence | resolves in |
| --- | --- | --- |
| Whether the original stores an environment record at all, its layout, its addressing and its units | no original bytes were read for this task | F19-D and any F19 format stage |
| The original's sun, ambient, fog and wind tuning values | no measurement exists; both fixtures author `designed` or explicitly unknown values | F19-D against private captures |
| Whether a projectile is ballistic, guided or straight at all in the original, and whether wind affects it | no original weapon or projectile dynamics exist in this workspace yet; `ProjectileMotion` models constant air-relative velocity and says so | a weapon/dynamics stage, then F19-D |
| Whether the original's precipitation affects flight at all, and with what profile | nothing measured; this stage deliberately draws **no** rain/snow fall-speed or drift tuning, so the two kinds share one authored-agnostic field | F19-D |
| Whether the original applies fog as an exponential transmittance, a lookup or a fixed curve | the exponential here is a declared presentation model, not a reproduction | F19-D |
| What particle count and camera-relative volume the original uses for decorative weather | `COSMETIC_PARTICLE_COUNT` and `COSMETIC_FIELD_HALF_EXTENT_M` are declared presentation design | F19-D |
| Where environment records belong in the canonical catalog | inherited from F19-A: `ContentKind` has no environment namespace, tracked as #407 `F19-A-CATALOG-KIND` | #407, owner decision |
| Whether a retail run may substitute anything for a missing sky texture | this stage answers "no" from the sheet; the original's own behaviour is unmeasured | F19-D |

## What is not claimed

A code/test pass awards at most **checked**. No original environment data was
read, no original sky, fog, light or weather behavior was reproduced, and
nothing here is `verified_original`. No renderer draws a sky from these
records and no ECS component holds an environment state: F19-C wires the
records into their real producer and consumer, and F19-D is the stage that may
compare them against original captures. Synthetic fixtures alone cannot
certify original-data behavior.