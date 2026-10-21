# PARITY3-CAMERA-MODES.04 (#1826): the owner-command smoke photographs every camera mode

Date: 2026-10-21. Parent #1743. Predecessors: camera-modes-01 to -03 findings.
Code: `crates/cs_app/src/mission_session/smoke/camera_modes.rs` (stage table,
sampler, judge), `smoke.rs` (`Phase::Cameras` after the `F1` help phase).
Tests: `crates/cs_app/tests/accept_parity3_camera_modes_04.rs`, prefix
`accept_parity3_camera_modes_04_`. Nothing here is `verified_original`; a
scripted run is never `human_play`.

## What the stage does

After the keymap overlay the scripted run presses, through the real keyboard
path, one stage per 50 frames: `F7`, `F9`, `F10`, `F11`, `F12`, `F8` (back to the
remembered view, chase after the cockpit phase), numpad `-` (zoom out), numpad `+` (zoom
in), `K` + numpad 8, numpad 4, `J` + numpad 6, `L`, and in the cockpit eye `K`
+ numpad 4. At frame 40 of each stage it reads the view, look and zoom state
and the camera's offset from the aircraft (body frame), and takes a
framebuffer `frame_*_cam_<stage>.png`. Checks `camera_<stage>` judge state,
a drawn frame (>= 16 luminance levels, designed) and, for zoom and look, the
effect; `camera_external_views_differ` requires chase, F9-F12 to stand >= 1 m
apart (designed).

## Run

`CS_CAPABILITIES=retail,gpu,audio target/release/cs --cs-path $CS_GAME_DIR
--mission M01 --smoke-seconds 60 --capture-dir private/camera-smoke`: exit 0,
3790 frames, 55 checks, no failures; the 14 frames are in
`private/camera-smoke/` (ignored by Git). Measured camera offsets from the
aircraft: chase 18.6 m, F7 91.2 m (point held where it was, aircraft flew on),
F9 9.6 m above/behind, F10 12.0 m behind, F11/F12 10.1 m left/right, eye 0.8 m;
zoom out 23.3 m; look up pitch 1.36, side yaw 1.51, smooth yaw -0.96.

## Not compared

The owner's original external frames are not in the repository or readable by
an agent, so no side-by-side comparison was made; that is a `human_review`
step. Track target (`L`) had a selected target in the run (pitch -0.37, yaw
0.18), but no frame proves it follows the right actor.
