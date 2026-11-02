# M01 wingman after the AI ground blow (#2260)

Status: measured on main, `static`/port behaviour only; nothing here is
`verified_original`. The original executable was not run.

## What moved

`accept_m01_wingman_formation_betty_engages_and_closes_on_the_player` and
`accept_m01_scenery_collider_ring_and_cliffs_stop_a_fast_body` failed on main
and were bisected to the commits that let the law's ground blow fire for every
aircraft (`38c0bf7f4` probe for all flying aircraft, `2eea48d98` the blow
itself): the wingman test passes on `38c0bf7f4`, fails on `2eea48d98` with
identical numbers (118 -> 157 m, travelled 4437 m) through `4ca5805ca`. Both
are ignored tests (retail), so CI never saw the move. Neither failure is a
follow-law regression, and the two did not share a root cause beyond the blow.

## Wingman

Betty closes the 118 m spawn gap to 36 m by tick 4565 (t=38 s) with no probe
hit. At tick 4485 (t=37.4 s) her 400 m probe meets the hillside (hit at
398.9 m, owner World) for one tick; the player's probe meets it too. The
player climbs 150 -> 211 m by the end of the run and Betty 170 -> 275 m on a
different line, so the gap opens to 240 m at tick 5060 and ends at 157 m
(inside the smoke's 500 m threshold). The test now pins the closest approach
(< half the spawn gap) and keeps the 500 m end-of-run bound. Whether the
original wingman diverges from the player at the hillside is unknown; the
measured original shows a throw to 373.9 m at t=40 for the player only.

## Scenery colliders

With the blow on, the az 6 / h 110 flight is lifted over the cliff ring
(contacts 0, furthest 475.5 m, no triangle crossing); az 3 and az 6 at
h 25 still contact. The test asks whether the scenery is solid, so its
flights now run with `groundblow_mag` = 0 on the body; the blow keeps its own
pins (`accept_ground_blow_*`, `m01_collision`).
