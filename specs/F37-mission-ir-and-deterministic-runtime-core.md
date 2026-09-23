# F37: Mission IR and deterministic runtime core

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F16, F29, F30.
**Owner paths:** `crates/cs_script/src/ir.rs`; `crates/cs_script/src/runtime.rs`; `crates/cs_sim/src/mission.rs`; `crates/cs_script/tests/`.
**Shared contract:** [SCRIPT-MISSION](../docs/contracts/SCRIPT-MISSION.md).

## Deliverable and interfaces

Engine-facing MissionProgram is a versioned, validated representation with stable symbol ids, typed values, conditions, ordered actions, timers, event subscriptions, actor references and source spans. This IR is a new design, not a claim that original programs use this structure.

## Non-negotiable behavior

1. Separate program data, mutable execution state and host effects. Evaluation uses integer simulation ticks, explicit RNG and a bounded work budget.
2. Prevent reentrant callbacks from mutating collections being evaluated. Queue effects, resolve them in documented phases and emit next-tick events where necessary.
3. Each event/action has an execution key enabling exactly-once consumption. Save/restore and retry cannot repeat rewards or captures.
4. Validate jumps/references/types before mission launch. Unknown instruction or native call fails with a precise trace; never treat it as NOP.
5. Terminal state is one of Running, Succeeded, Failed, Aborted or Unsupported. Success cannot coexist with failure; simultaneous conditions have an evidence-backed precedence rule.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Two objective conditions become true on one tick; result ordering is stable.
**AC02:** A zero-delay action that schedules itself hits the work budget instead of hanging.
**AC03:** Save/restore at a pending timer preserves the exact remaining ticks.
**AC04:** Unknown instruction returns Unsupported and prevents reward/progression.

## Bounded implementation slices

### F37-A: Define typed mission IR, phases and validation

Dependencies: F14-A, F16-A, F29-A, F30-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f37_a_`. Minimum scenario: Two objective conditions become true on one tick; result ordering is stable.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F37-B: Implement bounded evaluator and event queues

Dependencies: F37-A, F14-C, F16-C, F29-C, F30-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f37_b_`. Minimum scenario: A zero-delay action that schedules itself hits the work budget instead of hanging.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F37-C: Integrate authoritative host effects and terminal states

Dependencies: F37-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f37_c_`. Minimum scenario: Save/restore at a pending timer preserves the exact remaining ticks.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F37-D: Run adversarial mission-runtime corpus and reference ordering probes

Dependencies: F37-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f37_d_`. Minimum scenario: Unknown instruction returns Unsupported and prevents reward/progression.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
