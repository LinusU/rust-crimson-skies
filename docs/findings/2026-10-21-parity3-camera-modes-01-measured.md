# PARITY3-CAMERA-MODES.01 (#1823): the original's external camera, look, track-target and zoom parameters

Date: 2026-10-21. Parent #1743 (children #1824 view state, #1825 look modes,
#1826 smoke). Predecessors: `2026-10-21-parity3-cockpit-view-01-original-view-cycle.md`
(the setter, modes 0/6/7), `2026-10-11-t542-cockpit-camera-eye-placement.md`.
Tests: `crates/cs_app/tests/accept_parity3_camera_modes_01.rs`, prefix
`accept_parity3_camera_modes_01_`.

Capability: `retail` (the owner's decrypted image `$CS_ENGINE_IMAGE`, SHA-256
`43540fc9…37d75`, plus read access to `$CS_GAME_DIR/ZBD/zrdr.zbd`). **Static
analysis only; nothing is `verified_original`**, no original run happened.
Committed: 104 cited rows `[[camera_mode_fact]]` in
`data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-21`, each
with VA, the instruction bytes and a statement). No disassembly is committed.
Addresses below are virtual addresses in the image.

## The two halves: code in the image, numbers in `camparam.zrd`

The camera *behaviour* is in the engine; the camera *distances and catch-up
rates* are not in the image. The loader `0x42fa80..0x42fb51` opens the keyed
list `camparam.zrd` (string at `0x621848`; carried in `ZBD/zrdr.zbd`) and fills
11 + 1 records of `0x80` bytes: the `default` record at `0x64efd8`, and one per
airframe at `0x64f058 + i*0x80`, `i` indexing the airframe name table at
`0x620c70` (stride 28: Autogyro, Hellhound, Balmoral, Bloodhawk, Brigand,
Devastator, Firebrand, Fury, Kestrel, Peacemaker, Warhawk). A record starts as
a copy of the default; keys the file names override it. `0x42fb30(i)` selects
the record (`i == 11` the default) into the pointer `[0x64efd0]`.

Keys and record offsets (rows `param.<key>`): `dist` 0x00, `dist_factor` 0x04,
`dist_vary` 0x08, `dist_catch_up` 0x0c, `dist_min` 0x10, `dist_max` 0x14,
`back_dist_min` 0x18, `back_dist_max` 0x1c, `pos_catch_up` 0x20,
`thirdp_height` 0x24, `thirdp_pitch` 0x28 (multiplied by pi/180 on load,
`0x42f835`), `look_catch_up` 0x2c, then the death-cam (0x30..0x3c), crash-cam
(0x40..0x50) and fly-by (0x54..0x7c) keys.

The values the *retail* file holds (read from the owner's data with a private
script; the file itself is not committed, #1824/#1825 should read it at run
time through the ZBD reader, not copy these):

| key | default | Bloodhawk | Fury | Peacemaker | Kestrel | Firebrand | Warhawk | Balmoral |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `dist` | 13.0 | 18.5 | 17.0 | 18.0 | 14.5 | 20.5 | 20.0 | 25.0 |
| `dist_min` | 15.7 | 18.5 | 17.0 | 18.0 | 14.5 | 20.5 | 20.0 | 25.0 |
| `dist_max` | 25.0 | 25.0 | 25.0 | 30.0 | 30.0 | 30.0 | 35.0 | 30.0 |

Default only: `dist_factor` 0.01, `dist_vary` 0.1, `dist_catch_up` 1.0,
`pos_catch_up` 2.0, `thirdp_height` 0.138, `thirdp_pitch` 0.29,
`look_catch_up` 3.0, `back_dist_min` 15.5, `back_dist_max` 55.0. Balmoral also
sets `thirdp_height` 0.2 and `thirdp_pitch` 0.2. Autogyro, Hellhound, Brigand
and Devastator name no record and keep the default. The engine looks up
`crash_min_elev` while the file carries `crash_elev`: that field keeps its
default initialisation (unmeasured; crash camera, outside this task). The
units of the distances (metres?) and which unit `thirdp_pitch` ends in after
the degree-to-radian scale (0.29 -> 0.005 rad as loaded) are **unknown**; the
consumer of `[ctx+0x28]` is `0x42c887` (added to the smoothed look pitch).

## Command → camera mode (rows `register.*`, `handler.*`)

The command registrar `0x537bf0` binds, in the block at `0x4896c9..0x48971d`:

| cmd | label (`cs_types::keymap`) | default key | handler | effect |
| --- | --- | --- | --- | --- |
| 50 | Camera down | F9 | 0x4893b0 | mode 1 |
| 51 | Camera back | F10 | 0x4893d0 | mode 2 |
| 52 | Camera left | F11 | 0x4893f0 | mode 3 |
| 53 | Camera right | F12 | 0x489410 | mode 4 |
| 54 | Chase view | F7 | 0x489430 | **mode 9** |
| 55 | Snap look | K | 0x489450 | look-mode word `[0x64ef68] = 0` |
| 57 | Smooth look | J | 0x489460 | `[0x64ef68] = 1`, look state zeroed |
| 56 | Track target | L | 0x4894a0 | `[0x64ef68] = (== 2) ? 0 : 2` (toggle) |
| 58..66 | Look directions | numpad 1..9 | none registered | polled each frame (below) |
| 67 / 68 | Zoom in / out | numpad + / − | none registered | polled each frame (below) |

All of 50..54 are inert while `[[0x71c298]+0x91d]` is set (same guard as the
F8 cycle). The setter is `0x42c280`; per-frame dispatch `0x42c5c0` by
`[camera+0x14c]` (rows `mode.dispatch_table`, `mode.init_table`):

| mode | function | notes |
| --- | --- | --- |
| 0 | 0x42c7f0 (flag 0) | the default third-person camera; uses `dist_min/max`, zoom, look |
| 2 | 0x42c7f0 (flag 1) | same routine; the flag picks `back_dist_min/max` (+0x18/+0x1c) and a fixed back direction |
| 1 | 0x42ca50 | placed from the target's velocity (`[esi+0x18c..0x194]*0.15`), `dist` smoothed by `dist_catch_up` |
| 3, 4 | 0x42cb70 (flag = mode 4) | beside the target; a side flag `[0x64ef70]` flips when the heading changes by more than pi/2; speed gate 0.1 |
| 5 | 0x42ce00 | camera held at the world point `[0x64ef08..10]` looking at the target |
| 6, 7 | 0x42d980 | the cockpit eye (T542) |
| 8 | 0x42cf10 | not traced |
| 9 | 0x42db40 | camera held at `[0x64ef08..10]` while a timer `[0x64ef40]` runs; initialised by `0x42e1f0` |

`mode.keep_table` (`0x621380`) says which transitions keep the running mode
state: 0↔2, 1→1, 3↔4, any→6/7, 9→9. Every other transition sets `[0x64ef2c]`,
so the next frame runs the mode's init routine first (`0x42dcf0` for 0/2,
`0x42dd50` for 1, `0x42de10` for 3/4, `0x42df90` for 5, `0x42e0b0` for 8,
`0x42e1f0` for 9). Entering mode 0 clears zoom and look; entering 6/7 clears
look only (`setter.zero_*`).

**Mode 9 is the "Chase view" key's target and is a held-point camera**, the
one the `flyby_*` parameters (watch time, radius, switch distance) belong to;
mode 0 is the ordinary behind-the-plane camera. That mode 9 is also what the
owner calls "chase" is unproven: the cycle's `Chase` = mode 0 reading (F8
finding) rests on the observation only. #1824 must not collapse 0 and 9.

### Field of view (rows `fov.*`)

`0x42b660` returns `1.0471976` (60°) for every mode, except `1.3962634` (80°)
when `[camera+0x150] != 0` and the mode is exactly 6; `0x42b570` applies it
(tan(angle/2), times the screen factor `[0x71b3f8]`, `2*atan`). Whether 60°/80°
is a horizontal or vertical angle depends on `[0x71b3f8]`, a run-time value
(unknown). The project's designed 55° vertical chase FOV is therefore *not* a
measured value, and is probably wrong for the cockpit (80° in mode 6).

## Look (commands 55–66; rows `look.*`)

`0x42d010(limit, flag)` is the look reader, called from the chase routine with
`(-pi/2, 0)` (`0x42c87c`) and from the eye view (`0x42da70`). State:
`[0x64ef68]` look mode (0 snap, 1 smooth, 2 track), `[0x64ef60]` pitch target,
`[0x64ef64]` yaw target, `[0x64ef58]`/`[0x64ef5c]` the smoothed pitch/yaw the
cameras read. Directions 58..66 are polled with `0x537c10(command)` each frame
(bits 0..1 of the result = held); x is +1 for left, y +1 for up: 58 (+1,−1),
59 (0,−1), 60 (−1,−1), 61 (+1,0), 63 (−1,0), 64 (+1,+1), 65 (0,+1),
66 (−1,+1); 62 (middle) is only read by the track branch, where it recentres.
A joystick hat is read through `0x536c70(0)` (input mode 2): `0xffff` centred,
else hundredths of a degree.

* **Snap look**: the direction angle `atan2(x, y)` in hundredths of a degree
  (`*57.29578*-100`) gives the yaw target `angle * -0.01 * pi/180`; the pitch
  target is pi/2 for the straight-up key (65), pi/4 for the four diagonals
  (58, 60, 64, 66), 0 for 59, 61, 63. No key: both zero. (Whether "down" 59
  really looks back — yaw ±pi, pitch 0 — follows from the rows but was not
  confirmed by a run.)
* **Smooth look**: each frame the pitch target `+= 2*dt*y`, the yaw target
  `+= 2*dt*x`, pitch clamped to pi/2 (or the caller's limit), yaw wrapped
  (`0x460ab0`).
* **Smoothing of both**: `0x460490(&v, target, rate)` sets
  `v = target + (v - target) * e(rate*dt)` with `e` a polynomial for
  `0 < x < 0.1` and `0x53e2e0` otherwise (read as `exp(-x)`, not traced);
  pitch rate **3.0**, yaw rate **5.0** (`0x42d827`, `0x42d867`).
* **Track target**: the target is `[[0x71c298]+0x948]`; none → both targets
  zero. Otherwise the vector target − player (`+0x204`) is rotated into the
  body frame (`+0x180`), the pitch target is `atan2(y, hypot(x, z))` capped by
  the limit and the yaw target `atan2(-x, -z)`. A second path (flag argument)
  recomputes the angles from the camera's own target vector (`0x71c464..6c`
  are run-time values; unknown).
* The eye view additionally rotates by a fixed **−0.08203 rad (−4.7°)**
  (`0x42dac2`), after the look rotations. #1810's cockpit camera does not apply
  this, nor the look rotations.

## Zoom (commands 67, 68; rows `zoom.*`)

A zoom target `[0x64ef30]` changes by `∓2*dt` per second while command 67
(in, decreases) or 68 (out, increases) is held, clamped to `[0, 1]`; the
smoothed value `[0x64ef38]` follows it with rate **1.5**. Only **mode 0** (not
mode 2) uses it: it adds `[0x64ef38] * 10.0` (`0x603390`) to the chase
distance. So zoom-out is up to 10 more units behind; it is reset on entering
mode 0. Unknown: the meaning of bit 4 of the command-68 query result, which
blocks command 67's decrement (a modifier?), and what the other modes do on the
zoom keys (they ignore them, as far as traced).

## Which owner Drydock frames each mode maps to

The three external frames in #1743 are not labelled with a mode, and no
original run is available to bind them; the mapping stays **unknown**, with the
measured geometry as the only evidence:

* Frame 1 (chase): the ordinary behind-the-plane view = mode 0 at `dist`
  (unzoomed), most likely.
* Frame 2 (close external chase while scraping the hill): mode 0 at/near
  `dist_min`, or mode 2 (`back_dist_min` 15.5): not decidable from the data.
* Frame 3 (external above/behind): mode 1 (Down: placed from the velocity
  with the 2.85 constant) or mode 0 with look pitch up: not decidable.

Modes 3/4 (left/right) and 9 (held point) have no frame in the list; the owner
should say which key produced each frame (the smoke in #1826 would show it).

## Unknown (rule 4)

* The mode placement maths of modes 1, 3, 4, 5, 8, 9 beyond the inputs above
  (the common placer is `0x42c670`; its smoothing of position is
  `pos_catch_up`, its look-at smoothing `look_catch_up`, not traced).
* Horizontal vs vertical meaning of the 60°/80° angles; units of the
  `camparam` distances.
* `exp(-x)` identity of `0x53e2e0`; the `thirdp_pitch` unit after the scale.
* The strings naming modes 1..9 (none found); the HUD message `0x4414a0`
  shows after the F8 cycle.
* Keys 58..68 have no registered handler: they are polled, so a rebinding
  must reach the polled command ids, not a handler (the control table already
  holds them).

## For the next stages

* #1824: dispatch on modes {0,1,2,3,4,9}, keep 0 and 9 distinct, apply the
  `keep_table` reset rule and the zero-on-enter rules; read `camparam.zrd`
  at run time.
* #1825: look state machine above (three modes, two smoothings 3.0/5.0, hat
  codes), track target from the HUD target.
* Follow-up for the cockpit task: FOV 80° in mode 6, the −4.7° eye tilt and
  the look rotations are missing from the #1810 camera.
