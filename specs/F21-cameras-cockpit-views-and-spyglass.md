# F21: Cameras, cockpit views, and spyglass

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F11, F16, F17, F22, F30.
**Owner paths:** `crates/cs_app/src/camera/`; `crates/cs_content/src/cameras.rs`; `crates/cs_app/tests/camera/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Camera modes include original cockpit and external views, look directions, target tracking, spyglass magnification and authored camera sequences as discovered. CameraRig consumes authoritative aircraft pose but never writes flight state. Modern free-look/controller support is separate from original default mappings.

## Non-negotiable behavior

1. Cockpit viewpoint comes from verified model/config bindings. HUD-only synthetic camera is not a replacement for every original cockpit.
2. Projection uses aspect-correct FOV; define vertical vs horizontal conversion and preserve scene framing across 4:3,16:9 and ultrawide without stretching art.
3. Spyglass shows the selected target, handles invalid targets, and obeys its own near/far rendering requirements. It must not change target authority or fire direction unless evidence says so.
4. Camera smoothing is frame-rate independent, reset on teleport/plane swap and preserved correctly through origin shifts.
5. Screenshot CLI accepts a reproducible world pose, mission id, tick and deterministic settings; report all overrides.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Compare framing at three aspect ratios using an invariant world-space target.
**AC02:** Destroy or switch the spyglass target mid-frame without stale entity access.
**AC03:** Swap aircraft during a scripted capture and verify camera binds to the new player body.
**AC04:** Match original cockpit/view behaviors with recorded input and captures.

## Bounded implementation slices

### F21-A: Define camera modes and projection policy

Dependencies: F11-A, F16-A, F17-A, F22-A, F30-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f21_a_`. Minimum scenario: Compare framing at three aspect ratios using an invariant world-space target.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F21-B: Implement cockpit/chase/look/spyglass rigs

Dependencies: F21-A, F11-C, F16-C, F17-C, F22-C, F30-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f21_b_`. Minimum scenario: Destroy or switch the spyglass target mid-frame without stale entity access.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F21-C: Integrate script cameras and deterministic capture flags

Dependencies: F21-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f21_c_`. Minimum scenario: Swap aircraft during a scripted capture and verify camera binds to the new player body.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F21-D: Verify original view controls and cockpit coverage

Dependencies: F21-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f21_d_`. Minimum scenario: Match original cockpit/view behaviors with recorded input and captures.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
