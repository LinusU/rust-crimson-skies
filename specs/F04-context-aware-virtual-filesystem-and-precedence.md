# F04: Context-aware virtual filesystem and precedence

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F02, F03.
**Owner paths:** `crates/cs_assets/src/vfs/`; `crates/cs_types/src/asset_id.rs`; `crates/cs_assets/tests/`; `tools/cs_inspect/src/resolve.rs`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

AssetKey is (mount namespace, logical path, variant). ResolveContext includes installation fingerprint, world group, locale, mission and mod stack. The VFS returns immutable SourceSpan plus a resolution trace. Files with equal basenames from different archives must remain distinct.

## Non-negotiable behavior

1. Normalize separators and ASCII case for legacy lookup, but retain original spelling. Reject .., absolute paths, drive prefixes, NUL and symlink escapes.
2. Specify precedence explicitly: opt-in mods over verified patch overlays over mission/world-specific sources over shared sources. Until original ordering is measured, label the baseline order designed and block conflicting retail resolutions.
3. Do not flatten all texture archives into a first-wins map. Ambiguous same-priority matches are diagnostic errors.
4. Mount lifetime belongs to a content session; in-flight reads survive unloading by owned backing storage, not dangling file handles.
5. VFS supports random reads without writing into the installation. Extracting is an explicit private research command, never an implicit runtime requirement.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Two worlds contain a same-named texture with different hashes; each resolves its own version.
**AC02:** Case-only collision at equal priority fails with both origins.
**AC03:** Malicious ZIP/ROF names cannot leave a private export directory.
**AC04:** Cancel an asynchronous read during world switch without use-after-free or cross-world texture reuse.

## Bounded implementation slices

### F04-A: Define AssetKey and precedence contracts

Dependencies: F02-A, F03-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f04_a_`. Minimum scenario: Two worlds contain a same-named texture with different hashes; each resolves its own version.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F04-B: Implement mounted sources and path validation

Dependencies: F04-A, F02-C, F03-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f04_b_`. Minimum scenario: Case-only collision at equal priority fails with both origins.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F04-C: Add resolve tracing and session mount lifecycle

Dependencies: F04-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f04_c_`. Minimum scenario: Malicious ZIP/ROF names cannot leave a private export directory.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F04-D: Compare original lookup behavior for every observed collision

Dependencies: F04-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f04_d_`. Minimum scenario: Cancel an asynchronous read during world switch without use-after-free or cross-world texture reuse.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S03](../docs/research/SOURCES.md); [S04](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
