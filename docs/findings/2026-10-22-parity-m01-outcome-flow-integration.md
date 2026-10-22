# PARITY-M01-OUTCOME-FLOW (#1645): integration of the M01 failure flow

Date: 2026-10-22. Implementer: `claude-1` (Rally); no independent review yet.
Capabilities: `retail`, `gpu`, `audio`. Nothing here is `verified_original`; no
original executable was run. The owner's reference frames (1:37 debrief, wing
on fire, 1:11 crash debrief) were not fetched or compared pixel for pixel; they
stay out of Git.

## What is in place (sub-tasks, all merged)

#1653 player is a damage victim and `--smoke-engagement`; #1654 player
destruction settles a failure with run time, gun hit ratio, planes downed;
#1655 the journal debrief screen; #1656 Replay Mission restarts through the
host's restart path. See their findings files.

## Runs on this commit (release build, private frames in
`private/evidence/PARITY-M01-OUTCOME-FLOW/`)

* `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 60 --smoke-failure
  --capture-dir .../failure`: exit 0, 7 checks, 0 failed. Frames
  `frame_db02492_debrief.png` (Mission Failed, Mission Results, Run Time 0:41,
  Gun Hit Ratio 0%, Cash Earned drawn `unknown`, Overall Planes Downed 0, tabs
  Best to Date / Most Recent, Replay Mission, View All Missions, Return to
  Cabin), `frame_db02500_debrief_bare.png`, `frame_db02594_replayed.png`.
  The failure is a scripted lethal hit, not the original's cause.
* `--smoke-seconds 120 --smoke-engagement --capture-dir .../engagement`: exit
  0, `hostiles_damage_idle_player` passes: hull 1200 -> 1100, first hit at
  t+94.7 s (frame 5683), no input, no teleport. Frame `frame_05773_damaged.png`.

## Timing and cause, stated honestly

* **The three Kestrels are still asleep in that run** (`mission actor ...
  asleep`, 7.6 km away; `WAKEUP_ENEMIES` sits behind the player's approach to
  `t_chamber`, see `2026-10-18-m01-hostile-engagement.md`). So the hull loss at
  t+94.7 s is **not** a Kestrel attack, and the source of that hit is **not
  identified here**: the frame shows the idle player at ALT 5 over the sea
  beneath the island. Not attributed, not tuned.
* The capture's first damage is about +70 s after launch and failure at 1:37;
  ours is a first hit at +94.7 s after the run's start and, with no mortal
  damage source wired, no failure at all in a 120 s idle run. The designed hull
  pool (1200) is not measured and was not fitted to the capture.
* Terrain/sea impact failing the mission (owner note 2026-10-20, 1:11 run) is
  not wired: #1752 / #1755 own it. What fired the flak in the capture: #1822
  (blocked on the owner).
* Run Time here counts from the host's ticks; where the original's clock
  starts is unmeasured, so 0:41 vs 1:37 cannot be compared.

## Capture limits

One run, no input after the cinematic, retail 1.02 under Drydock (virtual
time); exact timings may differ on real hardware. It cannot be used to derive
constants.

## Unmet acceptance

1. Hostile Kestrels engaging and damaging an idle player from the mission's own
   data: not reproducible from the data in hand (asleep). Needs the owner's
   answer on #1822 / the wake route.
2. Failure of an idle player ending the mission by itself: needs collision
   (#1752, #1755) or a measured damage source.
3. Cash Earned shows `unknown` (no measured rule); the Mission Succeeded
   debrief and tab states are unphotographed (see #1655's finding).
4. Side-by-side comparison with the owner's frames was not done by this agent.
