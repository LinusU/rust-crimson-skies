# F08: Texture archives and conventional image decoding

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F06.
**Owner paths:** `crates/cs_formats/src/texture/`; `crates/cs_content/src/textures.rs`; `crates/cs_formats/tests/texture/`; `tools/cs_inspect/src/textures.rs`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Decode the actual CS texture archive variants into image descriptors with size, layout, row order, channels, palette, mip levels, alpha interpretation and color-space provenance. Conventional TIFF/TGA/BMP or other discovered files use audited decoders; extension alone does not decide byte format.

## Non-negotiable behavior

1. Keep palette transparency, alpha-test thresholds and mip metadata separate. A black texel is not automatically transparent.
2. Limit dimensions and decoded allocations; validate every mip level and palette index. Reject partial images for release content rather than silently resizing.
3. Raw pixel decoding performs no stylistic enhancement. GPU upload and color-space conversion happen once at the presentation boundary.
4. Texture identity includes origin namespace and variant, not only the name. Alias tables require source evidence and collision tests.
5. Differential decoding compares dimensions, channel bytes and orientation independently from an attractive screenshot.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Use a 3x2 asymmetric color fixture to expose row/column and vertical inversion errors.
**AC02:** Exercise palette index out of range, alpha edge and non-square mip chains.
**AC03:** Resolve same-name textures in two chapter archives correctly.
**AC04:** Compare private decodes against the pinned reference with zero unexplained pixel differences.

## Bounded implementation slices

### F08-A: Define image descriptors and asymmetric fixtures

Dependencies: F03-A, F06-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f08_a_`. Minimum scenario: Use a 3x2 asymmetric color fixture to expose row/column and vertical inversion errors.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F08-B: Implement verified texture variants and conventional readers

Dependencies: F08-A, F03-C, F06-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f08_b_`. Minimum scenario: Exercise palette index out of range, alpha edge and non-square mip chains.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F08-C: Connect images to the content catalog and GPU upload boundary

Dependencies: F08-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f08_c_`. Minimum scenario: Resolve same-name textures in two chapter archives correctly.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F08-D: Produce a private texture contact sheet and full decode audit

Dependencies: F08-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f08_d_`. Minimum scenario: Compare private decodes against the pinned reference with zero unexplained pixel differences.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Do not assume DXTC, 565, indexed color or any particular compression until the archive variant establishes it. Missing decode support remains a release blocker for referenced textures.

## References

[S02](../docs/research/SOURCES.md); [S03](../docs/research/SOURCES.md); [S08](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
