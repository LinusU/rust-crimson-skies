# F24: Fixed-wing flight, engine, stall, and arcade assists

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F16, F22, F23.
**Owner paths:** `crates/cs_sim/src/flight/`; `crates/cs_content/src/flight_tuning.rs`; `crates/cs_app/src/physics/flight.rs`; `tests/`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

FlightModel consumes normalized AirframeTuning, LoadoutMass, damage state, air-relative velocity and FlightCommand; it returns forces/torques and instrument state. Start with a documented aerodynamic/controller approximation, then calibrate against measured original behavior. Improved handling is an optional named profile, never silently the fidelity profile.

## Non-negotiable behavior

1. Use air-relative velocity for lift/drag; gravity remains world-space. Define angle-of-attack, lift direction, drag and low-speed behavior so no normalization divides by zero.
2. Stall is gradual loss of lift/control authority with recoverable dynamics unless observations require otherwise; do not just clamp speed to a minimum.
3. Engine thrust, throttle response, roll/pitch/yaw authority and damping are separate parameters. Avoid imposing a modern simulator that makes original stunt routes unplayable.
4. Mass and armor changes affect the same model used by UI performance bars. Cosmetic LOD never changes inertia.
5. Never add energy accidentally through assists. Record assist force/torque contributions, disable them in calibrated probes and maintain original/modern profile separation.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** At zero airspeed every computed value is finite; gravity still acts.
**AC02:** Power-off climb loses energy; a dive converts altitude into speed.
**AC03:** Sustained turn, roll, acceleration and stall recovery traces remain within approved reference envelopes.
**AC04:** Switching render FPS changes no force integration count or command sampling.

## Bounded implementation slices

### F24-A: Define flight equations, tuning schema and synthetic probes

Dependencies: F14-A, F16-A, F22-A, F23-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f24_a_`. Minimum scenario: At zero airspeed every computed value is finite; gravity still acts.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F24-B: Implement fixed-wing forces and bounded arcade controller

Dependencies: F24-A, F14-C, F16-C, F22-C, F23-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f24_b_`. Minimum scenario: Power-off climb loses energy; a dive converts altitude into speed.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F24-C: Connect loadouts, damage, instruments and profile selection

Dependencies: F24-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f24_c_`. Minimum scenario: Sustained turn, roll, acceleration and stall recovery traces remain within approved reference envelopes.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F24-D: Calibrate all original fixed-wing airframes against reference traces

Dependencies: F24-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f24_d_`. Minimum scenario: Switching render FPS changes no force integration count or command sampling.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Public research did not recover exact flight equations or tuning units. Engine/controller constants must be extracted or calibrated; no claim of byte-identical or physically exact original simulation is made.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
