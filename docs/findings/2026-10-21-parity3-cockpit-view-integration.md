# PARITY3-COCKPIT-VIEW (#1738): integration of the five stages

Parent of `.01`-`.05` (#1809-#1813). Capabilities used: `retail`, `gpu`. Nothing here is
`verified_original`; the original frame is the owner-supplied capture linked in #1738
(viewed from `private/`, never committed).

## What is in

F8 cycles the measured original view list (`.01`), the eye camera sits at the player
airframe's `cockpit_camera` node (`.02`), the `cockpit1` interior is drawn (`.03`), six live
dials, the horizon and the compass strip are driven from state (`.04`), and the owner-command
smoke has a cockpit phase (`.05`).

Integration run on the merged tree, `cs --cs-path <retail> --mission M01 --smoke-seconds 45
--capture-dir private/cockpit-int`: exit 0, 43 checks, no failures, including
`cockpit_view_cycles`, `cockpit_eye_placement`, `cockpit_dials_drawn` (107/107 meshes at modes
7 and 6, 0 in chase) and `cockpit_interior_drawn` (45/45, 0 in chase). Frames:
`private/cockpit-int/frame_03048_cockpit_view.png`, `..._second_eye.png`,
`..._chase_after_cockpit.png`. Side-by-side notes against the original are in
`2026-10-21-parity3-cockpit-view-05-smoke.md`.

## Field of view: not closed (rule 4)

The engine facts (`data/owner-static/engine-facts.toml`, `fov.*`, from CAMERA-MODES-01) give
the angle `0x42b660` returns: 60 degrees (VA `0x0060409c`) for every mode except 6 with
`[camera+0x150] != 0`, which gets 80 degrees (VA `0x006040a0`). `0x42b570` multiplies
`tan(angle/2)` by the run-time screen factor `[0x71b3f8]` before projecting, so whether the
angle is horizontal or vertical is **unknown**.

Ours still uses the chase view's designed 55 degrees vertical. It is not changed here: read as
60 degrees horizontal on a 4:3 frame the vertical angle is about 46 degrees, which makes the
panel *larger* than ours, while the original's panel is smaller (the ROCKETS-to-GUNS dial
spacing is 0.074 of the frame height there versus 0.124 in ours, implying roughly 80 degrees
vertical if our eye-to-panel distance and unit scale are right). The measurements disagree, so
a value picked now would be a guess. Resolving needs the value of `[0x71b3f8]` (its writer),
and the stored-unit scale of the cockpit models.

## Still unknown

* The cockpit field of view (above); the gunsight offset; what distinguishes mode 6 from 7.
* Burl-wood side panels and the dial text, which are cropped in our frame by the FOV gap.
* Whether the original draws the `rcircle` reticle and the text layer in the cockpit.
* Attitude-ball mapping and flag/arrow thresholds.
