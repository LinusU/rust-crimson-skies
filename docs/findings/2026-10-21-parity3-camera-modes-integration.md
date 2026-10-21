# PARITY3-CAMERA-MODES (#1743): integration run of the owner-command smoke

Date: 2026-10-21. Children #1823-#1826 are merged; see their findings
(`2026-10-21-parity3-camera-modes-01-measured.md` to `-04-smoke.md`). Nothing
here is `verified_original`; a scripted run is not `human_play`.

## Run on main (e5f8013f + children)

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 60 --capture-dir private/camera-smoke`
(`CS_CAPABILITIES=retail,gpu,audio`): exit 0, 3789 frames, 58 checks, failures: none.
The 14 camera frames (`frame_*_cam_*.png`, ignored by Git) cover chase, F7 held
point, F9 down, F10 back, F11 left, F12 right, F8 remembered view, zoom out/in,
K snap look up and side, J smooth look, L track target and the cockpit eye with
a snap look. F9 was viewed: the aircraft is seen from above, filling the frame.

## Still open

* Per-airframe external distances come from `camparam.zrd`, which is not loaded
  yet; the placements use the capture-fitted chase distance (designed).
* No side-by-side with the owner's original external frames (`human_review`).
* `L` had a target, but no frame proves it follows the right actor.
