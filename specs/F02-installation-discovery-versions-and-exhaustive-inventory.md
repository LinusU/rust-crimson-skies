# F02: Installation discovery, versions, and exhaustive inventory

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F00, F01.
**Owner paths:** `crates/cs_assets/src/install.rs`; `crates/cs_types/src/install.rs`; `tools/cs_inspect/src/install.rs`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

CS_GAME_DIR and --cs-path select the original installed PC game. The explicit CLI argument wins. Discovery produces InstallManifest containing every regular source file, relative spelling, byte size, SHA-256, detected family, role and parse status. Fingerprints describe the actual installation, not merely an EXE version string.

## Non-negotiable behavior

1. Recognize full, partial, patched, localized and demo-like installations without conflating them. Primary target is an owner-provided full PC installation; no automated download of game media.
2. Enumerate ZBD/PLANES.ZBD, world groups and ROF candidates case-insensitively while preserving originals. Never require an English Windows install path.
3. The groups c1,c1b,c1c,c2,c2b,c3,c4,c5 are reference leads, not the authoritative mission list. Discover additional groups and report absent expected groups.
4. Classify every file as consumed, needed-unimplemented, optional-media, unused-with-reason, platform-support, or unknown. Unknown gameplay dependencies fail completeness.
5. Handle chapter CABs only after discovering their role and member inventory; installed-data support comes first. Missing assets report exact paths and affected content.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Inventory identical data copied under differently cased host paths; logical identity is stable.
**AC02:** A one-byte edit changes the fingerprint and invalidates cache entries.
**AC03:** A missing mission archive remains visible as unavailable, not omitted from the count.
**AC04:** A partial installation never passes the full-content readiness check.

## Bounded implementation slices

### F02-A: Define installation inventory and compatibility-profile schema

Dependencies: F00-A, F01-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f02_a_`. Minimum scenario: Inventory identical data copied under differently cased host paths; logical identity is stable.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F02-B: Implement safe discovery, hashing and diagnosis

Dependencies: F02-A, F00-C, F01-C. Required capabilities: retail.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f02_b_`. Minimum scenario: A one-byte edit changes the fingerprint and invalidates cache entries.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F02-C: Add cs-inspect inventory and dependency impact reports

Dependencies: F02-B. Required capabilities: retail.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f02_c_`. Minimum scenario: A missing mission archive remains visible as unavailable, not omitted from the count.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F02-D: Audit the full private installation with zero unclassified gameplay files

Dependencies: F02-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f02_d_`. Minimum scenario: A partial installation never passes the full-content readiness check.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No retail installation was supplied during authoring. File counts, patch hashes, locales, CAB member layout and mission path mappings must be discovered, not copied from another machine.

## References

[S03](../docs/research/SOURCES.md); [S04](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
