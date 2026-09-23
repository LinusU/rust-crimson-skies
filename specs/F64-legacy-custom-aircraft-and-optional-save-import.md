# F64: Legacy custom aircraft and optional save import

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F02, F03, F12, F44, F48.
**Owner paths:** `crates/cs_formats/src/legacy_profile/`; `crates/cs_content/src/legacy_import.rs`; `crates/cs_app/src/ui/import.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

Inventory original custom-aircraft and profile/save formats. Custom-aircraft import is required when an original content path references it. Existing campaign-save import is a separately labeled compatibility enhancement: inability to import an old save does not justify corrupting it or blocking a new-engine fresh campaign.

## Non-negotiable behavior

1. Never overwrite original save/blueprint files. Import to a new profile and retain source fingerprint and migration report.
2. Unknown fields remain unresolved; do not guess currency, mission index or equipment enums from nearby numbers.
3. Map old ids through verified original content identities, not current list positions.
4. Validate imported blueprints with the same constructor/host rules. Imported progression requires source integrity and explicit owner action.
5. Clearly distinguish full import, partial import and unsupported version; no silent reset to a blank profile called imported.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A malicious/oversized old profile fails without touching source or new saves.
**AC02:** A legacy blueprint violating current stock constraints is rejected with specific fields.
**AC03:** Reordering catalog entries does not remap an imported weapon to another type.
**AC04:** Optional old-save migration can be disabled while new campaigns still work.

## Bounded implementation slices

### F64-A: Define legacy-import inventory and validation contracts

Dependencies: F02-A, F03-A, F12-A, F44-A, F48-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f64_a_`. Minimum scenario: A malicious/oversized old profile fails without touching source or new saves.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F64-B: Implement verified read-only import subset

Dependencies: F64-A, F02-C, F03-C, F12-C, F44-C, F48-C. Required capabilities: retail.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f64_b_`. Minimum scenario: A legacy blueprint violating current stock constraints is rejected with specific fields.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F64-C: Build import validation and user-facing migration report

Dependencies: F64-B. Required capabilities: retail.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f64_c_`. Minimum scenario: Reordering catalog entries does not remap an imported weapon to another type.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F64-D: Verify original custom aircraft and explicitly scoped optional saves

Dependencies: F64-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f64_d_`. Minimum scenario: Optional old-save migration can be disabled while new campaigns still work.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
