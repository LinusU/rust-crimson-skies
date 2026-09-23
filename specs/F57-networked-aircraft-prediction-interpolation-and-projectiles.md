# F57: Networked aircraft, prediction, interpolation, and projectiles

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F23, F24, F25, F27, F28, F54.
**Owner paths:** `crates/cs_net/src/snapshot.rs`; `crates/cs_app/src/network/physics.rs`; `crates/cs_sim/src/net_state.rs`; `tests/`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

Server snapshots carry actor generations, transforms, velocities, essential flight/damage/weapon state and tick ids. Local prediction is a designed responsiveness layer; reconciliation corrects to authoritative state without granting client authority over damage or mission results.

## Non-negotiable behavior

1. Quantization scales, bounds and error budgets are explicit. Relative/origin-based coordinates require a shared epoch; a rebase is not a huge velocity impulse.
2. Remote interpolation uses a bounded jitter buffer and handles missing snapshots, teleport, spawn and despawn. Never interpolate between different actor generations.
3. Projectiles and effects distinguish predicted cosmetics from authoritative hits. A local tracer cannot award a kill.
4. If rollback of full Avian state is not supported reliably, use bounded state correction and document limitations rather than claiming exact rollback.
5. Packet loss and latency tests include 0/50/150 ms RTT and 0/2/10 percent loss as designed test conditions, not original network specifications.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Inject latency/loss/reordering; no duplicate destruction or permanent ghost aircraft.
**AC02:** A server correction during a local boost keeps ammo/fuel authoritative.
**AC03:** Origin change across snapshot boundaries produces no world-scale jump.
**AC04:** Two targets with recycled ids never share interpolation history.

## Bounded implementation slices

### F57-A: Define snapshot schema and quantization budgets

Dependencies: F23-A, F24-A, F25-A, F27-A, F28-A, F54-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f57_a_`. Minimum scenario: Inject latency/loss/reordering; no duplicate destruction or permanent ghost aircraft.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F57-B: Implement interpolation and bounded local prediction

Dependencies: F57-A, F23-C, F24-C, F25-C, F27-C, F28-C, F54-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f57_b_`. Minimum scenario: A server correction during a local boost keeps ammo/fuel authoritative.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F57-C: Wire reconciliation, projectile confirmation and origin epochs

Dependencies: F57-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f57_c_`. Minimum scenario: Origin change across snapshot boundaries produces no world-scale jump.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F57-D: Measure gameplay under latency/loss and on mixed supported platforms

Dependencies: F57-C. Required capabilities: network_real.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f57_d_`. Minimum scenario: Two targets with recycled ids never share interpolation history.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
