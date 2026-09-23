# F39: Objectives, triggers, timers, spawn groups, and dialogue cues

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F16, F29, F30, F37.
**Owner paths:** `crates/cs_sim/src/objectives/`; `crates/cs_content/src/objectives.rs`; `crates/cs_app/src/objectives.rs`; `tests/`.
**Shared contract:** [SCRIPT-MISSION](../docs/contracts/SCRIPT-MISSION.md).

## Deliverable and interfaces

Objective state supports Hidden, Pending, Active, Succeeded, Failed, Optional and Superseded as required by program semantics. Triggers include spatial entry/exit, sequence crossing, actor state, counts, timers, relationships and explicit scripted signals. Spawn groups and dialogue cues consume the same ordered event stream.

## Non-negotiable behavior

1. Swept triggers use actual movement segments; teleport cannot collect every volume crossed by an artificial segment.
2. Counters distinguish destroyed, disabled, captured, escaped and despawned actors. Never approximate every objective by enemy_alive == 0.
3. Timers have a declared start condition and time domain. Reaching a waypoint can unlock or reset objectives only through a specific program action.
4. Spawns have stable instance ids and idempotency keys. A repeated cue cannot spawn another wave or duplicate radio dialogue.
5. Optional rewards, failure, success and extraction are distinct. Show objectives only when the original reveal rules allow.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Cross a small trigger at high speed; emit one entry and appropriate exit.
**AC02:** Destroy a protected actor on the same tick as completing an objective; use declared terminal precedence.
**AC03:** Retry after several waves and confirm no old timers, actors or cues survive.
**AC04:** Complete supported objectives out of the common order without deadlocking the program.

## Bounded implementation slices

### F39-A: Define objective/trigger/spawn semantics and fixtures

Dependencies: F14-A, F16-A, F29-A, F30-A, F37-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f39_a_`. Minimum scenario: Cross a small trigger at high speed; emit one entry and appropriate exit.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F39-B: Implement continuous triggers, counters and timer actions

Dependencies: F39-A, F14-C, F16-C, F29-C, F30-C, F37-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f39_b_`. Minimum scenario: Destroy a protected actor on the same tick as completing an objective; use declared terminal precedence.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F39-C: Wire mission programs, UI and dialogue events

Dependencies: F39-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f39_c_`. Minimum scenario: Retry after several waves and confirm no old timers, actors or cues survive.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F39-D: Validate original branching, optional and failure conditions

Dependencies: F39-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f39_d_`. Minimum scenario: Complete supported objectives out of the common order without deadlocking the program.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S14](../docs/research/SOURCES.md); [S15](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
