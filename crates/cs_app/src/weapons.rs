//! The weapon application boundary (F27-A).
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! This module sits between the declared weapon schema
//! ([`cs_content::weapons`]) and the session resolver
//! ([`cs_sim::weapons`]), which cannot see each other — `cs_sim` must not
//! depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_gun`] — the conversion boundary: a validated
//!   [`cs_content::weapons::DeclaredGunDefinition`] becomes the runtime
//!   [`cs_sim::weapons::GunDefinition`] the [`FireResolver`] registers.
//!   Every `Resolved::Unknown` **refuses** rather than guessing: a session
//!   must not fire a gun whose muzzle velocity, cadence, spread or damage
//!   was invented, so the refusal names the field and carries the claim.
//! * [`lower_rules`] — the interaction-rules boundary: the declared
//!   self-hit, friendly-fire, penetration, ricochet and ammo-switching
//!   options become the runtime [`cs_sim::weapons::WeaponRules`], again
//!   refusing each unknown by name (F27 non-negotiable 4).
//! * [`WeaponActorBinding`] — the ECS record tying an entity to its
//!   session-qualified [`cs_sim::damage::ActorId`], its lowered guns and
//!   the declared loadout they came from, generation-stamped like
//!   [`crate::scene::SceneNodeBinding`] so a reload can never leave a stale
//!   binding looking live.
//!
//! Nothing here owns weapon state: the selection, cooldowns, ammunition and
//! disabled mounts are the [`FireResolver`]'s; these are the conversion and
//! binding records the ECS wiring consumes (F27-B/C).
//!
//! [`FireResolver`]: cs_sim::weapons::FireResolver

use bevy::ecs::component::Component;
use cs_content::scene::SceneNodeId;
use cs_content::weapons::{
    AmmunitionId as DeclaredAmmunitionId, DeclaredAmmunition, DeclaredFriendlyFireRule,
    DeclaredGunDefinition, DeclaredGunMountKind, DeclaredInheritanceRule, DeclaredSelfHitRule,
    DeclaredSpreadCone, DeclaredWeaponDamage, InteractionRules,
};
use cs_sim::damage::{ActorId, DamageNodeKey, NodeKeyError};
use cs_sim::weapons::{
    AmmunitionId, AmmunitionIdError, FriendlyFireRule, GunDefinition, GunDefinitionError,
    GunMountKind, GunRate, InheritanceRule, SelfHitRule, SpreadCone, WeaponDamage, WeaponRules,
};
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

use crate::scene::SceneGeneration;

/// Why a declared weapon record could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponLowerError {
    /// A declared mount key could not form a runtime key. Unreachable while
    /// both crates apply the same grammar, kept so the boundary stays
    /// honest if they ever diverge.
    MountKey {
        /// The declared key text.
        key: String,
        /// Why the runtime refused it.
        source: NodeKeyError,
    },
    /// A declared ammunition id could not form a runtime id — a
    /// wrong-namespace id slipped through the declared schema.
    AmmunitionId {
        /// The declared id text.
        id: String,
        /// Why the runtime refused it.
        source: AmmunitionIdError,
    },
    /// A field of the declared gun is `Resolved::Unknown`: no session may
    /// fire a gun whose ballistic parameter was invented.
    UnknownField {
        /// Which field is unknown.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// The runtime refused the assembled definition.
    Definition(GunDefinitionError),
}

impl std::fmt::Display for WeaponLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MountKey { key, source } => {
                write!(f, "mount key {key:?} cannot be lowered: {source}")
            }
            Self::AmmunitionId { id, source } => {
                write!(f, "ammunition id {id:?} cannot be lowered: {source}")
            }
            Self::UnknownField {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "weapon field {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::Definition(source) => {
                write!(f, "the runtime refused the lowered gun: {source}")
            }
        }
    }
}

impl std::error::Error for WeaponLowerError {}

/// Lowers a declared gun definition into the runtime one a
/// [`cs_sim::weapons::FireResolver`] registers.
///
/// The conversion is field-wise and refuses every unknown by name. Mount
/// keys map by text, the mount kind and inheritance rule map variant-wise,
/// and a declared `SceneNodeId` binding is *not* part of the runtime gun:
/// the mount transform is read from the live aircraft hierarchy by F27-B, so
/// carrying the visual binding into the runtime record would smuggle a
/// presentation reference into gameplay state. It stays available on the
/// declared record for F27-B to read there.
pub fn lower_gun(gun: &DeclaredGunDefinition) -> Result<GunDefinition, WeaponLowerError> {
    let mount = lower_mount_key(gun.mount());
    let caliber = known_or_refuse("caliber", gun.caliber())?;
    let ammunition = lower_ammunition_id(gun.ammunition())?;
    let rate = known_or_refuse("rate", gun.rate())?;
    let muzzle_velocity = known_or_refuse("muzzle_velocity_mps", gun.muzzle_velocity_mps())?;
    let lifetime = known_or_refuse("lifetime_ticks", gun.lifetime_ticks())?;
    let spread = lower_spread(gun.spread())?;
    let damage = lower_damage(gun.damage())?;
    let inheritance = lower_inheritance(gun.inheritance())?;
    let effect = known_or_refuse("effect", gun.effect())?;
    let sound = known_or_refuse("sound", gun.sound())?;

    GunDefinition::try_new(
        mount,
        lower_mount_kind(gun.mount_kind()),
        caliber.as_str(),
        ammunition,
        GunRate::try_new(rate.ticks_between_shots).map_err(WeaponLowerError::Definition)?,
        muzzle_velocity,
        lifetime,
        spread,
        damage,
        inheritance,
        effect,
        sound,
    )
    .map_err(WeaponLowerError::Definition)
}

/// Lowers the declared interaction rules into the runtime
/// [`WeaponRules`] a session filters a sweep's candidates with.
///
/// Every option refuses independently: an unmeasured self-hit rule does not
/// become "excluded" and an unmeasured penetration rule does not become
/// "false" — no session runs a gun under a guessed interaction rule
/// (F27 non-negotiable 4).
///
/// # Errors
///
/// [`WeaponLowerError::UnknownField`] naming the first unresolved option.
pub fn lower_rules(rules: &InteractionRules) -> Result<WeaponRules, WeaponLowerError> {
    let self_hit = known_or_refuse("rules.self_hit", &rules.self_hit)?;
    let friendly_fire = known_or_refuse("rules.friendly_fire", &rules.friendly_fire)?;
    let penetration = known_or_refuse("rules.penetration", &rules.penetration)?;
    let ricochet = known_or_refuse("rules.ricochet", &rules.ricochet)?;
    let ammo_switching = known_or_refuse("rules.ammo_switching", &rules.ammo_switching)?;
    Ok(WeaponRules {
        self_hit: match self_hit {
            DeclaredSelfHitRule::Excluded => SelfHitRule::Excluded,
            DeclaredSelfHitRule::Allowed => SelfHitRule::Allowed,
        },
        friendly_fire: match friendly_fire {
            DeclaredFriendlyFireRule::HostileOnly => FriendlyFireRule::HostileOnly,
            DeclaredFriendlyFireRule::NonFriendly => FriendlyFireRule::NonFriendly,
            DeclaredFriendlyFireRule::Everyone => FriendlyFireRule::Everyone,
        },
        penetration,
        ricochet,
        ammo_switching,
    })
}

/// Lowers a declared ammunition record into the runtime
/// [`AmmunitionId`] it names.
///
/// The id is the whole of what the runtime carries: a declared
/// ammunition's damage profile belongs to the gun that is loaded with it,
/// and F27 non-negotiable 1 forbids deriving one from a multiplier table.
/// The declared record stays where the audit reads it.
///
/// # Errors
///
/// [`WeaponLowerError::AmmunitionId`] when the declared id is not in the
/// `ammo` namespace.
pub fn lower_ammunition(ammunition: &DeclaredAmmunition) -> Result<AmmunitionId, WeaponLowerError> {
    AmmunitionId::try_new(ammunition.ammunition().id().clone()).map_err(|source| {
        WeaponLowerError::AmmunitionId {
            id: ammunition.ammunition().as_str().to_owned(),
            source,
        }
    })
}

/// A mount's live world transform, resolved from the aircraft hierarchy.
///
/// F27 non-negotiable 2 requires the mount transform to come from the live
/// hierarchy and damage state rather than a fixed center-screen origin.
/// This record is what F27-B's hierarchy walk produces: the muzzle's world
/// pose and the mount's forward axis at the tick being resolved, together
/// with the scene generation it was read under so a reloaded hierarchy's
/// stale poses are identifiable by mismatch.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MountPoseBinding {
    /// The mount this pose belongs to, by damage-node key.
    pub mount: DamageNodeKey,
    /// The scene generation the hierarchy was read under.
    pub generation: SceneGeneration,
}

/// Component: marks an entity as the visual face of one weapon actor.
///
/// `actor` is the session-qualified [`ActorId`] the [`FireResolver`]
/// registered weapons for, `guns` the catalog ids the runtime definitions
/// were lowered from, `loadout` the declared loadout they came from, and
/// `generation` the scene generation that spawned the binding — so a reload
/// stamps new bindings and stale ones are identified by mismatch, never by
/// surviving pointers (the same rule
/// [`crate::damage::DamageActorBinding`] and
/// [`crate::targeting::TargetableBinding`] follow).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct WeaponActorBinding {
    /// The fire resolver actor this entity presents.
    pub actor: ActorId,
    /// The `weapon` catalog ids this actor's guns were lowered from.
    pub guns: Vec<ContentId>,
    /// The declared loadout the guns came from.
    pub loadout: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

fn lower_mount_key(key: &cs_content::damage::DamageNodeKey) -> DamageNodeKey {
    // Both crates apply the same key grammar, so a declared key always
    // forms a runtime key; the boundary maps by text and a failure here
    // would mean the two grammars had diverged.
    DamageNodeKey::new(key.as_str()).unwrap_or_else(|source| {
        panic!(
            "a declared mount key that satisfied the content grammar must satisfy the runtime \
             grammar: {source}"
        )
    })
}

fn lower_mount_kind(kind: DeclaredGunMountKind) -> GunMountKind {
    match kind {
        DeclaredGunMountKind::Nose => GunMountKind::Nose,
        DeclaredGunMountKind::WingLeft => GunMountKind::WingLeft,
        DeclaredGunMountKind::WingRight => GunMountKind::WingRight,
        DeclaredGunMountKind::Tail => GunMountKind::Tail,
        DeclaredGunMountKind::Gondola => GunMountKind::Gondola,
    }
}

fn lower_ammunition_id(
    resolved: &Resolved<DeclaredAmmunitionId>,
) -> Result<AmmunitionId, WeaponLowerError> {
    let declared = known_or_refuse("ammunition", resolved)?;
    AmmunitionId::try_new(declared.id().clone()).map_err(|source| WeaponLowerError::AmmunitionId {
        id: declared.as_str().to_owned(),
        source,
    })
}

fn lower_spread(spread: &DeclaredSpreadCone) -> Result<SpreadCone, WeaponLowerError> {
    let half_angle: Radians = known_or_refuse("spread.half_angle", &spread.half_angle)?;
    SpreadCone::try_new(half_angle.0).map_err(WeaponLowerError::Definition)
}

fn lower_damage(damage: &DeclaredWeaponDamage) -> Result<WeaponDamage, WeaponLowerError> {
    let armor = known_or_refuse("damage.armor", &damage.armor)?;
    let internal = known_or_refuse("damage.internal", &damage.internal)?;
    WeaponDamage::try_new(armor, internal).map_err(WeaponLowerError::Definition)
}

fn lower_inheritance(
    inheritance: &Resolved<DeclaredInheritanceRule>,
) -> Result<InheritanceRule, WeaponLowerError> {
    let declared = known_or_refuse("inheritance", inheritance)?;
    Ok(match declared {
        DeclaredInheritanceRule::Full => InheritanceRule::Full,
        DeclaredInheritanceRule::Fraction { share } => InheritanceRule::Fraction { share },
        DeclaredInheritanceRule::None => InheritanceRule::None,
    })
}

/// Unwraps a declared [`Resolved`] value, refusing an unknown with its
/// claim and reason rather than substituting a default.
fn known_or_refuse<T: Clone>(
    field: &'static str,
    resolved: &Resolved<T>,
) -> Result<T, WeaponLowerError> {
    match resolved {
        Resolved::Known(Known { value, .. }) => Ok(value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(WeaponLowerError::UnknownField {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// The [`SceneNodeId`] a declared gun's visual mount binding names, when it
/// declares one.
///
/// F27-B reads the live hierarchy through this id; the weapon runtime itself
/// never sees it, because gameplay state must not depend on a presentation
/// reference.
#[must_use]
pub fn declared_scene_binding(gun: &DeclaredGunDefinition) -> Option<&Resolved<SceneNodeId>> {
    gun.scene_binding()
}
