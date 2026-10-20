# M01 hand-over pose and no-input flight (#1746)

Date: 2026-10-21. Implementer: `claude-2`. Evidence class: one **original run**
(retail 1.02 under the Drydock emulator, unmodified, memory reads only; values
read from the player vehicle at VA `0x64ef78` -> `+0x150`; position `+0x240`,
velocity `+0x924`, quaternion `+0x150`). Times are virtual, read as +-1 s. It
is a run of that one configuration and is never `verified_original` of any
other. Test prefix `accept_parity4_m01_`.

## Hand-over altitude

The original's vehicle first exists at the hand-over on the stored `aiv.zrd`
record, `(-1426, 150, -1813)`, yaw 40. #1652/#1685 composed the pose with
callback 10's `+100 m` (`launch_body::INTRO_RAISE`), reading it as the pose
control starts from. The run shows 150: the raise belongs to the cinematic's
body, not to the pose at hand-over. `stage_for` now uses the stored pose;
`place_player_at_intro_start` still replays callback 10 and is unchanged. Our
HUD altitude follows (150 m = 492 ft).

## No-input flight: divergences (our law from rest, throttle 0.4)

Test `accept_parity4_m01_free_flight_against_the_measured_trajectory` flies the
production law from the hand-over pose at rest and compares per second, a
sample matching when our trajectory passes within 3 m of it at some time in
t +-1 s (the clock's uncertainty). Only t=0 and t=1 match. At t=22 we are at
`(-2455.7, -917.7, -3040.1)` against the measured `(-2889.6, 35.0, -3557.3)`.
The heading agrees (bearing of the t=10..22 displacement within 1 degree).

| cause | evidence |
| --- | --- |
| no lift from rest | the law has no lift below `LIFT_MIN_SPEED_MPS` (2.4384 m/s), so ours falls ~20 m/s in the first second where the original sinks 0.2 m |
| steady sink | original cruises with a 5.6 m/s sink (nose 2.8 deg down); ours falls 53 m/s vertically at throttle 0.4 |
| thrust / speed | original cruise is 113.5 m/s; ours at 0.4 settles near 80 m/s horizontal (115 m/s at throttle 1.0) |
| spool-up | original accelerates for ~5 s; the hand-over throttle setting is not measured (0.4 is the static intro value) |
| spawn speed | `RETAIL_START_SPEED_M_S` (55 m/s, a declared development value) contradicts the measured near-rest start; left unchanged here, not tuned without a source |

No constant was tuned to the table (rules 4 and 5). The HUD dials read the
body's velocity, so cruise speed and sink are right when the causes above are.
