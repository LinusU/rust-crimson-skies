# F25: Hoplite/autogyro and exceptional flight configurations

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F11, F14, F23, F24.
**Owner paths:** `crates/cs_sim/src/flight/autogyro.rs`; `crates/cs_content/src/airframe_roles.rs`; `crates/cs_app/src/airframe_visual.rs`; `tests/`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

Implement every exceptional aircraft control law required by the discovered roster, including the Hoplite/autogyro reference lead. Use FlightModelKind, not filename conditionals scattered through gameplay. Special mission vehicles and oversized craft have explicit controllability, launch and weapon constraints.

## Non-negotiable behavior

1. An autogyro is not automatically a hovering helicopter and not just a fixed-wing plane with a spinning mesh. Derive its low-speed, yaw, lift and rotor visual behavior from data and observation.
2. Forced mission assignments override hangar selection only for that session; they do not corrupt the players owned loadout.
3. Physical and visual rotor speeds may differ but require an explicit mapping. Rotor animation cannot drive physics dt.
4. Allow mission-only pilotable models discovered in the data; never hide a required model because the normal shop does not list it.
5. A shared FlightTelemetry interface keeps HUD, AI and probes model-agnostic.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Forced-airframe launch uses the requested actor despite a different selected garage plane.
**AC02:** Low-speed and engine-off states remain finite and respond according to the approved profile.
**AC03:** A mission-only aircraft can be controlled, damaged and unloaded without shop registration.
**AC04:** Compare the distinctive handling against the original, not just a fixed-wing test.

## Bounded implementation slices

### F25-A: Define exceptional model roles and autogyro telemetry

Dependencies: F11-A, F14-A, F23-A, F24-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f25_a_`. Minimum scenario: Forced-airframe launch uses the requested actor despite a different selected garage plane.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F25-B: Implement measured special control-law subset

Dependencies: F25-A, F11-C, F14-C, F23-C, F24-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f25_b_`. Minimum scenario: Low-speed and engine-off states remain finite and respond according to the approved profile.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F25-C: Wire forced assignment and mission-only aircraft

Dependencies: F25-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f25_c_`. Minimum scenario: A mission-only aircraft can be controlled, damaged and unloaded without shop registration.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F25-D: Validate every exceptional playable configuration

Dependencies: F25-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f25_d_`. Minimum scenario: Compare the distinctive handling against the original, not just a fixed-wing test.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Roster presence and ordinary menu availability are different. The Hoplite name/prefix is source-observed; exact control law remains measurement-dependent.

## References

[S03](../docs/research/SOURCES.md); [S10](../docs/research/SOURCES.md); [S14](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
