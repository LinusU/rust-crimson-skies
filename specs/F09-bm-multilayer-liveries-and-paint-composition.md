# F09: BM multilayer liveries and paint composition

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F05, F08.
**Owner paths:** `crates/cs_formats/src/bm.rs`; `crates/cs_content/src/livery.rs`; `crates/cs_app/src/livery.rs`; `crates/cs_formats/tests/bm.rs`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

The observed BM subset begins with u16 height then u16 width. It contains RGB base pixels, three 8-bit masks and a final RGBA overlay: total 4 + 10*w*h bytes. Keep these planes separate; do not interpret the final layer as a physically based specular map merely because a tool names it specular.

## Non-negotiable behavior

1. Validate the exact covered byte length and retain unsupported tails as variant diagnostics. The reference exports each plane vertically flipped; canonical row order must be tested, not double-flipped.
2. Reference composition multiplies the base by three mask-weighted colors then alpha-composites the RGBA overlay. Treat this as an observed tool algorithm until matched against retail.
3. Faction, custom colors, decals and airframe determine the material variant. Do not bake one faction into the only texture asset.
4. Extract palette choices and valid combinations from original data; hardcoded colors from the Blender helper are research leads, not the authoritative catalog.
5. Cache keys include all color, mask, decal, source and algorithm version inputs. Only changed variants invalidate; switching factions must not mutate other aircraft.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Parse the 2x3 synthetic BM and assert each channel plane and orientation.
**AC02:** All-zero/all-one masks test the exact composition endpoints.
**AC03:** Two planes share source images but choose different faction colors without cross-contamination.
**AC04:** Compare a private painted aircraft from several angles with the original, including decals and overlay alpha.

## Bounded implementation slices

### F09-A: Implement BM layout with rectangular golden fixtures

Dependencies: F03-A, F05-A, F08-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f09_a_`. Minimum scenario: Parse the 2x3 synthetic BM and assert each channel plane and orientation.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F09-B: Implement deterministic layered composition

Dependencies: F09-A, F03-C, F05-C, F08-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f09_b_`. Minimum scenario: All-zero/all-one masks test the exact composition endpoints.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F09-C: Wire faction/custom paints into model instances and construction preview

Dependencies: F09-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f09_c_`. Minimum scenario: Two planes share source images but choose different faction colors without cross-contamination.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F09-D: Verify known stock liveries and every discovered valid combination

Dependencies: F09-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f09_d_`. Minimum scenario: Compare a private painted aircraft from several angles with the original, including decals and overlay alpha.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S09](../docs/research/SOURCES.md); [S10](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
