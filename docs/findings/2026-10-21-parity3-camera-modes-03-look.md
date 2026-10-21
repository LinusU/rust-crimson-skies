# PARITY3-CAMERA-MODES.03 (#1825): snap/smooth look, track target, numpad look

Date: 2026-10-21. Parent #1743. Predecessors: `...-camera-modes-01-measured.md`,
`...-camera-modes-02-view-state.md`.
Code: `crates/cs_app/src/mission_session/look_view.rs` (state, systems),
`original_keys.rs` (minting of commands 55-66), `view_cycle.rs` (`select_view`
clears the look). Tests: `crates/cs_app/tests/accept_parity3_camera_modes_03.rs`,
prefix `accept_parity3_camera_modes_03_`. Nothing here is `verified_original`.

## Measured and used (static, from #1823)

* `K`/`J`/`L` (55/57/56) set snap / smooth (state zeroed) / track (toggle with snap).
* Numpad 1-9 (58-66) are polled while held; table `look_direction`.
* Snap: yaw target `atan2(x, y)`, pitch target pi/2 up, pi/4 diagonals, else 0;
  no key, both 0. Smooth: targets move 2 rad/s, pitch clamped to pi/2, yaw wrapped.
* Smoothing `v = t + (v - t) * exp(-rate*dt)`, pitch 3.0, yaw 5.0.
* Track: body-frame vector to the target, pitch `atan2(y, hypot(x, z))`, yaw
  `atan2(-x, -z)`; no target, both 0. The target is the targeting session's
  published reticle, read-only: look never selects, drops or fires.
* Entering the chase view or a pilot-eye view clears the look.

## Designed (labelled in code)

* Several held directions add their vectors; the original reads each key.
* Numpad 5 held in track mode recentres (the finding records only that the
  track branch reads it).
* Application: the chase view orbits the camera about the aircraft by the look
  rotation (yaw about up, then pitch about right); the pilot-eye views turn the
  camera in place. External views 1-4 and 9 ignore the look (the look reader is
  only measured in the chase routine and the eye view).
* Yaw smoothing is plain (no shortest-arc), as the traced smoother is.

## Unknown / not done

* Joystick hat look (`0x536c70`): the input layer has no hat path; not read.
* The track branch's second path (camera target vector `0x71c464..6c`).
* The eye view's fixed -4.7 degree tilt is not applied (cockpit follow-up).
* Whether mode 2 (back) reads the look.
* No owner-command smoke was run; this slice claims no M01 visual acceptance
  (frames belong to #1826).
