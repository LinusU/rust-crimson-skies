# M01 opening crew radio starts at the launch hand-over (task #2010)

Provenance: `static` plus headless composed runs of C3 M01 (`retail`); nothing
`verified_original`. The ordering comes from the owner's A/B playtest of
2026-10-23 (retail: the crew speaks only once the player has control).

## Cause

`MissionHost::step` ran every fixed tick, including the 4818 ticks of
`LaunchCinematic`, and advanced the objective lifecycle clock on each.
`ZBD/C3/M01/zrdr.zbd` `OBJECTIVE1 BEGIN_DORMANT 2.0` therefore woke
`snd_HI1Start` (and `WAKEUP_ZEP_TURRETS piratezep`) at tick 240, about 38 s
before the hand-over.

## Change

While `launch_cinematic_playing`, the host holds the *gameplay clock*: the
lifecycle `advance` gets 0 s and the composed animation/objective step commits
0 ticks (the same treatment a playtest pause gets). Everything else keeps
stepping on every tick: mission animations, markers, world actors, weather,
the camera, and the launch `SOUND` one-shots (`introscene1..4`, owned by
`launch_sound.rs`/`measured_voice.rs`, which do not go through the host clock).
The hand-over is the one event: `LaunchCinematic::is_playing` turning false,
which also frees control, HUD and letterbox. Restart rebuilds the host and
reinstalls the phase, so the clock origin repeats; a world without the phase
(skip/bypass, headless) is unchanged and `OBJECTIVE1` wakes at tick 240.

## Measured (headless, C3 M01 `stage_for`)

* No cinematic: first `snd_HI1Start` line queued at tick 240 (2.0 s).
* Short phase (1000 ticks): first line queued 240 ticks after the hand-over.
  The prior code queued it at tick 240, inside the phase.
* Measured phase (4818 ticks): hand-over at tick 4818, no line before it.
* Members are `VO_c1-HA-m1_*`; none is queued twice.

Tests: `accept_m01_radio_handover_*` in `crates/cs_app/tests/campaign/m01_radio_handover.rs`.
The older `m01_play_radio` / `m01_radio_wire` tests read ZBD/C2/M01 (campaign
M06) and are not C3 M01 evidence.

## Unresolved

* Whether the original's first word comes exactly 2.0 s after hand-over or
  immediately is unmeasured; the authored `[2.0]` is kept and counted from the
  hand-over. Only the owner's "not over the cinematic" ordering is established.
* Other `OBJECTIVE1` outputs (zeppelin turret wake) now also start at the
  hand-over; whether the original wakes them earlier is unmeasured.
* Pause during the cinematic: the clock is held by either cause, so it cannot
  advance; no dedicated test drives a pause inside the phase.
* The owner-command smoke trace (cinematic start, hand-over, first radio onset)
  was not run in this session; audible comparison stays with the owner.
