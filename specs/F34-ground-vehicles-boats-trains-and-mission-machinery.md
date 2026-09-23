# F34: Ground vehicles, boats, trains, and mission machinery

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F18, F20, F29, F31, F39.
**Owner paths:** `crates/cs_sim/src/world_actors/`; `crates/cs_content/src/world_actors.rs`; `crates/cs_app/src/world_actors.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

Implement mission-required non-aircraft actors with route/trajectory motion, damage zones, weapon mounts, cargo/passenger sockets and event bindings. Trains, boats, trucks, gates, generators, elevators or similar machinery are catalog-driven actor kinds, not anonymous scenery.

## Non-negotiable behavior

1. Trajectory motion defines position and derivative velocity consistently; moving pickup/docking logic uses relative velocity.
2. Destroyable support/cargo relationships use explicit dependency graphs, not name substring checks. Geometry and collision transition together.
3. A ground route may stop at a closed gate; it cannot pass through because only aircraft collisions were implemented.
4. Detached payloads inherit source motion and keep objective identity if relevant.
5. Actor presentation can be culled; mission motion, timers and destruction state cannot stop simply because the player looks away.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A moving train pickup uses the same anchor pose seen by the renderer.
**AC02:** Destroy a gate before and after its convoy arrives; both supported orders behave correctly.
**AC03:** A boat released from a carrier retains appropriate velocity and faction.
**AC04:** Offscreen objective actor continues moving and can fail its objective.

## Bounded implementation slices

### F34-A: Define world-actor motion and dependency graph

Dependencies: F18-A, F20-A, F29-A, F31-A, F39-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f34_a_`. Minimum scenario: A moving train pickup uses the same anchor pose seen by the renderer.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F34-B: Implement rail/road/water/kinematic actors

Dependencies: F34-A, F18-C, F20-C, F29-C, F31-C, F39-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f34_b_`. Minimum scenario: Destroy a gate before and after its convoy arrives; both supported orders behave correctly.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F34-C: Wire pickups, gates, cargo and scripted transitions

Dependencies: F34-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f34_c_`. Minimum scenario: A boat released from a carrier retains appropriate velocity and faction.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F34-D: Verify each mission-required non-aircraft actor family

Dependencies: F34-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f34_d_`. Minimum scenario: Offscreen objective actor continues moving and can fail its objective.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S14](../docs/research/SOURCES.md); [S15](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
