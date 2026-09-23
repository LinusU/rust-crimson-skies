# F31: AI navigation, routes, and obstacle avoidance

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F16, F18, F23, F24.
**Owner paths:** `crates/cs_sim/src/ai/navigation.rs`; `crates/cs_content/src/routes.rs`; `tools/cs_inspect/src/routes.rs`; `tests/`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

AI navigation follows authored 3D routes, moving objectives and maneuver envelopes. It emits the same FlightCommand interface as player input. Navigation is not a ground navmesh solution blindly applied to aircraft; route topology, clearance and flight dynamics are explicit.

## Non-negotiable behavior

1. Original route nodes and trigger volumes retain ids and sequence semantics. Do not generate a new route that bypasses authored mission events.
2. Lookahead, pursuit and avoidance are bounded by turn/climb capabilities. Avoidance may temporarily deviate but must rejoin without skipping mandatory route markers.
3. Handle moving carriers, trains and escorts in relative coordinates. Arrival is a swept/continuous condition, not equality of float positions.
4. No teleport unsticking in fidelity mode without a documented original rule. Developer recovery is marked and invalidates parity evidence.
5. AI decisions run on a declared fixed cadence with deterministic seeds; cosmetic RNG cannot alter behavior.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A route through a narrow arch is followed without crossing a blocked wall.
**AC02:** Reorder ECS entities; the same seed produces the same local decision sequence.
**AC03:** An AI displaced off route rejoins before the next mandatory marker.
**AC04:** A moving waypoint and origin shift do not reset progress or trigger false arrival.

## Bounded implementation slices

### F31-A: Define route graph and maneuver-envelope contracts

Dependencies: F14-A, F16-A, F18-A, F23-A, F24-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f31_a_`. Minimum scenario: A route through a narrow arch is followed without crossing a blocked wall.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F31-B: Implement pursuit and bounded obstacle avoidance

Dependencies: F31-A, F14-C, F16-C, F18-C, F23-C, F24-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f31_b_`. Minimum scenario: Reorder ECS entities; the same seed produces the same local decision sequence.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F31-C: Wire original routes and moving reference frames

Dependencies: F31-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f31_c_`. Minimum scenario: An AI displaced off route rejoins before the next mandatory marker.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F31-D: Validate AI route coverage in every mission type

Dependencies: F31-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f31_d_`. Minimum scenario: A moving waypoint and origin shift do not reset progress or trigger false arrival.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S14](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
