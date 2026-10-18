# VS-M01-SCORE-ADMIT: the spawn host admits the actors it spawns

Date: 2026-10-18. Task: #1426 (follow-up of F29-C.3b, #704; see
`docs/findings/2026-10-14-f29-c-3b-score-wiring.md`). Capabilities used:
ordinary build/test plus `retail` read access for one ignored test; no render,
no audio, no original run, so no `private/evidence/` report.

## What changed

- `crates/cs_app/src/mission_actors.rs`: `admit_spawned`, called at the three
  places the mission host registers a damage actor with the session's
  `SessionDamageResolver` — a scripted aircraft, a zeppelin, a bound ground
  target. Each is admitted to the `SessionScoreLedger` of the session it was
  registered under; a ledger serving another generation (or none) admits
  nothing, and an admission the ledger refuses is named in the spawn report's
  `refusals` (`score admission refuses: …`).
- `crates/cs_app/src/mission_session/host.rs`: `MissionHost::restart` opens
  the fresh generation's ledger *before* `respawn_mission_actors`. Before,
  `mission_host_restart` replaced the ledger only after the host restart
  returned, so rebuilt actors would have been admitted to the torn-down
  ledger (skipped by the session check) and then dropped with it.
- `crates/cs_app/tests/campaign/f29_c_3c_admit.rs` (linked from
  `tests/suite_a.rs`): four `accept_f29_c_3c_` tests.

## What is admitted, and where each field comes from

| field | source |
| --- | --- |
| actor, session | the allocator id the spawn registered the damage actor under |
| pose | the pose the body was spawned at (aircraft: the measured stored pose; zeppelin: the session's pose; ground target: the bound world position, identity rotation as spawned) |
| lifecycle | `Alive` (just spawned) |
| velocity, angular velocity | zero (spawned at rest; the spawn records no other) |
| control | `Autopilot` for a flown aircraft, `Uncontrolled` for a held aircraft, zeppelin or target |
| flight, damage | `NetActorState::spawn`'s full-health defaults for a fresh actor |

## Recorded gap: weapon banks

The spawn records hold **no** weapon bank for a scripted actor (the original's
per-actor loadouts are unmeasured), so `primary_rounds` and `secondary_rounds`
are admitted as `0`, which states "none known" and is not an invented
loadout. Nothing in scoring reads them. A task that measures an actor's rounds
should publish them to the ledger after admission. The player's own admission
(`mission_session::weapons::admit_actor`) is unchanged.

## Tests

`cargo test -p cs_app --test cs_app_tests_a -- accept_f29_c_3 --include-ignored`:

- `…_a_spawned_actor_is_admitted_and_its_kill_scores`: a bound target is in the
  ledger at its spawn pose, and a kill resolved for it scores once.
- `…_an_unadmitted_actor_is_refused_by_name`: a kill for an actor nobody
  admitted yields `NetStateError::UnknownActor` in `DamageScheduleReport::score`
  and pays nothing.
- `…_a_restart_admits_the_rebuilt_actors_to_the_fresh_ledger`.
- `…_retail_every_damage_actor_is_admitted` (ignored, `CS_GAME_DIR`): every
  `DamageActorBinding` in composed M01 is in the ledger.

Removing the admission call fails the first, third and fourth.

Synthetic and retail-file evidence only; nothing here is `verified_original`.
