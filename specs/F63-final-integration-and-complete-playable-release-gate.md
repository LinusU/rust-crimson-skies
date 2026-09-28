# F63: Final integration and complete-playable release gate

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F26, F47, F49, F50, F52, F53, F56, F58, F60, F61, F62.
**Owner paths:** `docs/findings/release/`; `tools/cs_xtask/src/release.rs`; `tests/release/`.
**Shared contract:** [CLI-EVIDENCE](../docs/contracts/CLI-EVIDENCE.md).

## Deliverable and interfaces

The complete-playable target requires original campaign from fresh profile through ending, all original IA presets/customization, all discovered original multiplayer content through the new networking implementation, construction, progression, original media, menus, save/retry and stable platform packaging. Asset extraction or a single flyable world is not completion.

## Non-negotiable behavior

1. Require zero missing critical dependencies, zero reachable unsupported script instructions and no synthetic fallback in retail launches.
2. Require ordinary-play success evidence for every mission plus failure/retry and critical branch coverage. Final approval includes human visual/audio/gameplay review bound to the candidate tree and content fingerprints.
3. Separate completed, tested, original-verified and release-approved counts. Do not collapse them into a green percentage.
4. Every accepted deviation has a user-visible description and classification. Open critical fidelity or playability issues block the complete label.
5. A release report is generated from evidence manifests and coverage inventories, not manually edited success prose.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Fresh-install acceptance uses only packaged engine plus owners original files.
**AC02:** Play complete campaign with saved progression across several process restarts.
**AC03:** Exercise every original IA/multiplayer mode and loadout family.
**AC04:** Remove one required mission opcode implementation and prove the release gate turns red.

## Bounded implementation slices

### F63-A: Define machine-readable complete-playable criteria

Dependencies: F01-A, F50-A. Required capabilities: ordinary build/test.

Owner ruling 2026-09-28 ([AUDIT-PLAN-SYNC](README.md#owner-ruling-2026-09-28-first-playable-sequencing-audit-plan-sync-356)): this stage moves early and also waits for AUDIT-EVIDENCE-MODEL (#354). Derive the machine-readable criteria from the evidence model and the F50-A binding/coverage schema, not from already working game systems. Define three separate milestones (one real original mission, complete single-player candidate, complete Ultimate release) and enumerate every product category of this sheet, including the deliverables of F26, F47, F49, F52, F53, F56, F58, F60, F61 and F62 whose A stages are no longer scheduling prerequisites; they stay required criteria. Keep implemented, checked, original-verified and release-approved separate. Data not yet available is `not_assessed`, never assumed. Synthetic manifests can never grant original verification. F63-B/C/D keep full product integration and human approval.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f63_a_`. Minimum scenario: Synthetic complete/incomplete product manifests exercise every required criterion without claiming a real playthrough.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F63-B: Implement aggregate completeness and freshness validation

Dependencies: F63-A, F26-C, F47-C, F49-C, F50-C, F52-C, F53-C, F56-C, F58-C, F60-C, F61-C, F62-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f63_b_`. Minimum scenario: A stale candidate/content hash or missing mission evidence makes aggregate completeness validation fail.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F63-C: Execute full product acceptance and fix regressions

Dependencies: F63-B, F56-D, F58-D, F64-D, M01-C, M02-C, M03-C, M04-C, M05-C, M06-C, M07-C, M08-C, M09-C, M10-C, M11-C, M12-C, M13-C, M14-C, M15-C, M16-C, M17-C, M18-C, M19-C, M20-C, M21-C, M22-C, M23-C, M24-C. Required capabilities: retail, gpu, audio, network_real, human_play, human_review.

Owner ruling 2026-09-28: this stage also waits for the verification tasks listed in the [owner ruling](README.md#owner-ruling-2026-09-28-first-playable-sequencing-audit-plan-sync-356): world collision (#333), non-Unix VFS hardening (#339), original lookup order (#341), archive-member collisions (#342), keyed-list reading rules (#351), texture-archive selection (#352), the evidence integrity, model and producer work (#353, #354, #355), this plan sync (#356), the first original capture (#358), M01 controlled runs (#360) and Windows CI (#361). F63-D depends on this stage, so all of them gate release approval.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f63_c_`. Minimum scenario: Exercise every original IA/multiplayer mode and loadout family.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F63-D: Obtain explicit owner release approval with linked evidence

Dependencies: F00-D, F01-D, F02-D, F03-D, F04-D, F05-D, F06-D, F07-D, F08-D, F09-D, F10-D, F11-D, F12-D, F13-D, F14-D, F15-D, F16-D, F17-D, F18-D, F19-D, F20-D, F21-D, F22-D, F23-D, F24-D, F25-D, F26-D, F27-D, F28-D, F29-D, F30-D, F31-D, F32-D, F33-D, F34-D, F35-D, F36-D, F37-D, F38-D, F39-D, F40-D, F41-D, F42-D, F43-D, F44-D, F45-D, F46-D, F47-D, F48-D, F49-D, F50-D, F51-D, F52-D, F53-D, F54-D, F55-D, F56-D, F57-D, F58-D, F59-D, F60-D, F61-D, F62-D, F63-C, M01-C, M02-C, M03-C, M04-C, M05-C, M06-C, M07-C, M08-C, M09-C, M10-C, M11-C, M12-C, M13-C, M14-C, M15-C, M16-C, M17-C, M18-C, M19-C, M20-C, M21-C, M22-C, M23-C, M24-C, F64-D. Required capabilities: retail, gpu, audio, network_real, human_play, human_review.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f63_d_`. Minimum scenario: Remove one required mission opcode implementation and prove the release gate turns red.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
