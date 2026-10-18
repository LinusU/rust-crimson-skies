# F27-C-FX-SCHED: presenting the weapon effects from the session's fixed loop

Date: 2026-10-18. Task: #1304 "Drive the weapon-effect presentation consumer
from the session fixed loop" (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
non-negotiable 5: "consumption, sound and muzzle effects derive from accepted
fire events; no ammo drain from a denied input or duplicate network packet").
Predecessors: `docs/findings/2026-10-11-f27-c-fx-weapon-effect-presentation.md`
(task #539, the consumer this stage registers — it recorded "Scheduling is the
caller's … registering that schedule is follow-up wiring") and
`docs/findings/2026-10-02-f27-c-weapon-session-wiring.md` (task #119, the
effect log). Capabilities used: **retail, gpu and audio** for one
owner-command M01 smoke (below), which needs no evidence report of its own
because this task's acceptance needs no original data, render device or audio
hardware — everything it asserts is ordinary build/test over synthetic
fixtures.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/weapon_effects.rs` (this stage's main change):
  `WeaponEffectRefusal::NoAudioSession`, `present_weapon_effects_with_audio`,
  `WeaponEffectScheduleMissing`, `WeaponEffectScheduleReport`,
  `WeaponEffectScheduleState`, `drive_weapon_effect_presentation` and
  `WeaponEffectSchedulePlugin`.
- `crates/cs_app/src/weapons.rs`: `WeaponSchedulePlugin` adds the new plugin
  beside `drive_weapon_session` (wiring), and `WeaponEffectLog` keeps the
  monotone `recorded` count (`WeaponSession::effects_recorded`).
- `crates/cs_app/src/mission_session/smoke.rs`: the M01 owner smoke's gun
  check counts accepted shots through that count instead of the pending log.
- `crates/cs_app/tests/accept_f27_c_fx_sched_weapon_effect_schedule.rs` (new,
  registered from `tests/suite_a.rs`), task test prefix `accept_f27_c_fx_`
  (this file's own selection `accept_f27_c_fx_sched_`).
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file.

**One observable failure, before the change:** #538 gave the weapon step a
production driver (`WeaponSchedulePlugin` runs `drive_weapon_session` once per
committed fixed tick), so an accepted shot in the running game wrote one
`WeaponEffect` into the session's log — and `present_weapon_effects` had no
production caller. The only code that ever drained the log was a test, so in
the mission composition the log grew without bound, no shot-sound cue ever
reached the installed router, and no `MuzzleEffectSpawn` record ever existed.
The M01 owner smoke's gun check exposed the same hole from the other side: it
measured the *pending log* as a proxy for "rounds the session accepted",
which is only meaningful as long as nothing consumes the log.

## What this stage wires

`WeaponEffectSchedulePlugin` registers `drive_weapon_effect_presentation` on
the session's fixed loop, and `WeaponSchedulePlugin` adds it, so the two
compositions that run the weapon step — the production `PhysicsSession` and
the composed mission (`mission_session::compose`) — present their shots the
same way, with no per-composition wiring:

1. **Once per committed fixed tick, after the step that emitted the effects.**
   `FixedPostUpdate`, ordered `.after(drive_weapon_session)`, which is itself
   after `PhysicsSystems::StepSimulation`. The committed tick that accepted a
   shot is therefore the tick that presents it: the pass runs once per fixed
   tick and the drain is once per pass, so an effect is presented exactly once
   — never twice, never a tick late. The pass is exclusive over the world for
   the same reason the step is.
2. **One drain, the same one.** The pass calls `present_weapon_effects`, so
   the weapon session's log stays the consumer's only source and the
   once-per-effect property is the existing finding's, unchanged: a denied
   input and a duplicate packet emit no effect and present nothing, and a
   drained batch cannot be re-presented.
3. **The audio half is optional, the visual half is not.** A tick whose world
   has no `AudioSession` still drains the log exactly once and still spawns
   each shot's muzzle record; it refuses that shot's *sound* by name
   (`WeaponEffectRefusal::NoAudioSession`) instead of leaving the effect in the
   log for a later tick. This is the module's own independence rule — an
   undelivered asset already refuses only the audio half — and it keeps
   "presented in the tick that emitted it" true for the visual half in a world
   that never installed audio.
4. **The muzzle teardown follows the session teardown.** The schedule tracks
   the generation it last presented under; when the installed `WeaponSession`
   is a different generation — the same swap or removal
   `drive_weapon_session` tears down in that same tick, because it runs first —
   it calls `release_muzzle_effects` **before** the pass. So the dead
   generation's muzzle spawns go in the tick the generation went away, and the
   new generation's own effects, spawned in that same tick, are not eaten.
5. **An honest gap.** No `WeaponSession` → the tick presents nothing (and
   releases a removed generation's spawns). No `SceneGenerations` → the pass
   presents nothing and names `WeaponEffectScheduleMissing::Generation`, never
   spawning under a guessed stamp; a world in that state cannot have stepped
   either (the weapon schedule names the same absence), so no effect waits
   behind the unserved tick.

## The M01 owner smoke's gun measurement

The smoke's `weapon_guns_fire` check counted `WeaponSession::effects().len()`
during the `Space` hold — the *pending* log. With a production consumer that
drains every committed tick, that view is empty by the time the smoke's
`Last`-stage sample runs, so the check would have gone red without a change to
detect it (the check's own wording, in `docs/PLAYTEST.md`, is "the rounds the
weapon session accepted"). The log now keeps a monotone `recorded` total that
a drain never resets, and the check reads that
(`WeaponSession::effects_recorded`). The check's strength is unchanged: it
still requires at least two rounds through the session during the held second.

**Run of record.** The owner-command smoke on this branch's rebased head (the
2026-10-18 rebase onto `5be476b32`), on this machine (Apple M3 Pro, Metal),
the same commanded run `docs/PLAYTEST.md` documents:

```sh
cargo build --release --locked -p cs_app --bin cs
CS_M01_OWNER_SMOKE_OUT=private/evidence/M01-OWNER-SMOKE-1304 \
CS_CAPABILITIES=retail,gpu,audio CS_ENGINE_IMAGE=… \
cargo test -p cs_app --test cs_app_tests_a -- accept_m01_owner_smoke --include-ignored
```

`report.json` (private, not committed; 13 checks, **passed: true**,
`failures: []`, `human_play: false`) reports the gun check as
**`weapon_guns_fire` pass — "holding Space for a second put 90 rounds through
the mission's weapon session (66 live at once), floor 2"**, the same 90
rounds / 66 live the 2026-10-17 run
(`docs/findings/2026-10-17-m01-owner-smoke.md`) measured. This head also
carries the M01 ground-target binding another task landed in the same window,
and the run has no failures — the only false verdict is `world_sea_visible`,
the skip-by-name that check documents. The muzzle spawns this stage creates
are not drawn by anything (below), so no other check changes;
`weapon_rounds_drawn` still passes on the round tracers `M01-WINDOW-FIRE`
(#1536) draws.

## Test sensitivity (measured, one mutation at a time)

Each mutation was applied to the production source alone and
`cargo test -p cs_app --test cs_app_tests_a -- accept_f27_c_fx_` re-run. The
counts are what the run reported.

| mutation | caught by |
| --- | --- |
| `WeaponSchedulePlugin` no longer adds `WeaponEffectSchedulePlugin` | all 6 new tests (the report resource is absent: nothing presented them) |
| the release is removed (`report.released = 0`, no `release_muzzle_effects`) | 1 test: the session-swap release |
| the pass reads `effects()` instead of draining (`drain_effects()`) | 4 new tests, plus 3 of the existing `accept_f27_c_fx_` tests (the drain's once-only property) |

The mutation harness was a scratch script under the session's temporary
directory, not a committed tool: it rewrote production source in place and
restored it from `git` after each run.

## Unknowns recorded (not guessed)

- **How a `hardpoint_equipment` muzzle-effect id becomes a drawable asset.**
  This stage creates the `MuzzleEffectSpawn` record in production; still no
  pipeline resolves the declared id to a mesh, sprite or particle, so nothing
  is drawn for it and the record is what the render stage will read (the same
  designed boundary `SpawnedDebris` states). Affected content: every muzzle
  flash in the game. Resolving task: `F27-C-FX-RENDER` (#1595, filed from this
  task; it also owns measuring whether the original's declared muzzle-effect id
  resolves to a real asset).
- **One-shot device playback.** The accepted cue reaches the router's one-shot
  ledger; whether a shot is *audible* on real hardware is F41's device-voice
  follow-up, and the mission's own gun-sound binding is
  `M01-WINDOW-FIRE-GUN-SOUND` (#1546). Not claimed here.
- **Which ids and placement the original used.** Every value presented is the
  fixture's or the mission's own designed record; nothing here claims any id,
  bus, gain or spatial placement matches the 2000 original (F27-D's audit).
- **The sequence counter across an audio-session swap.** `WeaponShotSequences`
  outlives the audio session, so a restarted session's first cue continues the
  sequence (the router's fresh ledger accepts it, which is the counter's
  documented rule); whether the original restarted its own shot numbering per
  session is unmeasured and does not affect the ledger's dedup.

## Not claimed

No original-data verification, no audible or visual verification of the
effects, no mesh or sprite resolution for the muzzle effect, no one-shot
device voice, no change to the weapon session itself, and no claim that the
smoke's 90-round measurement is anything but this machine's own windowed
frames. This stage awards at most **checked**.
