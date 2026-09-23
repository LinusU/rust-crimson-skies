# F16: Coordinates, units, origin management, and clocks

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F00, F14.
**Owner paths:** `crates/cs_types/src/space.rs`; `crates/cs_content/src/coordinates.rs`; `crates/cs_app/src/origin.rs`; `crates/cs_sim/src/time.rs`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

Canonical world space is right-handed, +Y up, aircraft forward -Z, SI units and radians. This is a project convention, not a claim about the original. Verified per-format adapters map positions, directions, normals, rotations, winding, distances and angles into that convention exactly once.

## Non-negotiable behavior

1. Measure original scale, handedness, axis order and angle units using at least three independent landmarks/behaviors. A Blender transform is insufficient proof.
2. Use WorldPosition with an f64 world origin and local physics/render positions when precision requires it. Rebase all bodies, projectiles, triggers, AI paths, audio and camera histories atomically.
3. Simulation time is integer tick count with fixed dt. UI wall time, unscaled media time and authoritative gameplay time are distinct.
4. Pause and time acceleration policies are explicit per subsystem. Single-player speed-up advances fixed ticks, not a variable dt; multiplayer does not expose local speed-up authority.
5. Teleport and rebase are distinct: rebase preserves swept continuity and world identity; teleport invalidates prior sweep segments.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Round-trip a position, normal and quaternion through each source adapter within declared tolerance.
**AC02:** Rebase during a projectile flight and docking approach; outcomes equal an unre-based run.
**AC03:** Run equal input at 30,60,144 render FPS; simulated elapsed ticks and state agree within local tolerance.
**AC04:** Pause produces zero weapon cooldown and objective timer advancement.

## Bounded implementation slices

### F16-A: Define units, typed time and coordinate adapters

Dependencies: F00-A, F14-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f16_a_`. Minimum scenario: Round-trip a position, normal and quaternion through each source adapter within declared tolerance.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F16-B: Implement conversion and origin-shift transactions

Dependencies: F16-A, F00-C, F14-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f16_b_`. Minimum scenario: Rebase during a projectile flight and docking approach; outcomes equal an unre-based run.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F16-C: Integrate clocks and origin with every spatial subsystem

Dependencies: F16-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f16_c_`. Minimum scenario: Run equal input at 30,60,144 render FPS; simulated elapsed ticks and state agree within local tolerance.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F16-D: Calibrate original units and compare fixed-tick behavioral probes

Dependencies: F16-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f16_d_`. Minimum scenario: Pause produces zero weapon cooldown and objective timer advancement.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S08](../docs/research/SOURCES.md); [S11](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
