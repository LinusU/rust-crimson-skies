# F01: Evidence ledger, provenance, and reference policy

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F00.
**Owner paths:** `crates/cs_types/src/evidence.rs`; `tools/cs_inspect/src/evidence.rs`; `docs/findings/`; `tests/`.
**Shared contract:** [CLI-EVIDENCE](../docs/contracts/CLI-EVIDENCE.md).

## Deliverable and interfaces

Every factual compatibility claim has a ClaimId and a status: documented, observed_tool, verified_original, inferred, designed, unknown or contradicted. Evidence references a source, exact revision or game fingerprint, locator, observation method and limitations. Automated test success is a separate field, never the evidence status.

## Non-negotiable behavior

1. Preserve contradictions instead of choosing the convenient source. A changed source or asset fingerprint invalidates dependent claims.
2. Original data stays outside Git and is opened read-only. No game executables, decompiled game source, extracted art, voice, fonts or commercial manuals in fixtures or releases.
3. Do not copy EUPL reference implementation code into permissive files without a separately reviewed licensing decision. Reference-tool use is not an automatic license exemption.
4. Research reports contain offsets, sizes, identifiers and short diagnostic samples only; keep full extracts private.
5. Unknown rules cannot become verified because a test repeats an invented value. Distinguish an engineering safety limit from an original game constant.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Reject a verified_original claim without a content hash and observation locator.
**AC02:** Invalidate a dependent claim after changing the asset fingerprint.
**AC03:** Preserve two disagreeing source claims and the adjudication state.
**AC04:** Detect a committed retail binary in the release inventory.

## Bounded implementation slices

### F01-A: Define claim and evidence records

Dependencies: F00-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f01_a_`. Minimum scenario: Reject a verified_original claim without a content hash and observation locator.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F01-B: Implement ledger validation and dependency invalidation

Dependencies: F01-A, F00-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f01_b_`. Minimum scenario: Invalidate a dependent claim after changing the asset fingerprint.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F01-C: Wire provenance into inspector and content exports

Dependencies: F01-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f01_c_`. Minimum scenario: Preserve two disagreeing source claims and the adjudication state.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F01-D: Review licensing and factual provenance for the first vertical slice

Dependencies: F01-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f01_d_`. Minimum scenario: Detect a committed retail binary in the release inventory.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S01](../docs/research/SOURCES.md); [S02](../docs/research/SOURCES.md); [S03](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
