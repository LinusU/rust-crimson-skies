# F19-A: environment data and time domains

Date: 2026-09-30. Task: F19-A "Define environment data and time domains"
(`specs/F19-sky-atmosphere-weather-and-visibility.md`, section `### F19-A`).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no evidence report
required, no GPU.

## Files and the one observable failure (the slice plan)

* `crates/cs_content/src/environment.rs` (new): the Bevy-free typed
  contract — `EnvironmentId` (a validated subordinate key), the
  `EnvironmentProfile` (`Retail` \| `SyntheticDeveloper`), `SkyArt` +
  `SkyFallback`, `SkyOrientation`, `LightingDefinition`, `FogDefinition`,
  `CloudLayer`, `PrecipitationDefinition`, `WindField`,
  `GameplayVisibility`, the `EnvironmentState` they live in, the
  `WeatherEvent`/`EnvironmentTimeline` schedule in whole ticks, the
  validated `EnvironmentDefinition` and its `record_fingerprint`.
* `crates/cs_sim/src/visibility.rs` (new): `ENVIRONMENT_TIME_DOMAIN`,
  `environment_clock_policy`, the generic `VisibilityEvent<T>` /
  `VisibilityTimeline<T>` runner on the authoritative-gameplay clock.
* `crates/cs_app/src/environment/frame.rs` (new): `SkyFrame::capture`,
  `SkyFrame::from_local`, `SkyFrame::is_centered_on`.
* `crates/cs_app/src/environment/clock.rs` (new): `EnvironmentClock`, the
  wiring from a definition's authored timeline onto
  `cs_sim::visibility::VisibilityTimeline`.
* `crates/cs_app/src/environment/fixture.rs` (new): `clear_sky_environment`,
  `storm_environment` and the tick/rate constants.
* `crates/cs_app/src/environment/mod.rs` (new): module docs and re-exports.
* `crates/cs_content/src/lib.rs`, `crates/cs_sim/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declarations and doc
  paragraphs. No logic lives there.
* `crates/cs_app/tests/environment/{main,common,sky,records,clock}.rs`
  (new): the fifteen `accept_f19_a_*` acceptance tests.
* This file.

**One observable failure:** after an origin rebase the sky dome is parked at
the camera's *local* coordinate instead of its world one, so the dome jumps
by the origin offset — 512.0006 m in the scenario below — while the sun and
horizon stay put. That is exactly AC01's "rebase world under a fixed
horizon; sky and sun direction stay stable" failing in the only way it can
fail: the horizon is fine, the sky popped. The acceptance test
`accept_f19_a_rebase_keeps_sky_and_sun_direction_stable` measures it, and
the mutation recorded under "Test sensitivity" below makes it fail on
purpose.

## What the stage defines

| record | answers | type |
| --- | --- | --- |
| `SkyArt` | which sky texture, and what happens when it is missing | `Resolved<ContentId>` + `SkyFallback` (`Diagnostic` \| `Generated { profile }`) |
| `SkyOrientation` | which way is up and where the dome's heading points | `up` + `heading`, both unit and perpendicular (`1e-6`, the workspace unit tolerance) |
| `LightingDefinition` | where the sun points and what ambient light there is | two independent `Resolved` values |
| `FogDefinition` | screen-space distance fade | `density_per_m`, `color_linear`, each `Resolved`; a renderer default is built only by `designed_default`, which tags `ClaimStatus::Designed` |
| `CloudLayer` | one authored layer | altitude (finite, ≥ 0) + `Resolved` coverage in `[0, 1]` |
| `WindField` | the one authoritative air velocity | `[f64; 3]` m/s, finite |
| `GameplayVisibility` | how far an actor may be detected | finite range > 0, in canonical meters |
| `PrecipitationDefinition` | what falls | `Resolved<PrecipitationKind>` |
| `EnvironmentState` | the part a weather change replaces | wind + precipitation + visibility, whole-record |
| `WeatherEvent` | when a change happens | `at_tick: u64` + the next whole `EnvironmentState` |
| `EnvironmentDefinition` | one authored environment | all of the above, each as its own field, validated once |

Three decisions are worth recording:

1. **One wind field, exposed through the state.** The definition's
   `wind()` returns the `EnvironmentState`'s field, because a timeline
   event *is* a state. There is no second "definition wind" that could
   drift from the one the schedule installs (F19 non-negotiable behavior
   2). The same is true of precipitation and gameplay visibility.
2. **Visibility is not fog.** `FogDefinition` and `GameplayVisibility` are
   different fields with different resolutions, and there is no constructor
   anywhere that converts one into the other. The clear-sky fixture is
   built precisely so the pair "fog known (`designed`), visibility unknown"
   is exercised (`accept_f19_a_designed_fog_default_does_not_fill_in_gameplay_visibility`).
3. **An environment is not a catalog id.** `IDENTITY-CONTENT`'s
   `ContentKind` reserves no environment namespace (its required
   collections do not list environments either), so `EnvironmentId`
   validates the same `[a-z0-9._-]` subordinate grammar `cs_content::world`
   uses for sectors instead of claiming a namespace that does not exist.
   Whether the catalog should grow one is an owner decision on a protected
   contract; it is filed as a follow-up task rather than decided here.

## Time domains

The schedule's ticks are authored in `cs_content` (which must not depend on
`cs_sim`), and the *domain* is declared once in `cs_sim::visibility`:

* `ENVIRONMENT_TIME_DOMAIN = TimeDomain::AuthoritativeGameplay`, and
  `environment_clock_policy()` = `ClockPolicy::authoritative_gameplay()`:
  pause **freezes**, no local speed-up authority.
* `VisibilityTimeline<T>` is generic over the state for the same reason —
  `cs_sim` cannot depend on `cs_content` — and
  `cs_app::environment::EnvironmentClock` is the one place the two are
  paired, with `T = EnvironmentState`.
* An event fires when the clock has *committed* its tick
  (`event.at_tick <= clock.tick()`), events at tick 0 are part of the
  initial state, and applying one **replaces** the state whole, so a replay
  reaches the same state at the same tick with no leftover field.
* `advance_fixed_ticks` is present and always refused
  (`TimeError::NoSpeedUpAuthority`), before the pause check: a replay or a
  speed-up cannot move a weather event's instant. The refusal is tested
  rather than assumed.

Measured: at `TickRate::new(64)`, one fixture frame is
`Duration::from_nanos(15_625_000)` = exactly one tick
(`15_625_000 ns × 64 = 1_000_000_000 ns`), and four-tick frames
(`62_500_000 ns`) also divide exactly, so the frame-split test compares
integer schedules and not rounding. The gust is authored at tick 64 and the
sight range at tick 192; both fire on exactly those ticks in a 1-tick-per-frame
run *and* in a 4-ticks-per-frame run of the same total time.

## Sky frame and the rebase

`SkyFrame` holds `epoch`, `dome_position` (world meters), the authored
`Resolved<SkyOrientation>` and the authored `Resolved<UnitVec3>` sun. Two
constructors exist because they fail differently:

* `capture(&EnvironmentDefinition, &SpatialAnchor)` reads the camera's
  **world** position, which `OriginShift::apply` preserves exactly (it
  rewrites only the f32 local cache), so the frame is bit-identical across
  a rebase;
* `from_local(&EnvironmentDefinition, &WorldOrigin, LocalPosition)`
  converts through `WorldOrigin::world_of`. This is the path that fails
  observably if the conversion is skipped — the mutation below parks the
  dome at the origin, 512.0006 m from the camera.

`is_centered_on(camera_world, 1e-6 m)` makes both halves of behavior 3
observable: a frame held across a rebase is *still* centred (a rebase alone
never invalidates the sky), while a frame captured before the camera moved
is reported stale (a renderer must recapture instead of drawing a lagging
dome).

Scenario numbers (all dyadic, so the f32 local cache holds them exactly):
camera world `[4096, 512, -2048]`, origin rebased to
`camera + [512, 0, 0.25] = [4608, 512, -2047.75]`, epoch 0 → 1. The
camera's local coordinate changes from `[4096, 512, -2048]` to
`[-512, 0, -0.25]` — the test asserts it changed, so the rebase cannot pass
as a no-op — while `dome_position`, `sun_direction`, `sky_orientation` and
`horizon_normal` are unchanged (`assert_eq!` on the last three: they are
copies of authored values, so equality is exact, not tolerant).

## Test sensitivity

Two mutations, applied and reverted, both against production code:

1. `SkyFrame::from_local` returns `origin.position()` instead of
   `origin.world_of(camera_local)` →
   `accept_f19_a_rebase_keeps_sky_and_sun_direction_stable` **fails** with
   "the dome built from the local coordinate must land on the camera in
   world metres: WorldPosition { x: 4608.0, y: 512.0, z: -2047.75 } vs
   WorldPosition { x: 4096.0, y: 512.0, z: -2048.0 }".
2. `VisibilityTimeline::install_due` fires on `tick < event.at_tick`
   instead of `<=` (every event one tick late) →
   `accept_f19_a_weather_timeline_fires_each_event_at_its_own_tick_and_freezes_while_paused`
   and `accept_f19_a_replaying_the_same_frames_reaches_the_same_states`
   **fail** (13 passed, 2 failed).

Both were reverted; the tree is back to the implementation under review.

## Commands run (exit codes)

```text
cargo fmt --all -- --check                                       → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                  → 0
cargo test --workspace --locked -- accept_f19_a_ --include-ignored → 0 (15 tests, all passed)
```

The task selection discovers exactly the 15 `accept_f19_a_*` tests in
`crates/cs_app/tests/environment/`, plus the `environment_clock_policy`
doctest in `cs_sim`. None is `#[ignore]`d: nothing here needs
`CS_GAME_DIR`.

## Designed vs measured: the unknowns this stage met

Everything in this stage is **designed** engine contract. The following are
`unknown`, recorded rather than guessed; none of them blocks F19-A, and each
is named with the stage that resolves it:

| unknown | evidence | resolves in |
| --- | --- | --- |
| Whether the original stores an environment record at all, its layout, its addressing and its units | no original bytes read for this task (`CS_GAME_DIR` was not opened) | F19-D (`gpu` + `retail`) and any F19 format stage |
| The original's sun/ambient/fog/wind tuning values | same; no measurement exists | F19-D against private captures |
| Whether the original distinguishes precipitation kinds, and which | `PrecipitationKind` is a designed three-value vocabulary, documented as such in the module | F19-D |
| What the original uses as a gameplay sight range (value, unit, semantics) | `GameplayVisibility` is a typed home with an explicit unknown in both fixtures | F19-D, then F19-C |
| Whether the original's sky is a texture, a dome or a projection, and how it is oriented | `SkyArt`/`SkyOrientation` are the typed home; the fixture authors a texture because that is what this engine can load today | F19-B/F19-D |
| Where environment records belong in the canonical catalog (`ContentKind` has no environment namespace, and `IDENTITY-CONTENT`'s required collections do not list environments) | F19-A used a subordinate `EnvironmentId` instead of claiming a namespace | #407 `F19-A-CATALOG-KIND`; owner decision, protected path |

## What is not claimed

A code/test pass awards at most **checked**. No original environment data
was read, no original sky or weather behavior was reproduced, no renderer
consumes `SkyFrame` yet, and nothing here is `verified_original`. F19-B is
the stage that draws from these records and F19-D the stage that may compare
them against original captures; synthetic fixtures alone cannot certify
original-data behavior.
