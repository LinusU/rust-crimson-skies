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

### Integration with #1435 after the rebase

#1435 landed on `main` (`mission_actors`) while this branch was open. Its
Startup spawner creates the session's `SessionDamageResolver` if absent and
registers each scripted actor with a designed damage graph whose pool integrity
is an explicit **unknown** (#668: "no M01 damage table is measured"), and it
ranks no allegiances. Two consequences:

* this module's `MissionWeaponSessions::install` **keeps** an installed resolver
  that already serves the same session instead of replacing it, because
  replacing it would drop every registration the actors committed. It only
  installs its own when none is present or the installed one serves a torn-down
  generation;
* M01's own actors still resolve nothing from a gun or rocket hit: their
  integrity pool is an explicit unknown (reported unresolved rather than
  absorbed by an invented capacity) and no target record declares them hostile,
  so the F27 resolution refuses the shot rather than inventing damage. Wiring
  their measured damage table and allegiances is #668/#1435/#1426 work.

The rocket's engagement node is the designed `hull` key: the original's
per-airframe part names are unmeasured, so a rocket aims at one named part and
the resolver refuses an unknown node by name rather than aiming somewhere else.

## The rounds are records, not yet a picture

The task's acceptance also asks that "projectiles and tracers are visible".
That half is **not** implemented by this slice and is not claimed:

* a fired round is mirrored into the world as a `WeaponRoundMirror` — a
  `Transform` and a `ProjectileId` and nothing else (`sync_round_mirrors`);
  the F27-C-FX muzzle effect is a `MuzzleEffectSpawn` record, and a live
  rocket is a component in the ordnance session. None carries a mesh, a
  material, a light or a `Visibility`;
* `crate::weapon_effects::present_weapon_effects`, the consumer that drains
  the session's effect log into a shot-sound cue and a muzzle-effect record,
  is not driven from the composed fixed loop; **F27-C-FX-SCHED (#1304)** owns
  registering that schedule;
* no production pipeline resolves a `hardpoint_equipment` muzzle-effect id or
  a projectile id to a drawable asset — the recorded follow-up is "a
  follow-up render stage for weapon effects (not yet sequenced)"
  (`docs/findings/2026-10-11-f27-c-fx-weapon-effect-presentation.md`).

So in the composed mission the fire keys run the session and resolve damage,
but nothing is drawn at the muzzle or along the round: firing is currently
invisible and silent. This is a named gap, not a passed criterion; the visible
half is owed to #1304 and to the sequenceless render stage it names, and no
fidelity or release claim may treat "projectiles and tracers are visible" as
met.

## The player's actor serial is reserved

M01's scripted actors (M01-PLAY-ACTORS, #1435) allocate one session actor per
spawned aircraft and zeppelin from an `ActorAllocator` that issues serials from
`1` upward under the served session. The player's own actor serial is therefore
**not** a small number: `MISSION_PLAYER_SERIAL` is `u64::MAX`, the one serial
`ActorAllocator::allocate` can never issue (its successor would overflow, so
the allocator refuses instead of wrapping). A serial in the allocator's range
would be handed to the first spawned actor too, making the player and that actor
one identity — the F27 `SelfHitRule::Excluded` would read the player's own shot
on it as a self-hit and the rocket target search would skip it as the shooter.
`accept_m01_play_weapons_the_player_serial_is_reserved_from_the_actor_allocator`
pins the invariant.

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
* `..._the_loadout_is_designed_and_labelled`;
* `..._the_player_serial_is_reserved_from_the_actor_allocator`.

Every one drives the composed mission (`build_headless` over the campaign
suite's synthetic stage) and waits for the destruction through the production
schedules; none calls `step_weapon_session` or `step_ordnance_session`.
