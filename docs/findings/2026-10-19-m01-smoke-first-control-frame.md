# M01 smoke: the first control frame (#1697)

The owner-command smoke now takes `first_control`, the framebuffer of the first
script frame after the player gains control, before any scripted move
(`docs/PLAYTEST.md`, "The first control frame").

## Retail run

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir private/evidence/M01-SMOKE-FIRST-CONTROL`
exited 0: 21 checks, none failed, 27 framebuffers.
`first_control_frame`: tick 4818 (script frame 2410), position
(-1426, 250, -1813) m, yaw 40.0 deg, pitch 0, roll 0, nose (-0.643, 0, -0.766),
0 scripted moves before the frame. The position and 40 deg yaw are the start
pose #1652 and #1685 recorded. The frame is
`private/evidence/M01-SMOKE-FIRST-CONTROL/frame_02410_first_control.png`
(gitignored, 2560x1440).

## Beside `private/ref-drydock/m01/orig25.png`

`orig25.png` (owner capture, 1024x768) is the original at about +25 s of flight,
not at the first control tick, so differences in position and speed are partly
elapsed time. In words only, no pixel-parity claim:

- **Heading:** both compasses show the nose at north-west (`NW` under the
  marker in each), so the heading agrees by eye.
- **Island position:** the original shows large, hazy, green islands across the
  whole horizon with the "[Examine] - Site #1" marker over the middle island,
  left of the crosshair. Ours shows low, dark, flat-shaded islands, one left,
  one centre and one right of the crosshair, with a green hill behind the right
  one. The layout is similar in direction; the shapes, scale, haze and shading
  differ.
- **Zeppelin:** ours shows the cargo zeppelin as a small pale shape just above
  the centre island, at the crosshair, and lists it with the Kestrels as
  "5 o'clock" contacts. The original's frame shows no zeppelin.
- **Other differences seen:** ours has a pale grey sky and a flat teal sea with
  dark polygon patches where the original has a blue sky and a smooth dark
  sea; our speed gauge reads 0 where the original's needle is up, at +25 s.

Nothing here is `verified_original`.
