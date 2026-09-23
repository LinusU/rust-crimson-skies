# F32: AI combat, formations, aces, and difficulty

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F27, F28, F29, F30, F31.
**Owner paths:** `crates/cs_sim/src/ai/combat.rs`; `crates/cs_content/src/ai.rs`; `crates/cs_sim/tests/ai/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Combat AI separates role selection, target priority, maneuver selection, firing solution and formation coordination. Behaviors include fighter attack, bomber/torpedo run, escort, interception, evasion and retreat as required by content. Aces are data-driven behavior/skill variants, not just inflated health.

## Non-negotiable behavior

1. Difficulty modifies only evidence-backed parameters or explicitly designed alternatives. Never increase simulation speed to fake difficulty.
2. AI uses the same weapon availability, damage, flight limits and ordnance states as the player unless an original exception is verified.
3. Friendly fire avoidance and line-of-fire checks are separate from target hostility. Prioritize script-assigned objectives without omniscient knowledge outside the approved model.
4. Formation leader loss, assigned-target destruction and route interruption have defined recovery paths.
5. No endless spawn-wave substitute for authored enemies. Spawn ownership and counts come from mission execution.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** An escort prioritizes an attacker threatening its protected actor under the approved policy.
**AC02:** An ace cannot fire a disabled gun or an empty rocket rack.
**AC03:** Destroy formation leader mid-turn; followers recover without NaNs or permanent orbit.
**AC04:** Run repeated mission combat probes at every discovered difficulty and compare outcomes statistically.

## Bounded implementation slices

### F32-A: Define combat roles, skill knobs and decision traces

Dependencies: F27-A, F28-A, F29-A, F30-A, F31-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f32_a_`. Minimum scenario: An escort prioritizes an attacker threatening its protected actor under the approved policy.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F32-B: Implement maneuvers, target priorities and firing solutions

Dependencies: F32-A, F27-C, F28-C, F29-C, F30-C, F31-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f32_b_`. Minimum scenario: An ace cannot fire a disabled gun or an empty rocket rack.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F32-C: Integrate formations, aces and difficulty profiles

Dependencies: F32-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f32_c_`. Minimum scenario: Destroy formation leader mid-turn; followers recover without NaNs or permanent orbit.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F32-D: Verify original AI roles and difficulty-sensitive mission behavior

Dependencies: F32-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f32_d_`. Minimum scenario: Run repeated mission combat probes at every discovered difficulty and compare outcomes statistically.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S14](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
