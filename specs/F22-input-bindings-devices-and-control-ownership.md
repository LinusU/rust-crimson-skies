# F22: Input, bindings, devices, and control ownership

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F00, F16.
**Owner paths:** `crates/cs_types/src/input.rs`; `crates/cs_app/src/input/`; `crates/cs_sim/src/control.rs`; `tests/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Input maps keyboard, mouse, gamepad and joystick/HOTAS devices to typed FlightCommand and UI actions. Continuous axes and edge-triggered commands have separate buffering. Player control is owned by exactly one session actor; menus, cinematics and multiplayer authority explicitly gate it.

## Non-negotiable behavior

1. Calibrate axes with deadzone, inversion, response curve, saturation and device identity. Never tie a controller to unstable enumeration index alone.
2. Mouse flight mode is a disclosed designed option; preserve joystick-style control. Keyboard throttle steps and direct settings do not depend on render FPS.
3. Device removal releases held buttons and neutralizes unsafe controls; it cannot leave weapons firing forever.
4. Focus loss pauses single-player where allowed and suppresses input. In multiplayer it neutralizes local input without pausing the server.
5. Bindings persist atomically and conflicts are visible. Text entry and UI navigation cannot also fire weapons or bail out.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A one-frame key edge produces one action across multiple physics substeps.
**AC02:** Unplug a joystick while firing; stop fire and report device loss.
**AC03:** Replay the same quantized command stream at different display FPS.
**AC04:** Open text entry and confirm flight commands are not emitted.

## Bounded implementation slices

### F22-A: Define command schema and action map

Dependencies: F00-A, F16-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f22_a_`. Minimum scenario: A one-frame key edge produces one action across multiple physics substeps.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F22-B: Implement keyboard/mouse/controller adapters and calibration

Dependencies: F22-A, F00-C, F16-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f22_b_`. Minimum scenario: Unplug a joystick while firing; stop fire and report device loss.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F22-C: Wire focus, UI, replay and control ownership

Dependencies: F22-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f22_c_`. Minimum scenario: Replay the same quantized command stream at different display FPS.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F22-D: Test all declared device families and original command coverage

Dependencies: F22-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f22_d_`. Minimum scenario: Open text entry and confirm flight commands are not emitted.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md); [S11](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.
