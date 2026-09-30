# F25-B: the declared exceptional control law

Date: 2026-09-30. Task: F25-B "Implement measured special control-law subset"
(`specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`, section
`### F25-B`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
Capabilities used: ordinary build/test only. No `CS_GAME_DIR` read, no
`retail`/`gpu`/`audio`, therefore **no evidence report** is required or produced:
this stage ships a control law over declared design data, a synthetic fixture
and pure boundary checks, and awards at most **checked** — never
`verified_original`, and no part of it is a measured original value.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/flight/autogyro.rs` (+2094/−163 lines: the F25-B section
  and the module doc, which now covers both stages): the
  production law `ExceptionalControlLaw` (`new`, `commanded_rotor_radps`,
  `rotor_tip_speed_mps`, `rotor_support`, `compute`, `rate_command_torque`), its
  provenance-carrying `ExceptionalProfile` with `validate`/`is_measured`, the
  `HoverCapability` declaration, the `ProfileError` and
  `ExceptionalLawError` vocabularies, the per-source `ExceptionalDiagnostics`
  with `total_force_n`/`body_torque_nm`, the produced `ExceptionalTick` with
  `validate`/`telemetry`, the private vector helpers and
  `EXCEPTIONAL_CONTROL_AXIS`, and the two synthetic fixtures
  `synthetic_exceptional_tuning` / `synthetic_exceptional_profile`. The
  pre-existing `accept_f25_a_*` tests are untouched.
- `crates/cs_sim/src/flight/mod.rs` (wiring only): the new re-exports and the
  `autogyro` doc paragraph.
- `crates/cs_sim/tests/accept_f25_b_exceptional_control_law.rs` (new): three
  integration acceptance tests through the public `cs_sim::flight` surface.
- This file.

**One observable failure:** the control authority is
`damage · stall · max(wing airspeed ramp, rotor support)`. With the rotor term
removed, an exceptional airframe at 3 m/s answers the stick exactly as the
fixed wing on the identical airframe does — `control_authority` collapses to the
wing's `3/40 = 0.075` instead of the declared rotor band's `4.8/24 = 0.2` — and
at 0 m/s with a stopped rotor the airframe's own low-speed response is the wing's
rather than the profile's. `accept_f25_b_the_rotor_carries_low_speed_authority_and_lift`
fails on exactly that, on
`control_authority == rotor_support` and on
`rotor_support > wing_authority`.

## What the slice does, per sheet rule

**AC02 (the stage's minimum scenario), both halves.**

*Low speed.* `accept_f25_b_low_speed_states_stay_finite_and_answer_the_profile`
evaluates 6 airspeeds (0, 0.25, 1, 3, 8, 15 m/s) × 4 engine states (spool 0, 0.5,
1, stopped) × 8 stick positions. Every one is `Ok`, every produced value is
finite, and the rotor rate after the single tick equals the profile's own
first-order response `min(commanded, response · dt)` exactly, so the law cannot
drift into a plausible-looking number. At rest with a stopped rotor and the
throttle wide open the vertical force is gravity alone (to 1e-9) and the
airframe falls.

*Engine off.* `accept_f25_b_engine_off_winds_the_rotor_down_and_stays_finite`
runs a rotor pre-spun to 20 rad/s at 20 m/s with the engine stopped: the thrust
is exactly `0.0` and the boost consumption `0.0` on every tick, the per-tick
step is bounded by `rotor_response_per_s · dt` (so nothing but the fixed tick
can move it), the rate decreases monotonically onto the airflow's command
(8 rad/s), and the *same* airspeed with the engine running settles on the *same*
rate — which is the observation that shows the rotor is driven by the air and not
by the engine. With no airspeed at all a pre-spun rotor gives its energy back:
the rate reaches exactly `0.0`, the lift with it, and the airframe falls.

**Non-negotiable 1 — not a hovering helicopter, structurally.**
`ExceptionalControlLaw::commanded_rotor_radps(&self, airspeed_mps)` takes an
air-relative speed and returns a rate. Its signature has no `FlightInput` and no
`EngineState`, so there is *no path* from the throttle to the rotor.
`accept_f25_b_throttle_has_no_path_to_the_rotor` proves it behaviourally: two
ticks differing only in the engine spool change the thrust and leave all eight
rotor readings bit-identical, and a pre-spun rotor at 20 rad/s with no airspeed
decays to zero however wide the throttle is. `HoverCapability` makes "may this
law hold the airframe's weight with no forward airspeed" a **declared field**
rather than an inference from the model kind, and a profile claiming
`HoverCapability::Hover` is refused by name
(`ProfileError::HoverNotMeasured`) rather than flown.

**Non-negotiable 1 — not a fixed wing with a spinning mesh.**
`accept_f25_b_the_rotor_carries_low_speed_authority_and_lift` runs the same
airframe under both laws (`synthetic_exceptional_tuning` is
`synthetic_fixed_wing` with `model_kind` changed and nothing else). At 3 m/s the
rotor carries the authority (0.2 against the wing's 0.075) and more than fifty
times the wing's lift; the applied world forces differ. Rotor lift acts along
the shaft axis and follows the body, rotor drag acts against the air-relative
velocity, and both are recorded separately.

**Non-negotiable 5 — one shared telemetry channel, now fed by the real law.**
`ExceptionalTick::telemetry` builds the F25-A `TelemetryFrame::exceptional` from
the law's own output.
`accept_f25_b_the_real_law_feeds_the_shared_telemetry_channel` takes the frame
as `&dyn FlightTelemetry` and reads the same shared airspeed a fixed-wing frame
at the same state reports, while the exceptional channel carries the law's
authoritative rate and the drawn rate the explicit `RotorSpeedMapping` produced
(none at all without a mapping). This closes the limitation F25-A's reviewer
recorded: "TelemetryFrame is produced from a **fixed-wing** FlightOutput in the
tests because F25-B's exceptional law does not exist yet."

**One gravity, one drag, one integrator.** The law delegates the wing, engine,
world-space gravity and the declared assist to `FlightModel::compute` and *adds*
rotor terms to it; it never re-derives the aerodynamics.
`accept_f25_b_no_unaccounted_force_and_no_unbounded_torque` runs 2700 ticks over
a grid of airspeeds, engine states, sticks and body rates and asserts, on every
one, that the produced world force is exactly the sum of the recorded
contributions, that the recorded base force is bit-identical to the shared
boundary's own output for the same inputs, and that the applied torque is inside
the tuning's per-axis maximum. `ExceptionalTick::validate` runs the same check
inside `compute`, so a term the law added without recording it is a refusal
rather than a silent force.

**The contract's attitude section.** The law uses the same rate-command plus
bounded feedback torque in body space as the fixed wing, with three differences
that are all visible in `ExceptionalDiagnostics`:

1. the authority source is `max(wing airspeed ramp, rotor support)`, so a
   spinning rotor answers the stick where a wing cannot;
2. the rotor's torque-reaction yaw is added about the shaft axis;
3. the gyroscopic precession torque `ω × L` is added, which is an **identity**
   for a symmetric rotor rather than a fitted curve — only the rotor's polar
   inertia and rate are declared numbers.

The sign convention is restated (`EXCEPTIONAL_CONTROL_AXIS`, because
`model::CONTROL_AXIS` is private to that module) and
`accept_f25_b_rotor_torque_is_bounded_and_couples_attitude` compares the two
laws' body-space torques at 40 m/s, where the rotor's band is saturated and both
reach full authority: the pitch and roll components agree to 1e-6 while the yaw
component does not, which is the rotor reaction a wing has not got. The same
test asserts the coupling (`−I·Ω·ω_x` into the roll axis, `−I·Ω·ω_z` into the
pitch axis, zero yaw), that a stopped rotor has no precession and no yaw
reaction at all, and that a saturated axis is clamped to the tuning's maximum
rather than passed on.

**Bounded refusals.** A fixed-wing tuning, an empty or inverted control band, a
non-finite or out-of-bound profile field, a non-airframe profile id, a profile
that claims an installation origin with a claim that is not an observation, a
repeated tick, a zero-length tick, a corrupt command and an unusable environment
are each refused by name. A refused tick leaves the rotor exactly where it was.

## What is deliberately *not* here

- **No runtime wiring.** `cs_app::physics::flight` still refuses
  `ModelKind::Exceptional` (task #414), so no exceptional airframe can be flown
  yet; that is the dispatch decision F25-E owns and F25-C wires the role record
  into. Nothing here reads a roster, a script or a mission id.
- **No change to `cs_content::airframe_roles.rs` or
  `cs_app::airframe_visual.rs`.** Both are owner paths and neither needs a
  change for AC02: the profile carries its own `airframe_id`, `Origin` and
  `Provenance`, so a role can be mapped to a law by id rather than by a
  filename. Declaring the *role*-side control-law fields (a `Resolved` rotor
  inertia, a declared weapon constraint) belongs with the roster read that F25-C
  and F25-D need; inventing them now would be a second, unprovenanced place for
  the same numbers.
- **No measured value, anywhere.** See below.

## Unknowns recorded (not guessed)

| Unknown | Affected content | How this stage represents it |
| --- | --- | --- |
| What drives the original rotor: the airflow, the engine, or both | every exceptional airframe's low-speed, yaw and lift behaviour | the law declares a **free, airflow-driven** rotor because that is the only model with no engine path to the lift, and records the choice as a *design* decision, not a measurement. The profile is where a measured engine-coupled law would go; `commanded_rotor_radps`'s signature is the seam. |
| Whether the original airframe can hold station with no forward airspeed | the "no hover" claim itself | `HoverCapability::NoHover` is the only declared value and `Hover` is **refused** by name. This is the contract's prohibition, not evidence about the original. |
| Which way the rotor's reaction yaws the airframe, and whether it reverses with airspeed | exceptional yaw behaviour | a single signed declared gain, documented as a convention. The low-speed left-turn / high-speed right-turn behaviour of a real autogyro is *not* implemented: it is a measured fact this project does not have. |
| The rotor's radius, polar inertia, drive gain, response rate, lift gain and cap, drag gain, yaw gain and control band | the whole profile | every field is authored design on `Origin::SyntheticFixture` with a `designed` claim; `ExceptionalProfile::is_measured()` is `false`, and `validate` refuses a profile whose origin claims an installation while its provenance denies it. The numbers are chosen to be plausible in order of magnitude (0.4 rad/s per m/s puts a 4 m rotor at 64 m/s of tip speed in 40 m/s of flight; the 6 kN lift cap is about half the 1200 kg fixture's weight) and to make the *shape* testable. |
| The exceptional airframe's real mass, wing area, thrust and stall speed | every absolute handling number | `synthetic_exceptional_tuning` is the F24 fixture with only `model_kind` changed, so no test can read an absolute figure as a property of a real airframe. |
| A visual/physical rotor ratio for any real airframe | rotor visuals | unchanged from F25-A: the content record holds an explicit unknown, and the numeric channel answers `None` without a mapping. |
| The original roster, catalog ids and mission bindings for a forced autogyro | F25-C / F25-D | untouched by this stage; the `F25` research boundary and `missions/M17.md` still apply. |

## Test sensitivity (verified by perturbation, not asserted)

Each behavior was removed from the production law, the whole `accept_f25_b_`
selection re-run, and the change reverted from a byte-identical backup.

| Behavior removed | Tests that failed |
| --- | --- |
| the rotor lift term (`rotor_lift_n = 0.0`) | `accept_f25_b_the_rotor_carries_low_speed_authority_and_lift`, `accept_f25_b_engine_off_winds_the_rotor_down_and_stays_finite` |
| the rotor support in the authority law (wing ramp only) | `accept_f25_b_the_rotor_carries_low_speed_authority_and_lift` |
| the rotor's torque-reaction yaw | `accept_f25_b_rotor_torque_is_bounded_and_couples_attitude` |
| the gyroscopic precession term | `accept_f25_b_rotor_torque_is_bounded_and_couples_attitude` |
| the per-axis torque bound | `accept_f25_b_rotor_torque_is_bounded_and_couples_attitude` |
| the rotor drag term | `accept_f25_b_the_rotor_carries_low_speed_authority_and_lift` |
| the rotor drive's dependence on airspeed (a constant offset added) | all five behavioural unit tests |
| the shared channel's `control_authority` override (left as the wing's) | `accept_f25_b_the_real_law_feeds_the_shared_telemetry_channel` |
| the hover refusal in `validate` | `accept_f25_b_throttle_has_no_path_to_the_rotor` |
| the model-kind guard (a fixed-wing tuning may be flown) | `accept_f25_b_the_law_refuses_a_fixed_wing_tuning_and_a_corrupt_profile` |
| the strictly-newer-tick rule (the law pins the tick) | three tests |
| the produced-tick finiteness check | `accept_f25_b_a_doctored_or_nonfinite_tick_is_refused_by_name` |
| the produced-tick unaccounted-force check | `accept_f25_b_a_doctored_or_nonfinite_tick_is_refused_by_name` |
| the shared-boundary delegation (a second gravity added) | all seven unit tests |

The eighth row is a real defect this stage's own test found while it was being
written: `FlightDiagnostics` was being built with `..base.diagnostics`, which
left `control_authority` as the *wing's*, so the shared telemetry channel
reported an exceptional airframe as though it had no rotor. It is fixed and the
probe now fails the integration test.

## Limits of this pass

- Every number in the profile is authored design. The law's *shape* — a free
  rotor driven by airflow, lift along the shaft, drag against the airflow, a
  torque-reaction yaw and the exact precession term — is this project's
  engineering design and is **not** evidence about the original Hoplite.
- The tests compare the law against the profile's own declared functions and
  against the fixed-wing law on an identical airframe. They do **not** compare
  it against a reference trace: `ReferenceManeuverEnvelope` still ships
  `Unmeasured`, `is_ready_as_reference()` is `false`, and AC04 ("compare the
  distinctive handling against the original") is entirely F25-D's reach.
- `synthetic_exceptional_tuning` reuses the F24 fixture's mass, wing, engine and
  stall, so this stage has no opinion on an exceptional airframe's real
  performance. A real profile will want its own tuning, which is a content-side
  record this project cannot yet read.
- The law's authority law is `max(wing ramp, rotor support)` multiplied by the
  **wing's** stall factor. A spinning rotor arguably keeps authority in a
  stalled wing, and the choice made here is a declared simplification rather than
  a measured one. F25-D's stall-recovery trace is the place to revisit it.
- The precession term is the exact `ω × L` for a symmetric rotor in body
  coordinates, which assumes the rotor's angular momentum stays aligned with the
  shaft. A rotor with coning, a cyclic, or a shaft that is not the body up axis
  would need more, and the airframe's rotor axis is not read from any original
  data here.
- No consumer reads `ExceptionalControlLaw` yet. `crates/cs_app` still refuses
  `ModelKind::Exceptional` (#414), so the law has a runtime *producer* (its own
  tests) but no runtime consumer trace. The telemetry half is a
  `&dyn FlightTelemetry` reading, not a HUD.
- The rotation helpers (`rotated`, `added`, `scaled`, …) are restated in this
  module because `model.rs` keeps its copies private and `model.rs` is not an
  owner path of this task. They are restated, not re-derived: the integration
  test compares a torque computed here with one computed in `model.rs` at an
  airspeed where both laws share an authority, which is what keeps them honest.

## The ten `accept_f25_b_` tests

| Test | What it pins |
| --- | --- |
| `accept_f25_b_low_speed_states_stay_finite_and_answer_the_profile` | AC02, low speed: 192 legal low-speed ticks, all finite, the rotor equals the profile's own response, and at rest gravity is the only vertical force |
| `accept_f25_b_the_rotor_carries_low_speed_authority_and_lift` | non-negotiable 1 first half: the rotor, not the wing, carries low-speed authority and lift; the two laws differ on an identical airframe |
| `accept_f25_b_engine_off_winds_the_rotor_down_and_stays_finite` | AC02, engine off: zero thrust, a bounded per-tick step, monotone decay onto the airflow's command, the same steady rate with the engine on, and a total loss of lift in still air |
| `accept_f25_b_throttle_has_no_path_to_the_rotor` | non-negotiable 1 second half: no engine-spool or throttle path to any rotor term, and a hover claim is refused |
| `accept_f25_b_rotor_torque_is_bounded_and_couples_attitude` | the precession coupling, the yaw reaction reaching the applied torque, the per-axis bound, and the two laws agreeing on the sign convention |
| `accept_f25_b_the_law_refuses_a_fixed_wing_tuning_and_a_corrupt_profile` | nine named refusals, and a refused tick leaving the rotor untouched |
| `accept_f25_b_a_doctored_or_nonfinite_tick_is_refused_by_name` | the producing side: a non-finite value is named and an unaccounted force is caught |
| `accept_f25_b_a_closed_low_speed_and_engine_off_loop_stays_finite` | integration: a 1200-tick closed loop, engine on then off at 4 m/s, finite throughout and answering the profile |
| `accept_f25_b_the_real_law_feeds_the_shared_telemetry_channel` | non-negotiable 5 with the real law: a `&dyn FlightTelemetry` consumer, the exceptional rotor channel, and the model-agnostic shared airspeed |
| `accept_f25_b_no_unaccounted_force_and_no_unbounded_torque` | integration: 2700 ticks with the force accounting and the torque bound on every one |

## Follow-ups filed

None from this stage. The work this stage deliberately defers is already
tracked as task #99 (F25-C, wire the role record and the resolver into the real
producer and consumer) and task #414 (dispatch the flight driver on
`ModelKind`), and the calibration against an original trace is F25-D
(`retail`).
