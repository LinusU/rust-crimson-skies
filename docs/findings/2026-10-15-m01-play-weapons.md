# M01-PLAY-WEAPONS (#1437): the designed M01 loadout, the wind policy and the target-gap

**Task:** #1437 `M01-PLAY-WEAPONS` — "Let the player fire guns and rockets in
M01 and destroy targets".
**Status:** implementation complete, checked by `accept_m01_play_weapons_`.
Nothing in it is `verified_original`.

## What was wired

`crate::mission_session::weapons` opens the player's weapon sessions for a
running mission and is installed by `MissionWeaponsPlugin`, added by
`mission_session::compose::add_composition` beside the already-present
`WeaponSchedulePlugin` and `DamageSchedulePlugin`:

* a `WeaponSession` (F27) with three designed guns — a nose cannon and one gun
  per wing, all one selected bank;
* an `OrdnanceSession` (F28) with one designed proximity-fused flak rocket;
* the shared `SessionDamageResolver` and a `TargetingSession`;
* one `WeaponTickInput` per committed fixed tick, published before
  `drive_weapon_session` so the record serves the tick the ledger just
  committed;
* one `drive_mission_ordnance` run per tick, after the gun step and before
  `drive_damage_schedule`, so a rocket's resolved batch reaches the same tick's
  damage consumers (colliders, debris, score);
* the player's rig: a `WeaponActorBinding`, an `OrdnanceLauncherBinding` and one
  `MountPoseBinding` node per gun and launcher, hung under the live player body.

Fire keys: **`Space` → `FirePrimary`** (guns) and **`Left Ctrl` →
`FireSecondary`** (rockets) — the F22 designed default bindings, documented in
`docs/PLAYTEST.md`.

## The loadout is designed

`mission_launch`'s `player_configuration` surface recovers the airframe and the
start pose and nothing else. No production reader returns M01's Devastator
guns, ammunition or rocket loadout from the installation (`cs_content::weapons`
and `cs_content::ordnance` are populated in production only by synthetic
fixtures; `read_original_rocket_blasts` yields raw `weapons.zrd` blast numbers,
not an airframe loadout). Every record this task adds is therefore
`Origin::Designed` under the `m01-play-weapons.designed-loadout` claim. F27-E
and F28-D replace them when the original's loadout is measured.

## The wind policy is the session's, and it is labelled

`WeaponTickInput.wind_velocity_m_s` is `Option`, and the F27 schedule refuses to
step a tick whose wind is `None` rather than inventing still air. M01's authored
environment declares no wind, so `mission_wind` falls back to a **designed calm
air** (`[0,0,0]`, claim `m01-play-weapons.designed-calm-air`) whenever
`EnvironmentSession::wind()` reports an explicit unknown, and `MissionWeaponReport`
says which wind a tick resolved under (`designed_calm_air`). The F27 module doc
leaves this policy to the session ("F19-C owns that policy"); this is the
policy, labelled rather than silent. **F19-C should revisit it** once M01's wind
is measured.

## The gap this task does not close: no victim is spawned

Nothing in this slice spawns a target. A victim is an actor the spawn host binds
(`crate::damage::DamageActorBinding`, a `crate::weapons::PartSweptBox`, a
`crate::debris::PartDebrisBinding`) and registers with the session's shared
damage authority and its score ledger. M01's own actors are **M01-PLAY-ACTORS
(#1435)** and the single-player score admission is **VS-M01-SCORE-ADMIT
(#1426)**. With a target bound and admitted, the acceptance tests show the whole
path (fire → swept hit / detonation → F29 damage → destruction → debris →
score ledger). With none bound, the guns fire, the rockets refuse by name
(`no declared-hostile damage target is live`) and nothing resolves — the honest
state, reported, not papered over. The M01 objective ("blow up the fuel truck
on the bridge and the cargo zeppelin's fuel tanks") therefore cannot complete
until #1435 lands; that is a dependency, not a pared-down criterion.

The rocket's engagement node is the designed `hull` key: the original's
per-airframe part names are unmeasured, so a rocket aims at one named part and
the resolver refuses an unknown node by name rather than aiming somewhere else.

## One mechanical hazard worth recording

`crate::weapons::live_mount_transforms` and `part_sweep_candidates` read a
`NodeVisualTransform` (the F27 pose owner), **not** the `GlobalTransform` Bevy's
transform propagation writes. A rig that merely parents its mounts and lets
propagation run fires from the mount's authored local offset near the world
origin. `compose_mission_mount_poses` composes each rig mount's
`NodeVisualTransform` from the body's live global transform and the mount's
local offset each fixed tick, before the publisher and the step. Any future
producer of `MountPoseBinding` nodes in a composed mission must do the same.

## Tests

`crates/cs_app/tests/campaign/m01_play_weapons.rs`, prefix
`accept_m01_play_weapons_`:

* `..._the_fire_keys_are_bound` — `Space`/`Left Ctrl` deliver the two fire
  commands through the mission's own action map;
* `..._the_fire_guns_key_destroys_a_target_and_scores_it`;
* `..._the_fire_rockets_key_destroys_a_target_and_scores_it`;
* `..._a_rocket_with_no_target_is_refused_by_name`;
* `..._the_loadout_is_designed_and_labelled`.

Every one drives the composed mission (`build_headless` over the campaign
suite's synthetic stage) and waits for the destruction through the production
schedules; none calls `step_weapon_session` or `step_ordnance_session`.
