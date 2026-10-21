# #1854 PARITY4-CRASH-B: the fatal terrain contact trigger

Date: 2026-10-22. Implementer: `claude-1`. Rule source: `2026-10-22-crash-a-fatal-contact-and-crash-camera.md`
(`OWNER-STATIC`; nothing here is `verified_original`).

## What is in the build

* `cs_app::playtest::fatal_contact::judge_terrain_contacts` judges each new contact episode between the
  player's aircraft and terrain (slab, sea tile, area scenery). Severity `s = -dot(normalize(v), n)` uses the
  velocity at the end of the previous tick (the solver has already answered the contact when reports are read)
  and the touching manifold's normal.
* The damage is `crash_damage(health_damage_range, s, true)` (retail `[50, 300]`, doubled for the player), applied
  as a hit with no attacker on the player's hull node through the session's one damage authority. The mission
  host's existing `player_destroyed` rule (#1654) then ends the run as a failure.
* On the hit that destroys the player: `FatalContactRule::crash` records tick, position and cause
  (`FailureCause::TerrainContact`), one `PlayerCrashed` message is written, the body's velocities are zeroed and it
  becomes `RigidBody::Static`; `drive_original_flight` no longer steps that record. A reset despawns the aircraft.
* The composed M01 player and the retail playtest aircraft carry the rule with the imported `crash` record.

## Limits (unresolved)

* **Which contact kills is a design value.** The original's zone armor/health totals are unknown (#1853); the hull
  pool is the designed 1200 (`DESIGNED_PLAYER_INTEGRITY`). With it a single head-on contact (600) is survivable.
  The unit relation between the crash `health_damage_range` and weapon damage is also unmeasured.
* The armor half of the damage pair has no consumer; the angular share of the response is still #1849's limit.
* Only new contact episodes are judged; a body that stays in contact is not damaged again.
* "Removal" is a frozen static body, not the original's removal path (`0x48b920` past `0x48bc04` is undecoded).
* Suite E's `accept_t336_suite_e_image` was already over the 16 MiB `__eh_frame` limit on `main` (16 824 988 B);
  this change adds about 18 KB to it. The `cs` binary stays under the limit by about 1.4 KB.
