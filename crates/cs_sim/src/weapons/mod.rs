//! Guns, ammunition, hardpoints and swept ballistic hits (F27).
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stages
//! `### F27-A` and `### F27-C`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Stage **F27-A** defined the typed fire contract, the swept-hit query and
//! a minimal synthetic fixture. Stage **F27-C** adds the query stage that
//! turns a sweep into damage — where candidate filtering lives and why, in
//! `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`.
//! Still unowned: the per-tick cadence loop and the mount transforms read out
//! of the live aircraft hierarchy (F27-B), the ECS-side effects, audio and
//! bank-selection wiring (F27-C's application half) and the original
//! gun/ammunition audit (F27-D).
//!
//! [`guns`] is that one part:
//!
//! * [`guns::GunDefinition`] is one gun's declared behavior — mount,
//!   caliber, ammunition type, rate, muzzle velocity, lifetime, spread,
//!   damage channels, effects and sound, each its own field. Its `mount`
//!   is a [`crate::damage::DamageNodeKey`], the same key the F29 damage
//!   graph uses for its weapon-mount nodes, so the mount a destroyed
//!   weapon node disables is literally the mount that stops firing.
//! * [`guns::WeaponState`] is one actor's live weapon state: the selected
//!   [`guns::GunBank`], per-mount cooldown in ticks, per-mount remaining
//!   rounds and the disabled mounts.
//! * [`guns::FireIntent`] in, [`guns::FireResolution`] out. Fire intents
//!   are resolved **once** by the per-session [`guns::FireResolver`], the
//!   only thing that consumes a round, starts a cooldown, spawns a
//!   projectile or names a sound and muzzle effect.
//! * [`guns::Ballistics`] is the swept-segment query: relative motion, a
//!   segment-vs-box slab test, earliest time of impact with a stable
//!   tie-breaker, and the `(ProjectileId, ActorId)` ledger that makes one
//!   projectile apply a hit at most once.
//! * [`guns::SweepCandidate`] is the F27-C query's candidate — a swept box,
//!   the damage node a contact with it lands on, and the declared relation
//!   the rules filter it under — and [`guns::GunHitRouter`] turns one
//!   accepted shot's swept contacts into [`crate::damage::HitEvent`]s
//!   carrying the gun definition's own per-channel damage amounts.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`), so every record here is built from those
//! types and this crate's own damage and targeting contracts: no Bevy, no
//! Avian, no renderer, no file access.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod guns;

pub mod ordnance;

pub use ordnance::{
    ActiveStatusEffect, AreaEffect, ArmingRule, ClosestApproach, CompatibilityVerdict,
    EquipmentRules, ExpiredStatusEffect, FuseDecision, FuseInert, FuseRule, FuseTrigger,
    GuidanceError, GuidanceRule, GuidanceSet, GuidanceTracker, GuidanceUpdate, HardpointKind,
    InstallationVerdict, LaunchGeometry, LostTargetBehavior, LostTargetReason, NitroActivationRule,
    NitroError, NitroLedger, NitroOrdnance, NitroParameters, NitroRefusal, NitroTick,
    NitroTradeoffs, OrdnanceComponent, OrdnanceDefinitionError, OrdnanceFamily, OrdnanceId,
    OrdnanceIdError, OrdnanceMedia, OrdnanceRegistry, OrdnanceRegistryError, OrdnanceState,
    OrdnanceStatusEffect, ProjectileOrdnance, ProximityFuse, SYNTHETIC_AREA_DENIAL_KEY,
    SYNTHETIC_AREA_LIFETIME_TICKS, SYNTHETIC_AREA_RADIUS_M, SYNTHETIC_ARMING_TICKS,
    SYNTHETIC_CHOKE_STRENGTH, SYNTHETIC_CHOKE_TICKS, SYNTHETIC_DIRECT_KEY, SYNTHETIC_FLAK_KEY,
    SYNTHETIC_FLAK_LIFETIME_TICKS, SYNTHETIC_GUIDED_KEY, SYNTHETIC_GUIDED_LIFETIME_TICKS,
    SYNTHETIC_LAUNCH_SPEED_MPS, SYNTHETIC_LAUNCHER_MOUNT, SYNTHETIC_LAUNCHER_NODE,
    SYNTHETIC_NITRO_CAPACITY, SYNTHETIC_NITRO_CONSUMPTION_PER_S, SYNTHETIC_NITRO_EXTRA_THRUST_N,
    SYNTHETIC_NITRO_KEY, SYNTHETIC_NITRO_RECOVERY_PER_S, SYNTHETIC_ORDNANCE_ARMOR_DAMAGE,
    SYNTHETIC_ORDNANCE_EFFECT_KEY, SYNTHETIC_ORDNANCE_INTERNAL_DAMAGE,
    SYNTHETIC_ORDNANCE_PARTICLE_KEY, SYNTHETIC_ORDNANCE_SOUND_KEY, SYNTHETIC_STACK_CAPACITY,
    SYNTHETIC_TRIGGER_RADIUS_M, SYNTHETIC_UNIT_MASS_KG, StackLoad, StatusEffectError,
    StatusEffectInstanceId, StatusEffectKind, StatusEffectLedger, StatusEffectTarget,
    TargetObservation, TargetPath, closest_approach, synthetic_aerial_torpedo,
    synthetic_area_denial, synthetic_area_effect, synthetic_channels, synthetic_choke,
    synthetic_direct_explosive, synthetic_guided_rocket, synthetic_launch_geometry,
    synthetic_launcher_mount, synthetic_launcher_node, synthetic_media, synthetic_nitro,
    synthetic_nitro_parameters, synthetic_ordnance, synthetic_ordnance_fixture_claim,
    synthetic_proximity_flak, synthetic_proximity_fuse, synthetic_registry, synthetic_stack_load,
};

pub use guns::{
    AmmunitionId, AmmunitionIdError, Ballistics, CadenceRefusal, FireDenialReason, FireError,
    FireEvent, FireEventId, FireIntent, FireIntentId, FireResolution, FireResolver,
    FriendlyFireRule, GunBank, GunBankError, GunCadence, GunDefinition, GunDefinitionError,
    GunHitRouter, GunMountKind, GunRate, GunStateError, InheritanceRule, IntentRefusal,
    LiveProjectile, MountTransform, MountTransformError, ProjectileId, ProjectileRuntime,
    ProjectileRuntimeError, ProjectileSegment, ProjectileSpawn, ProjectileTick, RoutedHit,
    SYNTHETIC_AMMO_KEY, SYNTHETIC_ARMOR_DAMAGE, SYNTHETIC_CALIBER, SYNTHETIC_EFFECT_KEY,
    SYNTHETIC_GUN_MOUNT, SYNTHETIC_INTERNAL_DAMAGE, SYNTHETIC_LIFETIME_TICKS,
    SYNTHETIC_MUZZLE_VELOCITY_MPS, SYNTHETIC_SOUND_KEY, SYNTHETIC_SPREAD_HALF_ANGLE_RAD,
    SYNTHETIC_STARTING_ROUNDS, SYNTHETIC_TICKS_BETWEEN_SHOTS, SelfHitRule, SpreadCone,
    SweepCandidate, SweepOutcome, SweepRefusal, SweepTarget, SweepTargetError, SweptContact,
    SweptHit, WEAPON_DAMAGE_CHANNELS, WeaponDamage, WeaponRules, WeaponState, synthetic_ammunition,
    synthetic_claim, synthetic_effect, synthetic_gun_definition, synthetic_mount, synthetic_sound,
};
