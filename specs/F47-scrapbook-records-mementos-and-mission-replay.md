# F47: Scrapbook, records, mementos, and mission replay

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F42, F43, F45, F48.
**Owner paths:** `crates/cs_content/src/scrapbook.rs`; `crates/cs_app/src/ui/scrapbook/`; `crates/cs_sim/src/records.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

Scrapbook entries link original images/text/mementos to mission, stunt, ace and campaign outcomes. Persist distinct latest/best records where required, kill/trophy classifications and unlocked page state. Replay launches the correct original mission variant through the normal loading path.

## Non-negotiable behavior

1. Unlock predicates are data/evidence-backed, not award-everything-on-success. Hidden pages remain hidden until their rule is satisfied.
2. Stable ids prevent page reorder or localization from moving achievements between missions.
3. Keep imported artwork private at runtime; optional export of a user-owned screenshot/media needs an explicit local action and no automatic upload.
4. Updating a best score does not erase a newer latest-run record. Tie handling and difficulty scope are documented.
5. Cabin memento choice persists independently of campaign progress and uses only unlocked assets.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A better replay updates best but not unrelated progression.
**AC02:** Unlock one stunt photo without revealing other mission rewards.
**AC03:** Change locale and verify all saved entry ids still resolve.
**AC04:** Audit every discovered scrapbook page, memento and replay link against original progression.

## Bounded implementation slices

### F47-A: Define scrapbook records and unlock predicates

Dependencies: F14-A, F42-A, F43-A, F45-A, F48-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f47_a_`. Minimum scenario: A better replay updates best but not unrelated progression.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F47-B: Implement idempotent record and memento persistence

Dependencies: F47-A, F14-C, F42-C, F43-C, F45-C, F48-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f47_b_`. Minimum scenario: Unlock one stunt photo without revealing other mission rewards.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F47-C: Build paged scrapbook UI and replay launch

Dependencies: F47-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f47_c_`. Minimum scenario: Change locale and verify all saved entry ids still resolve.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F47-D: Verify all original scrapbook content and unlock paths

Dependencies: F47-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f47_d_`. Minimum scenario: Audit every discovered scrapbook page, memento and replay link against original progression.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
