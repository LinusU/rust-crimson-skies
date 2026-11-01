# #1845's release-state spawn re-measured with the ground blow (#2246)

Date: 2026-11-01. Implementer: `sonnet-1` (Sonnet 5.5); no independent review yet. Evidence class: measurement of
our own composed M01 under our law, no `verified_original`, and nothing about the original's behaviour. Main
is `ae4682d8a`; the spawn change was applied on a scratch branch (never pushed) by reverting `c493f1fb5`
(the #2109 rollback of `5cf6e2d5c`, see `2026-10-24-m01-release-state-spawn.md`). Conflict in
`m01_collision.rs` (a pin test, irrelevant here) resolved to main's side.

## Question

`2026-10-24-m01-release-state-spawn.md` says the release-state spawn (velocity 0, actual throttle 0, command
still the declared 0.75) made the player fall into the sea 3-9 s after hand-over. The ground-blow assist has landed
since (#2107 and follow-ups). Does M01 stay flyable with the release spawn now?

## Verdict

**No.** With the release spawn on current main, a player who gives no input hits the sea at **tick 273 (t = 4.55 s)**
and is destroyed. The ground blow is active and does not save the flight; it makes the attitude worse.
A pull-up (pitch command 0.2, held from the hand-over) does **not** hit the sea within 10 s: the lowest point is
90.0 m at tick 251, then it climbs. Unmodified main is level and airborne at 150 m with no input for the whole
10 s. So the release spawn is still unflyable for a player who does nothing, and flyable only if they pull up at
once. Whether the original does the same is not answered here (the three unknowns of the older finding,
`launch-flag-clear-site`, `hand-over-command`, `sink-at-rest`, stand).

## Commands

```sh
# trace (retail, ignored test; the same file on both checkouts)
cargo test --locked -p cs_app --test cs_app_tests_d accept_m01_release_groundblow -- --include-ignored --nocapture
# owner-command smoke (docs/PLAYTEST.md)
cargo build --release --locked -p cs_app --bin cs
CS_CAPABILITIES=retail,gpu,audio target/release/cs --cs-path "$CS_GAME_DIR" --mission M01 \
  --smoke-seconds 45 --capture-dir private/evidence/<dir>
```

The test (`crates/cs_app/tests/campaign/m01_release_groundblow.rs`) runs the composed headless mission, one row per
fixed tick (60 Hz) from the first tick after hand-over to tick 600. Pitch is written to the player record in
`FixedFirst`, after `apply_flight_command` has copied the session's neutral command (writing it earlier is
overwritten and gives the no-input trace twice: the first attempt did exactly that). Columns: altitude, speed,
vertical speed (the body's `LinearVelocity`), nose elevation (body `-Z`), the law's actual throttle, whether the
`GroundBlowProbes` entry for the player has a hit, its chord, the ground-blow amount, and the running
ground/obstacle/judged-contact counts. The amount is **recomputed** from the hit with the law's formula
`(elev - chord)/elev * sqrt(dot(+Z, normal))`; the law does not report it, and the player's torque factor and
backfire gate are not included, so it is the `amount`, not the applied torque.

## Unmodified main (declared start: 55.5 m/s along the nose, throttle 0.75)

No input, and pitch command 0.2: first contact: none within 10 s.

| tick | no input: alt m / speed m/s / vert m/s / pitch deg | pitch 0.2: alt / speed / vert / pitch |
| --- | --- | --- |
| 0 | 150.0 / 55.5 / 0.0 / 0.0 | (starts the same) |
| 60 | 150.0 / 78.9 / 0.0 / 0.0 | 150.8 / 76.7 / 2.7 / 9.1 |
| 180 | 150.0 / 97.1 / 0.0 / 0.0 | 177.7 / 84.1 / 24.6 / 25.5 |
| 300 | 150.0 / 100.1 / 0.0 / 0.0 | 245.0 / 77.1 / 41.4 / 41.2 |
| 600 | 150.0 / 100.5 / 0.0 / 0.0 | 492.3 / 54.4 / 51.3 / 79.4 |

The probe never hit and the amount was 0 in either flight (the nose stays at or above the horizon, the sea is 150 m
below and the probe is shorter than that along a level nose). Smoke: 81 checks, failures none, 4129 frames, 8082
ticks, exit 0 (`private/evidence/2246-main/`, 77 framebuffers; not committed).

## Release-state spawn (velocity 0, actual throttle 0, command 0.75) on current main

No input:

| tick | alt m | speed | vert | pitch deg | throttle | probe hit, chord m, blow amount |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 150.0 | 0.3 | -0.3 | 0.0 | 0.01 | no |
| 30 | 147.3 | 11.3 | -10.7 | -18.1 | 0.26 | no |
| 60 | 139.0 | 25.4 | -21.6 | 43.1 | 0.51 | no |
| 120 | 112.2 | 38.4 | -32.8 | -64.5 | 0.75 | yes, 122.7, 337.2 |
| 180 | 73.8 | 43.8 | -41.4 | -13.9 | 0.75 | no |
| 240 | 30.3 | 44.9 | -44.2 | -13.6 | 0.75 | no |
| 269 | 9.2 | 44.8 | -44.1 | -85.8 | 0.75 | yes, 9.9, 498.7 |
| 272 | 6.9 | 45.7 | -44.9 | -63.5 | 0.75 | no |
| 273 | 6.9 | 0.0 | 0.0 | 48.3 | 0.75 | judged contact 1, hull crash |

* The body has no lift at rest, sinks from the first tick (vertical speed -0.3 at tick 0, -21.6 by tick 60)
  and its speed is nearly all downward (45 of 45 m/s at tick 240).
* The probe first hits at **tick 36** (chord ~350 m, the nose pitched below the horizon and the 400 m probe
  reaching the sea) and hits in **106 of the 273 ticks**, with amounts up to **498.7**. From tick 36 the nose
  elevation swings by tens of degrees between ticks (at hit ticks 265-272: -56, 38, 36, 87, -86, -66, 69, -64
  degrees). The pitch column is the body's -Z axis: this is a tumble, not a controlled recovery. The blow is the
  large unnormalised-normal term the older findings describe (`approach.sqrt()` scales by the face area), not a
  gentle assist. I did not separate how much of the tumble is the blow's torque and how much is the law's own
  low-speed attitude: the older finding has the body already at -18 deg pitch at tick 30, before the first hit.
* First (and only) terrain-or-sea contact: **tick 273, altitude 6.9 m** where the body stops (speed 0 afterwards),
  `judged` goes 0 -> 1 and a hull crash follows. `ground` and `obstacle` contact counters stay 0: the contact
  arrives through the judged fatal-contact path alone.

Pitch command 0.2 from tick 0:

| tick | alt m | speed | vert | pitch deg |
| --- | --- | --- | --- | --- |
| 60 | 139.0 | 25.9 | -21.8 | 53.0 |
| 120 | 116.0 | 39.4 | -24.9 | -0.3 |
| 180 | 97.4 | 62.9 | -12.2 | -0.4 |
| 240 | 90.2 | 80.8 | -2.1 | 6.6 |
| 300 | 93.9 | 87.0 | 9.6 | 14.7 |
| 600 | 260.7 | 69.5 | 49.1 | 53.8 |

Lowest altitude **90.0 m at tick 251**; no contact within 10 s. The probe hits in 25 ticks (ticks 36-102, max
amount 102.1) and then stops once the nose is above the sea. Gentle here means 0.2 of full deflection held from
the hand-over; a later pull-up (the player reacting after a second or two) was not measured, and the fall is
only 11.3 m/s at tick 30 but 21.6 m/s at tick 60, so the margin shrinks the longer the player waits. 0.2 was
chosen once, not swept.

Owner-command smoke with the same spawn: **fails**, exit 1, 28 of 80 checks (actors_project_and_cover,
weapon_rocket_damages, the flak group, damage_trail_drawn, the target group, the cockpit-view group and others),
and the terminal is `outcome=failure source=player_destroyed tick=8221` against a hand-over at tick 4818, i.e. the
scripted player was destroyed 3403 ticks (56.7 s) after the hand-over (`private/evidence/2246-release/`). The
smoke's script flies inputs of its own, so this says the scripted flight does not survive the release spawn; I
did not triage the 28 failures one by one, and they may follow from the destroyed player rather than be independent.
The owner-command smoke on unmodified main passes with 81 checks and no failures.

## What this does and does not show

* The ground blow does not rescue the idle player in the release state: crash at 4.55 s, inside the 3-9 s window
  the older finding reported before the blow existed. The probe fires from tick 36 and is the largest
  attitude disturbance in the flight.
* A pull-up from the start does stay clear of the sea for 10 s.
* Nothing here says what the original does at hand-over; every statement is about our law on our composition.
* Main is unchanged by this task. #1845 stays blocked on the owner's capture (#1938, #1843, #1844).

## Follow-ups for the owner

* If the release spawn is revisited, the blow's tumble (amounts of hundreds, nose swinging ±90 deg per tick) needs a
  look first: it is large enough that the flight loses any attitude authority once the probe hits the sea.
