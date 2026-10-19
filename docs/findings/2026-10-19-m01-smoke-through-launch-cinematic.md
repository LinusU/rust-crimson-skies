# M01-LC smoke through the launch cinematic (#1702)

Date: 2026-10-19. Implementer: `claude-1`. Nothing here is `verified_original`.
Test prefix `accept_m01_owner_smoke_cinematic_`.

## What changed

The scripted owner-command smoke (`cs --cs-path <retail> --mission M01
--smoke-seconds 45 --capture-dir <dir>`) now runs through the #1663 launch
cinematic phase (installed after the Flight Check, #1672) (4818 ticks at 120 Hz for the M01 default) before its checks. The script has a `Cinematic` phase first: it takes
`cinematic_start`, `cinematic_mid`, `cinematic_end` (still playing) and
`cinematic_handover` (6 frames after control returned) screenshots, then settles
for 30 frames and carries on unchanged. The objective probe ignores the phase
(the HUD is meant to be held back during it). A new check
`launch_cinematic_plays` judges: all ticks played, handed over, body drift
<= 0.5 m (designed ceiling), the `LaunchLetterbox` flag on while playing and off
after. `--smoke-no-teleport` and capture scenarios still fly from frame 1.

Owner run on this branch (retail, gpu, audio): 19 checks (the 4 Flight Check ones included, run after the #1672 rebase), 0 failed, 2882 frames,
`report.json` `cinematic`: 4818/4818 ticks, drift 0 m, handed over.

## What the screenshots show (honest reading)

* Frames exist for the phase and the hand-over, so "no screenshot" is resolved.
* **They do not show a held HUD or a launch letterbox.** The gauges, crosshair
  and contact labels are drawn in `cinematic_start` and `cinematic_end` just
  as in `rocket` frame 2870, and the black top/bottom bars (picture ~2.33:1) are
  in every frame, including long after hand-over, so they are not the
  `LaunchLetterbox` bars. The check above reads the resource flags, not pixels.
  So "HUD held back" and "bars on" are *not seen* in M01 yet.
* The airframe's attitude differs between `cinematic_start` and
  `cinematic_handover`; the position is held (0 m drift) but the check does not
  measure rotation.

Follow-ups (outside this task's owner paths) are filed in Rally.

## Resolution (#1704)

Date: 2026-10-19. Implementer: `claude-2`. Nothing here is `verified_original`.
Test prefix `accept_m01_owner_smoke_cinematic_`.

* **Cause.** The smoke's probe restore (`apply_probe`) wrote
  `Visibility::Inherited` onto every UI node each frame, overriding the
  nodes whose visibility the cinematic owns: it showed the `LaunchLetterbox`
  bars after hand-over (the "permanent" black bars were exactly the measured
  bars, 169/175 rows at 2560x1440, not the viewport) and un-hid the HUD safe
  area during the phase. Separately, the contact and objective markers
  (`actor_census`) are not children of the safe area and were never held.
* **Fix.** Those nodes carry `CinematicOwnedVisibility`, which a restore skips;
  the markers are not drawn while the phase plays.
* **Pixel check.** `launch_cinematic_plays` now also reads the four cinematic
  screenshots: bar rows >= 990 per mille black and <= 20 HUD-green pixels while
  playing; bar rows <= 500 per mille black and >= 200 HUD-green pixels at
  hand-over. Owner run: 1000/0 in start, mid and end, 0/17054 at hand-over.
* **Attitude.** The held body turns 0.040 deg over the whole phase (new
  `max_turn_deg`, ceiling 0.5 deg); the different look between
  `cinematic_start` and `cinematic_handover` is the cinematic camera's
  viewpoint, not the airframe moving.
