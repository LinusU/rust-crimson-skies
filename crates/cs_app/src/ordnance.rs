//! The ordnance application boundary (F28-A).
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-A`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`, sections "Boost and special models"
//! and "Collision and ballistic tests".
//!
//! This module sits between the declared ordnance schema
//! ([`cs_content::ordnance`]) and the session registry
//! ([`cs_sim::weapons::ordnance`]), which cannot see each other — `cs_sim`
//! must not depend on `cs_content` (`docs/01-ARCHITECTURE.md`). It contains:
//!
//! * [`lower_ordnance`] — the conversion boundary: a validated
//!   [`cs_content::ordnance::DeclaredOrdnance`] becomes the runtime
//!   [`OrdnanceComponent`] a session registers. Every `Resolved::Unknown`
//!   **refuses** rather than guessing: a session must not fly a rocket whose
//!   trigger radius, arming delay, launch speed, blast radius, damage or
//!   lost-target rule was invented, so the refusal names the field and
//!   carries the claim.
//! * [`lower_equipment_rules`] — the equipment-compatibility boundary, for
//!   the same shared rule the shop (F44) and an import both read (F28
//!   non-negotiable 5). Every `Resolved::Unknown` refuses by name too.
//! * [`OrdnanceLauncherBinding`] — the generation-stamped ECS record tying
//!   an entity to its session-qualified [`ActorId`], its lowered components
//!   and the declared loadout they came from, so a reload can never leave a
//!   stale binding looking live.
//!
//! The scene binding stays on the declared record for F28-B's hierarchy walk:
//! gameplay state never depends on a presentation reference, so the runtime
//! component carries no `SceneNodeId`.
//!
//! Nothing here owns ordnance state: the in-flight fuse, the guidance
//! trackers, the status ledger and the nitro ledger are
//! `cs_sim::weapons::ordnance`'s; these are the conversion and binding
//! records the ECS wiring consumes (F28-C).

use std::collections::BTreeSet;

use bevy::ecs::component::Component;
use cs_content::ordnance::{
    DeclaredAreaEffect, DeclaredArmingRule, DeclaredEquipmentRules, DeclaredFuseRule,
    DeclaredGuidanceRule, DeclaredHardpointKind, DeclaredInheritanceRule,
    DeclaredLostTargetBehavior, DeclaredNitroActivationRule, DeclaredNitroParameters,
    DeclaredOrdnance, DeclaredProximityFuse, DeclaredStatusEffect,
};
use cs_sim::damage::{ActorId, DamageNodeKey, NodeKeyError};
use cs_sim::weapons::{
    AreaEffect, ArmingRule, EquipmentRules, FuseRule, GuidanceRule, GunDefinitionError,
    HardpointKind, InheritanceRule, LaunchGeometry, LostTargetBehavior, NitroActivationRule,
    NitroOrdnance, NitroParameters, NitroTradeoffs, OrdnanceComponent, OrdnanceDefinitionError,
    OrdnanceFamily, OrdnanceId, OrdnanceIdError, OrdnanceMedia, OrdnanceStatusEffect,
    ProjectileOrdnance, ProximityFuse, StackLoad, StatusEffectKind, WeaponDamage,
};
use cs_types::content::{ContentId, Known, Resolved};
use cs_types::evidence::ClaimId;

use std::collections::BTreeMap;

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, Transform, World};
use cs_sim::damage::{DamageError, DamageResolver, HitEvent, TickResolution};
use cs_sim::time::TickRate;
use cs_sim::weapons::{
    ExpiredStatusEffect, FuseDecision, FuseTrigger, GuidanceTick, MountTransform, NitroTick,
    OrdnanceRuntime, OrdnanceRuntimeError, ProjectileId, StatusEffectInstanceId,
    StatusEffectTarget, SweptHit, TargetObservation, TargetPath,
};
use cs_types::Tick;
use cs_types::net::{EventId, SessionId};
use cs_types::space::{UnitVec3, WorldPosition};

use crate::scene::{NodeVisualTransform, SceneGeneration};
use crate::weapons::{LiveMountTransforms, MountPoseBinding, MountPoseRefusal};

/// Why a declared ordnance record could not be lowered to the runtime
/// records.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceLowerError {
    /// A declared id could not form a runtime ordnance id — a
    /// wrong-namespace id slipped through the declared schema.
    OrdnanceId {
        /// The declared id text.
        id: String,
        /// Why the runtime refused it.
        source: OrdnanceIdError,
    },
    /// A declared mount key could not form a runtime key. Unreachable while
    /// both crates apply the same grammar, kept so the boundary stays honest
    /// if they ever diverge.
    MountKey {
        /// The declared key text.
        key: String,
        /// Why the runtime refused it.
        source: NodeKeyError,
    },
    /// A field of the declared component is `Resolved::Unknown`: no session
    /// may fly a component whose parameter was invented.
    UnknownField {
        /// Which field is unknown.
        field: &'static str,
        /// The claim the unknown is recorded under.
        claim_id: ClaimId,
        /// Why the value is unknown.
        reason: String,
    },
    /// A declared damage amount was refused by the shared damage
    /// vocabulary. `WeaponDamage` is the F27-F29 channel record whose
    /// validator is shared between guns and ordnance, so this names the
    /// declared channel field rather than folding a gun-specific error into
    /// the ordnance error type.
    DamageChannel {
        /// Which declared channel was refused.
        field: &'static str,
        /// Why it was refused.
        reason: String,
    },
    /// The runtime refused the assembled definition.
    Definition(OrdnanceDefinitionError),
}

impl OrdnanceLowerError {
    /// Maps a shared damage-vocabulary refusal onto the declared channel
    /// field it came from.
    fn shared_damage(source: GunDefinitionError) -> Self {
        Self::DamageChannel {
            field: "damage",
            reason: source.to_string(),
        }
    }
}

impl std::fmt::Display for OrdnanceLowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OrdnanceId { id, source } => {
                write!(f, "ordnance id {id:?} cannot be lowered: {source}")
            }
            Self::MountKey { key, source } => {
                write!(f, "launcher mount key {key:?} cannot be lowered: {source}")
            }
            Self::UnknownField {
                field,
                claim_id,
                reason,
            } => write!(
                f,
                "ordnance field {field} is unknown ({}: {reason}) and cannot be lowered",
                claim_id.as_str()
            ),
            Self::DamageChannel { field, reason } => {
                write!(
                    f,
                    "ordnance {field} was refused by the damage vocabulary: {reason}"
                )
            }
            Self::Definition(source) => {
                write!(f, "the runtime refused the lowered component: {source}")
            }
        }
    }
}

impl std::error::Error for OrdnanceLowerError {}

/// Lowers a declared component into the runtime one a session registers.
///
/// The conversion is field-wise and refuses every unknown by name. Families,
/// hardpoint kinds, arming rules, fuse rules, guidance rules, lost-target
/// behaviors, status kinds and nitro activation rules map variant-wise; the
/// numeric parameters are copied only when resolved.
///
/// A declared `SceneNodeId` binding is deliberately **not** part of the
/// runtime component: the launch pose is read from the live aircraft
/// hierarchy by F28-B, so carrying the visual binding into the runtime record
/// would smuggle a presentation reference into gameplay state. It stays
/// available on the declared record for F28-B to read there, which is what
/// [`declared_scene_binding`] returns.
///
/// # Errors
///
/// [`OrdnanceLowerError::UnknownField`] naming the first unresolved field,
/// [`OrdnanceLowerError::OrdnanceId`] or
/// [`OrdnanceLowerError::Definition`].
pub fn lower_ordnance(
    declared: &DeclaredOrdnance,
) -> Result<OrdnanceComponent, OrdnanceLowerError> {
    let id = OrdnanceId::try_new(declared.ordnance().clone()).map_err(|source| {
        OrdnanceLowerError::OrdnanceId {
            id: declared.ordnance().as_str().to_owned(),
            source,
        }
    })?;
    let family = lower_family(declared.family());
    match declared.details() {
        cs_content::ordnance::DeclaredOrdnanceDetails::Projectile(projectile) => Ok(
            OrdnanceComponent::Projectile(Box::new(lower_projectile(id, family, projectile)?)),
        ),
        cs_content::ordnance::DeclaredOrdnanceDetails::Nitro(nitro) => Ok(
            OrdnanceComponent::Nitro(Box::new(lower_nitro(id, &nitro.parameters, nitro)?)),
        ),
    }
}

/// Lowers a declared launched component.
fn lower_projectile(
    id: OrdnanceId,
    family: OrdnanceFamily,
    declared: &cs_content::ordnance::DeclaredProjectile,
) -> Result<ProjectileOrdnance, OrdnanceLowerError> {
    let launch = lower_launch(&declared.launch)?;
    let stack = lower_stack(&declared.stack)?;
    let arming = lower_arming(&declared.arming)?;
    let fuse = lower_fuse(&declared.fuse)?;
    let guidance = lower_guidance(&declared.guidance)?;
    let lifetime_ticks = known_or_refuse("lifetime_ticks", &declared.lifetime_ticks)?;
    let area_effect = declared
        .area_effect
        .as_ref()
        .map(lower_area_effect)
        .transpose()?;
    // `WeaponDamage` is the F27-F29 damage vocabulary, whose validator is
    // shared, so its refusals arrive as `GunDefinitionError`. They are
    // re-raised as the *declared* damage field they came from rather than
    // through a lossy string, so a caller can see which channel was refused.
    let channels = WeaponDamage::try_new(
        known_or_refuse("damage.armor", &declared.armor_damage)?,
        known_or_refuse("damage.internal", &declared.internal_damage)?,
    )
    .map_err(OrdnanceLowerError::shared_damage)?;
    let mut status = Vec::with_capacity(declared.status.len());
    for effect in &declared.status {
        status.push(lower_status(effect)?);
    }
    let media = lower_media(&declared.media)?;
    let equipment_rules = lower_equipment_rules(&declared.equipment_rules)?;

    ProjectileOrdnance::try_new(
        id,
        family,
        launch,
        stack,
        arming,
        fuse,
        guidance,
        lifetime_ticks,
        area_effect,
        channels,
        status,
        media,
        equipment_rules,
    )
    .map_err(OrdnanceLowerError::Definition)
}

/// Lowers the declared launch geometry.
fn lower_launch(
    declared: &cs_content::ordnance::DeclaredLaunchGeometry,
) -> Result<LaunchGeometry, OrdnanceLowerError> {
    let mount = DamageNodeKey::new(declared.mount.as_str()).map_err(|source| {
        OrdnanceLowerError::MountKey {
            key: declared.mount.as_str().to_owned(),
            source,
        }
    })?;
    let hardpoint = lower_hardpoint(known_or_refuse("launch.hardpoint", &declared.hardpoint)?);
    let launch_speed_mps = known_or_refuse("launch.launch_speed_mps", &declared.launch_speed_mps)?;
    let inheritance = lower_inheritance(known_or_refuse(
        "launch.inheritance",
        &declared.inheritance,
    )?);
    let release_delay_ticks =
        known_or_refuse("launch.release_delay_ticks", &declared.release_delay_ticks)?;
    LaunchGeometry::try_new(
        mount,
        hardpoint,
        launch_speed_mps,
        inheritance,
        release_delay_ticks,
    )
    .map_err(OrdnanceLowerError::Definition)
}

/// Lowers the declared stack load.
fn lower_stack(
    declared: &cs_content::ordnance::DeclaredStackLoad,
) -> Result<StackLoad, OrdnanceLowerError> {
    let capacity_units = known_or_refuse("stack.capacity_units", &declared.capacity_units)?;
    let unit_mass_kg = known_or_refuse("stack.unit_mass_kg", &declared.unit_mass_kg)?;
    StackLoad::try_new(capacity_units, unit_mass_kg).map_err(OrdnanceLowerError::Definition)
}

/// Lowers a declared arming rule.
fn lower_arming(declared: &DeclaredArmingRule) -> Result<ArmingRule, OrdnanceLowerError> {
    Ok(match declared {
        DeclaredArmingRule::Disarmed => ArmingRule::Disarmed,
        DeclaredArmingRule::AfterTicks(ticks) => {
            ArmingRule::AfterTicks(known_or_refuse("arming.after_ticks", ticks)?)
        }
        DeclaredArmingRule::AfterTravelMetres(metres) => {
            ArmingRule::AfterTravelMetres(known_or_refuse("arming.after_travel_m", metres)?)
        }
    })
}

/// Lowers a declared fuse rule.
fn lower_fuse(declared: &DeclaredFuseRule) -> Result<FuseRule, OrdnanceLowerError> {
    Ok(match declared {
        DeclaredFuseRule::Impact => FuseRule::Impact,
        DeclaredFuseRule::Proximity(DeclaredProximityFuse { trigger_radius_m }) => {
            FuseRule::Proximity(
                ProximityFuse::try_new(known_or_refuse(
                    "fuse.proximity.trigger_radius_m",
                    trigger_radius_m,
                )?)
                .map_err(OrdnanceLowerError::Definition)?,
            )
        }
        DeclaredFuseRule::Timed { ticks } => FuseRule::Timed {
            ticks: known_or_refuse("fuse.timed.ticks", ticks)?,
        },
    })
}

/// Lowers a declared guidance rule.
///
/// The lost-target behavior refuses to lower while unknown: F28
/// non-negotiable 4 requires the behavior to be *specified*, so there is no
/// default that would let a seeker coast or detonate by accident.
fn lower_guidance(declared: &DeclaredGuidanceRule) -> Result<GuidanceRule, OrdnanceLowerError> {
    Ok(match declared {
        DeclaredGuidanceRule::Unguided => GuidanceRule::Unguided,
        DeclaredGuidanceRule::Targeted { lost_target } => GuidanceRule::Targeted {
            lost_target: lower_lost_target(known_or_refuse("guidance.lost_target", lost_target)?)?,
        },
    })
}

/// Lowers a declared lost-target behavior.
fn lower_lost_target(
    declared: DeclaredLostTargetBehavior,
) -> Result<LostTargetBehavior, OrdnanceLowerError> {
    Ok(match declared {
        DeclaredLostTargetBehavior::Detonate => LostTargetBehavior::Detonate,
        DeclaredLostTargetBehavior::Coast => LostTargetBehavior::Coast,
        DeclaredLostTargetBehavior::Disarm => LostTargetBehavior::Disarm,
    })
}

/// Lowers a declared bounded area effect.
fn lower_area_effect(declared: &DeclaredAreaEffect) -> Result<AreaEffect, OrdnanceLowerError> {
    let radius_m = known_or_refuse("area_effect.radius_m", &declared.radius_m)?;
    let lifetime_ticks = known_or_refuse("area_effect.lifetime_ticks", &declared.lifetime_ticks)?;
    AreaEffect::try_new(radius_m, lifetime_ticks).map_err(OrdnanceLowerError::Definition)
}

/// Lowers one declared status effect.
fn lower_status(
    declared: &DeclaredStatusEffect,
) -> Result<OrdnanceStatusEffect, OrdnanceLowerError> {
    let kind = lower_status_kind(declared.kind);
    let duration_ticks = known_or_refuse("status.duration_ticks", &declared.duration_ticks)?;
    let strength = known_or_refuse("status.strength", &declared.strength)?;
    OrdnanceStatusEffect::try_new(kind, duration_ticks, strength)
        .map_err(OrdnanceLowerError::Definition)
}

/// Lowers the declared media record.
fn lower_media(
    declared: &cs_content::ordnance::DeclaredOrdnanceMedia,
) -> Result<OrdnanceMedia, OrdnanceLowerError> {
    let visual = known_or_refuse("media.visual", &declared.visual)?;
    let sound = known_or_refuse("media.sound", &declared.sound)?;
    let particles = declared
        .particles
        .as_ref()
        .map(|resolved| known_or_refuse("media.particles", resolved))
        .transpose()?;
    OrdnanceMedia::try_new(visual, sound, particles).map_err(OrdnanceLowerError::Definition)
}

/// Lowers the declared equipment rules.
///
/// The requires/forbids options refuse independently: an unmeasured
/// requirement does not become "no requirement" and an unmeasured
/// prohibition does not become "nothing is forbidden", because either
/// substitution would let an incompatible installation through the shop
/// (F28 non-negotiable 5).
///
/// # Errors
///
/// [`OrdnanceLowerError::UnknownField`] naming the first unresolved option.
pub fn lower_equipment_rules(
    declared: &DeclaredEquipmentRules,
) -> Result<EquipmentRules, OrdnanceLowerError> {
    let requires = declared
        .requires
        .as_ref()
        .map(|resolved| known_or_refuse("equipment_rules.requires", resolved))
        .transpose()?;
    let mut forbids = BTreeSet::new();
    for resolved in &declared.forbids {
        forbids.insert(known_or_refuse("equipment_rules.forbids", resolved)?);
    }
    Ok(EquipmentRules::new(requires, forbids))
}

/// Lowers a declared nitro booster.
fn lower_nitro(
    id: OrdnanceId,
    parameters: &DeclaredNitroParameters,
    declared: &cs_content::ordnance::DeclaredNitro,
) -> Result<NitroOrdnance, OrdnanceLowerError> {
    let capacity_units = known_or_refuse("nitro.capacity_units", &parameters.capacity_units)?;
    let consumption_per_s =
        known_or_refuse("nitro.consumption_per_s", &parameters.consumption_per_s)?;
    let recovery_per_s = known_or_refuse("nitro.recovery_per_s", &parameters.recovery_per_s)?;
    let extra_thrust_n = known_or_refuse("nitro.extra_thrust_n", &parameters.extra_thrust_n)?;
    let activation =
        lower_nitro_activation(known_or_refuse("nitro.activation", &parameters.activation)?)?;
    let tradeoffs = NitroTradeoffs::try_new(known_or_refuse(
        "nitro.authority_multiplier",
        &parameters.authority_multiplier,
    )?)
    .map_err(OrdnanceLowerError::Definition)?;
    let lowered = NitroParameters::try_new(
        capacity_units,
        consumption_per_s,
        recovery_per_s,
        extra_thrust_n,
        activation,
        tradeoffs,
    )
    .map_err(OrdnanceLowerError::Definition)?;
    let media = lower_media(&declared.media)?;
    let equipment_rules = lower_equipment_rules(&declared.equipment_rules)?;
    NitroOrdnance::try_new(id, lowered, media, equipment_rules)
        .map_err(OrdnanceLowerError::Definition)
}

/// Lowers a declared nitro activation rule.
fn lower_nitro_activation(
    declared: DeclaredNitroActivationRule,
) -> Result<NitroActivationRule, OrdnanceLowerError> {
    Ok(match declared {
        DeclaredNitroActivationRule::WhileHeld => NitroActivationRule::WhileHeld,
        DeclaredNitroActivationRule::FixedTicks { ticks } => NitroActivationRule::FixedTicks {
            ticks: known_or_refuse("nitro.activation.fixed_ticks", &ticks)?,
        },
    })
}

/// Lowers a declared family.
fn lower_family(declared: cs_content::ordnance::DeclaredOrdnanceFamily) -> OrdnanceFamily {
    match declared {
        cs_content::ordnance::DeclaredOrdnanceFamily::DirectExplosive => {
            OrdnanceFamily::DirectExplosive
        }
        cs_content::ordnance::DeclaredOrdnanceFamily::ProximityFlak => {
            OrdnanceFamily::ProximityFlak
        }
        cs_content::ordnance::DeclaredOrdnanceFamily::GuidedRocket => OrdnanceFamily::GuidedRocket,
        cs_content::ordnance::DeclaredOrdnanceFamily::AreaDenialEngine => {
            OrdnanceFamily::AreaDenialEngine
        }
        cs_content::ordnance::DeclaredOrdnanceFamily::AerialTorpedo => {
            OrdnanceFamily::AerialTorpedo
        }
        cs_content::ordnance::DeclaredOrdnanceFamily::NitroBooster => OrdnanceFamily::NitroBooster,
    }
}

/// Lowers a declared hardpoint kind.
fn lower_hardpoint(declared: DeclaredHardpointKind) -> HardpointKind {
    match declared {
        DeclaredHardpointKind::Nose => HardpointKind::Nose,
        DeclaredHardpointKind::WingLeft => HardpointKind::WingLeft,
        DeclaredHardpointKind::WingRight => HardpointKind::WingRight,
        DeclaredHardpointKind::Fuselage => HardpointKind::Fuselage,
        DeclaredHardpointKind::Underslung => HardpointKind::Underslung,
    }
}

/// Lowers a declared inheritance rule.
fn lower_inheritance(declared: DeclaredInheritanceRule) -> InheritanceRule {
    match declared {
        DeclaredInheritanceRule::Full => InheritanceRule::Full,
        DeclaredInheritanceRule::Fraction { share } => InheritanceRule::Fraction { share },
        DeclaredInheritanceRule::None => InheritanceRule::None,
    }
}

/// Lowers a declared status effect kind.
fn lower_status_kind(declared: cs_content::ordnance::DeclaredStatusEffectKind) -> StatusEffectKind {
    match declared {
        cs_content::ordnance::DeclaredStatusEffectKind::Damage => StatusEffectKind::Damage,
        cs_content::ordnance::DeclaredStatusEffectKind::Choke => StatusEffectKind::Choke,
        cs_content::ordnance::DeclaredStatusEffectKind::Stall => StatusEffectKind::Stall,
        cs_content::ordnance::DeclaredStatusEffectKind::Marker => StatusEffectKind::Marker,
    }
}

/// Unwraps a declared [`Resolved`] value, refusing an unknown with its claim
/// and reason rather than substituting a default.
fn known_or_refuse<T: Clone>(
    field: &'static str,
    resolved: &Resolved<T>,
) -> Result<T, OrdnanceLowerError> {
    match resolved {
        Resolved::Known(Known { value, .. }) => Ok(value.clone()),
        Resolved::Unknown { claim_id, reason } => Err(OrdnanceLowerError::UnknownField {
            field,
            claim_id: claim_id.clone(),
            reason: reason.clone(),
        }),
    }
}

/// The [`SceneNodeId`] a declared component's visual launcher binding names,
/// when it declares one.
///
/// F28-B reads the live aircraft hierarchy through this id; the ordnance
/// runtime never sees it, because gameplay state must not depend on a
/// presentation reference.
#[must_use]
pub fn declared_scene_binding(
    declared: &DeclaredOrdnance,
) -> Option<&Resolved<cs_content::scene::SceneNodeId>> {
    declared.scene_binding()
}

/// Component: marks an entity as the visual face of one ordnance launcher.
///
/// `actor` is the session-qualified [`ActorId`] the session's ordnance
/// registry holds the components for, `ordnance` the catalog ids those
/// components were lowered from, `loadout` the declared loadout they came
/// from, and `generation` the scene generation that spawned the binding — so
/// a reload stamps new bindings and stale ones are identified by mismatch,
/// never by surviving pointers (the same rule
/// [`crate::damage::DamageActorBinding`] and
/// [`crate::weapons::WeaponActorBinding`] follow).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct OrdnanceLauncherBinding {
    /// The session actor this entity's components belong to.
    pub actor: ActorId,
    /// The `weapon` catalog ids this launcher carries, in the registry's
    /// stable order.
    pub ordnance: Vec<ContentId>,
    /// The declared loadout the components came from.
    pub loadout: ContentId,
    /// The scene generation that spawned the binding.
    pub generation: SceneGeneration,
}

// ======================================================= F28-C integration =====
//
// F28-C. The F28-A boundary lowered and bound, and F28-B owned the per-tick
// runtime; this section is the application half that wires them into their
// actual producer and consumer for one session generation. It follows F27-C's
// `WeaponSession`/`step_weapon_session` (`crate::weapons`) because the two
// stages integrate the same way: one session owns the runtime and the per-tick
// step is the only production path that moves it.
//
// * **Producer**: a launch order names a registered component, and the step
//   reads that component's live launcher pose from the ECS hierarchy
//   ([`live_launcher_transforms`], the ordnance counterpart of F27-C's
//   `live_mount_transforms`). A mount with no readable pose is reported by
//   name and nothing is launched from the world origin.
// * **Consumer**: a triggered item's declared damage is routed into the shared
//   [`DamageResolver`] once, and its declared status effects are applied to a
//   stable [`StatusEffectTarget`]. [`EngineStatus`] is the flight-model
//   consumer of the choke/stall/damage/marker ledger.
// * **Network**: every launch, trigger, expiry, status transition and nitro
//   activation appends an [`OrdnanceNetworkEvent`] stamped with a session
//   [`EventId`].
// * **Teardown**: [`OrdnanceSession::close`] releases every live item and
//   despawns every mirror, and a closed session refuses every later order,
//   step and registration. A restart builds a **new** session whose status
//   ledger and nitro tables start empty — the minimum AC03 scenario.
//
// The launcher walk is local rather than a call to F27-C's private helpers:
// `crate::weapons` is not an owner path for F28-C, so this module reuses the
// public records (`MountPoseBinding`, `MountPoseRefusal`,
// `LiveMountTransforms`) and re-reads the hierarchy itself.

// ------------------------------------------------------------- registration ---

/// One registered hardpoint component, in the runtime form the ECS binds.
///
/// Registration returns these because the scene wiring needs the *lowered*
/// catalog id, the mount the projectile leaves from and the two media ids a
/// launch will name — the declared record is not reachable from the session,
/// and re-deriving it from the id would be a second lowering.
#[derive(Clone, Debug, PartialEq)]
pub struct RegisteredOrdnance {
    /// The lowered catalog id.
    pub ordnance: OrdnanceId,
    /// The damage-node mount a launched item occupies, `None` for a booster.
    pub mount: Option<DamageNodeKey>,
    /// The behavior family.
    pub family: OrdnanceFamily,
    /// The declared launch-effect catalog id.
    pub visual: ContentId,
    /// The declared launch-sound catalog id.
    pub sound: ContentId,
}

/// Why a declared loadout could not be registered.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceRegistrationError {
    /// The session has been closed by teardown.
    Closed,
    /// The actor belongs to another session generation.
    ForeignSession {
        /// The session's generation.
        expected: u64,
        /// The generation the actor carried.
        found: u64,
    },
    /// A declared component could not be lowered: an unknown field refuses
    /// rather than inventing an ordnance parameter.
    Lower(OrdnanceLowerError),
    /// The same catalog id was registered twice for one actor.
    DuplicateOrdnance {
        /// The repeated id.
        ordnance: OrdnanceId,
    },
    /// The runtime refused the nitro registration itself.
    Nitro(OrdnanceRuntimeError),
}

impl std::fmt::Display for OrdnanceRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "the ordnance session is closed"),
            Self::ForeignSession { expected, found } => write!(
                f,
                "the actor belongs to session {found}, but this session is {expected}"
            ),
            Self::Lower(source) => {
                write!(f, "the declared ordnance could not be lowered: {source}")
            }
            Self::DuplicateOrdnance { ordnance } => {
                write!(f, "{ordnance} is registered twice for one actor")
            }
            Self::Nitro(source) => write!(f, "the runtime refused the booster: {source}"),
        }
    }
}

impl std::error::Error for OrdnanceRegistrationError {}

// ------------------------------------------------------------ effects & net ---

/// One launch's presentation record.
///
/// It exists only for a launch that happened: a refused order emits none. The
/// ids are the component's declared media carried through unchanged — this
/// stage names them, it does not load, pick or play them.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceEffect {
    /// The item this effect belongs to.
    pub projectile: ProjectileId,
    /// Who launched it.
    pub shooter: ActorId,
    /// Which component it is.
    pub ordnance: OrdnanceId,
    /// The tick the launch was accepted on.
    pub at: Tick,
    /// The launcher's world position at the tick of release.
    pub origin: WorldPosition,
    /// The component's declared launch-effect catalog id.
    pub visual: ContentId,
    /// The component's declared launch-sound catalog id.
    pub sound: ContentId,
}

/// What one [`OrdnanceNetworkEvent`] records.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceEventKind {
    /// An item was launched.
    Launched {
        /// Who launched it.
        shooter: ActorId,
        /// The item.
        projectile: ProjectileId,
        /// The component it is.
        ordnance: OrdnanceId,
    },
    /// An item's fuse triggered and its declared effects landed.
    Triggered {
        /// Who launched the item.
        shooter: ActorId,
        /// The item.
        projectile: ProjectileId,
        /// The actor whose part took the routed damage.
        target: ActorId,
    },
    /// A guided item lost its target and detonated, applying its declared
    /// blast at the position the runtime last recorded.
    GuidanceDetonated {
        /// Who launched the item.
        shooter: ActorId,
        /// The item.
        projectile: ProjectileId,
        /// The actor whose part took the routed damage.
        target: ActorId,
    },
    /// An item's own lifetime ended without a trigger.
    Expired {
        /// The item.
        projectile: ProjectileId,
    },
    /// A declared status effect was applied to a stable recipient.
    StatusApplied {
        /// The recipient.
        target: StatusEffectTarget,
        /// The effect kind.
        kind: StatusEffectKind,
        /// The tick it expires on.
        expires_at: Tick,
    },
    /// A status effect reached its expiry boundary.
    StatusExpired {
        /// The recipient.
        target: StatusEffectTarget,
        /// The effect kind.
        kind: StatusEffectKind,
        /// The tick it expired on.
        expired_at: Tick,
    },
    /// A nitro activation was resolved.
    Nitro {
        /// The actor whose booster ran.
        shooter: ActorId,
        /// Whether the activation was accepted.
        active: bool,
        /// Extra thrust the accepted boost added this tick.
        extra_thrust_n: f64,
        /// Capacity consumed this tick.
        consumed_units: f64,
    },
}

/// One network event the session emitted, addressed by a session [`EventId`].
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceNetworkEvent {
    /// The stable event identity: session, tick, producer and sequence.
    pub id: EventId,
    /// What happened.
    pub kind: OrdnanceEventKind,
}

/// Appends one event to the log and returns it for the per-tick report.
fn push_network_event(
    log: &mut Vec<OrdnanceNetworkEvent>,
    sequence: &mut u32,
    session: SessionId,
    tick: Tick,
    producer: u32,
    kind: OrdnanceEventKind,
) -> OrdnanceNetworkEvent {
    let event = OrdnanceNetworkEvent {
        id: EventId {
            session,
            tick,
            producer,
            sequence: *sequence,
        },
        kind,
    };
    *sequence = sequence.wrapping_add(1);
    log.push(event.clone());
    event
}

// ------------------------------------------------------------------ orders ---

/// The once-only identity of one launch request.
///
/// A reconnect or retry can replay a request, so the step keeps the ids it has
/// already resolved and refuses a replay by name rather than launching twice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrdnanceLaunchId {
    /// The session generation the request was produced in.
    pub session: u64,
    /// The tick it names.
    pub tick: Tick,
    /// The producing system's serial.
    pub producer: u32,
    /// The sequence within that producer.
    pub sequence: u32,
}

/// Where a launched item's declared effects land when it triggers.
///
/// The producer (cockpit or AI) supplies this at launch: it names the damage
/// node the routed hits reach and the stable recipient a status effect is
/// applied to. The item itself carries no target geometry.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceEngagement {
    /// The actor whose part takes the routed damage.
    pub damage_target: ActorId,
    /// The damage node the routed hits name.
    pub node: DamageNodeKey,
    /// The stable recipient for the declared status effects.
    pub status_recipient: StatusEffectTarget,
}

/// One launch request.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceLaunch {
    /// The once-only request identity.
    pub id: OrdnanceLaunchId,
    /// Who is launching.
    pub shooter: ActorId,
    /// Which registered component is launched.
    pub ordnance: OrdnanceId,
    /// The guidance designation, when the component tracks a target.
    pub target: Option<ActorId>,
    /// Where the item's effects land when it triggers.
    pub engagement: OrdnanceEngagement,
}

/// One authoritative ordnance command for a tick.
///
/// Orders are the input boundary the cockpit and the AI share; the step
/// applies them in the order given. A launch is a *request*: the session
/// allocates the item's id, so a network replay can never name a second item.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceOrder {
    /// Launch one registered component from its live launcher pose.
    Launch(OrdnanceLaunch),
    /// Assert or release one actor's nitro control for this tick.
    Nitro {
        /// The actor whose booster is driven.
        shooter: ActorId,
        /// Whether the control is held this tick.
        requested: bool,
    },
}

/// Why one order was refused, changing no state.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceOrderRefusal {
    /// The request id was already resolved.
    DuplicateLaunch {
        /// The repeated id.
        id: OrdnanceLaunchId,
    },
    /// The actor has no such component registered.
    UnknownOrdnance {
        /// The actor that was named.
        shooter: ActorId,
        /// The component that was named.
        ordnance: OrdnanceId,
    },
    /// The named component is a booster, not a launched item.
    NotALauncher {
        /// The actor that was named.
        shooter: ActorId,
        /// The component that was named.
        ordnance: OrdnanceId,
    },
    /// The component's launcher pose could not be read from the hierarchy.
    MissingMountTransform {
        /// The actor whose launcher was unreadable.
        shooter: ActorId,
        /// The mount that had no readable pose.
        mount: DamageNodeKey,
    },
    /// The runtime refused the order: a foreign session, a duplicate item, a
    /// non-finite wind or velocity, or an unknown booster.
    Runtime(OrdnanceRuntimeError),
}

impl std::fmt::Display for OrdnanceOrderRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateLaunch { id } => write!(f, "launch request {id:?} was already resolved"),
            Self::UnknownOrdnance { shooter, ordnance } => {
                write!(f, "{shooter} has no {ordnance} registered in this session")
            }
            Self::NotALauncher { shooter, ordnance } => {
                write!(
                    f,
                    "{ordnance} registered for {shooter} is not a launched item"
                )
            }
            Self::MissingMountTransform { shooter, mount } => write!(
                f,
                "{shooter}'s launcher on mount {mount} has no readable live pose"
            ),
            Self::Runtime(source) => write!(f, "the runtime refused the order: {source}"),
        }
    }
}

impl std::error::Error for OrdnanceOrderRefusal {}

/// Why a whole tick was refused before any order was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdnanceStepRefusal {
    /// The session has been closed by teardown.
    Closed,
    /// The tick is not strictly after the last tick this session resolved.
    StaleTick {
        /// The last tick the session resolved.
        resolved_through: Tick,
        /// The tick the step was asked for.
        at: Tick,
    },
}

impl std::fmt::Display for OrdnanceStepRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => write!(f, "the ordnance session is closed"),
            Self::StaleTick {
                resolved_through,
                at,
            } => write!(f, "tick {at:?} was refused after tick {resolved_through:?}"),
        }
    }
}

impl std::error::Error for OrdnanceStepRefusal {}

// ------------------------------------------------------------------ reports ---

/// One live launcher pose the hierarchy walk could not read.
#[derive(Clone, Debug, PartialEq)]
pub struct UnreadableLauncher {
    /// The actor whose launcher was unreadable.
    pub shooter: ActorId,
    /// Why the pose could not be read.
    pub refusal: MountPoseRefusal,
}

/// One triggered item's whole outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceDetonation {
    /// The item that triggered.
    pub projectile: ProjectileId,
    /// Who launched it.
    pub shooter: ActorId,
    /// The fuse cause.
    pub trigger: FuseTrigger,
    /// Where its effects landed.
    pub engagement: OrdnanceEngagement,
    /// The routed hits, one per non-zero declared channel.
    pub hits: Vec<HitEvent>,
    /// The damage authority's resolution, when the batch was accepted.
    pub damage: Option<TickResolution>,
    /// Why the damage authority refused the batch, when it did.
    pub damage_refused: Option<DamageError>,
    /// The status instances the trigger applied.
    pub status_applied: Vec<StatusEffectInstanceId>,
    /// Why the status ledger refused the trigger, when it did.
    pub status_refused: Option<OrdnanceRuntimeError>,
}

/// One guidance-loss detonation's whole outcome.
///
/// The `LostTargetBehavior::Detonate` counterpart of [`OrdnanceDetonation`]: a
/// guided item whose target is lost ends and applies its declared blast at the
/// position the runtime last recorded, but it has no [`FuseTrigger`] — the
/// cause is the target loss the guidance tick reported, not a fuse decision —
/// so it is a separate record rather than a fabricated trigger.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceGuidanceBlast {
    /// The item that ended.
    pub projectile: ProjectileId,
    /// Who launched it.
    pub shooter: ActorId,
    /// The component it was.
    pub ordnance: OrdnanceId,
    /// The position the runtime last recorded for it.
    pub position: WorldPosition,
    /// The routed hits, one per non-zero declared channel.
    pub hits: Vec<HitEvent>,
    /// The damage authority's resolution, when the batch was accepted.
    pub damage: Option<TickResolution>,
    /// Why the damage authority refused the batch, when it did.
    pub damage_refused: Option<DamageError>,
    /// The status instances the blast applied.
    pub status_applied: Vec<StatusEffectInstanceId>,
    /// Why the status ledger refused the blast, when it did.
    pub status_refused: Option<OrdnanceRuntimeError>,
}

/// Why a triggered item's effects could not be routed.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceRoutingRefusal {
    /// The session holds a triggered item with no recorded engagement.
    MissingEngagement {
        /// The item that could not be routed.
        projectile: ProjectileId,
    },
    /// The runtime refused the routing or status call.
    Runtime(OrdnanceRuntimeError),
}

impl std::fmt::Display for OrdnanceRoutingRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingEngagement { projectile } => write!(
                f,
                "item {projectile} triggered but has no recorded engagement to route to"
            ),
            Self::Runtime(source) => write!(f, "the runtime refused the routing: {source}"),
        }
    }
}

impl std::error::Error for OrdnanceRoutingRefusal {}

/// What the ECS mirror pass changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrdnanceMirrorReport {
    /// Mirrors spawned for items that had none.
    pub spawned: Vec<ProjectileId>,
    /// Mirrors despawned because no live item backs them.
    pub despawned: Vec<ProjectileId>,
    /// Mirrors written onto this tick's authoritative position.
    pub moved: usize,
}

/// What one session teardown released.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrdnanceTeardownReport {
    /// The live items the session released, in ascending serial order.
    pub projectiles: Vec<ProjectileId>,
    /// How many ECS mirrors were despawned with them.
    pub mirrors: usize,
}

/// One tick's whole ordnance result.
///
/// Every list is a report of something that happened: an absent entry is a
/// miss, a denial or a refusal, each with its own list. Nothing is swallowed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrdnanceSessionTick {
    /// The tick that was resolved.
    pub at: Tick,
    /// The step was refused whole: nothing advanced and no order was read.
    pub refused: Option<OrdnanceStepRefusal>,
    /// Orders refused on their own, each changing nothing.
    pub orders_refused: Vec<OrdnanceOrderRefusal>,
    /// The items launched this tick, in launch order.
    pub launched: Vec<ProjectileId>,
    /// The guidance tick's outcome, when an observation was supplied.
    pub guidance: Option<GuidanceTick>,
    /// The guidance-loss detonations that applied their declared blast this
    /// tick, with everything each routed.
    pub guidance_blasts: Vec<OrdnanceGuidanceBlast>,
    /// The launch effects the accepted launches produced.
    pub effects: Vec<OrdnanceEffect>,
    /// The items that triggered, with everything each routed.
    pub detonations: Vec<OrdnanceDetonation>,
    /// Triggered items and routing calls that could not be resolved.
    pub routing_refused: Vec<OrdnanceRoutingRefusal>,
    /// Live launcher poses the hierarchy walk could not read.
    pub unreadable_launchers: Vec<UnreadableLauncher>,
    /// Items removed because their fuse had already triggered.
    pub retired: Vec<ProjectileId>,
    /// Items whose declared lifetime ended this tick.
    pub expired: Vec<ProjectileId>,
    /// Status effects that reached their expiry boundary this tick.
    pub status_expired: Vec<ExpiredStatusEffect>,
    /// The status advance was refused whole.
    pub status_refused: Option<OrdnanceRuntimeError>,
    /// One entry per nitro order that reached a registered booster.
    pub nitro: Vec<(ActorId, NitroTick)>,
    /// The network events emitted this tick.
    pub events: Vec<OrdnanceNetworkEvent>,
    /// What the ECS mirror pass changed.
    pub mirrors: OrdnanceMirrorReport,
    /// The item advance refused the whole tick.
    pub advance_refused: Option<OrdnanceRuntimeError>,
}

impl OrdnanceSessionTick {
    /// The tick report starts empty at `at`.
    fn new(at: Tick) -> Self {
        Self {
            at,
            ..Self::default()
        }
    }
}

/// The world inputs one ordnance step needs.
pub struct OrdnanceStep<'a> {
    /// The tick being resolved.
    pub at: Tick,
    /// The tick's length in seconds, used to move the items.
    pub dt_s: f64,
    /// The session's authoritative wind velocity in world m/s.
    pub wind_velocity_m_s: [f64; 3],
    /// The scene generation live entities carry.
    pub generation: SceneGeneration,
    /// One tick's target observation, when the session tracks guided items.
    /// `None` is "no observation this tick", not a lost target.
    pub guidance: Option<TargetObservation>,
    /// The eligible targets' swept paths for the proximity test.
    pub targets: &'a [TargetPath],
    /// The swept contacts the caller's ballistics query reported, in ascending
    /// time of impact. The ordnance path consumes them; it does not invent a
    /// collision.
    pub impacts: &'a [SweptHit],
}

// ------------------------------------------------------------------ session ---

/// One session generation's authority over the ordnance path.
///
/// The session owns the [`OrdnanceRuntime`], the lowered components a launch
/// can name, the engagement record each live item triggers against, the launch
/// request ledger, the launch-effect log and the network-event log. It does
/// **not** own the [`DamageResolver`]: that authority is shared with every
/// other damage producer, so [`step_ordnance_session`] takes it from its
/// caller exactly as F27-C does.
///
/// Dropping the session is the teardown; [`OrdnanceSession::close`] is the
/// explicit one, and it releases every live item and every mirror with it.
pub struct OrdnanceSession {
    session: u64,
    session_id: SessionId,
    rate: TickRate,
    producer: u32,
    runtime: OrdnanceRuntime,
    components: BTreeMap<ActorId, BTreeMap<OrdnanceId, OrdnanceComponent>>,
    engagements: BTreeMap<ProjectileId, OrdnanceEngagement>,
    next_projectile_serial: u64,
    next_event_sequence: u32,
    launch_ids: BTreeSet<OrdnanceLaunchId>,
    effects: Vec<OrdnanceEffect>,
    events: Vec<OrdnanceNetworkEvent>,
    resolved_through: Option<Tick>,
    closed: bool,
}

impl OrdnanceSession {
    /// Opens an ordnance session for one session generation at `tick`.
    ///
    /// `producer` is the serial the routed [`HitEvent`]s and the network
    /// [`EventId`]s are stamped with; it is distinct from the damage
    /// resolver's own producer, exactly as the F27-C session's router producer
    /// is.
    ///
    /// # Errors
    ///
    /// [`SessionRefusal::NoSession`] for session generation zero.
    pub fn new(
        session: u64,
        tick: Tick,
        rate: TickRate,
        producer: u32,
    ) -> Result<Self, crate::weapons::SessionRefusal> {
        let Some(session_id) = SessionId::new(session) else {
            return Err(crate::weapons::SessionRefusal::NoSession);
        };
        Ok(Self {
            session,
            session_id,
            rate,
            producer,
            runtime: OrdnanceRuntime::new(session, tick, rate, producer),
            components: BTreeMap::new(),
            engagements: BTreeMap::new(),
            next_projectile_serial: 0,
            next_event_sequence: 0,
            launch_ids: BTreeSet::new(),
            effects: Vec::new(),
            events: Vec::new(),
            resolved_through: None,
            closed: false,
        })
    }

    /// The session generation this session is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The tick the runtime is positioned at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.runtime.tick()
    }

    /// The declared tick rate the runtime converts time with.
    #[must_use]
    pub const fn rate(&self) -> TickRate {
        self.rate
    }

    /// The runtime, for a consumer that needs the authoritative item state.
    #[must_use]
    pub const fn runtime(&self) -> &OrdnanceRuntime {
        &self.runtime
    }

    /// Whether teardown has closed this session.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// The launch effects emitted so far, oldest first.
    #[must_use]
    pub fn effects(&self) -> &[OrdnanceEffect] {
        &self.effects
    }

    /// Takes every launch effect emitted so far, leaving the log empty.
    pub fn drain_effects(&mut self) -> Vec<OrdnanceEffect> {
        std::mem::take(&mut self.effects)
    }

    /// The network events emitted so far, oldest first.
    #[must_use]
    pub fn events(&self) -> &[OrdnanceNetworkEvent] {
        &self.events
    }

    /// Takes every network event emitted so far, leaving the log empty.
    pub fn drain_events(&mut self) -> Vec<OrdnanceNetworkEvent> {
        std::mem::take(&mut self.events)
    }

    /// Whether a recipient is under one effect kind right now.
    ///
    /// A closed session is under nothing: teardown releases the ledger.
    #[must_use]
    pub fn is_under(&self, target: &StatusEffectTarget, kind: StatusEffectKind) -> bool {
        !self.closed && self.runtime.status().is_under(target, kind)
    }

    /// The engine status one recipient is under: the consumer the flight
    /// model reads for a Propulsion choke, a stall, damage over time or a
    /// marker.
    #[must_use]
    pub fn engine_status(&self, target: &StatusEffectTarget) -> EngineStatus {
        let mut status = EngineStatus::new(*target);
        if self.closed {
            return status;
        }
        let ledger = self.runtime.status();
        let now = ledger.tick();
        for effect in ledger.effects_on(target) {
            if !effect.is_live_at(now) {
                continue;
            }
            match effect.kind {
                StatusEffectKind::Choke => status.choke = status.choke.max(effect.strength),
                StatusEffectKind::Stall => status.stall = true,
                StatusEffectKind::Damage => status.damage += effect.strength,
                StatusEffectKind::Marker => status.marked = true,
            }
        }
        status
    }

    /// Registers one actor's declared loadout.
    ///
    /// This is the stage's *producer* boundary: the components arrive as the
    /// declared records `cs_content::ordnance` describes, each is lowered here
    /// (so an unknown field refuses by name rather than being invented), a
    /// booster is registered with the runtime, and the lowered components are
    /// kept so a launch can resolve against them. Registration is
    /// all-or-nothing: a refused batch registers nothing.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRegistrationError`] for a closed session, an actor from
    /// another generation, a component whose declared record refuses to lower,
    /// a repeated id, or a booster the runtime refused.
    pub fn register(
        &mut self,
        shooter: ActorId,
        declared: &[DeclaredOrdnance],
    ) -> Result<Vec<RegisteredOrdnance>, OrdnanceRegistrationError> {
        if self.closed {
            return Err(OrdnanceRegistrationError::Closed);
        }
        if shooter.session.get() != self.session {
            return Err(OrdnanceRegistrationError::ForeignSession {
                expected: self.session,
                found: shooter.session.get(),
            });
        }
        let mut lowered = Vec::with_capacity(declared.len());
        for record in declared {
            let component = lower_ordnance(record).map_err(OrdnanceRegistrationError::Lower)?;
            let ordnance = component.ordnance().clone();
            let repeated = self
                .components
                .get(&shooter)
                .is_some_and(|existing| existing.contains_key(&ordnance))
                || lowered
                    .iter()
                    .any(|item: &OrdnanceComponent| item.ordnance() == &ordnance);
            if repeated {
                return Err(OrdnanceRegistrationError::DuplicateOrdnance { ordnance });
            }
            lowered.push(component);
        }

        let mut registered = Vec::with_capacity(lowered.len());
        for component in &lowered {
            let media = component.media();
            let mount = component
                .as_projectile()
                .map(|projectile| projectile.launch().mount().clone());
            if let Some(nitro) = component.as_nitro() {
                self.runtime
                    .register_nitro(shooter, *nitro.parameters())
                    .map_err(OrdnanceRegistrationError::Nitro)?;
            }
            registered.push(RegisteredOrdnance {
                ordnance: component.ordnance().clone(),
                mount,
                family: component.family(),
                visual: media.visual().clone(),
                sound: media.sound().clone(),
            });
        }

        let entry = self.components.entry(shooter).or_default();
        for component in lowered {
            entry.insert(component.ordnance().clone(), component);
        }
        Ok(registered)
    }

    /// Opens a tick, refusing a closed session or a tick that is not after the
    /// last one resolved.
    fn begin_tick(&mut self, at: Tick) -> Option<OrdnanceStepRefusal> {
        if self.closed {
            return Some(OrdnanceStepRefusal::Closed);
        }
        if let Some(resolved_through) = self.resolved_through
            && at <= resolved_through
        {
            return Some(OrdnanceStepRefusal::StaleTick {
                resolved_through,
                at,
            });
        }
        self.resolved_through = Some(at);
        None
    }

    /// Ends the session: every live item and its tracker, the routed ledger,
    /// the status ledger and the nitro tables are released, every mirror is
    /// despawned, and no further order, step or registration is accepted.
    ///
    /// The runtime is *dropped and rebuilt empty*, not merely made
    /// unreachable, so an accessor cannot read a stale effect or a spent
    /// capacity out of a session that no longer exists. A restart must build a
    /// new session, and this one carries nothing across it.
    pub fn close(&mut self, world: &mut World) -> OrdnanceTeardownReport {
        self.closed = true;
        let mut projectiles: Vec<ProjectileId> =
            self.runtime.iter().map(|live| live.projectile()).collect();
        projectiles.sort_unstable();
        self.engagements.clear();
        let mirrors = despawn_ordnance_mirrors(world);
        self.runtime =
            OrdnanceRuntime::new(self.session, self.runtime.tick(), self.rate, self.producer);
        OrdnanceTeardownReport {
            projectiles,
            mirrors,
        }
    }
}

// --------------------------------------------------------- engine consumer ---

/// The engine status one recipient is under, derived from the status ledger.
///
/// This is the flight-model consumer AC03 names: a Propulsion
/// [`StatusEffectTarget`]'s choke lowers [`thrust_scale`](Self::thrust_scale),
/// a stall is flagged, damage accumulates and a marker is noted. No particle
/// reaches here — the ledger holds none.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineStatus {
    /// The recipient this status is for.
    pub target: StatusEffectTarget,
    /// The strongest live choke on the recipient, as a fraction of engine
    /// output in `[0, 1]`. `0.0` is not choked.
    pub choke: f64,
    /// Whether a stall is live.
    pub stall: bool,
    /// The summed live damage-over-time amount.
    pub damage: f64,
    /// Whether a marker is live.
    pub marked: bool,
}

impl EngineStatus {
    /// An unchoked status for `target`.
    #[must_use]
    pub const fn new(target: StatusEffectTarget) -> Self {
        Self {
            target,
            choke: 0.0,
            stall: false,
            damage: 0.0,
            marked: false,
        }
    }

    /// The fraction of declared engine output the recipient can still use: a
    /// choke of `0.4` leaves `0.6`. Never negative and never above one.
    #[must_use]
    pub fn thrust_scale(&self) -> f64 {
        (1.0 - self.choke).clamp(0.0, 1.0)
    }

    /// Whether the recipient's engine output is degraded at all.
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        self.choke > 0.0 || self.stall
    }
}

// -------------------------------------------------------------------- step ---

/// Runs one tick of the ordnance path: launch from live poses, guidance,
/// motion, the fuse decisions, the routed damage and status effects, the
/// status expiry and the ECS mirror pass.
///
/// `orders` are applied in the order given. `damage` is the session's damage
/// authority, borrowed rather than owned: every triggered item's routed hits
/// reach it through [`DamageResolver::resolve`], so no ordnance damage is
/// applied anywhere else.
///
/// # What a refusal means here
///
/// A whole-tick refusal ([`OrdnanceStepRefusal`]) means nothing at all
/// happened. An order refusal means that order changed nothing while the rest
/// of the tick ran. An unreadable launcher is named rather than dropped, and
/// the launch that needed it is refused instead of firing from the world
/// origin. An advance refusal leaves every item exactly where it was.
///
/// # Ordering
///
/// Status expiry, then orders, then guidance, then motion, then the fuse
/// decisions, then the mirror pass. An item launched on this tick is moved by
/// the same step, and an item that triggers on this tick applies its effects
/// on this tick — before the next advance retires it. A guided item that loses
/// its target with `Detonate` applies its declared blast on the guidance tick,
/// before the motion advance, at the position the runtime last recorded, so an
/// item that ends by losing its target answers exactly like one that triggers.
#[must_use]
pub fn step_ordnance_session(
    world: &mut World,
    session: &mut OrdnanceSession,
    damage: &mut DamageResolver,
    orders: &[OrdnanceOrder],
    step: &OrdnanceStep<'_>,
) -> OrdnanceSessionTick {
    let mut tick = OrdnanceSessionTick::new(step.at);
    if let Some(refusal) = session.begin_tick(step.at) {
        tick.refused = Some(refusal);
        return tick;
    }

    // Expire the ledger before anything new is applied, so an effect whose
    // boundary is this tick is reported once, in this tick's report, and can
    // never be resurrected by a re-entered step.
    match session.runtime.advance_status(session.session, step.at) {
        Ok(expired) => {
            for effect in expired {
                let event = push_network_event(
                    &mut session.events,
                    &mut session.next_event_sequence,
                    session.session_id,
                    step.at,
                    session.producer,
                    OrdnanceEventKind::StatusExpired {
                        target: effect.target,
                        kind: effect.kind,
                        expired_at: effect.expired_at,
                    },
                );
                tick.events.push(event);
                tick.status_expired.push(effect);
            }
        }
        Err(source) => tick.status_refused = Some(source),
    }

    // One launcher read per actor per tick, shared by every launch order that
    // actor gives, so a mount is never walked twice in a tick.
    let mut reads: BTreeMap<ActorId, LiveMountTransforms> = BTreeMap::new();

    for order in orders {
        match order {
            OrdnanceOrder::Launch(launch) => {
                if !session.launch_ids.insert(launch.id) {
                    tick.orders_refused
                        .push(OrdnanceOrderRefusal::DuplicateLaunch { id: launch.id });
                    continue;
                }
                if launch.shooter.session.get() != session.session {
                    tick.orders_refused.push(OrdnanceOrderRefusal::Runtime(
                        OrdnanceRuntimeError::ForeignSession {
                            expected: session.session,
                            found: launch.shooter.session.get(),
                        },
                    ));
                    continue;
                }
                let Some(component) = session
                    .components
                    .get(&launch.shooter)
                    .and_then(|by_actor| by_actor.get(&launch.ordnance))
                    .cloned()
                else {
                    tick.orders_refused
                        .push(OrdnanceOrderRefusal::UnknownOrdnance {
                            shooter: launch.shooter,
                            ordnance: launch.ordnance.clone(),
                        });
                    continue;
                };
                let Some(projectile_def) = component.as_projectile() else {
                    tick.orders_refused
                        .push(OrdnanceOrderRefusal::NotALauncher {
                            shooter: launch.shooter,
                            ordnance: launch.ordnance.clone(),
                        });
                    continue;
                };

                if let std::collections::btree_map::Entry::Vacant(entry) =
                    reads.entry(launch.shooter)
                {
                    let read = live_launcher_transforms(world, launch.shooter, step.generation);
                    for refusal in &read.refused {
                        tick.unreadable_launchers.push(UnreadableLauncher {
                            shooter: launch.shooter,
                            refusal: refusal.clone(),
                        });
                    }
                    entry.insert(read);
                }
                let mounts = reads
                    .get(&launch.shooter)
                    .expect("the launcher read was just inserted");
                let mount = projectile_def.launch().mount();
                let Some(transform) = mounts.get(mount) else {
                    tick.orders_refused
                        .push(OrdnanceOrderRefusal::MissingMountTransform {
                            shooter: launch.shooter,
                            mount: mount.clone(),
                        });
                    continue;
                };

                let projectile = ProjectileId {
                    session: session.session,
                    serial: session.next_projectile_serial,
                };
                match session.runtime.launch(
                    launch.shooter,
                    projectile,
                    projectile_def,
                    transform,
                    launch.target,
                    step.wind_velocity_m_s,
                ) {
                    Ok(projectile) => {
                        session.next_projectile_serial =
                            session.next_projectile_serial.wrapping_add(1);
                        session
                            .engagements
                            .insert(projectile, launch.engagement.clone());
                        let effect = OrdnanceEffect {
                            projectile,
                            shooter: launch.shooter,
                            ordnance: component.ordnance().clone(),
                            at: step.at,
                            origin: transform.origin,
                            visual: component.media().visual().clone(),
                            sound: component.media().sound().clone(),
                        };
                        session.effects.push(effect.clone());
                        tick.effects.push(effect);
                        tick.launched.push(projectile);
                        let event = push_network_event(
                            &mut session.events,
                            &mut session.next_event_sequence,
                            session.session_id,
                            step.at,
                            session.producer,
                            OrdnanceEventKind::Launched {
                                shooter: launch.shooter,
                                projectile,
                                ordnance: component.ordnance().clone(),
                            },
                        );
                        tick.events.push(event);
                    }
                    Err(source) => tick
                        .orders_refused
                        .push(OrdnanceOrderRefusal::Runtime(source)),
                }
            }
            OrdnanceOrder::Nitro { shooter, requested } => {
                if shooter.session.get() != session.session {
                    tick.orders_refused.push(OrdnanceOrderRefusal::Runtime(
                        OrdnanceRuntimeError::ForeignSession {
                            expected: session.session,
                            found: shooter.session.get(),
                        },
                    ));
                    continue;
                }
                match session.runtime.request_nitro(shooter, step.at, *requested) {
                    Ok(nitro) => {
                        if nitro.is_active() {
                            let event = push_network_event(
                                &mut session.events,
                                &mut session.next_event_sequence,
                                session.session_id,
                                step.at,
                                session.producer,
                                OrdnanceEventKind::Nitro {
                                    shooter: *shooter,
                                    active: true,
                                    extra_thrust_n: nitro.extra_thrust_n,
                                    consumed_units: nitro.consumed_units,
                                },
                            );
                            tick.events.push(event);
                        }
                        tick.nitro.push((*shooter, nitro));
                    }
                    Err(source) => tick
                        .orders_refused
                        .push(OrdnanceOrderRefusal::Runtime(source)),
                }
            }
        }
    }

    if let Some(observation) = step.guidance {
        let guidance = session.runtime.guidance_tick(session.session, observation);
        for detonation in &guidance.detonated {
            // A `Detonate` loss removes the item from the live set, so its
            // blast cannot go through the trigger path. It carries the
            // declared behavior and the position the runtime last recorded;
            // the engagement the producer named at launch names where the
            // damage lands and who takes the status, exactly as for a trigger.
            let projectile = detonation.projectile();
            let Some(engagement) = session.engagements.remove(&projectile) else {
                tick.routing_refused
                    .push(OrdnanceRoutingRefusal::MissingEngagement { projectile });
                continue;
            };
            let mut blast = OrdnanceGuidanceBlast {
                projectile,
                shooter: detonation.shooter(),
                ordnance: detonation.ordnance().clone(),
                position: detonation.position(),
                hits: Vec::new(),
                damage: None,
                damage_refused: None,
                status_applied: Vec::new(),
                status_refused: None,
            };
            match session.runtime.route_detonation(
                session.session,
                step.at,
                detonation,
                engagement.damage_target,
                engagement.node.clone(),
            ) {
                Ok(hits) => {
                    blast.hits.clone_from(&hits);
                    match damage.resolve(step.at, &hits) {
                        Ok(resolution) => blast.damage = Some(resolution),
                        Err(source) => blast.damage_refused = Some(source),
                    }
                    match session.runtime.apply_detonation_statuses(
                        session.session,
                        step.at,
                        detonation,
                        engagement.status_recipient,
                    ) {
                        Ok(instances) => {
                            for instance in instances {
                                let Some(active) = session.runtime.status().get(&instance).cloned()
                                else {
                                    continue;
                                };
                                blast.status_applied.push(active.instance);
                                let event = push_network_event(
                                    &mut session.events,
                                    &mut session.next_event_sequence,
                                    session.session_id,
                                    step.at,
                                    session.producer,
                                    OrdnanceEventKind::StatusApplied {
                                        target: active.target,
                                        kind: active.kind,
                                        expires_at: active.expires_at,
                                    },
                                );
                                tick.events.push(event);
                            }
                        }
                        Err(source) => blast.status_refused = Some(source),
                    }
                    let event = push_network_event(
                        &mut session.events,
                        &mut session.next_event_sequence,
                        session.session_id,
                        step.at,
                        session.producer,
                        OrdnanceEventKind::GuidanceDetonated {
                            shooter: detonation.shooter(),
                            projectile,
                            target: engagement.damage_target,
                        },
                    );
                    tick.events.push(event);
                    tick.guidance_blasts.push(blast);
                }
                Err(source) => tick
                    .routing_refused
                    .push(OrdnanceRoutingRefusal::Runtime(source)),
            }
        }
        tick.guidance = Some(guidance);
    }

    match session.runtime.advance(step.dt_s, step.wind_velocity_m_s) {
        Err(source) => tick.advance_refused = Some(source),
        Ok(advanced) => {
            tick.retired = advanced.triggered;
            tick.expired = advanced.expired;

            // The fuse decision runs on every item that is still live after the
            // advance. An item that triggers here stays live until the next
            // advance retires it, so its effects are applied on this tick.
            let live: Vec<ProjectileId> = session
                .runtime
                .iter()
                .map(|item| item.projectile())
                .collect();
            for projectile in live {
                let decision = session
                    .runtime
                    .decide(&projectile, step.targets, step.impacts);
                let trigger = match decision {
                    Ok(FuseDecision::Triggered(trigger)) => trigger,
                    Ok(FuseDecision::Inert(_)) => continue,
                    Err(source) => {
                        tick.routing_refused
                            .push(OrdnanceRoutingRefusal::Runtime(source));
                        continue;
                    }
                };
                let Some(engagement) = session.engagements.get(&projectile).cloned() else {
                    tick.routing_refused
                        .push(OrdnanceRoutingRefusal::MissingEngagement { projectile });
                    continue;
                };
                let Some(shooter) = session.runtime.get(&projectile).map(|item| item.shooter())
                else {
                    continue;
                };

                let mut detonation = OrdnanceDetonation {
                    projectile,
                    shooter,
                    trigger,
                    engagement: engagement.clone(),
                    hits: Vec::new(),
                    damage: None,
                    damage_refused: None,
                    status_applied: Vec::new(),
                    status_refused: None,
                };

                match session.runtime.route_trigger(
                    session.session,
                    step.at,
                    &projectile,
                    engagement.damage_target,
                    engagement.node.clone(),
                ) {
                    Ok(hits) => {
                        detonation.hits.clone_from(&hits);
                        match damage.resolve(step.at, &hits) {
                            Ok(resolution) => detonation.damage = Some(resolution),
                            Err(source) => detonation.damage_refused = Some(source),
                        }
                        match session.runtime.apply_statuses(
                            session.session,
                            step.at,
                            &projectile,
                            engagement.status_recipient,
                        ) {
                            Ok(instances) => {
                                for instance in instances {
                                    let Some(active) =
                                        session.runtime.status().get(&instance).cloned()
                                    else {
                                        continue;
                                    };
                                    detonation.status_applied.push(active.instance);
                                    let event = push_network_event(
                                        &mut session.events,
                                        &mut session.next_event_sequence,
                                        session.session_id,
                                        step.at,
                                        session.producer,
                                        OrdnanceEventKind::StatusApplied {
                                            target: active.target,
                                            kind: active.kind,
                                            expires_at: active.expires_at,
                                        },
                                    );
                                    tick.events.push(event);
                                }
                            }
                            Err(source) => detonation.status_refused = Some(source),
                        }

                        let event = push_network_event(
                            &mut session.events,
                            &mut session.next_event_sequence,
                            session.session_id,
                            step.at,
                            session.producer,
                            OrdnanceEventKind::Triggered {
                                shooter,
                                projectile,
                                target: engagement.damage_target,
                            },
                        );
                        tick.events.push(event);
                        tick.detonations.push(detonation);
                    }
                    Err(source) => tick
                        .routing_refused
                        .push(OrdnanceRoutingRefusal::Runtime(source)),
                }
            }
        }
    }

    // An item that retires has no engagement left to route, so its record goes
    // with it: the session keeps one only while the item that needs it is live.
    // Its end was already announced as a `Triggered` event on the tick its fuse
    // fired, so only a lifetime expiry — an item that ended *without* a trigger
    // — emits an `Expired` event here.
    for projectile in tick.retired.iter().chain(tick.expired.iter()) {
        session.engagements.remove(projectile);
    }
    for projectile in &tick.expired {
        let event = push_network_event(
            &mut session.events,
            &mut session.next_event_sequence,
            session.session_id,
            step.at,
            session.producer,
            OrdnanceEventKind::Expired {
                projectile: *projectile,
            },
        );
        tick.events.push(event);
    }

    tick.mirrors = sync_ordnance_mirrors(world, session, step.generation);
    tick
}

// ------------------------------------------------------- live launcher read ---

/// Reads one actor's live launcher poses from the ECS hierarchy.
///
/// `actor` and `generation` select the actor root: the entity carrying a
/// matching [`OrdnanceLauncherBinding`]. Every descendant carrying a
/// [`MountPoseBinding`] becomes a [`MountTransform`] whose origin is the
/// node's composed [`NodeVisualTransform`] translation and whose inherited
/// velocity is the airframe body's live `LinearVelocity`. A mount whose
/// binding is stale, has no pose, a non-finite origin, a zero forward axis or
/// no reachable airframe velocity is **refused by name** rather than read.
#[must_use]
pub fn live_launcher_transforms(
    world: &World,
    actor: ActorId,
    generation: SceneGeneration,
) -> LiveMountTransforms {
    let Some(root) = launcher_actor_root(world, actor, generation) else {
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

/// The live launcher root whose hierarchy a read starts from: the entity
/// stamped with a matching [`OrdnanceLauncherBinding`], or `None` when no live
/// root exists for `generation`.
fn launcher_actor_root(
    world: &World,
    actor: ActorId,
    generation: SceneGeneration,
) -> Option<Entity> {
    world.iter_entities().find_map(|entity_ref| {
        let binding = entity_ref.get::<OrdnanceLauncherBinding>()?;
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

/// The live airframe velocity for a launcher node: the first `LinearVelocity`
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

// ------------------------------------------------------------------ mirrors ---

/// Component: the ECS mirror of one authoritative in-flight ordnance item.
///
/// The mirror carries no collider and is never integrated: the authoritative
/// position lives in [`cs_sim::weapons::ordnance::LiveOrdnance`], so Avian
/// never becomes a second contact authority for an item (FLIGHT-PHYSICS, "one
/// physics pose owner"). The fields are exactly what the item names — its id,
/// who launched it and the generation it was stamped under — so a reloaded
/// scene's orphan is identifiable by mismatch, never by a surviving pointer.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct OrdnanceItemMirror {
    /// The authoritative item this entity mirrors.
    pub projectile: ProjectileId,
    /// Who launched it.
    pub shooter: ActorId,
    /// The scene generation the mirror was stamped under.
    pub generation: SceneGeneration,
}

/// Reconciles the ECS mirror of the authoritative items.
///
/// Every live item gets a mirror entity whose [`Transform`] is written from
/// the runtime's own position, and a mirror whose item is gone — retired,
/// expired, removed by teardown, or left behind by a reloaded scene — is
/// despawned. The pass *reconciles* rather than tracks: it looks for what is
/// there and for what is live, so no entity id is stored and a stale mirror
/// cannot survive a reload by pointing at the past.
#[must_use]
pub fn sync_ordnance_mirrors(
    world: &mut World,
    session: &OrdnanceSession,
    generation: SceneGeneration,
) -> OrdnanceMirrorReport {
    let mut report = OrdnanceMirrorReport::default();
    let live: BTreeMap<ProjectileId, (ActorId, WorldPosition)> = session
        .runtime
        .iter()
        .map(|item| (item.projectile(), (item.shooter(), item.current())))
        .collect();

    let mut present: BTreeMap<ProjectileId, Entity> = BTreeMap::new();
    let mut stale: Vec<(Entity, ProjectileId)> = Vec::new();
    for entity_ref in world.iter_entities() {
        let Some(mirror) = entity_ref.get::<OrdnanceItemMirror>() else {
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
                world
                    .entity_mut(*entity)
                    .insert(mirror_transform(*position));
                report.moved += 1;
            }
            None => {
                world.spawn((
                    OrdnanceItemMirror {
                        projectile: *projectile,
                        shooter: *shooter,
                        generation,
                    },
                    mirror_transform(*position),
                ));
                report.spawned.push(*projectile);
            }
        }
    }
    report
}

/// The mirror [`Transform`] for an authoritative item position.
///
/// Position only: an item's orientation is not measured, so the mirror carries
/// none rather than a default rotation that would read as a claim. The f32
/// conversion is the ECS's own precision; the authoritative position stays f64
/// in the runtime and every mirror write comes from it.
fn mirror_transform(position: WorldPosition) -> Transform {
    let [x, y, z] = position.to_array();
    Transform::from_translation(bevy::prelude::Vec3::new(x as f32, y as f32, z as f32))
}

/// Despawns every [`OrdnanceItemMirror`] entity, returning how many there were.
fn despawn_ordnance_mirrors(world: &mut World) -> usize {
    let mirrors: Vec<Entity> = world
        .iter_entities()
        .filter(|entity_ref| entity_ref.contains::<OrdnanceItemMirror>())
        .map(|entity_ref| entity_ref.id())
        .collect();
    for entity in &mirrors {
        world.entity_mut(*entity).despawn();
    }
    mirrors.len()
}
