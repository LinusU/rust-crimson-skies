# F06: ZBD families, reader archives, and sound containers

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F04.
**Owner paths:** `crates/cs_formats/src/zbd/`; `crates/cs_assets/src/zbd.rs`; `crates/cs_formats/tests/zbd/`; `tools/cs_inspect/src/zbd.rs`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

ZBD is a family label, not one universal file layout. Dispatch by validated header/version and installation role into distinct sound, reader, texture, interp, GameZ and animation readers. Each reader reports consumed ranges and unsupported records.

## Non-negotiable behavior

1. Inspect the pinned reference implementations before defining field offsets. Do not reuse a MechWarrior format solely because it shares the extension.
2. Sound entries retain sample format, channels, rate, loop metadata if present, and source span. Reader entries retain byte content and encoding evidence.
3. Preserve duplicate entry names and numeric ids. Archive structural parse success does not prove semantic interpretation.
4. Invalid members fail their affected content closure; a diagnostic listing may continue and show every error without advertising playability.
5. The runtime may read members directly or through a versioned private cache; a mandatory Windows extractor or Blender preprocessing step is not the end-state architecture.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Route at least two distinct synthetic ZBD-family headers to different readers.
**AC02:** A valid header with incompatible family data fails explicitly, never falls back to another parser silently.
**AC03:** Decode a short sound sample and compare byte/sample count with its declared format.
**AC04:** Show a corrupt member alongside valid siblings in audit output while returning nonzero strict status.

## Bounded implementation slices

### F06-A: Inventory observed ZBD families and define dispatch

Dependencies: F03-A, F04-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f06_a_`. Minimum scenario: Route at least two distinct synthetic ZBD-family headers to different readers.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F06-B: Implement reader and sound container subset with bounds

Dependencies: F06-A, F03-C, F04-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f06_b_`. Minimum scenario: A valid header with incompatible family data fails explicitly, never falls back to another parser silently.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F06-C: Connect ZBD member producers to VFS and audio assets

Dependencies: F06-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f06_c_`. Minimum scenario: Decode a short sound sample and compare byte/sample count with its declared format.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F06-D: Complete family-by-family private corpus coverage

Dependencies: F06-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f06_d_`. Minimum scenario: Show a corrupt member alongside valid siblings in audit output while returning nonzero strict status.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Precise sound/reader header variants must be read from the pinned source and checked against the installed game. No universal ZBD header is asserted by this specification.

## References

[S02](../docs/research/SOURCES.md); [S06](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
