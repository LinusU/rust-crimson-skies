# F24-A: flight equations, tuning schema and synthetic probes

Date: 2026-09-29. Task: F24-A "Define flight equations, tuning schema and
synthetic probes"
(`specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`, section
`### F24-A`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/flight/tuning.rs` (new): the normalized numeric tuning the
  equations consume — `ModelKind`, `HandlingProfile`, `MassProperties`,
  `EngineCurve`, `BoostParameters`, `DragParameters`, `LiftCurve`,
  `StallBehavior`, `AngularResponse`, `AssistProfile`, `AirframeTuning`, plus
  `LoadoutMass` and `DamageState`. `AirframeTuning::validate` is the boundary:
  a non-finite, non-positive or out-of-range value is refused by name and never
  repaired.
- `crates/cs_sim/src/flight/model.rs` (new): `FlightInput`/`FlightState`/
  `FlightEnvironment`/`FlightOutput`/`InstrumentState`/`FlightDiagnostics` and
  `FlightModel::compute`, the one place the force and torque equations live.
- `crates/cs_sim/src/flight/synthetic.rs` (new): `synthetic_fixed_wing()`,
  `synthetic_cases()` and `SyntheticProbe`, the declared open-loop fixture.
- `crates/cs_sim/src/flight/mod.rs` (new): module docs and re-exports.
- `crates/cs_sim/src/lib.rs` (wiring only): `pub mod flight;` and the doc
  paragraph.
- `crates/cs_content/src/flight_tuning.rs` (new): the provenance-carrying,
  normalization-side schema — `TuningFieldSpec`, `TuningSchema::fixed_wing`,
  `DeclaredTuningValue`, `DeclaredAirframeTuning::validate` and
  `declared_synthetic_airframe()`.
- `crates/cs_content/src/lib.rs` (wiring only): `pub mod flight_tuning;` and the
  doc paragraph.
- `crates/cs_sim/tests/accept_f24_a_flight_equations.rs`,
  `crates/cs_content/tests/accept_f24_a_flight_tuning_schema.rs` (new): the
  `accept_f24_a_*` acceptance tests.
- This file.

**One observable failure:** at zero airspeed the airflow is along no
direction, so a naive `v / |v|` turns angle of attack, lift direction or dynamic
pressure into `NaN`/`0/0`, and a model that reads gravity only from the
air-relative term drops gravity entirely. Either way the aircraft at rest does
not fall. `accept_f24_a_zero_airspeed_is_finite_and_gravity_still_acts` fails on
both: measured forces/torque contain `NaN`, or the vertical force is `0`
instead of `-m·g`.

## The equations (designed starting model)

`FlightModel::compute` is a pure function of one tick — no wall clock, no render
frame and no ECS state — so the same inputs at 30/60/120/144 FPS produce equal
forces (AC04's force half):

- body forward is −Z, right +X, up +Y; `v_air = v_world − wind_world` is rotated
  into body space to get angle of attack `atan2(−v_up, v_forward)` and sideslip;
- `q = ½·ρ·V²`, `L = q·S·CL(α)` perpendicular to the airflow in the body-up
  plane, `D = q·S·CD(α)` opposite the airflow, with `CD = CD0 + k·CL²`;
- stall is a smoothstep from `residual_fraction` to `1` across
  `stall_width_rad` past `stall_angle_rad`, so it is finite, bounded and
  continuous with no `if speed < stall` branch;
- thrust acts along body forward with spool, damage and optional boost; boost
  consumption is recorded, and a press while unavailable consumes nothing;
- gravity is world-space and applied **only** here (Avian's global gravity is
  left zero for a flight body), so it acts at zero airspeed;
- attitude is rate-command plus bounded damping in body space, scaled by stall,
  damage and a speed-authority ramp, then rotated to world space; a transform is
  never rotated directly while a body integrates torque;
- the optional bank/level assist is recorded separately, acts only about the
  roll axis and is exactly zero in the calibrated profile, so it cannot hide
  energy or cancel gravity.

`cs_sim` may depend only on `cs_types`/`cs_script`
(`docs/01-ARCHITECTURE.md`), so the numeric model input is declared on the
consuming side; `cs_content::flight_tuning` is the provenance-carrying record
that says where each value came from, and F24-C maps it into this type.

## Designed vocabulary, not original data

Every unit, bound, gain, default, fixture value and field name is **newly
authored project design**, not a measurement. The following are **unknown** and
are not guessed here:

- the original 2000 PC game's exact force equations, their units and its engine
  and controller constants (`F24` "Research boundary"); no field in either new
  module is an extracted original coefficient, and the declared schema's
  `Origin::Designed` says so;
- whether the original's stall was an angle curve, which airframes share a
  tuning and its true stall/recovery behavior (calibration: **F24-D**, against
  `REF-OWNER-FIRST-CAPTURE` #358);
- the original tick rate and force-integration ordering (F23-D/F16-D), and
  therefore the fixed `dt` a calibrated probe will use;
- whether any original airframe used a bank/level or other assist, and its
  magnitude (assist sets are declared engine design here; the profile
  separation is F24-C).

The `TuningSchema::fixed_wing` approved ranges are this project's own envelope
chosen to bound the numerically safe region of the equations; they are not
derived from original data. The declared synthetic airframe is
`Origin::SyntheticFixture` and is a bootstrap projection of
`cs_sim::flight::synthetic::synthetic_fixed_wing`; F24-C is the stage that
asserts the projection and the mapping agree.

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F24 flight path. Resolving tasks: **F24-B**,
**F24-C**, **F24-D**.

1. **No Avian/runtime wiring in this stage.** `crates/cs_app/src/physics/flight.rs`
   is an F24 owner path but is intentionally untouched: wiring it would require
   editing `crates/cs_app/src/physics/mod.rs`, which is outside the owner paths
   and is not a `lib.rs`/`main.rs` wiring file. Producing `ForceRequest`s into
   the F23 adapter and integrating the body is F24-B's bounded slice.
2. **No instruments or UI consumer.** `InstrumentState` is computed but nothing
   displays it; the UI performance bar and the loadout/damage/profile producers
   are F24-C.
3. **No calibrated airframe.** The model has never been compared against a real
   airframe, loadout or reference trace; AC02 (power-off climb / dive energy
   exchange) is measured in F24-B and AC03 (reference envelopes) in F24-D.
4. **The content schema is declared, not wired.** Nothing constructs a
   `DeclaredAirframeTuning` from an original file; `declared_synthetic_airframe`
   is the only producer and F24-C owns the mapping into `AirframeTuning`.
5. **Boost capacity accounting is partial.** The model reports accepted
   consumption for one tick but owns no capacity pool or recovery; the
   equipment/state model is a later F24 stage, and `state.boost_available` is
   currently a caller-supplied flag.

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the working tree, the named test was run, and the tree
was restored. No probe was committed.

1. Gravity contribution set to `[0,0,0]` in `model.rs` →
   `accept_f24_a_zero_airspeed_is_finite_and_gravity_still_acts` panicked at
   "gravity must still act at zero airspeed" (`test result: FAILED`).
2. `StallBehavior::factor_at` short-circuited to `1.0` →
   `accept_f24_a_stall_factor_is_smooth_bounded_and_symmetric` panicked
   (`test result: FAILED`).
3. The approved-range check removed from `DeclaredAirframeTuning::validate` →
   `accept_f24_a_corrupt_or_undeclared_values_are_refused_by_name` panicked
   (`test result: FAILED`).

A fourth probe is implicit in the boundary tests: `FlightModel::compute` now
validates `FlightInput`, so the integration test
`accept_f24_a_corrupt_inputs_are_refused_by_name` failed during development
before that validation existed (it observed `NaN` torque instead of
`Err(FlightError::NonFinite { field: "input.pitch" })`).

## Commands run

All four required checks, run from the repository root; exit codes as printed.

```
cargo fmt --all -- --check                                             -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                        -> 0 (109 "test result: ok", no failures)
cargo test --workspace --locked -- accept_f24_a_ --include-ignored     -> 0 (24 tests selected, all passed)
```

The 24 selected tests are 10 unit + 5 integration in `cs_sim` and 6 unit + 3
integration in `cs_content`, all carrying the `accept_f24_a_` prefix. No test is
`#[ignore]`d, so `--include-ignored` selects the same set.

## Evidence

Synthetic fixtures and design only. No original-data, visual, audible or
ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`
  (`### F24-A`, "Research boundary"), `docs/contracts/FLIGHT-PHYSICS.md`,
  `docs/01-ARCHITECTURE.md`.
- `crates/cs_types/src/content.rs` (`Origin`, `Provenance`, `Resolved`,
  `PermittedRange`), `crates/cs_types/src/space.rs` (`Quaternion`).
- `crates/cs_content/src/coordinates.rs` and `crates/cs_content/src/config.rs`
  (declared-schema and provenance patterns), `docs/findings/2026-09-29-f23-a-avian-schedule-adapter-and-collision-layers.md`
  (findings template).
