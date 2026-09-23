# F05: ROF directory trees and compressed members

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F04.
**Owner paths:** `crates/cs_formats/src/rof.rs`; `crates/cs_formats/tests/rof.rs`; `crates/cs_assets/src/rof.rs`; `tools/cs_inspect/src/rof.rs`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Implement a ROF reader from observed layout, not a guessed generic ZIP wrapper. A directory starts with little-endian u32 entry_count and u32 names_length, then entry_count records of six u32 fields, then the name table. Preserve the two length fields as raw_length and raw_length_on_disk until a local corpus resolves their meaning.

## Non-negotiable behavior

1. Observed record fields are start, length, length_on_disk, flags, name_length, id; flags 1 and 2 indicate directory and compression in the reference script. Root is read from offset zero in that script.
2. Directory names are NUL-delimited; validate the count, termination and declared lengths. Do not assume UTF-8 is valid in every locale.
3. Directory offsets are followed with cycle detection and bounded depth. Compressed data uses a bounded zlib decoder only after the entry extent is established.
4. The reference extractor reads length for compressed entries and ignores length_on_disk. Do not silently reinterpret those fields from their names. Probe asymmetric fixtures and real compressed members, record exact boundaries and trailing data.
5. An unknown flags combination or overlap is not automatically fatal if independently documented as legitimate sharing; absent that evidence, surface UnsupportedLayout rather than extracting arbitrary spans.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Synthetic uncompressed tree has two directories, duplicate basenames and stable ids.
**AC02:** Test compressed data where stored and decoded lengths differ; the selected profile must explain both.
**AC03:** Cycle, invalid name table, outside-file pointer and expansion bomb fail before writes.
**AC04:** Byte-identical member reads match an independently configured reference tool on private data.

## Bounded implementation slices

### F05-A: Define raw ROF structs and synthetic boundary fixtures

Dependencies: F03-A, F04-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f05_a_`. Minimum scenario: Synthetic uncompressed tree has two directories, duplicate basenames and stable ids.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F05-B: Implement directory traversal and bounded member reads

Dependencies: F05-A, F03-C, F04-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f05_b_`. Minimum scenario: Test compressed data where stored and decoded lengths differ; the selected profile must explain both.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F05-C: Mount ROF into VFS and expose inspection

Dependencies: F05-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f05_c_`. Minimum scenario: Cycle, invalid name table, outside-file pointer and expansion bomb fail before writes.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F05-D: Resolve compressed length semantics and audit all private ROF members

Dependencies: F05-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f05_d_`. Minimum scenario: Byte-identical member reads match an independently configured reference tool on private data.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

The compressed length-field semantics are a known research blocker. The provided synthetic fixture covers only the uncompressed observed subset; it is not proof of retail compressed support.

## References

[S05](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
