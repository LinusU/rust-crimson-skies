# PARITY2-M01-CHASE-CAMERA-FRAMING (#1798): the chase view's field of view

Follow-up of `2026-10-20-parity2-m01-chase-camera.md`. Capability: `retail` (decrypted image,
static analysis only). Nothing here is `verified_original`.

## Measured and applied

* The chase view's field of view was the fixture's designed 55 degrees vertical. The original's
  per-mode angle function `0x42b660` returns 60 degrees (horizontal) for every mode but a
  vehicle-attached mode 6 (`fov.default`), and the vertical angle follows from the fixed 4:3 screen
  factor 0.75 (`fov.vertical_derivation`, finding `2026-10-24-parity3-cockpit-fov.md`): **46.83
  degrees vertical** for the chase too. `chase_preset` now declares it (claim
  `OWNER-STATIC-2026-10-21`, class observed-tool) through `cockpit_vertical_fov`.
* The fitted placement (10.1 m / 2.1 m at 55 degrees) is rescaled so the capture fractions
  (lower wing 0.92, fuselage drop 0.20 of the image height) hold at the new angle: distance times
  0.832 (ratio of the half-angle tangents) = **8.4 m astern**, height times 0.692 = **1.45 m**.
  The placement stays **designed**.

## The 16:9 wing proportion is the aspect, not a defect

The original's frame is a fixed 4:3 (factor 0.75, no window input); `PreserveVertical` at 16:9
shows the same vertical extent with a wider view, so the wings span 0.69 x 0.75 = 0.52 of the
width, matching the 0.50 measured in ours. Nothing to change for that.

## Read, not resolved: the original's distance

`camparam.zrd` gives `dist` 13.0 (default) to 25.0 (Balmoral) for the airframes
(`2026-10-21-parity3-camera-modes-01-measured.md`), against the 8.4 m that fits the capture. The
ratio (about 2 for the airframes the file names) points at a model/camera unit scale that is not
recovered, so the file's `dist` is **not** applied; applying it would move the plane away from the
capture. The mode 0 placer `0x42c7f0` (uses `dist` clamped to `dist_min..dist_max`, `thirdp_height`,
`thirdp_pitch`, then `0x42c670`) was read but its vector helpers (`0x53f550`, `0x53fb40`) were not
traced, so the height's unit stays unknown.

## Smoke

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir private/chase-fov`: exit 0,
4082 frames, 72 checks, failures none. `frame_03420_cam_chase.png` (not committed) shows the chase
view; the wing tips still reach the bottom edge in that frame. **No side-by-side with the original
capture was made** in this session.

## Unmet

* Spinner colour (yellow vs the capture's white) and blur-disc strength: not investigated.
* Horizon-level camera-up (#541): unchanged.
* Original distance/height units (above) and the side-by-side with the original frame.
