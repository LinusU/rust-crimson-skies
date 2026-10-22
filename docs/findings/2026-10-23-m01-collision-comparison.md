# M01 collision comparison: no-input flight with contact (#1851, part of #1752)

Date: 2026-10-23. Implementer: `claude-2`. Evidence class: our headless M01
composition (`build_headless`, the owner-command's own mission host) against the
owner's measured original run in #1752 (retail 1.02 under Drydock, memory reads
only, times virtual, +-1 s). Nothing here is `verified_original`; nothing was
tuned to the table. Test prefix `accept_parity4_m01_collision_`; the test
`accept_parity4_m01_collision_no_input_trajectory_is_pinned` flies 80 s with no
input and pins what main does today.

## Result (main at the time of writing, no input after the hand-over)

| t | ours (x, y, z) m/s | original (#1752) |
| --- | --- | --- |
| 0 | (-1426.6, 150.0, -1813.7) 55.5 | (-1426.0, 150.0, -1813.0) 111.8 |
| 10 | (-2036.7, 150.0, -2540.9) 100.5 | (-2014.8, 102.6, -2514.7) 113.6 |
| 20 | (-2682.7, 150.0, -3310.8) 100.5 | (-2743.8, 46.3, -3383.5) 113.6 |
| 28 | (-3202.5, 148.3, -3919.8) 100.1 | (-3327.1, 1.2, -4078.6) 113.6 |
| 30 | (-3332.7, 150.0, -4072.0) 100.2 | (-3470.5, 7.2, -4250.0) 111.0 |
| 34 | (-3592.9, 153.9, -4376.7) 100.1 | (-3749.3, 27.5, -4593.4) 110.4 |
| 36 | (-3722.9, 156.0, -4529.1) 100.1 | (-3839.8, 105.3, -4718.8) 78.5 |
| 40 | (-3982.3, 160.4, -4834.1) 100.1 | (-3883.0, 373.9, -4851.8) 75.8 |
| 60 | (-5212.3, 1.1, -6267.6) 100.5 | (-3993.7, 1608.7, -5940.9) 94.6 |
| 70 | (-5740.8, 1.0, -6808.8) 1.0 | (-3899.5, 1925.0, -6902.7) 111.2 |
| 80 | (-5713.1, 1.1, -6883.2) 59.3 | (-3634.7, 1707.9, -8014.6) 123.2 |

Per-second (not in the table): main flies level at 150 m until t=22, drops
1.5 m over a hill at t=23..24, climbs to 173 m by t=50, dives at t=51..54
(`vy` -33..-45 m/s, speed dips to 76 m/s) onto the sea, skims at 1.0..1.1 m from
t=55, is stopped to 19 m/s at t=62, sits at about 1 m/s at t=70..78 and
accelerates again. Player never reset, no contact counted before t=51; 5 ground
contacts by t=60, 8 by t=70. It is never destroyed.

## Divergences, each with the task that would close it

| divergence | measured | closes with |
| --- | --- | --- |
| spawn speed 55.5 m/s, original 111.8 | t=0 | #1746 left the declared 55 m/s `RETAIL_START_SPEED_M_S` (no source); needs a follow-up task that imports the start speed |
| no sink, level cruise at 100.5 m/s | original sinks to 46.3 m at t=20 at 113.6 m/s | #1746 sink and thrust causes (see `2026-10-21-m01-hand-over-pose.md`); open |
| no sea skim at t=28..30 | the original is 1.2 m over the sea at t=28 and bounces to 7.2 m | follows from the two rows above: the sea contact (#1850) works (main skims the sea at t=55..61, never below -3 m), but the flight never gets there |
| no beach impact at t=34..36, no 60 degree throw | speed 110 -> 78 m/s, path +61 degrees | #1852 (owner's finer capture of V+0x924 over t=34..36); the response law of #1849 is a normal impulse with restitution 0.6 and has no extra term |
| trajectory after t=40 unrelated | original climbs to 1925 m at t=70 | consequence of the above |
| ground-blow look-ahead (`groundblow_elev` 400, `groundblow_mag` 10) not applied | unknown whether the measured flight is affected | open unknown from `2026-10-21-m01-collision-response.md`; needs its own task |
| contact damage not compared | the original survives both contacts | destruction threshold unknown (rule 4) |

Heading agrees in the first seconds (positions at t=10 are 58 m apart, mostly altitude).
Nothing is tuned.

## Owner-command smoke

`target/release/cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45
--capture-dir private/evidence/1851`, exit 0, `mission smoke: 3894 frames, 7620
ticks, 66 checks, failures: none`. The scripted 45 s run starts at Fly Mission
and does not reach the sea skim or the beach, so its frames show the hand-over
world with the sea contact source present, not the comparison: cited frames
`frame_02451_world.png`, `frame_02458_sea_visible.png` (the sea
and the island ahead of the player body) and `frame_02854_hud.png`
(pause map). Frames of the skim at t=55 and of the beach impact do not exist and
are unmet. Private artifacts are under `private/evidence/1851/` (not committed).
