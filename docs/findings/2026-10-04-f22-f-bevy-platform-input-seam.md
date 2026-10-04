# F22-F: Bevy platform input seam

Task #405. Code: `crates/cs_app/src/input/platform.rs` (`BevyInputPlugin`, `PlatformInput`,
`PlatformFrameReport`). Tests: `accept_f22_f_*` in the same file (headless `App`, no window, no `gpu`).

## What it does

Once per frame in `PreUpdate`, after `InputSystems`, it emits `Connected`/`Removed`, one
`KeyboardFrame`/`MouseFrame`/`GamepadFrame` per device, forwards `WindowFocused` to
`InputSession::set_focus`, converts `Time<Real>` into fixed ticks with a real `SimClock` (frozen while the
session is paused, like `CommandReplay::frame`) and calls `pump_frame`. UI requests, losses, faults and the
delivered per-tick edges are drained into `PlatformFrameReport`. No policy lives in the system.

## Device identity (measured from the Bevy 0.19.1 source)

Bevy exposes no stable per-device identity: keyboard and mouse are one aggregate each, and a `Gamepad`
carries only optional vendor/product ids that two identical pads share. Every device is therefore named
with `DeviceId::enumeration_fallback` (keyboard 0, mouse 0, gamepads by connection order, `is_stable() ==
false`). `PlatformFrameReport::devices` lists each device named, its platform label and `stable: false`.
Calibration is consequently not persisted against a platform device. Whether a stable string exists on a
real OS (for example from the gamepad name or backend) is **unmeasured**.

## Scales (designed, unmeasured)

* Mouse motion: pixels per frame / `MOUSE_FULL_DEFLECTION_PIXELS` (64), clamped to `[-1, 1]`.
* Wheel: delta.y / 4 lines or / 64 pixels, clamped. Reported, still unbound.
* A non-finite reading is reported as `0.0`.
* Gamepad sticks pass through; triggers use the analog value of Bevy's `LeftTrigger2`/`RightTrigger2`.

## Known limits

* Joysticks/HOTAS are not produced (Bevy has no joystick class).
* Keys outside `cs_types::input::Key` and `Other(_)` buttons are dropped.
* Gamepad connection is detected by entity presence, not by `GamepadConnectionEvent`, so the OS name is not
  recorded.
* Original-control fidelity and device census stay with their own tasks; windowed UX with #647.
