# F24-B: fixed-wing forces and bounded arcade controller

Date: 2026-09-30. Task: F24-B "Implement fixed-wing forces and bounded
arcade controller"
(`specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`, section
`### F24-B`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Capabilities
used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence report
required).

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/physics/flight.rs` (new): the production path —
  `FlightAircraft` (the per-body record the tick consumes), `FlightSpawnSpec`
  and `spawn_flight_body` (the one entry point, binding declared mass and
  inertia), `drive_flight_aircraft` (the `FixedUpdate` driver), the
  `FlightForcesPlugin` registration, and the `FlightTickReport`/`FlightRefusal`
  accounting that makes a skipped or refused tick visible.
- `crates/cs_app/src/physics/mod.rs` (wiring only): `pub mod flight;`, the
  re-exports and the doc paragraph. This is the physics module's declaration
  file; the wiring is listed explicitly in the handover summary.
- `crates/cs_sim/src/flight/model.rs`: one behavior fix — a **stopped** engine
  no longer produces `idle_thrust_n` (see "Defect found by the runtime path"
  below).
- `crates/cs_app/tests/flight/main.rs` (new): the `accept_f24_b_*` acceptance
  tests, all driving the production path through the real `PhysicsFixture`.
- This file.

**One observable failure:** with the flight path absent, a powered-off
climbing body drifts at constant velocity — Avian's global `Gravity` is zero
because the model owns gravity, so no force ever acts — and total mechanical
energy `½mv² + mgh` never decreases.
`accept_f24_b_power_off_climb_loses_energy` fails on exactly that
(`energy < previous_energy` every tick, plus the altitude/speed exchange).

## The production path

Per fixed tick, in `FixedUpdate` (before the `FixedPostUpdate` drain the
F23-A adapter owns), `drive_flight_aircraft` does, for each dynamic
`FlightAircraft`:

1. refuses the tick — counted in `FlightTickReport.refused` with a named
   `FlightRefusalReason` — when Avian's `Gravity` is non-zero (the model
   applies world-space gravity itself; flying anyway would double it);
2. binds the body's `Mass` to `loadout + airframe`, so the integrator's mass
   is always the same number the gravity force was computed from — a mid-
   flight `set_loadout` moves it on the next tick (non-negotiable 4);
3. reads `Rotation` (renormalized on readback, still unit-validated),
   `LinearVelocity` and world-space `AngularVelocity` — rotated into body
   space by the conjugate — into a `FlightState`;
4. advances the engine spool toward the held throttle at the tuning's
   response rate, then calls `FlightModel::compute` — the F24-A equations,
   untouched as a pure per-tick function;
5. consumes exactly the reported `accepted_boost_consumption` from the
   aircraft's declared reserve (floored at zero; a press while empty consumes
   nothing), stores the `FlightOutput` as `last_output` for instruments, and
   submits the world force/torque as a one-tick `ForceRequest` that the
   adapter applies to the same tick.

`FlightTickReport` counts `ticks`, `driven`, `parked` (non-dynamic aircraft
are skipped, not errored — a kinematic scripted actor is not flown) and
`refused` with `last_refusal` carrying entity, tick and reason. Missing
`RigidBody`, missing readbacks, a non-unit readback, a model refusal and a
rejected `ForceRequest` each have their own named variant.

`spawn_flight_body` wraps `spawn_body` so layer bindings are never restated,
computes the body's mass itself (the caller never supplies one), writes the
spawn orientation onto `Rotation` **and** `Transform`, and binds the tuning's
principal inertia as `AngularInertia` with `NoAutoMass`/`NoAutoAngularInertia`
so the collider's derived properties cannot silently inflate the declared
values. The tuning lists inertia in (roll, pitch, yaw) order — about body
+Z, +X, +Y — so Avian's principal vector (about X, Y, Z) is
`(inertia[1], inertia[2], inertia[0])`, which
`accept_f24_b_spawn_binds_declared_mass_and_inertia` pins.

`FlightAircraft::new` refuses `ModelKind != FixedWing` with
`UnsupportedModelKind` — the exceptional control law is F25's and this driver
never guesses one. Every setter (`set_loadout`, `set_damage`, `set_engine`,
`set_boost_capacity`, `set_command`, `set_environment`) re-validates.

The bounded arcade controller is the F24-A rate-command law exercising
through the real integrator: `accept_f24_b_roll_command_produces_a_bounded_
body_rate` measures a full roll command settling at ≈1.78 rad/s —
`max_rate·gain/(gain+damping)` — which stays under the declared `2.0 rad/s`
cap, and the rate damps to zero when the command is released.

## Defect found by the runtime path (fixed in `model.rs`)

F24-A computed `base_thrust_n = engine.thrust_at(spool)` with `spool` forced
to `0` for a stopped engine — but `thrust_at(0)` returns `idle_thrust_n`, so a
*stopped* engine still produced 400 N. The zero-airspeed runtime test caught
it: a "dead" aircraft drifted 0.23 m/s along its nose instead of falling
straight down. The fix computes `base_thrust_n` only when
`state.engine.running` — idle thrust is the zero-spool output of a *running*
engine. All F24-A tests still pass; `EngineState::direct(0.0)` (running,
zero spool) still produces idle thrust as designed.

## Mutation probes (tests fail when the behavior is removed)

Each probe was applied to the working tree, the suite was run, and the tree
was restored. No probe was committed.

1. `FlightForcesPlugin::build` no longer registers `drive_flight_aircraft` →
   **10 of 13** integration tests failed (climb, dive, zero-airspeed fall,
   throttle, roll, tick accounting, boost accounting, parked/plain,
   gravity-conflict, instruments). The two spawn-boundary tests still passed —
   they exercise the constructor, not the driver.
2. The driver's submit replaced the model output with zero force/torque →
   the 5 physics-content tests failed: `power_off_climb_loses_energy`,
   `dive_converts_altitude_into_speed`, `zero_airspeed_falls_under_model_
   gravity`, `throttle_spools_then_accelerates`,
   `roll_command_produces_a_bounded_body_rate`.

## Commands run

All four required checks, run from the repository root; exit codes as printed.

```
cargo fmt --all -- --check                                             -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                        -> 0 (131 "test result: ok", no failures)
cargo test --workspace --locked -- accept_f24_b_ --include-ignored     -> 0 (15 tests selected, all passed)
```

The 15 selected tests are 13 integration tests in `crates/cs_app/tests/
flight/main.rs` plus 2 unit tests in `physics::flight::tests`. No test is
`#[ignore]`d, so `--include-ignored` selects the same set.

## Designed wiring, not original data

Every value is synthetic (`synthetic_fixed_wing`) or project design. The
tick ordering, the spawn binding and the report shape are this project's
design; the original game's force constants, tick rate and control law remain
**unknown** (F24-D calibrates against `REF-OWNER-FIRST-CAPTURE` #358). No
original-data, visual, audible or ordinary-play claim; this stage can award
at most **checked**.

## Known limitations that gate later stages (not silently dropped)

Affected content: the whole F24 flight path. Resolving tasks: **F24-C**,
**F24-D**, **F25**.

1. **No input producer wiring.** `FlightAircraft::command` is held state a
   producer writes (the consumption side of the contract); the
   `InputSession`/`ControlBuffer` producer that maps human control commands
   into it — plus throttle steps and profile selection — is F24-C.
2. **No loadout/damage source wiring.** `set_loadout`/`set_damage` are the
   boundary; the mission/equipment producers are F24-C. Mass consistency on
   change is already handled by the tick (above).
3. **The boost reserve is a caller-supplied number.** `capacity −=
   consumption` is tracked per tick but nothing recharges or derives it from
   equipment; that accounting is F24-C's equipment/state model.
4. **Exceptional airframes are refused, not flown.** `ModelKind::Exceptional`
   (autogyros and the like) is F25's control law.
5. **No aerodynamic moments.** Attitude only changes via the bounded rate
   command — no pitch stability, no weathervane — matching the designed
   F24-A torque model, which is deliberately stall/damage/authority scaled.
   Whether the original had more is an F24-D question.
6. **A flying aircraft can never sleep.** The gravity request wakes it every
   tick — correct for a force-driven body, but a *parked on the ground*
   aircraft will need a landed/contact notion (later stage) before it can
   sleep.
7. **The environment is per-aircraft.** `FlightEnvironment` is stored on the
   component (each aircraft records the air it flew in); a shared weather
   producer is a later wiring decision, and wind is symmetric per aircraft
   today.

## Sources

- `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md` (`###
  F24-B`), `docs/contracts/FLIGHT-PHYSICS.md`, `docs/01-ARCHITECTURE.md`.
- `crates/cs_sim/src/flight/{model,tuning,synthetic}.rs` (the F24-A surface
  this wires), `crates/cs_app/src/physics/{adapter,body,fixture}.rs` (the
  F23 production path), `crates/cs_app/src/physics/session.rs` (fixed-clock
  production runner, left untouched — the plugin slots into a consumer app).
- `docs/findings/2026-09-29-f24-a-flight-equations-tuning-schema-and-
  synthetic-probes.md` (the bounded slice definition and findings template).
- Avian 0.7.0 sources under the local registry (`dynamics/rigid_body/`):
  `AngularVelocity` is world-space (verified in the integrator's
  `rotation.inverse() * ang_vel`), `apply_torque` is world-space,
  `AngularInertia.principal` is the local-frame inertia, and `NoAutoMass`/
  `NoAutoAngularInertia` exclude collider-derived mass properties.
- Bevy 0.19 `bevy_app/src/main_schedule.rs`: `FixedUpdate` runs inside each
  fixed iteration before `FixedPostUpdate`, so a request submitted there is
  applied to the tick that computed it.

## Session notes

Implementer: `Jakob - Devin SWE-2/devin-1` (SWE-2). A stray `request_work`
call under a different `agentName` briefly claimed task #410; it was released
immediately with `save_checkpoint`, no work done on it.
