# F27-A: Weapon and ammunition schemas, fire intents and swept hits

Date: 2026-10-01. Task: F27-A "Define weapon/ammo schemas and fire-event
tests" (`specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
section `### F27-A`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/mod.rs` (new): the module declaration and the
  flat re-exports of the weapon contract. Not in the sheet's owner-path
  list (`crates/cs_sim/src/weapons/guns.rs`), which names only the leaf; a
  Rust subdirectory module needs its parent file, exactly as
  `crates/cs_sim/src/damage/mod.rs` does for F29-A. **No logic** — three
  `pub` lines and doc comments.
- `crates/cs_sim/src/weapons/guns.rs` (new): the runtime contract —
  `FireIntentId`/`FireEventId`/`ProjectileId`, `AmmunitionId`,
  `GunDefinition`, `GunRate`, `WeaponDamage`, `SpreadCone`,
  `InheritanceRule`, `WeaponRules`/`FriendlyFireRule`, `GunBank`,
  `WeaponState`, `MountTransform`, `FireIntent`, `FireEvent`,
  `ProjectileSpawn`, `FireDenialReason`,
  `IntentRefusal`, `FireResolution`, `FireResolver`, `FireError`, the
  sweep types (`ProjectileSegment`, `SweepTarget`, `SweptHit`, `Ballistics`)
  and the synthetic fixture (`synthetic_gun_definition`,
  `synthetic_ammunition`, `SYNTHETIC_*`).
- `crates/cs_content/src/weapons.rs` (new): the declared,
  provenance-carrying schema — `DeclaredGunDefinition`, `DeclaredAmmunition`,
  `InteractionRules`, `InheritanceRule`, `WeaponSchemaError` validation and
  the `declared_synthetic_gun` / `declared_synthetic_ammunition` fixtures.
- `crates/cs_app/src/weapons.rs` (new): `lower_gun`, `lower_ammunition`,
  `lower_rules`, `WeaponLowerError` and the generation-stamped
  `WeaponActorBinding` ECS record.
- `crates/cs_sim/src/lib.rs`, `crates/cs_content/src/lib.rs`,
  `crates/cs_app/src/lib.rs` (wiring only): module declarations and docs.
- `crates/cs_sim/tests/accept_f27_a_guns.rs`,
  `crates/cs_content/tests/accept_f27_a_weapon_schema.rs`,
  `crates/cs_app/tests/accept_f27_a_weapon_boundary.rs`: the
  `accept_f27_a_*` acceptance tests.
- This file.

**One observable failure:** with an endpoint-only (discrete position)
overlap test, a projectile travelling 1200 m/s at a 1/30 s tick advances
40 m per tick while the target it crosses is 0.5 m thick — both segment
endpoints land outside the target and the shot is silently missed.
`accept_f27_a_high_velocity_projectile_crosses_a_thin_target_and_hits_once`
asserts that both endpoints are outside the target box, that the swept
segment nevertheless reports exactly one hit at the expected
time-of-impact, and that re-sweeping the same projectile applies nothing
further. Removing the slab test, the relative-motion subtraction, the
time-of-impact ordering or the once-per-projectile ledger each makes it
fail.

## Semantics defined at this stage

- **Mount identity is the F29 weapon-mount damage node.** A gun's `mount`
  is a `DamageNodeKey`, and `WeaponState::disabled` holds the same keys, so
  the "disabled mount" that stops a gun firing is literally the
  `DamageEventKind::SystemDisabled { system: Weapon }` node F29-A already
  emits. No third copy of the key grammar is introduced, and no gun can be
  silenced by an unrelated node.
- **Ammunition identity is a catalog id, never an enum.** Non-negotiable 1
  names slug, armor-piercing, dum-dum and explosive as *discovery leads*
  and forbids an unverified multiplier table, so `AmmunitionId` is a
  `ContentId` in the `ammo` namespace and `DeclaredAmmunition` carries
  **no damage numbers at all** — damage lives on the gun definition's
  declared per-channel amounts, authored per gun+ammunition pair by the
  importer. A closed ammunition enum would have been a fabrication.
- **Caliber is a validated free string, not an enum.** The original caliber
  vocabulary is unmeasured; an invented closed set would be a fabrication,
  so an empty caliber is refused and a non-empty one is kept verbatim.
- **Mount transforms are inputs.** `MountTransform { origin, forward,
  inherited_velocity_mps }` is supplied by the live aircraft
  hierarchy/damage state; nothing in this stage has a center-screen origin,
  a fixed muzzle offset or a default transform
  (non-negotiable 2). The world velocity a spawn uses is the explicit
  composition `muzzle_world_velocity_mps`, and which rule composes it is
  declared (`InheritanceRule`), not assumed.
- **Fire intents are resolved once, by the session's authority.**
  `FireResolver` holds one `WeaponState` per registered actor, a cooldown
  ledger in ticks, a per-mount ammunition count and the set of disabled
  mounts. An intent is either wholly refused (`IntentRefusal`: foreign
  session, foreign tick, duplicate intent id, unknown shooter, empty
  selection) with **no state change at all**, or resolved per mount into
  `accepted` fire events and `refused` entries with a reason. Only accepted
  events consume a round, set a cooldown and carry sound and muzzle
  effects — so a denied input and a duplicate network packet drain nothing
  (non-negotiable 5).
- **Switching a bank never refills.** `WeaponState::select` replaces the
  selected bank and nothing else; cooldowns and ammunition live per mount
  and are untouched by a selection change.
- **Swept ballistics with relative motion.** `Ballistics::sweep` subtracts
  the target's own motion from the projectile's, so a target that crosses
  the path *between* ticks is still hit; it then runs a segment-vs-AABB
  slab test against the target's end-of-tick pose and orders the hits by
  ascending time of impact with `ActorId` as the stable tie-breaker
  (`FLIGHT-PHYSICS`, "Collision and ballistic tests"). The degenerate-axis
  branch tests `d == 0.0` exactly rather than introducing a numerical
  epsilon, so no singularity tolerance is invented; a near-zero axis yields
  enormous `t` values that the slab min/max rejects correctly.
- **One projectile applies a hit at most once.** The ledger is keyed by
  `(ProjectileId, ActorId)`, so several collision features reporting the
  same contact, or a second sweep of the same segment, apply one hit.
  Non-negotiable 3 and `FLIGHT-PHYSICS`: "Apply damage once even if several
  collision features report the same hit."
- **Self-hit and friendly fire are declared, load-bearing rules.**
  `WeaponRules { self_hit: SelfHitRule, friendly_fire: FriendlyFireRule }`
  filter the sweep's candidate set through the F30-A
  `Allegiance` vocabulary the relation is carried in, so a gun cannot hit
  its own shooter unless the declared rule allows it, and cannot hit a
  non-hostile actor the declared rule excludes (non-negotiable 4).

## Designed vocabulary, not original data

Everything load-bearing here is newly authored project design carrying
`Origin::SyntheticFixture` / designed provenance. The original gun,
caliber, ammunition, hardpoint and loadout sets, the cadence, muzzle
velocity, lifetime, spread and damage numbers, the bank/selection
vocabulary, the penetration/ricochet/ammo-switching behavior and the
inheritance rule are **all unmeasured** — F27's "Research boundary" says
the public manual establishes no ammunition multiplier or ballistic
parameter, and this stage had no `CS_GAME_DIR` at all. Nothing here claims
`verified_original`; the AC04 ammunition/loadout audit is F27-D.

## Unknowns recorded (not guessed)

- The real ammunition id set. `AmmunitionId` is deliberately open: F27-D
  enumerates the installation's ids and every one of them must map to a
  behavior and a damage consumer. Until then the enumeration is unknown,
  not "the four leads".
- Whether the original penetrates, ricochets, allows friendly fire, allows
  self-hit or permits in-flight ammunition switching. Each is a
  `Resolved<bool>`/enum in `InteractionRules`, refuse-to-lower when
  unknown; the synthetic fixture declares them `Known` **with designed
  provenance only**.
- The original cadence unit, muzzle velocity, projectile lifetime and
  spread model, and whether spread is a cone, a pattern or per-mount.
- Gun convergence: the distance at which a wing pair's barrels converge is
  unmeasured. `ProjectileSpawn::velocity_mps` is already the *resolved*
  world velocity — the mount transform's declared forward axis composed
  with the declared inheritance share; this stage invents no convergence
  distance and no convergence geometry.
- How a bank is named or cycled in the original cockpit (nose/wing/tail/all
  are the common community reading, not evidence). `GunBank` is an
  unordered set of mount keys, so no bank name is invented; the naming and
  cycling UI is F27-C.

## Follow-ups that gate later stages

- **Shared key/record types in `cs_types`.** `DamageNodeKey` is still
  validated independently in `cs_sim` and `cs_content` (F29-A follow-up),
  and F27-A reuses it rather than adding a third copy — but the
  duplication itself is still open.
- **`ActorId` still lives in `cs_sim::damage`.** `cs_sim::weapons`,
  `cs_sim::targeting` and the F27-A sweep all reuse it; the shared
  `IDENTITY-CONTENT` type is still unowned by `cs_types` (F29-A follow-up).
- The ammunition id set and the per-type damage consumer map are F27-D; a
  session cannot claim original weapon behavior until that audit exists.

## Not claimed

No original-data verification, no schedule wiring, no ECS systems, no
audio/effect consumer, no damage routing from a swept hit into
`cs_sim::damage::HitEvent`, no cockpit bank-selection UI. F27-B owns gun
cadence, the mount transforms from the live hierarchy and the swept
ballistics runtime; F27-C owns the damage/effect/selection wiring; F27-D
owns the original audit. The task awards at most **checked** status.