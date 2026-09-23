# F30: Targeting, classification, aim assistance, and threat cues

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F16, F22, F29.
**Owner paths:** `crates/cs_sim/src/targeting.rs`; `crates/cs_app/src/targeting.rs`; `crates/cs_content/src/target_rules.rs`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Targeting selects stable ActorId values from current mission rules and faction relations. Support original enemy/objective, ally, non-aircraft, nearest-attacker, under-crosshair and clear-target actions where verified. Target data drives HUD, spyglass and optional aim assistance without becoming a source of combat authority.

## Non-negotiable behavior

1. Target validity changes with destruction, ownership, hidden/revealed state and script phase. A target that becomes friendly is not silently still an enemy.
2. Sorting/tie-breaking is stable, independent of ECS iteration order. Cycling includes only eligible live actors.
3. Lead indicators and aim assistance are separate options with original evidence classification; no automatic hit correction is added to claim original behavior.
4. Threat cues use actual authoritative attack events, not every enemy in radius.
5. Spatial queries use canonical coordinates and origin-safe references; targets remain correct through rebases and aircraft swaps.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Cycle through equal-distance targets deterministically.
**AC02:** Faction change updates reticle and AI hostility in the same phase boundary.
**AC03:** Destroyed selected target clears safely before spyglass render.
**AC04:** Crosshair selection respects occlusion and eligibility according to approved rules.

## Bounded implementation slices

### F30-A: Define target queries and allegiance contracts

Dependencies: F14-A, F16-A, F22-A, F29-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f30_a_`. Minimum scenario: Cycle through equal-distance targets deterministically.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F30-B: Implement original selection actions and threat state

Dependencies: F30-A, F14-C, F16-C, F22-C, F29-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f30_b_`. Minimum scenario: Faction change updates reticle and AI hostility in the same phase boundary.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F30-C: Connect HUD, spyglass and weapon guidance

Dependencies: F30-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f30_c_`. Minimum scenario: Destroyed selected target clears safely before spyglass render.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F30-D: Verify target order, reveal rules and original assistance behavior

Dependencies: F30-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f30_d_`. Minimum scenario: Crosshair selection respects occlusion and eligibility according to approved rules.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
