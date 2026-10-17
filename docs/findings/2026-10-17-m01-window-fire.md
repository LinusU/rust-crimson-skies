# M01-WINDOW-FIRE: why firing produced nothing in the windowed M01, and what draws it now

Task: Rally #1536 (`M01-WINDOW-FIRE`). Date: 2026-10-17. Branch:
`rally/1536-guns-and-rockets-fire-nothing-in-the-win`. Capabilities used:
`build`, `test`. No retail data was read for this finding; its statements
about the retail installation cite the findings that measured them.

## The owner's report

On 2026-10-17, at main `9b75d132`, the owner ran
`cs --cs-path <retail> --mission M01` and found that `Space` (guns) and
`Left Ctrl` (rockets) "produced nothing: no tracers, no projectiles, no
sound". #1437 (`M01-PLAY-WEAPONS`) was marked done and its headless acceptance
tests pass, so the gun and rocket *paths* work.

## Diagnosis: the window fires into records nothing draws

The windowed composition is not a different code path from the acceptance
tests at the seams people suspect — it is one composition, built by
`mission_session::add_composition`, which both `build_windowed` and
`build_headless` call. Tracing the whole chain, link by link:

1. **Keys → session.** `PlaytestPlugin` adds `BevyInputPlugin`
   (`playtest/mod.rs:325`), whose `pump_platform_input`
   (`input/platform.rs:277`) reads `ButtonInput<KeyCode>`, maps
   `Space → Key::Space` and `ControlLeft → Key::LeftControl`
   (`platform.rs:214,216`), and pumps the F22 session once per frame.
2. **Session → frame report.** Delivered edges land in
   `PlatformFrameReport::delivered` (`platform.rs:416`).
3. **Report → mission.** `collect_mission_cockpit_actions`
   (`mission_session/weapons.rs:983`) copies the frame's flight edges into
   `MissionCockpitActions`; `publish_mission_weapon_input` (`:889`) turns them
   into the tick's `WeaponTickInput` and, for rockets, `OrdnanceOrder`s.
4. **Session → F27/F28.** `drive_weapon_session` steps the guns and
   `drive_mission_ordnance` steps the rockets; both schedules are installed by
   the composition (`compose.rs:1067-1072`).

Every one of those links is installed in the window, and nothing in the
windowed path sets the input context away from `InputContext::Flight`, takes
control authority or pauses the session. The one legitimate window-only drop
is focus loss (`input/session.rs:1065`, `PauseReason::FocusLost`, context
`Cinematic`), which the owner's report cannot be about: the player was flying.

**What was actually missing was the presentation half.** Three independent
gaps, each sufficient to make firing invisible and silent:

* **Nothing drew a round.** `sync_round_mirrors` (`weapons.rs:2089`) spawns a
  `WeaponRoundMirror` carrying a `WeaponRoundMirror` component and a
  position-only `Transform` — no mesh, no material, no visibility — and no
  system read those entities.
* **Nothing drew a rocket.** `sync_ordnance_mirrors` (`ordnance.rs:2280`) is
  the same shape for `OrdnanceItemMirror`.
* **Nothing presented a shot.** The weapon-effect consumer
  `present_weapon_effects` (`weapon_effects.rs:266`) — the one that mints a
  muzzle-effect record and resolves a shot-sound cue — had **no production
  caller** in any crate (only `tests/accept_f27_c_fx_weapon_effect_presentation.rs`),
  which is the named gap F27-C-FX-SCHED (#1304). And even a drained cue would
  be unheard: no declared audio record exists for
  `m01-play-weapons.designed-shot` (the id is born in `designed_gun` and never
  authored into an `AudioCatalog`), so `trigger_shot_sound` refuses it by name
  with `WeaponEffectRefusal::UnknownAsset`, and no one-shot device pass plays
  weapon cues yet (F41's follow-up space).

So the window fired exactly as the headless tests proved it would: rounds were
spawned, swept, resolved, damaged and scored — and all of it happened inside
records the player could not see or hear. The headless suite could not catch
this because #1437's tests inject `MissionCockpitActions` directly and assert
on ECS *records*, never on anything drawn.

## A second, real fragility: the collector's frame

`pump_platform_input` and `collect_mission_cockpit_actions` are both systems
in `PreUpdate`, and neither stated an order relative to the other. Whatever
order the executor picked, every edge was eventually collected — but the link
was an unasserted tie-break, and the #1437 suite exercised neither (its fire
helper bypasses both). The order is now stated: the collector runs
`.after(pump_platform_input)` (`mission_session/weapons.rs`,
`MissionWeaponsPlugin::build`), and
`accept_m01_window_fire_the_guns_key_reaches_the_weapon_session_through_the_real_input_path`
asserts the whole chain in one frame — platform report, empty pending
collector, publisher bookkeeping, live round.

## What this task changed

* **Hold-to-fire.** `publish_mission_weapon_input` mints one repeat edge per
  `MISSION_TICKS_BETWEEN_SHOTS` (4 ticks at 120 Hz — designed, and the same
  rate the F27 per-mount cooldown enforces) while the guns key is held, using
  the input session's own `held_edges` (F22-B's level-triggered hold), so a
  pause, focus loss or handover releases the trigger. A press still fires at
  once. `MissionGunHold` is the bookkeeping; `guns_key_held`/`gun_hold_due`
  are its predicates.
* **Tracers and rockets are drawn.** `mission_session/weapon_present.rs`
  parents one drawn child per live mirror: an unlit streak spanning the
  round's travelled segment, and a body oriented along the rocket's flight
  path. Both are `Origin`-style **design** under `m01-window-fire.designed-visuals`;
  only the pose is first-party, and it comes from the authoritative runtime.
* **A refusal is shown.** A new `MissionWeaponReport::rocket_refusals` entry
  publishes a `MissionHudNotice` (designed HUD line, 2 s lifetime), so the
  rocket's named refusal reaches the player's screen.
* **Tests.** `crates/cs_app/tests/campaign/m01_window_fire.rs`,
  prefix `accept_m01_window_fire_`, drives the **real** key path (a
  `ButtonInput<KeyCode>` press through the composed app) instead of injecting
  cockpit edges.

## The gun sound: no measured binding exists

The acceptance asks for "a gun sound, if a gun sound exists in the data".
Checked, and it does not — not as a *binding*:

* The installation holds gun-named members: `30cal_gun.wav` and
  `40cal_gun.wav` in both `soundsl` and `soundsh`, and `chaingun.wav` in
  `soundsh` alone
  (`docs/findings/2026-10-05-f41-d-media-audit.md:51,55,61`).
* **Nothing binds any of them to a gun.** The `BALLISTICS` gun records of
  `weapons.zrd` carry no sound field (`docs/findings/2026-10-11-f14-d-10-engine-gun-collections.md`),
  the F27-E gun-name recovery records no sound binding, and the measured
  `pdevastator` record chain's only sound fields are `engine_sound` and
  `cockpit_engine_sound`
  (`docs/findings/2026-10-11-vs-m01-rt-evidence-audio-binding.md:35-39`).
  F14-D.7 already ruled name-based inference out: "a name is not a class"
  (`docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md:136-138`).

Picking `30cal_gun.wav` because its filename says "gun" is exactly the guess
AGENTS.md rules 4 and 5 forbid, so the shot sound stays the named
`UnknownAsset` refusal. Follow-ups filed: `M01-WINDOW-FIRE-GUN-SOUND`
(measure the binding; blocked on evidence, not code), plus #1304
(schedule the presentation consumer) which this task did **not** take over.

## What is still unknown

* Whether the original's tracers, muzzle flashes and rocket sprites looked
  anything like these designed visuals: no original capture exists
  (`human_play` is unavailable to agents).
* The original's rate of fire. `MISSION_TICKS_BETWEEN_SHOTS = 4` is designed;
  the `FIRE_RATE` column measured in `weapons.zrd` has not been tied to this
  loadout (`F27-E`).
* Whether the notice layout collides with the mission HUD widgets; it is
  placed below the objective line and both are designed layouts.
