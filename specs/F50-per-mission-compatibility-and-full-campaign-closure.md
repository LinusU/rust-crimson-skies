# F50: Per-mission compatibility and full-campaign closure

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F18, F19, F20, F24, F25, F27, F28, F29, F32, F33, F34, F35, F36, F38, F39, F40, F41, F42, F43, F44, F45, F46, F47.
**Owner paths:** `missions/bindings/`; `crates/cs_content/src/campaign_bindings.rs`; `crates/cs_app/tests/campaign/`; `docs/findings/missions/`.
**Shared contract:** [SCRIPT-MISSION](../docs/contracts/SCRIPT-MISSION.md).

## Deliverable and interfaces

The 24 mission sheets are acceptance work orders, not hand-authored replacement scripts. Bind each mission to actual catalog ids, world variant, program, actors, media, rewards and source hashes. Extend the inventory if the selected edition contains additional authored playable content.

## Non-negotiable behavior

1. Each mission needs a success run, failure run, retry/reentry check, optional/branch checks and full dependency report. Titles in this pack are discovery labels until matched to localized original ids.
2. Do not use cheats, direct objective mutation, forced success or skipped instructions as campaign completion evidence. Separate low-level injected-event tests from ordinary gameplay/controller-driven runs.
3. A successful run on one difficulty does not establish all difficulty variants. Enumerate actual original difficulty ids and cover every difficulty-sensitive branch.
4. All mission states are constructed from authored data, including forced airframes, captured craft, scripted alliances and chapter-start changes.
5. Aggregate completion fixes the denominator before testing. A missing/unparseable mission stays red; no filtering to the working subset.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Run all mission dependency closures and assert none are silently omitted.
**AC02:** Play M01 through M24 in progression order with profile continuity and final ending.
**AC03:** Retry selected missions after death, bailout, skip-media, save/restart and settings changes.
**AC04:** Exercise critical alternative-order and timed-branch cases from the mission sheets.

## Bounded implementation slices

### F50-A: Define complete mission binding/coverage records

Dependencies: F14-A, F13-A, F16-A. Required capabilities: ordinary build/test.

Owner ruling 2026-09-28 ([AUDIT-PLAN-SYNC](README.md#owner-ruling-2026-09-28-first-playable-sequencing-audit-plan-sync-356)): this is the early, engine-independent binding/coverage schema. Represent each required subsystem by a stable identity and an explicit unresolved dependency row instead of waiting for every subsystem's runtime schema. Preserve every required content category: mission identity, world variant and program; actors and forced/captured airframes; assets and media; objectives, branches and failure causes; interactions; rewards and progression; reference and evidence links. The denominator is fixed from the supported original inventory and cannot shrink; missing, unknown and unsupported rows are never unused and never ready. Synthetic fixtures prove the schema and validation only, not the campaign. F50-B/C/D still perform the complete campaign integration and verification with unchanged acceptance.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f50_a_`. Minimum scenario: Run all mission dependency closures and assert none are silently omitted.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F50-B: Bind discovered campaign identities and prerequisite closures

Dependencies: F50-A, F18-C, F19-C, F20-C, F24-C, F25-C, F27-C, F28-C, F29-C, F32-C, F33-C, F34-C, F35-C, F36-C, F38-C, F39-C, F40-C, F41-C, F42-C, F43-C, F44-C, F45-C, F46-C, F47-C. Required capabilities: retail.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f50_b_`. Minimum scenario: Play M01 through M24 in progression order with profile continuity and final ending.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F50-C: Build per-mission automated probes and human playtest routes

Dependencies: F50-B. Required capabilities: retail.

Owner ruling 2026-09-28: M01-B no longer waits for this stage (the first mission runs through VS-M01-RUNTIME and VS-M01-CONTROLLED-RUNS). F50-C still integrates the M01 route with every other mission route and still gates M02-B through M24-B.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f50_c_`. Minimum scenario: Retry selected missions after death, bailout, skip-media, save/restart and settings changes.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F50-D: Complete ordinary-play evidence for every mission and ending

Dependencies: F50-C, M01-C, M02-C, M03-C, M04-C, M05-C, M06-C, M07-C, M08-C, M09-C, M10-C, M11-C, M12-C, M13-C, M14-C, M15-C, M16-C, M17-C, M18-C, M19-C, M20-C, M21-C, M22-C, M23-C, M24-C. Required capabilities: retail, gpu, audio, human_play.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f50_d_`. Minimum scenario: Exercise critical alternative-order and timed-branch cases from the mission sheets.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

All original mission data and real gameplay evidence are mandatory. The package author did not run the original or rebuilt game; every mission starts unverified.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Exact mission programs, positions, timings, object ids, rewards and failure priorities must come from the private installation and reference observations. The public guide is not an executable specification.

## References

[S14](../docs/research/SOURCES.md); [S15](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
