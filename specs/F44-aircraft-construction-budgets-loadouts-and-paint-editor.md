# F44: Aircraft construction, budgets, loadouts, and paint editor

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F09, F11, F14, F24, F25, F27, F28, F43.
**Owner paths:** `crates/cs_content/src/construction.rs`; `crates/cs_sim/src/economy.rs`; `crates/cs_app/src/construction/`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

AircraftBlueprint selects airframe, engine, per-zone armor, gun positions, rocket hardpoints, equipment, paint and decals. One validator serves campaign construction, IA, multiplayer and imports. It returns exact weight/cost totals, constraints, warnings and a normalized performance preview.

## Non-negotiable behavior

1. Observed manual constraints include four gun positions supporting single/pair selections, paired gun compatibility and up to eight rocket hardpoints. Confirm these against each discovered airframe/rule profile before declaring universal limits.
2. Use integer money and documented weight units. Rounding occurs at a specified boundary; float UI formatting cannot change purchase eligibility.
3. Edit in a draft transaction. Cancel changes nothing; purchase/sell commits once with sufficient resources and valid availability. Selling cannot leave an illegal active reference.
4. Preview uses the real flight/damage/loadout evaluator, not unrelated hand-tuned bars. Asymmetric valid configurations remain valid.
5. Custom paint/decals preserve source masks and valid combinations. Export/import never includes copyrighted source textures implicitly.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Boundary loadout exactly at weight/cost limit is accepted; one unit over rejected.
**AC02:** Cancel an edited draft and verify inventory/currency unchanged.
**AC03:** Imported blueprint cannot bypass paired-gun or host banned-component rules.
**AC04:** Preview and actual spawned aircraft have equal normalized mass, weapons and paint.

## Bounded implementation slices

### F44-A: Define blueprint constraints and exact budget arithmetic

Dependencies: F09-A, F11-A, F14-A, F24-A, F25-A, F27-A, F28-A, F43-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f44_a_`. Minimum scenario: Boundary loadout exactly at weight/cost limit is accepted; one unit over rejected.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F44-B: Implement shared validator and transactional economy

Dependencies: F44-A, F09-C, F11-C, F14-C, F24-C, F25-C, F27-C, F28-C, F43-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f44_b_`. Minimum scenario: Cancel an edited draft and verify inventory/currency unchanged.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F44-C: Build construction, paint and loadout UI with preview

Dependencies: F44-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f44_c_`. Minimum scenario: Imported blueprint cannot bypass paired-gun or host banned-component rules.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F44-D: Verify original component availability, budgets and every stock blueprint

Dependencies: F44-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f44_d_`. Minimum scenario: Preview and actual spawned aircraft have equal normalized mass, weapons and paint.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
