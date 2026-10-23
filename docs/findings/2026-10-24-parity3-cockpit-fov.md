# PARITY3-COCKPIT-FOV (#1909): the cockpit field of view

Follow-up of `2026-10-21-parity3-cockpit-view-integration.md`. Capabilities: `retail`
(the decrypted engine image). Provenance is `static` throughout; nothing here is
`verified_original`.

## Resolved: `[0x71b3f8]` and the angle's axis

`[0x71b3f8]` has exactly one writer, the code at VA `0x004603f0` (`fld [0x6032dc]; fdiv
[0x607bd0]; fstp [0x71b3f8]`), reached through the `jmp` thunk `0x4603e0` whose address sits
at `0x006190ec` in `.data` (read as a start-up initializer entry; the table was not traced).
The dividend is `1.0f` and the divisor `1.3333334f` (4/3), so the factor is the constant
**0.75**, independent of the window or resolution.

`0x42b570` computes `vertical = 2 * atan(0.75 * tan(angle / 2))` and hands `(angle,
vertical)` to `0x4d2c20`, which stores them at `[obj+0xe0]` and `[obj+0xe4]`. So the 60
degrees (and the mode 6 second-eye 80 degrees) are the **horizontal** angle on a fixed 4:3
frame, and the cockpit vertical angle is **46.83 degrees**. Rows `fov.screen_factor_*` and
`fov.vertical_derivation` in `data/owner-static/engine-facts.toml` cite the bytes.

## Changed

`drive_cockpit_camera` sets the camera's vertical FOV to 46.83 degrees while a pilot-eye
view is on and restores the chase value on leaving (tests `accept_parity3_cockpit_fov_*`).
The mode 6 second-eye 80 degrees horizontal (about 66.0 degrees vertical) is recorded but
not applied: the second-eye view is not wired.

## Still unknown (rule 4)

* The **stored-unit scale** of the cockpit models and the eye-to-panel distance. The
  integration finding measured the original's ROCKETS-to-GUNS dial spacing at 0.074 of the
  frame height against 0.124 in ours; a 46.83 degree vertical angle cannot by itself account
  for that, since it is close to the angle ours already used. The remaining gap therefore
  belongs to the unit scale (or the interior model's own scale), which no static trace here
  has recovered. It is not guessed.
* The side-by-side smoke frames against the original were not re-captured in this session;
  that comparison needs `gpu` plus the owner's reference frame and stays with the
  integration task.

## Owner-command smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir private/cockpit-fov`
on this change: 4082 frames, 72 checks, failures none (the cockpit-eye check passes). Frames
(in `private/`, not committed): `frame_03316_cockpit_view.png`,
`frame_03332_cockpit_view_second_eye.png`, `frame_03348_chase_after_cockpit.png`. The cockpit
frame now shows the interior at the 46.83 degree vertical angle; the panel is not visibly
smaller than before, consistent with the unit-scale gap above. No side-by-side with the
owner's original frame was made, so no fidelity claim is made.
