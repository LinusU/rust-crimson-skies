# F22-B: Device adapters, calibration and the keyboard throttle

Date: 2026-09-30. Task: F22-B "Implement keyboard/mouse/controller adapters and
calibration" (`specs/F22-input-bindings-devices-and-control-ownership.md`,
section `### F22-B`). Shared contract: `docs/contracts/UI-NETWORK.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no evidence
report required, no `gpu`/`audio`/`human_play` needed).

## Files and the one observable failure (listed before editing)

- `crates/cs_types/src/input.rs`: the dependency-free calibration schema —
  `AxisChannel` (the key calibration is stored under), `ResponseCurve`,
  `AxisCalibration` (dead zone, inversion, curve, saturation, analog activation
  threshold), `CalibrationError`, `CalibrationStore` (keyed by device identity
  and channel, with `promote`, `unstable_devices` and `forget_device`),
  `CalibrationStoreError`, `ActionMap::targets_for` /
  `ActionMap::bindings_for_channel`, and `DeviceIdentity::stable_identity` /
  `DeviceId::stable_identity`.
- `crates/cs_app/src/input/devices.rs` (new): the adapters — `DeviceEvent`
  (one device's raw state for one render frame), `DeviceAdapters` (the device
  set, the calibration, the held edges, the per-device driven axes, the losses),
  `DeviceLoss`, `HeldEdge`, `AdapterError`, `normalize_gamepad_axis`,
  `DESIGNED_DEAD_ZONE` and `designed_axis_calibration`.
- `crates/cs_app/src/input/mod.rs`: the collector keeps F22-A's
  `InputBindings`/`InputCollector` and gains the device half —
  `observe_device`, `connect_device`, `disconnect_device`,
  `adopt_device_identity`, `take_device_losses`, and a `take_frame` that closes
  the frame.
- `crates/cs_sim/src/control.rs`: `ThrottleSteps`, `ThrottleChange`,
  `ThrottleError` — the keyboard throttle's tick-driven step rules.
- `crates/cs_app/tests/accept_f22_b_device_adapters.rs` (new): the
  cross-crate acceptance scenario, driving collector → `ControlBuffer` →
  `ThrottleSteps` exactly like a render loop.
- `crates/{cs_types,cs_sim,cs_app}/src/lib.rs` (wiring only): module
  documentation.
- This file.

**One observable failure:** the pilot is firing from a joystick trigger and the
joystick is unplugged. Without a release on removal, the session keeps
queueing `FirePrimary` for as long as it lives, the stick's last deflection
stays in `AxisState` forever, and the player is never told which device went
away. `accept_f22_b_unplug_joystick_while_firing_stops_fire_and_reports_loss`
fails under any of those three.

## What the stage makes structural

1. **Calibration is keyed by device identity, never by an enumeration index
   alone** (non-negotiable behavior 1). `CalibrationStore` is keyed by
   `(DeviceId, AxisChannel)`. A record stored under
   `DeviceIdentity::EnumerationFallback` works for the session but is listed by
   `unstable_devices()` so a persistence writer can refuse it;
   `DeviceAdapters::adopt_identity` re-keys the store, the connected device, the
   held edges and the driven axes when the platform finally reports the real
   identity, so nothing is left behind under the index. `DeviceLoss::
   stable_identity` is `None` for an index-only device, so a caller cannot
   persist its tuning by accident.
   The four stages are applied in a fixed, documented order — inversion, dead
   zone, response curve, saturation — each by a distinct line of
   `AxisCalibration::apply`, and each covered by a value in the tests.
2. **A device report is level-triggered.** A report is the device's whole
   state, so what it omits was released: a held *edge* fires once however many
   frames it stays down, and a held *digital* source bound to a continuous axis
   is at full scale while it is listed and exactly neutral once it is not.
3. **Device removal releases held buttons and neutralizes unsafe controls**
   (non-negotiable behavior 3) and reports it: `disconnect` drops the device's
   holds, forgets the axes it drove and records a `DeviceLoss` naming the
   released edges, the neutralized commands and the stable identity. An edge
   already delivered to a consumer is **not** withdrawn — the input layer never
   rewrites a delivered command; it guarantees no *further* command.
4. **An axis no device drives is exactly neutral.** `finish_frame` writes an
   explicit zero for every continuous axis the previous frame drove and this one
   does not. Without it, `AxisState` — which only moves an axis a frame names —
   would keep the last deflection of a released key or a removed stick forever.
   A frame that observed no device at all therefore also neutralizes: a caller
   that stops polling loses control input instead of leaving it stuck.
5. **Keyboard throttle steps and direct settings do not depend on render FPS**
   (non-negotiable behavior 2). A step is applied by `ThrottleSteps::apply_tick`
   at the simulation's input boundary, once per *executed edge*, never by a
   per-frame rate; a direct setting is applied after that tick's steps, so it
   wins over them whatever order the edges arrived in.
6. **The context gate governs the device path** (non-negotiable behavior 5).
   `DeviceAdapters` does not own the context: the session's `InputBindings`
   context is passed into every `apply`, so a text field or a cinematic silences
   every device and neutralizes the axes they were driving.

## Designed vocabulary, not original data

Every event, axis channel, dead zone, response curve, saturation, activation
threshold, throttle step, combination rule and binding this stage touches is
newly authored project design. The following are **unknown** and are not
guessed here:

- which input devices the original 2000 PC game supports, how it names them and
  which of them it enumerates as a joystick/HOTAS rather than a gamepad;
- which platform identity string is stable on the original platform, and what
  its GUID/device-path form is (`DeviceIdentity::Stable` exists; what a real
  2000-era platform reported is unmeasured);
- the original's stick, trigger and mouse-flying defaults: dead zone, curve,
  saturation, inversion, and whether mouse flight is a *mode* at all;
- whether the original's keyboard throttle is a step, a rate, or a lever with
  discrete positions, and what one press is worth;
- whether the original's throttle axis runs `[-1, 1]` with the idle end at
  `-1` (this stage's convention, forced by F22-A's normalized `[-1, 1]`
  `FlightCommand::Throttle`) or something else;
- what an analog trigger bound to a weapon crosses, and whether the original
  applies any shaping to it (this stage declares the threshold
  `AxisCalibration::activation` and uses 0.5 as a designed default);
- the original's treatment of a device that disappears mid-mission (does it
  pause, prompt, or silently neutralize?).

Affected content: the whole F22 input surface. Resolving tasks already in the
queue: **F22-C** (focus, UI, replay and control-ownership wiring, plus the
persistence writer that must refuse `unstable_devices()`), **F22-D** (all
declared device families and original command coverage). The designed dead zone
`DESIGNED_DEAD_ZONE = 0.08`, the designed throttle step
`ThrottleSteps::DESIGNED_STEP = 0.05` and F22-A's `ActionMap::designed_default`
are project defaults, not measurements.

## Known limits of this stage (carried forward, not hidden)

- **Persistence is not implemented.** `CalibrationStore` lives in memory for the
  session. F22-C owns the atomic binding/calibration writer and must refuse the
  records `unstable_devices()` lists; until then a calibration survives a
  replug but not a reboot.
- **The mouse is a disclosed designed option and is *not* the original's.** A
  mouse axis is a **relative** channel: the calibrated per-frame motion drives
  the axis directly and returns to neutral when the mouse stops. No
  sensitivity setting, deflection smoothing, self-centering or wheel binding is
  modelled here, so the mouse-flight feel is a project choice, not a claim.
  Whether the original offers mouse flight at all, and what it does with a
  relative axis, is F22-D's measurement.
- **No platform adapter.** `DeviceEvent` is the seam; a Bevy system that reads
  `ButtonInput`, `Gamepad` and the mouse accumulation resources and emits these
  events is F22-C's wiring, deliberately not this stage.
- **Held-button consumers are F22-C's.** `HeldEdge`/`held_edges` and
  `DeviceLoss` are how a weapon system learns that a trigger is still down or
  was lost; no weapon, cooldown or fire-rate state is touched here.
- **Focus loss and multiplayer neutralization** (non-negotiable behavior 4) are
  F22-C: the context switch they perform already silences the device path, and
  the throttle steps with it.
- **`ThrottleSteps` is a position model, not a flight model.** It is the
  keyboard's own `[0, 1]` throttle; an analog throttle (a gamepad trigger) is
  set through `set_position`, and mapping the signed axis into the flight
  model's `[0, 1]` throttle is F22-C/F24 wiring.
- **Analog-as-edge needs a declared threshold.** A trigger bound to an edge
  (the natural HOTAS "fire" binding) fires while its calibrated magnitude is at
  or above `AxisCalibration::activation` and releases below it, so it can both
  fire and stop. The threshold is a designed default; the original's is unknown.

## Tests

Task-test prefix `accept_f22_b_`. The selection
`cargo test --workspace --locked -- accept_f22_b_ --include-ignored` discovers
and runs **32** tests, all passing:

- `cs_types::input` (4): the four calibration stages in order and their
  composition; malformed fields and readings refused by name; calibration keyed
  by device identity, surviving a change of enumeration index, with
  `unstable_devices` and every refused `promote`; the axis-channel vocabulary
  and the map walk by channel.
- `cs_app::input::devices` (16): AC02 at the adapter level; an index-only device
  reporting no stable identity; identity adoption keeping calibration, holds and
  axes, and every refused adoption named; level-triggered edges and axes; the
  context gate over all four contexts; every device family reaching the frame;
  a source's wiring inversion composing with the player's calibration
  inversion; an analog source bound to an edge; a resting trigger not firing an
  edge binding; the strongest deflection winning whatever the event order; every
  malformed event refused by name; a refused report leaving no partial state and
  not swallowing the press it named; connect and remove driving the device set.
- `cs_app::input` (3): the collector routing device events through the context,
  a refused event keeping the input other devices delivered, and a held axis
  staying driven until it is explicitly neutralized.
- `cs_sim::control` (2): the throttle's tick-driven steps across 12/30/60-FPS
  frame groupings, and its malformed steps and positions refused by name.
- `crates/cs_app/tests/accept_f22_b_device_adapters.rs` (8): AC02 end to end
  (fire, unplug, no further fire, axes neutral, loss reported once, replug
  works); refused events changing nothing while firing; a refused report neither
  swallowing a press nor dropping another device's frame; calibrated axes
  reaching `AxisState` by device identity; the identical throttle at 12/30/60
  render FPS; text entry silencing every device; every device family reaching
  `ControlBuffer`; the designed calibration and channel vocabulary.

No test needs the original installation, so none is `#[ignore]`d.

## Review (2026-09-30, same branch, reviewer fixed the branch)

Reviewed by `bunny-alpha-1`, the agent that also implemented this stage. This
is therefore **not** independent review and is not original-reference evidence;
F22-D remains the stage that measures anything. Two real defects were found and
fixed on the branch, each with a regression test that was run against the
pre-fix behavior and observed to fail:

1. **A refused device report was applied half way.** `DeviceAdapters::apply`
   applied a report's buttons before calibrating its axes and returned the
   refusal afterwards, so a report that named the fire button and one bad
   reading established the hold and pushed the edge, and the caller then got an
   error. The `InputCollector` made it worse: it "undid" the event by replacing
   the whole frame with an empty one, so the edge was dropped while the hold
   stayed — the pilot's press was silently swallowed and the gun stayed silent
   until the trigger was released and pressed again — and every *other* device's
   input collected in the same render frame was discarded with it. The report
   counter and the device's driven axes were also mutated by the refused report,
   so a later `DeviceLoss` under-reported what the device was driving.
   Fixed by calibrating the whole report in a pass of its own
   (`calibrated_readings`) before any of it is applied, and by no longer
   replacing the frame in `InputCollector::observe_device`. Tests:
   `accept_f22_b_a_refused_report_leaves_no_partial_state`,
   `accept_f22_b_a_refused_report_does_not_swallow_the_press_it_named`,
   `accept_f22_b_a_refused_event_keeps_the_input_other_devices_delivered`,
   `accept_f22_b_a_refused_report_neither_swallows_a_press_nor_drops_a_devices_frame`
   (all four failed on the pre-fix code).
2. **A gamepad trigger bound to an edge fired from rest.** The activation
   threshold was compared against the calibrated **magnitude**, but
   `normalize_gamepad_axis` maps a trigger's `[0, 1]` into `[-1, 1]`, so an
   untouched trigger reads `-1.0` and `|-1.0|` clears any threshold. A trigger
   bound to a weapon — the natural replacement for a face button — therefore
   held the guns on from the moment the device was touched, with no press, and
   nothing in the stage's tests bound a trigger to an edge. The decision is now
   made once per channel in `calibrate` and measured from the channel's resting
   end: the *pull* of a one-directional channel, the *deflection* of a
   two-directional one, which is what `AxisChannel::is_unipolar` declares. Test:
   `accept_f22_b_a_resting_trigger_does_not_fire_an_edge_binding` (fails on the
   pre-fix magnitude test, with the gun firing on an untouched trigger).

One further limit is now disclosed rather than fixed, because the answer is a
design decision F22-C owns: **a device that stops reporting without a removal
event keeps its hold.** `release_unreported` can only release what a later
report from the same device fails to re-establish, and continuous axes *are*
neutralized by `finish_frame` when nothing drives them, but a held edge waits
for the device's next report or for its removal. A consumer that polls
`held_edges()` for a sustained trigger must therefore also own a policy for a
device that has gone quiet; the sheet's non-negotiable 3 (a *removed* device
cannot leave weapons firing) is covered and tested.

## Checks

Run locally before hand-over (all exit 0):

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f22_b_ --include-ignored
```

## Mutation probe (test sensitivity)

Each mutation below was applied, the `accept_f22_b_` selection was run, and the
mutation was reverted. Every one is caught.

| Mutation | Caught by |
| --- | --- |
| `disconnect` keeps the device's held edges | `accept_f22_b_joystick_removal_releases_held_fire_and_reports_loss` |
| `finish_frame` neutralizes nothing | `..._joystick_removal_...`, `..._reports_are_level_triggered_...`, `..._collector_keeps_a_held_axis_...` (3) |
| `AxisCalibration::apply` ignores the dead zone | `accept_f22_b_axis_calibration_applies_deadzone_inversion_curve_and_saturation` |
| `AxisCalibration::apply` ignores the saturation | `accept_f22_b_axis_calibration_applies_deadzone_inversion_curve_and_saturation` |
| `adopt_identity` moves the store but not the connection | `accept_f22_b_calibration_survives_identity_adoption_and_replug` |
| the last event wins an axis instead of the strongest | `accept_f22_b_strongest_deflection_wins_when_two_devices_drive_one_axis` |
| a held trigger is not re-established (fires every frame) | `..._analog_source_bound_to_an_edge_...`, `..._reports_are_level_triggered_...` (2) |
| the adapters ignore the input context | `..._device_path_honors_the_input_context`, `..._collector_routes_device_events_through_the_context` (2) |
| `ThrottleSteps::apply_tick` ignores a step edge | `accept_f22_b_keyboard_throttle_steps_ignore_render_frame_grouping` |
| a direct throttle setting loses to its own tick's step | `accept_f22_b_keyboard_throttle_steps_ignore_render_frame_grouping` |
| `release_unreported` never releases | `..._analog_source_bound_to_an_edge_...`, `..._reports_are_level_triggered_...` (2) |
| the analog activation threshold is measured from the middle of the signed axis (the pre-review behavior) | `accept_f22_b_a_resting_trigger_does_not_fire_an_edge_binding` |
| a report is validated lazily instead of before it is applied (the pre-review behavior) | `accept_f22_b_a_refused_report_does_not_swallow_the_press_it_named`, `accept_f22_b_a_refused_report_leaves_no_partial_state` (2) |
| `observe_device` replaces the frame when an event is refused (the pre-review behavior) | `accept_f22_b_a_refused_event_keeps_the_input_other_devices_delivered`, `accept_f22_b_a_refused_report_neither_swallows_a_press_nor_drops_a_devices_frame` (2) |

The three rows marked *pre-review* were the review's own mutations: each was
applied to the reviewed branch, the new test was run and observed to fail, and
the mutation was reverted. They are listed with the implementer's table because
they are the same kind of evidence.

The dead-zone mutation was **not** caught by the first version of the
calibration test: with the dead zone applied only through the rescale, a
reading below the dead zone produced a small *reversed* deflection instead of
neutral, and no test looked at a reading strictly inside the dead zone. The
test now asserts `apply(0.1)` and `apply(-0.1)` are exactly `0.0` for a 0.25
dead zone, and the mutation fails. This is recorded rather than quietly fixed
because it is the kind of hole a reviewer has to look for.
