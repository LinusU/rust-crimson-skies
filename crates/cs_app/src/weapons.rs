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
//!
//! Nothing here owns weapon state: the selection, cooldowns, ammunition and
//! disabled mounts are the [`FireResolver`]'s; these are the conversion,
//! binding and application records the ECS wiring consumes (F27-B/C).
//!
//! [`FireResolver`]: cs_sim::weapons::FireResolver
//! [`GunHitRouter`]: cs_sim::weapons::GunHitRouter
//! [`DamageResolver`]: cs_sim::damage::DamageResolver
//! [`HitEvent`]: cs_sim::damage::HitEvent

use avian3d::prelude::LinearVelocity;
use bevy::ecs::component::Component;
use bevy::prelude::{ChildOf, Entity, World};
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
    AmmunitionId, AmmunitionIdError, FireEvent, FriendlyFireRule, GunDefinition,
    GunDefinitionError, GunHitRouter, GunMountKind, GunRate, InheritanceRule, MountTransform,
    ProjectileSegment, SelfHitRule, SpreadCone, SweepCandidate, SweepOutcome, SweepRefusal,
    SweepTarget, SweepTargetError, WeaponDamage, WeaponRules,
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
