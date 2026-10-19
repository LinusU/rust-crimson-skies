# PARITY-M01-LC-CAMERA (#1665): the launch cinematic plays the measured camera script

Date: 2026-10-19. Implementer: `claude-2`. Slice (iv) of #1642. Built on the
pose source (#1662), frame and rotation (#1673), timeline (#1674), phase (#1663)
and letterbox (#1664) findings. Nothing here is `verified_original`. Test prefix
`accept_parity_m01_lc_camera_`.

## What it does

`cs_app::cinematics::launch_camera` reads `ZBD/C3/cam_anim.zbd`, replays
`generic_intro` for the player's scene root, and for each scene that moves
`camera1` picks one track (the variant chain's draw, or the scene's only camera
track) and keeps the scene's start and stop times. `pose_at(t)` gives the
position and quaternion in the Pandora's frame; a segment without a position or
rotation part keeps the previous one (the engine's setters leave the node alone).

`mission_session::launch_camera` drives the window's camera in `PostUpdate` while
`LaunchCinematic` plays: time is `elapsed_ticks / BASELINE_FIXED_HZ`; the pose is
put into the world through the live `GlobalTransform` of the `piratezep` world-actor
entity (scale dropped). The chase rig keeps resolving underneath, so control returns
without a swing. `compose` installs it with the phase; a capture scenario and the
no-teleport smoke still start flying directly.

The scripted owner-command smoke sits through the cinematic (#1702's phase). This
task adds screenshots `scene_<s>s` at 1, 8, 15, 21, 27, 33 and 39 s of it. A new check
`launch_cinematic_camera_played` needs frames driven, every scene seen, the
Pandora entity found and a screenshot taken.

## Measured here

* **Forward axis.** With the stored quaternion read as a column-vector rotation,
  the camera looks along local **-Z** (Bevy's convention): at the first variants of
  scenes 1 and 2 the -Z axis points at the Pandora's origin, dot product > 0.95
  everywhere and > 0.99 for most of each scene
  (`..._the_rotation_looks_along_minus_z_at_the_pandora`). No handedness flip is
  needed, which closes `handedness` and `camera-view-composition` for this camera.
* The camera path starts at `gi_cam1`'s stored first position (test).

## Smoke evidence (2026-10-19, release build, this branch)

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-LC-CAMERA`: 20 checks (the run also passes the Flight Check and #1702's phase check), 0 failed,
2882 frames, the camera drove 2441 frames, 4 of 4 scenes. Screenshots (private, not committed):
`frame_00062_scene_01s.png` (the Pandora from behind with the letterbox bars,
as the owner's Drydock frame `d1`) and `frame_01262_scene_21s.png` (alongside the
hull and rigs, scene 2).

## Not covered, and why

* **Field of view** is unmeasured (no track carries one); the camera keeps the
  window's projection. Compared with `d1` the Pandora looks similar in size, but
  that is a look, not a measurement.
* **The player's plane and the pirate fighters do not move**: the `gi_player1/2`
  and `gi_pfighter1/2` tracks are not played, so the red plane does not drop away
  past the Pandora, and the actors keep their composed poses. The cockpit HUD root
  is hidden, but the contact list, radar ring and objective text are still drawn
  during the cinematic (visible in the screenshots); the original's HUD during the
  cinematic is unmeasured.
* **Variant choice** is a seeded draw (clock seed, printed on the
  `mission launch camera:` line); the original's `rand()` stream is not
  reproducible (timeline finding).
* The Pandora's motion during the intro is taken from the live entity
  (`parent-motion`, #1679: nothing measured moves it).
* The letterbox bars switch on and off instantly (#1664).
