# F07: INTERP loading-script container

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F06.
**Owner paths:** `crates/cs_formats/src/interp.rs`; `crates/cs_content/src/loading.rs`; `crates/cs_formats/tests/interp.rs`; `tools/cs_inspect/src/interp.rs`.
**Shared contract:** [SCRIPT-MISSION](../docs/contracts/SCRIPT-MISSION.md).

## Deliverable and interfaces

The inspected legacy parser expects a 12-byte header: u32 signature 0x08971119, version 7, script count. Each 128-byte index entry holds a 120-byte name, u32 timestamp and u32 script offset. Script lines are a u32 size, u32 argument count and size bytes; zero size terminates the script.

## Non-negotiable behavior

1. Preserve raw NUL-separated argument bytes as well as decoded tokens; reconstructing a joined string can destroy argument boundaries. A NUL count matching argument_count is observed behavior, not a universal whitespace grammar.
2. Validate index and script extents independently, require termination within the entry range, retain trailing/unknown regions as findings.
3. Do not call this a complete mission VM. First determine which commands load resources and which actually refer to game behavior.
4. A loading command resolves assets through the VFS and records its dependencies. Unknown commands fail the affected loading plan until classified.
5. Source timestamps are metadata only, never trusted cache identifiers. Content hashes determine identity.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Parse the generated synthetic INTERP file and preserve two arguments exactly.
**AC02:** Missing script terminator and inconsistent argument count fail.
**AC03:** Two scripts with equal names retain distinct origins.
**AC04:** An unsupported loading command yields its source offset and affected world, not a fake loaded state.

## Bounded implementation slices

### F07-A: Add INTERP raw records and golden synthetic fixture

Dependencies: F03-A, F06-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f07_a_`. Minimum scenario: Parse the generated synthetic INTERP file and preserve two arguments exactly.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F07-B: Implement lossless token decoder and validation

Dependencies: F07-A, F03-C, F06-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f07_b_`. Minimum scenario: Missing script terminator and inconsistent argument count fail.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F07-C: Build loading-plan adapter with dependency tracing

Dependencies: F07-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f07_c_`. Minimum scenario: Two scripts with equal names retain distinct origins.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F07-D: Audit installed loading commands and classify every opcode

Dependencies: F07-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f07_d_`. Minimum scenario: An unsupported loading command yields its source offset and affected world, not a fake loaded state.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Header facts are observed in mech3ax v0.6.0 source, not measured from the users installation. Full campaign scripting remains F13/F38.

## References

[S07](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
