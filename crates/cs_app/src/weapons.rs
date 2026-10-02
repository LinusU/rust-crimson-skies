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
//! * [`resolve_swept_damage`] — the F27-C application consumer: it runs one
//!   accepted shot's swept contacts through the declared rules, the swept
//!   geometry and the [`GunHitRouter`], and hands the resulting
//!   [`HitEvent`]s to the authoritative [`DamageResolver`], so a gun's
//!   declared per-channel damage becomes applied damage through the one
//!   authority that owns it. Every refusal is returned, never swallowed.
//! * [`WeaponSession`] and [`step_weapon_session`] — the F27-C wiring proper:
//!   one session generation's authority over the cadence, the router, the
//!   interaction rules, the live rounds and the effects, and the one
//!   per-tick step that runs selection, fire, the accepted-shot effects,
//!   the sweep into damage and the retirement of spent rounds, plus
//!   [`sync_round_mirrors`], the ECS mirror of the authoritative rounds.
//!
//!
//! Nothing here owns weapon state: the selection, cooldowns, ammunition and
//! disabled mounts are the [`FireResolver`]'s; these are the conversion,
//! binding and application records the ECS wiring consumes (F27-B/C).
//!
//! [`FireResolver`]: cs_sim::weapons::FireResolver
//! [`GunHitRouter`]: cs_sim::weapons::GunHitRouter
//! [`DamageResolver`]: cs_sim::damage::DamageResolver
//! [`HitEvent`]: cs_sim::damage::HitEvent

use std::collections::BTreeMap;

use avian3d::prelude::LinearVelocity;
use bevy::ecs::component::Component;
use bevy::prelude::{ChildOf, Entity, Transform, World};
use cs_content::scene::SceneNodeId;
use cs_content::weapons::{
    AmmunitionId as DeclaredAmmunitionId, DeclaredAmmunition, DeclaredFriendlyFireRule,
    DeclaredGunDefinition, DeclaredGunMountKind, DeclaredInheritanceRule, DeclaredSelfHitRule,
    DeclaredSpreadCone, DeclaredWeaponDamage, InteractionRules,
};
use cs_sim::damage::{
    ActorId, DamageError, DamageNodeKey, DamageResolver, NodeKeyError, TickResolution,
};
use cs_sim::targeting::Allegiance;
use cs_sim::weapons::{
    AmmunitionId, AmmunitionIdError, CadenceRefusal, FireDenialReason, FireError, FireEvent,
    FireIntent, FriendlyFireRule, GunBank, GunCadence, GunDefinition, GunDefinitionError,
    GunHitRouter, GunMountKind, GunRate, GunStateError, InheritanceRule, LiveProjectile,
    MountTransform, ProjectileId, ProjectileRuntimeError, ProjectileSegment, SelfHitRule,
    SpreadCone, SweepCandidate, SweepOutcome, SweepRefusal, SweepTarget, SweepTargetError,
    WeaponDamage, WeaponRules, WeaponState,
};
use cs_types::Tick;
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Radians, UnitVec3, WorldPosition};

use crate::scene::{NodeVisualTransform, SceneGeneration};

/// Why a declared weapon record could not be lowered to the runtime records.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponLowerError {
    /// A declared mount key could not form a runtime key. Retained for
    /// signature compatibility: declared and runtime keys are now the one
    /// shared [`cs_types::content::DamageNodeKey`], so this is unreachable.
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
    let mount = lower_mount_key(gun.mount())?;
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
/// The three options that **no production code applies** — `penetration`,
/// `ricochet` and `ammo_switching` — are carried here too, because the
/// declared record is their only home and dropping them at the boundary would
/// make the declared schema and the runtime disagree about what a gun carries.
/// Their deferral to F27-D, and why none of them is applied, is reported by
/// `cs_content::weapons::InteractionRules::deferred` rather than only in a
/// findings file.
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

fn lower_mount_key(
    key: &cs_content::damage::DamageNodeKey,
) -> Result<DamageNodeKey, WeaponLowerError> {
    // The declared and runtime mount keys are now one shared
    // `cs_types::content::DamageNodeKey`, so lowering is an identity map.
    // The `Result` is kept for the boundary's existing signatures; this
    // cannot fail.
    Ok(key.clone())
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

// ------------------------------------------------------ F27-B live geometry ---
//
// F27-B. `lower_gun` deliberately drops the declared gun's visual
// `SceneNodeId` (F27-A): the runtime gun carries gameplay state, and the
// *live* hierarchy is where the muzzle pose and the airframe's velocity
// actually are. This section is that read.
//
// Two producers turn live ECS state into the runtime geometry `cs_sim`'s
// resolver and swept query consume:
//
// * [`live_mount_transforms`] turns the mount nodes of one actor's live
//   hierarchy into the `DamageNodeKey -> MountTransform` map a `FireIntent`
//   resolves against. A muzzle origin, forward axis and inherited velocity
//   come from the node's composed [`NodeVisualTransform`] and the airframe
//   body's live `LinearVelocity` — never from a fixed center-screen origin
//   (F27 non-negotiable 2).
// * [`part_sweep_candidates`] turns the live part boxes into the
//   [`SweepCandidate`]s the swept query tests, so a round sweeps against the
//   target's real geometry and its motion through the tick (F27
//   non-negotiable 3).
//
// Both are pure `&World` reads that report a refusal by name for every node
// they could not read. Nothing is dropped silently: a mount with no readable
// pose is named here *and* refused by the resolver as
// `MissingMountTransform`, so a gun never fires from the world origin because
// a walk quietly skipped it.
//
// The ECS systems that call these producers, the Avian projectile body and the
// effects/audio consumers are F27-C's application half
// (`docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`).

/// Reason: one live mount node's pose could not become a [`MountTransform`].
///
/// Every variant names the mount, so a caller can attribute a missing or
/// refused pose to the gun that needed it instead of guessing which node was
/// bad.
#[derive(Clone, Debug, PartialEq)]
pub enum MountPoseRefusal {
    /// The node carries a [`MountPoseBinding`] from another scene generation:
    /// its pose belongs to a hierarchy that was replaced.
    StaleGeneration {
        /// The mount the stale binding named.
        mount: DamageNodeKey,
        /// The generation the binding was stamped with.
        found: SceneGeneration,
        /// The generation being read.
        expected: SceneGeneration,
    },
    /// The mount node carries no [`NodeVisualTransform`], so its live pose is
    /// unknown.
    MissingPose {
        /// The mount with no pose.
        mount: DamageNodeKey,
    },
    /// The node's composed translation was not finite.
    NonFiniteOrigin {
        /// The mount with the bad origin.
        mount: DamageNodeKey,
    },
    /// The node's forward axis was zero-length or non-finite, so it cannot be
    /// a direction.
    ZeroForward {
        /// The mount with the bad forward axis.
        mount: DamageNodeKey,
    },
    /// No ancestor of the mount (up to the actor root) carries a live
    /// `LinearVelocity`, so the airframe's inherited velocity is unknown.
    MissingAirframeVelocity {
        /// The mount whose airframe velocity is unknown.
        mount: DamageNodeKey,
    },
    /// The airframe's live velocity had a non-finite component.
    NonFiniteVelocity {
        /// The mount with the bad inherited velocity.
        mount: DamageNodeKey,
    },
}

impl MountPoseRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::StaleGeneration { .. } => "stale_generation",
            Self::MissingPose { .. } => "missing_pose",
            Self::NonFiniteOrigin { .. } => "non_finite_origin",
            Self::ZeroForward { .. } => "zero_forward",
            Self::MissingAirframeVelocity { .. } => "missing_airframe_velocity",
            Self::NonFiniteVelocity { .. } => "non_finite_velocity",
        }
    }

    /// The mount this refusal is about.
    #[must_use]
    pub const fn mount(&self) -> &DamageNodeKey {
        match self {
            Self::StaleGeneration { mount, .. }
            | Self::MissingPose { mount }
            | Self::NonFiniteOrigin { mount }
            | Self::ZeroForward { mount }
            | Self::MissingAirframeVelocity { mount }
            | Self::NonFiniteVelocity { mount } => mount,
        }
    }
}

/// The live mount poses read for one actor: the transforms a `FireIntent`
/// resolves against, and the mounts that could not be read.
///
/// A `FireResolver` consumes [`Self::transforms`] directly; the refused mounts
/// are the evidence for the `MissingMountTransform` refusal that follows. A
/// mount appears in at most one of the two lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveMountTransforms {
    /// The readable mount poses, keyed by damage-node key.
    pub transforms: std::collections::BTreeMap<DamageNodeKey, MountTransform>,
    /// The mounts whose live pose could not be read, by name.
    pub refused: Vec<MountPoseRefusal>,
}

impl LiveMountTransforms {
    /// Whether the read found no readable mount pose.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }

    /// One mount's live pose, if it was read.
    #[must_use]
    pub fn get(&self, mount: &DamageNodeKey) -> Option<&MountTransform> {
        self.transforms.get(mount)
    }
}

/// Reads one actor's live mount poses from the ECS hierarchy.
///
/// `actor` and `generation` select the actor root: the entity carrying a
/// matching [`WeaponActorBinding`], which is the live hierarchy the mounts
/// hang under. Every descendant carrying a [`MountPoseBinding`] is turned into
/// a [`MountTransform`]:
///
/// * the **origin** is the node's composed [`NodeVisualTransform`] translation,
///   so a muzzle tracks the animated airframe pose rather than a fixed
///   center-screen point (F27 non-negotiable 2);
/// * the **forward** is that transform's `-Z` axis, the canonical forward
///   (FLIGHT-PHYSICS, "Coordinate convention");
/// * the **inherited velocity** is the first `LinearVelocity` found walking
///   from the mount up to the actor root — the airframe body's live velocity,
///   which is exactly the vector F27 non-negotiable 2 leaves as a supplied,
///   explicit input.
///
/// A mount whose binding belongs to a stale generation is refused, not read;
/// so is one with no pose, a non-finite origin, a zero forward axis, or no
/// reachable airframe velocity. An actor with no live root yields an empty
/// read (the resolver then refuses the intent's mounts one by one).
#[must_use]
pub fn live_mount_transforms(
    world: &World,
    actor: ActorId,
    generation: SceneGeneration,
) -> LiveMountTransforms {
    let Some(root) = actor_root(world, actor, generation) else {
        return LiveMountTransforms::default();
    };

    let mut read = LiveMountTransforms::default();
    for entity_ref in world.iter_entities() {
        let entity = entity_ref.id();
        let Some(binding) = entity_ref.get::<MountPoseBinding>() else {
            continue;
        };
        if !is_descendant_of(world, entity, root) {
            continue;
        }
        let mount = binding.mount.clone();
        if binding.generation != generation {
            read.refused.push(MountPoseRefusal::StaleGeneration {
                mount,
                found: binding.generation,
                expected: generation,
            });
            continue;
        }
        let Some(visual) = entity_ref.get::<NodeVisualTransform>() else {
            read.refused.push(MountPoseRefusal::MissingPose { mount });
            continue;
        };

        let global = visual.global();
        let translation = global.translation();
        let origin = [
            f64::from(translation.x),
            f64::from(translation.y),
            f64::from(translation.z),
        ];
        let Ok(origin) = WorldPosition::try_new(origin) else {
            read.refused
                .push(MountPoseRefusal::NonFiniteOrigin { mount });
            continue;
        };
        let forward = global.forward().as_vec3();
        let Ok(forward) = UnitVec3::try_new([
            f64::from(forward.x),
            f64::from(forward.y),
            f64::from(forward.z),
        ]) else {
            read.refused.push(MountPoseRefusal::ZeroForward { mount });
            continue;
        };
        let Some(velocity) = airframe_velocity(world, entity) else {
            read.refused
                .push(MountPoseRefusal::MissingAirframeVelocity { mount });
            continue;
        };
        match MountTransform::try_new(origin, forward, velocity) {
            Ok(transform) => {
                read.transforms.insert(mount, transform);
            }
            Err(_) => read
                .refused
                .push(MountPoseRefusal::NonFiniteVelocity { mount }),
        }
    }
    read
}

/// The live actor root whose hierarchy a mount read starts from: the entity
/// stamped with a matching [`WeaponActorBinding`], or `None` when no live
/// actor root exists for `generation`.
fn actor_root(world: &World, actor: ActorId, generation: SceneGeneration) -> Option<Entity> {
    world.iter_entities().find_map(|entity_ref| {
        let binding = entity_ref.get::<WeaponActorBinding>()?;
        (binding.actor == actor && binding.generation == generation).then(|| entity_ref.id())
    })
}

/// Whether `entity` is `root` or a descendant of it in the live `ChildOf`
/// hierarchy.
fn is_descendant_of(world: &World, mut entity: Entity, root: Entity) -> bool {
    loop {
        if entity == root {
            return true;
        }
        match world.get::<ChildOf>(entity) {
            Some(child_of) => entity = child_of.parent(),
            None => return false,
        }
    }
}

/// The live airframe velocity for a mount node: the first `LinearVelocity`
/// found walking from the node up to the hierarchy root, in world m/s.
fn airframe_velocity(world: &World, entity: Entity) -> Option<[f64; 3]> {
    let mut current = Some(entity);
    while let Some(node) = current {
        if let Some(velocity) = world.get::<LinearVelocity>(node) {
            return Some([
                f64::from(velocity.x),
                f64::from(velocity.y),
                f64::from(velocity.z),
            ]);
        }
        current = world.get::<ChildOf>(node).map(ChildOf::parent);
    }
    None
}

/// Component: the declared swept-box geometry of one damage-node part.
///
/// The box's **centre and motion are not here**: the part entity's composed
/// [`NodeVisualTransform`] is the one pose owner (the same value collision
/// evaluates, F11 non-negotiable 4) and the airframe body's `LinearVelocity`
/// is the one velocity owner. This component carries only the part's identity
/// and its box half extents, so [`part_sweep_candidates`] reads a live pose
/// instead of a baked one.
///
/// A part record is a *collision feature's* record: F27-C's finding assigns
/// the part geometry that feeds a [`SweepCandidate`] to F27-B, while the
/// system that calls it is F27-C's. The box is the declared geometry the
/// evidence gives; where the original's part shapes are unmeasured, this
/// stage records the unknown rather than inventing one.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct PartSweptBox {
    /// The actor the part belongs to.
    pub actor: ActorId,
    /// The damage-graph node a contact with the box damages.
    pub node: DamageNodeKey,
    /// The box's half extents, in meters. Each must be finite and
    /// non-negative; [`part_sweep_candidates`] refuses the rest by name.
    pub half_extents_m: [f64; 3],
}

impl PartSweptBox {
    /// Binds one part's box geometry and damage node to its actor.
    #[must_use]
    pub const fn new(actor: ActorId, node: DamageNodeKey, half_extents_m: [f64; 3]) -> Self {
        Self {
            actor,
            node,
            half_extents_m,
        }
    }
}

/// Why one live part could not become a [`SweepCandidate`].
#[derive(Clone, Debug, PartialEq)]
pub enum PartSweepRefusal {
    /// The part entity carries no [`NodeVisualTransform`], so its live centre
    /// is unknown.
    MissingPose {
        /// The part's damage node.
        node: DamageNodeKey,
    },
    /// The part's composed translation was not finite.
    NonFinitePose {
        /// The part's damage node.
        node: DamageNodeKey,
    },
    /// The swept box was refused by the shared geometry vocabulary (a
    /// non-finite or negative half extent, or a non-finite centre).
    Target {
        /// The part's damage node.
        node: DamageNodeKey,
        /// Why the box was refused.
        source: SweepTargetError,
    },
    /// No ancestor of the part (up to the hierarchy root) carries a live
    /// `LinearVelocity`, so the target's motion through the tick is unknown.
    ///
    /// The relative motion is an input to the sweep (F27 non-negotiable 3), so
    /// an unmeasurable one is refused by name rather than assumed to be zero:
    /// a still target and an unreadable one are different statements, and the
    /// latter must not be silently treated as the former.
    MissingAirframeVelocity {
        /// The part's damage node.
        node: DamageNodeKey,
    },
}

impl PartSweepRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::MissingPose { .. } => "missing_pose",
            Self::NonFinitePose { .. } => "non_finite_pose",
            Self::Target { .. } => "target",
            Self::MissingAirframeVelocity { .. } => "missing_airframe_velocity",
        }
    }

    /// The part this refusal is about.
    #[must_use]
    pub const fn node(&self) -> &DamageNodeKey {
        match self {
            Self::MissingPose { node }
            | Self::NonFinitePose { node }
            | Self::Target { node, .. }
            | Self::MissingAirframeVelocity { node } => node,
        }
    }
}

/// The live part candidates read for a swept query, and the parts that could
/// not be read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartSweepCandidates {
    /// The part boxes the swept query may cross, in ascending entity order.
    pub candidates: Vec<SweepCandidate>,
    /// The parts whose geometry could not be read, by name.
    pub refused: Vec<PartSweepRefusal>,
}

impl PartSweepCandidates {
    /// Whether no part could be read.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
}

/// Reads the live part boxes of a world into [`SweepCandidate`]s.
///
/// Each entity carrying a [`PartSweptBox`] contributes one candidate:
///
/// * its **centre** is the part's composed [`NodeVisualTransform`] translation
///   at the end of the tick;
/// * its **previous centre** is the same position less the airframe body's
///   live `LinearVelocity` over `dt_s`, so a round sweeping a moving target
///   crosses the box where it actually was (F27 non-negotiable 3). The
///   original's per-part motion (in particular any angular contribution) is
///   unmeasured and is *not* applied: the finding records the omission rather
///   than inventing a rule.
/// * its **relation** comes from `relation(target_actor)`, so the declared
///   allegiance vocabulary decides admission and an undeclared pair stays
///   `None` (the exact statement `FriendlyFireRule` needs).
///
/// One read serves one tick, while the admission decision belongs to a
/// `(shooter, target)` *pair*: a caller routing the rounds of several shooters
/// from this single read should therefore pass a relation it can ignore here and
/// bind the pair per round, as [`step_weapon_session`] does. A lookup that
/// cannot see the firing actor would decide one shooter's rounds — and the
/// player's own — under somebody else's allegiance.
///
/// `dt_s` is the tick length the previous centre is reconstructed over. A
/// part with no pose, a non-finite pose, no reachable airframe velocity, or a
/// box the shared geometry vocabulary refuses is named in
/// [`PartSweepCandidates::refused`]; one bad part never drops the others,
/// because a refused contact is a reported defect in one candidate, not a
/// licence to lose the round.
#[must_use]
pub fn part_sweep_candidates(
    world: &World,
    dt_s: f64,
    relation: impl Fn(ActorId) -> Option<Allegiance>,
) -> PartSweepCandidates {
    let mut read = PartSweepCandidates::default();
    for entity_ref in world.iter_entities() {
        let entity = entity_ref.id();
        let Some(box_record) = entity_ref.get::<PartSweptBox>() else {
            continue;
        };
        let node = box_record.node.clone();
        let Some(visual) = entity_ref.get::<NodeVisualTransform>() else {
            read.refused.push(PartSweepRefusal::MissingPose { node });
            continue;
        };

        let current = visual.global().translation();
        let current = [
            f64::from(current.x),
            f64::from(current.y),
            f64::from(current.z),
        ];
        if !current.iter().all(|value| value.is_finite()) {
            read.refused.push(PartSweepRefusal::NonFinitePose { node });
            continue;
        }
        let Some(velocity) = airframe_velocity(world, entity) else {
            read.refused
                .push(PartSweepRefusal::MissingAirframeVelocity { node });
            continue;
        };
        let previous = [
            current[0] - velocity[0] * dt_s,
            current[1] - velocity[1] * dt_s,
            current[2] - velocity[2] * dt_s,
        ];

        match SweepTarget::try_new(
            box_record.actor,
            previous,
            current,
            box_record.half_extents_m,
        ) {
            Ok(target) => read.candidates.push(SweepCandidate::new(
                target,
                node,
                relation(box_record.actor),
            )),
            Err(source) => read.refused.push(PartSweepRefusal::Target { node, source }),
        }
    }
    read
}

// ---------------------------------------------- the swept-hit → damage seam ---
//
// F27-C. The gun half (`cs_sim::weapons::guns`) owns the *shape* of a swept
// hit: [`GunHitRouter::route`] filters candidates by the declared rules,
// sweeps and converts each contact into one `HitEvent` per non-zero declared
// damage channel. It deliberately does not apply anything — `cs_sim::damage`
// is the only authority that may turn a hit into destroyed structure.
//
// This function is the production caller that closes the loop: it takes the
// routed hits and resolves them through the session's [`DamageResolver`].
// Nothing else in the codebase consumes a `RoutedHit`, so without this the
// sweep would compute a hit nobody could act on.
//
// # Ordering and failure
//
// The routing runs first and produces the whole batch, which is then handed
// to the resolver in one call. Routing first means the resolver sees a
// *complete* tick's damage from this shot, so its same-tick lethal and
// attribution rules apply across the whole batch rather than per shot.
//
// The resolver's error is returned, not swallowed: a `ForeignSession` or a
// `DuplicateHit` means the batch was wrong before anything was applied, and
// the routing outcome is still returned alongside so the caller can see which
// contacts produced it. A routing refusal does **not** stop the batch: the
// other contacts' damage is still the gun's declared damage and is still
// applied, because a refused contact is a reported defect in one candidate,
// not a licence to drop the rest of the round's damage.
//
// # What this does not do
//
// It does not select the bank, play the sound, spawn the muzzle effect or
// move a mount's transform: the accepted fire event already carries the sound
// and effect ids (F27-A) and F27-B owns the hierarchy walk. It does not decide
// eligibility either — the lowered [`WeaponRules`] do, by declaration.

/// What one shot's swept hits produced: the routing outcome and the damage
/// resolution, or the reason the damage could not be resolved.
///
/// Both are returned together, always: a caller must be able to see *which
/// contacts produced damage* and *which damage was refused* in the same pass,
/// so a lost hit is never invisible behind a successful resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct SweptDamageOutcome {
    /// The routing: which candidates were admitted, which hits they produced
    /// and which contacts were refused by name.
    pub sweep: SweepOutcome,
    /// The resolver's output for the routed hits.
    ///
    /// A round that crossed nothing routed no hits, and an empty batch is what
    /// the authority answers with: an empty resolution for `at`, with no state
    /// advanced. A caller therefore reads [`SweptDamageOutcome::sweep`] to tell
    /// a miss or a refused contact from damage that was applied, never the
    /// emptiness of this field.
    pub damage: Result<TickResolution, DamageError>,
}

impl SweptDamageOutcome {
    /// Whether the shot's contacts produced no hit at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sweep.is_empty()
    }

    /// The contacts the routing refused, by name.
    #[must_use]
    pub fn refused_contacts(&self) -> &[SweepRefusal] {
        &self.sweep.refused
    }
}

/// Runs one accepted shot's swept contacts through the declared rules, the
/// swept geometry and the damage authority.
///
/// `candidates` are the world candidates the collision features reported for
/// this projectile's segment; each names the damage node a contact with it
/// lands on. `at` is the tick being resolved, and it is what the resulting
/// hits are stamped with — a round fired on one tick lands on another.
///
/// The damage resolver is the session's authority: nothing here bypasses it,
/// and the gun's declared per-channel damage amounts reach the graph only
/// through the [`cs_sim::damage::HitEvent`]s it resolves.
///
/// # Why this returns an outcome rather than a `Result`
///
/// The resolver's own error — a foreign session, a duplicate hit id — is
/// carried inside [`SweptDamageOutcome::damage`] rather than as this
/// function's `Err`, because the routing outcome is still meaningful
/// alongside it: dropping it would hide which contacts produced the batch
/// that could not be applied. A caller that only wants the damage reads
/// `outcome.damage`; a caller auditing the routing reads `outcome.sweep`.
#[must_use]
pub fn resolve_swept_damage(
    router: &mut GunHitRouter,
    rules: &WeaponRules,
    damage: &mut DamageResolver,
    shot: &FireEvent,
    segment: &ProjectileSegment,
    candidates: impl IntoIterator<Item = SweepCandidate>,
    at: Tick,
) -> SweptDamageOutcome {
    let sweep = router.route(shot, segment, candidates, rules, at);
    // The whole batch goes to the authority in one call, empty or not:
    // `DamageResolver::resolve` answers an empty batch with an empty
    // `TickResolution` for `at` and changes nothing, so a round that crossed
    // nothing needs no second code path here and this stays a single call site
    // through which a gun's declared damage reaches the authority.
    let resolved = damage.resolve(at, &sweep.damage());
    SweptDamageOutcome {
        sweep,
        damage: resolved,
    }
}

// ------------------------------------------- the weapon session and its step ---
//
// F27-C's application half. F27-A owned the resolver, F27-B the cadence and the
// two live-ECS producers, and #443 the swept-hit → `HitEvent` seam
// (`docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`).
// What none of them owns is the *sequence*: no production code advanced the
// cooldowns and the live rounds together, carried an accepted shot forward so a
// round's later ticks could be routed, derived a sound or a muzzle effect from
// an accepted event, applied a bank selection, or made a round visible to the
// ECS.
//
// This section is that sequence. [`WeaponSession`] is the authority one session
// generation holds over it — the cadence (which owns the resolver and the live
// rounds), the router, the per-mount interaction rules, the accepted-shot
// records and the effect log — and [`step_weapon_session`] is the one function
// that runs a tick's commands through the whole path.
//
// Three ownership rules decide what this section is allowed to do:
//
// * **The cadence is the only thing that consumes a round**, starts a cooldown
//   or spawns a projectile. The step feeds it live mount poses and reports what
//   it answered; it never touches the weapon state itself, which is why a
//   denied input cannot drain ammunition (non-negotiable 5).
// * **The damage authority is borrowed, not owned.** `DamageResolver` belongs
//   to F29 and is shared with collision, crash and script producers, so the
//   step takes it as a caller-supplied authority and applies nothing outside it.
// * **The ECS mirror is a mirror.** Its `Transform` is written from the
//   authoritative [`cs_sim::weapons::LiveProjectile`] every tick and it carries
//   no collider, so Avian never integrates a round and there is no second
//   contact authority (FLIGHT-PHYSICS, "one physics pose owner").
//
// [`cs_sim::weapons::LiveProjectile`]: cs_sim::weapons::LiveProjectile

/// Component: the ECS face of one live authoritative round.
///
/// The entity exists so rendering, effects and the HUD have something to read;
/// it is not the round. Its [`Transform`] is **written** by
/// [`sync_round_mirrors`] from the authoritative
/// [`cs_sim::weapons::LiveProjectile`], never integrated, and it carries no
/// collider. The fields are exactly what the authoritative round names —
/// the round's id, who fired it, and the scene generation the mirror was
/// stamped under, so a reloaded scene's orphan is identifiable by mismatch
/// rather than by a surviving pointer (the same rule
/// [`crate::scene::SceneNodeBinding`] and [`crate::damage::DamageActorBinding`]
/// follow). Which mount fired is a property of the *shot*, and it reaches
/// consumers as the [`WeaponEffect`] the accepted event produced.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct WeaponRoundMirror {
    /// The authoritative round this entity mirrors.
    pub projectile: ProjectileId,
    /// Who fired the round.
    pub shooter: ActorId,
    /// The scene generation the mirror was stamped under.
    pub generation: SceneGeneration,
}

/// One accepted shot's effects, derived from an accepted [`FireEvent`] alone.
///
/// This is the muzzle-effect and sound record the presentation side consumes.
/// It exists **only** for a shot that happened: a denied mount, a refused whole
/// intent and a duplicate network packet produce none of these, which is
/// non-negotiable 5's rule ("consumption, sound and muzzle effects derive from
/// accepted fire events") made observable. The catalog ids are the gun
/// definition's own [`cs_sim::weapons::GunDefinition::effect`] and
/// [`cs_sim::weapons::GunDefinition::sound`] carried through unchanged — this
/// stage names them, it does not load, pick or play them.
///
/// [`cs_sim::weapons::GunDefinition::effect`]: cs_sim::weapons::GunDefinition::effect
/// [`cs_sim::weapons::GunDefinition::sound`]: cs_sim::weapons::GunDefinition::sound
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponEffect {
    /// The round this effect belongs to.
    pub projectile: ProjectileId,
    /// Who fired it.
    pub shooter: ActorId,
    /// Which mount fired it.
    pub mount: DamageNodeKey,
    /// The gun's declared muzzle-effect catalog id.
    pub effect: ContentId,
    /// The gun's declared shot-sound catalog id.
    pub sound: ContentId,
    /// The muzzle's world position at the tick the shot was accepted on.
    pub origin: WorldPosition,
    /// The tick the shot was accepted on.
    pub at: Tick,
}

impl WeaponEffect {
    /// The effect one accepted shot produced, on the tick it was accepted.
    ///
    /// Everything here is read off the [`FireEvent`], so an effect cannot name
    /// a resource the gun did not declare nor a position the muzzle did not
    /// have. There is no other constructor: this is what "derived from an
    /// accepted fire event" means in code.
    #[must_use]
    pub fn of(shot: &FireEvent, at: Tick) -> Self {
        Self {
            projectile: shot.projectile.projectile,
            shooter: shot.shooter,
            mount: shot.mount.clone(),
            effect: shot.effect.clone(),
            sound: shot.sound.clone(),
            origin: shot.projectile.origin,
            at,
        }
    }
}

/// The effects one session has emitted, drained by the presentation side.
///
/// The log is the *only* effect source the step writes, so a consumer that
/// drains it sees every accepted shot exactly once and can tell an effect that
/// was emitted from one that was never produced.
///
/// It holds the **same records** as [`WeaponTick::effects`] — one
/// [`WeaponEffect`] per accepted shot, recorded once and reported once. A
/// consumer plays one view or the other (drain the log for a session-wide
/// stream, read the per-tick report to stay in step with a tick), never both:
/// playing both would sound every shot twice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponEffectLog {
    effects: Vec<WeaponEffect>,
}

impl WeaponEffectLog {
    /// Records one accepted shot's effect.
    fn record(&mut self, effect: WeaponEffect) {
        self.effects.push(effect);
    }

    /// The effects emitted so far, oldest first.
    #[must_use]
    pub fn effects(&self) -> &[WeaponEffect] {
        &self.effects
    }

    /// Takes every effect emitted so far, leaving the log empty.
    #[must_use]
    pub fn drain(&mut self) -> Vec<WeaponEffect> {
        std::mem::take(&mut self.effects)
    }

    /// How many effects are waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether no effect is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

/// One authoritative weapon command for a tick.
///
/// Orders are the *input* boundary: the cockpit control that sends them is
/// F46's and the original's bank names and cycle order are unmeasured
/// (F27-D), so a bank is the set of mounts it names and nothing more. They are
/// applied in the order they are given, so a caller that wants a switch to
/// apply to this tick's shot puts it first.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponOrder {
    /// Switch the named actor's selected gun bank.
    ///
    /// The switch reaches [`WeaponState::select`], which owns the selected set
    /// alone: it cannot refill a magazine, restart a cooldown or re-enable a
    /// mount, because it never touches those tables (AC03).
    SelectBank {
        /// The actor whose selection changes.
        shooter: ActorId,
        /// The mounts to select.
        bank: GunBank,
    },
    /// Ask the named actor's selected bank to fire on this tick.
    ///
    /// The intent's id is the once-only key, so a replayed packet consumes
    /// nothing. Through this step a replay is refused by the *tick* it names
    /// first — the step only ever resolves `step.at`, so a packet carrying the
    /// tick it was first sent on is refused as a foreign tick — and by the
    /// once-only id itself when it names the tick being resolved. Either way it
    /// fires nothing, spends nothing and emits no effect.
    Fire(FireIntent),
}

/// Why one order was refused, changing no state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderRefusal {
    /// The session has been closed by teardown.
    Closed,
    /// The order names a session generation other than the session's.
    ForeignSession {
        /// The session's generation.
        expected: u64,
        /// The generation the order carried.
        found: u64,
    },
    /// The selection names an actor this session has no registered guns for.
    UnknownShooter {
        /// The actor that was named.
        shooter: ActorId,
    },
}

impl OrderRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::ForeignSession { .. } => "foreign_session",
            Self::UnknownShooter { .. } => "unknown_shooter",
        }
    }
}

impl std::fmt::Display for OrderRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "the weapon session is closed"),
            Self::ForeignSession { expected, found } => {
                write!(
                    f,
                    "the order is for session {found}, but this session is {expected}"
                )
            }
            Self::UnknownShooter { shooter } => {
                write!(f, "{shooter} has no guns registered in this session")
            }
        }
    }
}

impl std::error::Error for OrderRefusal {}

/// Why a whole tick was refused before any order was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepRefusal {
    /// The session has been closed by teardown.
    Closed,
    /// The tick is not strictly after the last tick this session resolved.
    ///
    /// A step consumes its tick: cooldowns walked, rounds moved and, where an
    /// order fired, ammunition spent. Re-running the same tick would do that
    /// work twice, so the repeat is refused rather than made idempotent — and
    /// because the refusal is the honest statement (the first pass really did
    /// happen), a caller that lost a step's result cannot use the retry to undo
    /// it either.
    StaleTick {
        /// The last tick the session resolved.
        resolved_through: Tick,
        /// The tick the step was asked for.
        at: Tick,
    },
}

impl StepRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::StaleTick { .. } => "stale_tick",
        }
    }
}

impl std::fmt::Display for StepRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "the weapon session is closed"),
            Self::StaleTick {
                resolved_through,
                at,
            } => {
                write!(f, "tick {at:?} was refused after tick {resolved_through:?}")
            }
        }
    }
}

impl std::error::Error for StepRefusal {}

/// Why a live round's segment could not be routed.
///
/// Both arms are structurally prevented by this session's own API — the record
/// is stored when the shot is accepted and released only with the round — and
/// both exist so a round that reached the runtime by some *other* route is
/// reported by name instead of being swept with a guessed attacker or a
/// default hostility test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutingRefusal {
    /// The session holds a live round whose accepted shot it does not know.
    ///
    /// Routing needs the shot for the attacker, the projectile identity and the
    /// declared damage profile, so a round whose shot was lost is reported here
    /// rather than swept with a guessed profile.
    UnknownRound {
        /// The round whose shot is missing.
        projectile: ProjectileId,
    },
    /// An accepted shot's gun has no lowered interaction rules, so the round it
    /// spawned cannot be admitted against anything.
    MissingInteractionRules {
        /// The round that cannot be routed.
        projectile: ProjectileId,
        /// The mount whose rules are missing.
        mount: DamageNodeKey,
    },
}

impl RoutingRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::UnknownRound { .. } => "unknown_round",
            Self::MissingInteractionRules { .. } => "missing_interaction_rules",
        }
    }
}

impl std::fmt::Display for RoutingRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRound { projectile } => {
                write!(f, "round {projectile} has no accepted shot to route with")
            }
            Self::MissingInteractionRules { projectile, mount } => {
                write!(
                    f,
                    "round {projectile} has no interaction rules for mount {mount}"
                )
            }
        }
    }
}

impl std::error::Error for RoutingRefusal {}

/// One live round's routing inputs: the accepted shot it is routed with and the
/// declared interaction rules of the gun that fired it.
///
/// The two travel together because they must: a round that outlives the tick it
/// was fired on can only be routed with the shot that spawned it, and it can
/// only be admitted against the rules that shot's gun declared.
#[derive(Clone, Debug, PartialEq)]
struct RoundRecord {
    shot: FireEvent,
    rules: WeaponRules,
}

/// One mounted gun the session registered, in the runtime form the ECS binds.
///
/// Registration returns these because the scene wiring needs the *lowered*
/// mount key, mount kind and the two catalog ids an accepted shot will name —
/// the declared records are not reachable from the cadence, and re-deriving
/// them from the mount key would be a second lowering.
#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredWeapon {
    /// The lowered damage-node key the gun is mounted on.
    pub mount: DamageNodeKey,
    /// Where the mount sits on the airframe.
    pub kind: GunMountKind,
    /// The gun's declared muzzle-effect catalog id.
    pub effect: ContentId,
    /// The gun's declared shot-sound catalog id.
    pub sound: ContentId,
}

/// Why a declared loadout could not be registered.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponRegistrationError {
    /// The session has been closed by teardown.
    Closed,
    /// The actor belongs to another session generation.
    ForeignSession {
        /// The session's generation.
        expected: u64,
        /// The generation the actor carried.
        found: u64,
    },
    /// A declared gun or its declared rules could not be lowered: an unknown
    /// field refuses rather than inventing a ballistic parameter or an
    /// interaction rule.
    Lower(WeaponLowerError),
    /// The weapon state the session would register was refused, for example a
    /// selected bank naming a mount the loadout does not carry.
    State(GunStateError),
    /// The cadence refused the registration itself.
    Fire(FireError),
}

impl std::fmt::Display for WeaponRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "the weapon session is closed"),
            Self::ForeignSession { expected, found } => {
                write!(
                    f,
                    "the actor belongs to session {found}, but this session is {expected}"
                )
            }
            Self::Lower(source) => write!(f, "the declared gun could not be lowered: {source}"),
            Self::State(source) => write!(f, "the weapon state was refused: {source}"),
            Self::Fire(source) => write!(f, "the cadence refused the registration: {source}"),
        }
    }
}

impl std::error::Error for WeaponRegistrationError {}

/// Why a session could not be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionRefusal {
    /// Session generation zero cannot be a weapon session.
    ///
    /// Zero *is* "no session" in [`cs_types::net::SessionId`] and no
    /// [`ActorId`] can be built for it, so a session-0 weapon session could
    /// never register a shooter and would only exist to refuse. It is refused
    /// at the door instead.
    NoSession,
}

impl std::fmt::Display for SessionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSession => write!(f, "session generation zero is not a weapon session"),
        }
    }
}

impl std::error::Error for SessionRefusal {}

/// One session generation's authority over the weapon path.
///
/// The session owns the [`GunCadence`] (which in turn owns the
/// [`cs_sim::weapons::FireResolver`] and the live rounds), the
/// [`GunHitRouter`] and its once-per-`(projectile, actor)` ledger, the
/// lowered per-mount [`WeaponRules`], the accepted [`FireEvent`] every live
/// round is routed with, and the effect log. It does **not** own the
/// [`DamageResolver`]: that authority belongs to F29 and is shared with every
/// other damage producer, so [`step_weapon_session`] takes it from its caller.
///
/// The accepted-shot records are the reason a round can be routed at all: a
/// round is fired on one tick and lands on another, and the router needs the
/// shooter, the projectile identity and the gun's declared damage profile — all
/// of which live on the shot, not on the moving round.
///
/// Dropping the session is the teardown; [`WeaponSession::close`] is the
/// explicit one, and it releases every live round and every mirror with it.
#[derive(Clone, Debug)]
pub struct WeaponSession {
    session: u64,
    cadence: GunCadence,
    router: GunHitRouter,
    rules: BTreeMap<ActorId, BTreeMap<DamageNodeKey, WeaponRules>>,
    rounds: BTreeMap<ProjectileId, RoundRecord>,
    effects: WeaponEffectLog,
    resolved_through: Option<Tick>,
    closed: bool,
}

impl WeaponSession {
    /// Opens a weapon session for one session generation, positioned at `tick`.
    ///
    /// `router_producer` is the serial the routed [`HitEvent`]s are stamped
    /// with; the session's schedule allocates one serial for this stage, exactly
    /// as it does for `DamageResolver::new`'s producer.
    ///
    /// # Errors
    ///
    /// [`SessionRefusal::NoSession`] for session generation zero.
    pub fn new(session: u64, tick: Tick, router_producer: u32) -> Result<Self, SessionRefusal> {
        if session == 0 {
            return Err(SessionRefusal::NoSession);
        }
        Ok(Self {
            session,
            cadence: GunCadence::new(session, tick),
            router: GunHitRouter::new(session, router_producer),
            rules: BTreeMap::new(),
            rounds: BTreeMap::new(),
            effects: WeaponEffectLog::default(),
            resolved_through: None,
            closed: false,
        })
    }

    /// The session generation this session is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The tick the cadence has advanced to.
    #[must_use]
    pub fn tick(&self) -> Tick {
        self.cadence.tick()
    }

    /// The cadence: the authority over weapon state and the live rounds.
    #[must_use]
    pub const fn cadence(&self) -> &GunCadence {
        &self.cadence
    }

    /// The hit router and its once-per-`(projectile, actor)` ledger.
    #[must_use]
    pub const fn router(&self) -> &GunHitRouter {
        &self.router
    }

    /// Whether teardown has closed this session.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// One actor's live weapon state, if it has registered guns.
    ///
    /// The damage consumer's `SystemDisabled` transition reaches the weapon
    /// gate through [`WeaponSession::state_mut`]: disabling a mount under the
    /// gun's own [`DamageNodeKey`] is what makes a destroyed wing gun stop
    /// firing.
    ///
    /// A closed session holds no live weapon state: `None` after teardown, so a
    /// consumer cannot read the ammunition of a session that no longer exists
    /// (see [`WeaponSession::close`]).
    #[must_use]
    pub fn state(&self, shooter: &ActorId) -> Option<&WeaponState> {
        if self.closed {
            return None;
        }
        self.cadence.state(shooter)
    }

    /// One actor's mutable live weapon state, if it has registered guns.
    ///
    /// An unregistered actor is `None`, never a panic: a damage event naming an
    /// actor this session never armed has nothing to disable. A closed session
    /// is `None` for the same reason — nothing may mutate a torn-down session's
    /// mounts, so a late repair event cannot re-enable a gun whose session is
    /// gone.
    pub fn state_mut(&mut self, shooter: &ActorId) -> Option<&mut WeaponState> {
        if self.closed {
            return None;
        }
        self.cadence.state_mut(shooter)
    }

    /// The effects emitted so far, oldest first.
    #[must_use]
    pub fn effects(&self) -> &[WeaponEffect] {
        self.effects.effects()
    }

    /// Takes every effect emitted so far, leaving the log empty.
    #[must_use]
    pub fn drain_effects(&mut self) -> Vec<WeaponEffect> {
        self.effects.drain()
    }

    /// The live round ids, in ascending serial order.
    #[must_use]
    pub fn round_ids(&self) -> Vec<ProjectileId> {
        self.cadence
            .projectiles()
            .iter()
            .map(|live| live.projectile())
            .collect()
    }

    /// The accepted shot a live round is routed with, if the session holds it.
    #[must_use]
    pub fn shot(&self, projectile: &ProjectileId) -> Option<&FireEvent> {
        self.rounds.get(projectile).map(|record| &record.shot)
    }

    /// The declared interaction rules a live round is admitted against.
    #[must_use]
    pub fn round_rules(&self, projectile: &ProjectileId) -> Option<&WeaponRules> {
        self.rounds.get(projectile).map(|record| &record.rules)
    }

    /// The whole routing record of one live round, for the step that routes it.
    fn round_record(&self, projectile: &ProjectileId) -> Option<&RoundRecord> {
        self.rounds.get(projectile)
    }

    /// The lowered interaction rules one mounted gun runs under.
    #[must_use]
    pub fn rules(&self, shooter: &ActorId, mount: &DamageNodeKey) -> Option<&WeaponRules> {
        self.rules.get(shooter)?.get(mount)
    }

    /// Registers one actor's declared loadout.
    ///
    /// This is the stage's *producer* boundary: the guns arrive as the declared
    /// records `cs_content::weapons` describes, each one is lowered here (so an
    /// unknown ballistic field or an unmeasured interaction rule refuses by name
    /// instead of being invented), the weapon state is built from the lowered
    /// definitions, and the per-mount rules are kept beside them so a round can
    /// be routed for its whole life.
    ///
    /// The returned [`RegisteredWeapon`]s are what the scene wiring binds: the
    /// lowered mount key and the two catalog ids an accepted shot will name.
    ///
    /// # Errors
    ///
    /// [`WeaponRegistrationError`] for a closed session, an actor from another
    /// generation, a gun whose declared record refuses to lower, a state the
    /// selected bank does not fit, or a registration the cadence refused.
    pub fn register(
        &mut self,
        shooter: ActorId,
        guns: &[DeclaredGunDefinition],
        bank: GunBank,
        starting_rounds: u64,
    ) -> Result<Vec<RegisteredWeapon>, WeaponRegistrationError> {
        if self.closed {
            return Err(WeaponRegistrationError::Closed);
        }
        if shooter.session.get() != self.session {
            return Err(WeaponRegistrationError::ForeignSession {
                expected: self.session,
                found: shooter.session.get(),
            });
        }

        let mut lowered = Vec::with_capacity(guns.len());
        let mut rules = BTreeMap::new();
        let mut registered = Vec::with_capacity(guns.len());
        for declared in guns {
            let gun = lower_gun(declared).map_err(WeaponRegistrationError::Lower)?;
            let interaction =
                lower_rules(declared.rules()).map_err(WeaponRegistrationError::Lower)?;
            let mount = gun.mount().clone();
            rules.insert(mount.clone(), interaction);
            registered.push(RegisteredWeapon {
                mount: mount.clone(),
                kind: gun.kind(),
                effect: gun.effect().clone(),
                sound: gun.sound().clone(),
            });
            lowered.push(gun);
        }

        let state = WeaponState::try_new(&lowered, bank, starting_rounds)
            .map_err(WeaponRegistrationError::State)?;
        self.cadence
            .register(shooter, lowered, state)
            .map_err(WeaponRegistrationError::Fire)?;
        self.rules.insert(shooter, rules);
        Ok(registered)
    }

    /// Switches one actor's selected gun bank.
    ///
    /// The switch reaches [`WeaponState::select`] and nothing else, which is
    /// what makes AC03 hold by construction: no ammunition is added back and no
    /// cooldown is cleared, so the switched-to mount still serves the cooldown
    /// it was serving.
    ///
    /// # Errors
    ///
    /// [`OrderRefusal`] for a closed session, an actor from another generation
    /// or a shooter with no registered guns.
    pub fn select(&mut self, shooter: ActorId, bank: GunBank) -> Result<(), OrderRefusal> {
        if self.closed {
            return Err(OrderRefusal::Closed);
        }
        if shooter.session.get() != self.session {
            return Err(OrderRefusal::ForeignSession {
                expected: self.session,
                found: shooter.session.get(),
            });
        }
        let Some(state) = self.cadence.state_mut(&shooter) else {
            return Err(OrderRefusal::UnknownShooter { shooter });
        };
        state.select(bank);
        Ok(())
    }

    /// Records one accepted shot: the round's own routing record for the rest
    /// of its life, and the effect a consumer plays.
    fn accept(&mut self, event: &FireEvent, at: Tick) -> Result<WeaponEffect, RoutingRefusal> {
        let projectile = event.projectile.projectile;
        let effect = WeaponEffect::of(event, at);
        // The rules are the gun's, and the gun was registered, so this cannot
        // be missing for a shot the session itself accepted; the record keeps
        // them beside the shot so routing never looks them up again.
        let Some(rules) = self
            .rules
            .get(&event.shooter)
            .and_then(|by_mount| by_mount.get(&event.mount))
            .cloned()
        else {
            return Err(RoutingRefusal::MissingInteractionRules {
                projectile,
                mount: event.mount.clone(),
            });
        };
        self.rounds.insert(
            projectile,
            RoundRecord {
                shot: event.clone(),
                rules,
            },
        );
        self.effects.record(effect.clone());
        Ok(effect)
    }

    /// Removes one live round, as a scripted despawn does, and releases its
    /// routing record with it.
    ///
    /// A round is **not** removed because it hit something: that would be a
    /// penetration rule, and `WeaponRules::penetration` and `ricochet` are
    /// declared, unapplied and deferred to F27-D. The once-per-`(projectile,
    /// actor)` ledger, not a despawn, is what stops a round from applying its
    /// damage twice.
    pub fn remove_round(&mut self, projectile: ProjectileId) -> Option<LiveProjectile> {
        let removed = self.cadence.remove_projectile(projectile);
        self.rounds.remove(&projectile);
        removed
    }

    /// Releases one *retired* round's routing record, leaving the round's own
    /// retirement to the runtime that already removed it.
    ///
    /// The runtime retires a spent round itself, after its last segment has been
    /// swept, so the record is all that is left to release here.
    fn forget(&mut self, projectile: &ProjectileId) -> Option<RoundRecord> {
        self.rounds.remove(projectile)
    }

    /// Opens a tick, refusing a closed session or a tick that is not after the
    /// last one resolved.
    fn begin_tick(&mut self, at: Tick) -> Option<StepRefusal> {
        if self.closed {
            return Some(StepRefusal::Closed);
        }
        if let Some(resolved_through) = self.resolved_through
            && at <= resolved_through
        {
            return Some(StepRefusal::StaleTick {
                resolved_through,
                at,
            });
        }
        self.resolved_through = Some(at);
        None
    }

    /// Ends the session: every live round is released and every mirror is
    /// despawned, and no further order, step or registration is accepted.
    ///
    /// What survives inside the cadence — the cooldown and ammunition tables of
    /// the registered actors — becomes unreachable: [`Self::state`] and
    /// [`Self::state_mut`] answer `None` once the session is closed and
    /// [`Self::register`] refuses, so a restart must build a *new* session
    /// rather than re-arm this one, and dropping the session frees the tables.
    /// That is the session-generation discipline `crate::targeting` and
    /// `crate::scene` follow for their own authorities.
    pub fn close(&mut self, world: &mut World) -> TeardownReport {
        self.closed = true;
        let mut rounds = self.round_ids();
        for projectile in &rounds {
            self.cadence.remove_projectile(*projectile);
        }
        rounds.sort_unstable();
        self.rounds.clear();
        let despawned = despawn_round_mirrors(world);
        TeardownReport {
            rounds,
            mirrors: despawned,
        }
    }
}

/// What one session teardown released.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TeardownReport {
    /// The live rounds the session released, in ascending serial order.
    pub rounds: Vec<ProjectileId>,
    /// How many ECS mirrors were despawned with them.
    pub mirrors: usize,
}

/// One mount that did not fire, with the reason the resolver gave.
#[derive(Clone, Debug, PartialEq)]
pub struct DeniedShot {
    /// The intent the mount answered.
    pub intent: cs_sim::weapons::FireIntentId,
    /// Why the mount did not fire.
    pub reason: FireDenialReason,
}

/// One live mount pose the hierarchy walk could not read.
#[derive(Clone, Debug, PartialEq)]
pub struct UnreadableMount {
    /// The actor whose mount was unreadable.
    pub shooter: ActorId,
    /// Why the pose could not be read.
    pub refusal: MountPoseRefusal,
}

/// One live round's routing and damage outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutedRound {
    /// The round that was swept.
    pub projectile: ProjectileId,
    /// The routing outcome and the damage resolution for its segment.
    pub outcome: SweptDamageOutcome,
}

/// What the ECS mirror pass changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MirrorReport {
    /// Mirrors spawned for rounds that had none.
    pub spawned: Vec<ProjectileId>,
    /// Mirrors despawned because no live round backs them.
    pub despawned: Vec<ProjectileId>,
    /// Mirrors written onto this tick's authoritative position.
    pub moved: usize,
}

/// The world inputs one weapon step needs.
pub struct WeaponStep<'a> {
    /// The tick being resolved.
    pub at: Tick,
    /// The tick's length in seconds, used to move the rounds and to
    /// reconstruct each part's previous centre.
    pub dt_s: f64,
    /// The session's authoritative wind velocity in world m/s.
    pub wind_velocity_m_s: [f64; 3],
    /// The scene generation live entities carry, which is what makes a
    /// reloaded hierarchy's stale poses identifiable.
    pub generation: SceneGeneration,
    /// The declared relation of a *shooting* actor to a candidate actor, in the
    /// F30-A [`Allegiance`] vocabulary. `None` is an *undeclared* pair, which
    /// is not the same statement as friendly: only
    /// [`FriendlyFireRule::Everyone`] admits it. The session's targeting
    /// authority supplies this; the weapon path never re-derives hostility.
    ///
    /// It takes **both** actors because one tick routes the rounds of every
    /// shooter in the session, and a relation belongs to a pair: the same
    /// aircraft is an ally of one shooter and an enemy of another, so a lookup
    /// that could not see the shooter would decide a wingman's rounds — and the
    /// player's own — under somebody else's allegiance. The step binds this per
    /// round, against the shot's own shooter.
    pub relation: &'a dyn Fn(ActorId, ActorId) -> Option<Allegiance>,
}

/// One tick's whole weapon result.
///
/// Every list is a report of something that happened, not a summary of what
/// should have: an absent entry is never "swallowed", it is either a miss, a
/// denial or a refusal, and each of those has its own list.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WeaponTick {
    /// The tick that was resolved.
    pub at: Tick,
    /// The step was refused whole: nothing advanced and no order was read.
    pub refused: Option<StepRefusal>,
    /// Orders refused on their own, each changing nothing.
    pub orders_refused: Vec<OrderRefusal>,
    /// The shots that happened, in the order the orders resolved them.
    pub accepted: Vec<FireEvent>,
    /// Mounts that did not fire, with the resolver's reason.
    pub denied: Vec<DeniedShot>,
    /// Whole intents the cadence refused, changing no state.
    pub fire_refused: Vec<CadenceRefusal>,
    /// Live mount poses the hierarchy walk could not read.
    pub unreadable_mounts: Vec<UnreadableMount>,
    /// Live part boxes the swept query could not read.
    ///
    /// The world read that produces this exists to feed the sweep, so a tick
    /// with no live round reports nothing here rather than walking the part
    /// boxes for a query it will not run.
    pub unreadable_parts: Vec<PartSweepRefusal>,
    /// The effects the accepted shots produced.
    pub effects: Vec<WeaponEffect>,
    /// One entry per live round whose segment was routed and resolved.
    pub routed: Vec<RoutedRound>,
    /// Live rounds whose damage could not even be routed.
    pub routing_refused: Vec<RoutingRefusal>,
    /// The round advance refused the whole tick.
    ///
    /// The advance is all-or-nothing inside the runtime, so this means no round
    /// moved and no contact was tested: the ticks before it (selection, fire,
    /// effects) really did happen and are reported above.
    pub advance_refused: Option<ProjectileRuntimeError>,
    /// The rounds whose declared lifetime was spent this tick.
    pub retired: Vec<ProjectileId>,
    /// What the ECS mirror pass changed.
    pub mirrors: MirrorReport,
}

impl WeaponTick {
    /// The tick report starts empty at `at`.
    fn new(at: Tick) -> Self {
        Self {
            at,
            ..Self::default()
        }
    }
}

/// Runs one tick of the weapon path: selection, fire, effects, motion, swept
/// damage and retirement, plus the ECS mirror pass.
///
/// `orders` are applied in the order they are given, so a caller that wants a
/// bank switch to apply to this tick's shot puts it first. `damage` is the
/// session's damage authority, borrowed rather than owned: F29 owns that
/// lifecycle and every routed contact reaches it through
/// [`resolve_swept_damage`], so no weapon damage is applied anywhere else.
///
/// # What a refusal means here
///
/// A whole-tick refusal ([`StepRefusal`]) means nothing at all happened. An
/// order refusal means that order changed nothing while the rest of the tick
/// ran. A fire refusal is the cadence's: no round consumed, no cooldown started
/// and no round spawned (F27-B). A denied mount is per mount, so a disabled
/// wing gun emits no projectile, no sound and no effect while its enabled
/// sibling still fires (AC02). An unreadable mount or part is named rather than
/// dropped, and an advance refusal leaves every round exactly where it was.
/// None of them is swallowed, and the accepted shots that did happen are
/// reported beside them.
///
/// # Ordering
///
/// Selection, then fire, then motion, then the sweep, then retirement. A round
/// fired on this tick is moved by the same step, so its first segment is swept
/// here; a round that lands several ticks later is routed from the accepted shot
/// this session has been keeping for it.
#[must_use]
pub fn step_weapon_session(
    world: &mut World,
    session: &mut WeaponSession,
    damage: &mut DamageResolver,
    orders: &[WeaponOrder],
    step: &WeaponStep<'_>,
) -> WeaponTick {
    let mut tick = WeaponTick::new(step.at);
    if let Some(refusal) = session.begin_tick(step.at) {
        tick.refused = Some(refusal);
        return tick;
    }
    session.cadence.advance_to(step.at);

    for order in orders {
        match order {
            WeaponOrder::SelectBank { shooter, bank } => {
                if let Err(refusal) = session.select(*shooter, bank.clone()) {
                    tick.orders_refused.push(refusal);
                }
            }
            WeaponOrder::Fire(intent) => {
                // The live read first, so a mount with no readable pose is
                // named here as well as refused by the resolver: a gun never
                // fires from the world origin because a walk skipped it.
                let mounts = live_mount_transforms(world, intent.shooter, step.generation);
                tick.unreadable_mounts
                    .extend(mounts.refused.into_iter().map(|refusal| UnreadableMount {
                        shooter: intent.shooter,
                        refusal,
                    }));
                match session
                    .cadence
                    .fire(intent, &mounts.transforms, step.wind_velocity_m_s)
                {
                    Ok(resolution) => {
                        tick.denied
                            .extend(resolution.refused.iter().map(|reason| DeniedShot {
                                intent: resolution.intent,
                                reason: reason.clone(),
                            }));
                        for event in &resolution.accepted {
                            match session.accept(event, step.at) {
                                Ok(effect) => tick.effects.push(effect),
                                Err(refusal) => {
                                    // The shot happened and consumed its round,
                                    // so its effect is still the gun's declared
                                    // one — but a round nothing can admit is
                                    // reported instead of being swept blind.
                                    tick.effects.push(WeaponEffect::of(event, step.at));
                                    tick.routing_refused.push(refusal);
                                }
                            }
                        }
                        tick.accepted.extend(resolution.accepted);
                    }
                    Err(refusal) => tick.fire_refused.push(refusal),
                }
            }
        }
    }

    match session
        .cadence
        .advance_projectiles(step.dt_s, step.wind_velocity_m_s)
    {
        Err(source) => tick.advance_refused = Some(source),
        Ok(rounds) => {
            tick.retired = rounds.expired.clone();
            if !rounds.segments.is_empty() {
                // One read of the live part boxes per tick, shared by every
                // round: the boxes are a property of the world at the end of
                // the tick, not of one projectile. The read carries no
                // allegiance — the declared relation belongs to a
                // (shooter, candidate) pair and this tick routes the rounds of
                // *every* shooter, so `route_round` binds it per round
                // against the shot's own shooter instead of baking one
                // shooter's relations into a shared read.
                let parts = part_sweep_candidates(world, step.dt_s, |_target| None);
                tick.unreadable_parts = parts.refused;
                for segment in &rounds.segments {
                    match route_round(session, damage, segment, &parts.candidates, step) {
                        Ok(routed) => tick.routed.push(routed),
                        Err(refusal) => tick.routing_refused.push(refusal),
                    }
                }
            }
        }
    }

    // A round whose lifetime is spent has no segment to sweep next tick, so its
    // accepted shot goes with it: the session keeps a record only while the
    // round that needs it is live.
    for projectile in &tick.retired {
        session.forget(projectile);
    }
    tick.mirrors = sync_round_mirrors(world, session, step.generation);
    tick
}

/// Routes one live round's segment through its gun's declared rules and the
/// damage authority.
///
/// The declared relation is bound here, against the round's own shooter: the
/// same candidate box is an ally of one shooter and an enemy of another, so a
/// relation decided once per tick rather than once per round would filter
/// somebody's rounds under the wrong shooter's allegiance.
///
/// # Errors
///
/// [`RoutingRefusal::UnknownRound`] when the session does not hold the round's
/// routing record — reported by name, because a round swept with a guessed
/// attacker and a default hostility test would apply damage no rule stands
/// behind.
fn route_round(
    session: &mut WeaponSession,
    damage: &mut DamageResolver,
    segment: &ProjectileSegment,
    candidates: &[SweepCandidate],
    step: &WeaponStep<'_>,
) -> Result<RoutedRound, RoutingRefusal> {
    let Some(record) = session.round_record(&segment.projectile).cloned() else {
        return Err(RoutingRefusal::UnknownRound {
            projectile: segment.projectile,
        });
    };
    let candidates: Vec<SweepCandidate> = candidates
        .iter()
        .map(|candidate| {
            SweepCandidate::new(
                candidate.target,
                candidate.node.clone(),
                (step.relation)(record.shot.shooter, candidate.target.actor),
            )
        })
        .collect();
    Ok(RoutedRound {
        projectile: segment.projectile,
        outcome: resolve_swept_damage(
            &mut session.router,
            &record.rules,
            damage,
            &record.shot,
            segment,
            candidates,
            step.at,
        ),
    })
}

/// Reconciles the ECS mirror of the authoritative rounds.
///
/// Every live round gets a [`WeaponRoundMirror`] entity whose [`Transform`] is
/// written from the runtime's own position, and a mirror whose round is gone —
/// retired this tick, removed by a despawn, released by teardown, or left behind
/// by a reloaded scene — is despawned. The pass *reconciles* rather than tracks:
/// it looks for what is there and for what is live, so no entity id is stored
/// and a stale mirror cannot survive a reload by pointing at the past.
///
/// The mirror carries no collider and is never integrated: the authoritative
/// position lives in [`cs_sim::weapons::LiveProjectile`], so Avian never
/// becomes a second contact authority for a round (FLIGHT-PHYSICS, "one physics
/// pose owner").
///
/// [`cs_sim::weapons::LiveProjectile`]: cs_sim::weapons::LiveProjectile
#[must_use]
pub fn sync_round_mirrors(
    world: &mut World,
    session: &WeaponSession,
    generation: SceneGeneration,
) -> MirrorReport {
    let mut report = MirrorReport::default();
    let live: BTreeMap<ProjectileId, (ActorId, WorldPosition)> = session
        .cadence
        .projectiles()
        .iter()
        .map(|round| (round.projectile(), (round.shooter(), round.current())))
        .collect();

    let mut present: BTreeMap<ProjectileId, Entity> = BTreeMap::new();
    let mut stale: Vec<(Entity, ProjectileId)> = Vec::new();
    for entity_ref in world.iter_entities() {
        let Some(mirror) = entity_ref.get::<WeaponRoundMirror>() else {
            continue;
        };
        if mirror.generation != generation || !live.contains_key(&mirror.projectile) {
            stale.push((entity_ref.id(), mirror.projectile));
            continue;
        }
        present.insert(mirror.projectile, entity_ref.id());
    }
    for (entity, projectile) in stale {
        world.entity_mut(entity).despawn();
        report.despawned.push(projectile);
    }

    for (projectile, (shooter, position)) in &live {
        match present.get(projectile) {
            Some(entity) => {
                world.entity_mut(*entity).insert(transform_of(*position));
                report.moved += 1;
            }
            None => {
                world.spawn((
                    WeaponRoundMirror {
                        projectile: *projectile,
                        shooter: *shooter,
                        generation,
                    },
                    transform_of(*position),
                ));
                report.spawned.push(*projectile);
            }
        }
    }
    report
}

/// The mirror [`Transform`] for an authoritative round position.
///
/// Position only: no orientation is measured for a round in the original data,
/// so the mirror carries none rather than a default rotation that would read as
/// a claim. The f32 conversion is the ECS's own precision — the authoritative
/// position stays f64 in the runtime, and every mirror write comes from it.
fn transform_of(position: WorldPosition) -> Transform {
    let [x, y, z] = position.to_array();
    Transform::from_translation(bevy::prelude::Vec3::new(x as f32, y as f32, z as f32))
}

/// Despawns every [`WeaponRoundMirror`] entity, returning the round ids released.
fn despawn_round_mirrors(world: &mut World) -> usize {
    let mirrors: Vec<Entity> = world
        .iter_entities()
        .filter(|entity_ref| entity_ref.contains::<WeaponRoundMirror>())
        .map(|entity_ref| entity_ref.id())
        .collect();
    for entity in &mirrors {
        world.entity_mut(*entity).despawn();
    }
    mirrors.len()
}
