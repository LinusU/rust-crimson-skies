# F27-C: the weapon session, its per-tick step and the ECS mirror

Date: 2026-10-02. Task: #119 "Wire damage, effects, selection and authoritative
ownership" (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
section `### F27-C`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
sections "Collision and ballistic tests" and "Inputs and outputs".
Predecessors recorded in
`docs/findings/2026-10-02-f27-a-weapon-ammo-schemas-and-fire-events.md`,
`docs/findings/2026-10-02-f27-b-gun-cadence-mounts-and-swept-ballistics.md` and
`docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`
(task #443, already merged). Capabilities used: ordinary build/test only — no
`CS_GAME_DIR` read, no render, no audio, so no `private/evidence/` report is
produced.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/weapons.rs` (extend, this stage's whole change):
  `WeaponRoundMirror`, `WeaponEffect`, `WeaponEffectLog`, `WeaponOrder`,
  `OrderRefusal`, `StepRefusal`, `RoutingRefusal`, `RegisteredWeapon`,
  `WeaponRegistrationError`, `SessionRefusal`, `WeaponSession`,
  `WeaponStep`, `WeaponTick`, `DeniedShot`, `UnreadableMount`, `RoutedRound`,
  `MirrorReport`, `step_weapon_session` and `sync_round_mirrors`.
- `crates/cs_app/src/lib.rs` (wiring only): the module docs name the session
  and the step.
- `crates/cs_app/tests/accept_f27_c_weapon_session_step.rs` (**new**), task test
  prefix `accept_f27_c_`.
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file.

**One observable failure, before the change:** F27-A owned the resolver, F27-B
owned the cadence and the two live-ECS producers, and #443 owned the
swept-hit → `HitEvent` seam, but **no production code ever called them in
sequence**. Nothing advanced the cooldowns and the live rounds together, nothing
carried an accepted `FireEvent` forward so a round's *second* tick could be
routed at all (the router needs the shot for the attacker, the projectile
identity and the declared damage profile), nothing derived a muzzle effect or a
shot sound from an accepted event, nothing applied the bank-selection input a
cockpit would send, and no live round was visible to the ECS at all. The gun
half of the game therefore stopped at "a caller may do this": pressing fire in
the running application produced no shot, no round, no sound, no effect and no
damage, and the `resolve_swept_damage` seam had exactly one caller — a test.

## What this stage wires

`step_weapon_session` is the one path that turns a tick's commands into the
whole weapon pipeline, in this order:

1. **ownership gate** — one session generation owns the cadence, the router,
   the per-mount interaction rules, the accepted-shot records and the effect
   log. A tick that is not strictly after the last one resolved, or any order
   after teardown, is refused whole and changes nothing.
2. **selection** — `WeaponOrder::SelectBank` reaches
   `WeaponState::select` through the session, which owns the cooldown and
   ammunition tables. Switching therefore cannot refill a magazine or restart a
   cooldown, because it does not touch them (AC03).
3. **fire** — `WeaponOrder::Fire` reads the live mount poses
   ([`live_mount_transforms`], F27-B) and hands them to
   [`GunCadence::fire`], which is the only place a round is consumed, a
   cooldown is started and a round is spawned.
4. **effects** — every *accepted* [`FireEvent`] becomes one
   [`WeaponEffect`] carrying that shot's own muzzle-effect and sound catalog
   ids, its mount, its projectile and the muzzle origin. A denied mount or a
   refused whole intent emits nothing (non-negotiable 5), so a disabled gun is
   silent and a duplicate packet is silent.
5. **motion and damage** — the rounds advance once through
   `GunCadence::advance_projectiles`, and each live round's segment is routed
   through the #443 seam with the live part boxes
   ([`part_sweep_candidates`], F27-B) and resolved by the session's
   `DamageResolver`. The accepted shot is *retained* for the round's whole
   life, which is what makes a round that lands several ticks after it was
   fired routable at all.
6. **retirement and mirror** — a round whose declared lifetime is spent is
   retired, its accepted-shot record is released, and `sync_round_mirrors`
   reconciles the ECS: every live round gets a
   [`WeaponRoundMirror`] entity whose `Transform` is *written* from the
   authoritative [`cs_sim::weapons::LiveProjectile`], and a mirror with no live
   round (retired, removed by a hit, or left by a reloaded scene) is despawned.

Teardown is [`WeaponSession::close`]: it releases every round record, marks the
session closed and leaves the mirror pass to despawn what it orphaned.

## Ownership, authority and what is deliberately not here

- **One authority, one session.** The session owns the `GunCadence` (which owns
  the `FireResolver` and the `ProjectileRuntime`), the `GunHitRouter` and the
  per-mount `WeaponRules`; the `DamageResolver` is *borrowed* from the caller,
  because F29 owns that authority's lifecycle and collision/crash producers
  share it. Nothing here applies damage outside it.
- **A round is not retired by hitting something.** Retiring on contact is a
  penetration rule, and `WeaponRules::penetration`/`ricochet` are declared,
  unapplied and deferred to F27-D. The once-per-`(projectile, actor)` ledger, not
  a despawn, is what stops a round from applying its damage twice.
- **The ECS mirror is a mirror.** Its `Transform` is written from the
  authoritative position every tick; it carries no collider, so Avian never
  integrates a round and no second contact authority exists.
- **Selection is a bank of mounts, not a cockpit.** `WeaponOrder::SelectBank`
  is the input boundary; the original's bank names, cycle order and the cockpit
  control that sends it are unmeasured and stay F27-D/F46's, which is why
  `GunBank`'s vocabulary is designed rather than measured.
- **Not in this stage**: scheduling the step inside the session run loop
  (`crates/cs_app/src/run.rs` is not an owner path), an Avian collider or body
  for a round, audio-device playback or rendering of the emitted effects,
  in-flight ammunition switching, and any claim about the original.

## Choices worth stating

- **A round's record holds the shot *and* its gun's rules.** Routing needs the
  attacker, the projectile identity and the declared damage profile (all on the
  shot) and the admission rules (on the gun that fired it), and both are fixed
  for the round's whole life, so they are stored together in one record keyed by
  `ProjectileId` and released only with the round. That makes "a live round can
  always be routed" a structural property instead of a lookup that can fail
  halfway, and it is why the step keeps the accepted shot at all: without it a
  round that lands three ticks after it was fired has nothing to route with.
- **The step consumes its tick.** Cooldowns are walked, rounds move and orders
  fire exactly once per tick, so a repeated tick is refused whole
  (`StepRefusal::StaleTick`) rather than made idempotent. The honest statement is
  the refusal: the first pass really did spend that ammunition, so a caller that
  lost a step's result cannot use a repeat to get a second shot out of it. This is
  also why an advance refusal is *not* retryable — the tick was entered, and the
  report says which part of it failed.
- **Selection goes through `WeaponState::select` and nothing else.** AC03 holds
  by construction rather than by a check: the function that changes the selected
  set cannot touch the ammunition or the cooldown tables, so a bank switched
  mid-cooldown still finds the mount it switched to cooling down.
- **One part-box read per tick, shared by every round.** The candidates are a
  property of the world at the end of the tick, not of one projectile, so the
  swept query is not re-read per round; each round sweeps the same list against
  its own segment.
- **The mirror reconciles rather than tracks.** `sync_round_mirrors` compares the
  live rounds with the mirrors that exist, so no entity id is stored anywhere: a
  reloaded scene's orphan is found by its generation stamp and by having no live
  round, and is despawned by the next pass.
- **Registration is the declared-record boundary.** A gun arrives as
  `cs_content::weapons::DeclaredGunDefinition`, is lowered here (so an unknown
  muzzle velocity or an unmeasured interaction rule refuses by name) and is
  returned to the caller as a `RegisteredWeapon` carrying the lowered mount key
  and the two catalog ids an accepted shot will name. The scene wiring needs
  exactly those and has no other source for them.
- **The mirror's position is an f32 cast of the authoritative f64 position.**
  The authoritative value stays f64 in `cs_sim`; every mirror write is derived
  from it, so there is no second position to drift.

## Test sensitivity (measured, one mutation at a time)

Ten mutations were applied to `crates/cs_app/src/weapons.rs`, one at a time, and
`cargo test -p cs_app --test accept_f27_c_weapon_session_step` was re-run after
each. Every one was caught; the counts are what the run reported, not an
estimate.

| mutation | caught by |
| --- | --- |
| the accepted shot's effect is not recorded (no effect, no log entry) | 4 tests: `accept_f27_c_an_accepted_fire_emits_one_effect_and_mirrors_the_round`, `accept_f27_c_a_disabled_mount_emits_nothing_through_the_step`, `accept_f27_c_an_unreadable_mount_pose_is_named_and_nothing_fires_from_it`, `accept_f27_c_ac03_a_switched_bank_neither_duplicates_fire_nor_refills_ammo` |
| the `SelectBank` order is not applied | `accept_f27_c_ac03_a_switched_bank_neither_duplicates_fire_nor_refills_ammo`, `accept_f27_c_an_unknown_shooter_selection_is_refused_by_name` |
| the stale-tick gate becomes `at < resolved_through` | `accept_f27_c_a_repeated_tick_is_refused_whole` |
| the live rounds are never advanced | 3 tests: `accept_f27_c_a_live_round_sweeps_its_declared_damage_into_the_authority`, `accept_f27_c_ac03_a_switched_bank_neither_duplicates_fire_nor_refills_ammo`, `accept_f27_c_the_declared_rules_admit_candidates_through_the_step` |
| the advance runs but the sweep is skipped | the same 3 tests |
| a retired round's routing record is kept | `accept_f27_c_a_spent_lifetime_retires_the_round_and_its_mirror` |
| the ECS mirror pass is skipped | 4 tests: `accept_f27_c_an_accepted_fire_emits_one_effect_and_mirrors_the_round`, `accept_f27_c_a_spent_lifetime_retires_the_round_and_its_mirror`, `accept_f27_c_removing_a_round_releases_its_record_and_its_mirror`, `accept_f27_c_teardown_releases_the_rounds_and_refuses_later_orders` |
| the live mount poses are not read from the hierarchy | all 12 except `accept_f27_c_a_repeated_tick_is_refused_whole`, `accept_f27_c_a_session_on_generation_zero_refuses_at_the_door` and `accept_f27_c_an_unknown_shooter_selection_is_refused_by_name` (the three that never fire) |
| the teardown keeps the mirrors | `accept_f27_c_teardown_releases_the_rounds_and_refuses_later_orders` |
| the live part boxes are not read from the world | `accept_f27_c_a_live_round_sweeps_its_declared_damage_into_the_authority`, `accept_f27_c_the_declared_rules_admit_candidates_through_the_step` |

The mutation harness is a scratch script under the session's temporary
directory, not a committed tool: it rewrites production source in place and
restores it after each run, which is a one-off measurement, not a test.

## Unknowns recorded (not guessed)

- **The original's cockpit bank names, cycle order and the control that sends
  them.** `WeaponOrder::SelectBank` is the input boundary and a bank is the set
  of mounts it names; nothing here claims what the original's cockpit called
  them or how a pilot cycled them. F27-D audits the data, F46 owns the control.
- **Whether a hit retires a round in the original.** Not applied here, and the
  finding records why: that is the penetration rule F27-D still owes. A round
  that crosses a part keeps flying in this stage, which is also what the
  once-per-`(projectile, actor)` ledger is for.
- **A round's orientation, and any per-tick angular motion of a target part.**
  The mirror carries position only, and `part_sweep_candidates` reconstructs a
  part's previous centre from linear velocity alone (F27-B's recorded omission,
  unchanged here).
- **Every numeric value in these tests** is the synthetic fixture's own: muzzle
  velocity, cadence, lifetime, damage, starting rounds, and the geometry of the
  boxes the sweep tests.
- **The step is not scheduled.** It is a function, called by whoever owns the
  session's tick. `crates/cs_app/src/run.rs` is not an owner path for this task,
  so the schedule registration is follow-up work rather than something guessed
  around.

## Not claimed

No original-data verification, no claim that any gun, ammunition type, damage
amount or convergence rule matches the 2000 original, no audio or visual
verification of the emitted effects, no Avian projectile body, no cockpit input
device, no in-flight ammunition switching, and no coverage of
`penetration`/`ricochet`/`ammo_switching`, which stay deferred to F27-D as
`cs_content::weapons::InteractionRules::deferred` already reports. This stage
awards at most **checked**.