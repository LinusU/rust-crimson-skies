# F24-C: connect loadouts, damage, instruments and profile selection

Date: 2026-09-30. Task: F24-C "Connect loadouts, damage, instruments and
profile selection"
(`specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`, section
`### F24-C`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

Owner ruling 2026-09-28: F24-C accepts the **synthetic** wiring of loadouts,
damage, instruments and profile selection; the AC03 comparison against original
reference envelopes is not dropped, it is F24-D's separate capability-gated job.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/flight_tuning.rs`: the profile producer. Named
  `FIDELITY_PROFILE`/`IMPROVED_PROFILE` constants, the optional
  `declared_synthetic_improved_airframe()`, and
  `declared_synthetic_airframe_for(profile)` which returns `None` for an
  undeclared label instead of falling back to fidelity.
- `crates/cs_sim/src/flight/synthetic.rs` and `mod.rs`: the declared
  `SyntheticManeuver`/`SyntheticEnvelope` data and
  `synthetic_trace_envelopes()`/`synthetic_trace_envelope(name)` — the four
  AC03 maneuver classes with their (designed, not original) bounds.
- `crates/cs_app/src/physics/flight.rs`: the production wiring —
  `airframe_tuning_from_declared`/`declared_flight_model` (content record +
  profile → the model), the `FlightEquipment` producer component and its
  atomic, idempotent binding in the driver, and the `FlightInstruments`
  consumer component plus `FlightEvidenceClass` and the
  `publish_flight_instruments` system.
- `crates/cs_content/tests/accept_f24_c_flight_profiles.rs`,
  `crates/cs_app/tests/accept_f24_c_flight_wiring.rs`: the `accept_f24_c_*`
  acceptance tests, driving production code only.
- This file.

**One observable failure:** with the F24-B path absent or the driver
unregistered, a body never receives a force, so a full-throttle aircraft does not
accelerate, a held yaw command does not turn it, and a stall never happens —
every one of the four maneuver traces falls outside its declared synthetic
envelope (`accept_f24_c_acceleration_trace_stays_in_its_synthetic_envelope`,
`..._sustained_turn_...`, `..._roll_...`, `..._stall_recovery_...`). Probe 5
below confirms all four fail when `drive_flight_aircraft` is unregistered.

## The production wiring

**Profile selection.** `airframe_tuning_from_declared(record, profile)` is the
one place a `cs_content` record becomes the numeric `cs_sim` tuning. It refuses,
by name: a record whose profile text does not match the requested
`HandlingProfile` (`ProfileMismatch`, so an improved record is never silently
flown as fidelity), an unknown profile/model-kind label, a schema failure, a
required missing/unknown field (`MissingField` — never read as zero) and a
tuning that fails the model's own validation. The record's `Origin` travels into
the tuning unchanged, so a synthetic record cannot be reported as an original
airframe. `declared_flight_model` is the thin constructor on top.

**Equipment (loadout + damage + boost).** `FlightEquipment` is the producer
record: loadout mass, damage state and boost reserve as one component.
`drive_flight_aircraft` binds it into the `FlightAircraft` before computing the
tick, so a mission's fuel burn, ordnance release or damage event reaches the
equations and the integrator's `Mass` on the next tick. Binding is atomic
(validate the whole record, then apply) — a corrupt record refuses the tick with
`FlightRefusalReason::Equipment` and changes nothing, and the next tick retries
the record still on the entity. A record equal to the last one bound is a no-op,
which is what keeps the **boost reserve a draining quantity**: a mission that
keeps declaring the same equipment cannot silently refill a reserve the ticks
consumed. Removing the component is clean teardown — the aircraft keeps its last
bound equipment and does not report an error.

**Instruments.** `FlightInstruments`, updated by `publish_flight_instruments`
(chained after the driver in the same fixed tick), is the panel consumer. It
copies the tick's measured `InstrumentState` together with a
`FlightEvidenceClass` derived from the tuning's `Origin`: `Synthetic` for
synthetic/designed tuning, `InstallationBacked` for installation bytes — neither
is an original-reference claim. The reading carries the *measuring* tick
(`last_output_tick`), and a refused tick measures nothing, so the panel keeps its
last measured state and is not re-stamped.

## The four synthetic maneuver traces

Measured through the runtime (fixed 120 Hz, synthetic airframe), checked against
`cs_sim::flight::synthetic_trace_envelopes()` and labelled `Synthetic`:

| Envelope | Measured | Declared range |
| --- | --- | --- |
| `acceleration.full_throttle_speed_gain_mps` | 12.17 | `[5, 40]` |
| `turn.heading_change_rad` | 1.65 | `[1.0, 3.0]` |
| `roll.peak_rate_radps` | 1.76 | `[1.0, 2.05]` |
| `roll.released_settled_rate_radps` | 0.018 | `[0, 0.05]` |
| `stall.minimum_stall_scale` | 0.438 | `[0.2, 0.6]` |
| `stall.recovered_stall_scale` | 1.0 | `[0.9, 1.0]` |

The envelopes are newly authored design, not original reference envelopes; F24-D
replaces them with bounds fitted to fingerprinted original captures.

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the working tree, the affected suite was run, and the
tree was restored. No probe was committed.

1. `FlightForcesPlugin::build` no longer registers `publish_flight_instruments`
   → `accept_f24_c_instruments_publish_the_measured_tick_with_synthetic_evidence`
   failed (11 passed, 1 failed).
2. The driver no longer binds `FlightEquipment` → 3 failed:
   `equipment_reaches_the_production_model`,
   `corrupt_equipment_refuses_then_retries`,
   `instruments_publish_the_measured_tick_with_synthetic_evidence`.
3. The mapped assist was forced `enabled: false` → 2 failed, including
   `improved_profile_assist_reaches_the_runtime`.
4. The profile-mismatch refusal in `airframe_tuning_from_declared` was removed →
   `declared_profiles_build_production_models` failed (asking for fidelity while
   handing it the improved record started succeeding).
5. `FlightForcesPlugin` no longer registered `drive_flight_aircraft` → 9 of 12
   integration tests failed, including all four maneuver traces.
6. `declared_synthetic_airframe_for(IMPROVED_PROFILE)` returned the fidelity
   record → the `cs_content` profile tests failed.

## Commands run

All four required checks, run from the repository root; exit codes as printed.

```
cargo fmt --all -- --check                                             -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                        -> 0 (1180 passed, 0 failed, 96 ignored across 134 suites)
cargo test --workspace --locked -- accept_f24_c_ --include-ignored     -> 0 (17 tests selected, all passed)
```

The 17 selected tests are 12 integration tests in
`crates/cs_app/tests/accept_f24_c_flight_wiring.rs`, 2 integration tests in
`crates/cs_content/tests/accept_f24_c_flight_profiles.rs`, 2 unit tests in
`cs_content::flight_tuning::tests` and 1 unit test in
`cs_sim::flight::synthetic::tests`. No test is `#[ignore]`d, so
`--include-ignored` selects the same set.

## Designed wiring, not original data

Every value is synthetic (`synthetic_fixed_wing`, `Origin::SyntheticFixture`) or
project design. The envelope bounds, the improved-profile handling values, the
equipment/instrument wiring and the evidence classes are this project's design;
the original game's force constants, loadout effects and instrument behavior
remain **unknown** (F24-D calibrates against `REF-OWNER-FIRST-CAPTURE` #358). No
original-data, visual, audible or ordinary-play claim; this stage can award at
most **checked**.

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F24 flight path. Resolving tasks: **F24-D**, **F25**.

1. **Input producer still unwired.** The `InputSession`/`ControlBuffer` producer
   that maps human control commands into `FlightAircraft::command` lives in
   `cs_app::input`, outside this task's owner paths; the owner ruling scoped
   F24-C to loadout/damage/instrument/profile. It remains a later wiring
   decision.
2. **The equipment record is a whole-record producer.** Refilling a boost
   reserve requires the producer to change the declared value (it can read
   `FlightAircraft::boost_capacity_units()` and add a grant); re-declaring the
   identical record is intentionally a no-op. This is the designed way to keep
   the reserve draining without a second integrator.
3. **Boost recovery/recharge is still not modeled.** A producer can grant a
   reserve, but nothing derives it from fuel burn or recharges it over time.
4. **The envelopes are designed, not measured.** AC03 against original reference
   envelopes is F24-D.
5. **No visual/audio instrument consumer.** `FlightInstruments` is the typed
   panel snapshot; binding it to a rendered gauge is a later UI stage.
6. **Exceptional airframes are refused, not flown** (`ModelKind::Exceptional` is
   F25's control law).

## Sources

- `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md` (`### F24-C`),
  `docs/contracts/FLIGHT-PHYSICS.md`, `docs/01-ARCHITECTURE.md`, `AGENTS.md`.
- `crates/cs_content/src/flight_tuning.rs`, `crates/cs_sim/src/flight/{tuning,
  model,synthetic}.rs` (the F24-A surface), `crates/cs_app/src/physics/flight.rs`
  (the F24-B driver this stage wires).
- `docs/findings/2026-09-30-f24-b-fixed-wing-forces-and-bounded-arcade-controller.md`
  (the driver, its refusal accounting and the findings template).

## Session notes

Implementer: `DeepSeek V4.1 Flash` (DeepSeek, this session) on branch
`rally/95-connect-loadouts-damage-instruments-and`. Independent reviewer: not yet
assigned. No production work needed a fix during the probes; the equipment
idempotence was a design fix found while writing the reserve-drain test.
