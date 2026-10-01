//! Guns, ammunition, hardpoints and swept ballistic hits (F27).
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Stage **F27-A** defines the typed fire contract, the swept-hit query and
//! a minimal synthetic fixture — not the per-tick cadence loop and the mount
//! transforms read out of the live aircraft hierarchy (F27-B), not the
//! wiring into damage, effects, audio and bank selection (F27-C), and not
//! the original gun/ammunition audit (F27-D).
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
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`), so every record here is built from those
//! types and this crate's own damage and targeting contracts: no Bevy, no
//! Avian, no renderer, no file access.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod guns;

pub use guns::{
    AmmunitionId, AmmunitionIdError, Ballistics, FireDenialReason, FireError, FireEvent,
    FireEventId, FireIntent, FireIntentId, FireResolution, FireResolver, FriendlyFireRule, GunBank,
    GunBankError, GunDefinition, GunDefinitionError, GunMountKind, GunRate, GunStateError,
    InheritanceRule, IntentRefusal, MountTransform, MountTransformError, ProjectileId,
    ProjectileSegment, ProjectileSpawn, SYNTHETIC_AMMO_KEY, SYNTHETIC_ARMOR_DAMAGE,
    SYNTHETIC_CALIBER, SYNTHETIC_EFFECT_KEY, SYNTHETIC_GUN_MOUNT, SYNTHETIC_INTERNAL_DAMAGE,
    SYNTHETIC_LIFETIME_TICKS, SYNTHETIC_MUZZLE_VELOCITY_MPS, SYNTHETIC_SOUND_KEY,
    SYNTHETIC_SPREAD_HALF_ANGLE_RAD, SYNTHETIC_STARTING_ROUNDS, SYNTHETIC_TICKS_BETWEEN_SHOTS,
    SelfHitRule, SpreadCone, SweepTarget, SweepTargetError, SweptHit, WeaponDamage, WeaponRules,
    WeaponState, synthetic_ammunition, synthetic_claim, synthetic_effect, synthetic_gun_definition,
    synthetic_mount, synthetic_sound,
};
