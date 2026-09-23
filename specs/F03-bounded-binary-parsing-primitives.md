# F03: Bounded binary parsing primitives

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F00.
**Owner paths:** `crates/cs_formats/src/io.rs`; `crates/cs_formats/src/error.rs`; `crates/cs_formats/tests/`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Build a safe byte reader over immutable byte slices with explicit little-endian integer and float reads, checked ranges, bounded strings, checked multiplication and contextual ParseError. Errors retain archive, offset, field, expected condition and observed value without dumping unrelated private data.

## Non-negotiable behavior

1. No unsafe transmute of file bytes into Rust structs. Alignment and host endianness must not matter.
2. Apply independent limits for bytes, entries, dimensions, recursion, allocations and decompressed output. Defaults are designed safety budgets and configurable only within tested ranges.
3. Reject nonfinite transforms, invalid indices, range overflow, directory cycles and impossible texture dimensions before allocating GPU resources.
4. Unknown bytes remain byte ranges with provenance; no consuming the rest as padding without evidence.
5. Parsing must be independent of renderer, window, network, game state and asset directory enumeration.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** For every reader operation truncate at each byte boundary and assert error, never panic.
**AC02:** Exercise u32::MAX counts and offset-plus-length overflow without allocation.
**AC03:** Parse deliberately unaligned slices on supported targets.
**AC04:** Fuzz nested range and string decoding with deterministic seeds.

## Bounded implementation slices

### F03-A: Implement checked reader and structured errors

Dependencies: F00-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f03_a_`. Minimum scenario: For every reader operation truncate at each byte boundary and assert error, never panic.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F03-B: Add bounded allocation and recursion utilities

Dependencies: F03-A, F00-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f03_b_`. Minimum scenario: Exercise u32::MAX counts and offset-plus-length overflow without allocation.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F03-C: Integrate contextual errors into all parser entrypoints

Dependencies: F03-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f03_c_`. Minimum scenario: Parse deliberately unaligned slices on supported targets.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F03-D: Run truncation and fuzz corpus with recorded resource limits

Dependencies: F03-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f03_d_`. Minimum scenario: Fuzz nested range and string decoding with deterministic seeds.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
