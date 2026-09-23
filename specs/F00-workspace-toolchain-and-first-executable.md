# F00: Workspace, toolchain, and first executable

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** None.
**Owner paths:** `Cargo.toml`; `Cargo.lock`; `rust-toolchain.toml`; `.github/`; `crates/`; `tools/cs_inspect/`; `tools/cs_xtask/`.
**Shared contract:** [CLI-EVIDENCE](../docs/contracts/CLI-EVIDENCE.md).

## Deliverable and interfaces

Create a Rust 2024 workspace with cs_types, cs_formats, cs_assets, cs_content, cs_sim, cs_script, cs_net, cs_app, cs_inspect and cs_xtask. The application binary is cs; the inspector binary is cs-inspect. The intended baseline is Bevy 0.19 and Avian3d 0.7, matching the inspected MM2 repository. Bootstrap must resolve and compile that pair, then pin exact patches and the Rust toolchain in Cargo.lock and rust-toolchain.toml.

## Non-negotiable behavior

1. No Bevy dependency in cs_types, cs_formats, cs_assets, cs_content, cs_script or cs_net. cs_sim may use bevy_ecs/math only through an approved boundary; physics adapter stays in cs_app.
2. Create an asset-free development scene, explicitly marked SYNTHETIC; it is never selected as a replacement for missing retail content.
3. Default cargo run selects the game. --help and --version work without a GPU or retail installation; --headless smoke creates no window.
4. Establish fmt, clippy with -D warnings, workspace tests, positive test-selection checks and a headless application smoke. Commit Cargo.lock; subsequent gates use --locked.
5. Do not copy the complete MM2 dependency graph or claim its source license by implication. New code licensing is an owner decision, with the supplied policy as a proposal.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Compile the pinned Bevy/Avian pair and create a dynamic synthetic body.
**AC02:** Run --help with CS_GAME_DIR unset; exit zero and no asset discovery side effects.
**AC03:** Run a fixed-tick synthetic smoke twice; both end at the requested tick count.
**AC04:** Remove a required workspace member and prove the gate fails.

## Bounded implementation slices

### F00-A: Create the minimal workspace and command-line smoke

Dependencies: none. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f00_a_`. Minimum scenario: Compile the pinned Bevy/Avian pair and create a dynamic synthetic body.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F00-B: Pin compatible dependencies and document the schedule API

Dependencies: F00-A. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f00_b_`. Minimum scenario: Run --help with CS_GAME_DIR unset; exit zero and no asset discovery side effects.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F00-C: Install CI plus task-specific positive-test discovery

Dependencies: F00-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f00_c_`. Minimum scenario: Run a fixed-tick synthetic smoke twice; both end at the requested tick count.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F00-D: Run platform bootstrap evidence and freeze toolchain

Dependencies: F00-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f00_d_`. Minimum scenario: Remove a required workspace member and prove the gate fails.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

This is infrastructure, not a retail claim. Attach compiler versions, cargo metadata and actual successful command logs.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

The pack does not ship a compiled Cargo workspace. Dependency compatibility must be verified locally before other game work; do not automatically upgrade to a different major/minor pair.

## References

[S01](../docs/research/SOURCES.md); [S11](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
