# SIDEWINDER-FFB2: the SideWinder Force Feedback 2 on Apple Silicon macOS

Task #1968. Code: `crates/cs_app/src/input/hid/` (descriptor, layout, effect, pid, device,
probe, the `HostJoystickPlugin` seam), `crates/cs_app/src/force_feedback.rs` (`FlightFeel`),
`crates/cs_app/src/mission_session/flight_feel.rs` (mission cues and gate),
`cs_types::keymap::original_joystick_axis_bindings`, `InputSession::follow_throttle_lever`.
Tests: `accept_sidewinder_ffb2_*` (`crates/cs_app/tests/accept_sidewinder_ffb2.rs`, suite b)
and the unit tests of the `hid` modules. User guide: `docs/PLAYTEST.md`, "Joystick and force
feedback".

## Route

Bevy's gamepad input (gilrs) offers the stick as a gamepad with no twist or lever axis of its
own and no force feedback, and macOS has no force-feedback API for a USB PID device. The game
therefore reads and drives the stick itself through hidapi (shared, not exclusive): every
control is found by HID usage in the stick's report descriptor, and force feedback is played
by writing the device's own USB PID output and feature reports (Set Effect, Set Condition,
Set Periodic, Set Constant Force, Set Envelope, Effect Operation, Block Free, Device Control,
Device Gain; effect blocks from Create New Effect / PID Block Load). The driver is a port of
the owner's drydock86 `drydock-joystick` crate, which drove the same stick. SDL3 haptics was
not needed.

## Hardware evidence (owner's Mac and stick, 2026-10-23)

Machine: Apple Silicon, macOS 26 (Darwin 25.6.0). Device: `SideWinder Force Feedback 2
Joystick`, USB `045e:001b`, no serial number. Agent: Claude (Opus 5.5) in the owner's
session; the owner moved the stick and held the grip.

* **Descriptor.** 1343 bytes, byte-identical to the bundled capture
  (`crates/cs_app/src/input/hid/testdata/sidewinder_ff2.hex`). Input reports 1 (controls) and
  2 (PID state); output reports 1–8 and 10–14; feature reports 1–3.
* **Controls** (`cs --joystick-probe`, owner moving the stick through a set sequence):
  X `-512..511` (right positive), Y `-512..511` (pulled back positive), twist Rz `-32..31`
  (clockwise positive), throttle Slider `0..127` (lever forward `0`, back `127`); all ends
  reached; hat 0–7 clockwise from up in order, 8 (null) centred; buttons 1–8 on bits 0–7 in
  order. At rest the stick read `X 0, Y -22` (−0.043) on one run and `X -27, Y -14` on
  another, which the designed 8% dead zone absorbs. The stick reports only changes, and
  stalls a request for its current input report, so an axis is unknown until the stick moves.
* **Force feedback** (`cs --joystick-probe --test-force-feedback`, 25%, three runs): PID pool
  `ram_pool_size 1350`, `simultaneous_effects_max 15`. Every step (spring, damper, constant
  force right, left, forward, back, sine, square) was accepted, and the stick's own PID state
  reports said an effect was playing for each of the eight. The owner held the grip and
  reported: "Seems good! It's a little weak", which is expected at 25% (the probe's pushes
  are 60% of full scale, so about 15% of the stick's force).

* **In the mission** (`cs --cs-path … --mission M01`, release build, the owner flying): two
  windowed runs, about 6 minutes at the default 25% and about 2 minutes at
  `--force-feedback-gain 75`. Both logged `joystick: SideWinder Force Feedback 2 Joystick
  (045e:001b) connected as usb:045e:001b: 4 axes, 8 buttons, hat, force feedback at …%`, logged
  no joystick or force-feedback error and quit with exit 0. The owner's verdict after the 75%
  run: "It feels good! Ship it!" This is `human_play` evidence for the owner's stick and Mac;
  the 25% default stays (the owner preferred 75% for themselves, which is the flag's job).

## Decisions

* **Axis assignment: original.** `ORIGINAL_JOYSTICK_AXES` (static, `OWNER-STATIC`) maps X to
  roll, Y to pitch, Z/slider to throttle (`0.5 - 0.5 * normalised`) and Rz to yaw; the hat
  looks. The live map now uses it (`original_joystick_axis_bindings`) instead of the designed
  default's joystick axes, whose pitch scale (−1) would have flown a real stick inverted.
  Signs are this project's (HID right/back/clockwise positive; roll right, nose up and nose
  right positive), so the original's own `inverted` flags, which describe its internal roll and
  yaw signs, are not copied. The lever's direction (forward is full) is the original's and
  matches the stick.
* **Buttons: original.** The joystick column of the original control table: 1 guns, 2 rockets,
  3 next enemy, 4 cockpit views, 5 spyglass, 6 next ground target, 7 and 8 commands with no
  consumer yet.
* **Designed:** the 8% dead zone of X, Y and twist, no dead zone on the lever, the lever's
  0.02 hysteresis and "last control moved wins" between lever and throttle keys, the hat as
  four virtual buttons (128–131), the stable identity `usb:045e:001b` (only while no second
  stick of the model is connected; with a serial, `usb:vid:pid:serial=…`), and every force
  effect: spring (1500–6000, stiffening to 100 m/s, ramping in over 1.5 s), damper (1200),
  buffet (sine, 80 ms, 1500–5000 with the law's stall margin), guns (square, 60 ms, 2500),
  hit (constant 3000–6500, 150 ms, alternating sides), impact (constant 4000–8000, 250 ms).
  Default strength 25%, at most 100%, refused above.
* **Stall cue.** `OriginalStep::stall_margin` exposes the stall margin the recovered law
  already computed for its stall rotation, unchanged.

## Still open

* **Felt in flight, item by item.** The owner flew the owner checklist's flights but reported
  one overall judgement, not each item; the pause, focus, unplug and exit stops in particular
  were not reported separately. The tests cover them; a regression there would be a new task.
* **Original effects.** The original's own Immersion effects (`crimsonff.ifr` through IFC) and
  the events that trigger them are unmeasured; nothing here reproduces them.
* **Persistence.** Calibration and bindings are not saved (#404 F22-E owns persistent
  profiles); a joystick button can be rebound only through the in-memory `ControlsMap`.
* **Start flow.** `cs --cs-path <dir>` hands a mission over with the joystick defaults; the
  joystick flags are flags of `--mission`.
* **Process kill.** A process killed outright cannot reset the stick; replugging it or the next
  start does. Normal exit, `Esc`, focus loss, unplug and a panic reset it.
* **Other platforms.** Windows and Linux build the same backend but were not tried.
