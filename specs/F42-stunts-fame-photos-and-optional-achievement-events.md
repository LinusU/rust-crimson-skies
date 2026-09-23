# F42: Stunts, fame, photos, and optional achievement events

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F18, F23, F31, F39, F43.
**Owner paths:** `crates/cs_sim/src/stunts.rs`; `crates/cs_content/src/stunts.rs`; `crates/cs_app/src/stunts.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

StuntDefinition binds an authored traversal volume/sequence to mission eligibility, direction/clearance rules, reward and scrapbook media. Detect actual continuous passage, not proximity or a screenshot. Fame/reward and any pursuing-AI effects are separate consumers with their own evidence.

## Non-negotiable behavior

1. A teleport or developer camera movement cannot earn a stunt. A rebase does not break a genuine continuous passage.
2. Repeated loops through the same stunt respect original repeatability rules; deduplicate one-time photo rewards by profile/mission/stunt identity.
3. Stunt availability may differ between missions sharing one world. Do not award every world stunt in every mission automatically.
4. Optional stunts cannot force mission success or change the critical path unless authored. Modern achievement systems are supplementary.
5. Measure trigger geometry and direction rules from original data/observation; manually drawn replacement volumes are marked reconstructed until validated.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Fly through, beside, backwards and teleport across the same synthetic gate; only eligible traversals count.
**AC02:** Complete a stunt twice and retry mission; reward duplication policy is correct.
**AC03:** Visit the same world in another mission with a different eligible set.
**AC04:** Verify each original collectible/stunt trigger and its linked media consumer.

## Bounded implementation slices

### F42-A: Define traversal predicates and reward identity

Dependencies: F18-A, F23-A, F31-A, F39-A, F43-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f42_a_`. Minimum scenario: Fly through, beside, backwards and teleport across the same synthetic gate; only eligible traversals count.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F42-B: Implement swept/sequence stunt detection

Dependencies: F42-A, F18-C, F23-C, F31-C, F39-C, F43-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f42_b_`. Minimum scenario: Complete a stunt twice and retry mission; reward duplication policy is correct.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F42-C: Wire fame, AI responses and scrapbook rewards

Dependencies: F42-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f42_c_`. Minimum scenario: Visit the same world in another mission with a different eligible set.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F42-D: Audit all original mission-scoped stunts and optional unlocks

Dependencies: F42-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f42_d_`. Minimum scenario: Verify each original collectible/stunt trigger and its linked media consumer.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md); [S14](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
