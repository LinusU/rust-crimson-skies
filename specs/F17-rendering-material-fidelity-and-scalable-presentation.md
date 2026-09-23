# F17: Rendering, material fidelity, and scalable presentation

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F08, F09, F10, F11, F15, F16.
**Owner paths:** `crates/cs_app/src/render/`; `crates/cs_app/assets/shaders/`; `crates/cs_app/tests/render/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Render original meshes, textures, colors and ordered effects with explicit material classes: opaque, masked, blended, additive, emissive and other observed classes. Fidelity mode preserves authored appearance; enhanced options are independently switchable and cannot alter collision or visibility rules.

## Non-negotiable behavior

1. Preserve alpha test, two-sidedness, texture addressing, vertex colors and material ordering. Do not apply one generic physically based material to every surface.
2. Handle translucent layers consistently, including propellers, glass, sprites and smoke. Report sorting limitations instead of hiding geometry.
3. Camera exposure, tonemapping and gamma are fixed in comparison mode; original texture decoding and GPU sRGB sampling must not double-correct colors.
4. Instancing and batching retain per-instance livery and damage state. LOD/culling cannot remove gameplay colliders or mission objects.
5. Modern resolution, antialiasing and optional shadows are designed improvements, not proof of original parity. No automatic texture upscaling or asset redistribution pipeline.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Golden synthetic scene contains overlapping glass, alpha-cut fence, additive sprite and per-corner colors.
**AC02:** Screenshot same camera/tick twice under fixed comparison settings.
**AC03:** Two instances with different paint/damage remain visually independent after batching.
**AC04:** Original comparison set includes cockpit, skyline, vegetation, night effects and close-up aircraft.

## Bounded implementation slices

### F17-A: Build material classification and render test scene

Dependencies: F08-A, F09-A, F10-A, F11-A, F15-A, F16-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f17_a_`. Minimum scenario: Golden synthetic scene contains overlapping glass, alpha-cut fence, additive sprite and per-corner colors.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F17-B: Implement canonical mesh/image to Bevy adapters

Dependencies: F17-A, F08-C, F09-C, F10-C, F11-C, F15-C, F16-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f17_b_`. Minimum scenario: Screenshot same camera/tick twice under fixed comparison settings.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F17-C: Add faithful and optional enhanced rendering profiles

Dependencies: F17-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f17_c_`. Minimum scenario: Two instances with different paint/damage remain visually independent after batching.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F17-D: Review original-data screenshot matrix and material coverage

Dependencies: F17-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f17_d_`. Minimum scenario: Original comparison set includes cockpit, skyline, vegetation, night effects and close-up aircraft.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
