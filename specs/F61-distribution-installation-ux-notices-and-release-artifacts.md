# F61: Distribution, installation UX, notices, and release artifacts

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F01, F02, F45, F48, F51, F53, F60.
**Owner paths:** `docs/user/`; `tools/cs_xtask/src/package.rs`; `.github/workflows/`; `packaging/`; `tests/`.
**Shared contract:** [CLI-EVIDENCE](../docs/contracts/CLI-EVIDENCE.md).

## Deliverable and interfaces

Distribute the new engine and redistributable original project material only. First launch locates an owners installed original game, verifies supported content, explains missing/unsupported files and creates private cache/save folders. Releases include versioned compatibility reports, licenses and user instructions.

## Non-negotiable behavior

1. No original game textures, audio, scripts, fonts, executables, manual scans, extracted archives or decompiled source in release archives. A source hash manifest is not an asset bundle.
2. Preserve third-party notices and transitive dependency licensing. Reference-tool licensing decisions are recorded separately from new-engine code.
3. Do not present Ultimate Edition as official Microsoft/Zipper software. Branding, trademark and distribution decisions remain owner-reviewed, not legal assurances from this pack.
4. Release build startup must work outside the source checkout and without developer absolute paths.
5. Packaging tests inspect actual archive members and run the packaged binary against a private fixture installation; a successful cargo build is not a distribution test.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Scan a candidate package and fail on forbidden proprietary content or missing notices.
**AC02:** Run extracted release from a directory with spaces and non-ASCII characters.
**AC03:** Missing installation opens a useful selector/diagnostic rather than crashing.
**AC04:** Compare packaged and development build content fingerprints and basic behavior.

## Bounded implementation slices

### F61-A: Define release contents and user-data directory policy

Dependencies: F01-A, F02-A, F45-A, F48-A, F51-A, F53-A, F60-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f61_a_`. Minimum scenario: Scan a candidate package and fail on forbidden proprietary content or missing notices.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F61-B: Implement packaging and automated content/notices checks

Dependencies: F61-A, F01-C, F02-C, F45-C, F48-C, F51-C, F53-C, F60-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f61_b_`. Minimum scenario: Run extracted release from a directory with spaces and non-ASCII characters.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F61-C: Write first-run, troubleshooting and compatibility documentation

Dependencies: F61-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f61_c_`. Minimum scenario: Missing installation opens a useful selector/diagnostic rather than crashing.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F61-D: Test actual distributable artifacts on every supported platform

Dependencies: F61-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f61_d_`. Minimum scenario: Compare packaged and development build content fingerprints and basic behavior.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
