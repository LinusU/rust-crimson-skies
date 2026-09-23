# F48: Profiles, saves, settings, migration, and recovery

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F00, F01, F14.
**Owner paths:** `crates/cs_content/src/save/`; `crates/cs_types/src/profile.rs`; `crates/cs_app/src/profile.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

New-engine saves are versioned documents with profile id, monotonic revision, campaign state, owned blueprints, records, settings and compatibility fingerprints. Store them outside installation and repository. Whole-document writes are atomic, with backup and recovery diagnostics; legacy compatibility is F64.

## Non-negotiable behavior

1. Validate size/ranges/ids before use and preserve safely ignorable future fields. Unsupported major versions fail without overwrite.
2. Atomic save uses temporary file, flush/fsync where supported, rename and directory sync with platform-specific behavior tested. Recovery selects the highest valid revision without inventing a merged state.
3. Profile ids are persistent identifiers, not display names or recycled list indexes. Deleting the highest id must not reuse it.
4. Separate production profiles from synthetic/modded/evidence sessions. Automated tests never touch the users live profile directory.
5. Settings changes that require restart are labeled; invalid device/display settings have a safe recovery path.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Interrupt every save-write phase and reopen without losing both current and backup.
**AC02:** Corrupt the newest file and recover a valid backup with a visible warning.
**AC03:** Create/delete/create profiles and prove old ids never refer to a new profile.
**AC04:** Load a future/oversized/malicious save without panic, traversal or destructive overwrite.

## Bounded implementation slices

### F48-A: Define versioned profile and save schema

Dependencies: F00-A, F01-A, F14-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f48_a_`. Minimum scenario: Interrupt every save-write phase and reopen without losing both current and backup.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F48-B: Implement atomic persistence, ids and recovery

Dependencies: F48-A, F00-C, F01-C, F14-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f48_b_`. Minimum scenario: Corrupt the newest file and recover a valid backup with a visible warning.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F48-C: Wire settings, campaign and sandbox profile ownership

Dependencies: F48-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f48_c_`. Minimum scenario: Create/delete/create profiles and prove old ids never refer to a new profile.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F48-D: Run crash/recovery matrix on every supported platform

Dependencies: F48-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f48_d_`. Minimum scenario: Load a future/oversized/malicious save without panic, traversal or destructive overwrite.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
