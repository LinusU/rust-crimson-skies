# F52: Accessibility and explicitly separated modern options

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F21, F22, F41, F45, F46, F51.
**Owner paths:** `crates/cs_app/src/accessibility/`; `crates/cs_content/src/settings.rs`; `crates/cs_app/tests/accessibility/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Ultimate presentation includes remappable controls, scalable UI, subtitles, separate audio buses, color-independent target cues, reduced camera shake/flash, optional modern mouse/controller flight and resolution/FOV support. These are project-designed improvements; original-rule and modern-assist settings have separate profiles.

## Non-negotiable behavior

1. Accessibility settings must not silently change combat timing, weapon damage or mission unlocks. Any gameplay assist is visibly labeled and evidence runs declare it.
2. Never communicate objective status solely by red/green color. Keep shapes/text/icon alternatives.
3. UI can be navigated without mouse; control remapping permits cancel/reset and does not strand the user.
4. Reduced motion disables cosmetic shake without suppressing required damage/target notifications.
5. Persist settings atomically and provide a safe defaults startup flag if a display/input configuration becomes unusable.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Complete navigation from boot to launch using keyboard only and controller only.
**AC02:** Read objectives with color filters disabled and at large UI scale.
**AC03:** Turn off shake/flash and verify gameplay telemetry unchanged.
**AC04:** Enable a gameplay assist and ensure comparison/replay metadata clearly records it.

## Bounded implementation slices

### F52-A: Define accessibility settings and fidelity boundaries

Dependencies: F21-A, F22-A, F41-A, F45-A, F46-A, F51-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f52_a_`. Minimum scenario: Complete navigation from boot to launch using keyboard only and controller only.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F52-B: Implement UI/audio/visual accessibility options

Dependencies: F52-A, F21-C, F22-C, F41-C, F45-C, F46-C, F51-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f52_b_`. Minimum scenario: Read objectives with color filters disabled and at large UI scale.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F52-C: Integrate safe recovery and labeled modern control profiles

Dependencies: F52-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f52_c_`. Minimum scenario: Turn off shake/flash and verify gameplay telemetry unchanged.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F52-D: Review accessibility flows without changing original mission semantics

Dependencies: F52-C. Required capabilities: gpu.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f52_d_`. Minimum scenario: Enable a gameplay assist and ensure comparison/replay metadata clearly records it.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
