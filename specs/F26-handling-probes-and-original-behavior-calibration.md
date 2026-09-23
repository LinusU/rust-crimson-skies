# F26: Handling probes and original-behavior calibration

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F01, F24, F25.
**Owner paths:** `crates/cs_sim/src/probes/`; `tools/cs_inspect/src/handling.rs`; `docs/findings/handling/`; `tests/`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

Create reproducible telemetry probes for straight acceleration, coast-down, climb, dive, turn, roll, yaw, stall recovery, damage and boost. A ReferenceEnvelope records original input, initial state, difficulty, loadout, timing uncertainty and units. Comparisons use justified tolerances, not a blanket exact float equality.

## Non-negotiable behavior

1. Do not fit every model to a single top-speed number. Fit several independent maneuvers and reserve holdout probes to catch overfitting.
2. Original executable frame pacing and capture timing can introduce uncertainty. Record it rather than claiming exact original tick rate.
3. Across the same build/platform/seed require stable results; across platforms use numeric envelopes and authoritative networking, not unsupported bitwise determinism claims.
4. Show original tuning versus calibrated deviations in an inspector report. Every deliberate modern assist has an off switch and provenance.
5. All supported airframes need a coverage row, including custom loadout extremes and forced mission configurations.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A tuned acceleration curve cannot pass if its held-out turn radius is outside the envelope.
**AC02:** Repeated same-build probe hashes agree where determinism is promised.
**AC03:** A missing reference trace reports unavailable, not pass.
**AC04:** Armor/mass extremes remain flyable and retain expected performance ordering.

## Bounded implementation slices

### F26-A: Define telemetry and reference-envelope schemas

Dependencies: F01-A, F24-A, F25-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f26_a_`. Minimum scenario: A tuned acceleration curve cannot pass if its held-out turn radius is outside the envelope.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F26-B: Implement headless maneuver probes and comparisons

Dependencies: F26-A, F01-C, F24-C, F25-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f26_b_`. Minimum scenario: Repeated same-build probe hashes agree where determinism is promised.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F26-C: Add roster-wide handling audit and deviation reports

Dependencies: F26-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f26_c_`. Minimum scenario: A missing reference trace reports unavailable, not pass.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F26-D: Approve faithful handling envelopes for the complete roster

Dependencies: F26-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f26_d_`. Minimum scenario: Armor/mass extremes remain flyable and retain expected performance ordering.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S01](../docs/research/SOURCES.md); [S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
