# F62: Differential corpus, fuzzing, and regression closure

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F05, F06, F07, F08, F09, F10, F12, F13, F38, F59.
**Owner paths:** `crates/cs_formats/fuzz/`; `crates/cs_formats/tests/corpus/`; `tools/cs_xtask/src/corpus.rs`; `docs/findings/corpus/`.
**Shared contract:** [CLI-EVIDENCE](../docs/contracts/CLI-EVIDENCE.md).

## Deliverable and interfaces

Public CI uses synthetic/minimal redistributable fixtures; private CI exercises the full original corpus. Differential tests compare independently produced parse/decode outputs where reference tools support them. Every fixed bug receives a minimized regression case without leaking original copyrighted content.

## Non-negotiable behavior

1. Round-trip byte equality proves serialization consistency, not gameplay interpretation; include independent semantic invariants.
2. Fuzz parsers and mission validation with bounded memory/time and malformed nesting. Preserve crash seeds privately when minimization would expose protected bytes.
3. Reference version/hash/schema is recorded per comparison. Do not compare v0.6 geometry JSON to a newer incompatible schema and call differences bugs.
4. Every unknown referenced record and decoder discrepancy is in the ledger; no growing allowlist without rationale and affected-content scope.
5. CI reports actual test counts and private capability availability. A skipped private suite cannot make the public pipeline look like complete game verification.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Truncate every known container header and variable-length table boundary.
**AC02:** Regression fixture fails before and passes after the corresponding fix.
**AC03:** Mutate an opcode/texture length/parent index and get a bounded diagnostic.
**AC04:** Full corpus audit counts all inputs and links each unresolved discrepancy to blocked content.

## Bounded implementation slices

### F62-A: Define synthetic/private corpus separation and oracle contracts

Dependencies: F03-A, F05-A, F06-A, F07-A, F08-A, F09-A, F10-A, F12-A, F13-A, F38-A, F59-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f62_a_`. Minimum scenario: Truncate every known container header and variable-length table boundary.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F62-B: Implement fuzz/truncation/property regression runners

Dependencies: F62-A, F03-C, F05-C, F06-C, F07-C, F08-C, F09-C, F10-C, F12-C, F13-C, F38-C, F59-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f62_b_`. Minimum scenario: Regression fixture fails before and passes after the corresponding fix.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F62-C: Connect differential tools and completeness reports

Dependencies: F62-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f62_c_`. Minimum scenario: Mutate an opcode/texture length/parent index and get a bounded diagnostic.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F62-D: Close every release-critical parser/script discrepancy

Dependencies: F62-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f62_d_`. Minimum scenario: Full corpus audit counts all inputs and links each unresolved discrepancy to blocked content.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S02](../docs/research/SOURCES.md); [S03](../docs/research/SOURCES.md); [S05](../docs/research/SOURCES.md); [S07](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
