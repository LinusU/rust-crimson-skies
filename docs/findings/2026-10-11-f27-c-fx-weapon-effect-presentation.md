# F27-C-FX: consuming the weapon effect log in the audio and render paths

Date: 2026-10-11. Task: #539 "Consume the F27-C weapon effects in the audio and
rendering paths" (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
non-negotiable 5: "consumption, sound and muzzle effects derive from accepted
fire events; no ammo drain from a denied input or duplicate network packet").
Predecessor: `docs/findings/2026-10-02-f27-c-weapon-session-wiring.md` (task
#119, merged), which built `WeaponEffect` / `WeaponEffectLog` and recorded that
"nothing plays or draws them yet". Capabilities used: ordinary build/test only —
no `CS_GAME_DIR` read, no render device, no audio hardware, so no
`private/evidence/` report is produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/weapon_effects.rs` (**new**, this stage's whole change):
  `WEAPON_SHOT_PRODUCER`, `WeaponShotSequences`, `SpatialPlacement`,
  `WeaponShotCue`, `MuzzleEffectSpawn`, `WeaponEffectRefusal`,
  `WeaponEffectReport`, `trigger_shot_sound`, `spawn_muzzle_effect`,
  `present_weapon_effects` and `release_muzzle_effects`.
- `crates/cs_app/src/audio/loops.rs` (extend, one accessor):
  `AudioSession::spec` — the delivered-load lookup the audio half resolves a
  declared shot-sound id through.
- `crates/cs_app/src/lib.rs` (wiring only): the module declaration and one doc
  sentence.
- `crates/cs_app/tests/accept_f27_c_fx_weapon_effect_presentation.rs` (**new**),
  task test prefix `accept_f27_c_fx_`.
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file.

**One observable failure, before the change:** F27-C made every *accepted* shot
emit one `WeaponEffect` carrying the gun's declared muzzle-effect and shot-sound
catalog ids, the mount, the projectile and the muzzle's world position — and the
record's only consumer was the drain API itself. No production code turned an
emitted effect into an `AudioRouter::play_one_shot` cue or into anything the
render path could draw, so pressing fire in the running application was silent
and left no muzzle effect anywhere: the declared ids were carried faithfully to
the edge of the presentation side and then dropped.

## What this stage wires

`present_weapon_effects` is the one presentation pass, and the weapon
session's drained log is its **only** source:

1. **one drain** — it calls `WeaponSession::drain_effects` exactly once per
   call. An effect that was never emitted is therefore structurally incapable of
   being played or drawn, and a caller cannot re-present a drained batch (the
   second call over an unchanged session sees an empty log). The F27-C finding
   warned that a consumer playing both the log and the per-tick report would
   sound every shot twice; with one consumer draining the log, the per-tick
   report stays the tick-aligned view and is never also played here.
2. **the audio half** (`trigger_shot_sound`) resolves the effect's declared
   shot-sound id through the installed load's lowered audio specs — the same
   `AudioAssetSpec` records `insert_audio_session` lowered from the delivered
   closure — and refuses a name the load did not deliver by name
   ([`WeaponEffectRefusal::UnknownAsset`]) instead of playing a guessed clip. A
   resolved spec becomes one `OneShotEvent` (asset, bus and gain all from the
   declared record) stamped with this consumer's own producer serial
   (`WEAPON_SHOT_PRODUCER`) and a per-session monotonic sequence, and is handed
   to `AudioRouter::play_one_shot`, whose outcome — accepted, suppressed
   duplicate, foreign session — is reported exactly as the router answered.
3. **spatialisation** uses the effect's own emitted muzzle position
   (`WeaponEffect::origin`) against the world's `AudioSpatial` listener and
   attenuation policy. A world with no spatial configuration records the cue as
   `SpatialPlacement::NoListener` — reported, never defaulted onto the
   listener's own position, the same rule the loop mixer follows for an
   emitter with no `GlobalTransform`.
4. **the visual half** (`spawn_muzzle_effect`) spawns one
   [`MuzzleEffectSpawn`] entity per accepted shot, carrying the gun's declared
   muzzle-effect catalog id and stamped with the caller's scene generation, with
   its `Transform` written from the emitted muzzle origin. The two halves are
   independent: an undelivered sound asset refuses the audio half by name and
   the muzzle entity still spawns, because a missing audio record is not a
   reason to erase the flash.

Teardown is [`release_muzzle_effects`]: it despawns every muzzle-effect entity
in one call and reports how many went, so a reload, restart or aircraft swap
leaves nothing behind and a second teardown is a no-op.

## Ownership, authority and what is deliberately not here

- **The log is the only source.** Neither half derives an effect from a live
  round, a mount, a catalog iteration or any other state: F27 non-negotiable 5
  makes the accepted fire event the sole origin of sound and muzzle effects, and
  the drain makes that structural rather than a check.
- **No content invention.** The sound id, the muzzle-effect id, the bus, the
  gain and the playback form all arrive on the emitted record and the
  delivered-load's own lowered specs. Nothing here hardcodes a clip, a bus or a
  mesh, and nothing claims any id matches the original (F27's research
  boundary; the original's effect and sound assignments are F27-D's audit).
- **The muzzle entity is a record, not a rendering.** Exactly like
  [`crate::debris::SpawnedDebris`] and [`crate::weapons::WeaponRoundMirror`],
  it exists so the render path has something to read — the declared effect id,
  the pose and the generation — and it carries no mesh, sprite, light or
  lifetime. No pipeline yet resolves a `hardpoint_equipment` id to a drawable
  asset; that resolution belongs to a later stage and is recorded as unknown
  below rather than guessed.
- **Scheduling is the caller's.** Like `step_weapon_session`, this is a
  function, not a registered system: `WeaponSession` lives with its caller, not
  in the ECS, and the pass needs the same three inputs together (weapon
  session, audio session, world). Whoever owns the session's tick calls it once
  per tick after the step; registering that schedule is follow-up wiring, not
  something guessed around here.
- **The one-shot device path is not extended here.** `AudioRouter::play_one_shot`
  answers *what* to play; the ECS mixer currently carries *loop* outcomes to the
  device, and a one-shot device voice path is F41's follow-up space. This stage
  stops at the accepted, placed cue — the same boundary the loop systems had
  before #445 — and the cue report is what that later pass will read.

## Choices worth stating

- **One module drains once instead of two consumers racing for the log.** The
  audio half alone could live under `crates/cs_app/src/audio/` and the visual
  half under the render path, but `WeaponEffectLog::drain` can only be called
  once per record: two independent consumers would need two sources, and the
  second source would be exactly the duplicate-play hazard the log's doc warns
  about. One pass over one drain keeps "never emitted ⇒ never played" a
  structural property and puts both halves in the report a single caller logs.
- **The cue keeps the whole `OneShotEvent`, not a copy of its fields.** The
  event the router accepted *is* the record the later device pass needs; a
  second transcription of asset/bus/gain would be a second place to drift.
- **The sequence counter is caller-held state, not a global.** `AudioEventId`s
  must be strictly increasing per `(session, producer)` in the router's
  one-shot ledger, so the counter belongs to the installed session's
  presentation state (`WeaponShotSequences`, a `Resource` for the future
  scheduled wiring) and restarts with a fresh session, exactly as
  `AudioEmitterIds` does for emitter serials. A burned sequence (a cue that
  refused later) leaves a harmless gap; the ledger only rejects non-increasing
  ids.
- **An unknown sound asset refuses only the audio half.** The muzzle-effect id
  and the shot-sound id are independent declarations of the same accepted
  event; refusing one half by name must not silently erase the other.

## Test sensitivity (measured, one mutation at a time)

Three mutations were applied to `crates/cs_app/src/weapon_effects.rs`, one at a
time, and `cargo test -p cs_app --test accept_f27_c_fx_weapon_effect_presentation`
was re-run after each. The counts are what the run reported, not an estimate.

| mutation | caught by |
| --- | --- |
| the consumer never presents the drained log (`drain_effects()` results are dropped) | all 6 tests |
| the visual half is removed (no muzzle entity is spawned) | 5 tests (all but the spatialisation test, which asserts placements only) |
| the audio half plays a cue per **live round** instead of per **drained effect** (`WeaponEffect::of` fabricated from the session's retained shot records) | 3 tests: the denied-shot test, the repeated-present test and the spatialisation test |

The mutation harness is a scratch script under the session's temporary
directory, not a committed tool: it rewrites production source in place and
restores it after each run.

## Unknowns recorded (not guessed)

- **What the original played and drew for a shot.** Every id here is the
  synthetic fixture's own (`synthetic.fixture_shot`,
  `synthetic.fixture_muzzle`); whether the original's gun catalog pointed at
  the same assets, and what they sounded and looked like, is F27-D's
  original-data audit plus an audible/visual capability, neither of which this
  task has. Affected content: every gun's muzzle flash and shot sound.
  Resolving task: F27-D (`accept_f27_d_`).
- **How a `hardpoint_equipment` muzzle-effect id becomes a drawable asset.**
  No declared pipeline resolves that namespace to a mesh, sprite or particle
  today, so the spawn records the declared id and the pose and draws nothing —
  the same designed boundary `SpawnedDebris` states. Affected content: every
  muzzle-effect entity this pass spawns. Resolving task: a follow-up render
  stage for weapon effects (not yet sequenced; this finding is the record).
- **Whether the original spatialised gun fire at all, and with which law.** The
  cue is placed with F41-B's designed inverse-distance policy, whose original-law
  status is already recorded in
  `docs/findings/2026-10-01-f41-b-loops-and-spatial-emitters.md`; nothing here
  measures an original attenuation. Affected content: every shot cue's mix.
  Resolving task: F41's evidence stage.
- **One-shot device playback.** The accepted cue reaches the router, not yet an
  output device voice; whether a shot is audible on real hardware is an F41
  device-path follow-up plus the `audio` capability, and is not claimed here.

## Not claimed

No original-data verification, no claim that any sound, effect, bus, gain or
spatial placement matches the 2000 original, no audible or visual verification
(no device, no GPU), no mesh or sprite resolution for the muzzle effect, no
scheduled system registration, and no change to the weapon session itself. This
stage awards at most **checked**.
