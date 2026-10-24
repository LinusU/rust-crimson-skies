# PARITY-M01-OUTCOME-A (#1653): do the hostile Kestrels attack an idle player in M01?

Task: Rally #1653, 2026-10-18. Implementer: sonnet-2 (Claude Sonnet 5.5).
Capabilities: retail, gpu, audio. Nothing here is `verified_original`.

## Result

**In retail M01 (`ZBD/C3/M01`, "The Lost Treasure") the mission's own program does
not wake the Kestrels for a player who gives no input**, so the acceptance wording
("hostile Kestrels attack an idle player from the mission's AI/script data") cannot
be met from the data in hand. What was built instead is the part of the path that
was missing and is independent of that question: the player is now a damage victim.
The task is blocked on the owner for what the original's idle player actually met.

## Evidence

1. **The Kestrels start asleep.** `ZBD/C3/M01`'s `aiv.zrd` holds `player`,
   `medkestrel_1..3` and `wingman_1`; the three Kestrels carry the measured
   `deactivated` field, 7.6 km from the start (`mission actor … asleep` in the
   windowed log). They get no law, no velocity and no weapons.
2. **One program site wakes them, behind the player's own progress.** The lowered
   control program (`survey_mission_control_programs`, dumped with `{:#?}` and
   read) spells `WAKEUP_ENEMIES [medkestrel_1, medkestrel_2, medkestrel_3]` in
   `OBJECTIVE23` (a second `WAKEUP_ENEMIES` is in `OBJECTIVE39`). `OBJECTIVE23`
   starts `DORMANT -1`; it is reached by `OBJECTIVE22`'s `NAP 23 10 s`, which is
   reached through the chain of `Travelers`, `INACTIVE` and `ANIM_STATE` gates that
   begins with `OBJECTIVE2` — the player `APPROACHING` `t_chamber` within 200 m.
   Nothing in that chain is time-driven from the start.
3. **Run, no input.** `accept_m01_outcome_a_an_idle_player_never_wakes_the_kestrels`
   (retail, headless, 120 simulated seconds at 120 Hz): `WakeEnemiesLedger::applied()`
   is 0 and no Kestrel ever carries a `MissionAircraftAi`. These are the test's
   claim. The hull is *not* untouched: the unflown player drifts into the sea tiles
   from about t=50 s, and the fatal contact rule judges those contacts (#1854), then
   the `WorldColliderInstance { role: Solid }` mesh (#1983). Measured by #2003's
   author on the tree at the #1983 rebase point: before #1983 the contacts spent 600
   of the 1200 designed hull; after it the pool reaches 0 (the idle player crashes).
   The test therefore no longer pins an untouched hull; it asserts that the hull lost
   is exactly the sum of the `TerrainContactJudged::hull_damage` messages (no hostile
   round contributed). See `docs/findings/2026-10-23-crash-e-01-world-collider-is-terrain.md`
   and `docs/findings/2026-10-23-m01-collision-comparison.md`. The windowed `--smoke-engagement --smoke-seconds 120` run says the same
   (below).

## What was built

* `mission_session::player_victim` — the player is registered with the session's
  damage resolver (one lethal `hull` node, pool `DESIGNED_PLAYER_INTEGRITY`), carries
  a `DamageActorBinding`, and has a `PartSweptBox` hull box that follows the body
  every fixed tick, so the rounds the gun step routes can hit it. Before this the
  player was in no resolver and had no box: no round could ever touch it. Pool and
  box are **designed** (`m01-player-victim.designed-hull-integrity`), with the
  unmeasured original table carried beside them as an explicit unknown
  (`m01-player-victim.damage-table-unmeasured`).
* `cs --mission M01 --smoke-seconds <n> --smoke-engagement` — the owner-command smoke
  in an idle mode (no input, no teleport), judging `hostiles_damage_idle_player`
  (`docs/PLAYTEST.md`). The default smoke and `--smoke-no-teleport` are unchanged.

## The windowed run on this Mac

`target/release/cs --cs-path <retail> --mission M01 --smoke-seconds 120 --smoke-engagement
--capture-dir private/evidence/PARITY-M01-OUTCOME-A` exits 1: `hostiles_damage_idle_player`
fails ("the hull stayed at 1200 for the 120s the run watched", samples every 10 s all
1200), `smoke_shots_delivered` passes. Framebuffers `frame_00600_flight.png` (t+10 s) and
`frame_07200_end.png` (t+120 s) exist in the private capture directory; they show no
enemy engaging, because none is awake. They are not committed.

## Unknowns and what is needed

* **What damaged the original idle player at 1:37 is not identified here.** The capture
  is not in this repository. Candidates the data does not exclude (listed, not
  asserted): terrain or sea impact (our idle player is not held up and reaches sea
  level around t+56 s and keeps descending, because no ground or sea collision is
  wired — a #1654 matter), ground fire from `t_truck01..03` / bridge units (not
  modelled as shooters), or a Kestrel wake that the original reaches by a route this
  runtime's observation providers do not yet report (e.g. the `Travelers` approach
  to `t_chamber` or the `dzpath1` danger-zone gate being satisfied by flying the
  start heading).
* Needed from the owner: which of those the capture shows (screenshots/Drydock log at
  run time 1:30-1:37), or the capture itself. Until then the Kestrels cannot honestly
  be made to attack an idle player, and neither the wake nor the timing was changed.
