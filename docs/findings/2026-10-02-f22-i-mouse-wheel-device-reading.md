# F22-I: the mouse wheel as a reportable device reading

Date: 2026-10-02. Task: F22-I "Carry the mouse wheel through DeviceEvent and its
calibration" (resolving task named in
`docs/findings/2026-09-30-f22-d-device-families-and-command-coverage.md`, §
"Unknown and not guessed", last bullet). Feature sheet:
`specs/F22-input-bindings-devices-and-control-ownership.md` (stage F22-I is a
Rally-split follow-up to `### F22-D`, not a sheet section of its own). Shared
contract: `docs/contracts/UI-NETWORK.md`.
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no evidence
report required, no `gpu`/`audio`/`human_play`/`network_real` needed.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/input/devices.rs`: `DeviceEvent::MouseFrame` gains the
  wheel reading; `DeviceAdapters::calibrated_readings` reads every declared
  mouse axis. Nothing else in the producer half changes: the calibrated
  readings pass already decides an edge target's activation, the scale, the
  clamp and the report's atomicity, so the wheel needed a producer, not a
  second pipeline.
- `crates/cs_app/tests/accept_f22_wheel_mouse_wheel_producer.rs` (new): the
  adapter-level and session-level acceptance scenarios.
- `crates/cs_app/tests/accept_f22_b_device_adapters.rs`,
  `crates/cs_app/tests/accept_f22_d_device_families_and_command_coverage.rs`:
  their `MouseFrame` fixtures gain the field, and F22-D's fixture stops
  asserting that a wheel report is impossible.
- This file.

**One observable failure:** a platform that reports a mouse wheel delta has
nowhere to put it. `MouseAxis::Wheel` and `AxisChannel::Mouse(MouseAxis::Wheel)`
are declared, calibratable and persisted, but `DeviceEvent::MouseFrame` carried
only `motion_x`/`motion_y`, so `DeviceAdapters::calibrated_readings` could never
calibrate, shape or refuse a wheel reading: a driver reporting a nonsense wheel
was invisible, and a channel a settings screen could tune was one no device
report could ever reach. `accept_f22_wheel_a_scrolled_wheel_is_calibrated_like_any_relative_channel`
and `accept_f22_wheel_a_refused_wheel_reading_refuses_the_whole_report` both
fail while the wheel is not in the calibration pass.

## What changed, and what stays the same

- `DeviceEvent::MouseFrame` carries `wheel: f32`, the wheel's scroll delta since
  the last frame **in the same device-count units and the same sign convention
  as `motion_x`/`motion_y`**. One count is one detent the platform reported;
  the sign is the sign the platform reported. `0.0` means the wheel did not move,
  which is what makes the existing "no movement is no drive" rule apply to it
  unchanged: a stopped mouse, stopped wheel included, contributes no reading at
  all rather than a deflection of zero.
- The wheel is read by the same `calibrate` call as the motion axes, on the
  same `AxisChannel::Mouse(MouseAxis::Wheel)` key the settings and persistence
  paths already use. It therefore gets the same dead zone, inversion, response
  curve, saturation and analog activation threshold, and the same atomicity: a
  wheel reading the calibration refuses refuses **the whole report** by device,
  channel and reason (`AdapterError::ReadingRejected`) before the buttons the
  same report named are applied, so the pilot's press is neither swallowed nor
  half-applied and the report counter, the held edges and the driven-axis record
  are all untouched.
- **The wheel is deliberately still unbound.** `ActionMap::designed_default`
  binds no wheel channel, and F22-B's
  `accept_f22_b_designed_calibration_covers_every_bound_analog_channel` pins
  that (`map.bindings_for_channel(wheel).next().is_none()`). F22-I did not touch
  it: which command a mouse wheel drives, if any, is a *design* decision, and
  the task explicitly forbids weakening that passing test. What F22-I removes
  is the *producer/adapter gap* F22-D recorded, not a missing binding. An
  unbound wheel therefore still calibrates and is still refused — a driver
  reporting a nonsense wheel is a real fault whether or not a binding reads the
  channel — and it drives no command and adds no axis to the frame.

## Can any real platform adapter in this repo produce the reading yet?

**No.** Verified, not assumed:

- `grep -rn "DeviceEvent" --include="*.rs" crates/` finds the type only in
  `crates/cs_app/src/input/{devices,mod,session}.rs` and in
  `crates/cs_app/tests/`; there is no producer outside the test fixtures.
- `grep -rln "bevy::input" --include="*.rs" crates/` matches nothing:
  `cs_app` depends on `bevy` (workspace) but no code reads `ButtonInput`,
  `MouseMotion`, `MouseWheel` or `AccumulatedMouseMotion` yet.
- Consequently no runtime path in this repository emits a non-zero `wheel`
  today. The channel is now *reportable and calibratable*; it is not yet
  *reported by a real windowing layer*. Writing the Bevy system that reads the
  wheel resource and emits the event is **F22-F**'s work and was not attempted
  here (and is outside F22-I's owner paths).

This is a producer/adapter gap **in one declared device family**, closed at the
adapter boundary. It is not evidence that any platform, original or current,
reports a wheel in these units — see the unknowns below.

## Unknown and not guessed

The following remain **unknown**; nothing in this stage measures or assumes
them, and no test claims anything about them:

- **Whether the original 2000 PC game binds a mouse wheel to anything at all,
  and to what.** A 2000-era PC flight game steering with a relative mouse would
  not obviously want a wheel axis on the stick, but that is a guess about a
  guess. The designed default here binds the wheel to nothing, which is a
  project choice; F22-H (#411) measures the original's own control vocabulary
  and bindings from the installation.
- **The original's wheel sensitivity, detent size and direction convention.**
  `DeviceEvent::MouseFrame::wheel`'s units (device counts, sign as the platform
  reports) are this project's *seam* convention, chosen to match `motion_x` and
  `motion_y`; they are not a measurement of the original or of any driver.
- **Which current platforms report a wheel delta at all**, and whether any of
  them reports it in detents, lines or pixels. F22-F will have to state the
  unit it normalizes to; this stage only requires that a reported reading be
  finite and inside `[-1, 1]` or be refused by name.
- **Whether a wheel should ever drive an edge target.** The generic
  `AxisCalibration::activation` threshold makes that possible (a wheel bound to
  a command fires while the calibrated reading is at or above the threshold and
  releases below it), but no binding does it today, so the rule is untested in
  the wheel's case and unclaimed.
- **Mouse flight itself remains a disclosed designed option, not the original's
  model** (carried from F22-B).

Affected content: the F22 input surface's mouse family. Resolving tasks:
**#411 `F22-H`** (the original's control vocabulary and bindings) and
**`F22-F`** (the Bevy platform adapter that would actually report a wheel).

## Tests

Task-test prefix `accept_f22_wheel_`. The selection
`cargo test --workspace --locked -- accept_f22_wheel_ --include-ignored`
discovers and runs **5** tests, all passing (exit 0), all in
`crates/cs_app/tests/accept_f22_wheel_mouse_wheel_producer.rs`, all driving
production code (`DeviceAdapters::apply`/`calibration_mut`, and one
`InputSession::pump_frame` reading the `cs_sim::control::ControlBuffer`):

- `accept_f22_wheel_a_scrolled_wheel_is_calibrated_like_any_relative_channel`:
  a scrolled wheel with a still mouse reaches the bound axis, shaped to exactly
  the value the same `AxisCalibration` produces for that raw reading — so a
  wheel calibrated by any other rule (no dead zone, a different curve, a
  different saturation) fails; the reading does not leak into the motion
  channels; a scroll inside the dead zone is neutral; a wheel that did not move
  adds no axis and is recorded as driving nothing.
- `accept_f22_wheel_a_refused_wheel_reading_refuses_the_whole_report`: an
  out-of-range (`ReadingOutOfRange { value: 1.5 }`) and a `NaN` wheel reading
  are refused by device, channel and reason, contribute nothing, establish no
  hold from the left button they named, and are not counted as reports; the
  next real report of that same button is still a first press, so the gun fires
  exactly once.
- `accept_f22_wheel_a_refused_wheel_reading_leaves_no_partial_state`: a mouse
  that was already holding a button and driving the wheel axis keeps both
  across an infinite-wheel refusal — the report counter, the hold and the
  driven-axis record all survive — and the later `DeviceLoss` still names the
  axis and releases the edge.
- `accept_f22_wheel_an_unbound_wheel_drives_nothing_and_is_still_validated`:
  under `ActionMap::designed_default` (asserted to bind the wheel to nothing,
  restating F22-B's rule here rather than replacing it) a scrolled wheel fires
  nothing, adds no axis and is recorded as driving nothing, while a `NaN` wheel
  is still refused by channel.
- `accept_f22_wheel_a_wheel_report_reaches_the_control_buffer`: the production
  `InputSession` loop — a scrolled-wheel report reaches
  `session.controls().axis(Roll)` at the calibrated value, delivers no edge, and
  a wheel that stops returns the axis to exactly `0.0`.

**Sensitivity, measured rather than asserted:** deleting the single
`(MouseAxis::Wheel, *wheel)` entry from
`DeviceAdapters::calibrated_readings` makes all 5 tests fail (0 passed, 5
failed); removing the `wheel` field from `DeviceEvent::MouseFrame` breaks the
compilation of every `MouseFrame` fixture, including this file. No test in this
file passes with the production change reverted.

No existing test was weakened: F22-B's unbound-wheel assertion is untouched and
still passes, and the only edit to the F22-D fixture replaces its
`panic!("the mouse wheel has no field in DeviceEvent::MouseFrame")` arm with a
real reading, because after this change that statement is no longer true.

## Checks

Run on this branch (all exit 0):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f22_wheel_ --include-ignored
```

No protected path was touched, no original data was read, and no original-data
or fidelity claim is made: this stage's evidence is the declared channel
reaching the calibration pass, which a synthetic fixture can and does establish.
It is not evidence about the original game, and it does not lift any limitation
on the F22 release claims.
