# M01-LC smoke through the launch cinematic (#1702)

Date: 2026-10-19. Implementer: `claude-1`. Nothing here is `verified_original`.
Test prefix `accept_m01_owner_smoke_cinematic_`.

## What changed

The scripted owner-command smoke (`cs --cs-path <retail> --mission M01
--smoke-seconds 45 --capture-dir <dir>`) now installs the #1663 launch
cinematic phase (4818 ticks at 120 Hz for the M01 default) and runs through it
before its checks. The script has a `Cinematic` phase first: it takes
`cinematic_start`, `cinematic_mid`, `cinematic_end` (still playing) and
`cinematic_handover` (6 frames after control returned) screenshots, then settles
for 30 frames and carries on unchanged. The objective probe ignores the phase
(the HUD is meant to be held back during it). A new check
`launch_cinematic_plays` judges: all ticks played, handed over, body drift
<= 0.5 m (designed ceiling), the `LaunchLetterbox` flag on while playing and off
after. `--smoke-no-teleport` and capture scenarios still fly from frame 1.

Owner run on this branch (retail, gpu, audio): 15 checks, 0 failed, 2881 frames,
`report.json` `cinematic`: 4818/4818 ticks, drift 0 m, handed over.

## What the screenshots show (honest reading)

* Frames exist for the phase and the hand-over, so "no screenshot" is resolved.
* **They do not show a held HUD or a launch letterbox.** The gauges, crosshair
  and contact labels are drawn in `cinematic_start` and `cinematic_end` just
  as in `rocket` frame 2869, and the black top/bottom bars (picture ~2.33:1) are
  in every frame, including long after hand-over, so they are not the
  `LaunchLetterbox` bars. The check above reads the resource flags, not pixels.
  So "HUD held back" and "bars on" are *not seen* in M01 yet.
* The airframe's attitude differs between `cinematic_start` and
  `cinematic_handover`; the position is held (0 m drift) but the check does not
  measure rotation.

Follow-ups (outside this task's owner paths) are filed in Rally.
