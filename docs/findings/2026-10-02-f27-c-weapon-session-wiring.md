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