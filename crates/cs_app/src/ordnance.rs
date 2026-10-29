//! The ordnance application boundary (F28-A/C) and the live-session catalogue
//! audit (F28-D).
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stages `### F28-A`, `### F28-C` and `### F28-D`. Shared contract:
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
//! records the ECS wiring consumes (F28-C). The one exception is the measured
//! tangler field ledger ([`TanglerLedger`], task #2196): its vocabulary is
//! `cs_content`'s ([`cs_content::ordnance::TanglerFieldDeclaration`]) and
//! `cs_sim` may not depend on `cs_content`, so the ledger that applies it per
//! tick lives at this boundary, which sees both. Task #2186 gives that ledger
//! a production producer: a registered component with a measured
//! [`measured_tangler_field`] opens its field at fire time from the launch
//! order's own pose, and a recorded `wep_12` registers with that field instead
//! of the one-shot bounded area the runtime's `AreaEffect` expresses.
//!
//! [`session_ordnance_audit`] is the one reader that walks a whole live
//! session at once: it reports, per registered component, which channels reach
//! a consumer and which declared fields reach nothing, and per booster the
//! declared numbers the flight model will read (F28-D). It reports and never
//! repairs — the area effect whose radius no production path reads is named,
//! not silently implemented.

use std::collections::{BTreeMap, BTreeSet};

use bevy::ecs::component::Component;
use cs_content::ordnance::{
    DeclaredAreaEffect, DeclaredArmingRule, DeclaredEquipmentRules, DeclaredFuseRule,
    DeclaredGuidanceRule, DeclaredHardpointKind, DeclaredInheritanceRule, DeclaredLaunchGeometry,
    DeclaredLostTargetBehavior, DeclaredNitroActivationRule, DeclaredNitroParameters,
    DeclaredNitroRefusedRecovery, DeclaredOrdnance, DeclaredOrdnanceDetails,
    DeclaredOrdnanceFamily, DeclaredProjectile, DeclaredProximityFuse, DeclaredStackLoad,
    DeclaredStatusEffect, OriginalComponentLaunch, TangleStatus, TanglerFieldDeclaration,
};
use cs_sim::damage::{ActorId, DamageNodeKey, NodeKeyError};
use cs_sim::weapons::{
    AreaEffect, ArmingRule, BlastRecipient, CatalogueError, CatalogueUnknown, CataloguedArming,
    CataloguedFuse, CataloguedGuidance, CataloguedMount, CataloguedOrdnance, CompatibilityVerdict,
    EquipmentRules, FuseRule, GuidanceRule, GunDefinitionError, HardpointKind, InheritanceRule,
    LaunchGeometry, LostTargetBehavior, NitroActivationRule, NitroOrdnance, NitroParameters,
    NitroRefusedRecovery, NitroTradeoffs, OrdnanceComponent, OrdnanceDefinitionError,
    OrdnanceFamily, OrdnanceId, OrdnanceIdError, OrdnanceMedia, OrdnanceRegistry,
    OrdnanceRegistryError, OrdnanceStatusEffect, ProjectileOrdnance, ProximityFuse,
    ProximitySampling, ProximityShape, StackLoad, StatusEffectKind, WEAPON_DAMAGE_CHANNELS,
    WeaponDamage,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, Transform, World};
use cs_sim::damage::{DamageError, DamageResolver, HitEvent, TickResolution};
use cs_sim::time::TickRate;
use cs_sim::weapons::{
    AreaInstanceId, ExpiredArea, ExpiredStatusEffect, FuseDecision, FuseTrigger, GuidanceTick,
    MountTransform, NitroTick, OrdnanceRuntime, OrdnanceRuntimeError, ProjectileId,
    StatusEffectInstanceId, StatusEffectTarget, SweptHit, TargetObservation, TargetPath,
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
    let acceleration_mps2 =
        known_or_refuse("launch.acceleration_mps2", &declared.acceleration_mps2)?;
    let instant_range_m = known_or_refuse("launch.instant_range_m", &declared.instant_range_m)?;
    let release_delay_ticks =
        known_or_refuse("launch.release_delay_ticks", &declared.release_delay_ticks)?;
    LaunchGeometry::try_new(
        mount,
        hardpoint,
        launch_speed_mps,
        inheritance,
        release_delay_ticks,
        acceleration_mps2,
        instant_range_m,
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
        DeclaredFuseRule::ExpiresAtRange { range_m } => FuseRule::ExpiresAtRange {
            range_m: known_or_refuse("fuse.range_m", range_m)?,
        },
    })
}

/// Whether the measured record installs the `BEEPER_SEEKER` callback.
fn is_marker_seeker(row: &cs_content::ordnance::OriginalComponentLaunch) -> bool {
    cs_content::ordnance::measure_component_behavior(row)
        .effects
        .iter()
        .any(|effect| effect.action == cs_content::ordnance::MeasuredEffectAction::GuidanceModifier)
}

/// Lowers a declared guidance rule.
///
/// The lost-target behavior refuses to lower while unknown: F28
/// non-negotiable 4 requires the behavior to be *specified*, so there is no
/// default that would let a seeker coast or detonate by accident.
fn lower_guidance(declared: &DeclaredGuidanceRule) -> Result<GuidanceRule, OrdnanceLowerError> {
    Ok(match declared {
        DeclaredGuidanceRule::Unguided => GuidanceRule::Unguided,
        DeclaredGuidanceRule::MarkerSeeker => GuidanceRule::MarkerSeeker,
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
        DeclaredLostTargetBehavior::CoastThenDetonate => LostTargetBehavior::CoastThenDetonate,
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
    let sound = declared
        .sound
        .as_ref()
        .map(|resolved| known_or_refuse("media.sound", resolved))
        .transpose()?;
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
    let recovery_on_refused =
        match known_or_refuse("nitro.recovery_on_refused", &parameters.recovery_on_refused)? {
            DeclaredNitroRefusedRecovery::Recovers => NitroRefusedRecovery::Recovers,
            DeclaredNitroRefusedRecovery::Withheld => NitroRefusedRecovery::Withheld,
        };
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
    .map_err(OrdnanceLowerError::Definition)?
    .with_recovery_on_refused(recovery_on_refused);
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
        DeclaredNitroActivationRule::UntilCharge {
            start_min_fraction,
            stop_below_fraction,
        } => NitroActivationRule::UntilCharge {
            start_min_fraction: known_or_refuse(
                "nitro.activation.start_min_fraction",
                &start_min_fraction,
            )?,
            stop_below_fraction: known_or_refuse(
                "nitro.activation.stop_below_fraction",
                &stop_below_fraction,
            )?,
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
        DeclaredInheritanceRule::LateralRemainder => InheritanceRule::LateralRemainder,
        DeclaredInheritanceRule::Instant => InheritanceRule::Instant,
    }
}

/// Lowers a declared status effect kind.
fn lower_status_kind(declared: cs_content::ordnance::DeclaredStatusEffectKind) -> StatusEffectKind {
    match declared {
        cs_content::ordnance::DeclaredStatusEffectKind::Damage => StatusEffectKind::Damage,
        cs_content::ordnance::DeclaredStatusEffectKind::Choke => StatusEffectKind::Choke,
        cs_content::ordnance::DeclaredStatusEffectKind::Stall => StatusEffectKind::Stall,
        cs_content::ordnance::DeclaredStatusEffectKind::Marker => StatusEffectKind::Marker,
        cs_content::ordnance::DeclaredStatusEffectKind::Incapacitation => {
            StatusEffectKind::Incapacitation
        }
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

/// The [`SceneNodeId`](cs_content::scene::SceneNodeId) a declared component's
/// visual launcher binding names,
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
    /// The declared launch-sound catalog id, `None` for a measured silence.
    pub sound: Option<ContentId>,
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
    /// The session's supported catalogue holds no component with this id, so
    /// an import naming it cannot bypass the shop (F28 non-negotiable 5).
    Unsupported(OrdnanceRegistryError),
    /// The shared equipment rule refuses this installation on the airframe.
    Incompatible {
        /// The component that cannot be installed.
        ordnance: OrdnanceId,
        /// The verdict, naming the missing or forbidden equipment.
        verdict: CompatibilityVerdict,
    },
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
            Self::Unsupported(source) => {
                write!(f, "the supported ordnance catalogue refused it: {source}")
            }
            Self::Incompatible { ordnance, verdict } => {
                write!(
                    f,
                    "{ordnance} cannot be installed on this airframe: {verdict}"
                )
            }
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
    /// The component's declared launch-sound catalog id, `None` when it
    /// declares none and the launch plays nothing.
    pub sound: Option<ContentId>,
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
    /// An item reached the measured end of its record's `RANGE` and was
    /// consumed: no detonation, no area, no damage.
    Consumed {
        /// The item.
        projectile: ProjectileId,
        /// The `RANGE` it reached, in meters.
        range_m: f64,
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
    /// A measured tangler field tangled an actor: the start of one episode.
    ///
    /// The measured field re-applies its hit every tick, but a re-hit that only
    /// raises a live timer is the same continuous status, so it is not
    /// announced again.
    TangleStarted {
        /// The tangled actor.
        target: ActorId,
        /// The field that tangled it.
        field: TanglerFieldId,
        /// The tick the timer clears on.
        expires_at: Tick,
    },
    /// A tangle timer reached its expiry boundary.
    TangleExpired {
        /// The actor that cleared.
        target: ActorId,
        /// The field that last applied it.
        field: TanglerFieldId,
        /// The tick it cleared on.
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

/// One tangler field opened by an explicit order on this tick (task #2196).
///
/// A registered component opens its field itself, from its own launch order
/// (`OrdnanceSession::register_tangler_field`, task #2186); this order is the
/// other producer, for a fire path that knows a record's measured field but
/// does not register the component. Both open the same measured declaration
/// (task #2185).
///
/// The centre is the fire path's, not the ledger's: see
/// [`T452_REG_TANGLER_FIELD_ORIGIN_CLAIM`].
#[derive(Clone, Debug, PartialEq)]
pub struct TanglerFieldOrder {
    /// The declaration the record lowered to.
    pub declaration: TanglerFieldDeclaration,
    /// The actor whose weapon opened it. It is hit like every other actor
    /// inside the field.
    pub owner: ActorId,
    /// Where the fire path centres it, world metres.
    pub centre: WorldPosition,
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
    /// Open one measured tangler field at fire time.
    ///
    /// A replayed order opens a second field: unlike a launch, the order
    /// carries no once-only request id, because the fire path that produces it
    /// owns its own ledger.
    Tangler(TanglerFieldOrder),
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
    /// The launcher's stack is spent: every declared unit was launched.
    StackExhausted {
        /// The actor that was named.
        shooter: ActorId,
        /// The component whose stack is spent.
        ordnance: OrdnanceId,
    },
    /// The runtime refused the order: a foreign session, a duplicate item, a
    /// non-finite wind or velocity, or an unknown booster.
    Runtime(OrdnanceRuntimeError),
    /// The tangler ledger refused the field the order asked to open.
    Tangler(TanglerFieldRefusal),
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
            Self::StackExhausted { shooter, ordnance } => {
                write!(f, "{shooter}'s {ordnance} stack is spent")
            }
            Self::Runtime(source) => write!(f, "the runtime refused the order: {source}"),
            Self::Tangler(source) => write!(f, "the tangler ledger refused the order: {source}"),
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
    /// The area the trigger opened, when the component declares one.
    pub area: Option<AreaInstanceId>,
    /// Why the area could not be opened, when it could not.
    pub area_refused: Option<OrdnanceRuntimeError>,
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
    /// The area the blast opened, when the component declares one.
    pub area: Option<AreaInstanceId>,
    /// Why the area could not be opened, when it could not.
    pub area_refused: Option<OrdnanceRuntimeError>,
}

/// Why a triggered item's effects could not be routed.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceRoutingRefusal {
    /// The session holds a triggered item with no recorded engagement.
    MissingEngagement {
        /// The item that could not be routed.
        projectile: ProjectileId,
    },
    /// A fuse `Impact`/`Proximity` trigger named a victim the producer's
    /// [`OrdnanceStep::victim_nodes`] table binds no damage node for. The
    /// detonation is refused rather than aimed at the engagement's
    /// designated target.
    UnboundVictim {
        /// The item that triggered.
        projectile: ProjectileId,
        /// The victim the trigger reached.
        target: ActorId,
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
            Self::UnboundVictim { projectile, target } => write!(
                f,
                "item {projectile} reached victim {target} but no damage node is bound for it"
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

/// One item the measured end of `RANGE` consumed.
///
/// This is not a detonation: the item reached the `RANGE` of its record's
/// [`FuseRule::ExpiresAtRange`](cs_sim::weapons::FuseRule::ExpiresAtRange), so
/// nothing detonates, no area opens and no damage is routed. It is not a
/// lifetime expiry either — the runtime reports the measured end it actually
/// reached, with the bound, rather than only the designed cap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrdnanceConsumption {
    /// The item that ended.
    pub projectile: ProjectileId,
    /// The record's `RANGE` it reached, in meters.
    pub range_m: f64,
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
    /// Items the measured end of their record's `RANGE` consumed this tick:
    /// they reached the bound and were removed without a detonation, an area
    /// or damage.
    pub consumed: Vec<OrdnanceConsumption>,
    /// Status effects that reached their expiry boundary this tick.
    pub status_expired: Vec<ExpiredStatusEffect>,
    /// The status advance was refused whole.
    pub status_refused: Option<OrdnanceRuntimeError>,
    /// Areas that reached their expiry boundary this tick.
    pub areas_expired: Vec<ExpiredArea>,
    /// The area advance was refused whole.
    pub areas_refused: Option<OrdnanceRuntimeError>,
    /// Tangler fields that reached their expiry boundary this tick.
    pub tangler_expired: Vec<ExpiredTanglerField>,
    /// The tangler fields opened this tick, in order: one per `Tangler` order,
    /// plus the field a launch opens at fire time for a component registered
    /// with [`OrdnanceSession::register_tangler_field`].
    pub tangler_opened: Vec<TanglerFieldId>,
    /// One entry per live tangler field this tick, with every victim it
    /// re-tangled. An empty `applied` list is "nobody was inside".
    pub tangler: Vec<TanglerFieldTick>,
    /// Tangle timers that began this tick, one per episode.
    pub tangles_started: Vec<TangleTransition>,
    /// Tangle timers that cleared this tick.
    pub tangles_expired: Vec<ExpiredTangle>,
    /// The tangler ledger refused this tick's field pass whole.
    pub tangler_refused: Option<TanglerFieldRefusal>,
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
    /// Every actor node a blast may reach, with where it is this tick (the sim
    /// holds no actor list). A guidance-loss, timed, proximity or impact
    /// detonation whose item declares an
    /// [`AreaEffect`] routes through
    /// [`OrdnanceRuntime::route_detonation_area`] over these: each recipient
    /// inside the radius takes the declared channels scaled by the measured
    /// falloff, and the engagement's single damage target is not consulted. On
    /// the impact path the directly-hit actor is skipped, as in the original.
    /// `None` is "the producer supplies no recipients": the blast goes to the
    /// launch engagement's single target, as for an item with no area. `Some`
    /// of an empty slice is "nobody is there" and delivers no damage.
    pub blast_recipients: Option<&'a [BlastRecipient]>,
    /// The damage node each fuse victim's hits name, when the producer binds
    /// one per actor: an `Impact` or `Proximity` trigger names the actor it
    /// actually reached, and that actor's hits — and its status effects —
    /// route to its own registered node here rather than the launch
    /// engagement's designated target, which may be a different actor
    /// entirely. A triggered victim absent from the map is an
    /// [`OrdnanceRoutingRefusal::UnboundVictim`], not a guess. A `Timed`
    /// trigger names no victim and still routes through the engagement.
    /// `None` keeps the single-target behavior: every trigger routes to the
    /// engagement's `damage_target`/`node` unchanged.
    pub victim_nodes: Option<&'a BTreeMap<ActorId, DamageNodeKey>>,
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
    catalogue: OrdnanceRegistry,
    components: BTreeMap<ActorId, BTreeMap<OrdnanceId, OrdnanceComponent>>,
    engagements: BTreeMap<ProjectileId, OrdnanceEngagement>,
    next_projectile_serial: u64,
    next_event_sequence: u32,
    launch_ids: BTreeSet<OrdnanceLaunchId>,
    launched_units: BTreeMap<(ActorId, OrdnanceId), u64>,
    effects: Vec<OrdnanceEffect>,
    events: Vec<OrdnanceNetworkEvent>,
    /// The measured tangler fields and the tangle timers they leave on actors
    /// (task #2196). Session-confined like every other ledger here.
    tangler: TanglerLedger,
    /// The measured tangler field each registered component opens at fire time
    /// (task #2186), keyed by the component's id. A launch of a keyed id opens
    /// that field from the launch order's own pose; an id with no entry opens
    /// none.
    tangler_fields: BTreeMap<OrdnanceId, TanglerFieldDeclaration>,
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
    /// `catalogue` is the supported set every registration is resolved
    /// against. There is no default: an empty catalogue supports nothing and
    /// refuses every registration, so a session cannot be opened that quietly
    /// accepts whatever an import names.
    ///
    /// # Errors
    ///
    /// [`SessionRefusal::NoSession`](crate::weapons::SessionRefusal::NoSession)
    /// for session generation zero.
    pub fn new(
        session: u64,
        tick: Tick,
        rate: TickRate,
        producer: u32,
        catalogue: OrdnanceRegistry,
    ) -> Result<Self, crate::weapons::SessionRefusal> {
        let Some(session_id) = SessionId::new(session) else {
            return Err(crate::weapons::SessionRefusal::NoSession);
        };
        Ok(Self {
            session,
            session_id,
            rate,
            producer,
            runtime: OrdnanceRuntime::new(session_id, tick, rate, producer),
            catalogue,
            components: BTreeMap::new(),
            engagements: BTreeMap::new(),
            next_projectile_serial: 0,
            next_event_sequence: 0,
            launch_ids: BTreeSet::new(),
            launched_units: BTreeMap::new(),
            effects: Vec::new(),
            events: Vec::new(),
            tangler: TanglerLedger::new(session_id, tick, rate),
            tangler_fields: BTreeMap::new(),
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

    /// Every component registered for one actor, in ascending id order.
    ///
    /// A closed session holds none, and it holds none because
    /// [`close`](Self::close) released the lowered loadout with the runtime:
    /// teardown clears `components`, so an audit or a loadout check cannot read
    /// a fireable component out of a session that no longer exists. The
    /// invariant is deliberately *not* re-checked here, so the teardown has one
    /// place that establishes it and this accessor cannot disagree with it.
    #[must_use]
    pub fn registered_ids(&self, shooter: &ActorId) -> Vec<OrdnanceId> {
        self.components
            .get(shooter)
            .map(|by_actor| by_actor.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// The units left in one launcher's stack: its declared capacity less the
    /// launches this session accepted.
    ///
    /// Designed semantics: one accepted launch spends one unit of the
    /// declared [`StackLoad`] capacity; whether the original ever spends more
    /// than one per launch is unmeasured. `None` for a component that is not
    /// registered, is a booster (no stack) or belongs to a closed session.
    #[must_use]
    pub fn stack_remaining(&self, shooter: &ActorId, ordnance: &OrdnanceId) -> Option<StackStock> {
        let capacity_units = self
            .components
            .get(shooter)?
            .get(ordnance)?
            .as_projectile()?
            .stack()
            .capacity_units();
        let launched = self
            .launched_units
            .get(&(*shooter, ordnance.clone()))
            .copied()
            .unwrap_or(0);
        Some(StackStock {
            capacity_units,
            remaining_units: capacity_units.saturating_sub(launched),
        })
    }

    /// Every actor with a component registered, in ascending actor order.
    ///
    /// Empty for a closed session, for the same reason and by the same single
    /// mechanism as [`registered_ids`](Self::registered_ids).
    #[must_use]
    pub fn registered_shooters(&self) -> Vec<ActorId> {
        self.components.keys().copied().collect()
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
                StatusEffectKind::Incapacitation => status.incapacitated = true,
            }
        }
        status
    }

    /// The tangle timer on one actor, when it is tangled right now.
    ///
    /// This is the flight-model consumer of the measured tangler status: the
    /// original suppresses the force scalar `[ebp-8]` of `0x48fc40` while the
    /// status is live, and what that scalar *is* stays the named claim
    /// [`T452_REG_TANGLER_FORCE_SCALAR`](cs_content::ordnance::T452_REG_TANGLER_FORCE_SCALAR).
    /// The timer is whole simulation ticks, so a consumer never reads a
    /// render-frame duration.
    ///
    /// A closed session has tangled nobody: [`close`](Self::close) rebuilds
    /// the ledger empty with the runtime.
    #[must_use]
    pub fn tangle_status(&self, target: &ActorId) -> Option<TangleState> {
        self.tangler.tangled(target).copied()
    }

    /// Whether one actor is tangled right now.
    #[must_use]
    pub fn is_tangled(&self, target: &ActorId) -> bool {
        self.tangle_status(target).is_some()
    }

    /// Whether one actor may boost right now, as the measured tangle rules it.
    ///
    /// The measured fact (`docs/findings/2026-10-28-t452-tangler-effect.md`,
    /// `[[tangler_fact]]` `tangler.boost_refuse`): the boost setter `0x4b2110`
    /// refuses to enable while the tangle flag is set, and the status routine
    /// calls it with `false` whenever the flag is touched, so a tangle refuses
    /// a boost and ends one in progress. This is the measured half of that
    /// consequence in the narrowest form the runtime can carry: a flight
    /// producer passes it to `FlightState::boost_available`. What a tangle does
    /// to the rest of a flight model — the force scalar's physical identity —
    /// stays the named claim
    /// [`T452_REG_TANGLER_FORCE_SCALAR`](cs_content::ordnance::T452_REG_TANGLER_FORCE_SCALAR).
    #[must_use]
    pub fn boost_allowed(&self, target: &ActorId) -> bool {
        !self.is_tangled(target)
    }

    /// Every tangled actor right now, in ascending actor order.
    #[must_use]
    pub fn tangled(&self) -> Vec<TangleState> {
        self.tangler.tangles().copied().collect()
    }

    /// The open tangler fields, in ascending id order.
    #[must_use]
    pub fn tangler_fields(&self) -> Vec<OpenTanglerField> {
        self.tangler.iter().cloned().collect()
    }

    /// The tangler ledger, for a consumer that needs the whole of it.
    #[must_use]
    pub const fn tangler(&self) -> &TanglerLedger {
        &self.tangler
    }

    /// The measured tangler field one component opens at fire time, if it opens
    /// one.
    #[must_use]
    pub fn tangler_field_for(&self, ordnance: &OrdnanceId) -> Option<&TanglerFieldDeclaration> {
        self.tangler_fields.get(ordnance)
    }

    /// Every component that opens a measured tangler field, in ascending id
    /// order.
    #[must_use]
    pub fn tangler_field_ids(&self) -> Vec<OrdnanceId> {
        self.tangler_fields.keys().cloned().collect()
    }

    /// Declares that a component opens `declaration`'s measured tangler field
    /// when it is launched (task #2186).
    ///
    /// This is the fire-time producer: the measured field is created at fire
    /// time by the record's own fire path (T452-R.02 §1, `0x4b94e0`), so the
    /// launch order that fires the component is the one place a field can
    /// honestly open. The opening tick and the centre are the launch's, never
    /// a clock or a pose invented here.
    ///
    /// The declaration must be the measured shape this runtime can fly: a
    /// field created at fire time and re-applied every tick. A declaration
    /// that says otherwise is refused by name rather than flown as something
    /// it is not.
    ///
    /// Registering the same id twice replaces the declaration: a reload that
    /// re-reads the same record must not accumulate fields, and the field
    /// ledger itself is per-launch.
    ///
    /// # Errors
    ///
    /// [`TanglerFieldRegistrationError::Closed`] for a closed session,
    /// [`TanglerFieldRegistrationError::NotCreatedAtFireTime`] or
    /// [`TanglerFieldRegistrationError::NotReappliedEveryTick`] for a
    /// declaration this runtime does not express.
    pub fn register_tangler_field(
        &mut self,
        ordnance: &OrdnanceId,
        declaration: TanglerFieldDeclaration,
    ) -> Result<(), TanglerFieldRegistrationError> {
        if self.closed {
            return Err(TanglerFieldRegistrationError::Closed);
        }
        if !declaration.created_at_fire_time {
            return Err(TanglerFieldRegistrationError::NotCreatedAtFireTime {
                record: declaration.record,
            });
        }
        if !declaration.reapplied_every_tick {
            return Err(TanglerFieldRegistrationError::NotReappliedEveryTick {
                record: declaration.record,
            });
        }
        self.tangler_fields.insert(ordnance.clone(), declaration);
        Ok(())
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
    /// The airframe is taken to carry no equipment; use
    /// [`Self::register_installed`] to register against a real equipment set.
    /// Both go through the same catalogue and compatibility check.
    ///
    /// # Errors
    ///
    /// As [`Self::register_installed`].
    pub fn register(
        &mut self,
        shooter: ActorId,
        declared: &[DeclaredOrdnance],
    ) -> Result<Vec<RegisteredOrdnance>, OrdnanceRegistrationError> {
        self.register_installed(shooter, declared, &BTreeSet::new())
    }

    /// Registers one actor's declared loadout on an airframe carrying
    /// `equipment`, resolving every id against the session's supported
    /// catalogue first.
    ///
    /// An id the catalogue does not hold is refused by name
    /// ([`OrdnanceRegistrationError::Unsupported`]), and a component whose
    /// shared equipment rule gives a non-compatible verdict is refused
    /// ([`OrdnanceRegistrationError::Incompatible`]); either refusal
    /// registers nothing, so an import and the shop read one answer
    /// (F28 non-negotiable 5).
    ///
    /// # Errors
    ///
    /// [`OrdnanceRegistrationError`] for a closed session, an actor from
    /// another generation, a component whose declared record refuses to lower,
    /// an id outside the catalogue, a repeated id, an incompatible
    /// installation, or a booster the runtime refused.
    pub fn register_installed(
        &mut self,
        shooter: ActorId,
        declared: &[DeclaredOrdnance],
        equipment: &BTreeSet<ContentId>,
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
            self.catalogue
                .require(&ordnance)
                .map_err(OrdnanceRegistrationError::Unsupported)?;
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
        let ids: Vec<OrdnanceId> = lowered.iter().map(|item| item.ordnance().clone()).collect();
        let verdicts = self
            .catalogue
            .resolve_installation(&ids, equipment)
            .map_err(OrdnanceRegistrationError::Unsupported)?;
        if let Some(refused) = verdicts
            .into_iter()
            .find(|verdict| !verdict.compatibility.is_compatible())
        {
            return Err(OrdnanceRegistrationError::Incompatible {
                ordnance: refused.ordnance,
                verdict: refused.compatibility,
            });
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
                sound: media.sound().cloned(),
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
    /// the status ledger, the tangler fields and tangle timers, the nitro
    /// tables and the lowered loadout are released, every mirror is
    /// despawned, and no further order, step or registration is accepted.
    ///
    /// The runtime is *dropped and rebuilt empty*, not merely made
    /// unreachable, so an accessor cannot read a stale effect or a spent
    /// capacity out of a session that no longer exists. The lowered
    /// components go with it: they are the session's loadout, and an audit
    /// (F28-D's `session_ordnance_audit`) must not be able to report a
    /// fireable loadout for a session that was torn down. A restart must
    /// build a new session, and this one carries nothing across it.
    pub fn close(&mut self, world: &mut World) -> OrdnanceTeardownReport {
        self.closed = true;
        let mut projectiles: Vec<ProjectileId> =
            self.runtime.iter().map(|live| live.projectile()).collect();
        projectiles.sort_unstable();
        self.engagements.clear();
        self.components.clear();
        self.launched_units.clear();
        self.tangler_fields.clear();
        let mirrors = despawn_ordnance_mirrors(world);
        self.runtime = OrdnanceRuntime::new(
            self.session_id,
            self.runtime.tick(),
            self.rate,
            self.producer,
        );
        self.tangler = TanglerLedger::new(self.session_id, self.runtime.tick(), self.rate);
        OrdnanceTeardownReport {
            projectiles,
            mirrors,
        }
    }
}

// --------------------------------------------------------- engine consumer ---

/// One launcher's stack: what it holds and what is left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackStock {
    /// The declared capacity, in units.
    pub capacity_units: u64,
    /// The units not yet launched.
    pub remaining_units: u64,
}

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
    /// Whether the pilot is incapacitated: the actor's controls read zero.
    pub incapacitated: bool,
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
            incapacitated: false,
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

// ==================================================== T452-REG-TANGLER-LEDGER ====
//
// Task #2196. `docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md`
// §1 (`TANGLER`) measured, statically in the owner-supplied decrypted engine
// image, what a `TANGLER` record does: at fire time it builds an area field
// object (`0x4b94e0`) holding the owner, the weapon object and a world
// position at `+0x08`, the squared radius at `+0x14` and the remaining `TIME`
// at `+0x18`; the field's tick (`0x4b9590`) subtracts the frame time from
// `+0x18` and, while that stays positive, walks the actor list and re-applies
// the weapon's own per-hit routine to every live actor whose squared distance
// from the field position is below the squared radius
// (`0x4b95db`–`0x4b961a`), the owner included; at `<= 0` it unlinks and frees
// itself (`0x4b96f2`–`0x4b9736`). The victim's tangle is the timed status the
// vocabulary of task #2185 already declares
// ([`TangleStatus`](cs_content::ordnance::TangleStatus)): a re-hit raises the
// timer but never shortens it, and the timer counts down to expiry.
//
// The pure vocabulary is `cs_content`'s
// ([`cs_content::ordnance::TanglerFieldDeclaration`], [`TangleStatus`]); it
// cannot move into `cs_sim`, which must not depend on `cs_content`. So the
// ledger that *applies* that vocabulary per tick lives here, at the
// application boundary that sees both, and it owns no pose: the field centre
// is supplied by the fire path that opens the field, and the victims are the
// tick's own [`TargetPath`] list, exactly as
// [`OrdnanceRuntime::open_area`] reads them.

/// Claim: which world position the original's fire path hands the field
/// constructor `0x4b94e0`.
///
/// The finding pins the constructor, the field's own `+0x08` position slot,
/// its `RADIUS²`/`TIME` slots and its tick, but not which entity's position
/// the caller passes — the launcher at fire time, the projectile's first
/// pose, or another point. This ledger therefore takes the centre from its
/// producer and names the gap rather than choosing one of them.
pub const T452_REG_TANGLER_FIELD_ORIGIN_CLAIM: &str = "t452_reg.limit.tangler_field_origin";

/// The identity of one open tangler field: a session-qualified, never-recycled
/// serial, in the shape of [`ProjectileId`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TanglerFieldId {
    /// The session generation the field belongs to.
    pub session: u64,
    /// The field's serial within that session.
    pub serial: u64,
}

impl std::fmt::Display for TanglerFieldId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "tangler field {}:{}", self.session, self.serial)
    }
}

/// One open tangler field.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTanglerField {
    /// The field's identity.
    pub instance: TanglerFieldId,
    /// The record that declared it.
    pub record: String,
    /// The actor whose weapon opened it. It is hit like every other actor
    /// inside; the measured tick excludes nobody.
    pub owner: ActorId,
    /// Where it is centred, from the fire path
    /// ([`T452_REG_TANGLER_FIELD_ORIGIN_CLAIM`]).
    pub centre: WorldPosition,
    /// Its radius, metres, unsquared.
    pub radius_m: f64,
    /// How long it lives, in whole simulation ticks.
    pub lifetime_ticks: u64,
    /// The tick it opened on.
    pub opened_at: Tick,
    /// The tick it expires on: live at every tick before this one, gone at it,
    /// the status ledger's boundary rule.
    pub expires_at: Tick,
    /// The measured per-victim duration rule it re-applies every tick.
    pub status: TangleStatus,
}

impl OpenTanglerField {
    /// Whether the field is live at `tick`.
    #[must_use]
    pub fn is_live_at(&self, tick: Tick) -> bool {
        tick < self.expires_at
    }

    /// Whether an actor at squared distance `d2` (square metres) is inside it:
    /// strictly below `RADIUS²`, as the measured tick compares.
    #[must_use]
    pub fn contains_d2(&self, d2: f64) -> bool {
        d2 < self.radius_m * self.radius_m
    }
}

/// The tangle one actor is under right now.
///
/// This is the state the flight model reads: the measured fact is that the
/// original suppresses the force scalar `[ebp-8]` of `0x48fc40` while the
/// status is live, and what that scalar *is* stays the named claim
/// [`T452_REG_TANGLER_FORCE_SCALAR`](cs_content::ordnance::T452_REG_TANGLER_FORCE_SCALAR).
/// Nothing here decides a flight-model meaning beyond the timer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TangleState {
    /// The tangled actor.
    pub target: ActorId,
    /// Whole ticks left before it clears, as of the ledger's tick. Both the
    /// field-expiry advance and the application pass refresh it, so a consumer
    /// never reads a value older than the last tick the ledger resolved.
    pub remaining_ticks: u64,
    /// The longest single hit duration this tangle has been given. A re-hit
    /// raises the timer toward it; it never shortens it.
    pub duration_ticks: u64,
    /// The tick it clears on.
    pub expires_at: Tick,
    /// The tick it was last (re-)applied on.
    pub applied_at: Tick,
    /// The field that last applied it.
    pub field: TanglerFieldId,
}

/// One field's tangle of one victim on one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TangleApplication {
    /// The tangled actor.
    pub target: ActorId,
    /// Its squared distance from the field centre, square metres: the value
    /// the measured duration formula divides.
    pub distance_squared: f64,
    /// The duration this hit gave, whole ticks.
    pub duration_ticks: u64,
    /// The timer after the raise, whole ticks.
    pub remaining_ticks: u64,
    /// The tick the timer now clears on.
    pub expires_at: Tick,
}

/// One live field's application pass on one tick.
#[derive(Clone, Debug, PartialEq)]
pub struct TanglerFieldTick {
    /// The field.
    pub field: TanglerFieldId,
    /// Every victim it tangled this tick, in ascending actor order. Empty is
    /// "nobody was inside", which is a miss and not a silent pass.
    pub applied: Vec<TangleApplication>,
}

/// A tangle timer that started.
///
/// One event per episode, not one per re-application: a re-hit that only raises
/// a live timer is the same continuous status, so it changes no transition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TangleTransition {
    /// The actor that became tangled.
    pub target: ActorId,
    /// The field that tangled it.
    pub field: TanglerFieldId,
    /// The tick the timer clears on.
    pub expires_at: Tick,
}

/// One tick's tangler result: what each live field applied, the tangles that
/// began and the ones that cleared.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TanglerTick {
    /// One entry per live field, in ascending field id order.
    pub passes: Vec<TanglerFieldTick>,
    /// Tangles that began this tick.
    pub started: Vec<TangleTransition>,
    /// Tangle timers that cleared this tick.
    pub expired: Vec<ExpiredTangle>,
}

/// A tangler field that reached its expiry boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpiredTanglerField {
    /// The field.
    pub instance: TanglerFieldId,
    /// The record that declared it.
    pub record: String,
    /// The tick it expired on.
    pub expired_at: Tick,
}

/// A tangle timer that cleared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpiredTangle {
    /// The actor that cleared.
    pub target: ActorId,
    /// The field that last applied it.
    pub field: TanglerFieldId,
    /// The tick it cleared on.
    pub expired_at: Tick,
    /// The longest single hit duration it had been given.
    pub duration_ticks: u64,
}

/// Why the tangler ledger refused a call.
#[derive(Clone, Debug, PartialEq)]
pub enum TanglerFieldRefusal {
    /// The call named another session generation.
    ForeignSession {
        /// The ledger's generation.
        expected: u64,
        /// The generation the call named.
        found: u64,
    },
    /// The tick went backwards.
    NonMonotonicTick {
        /// The ledger's tick.
        at: Tick,
        /// The tick the call named.
        found: Tick,
    },
    /// The declaration's `TIME` covers no whole tick.
    NonPositiveLifetime {
        /// The declared lifetime, seconds.
        seconds: f64,
    },
    /// The declaration's radius is not a finite positive length.
    NonFiniteRadius {
        /// The declared radius, metres.
        metres: f64,
    },
    /// A victim's position was not finite.
    NonFinitePosition {
        /// The victim whose position could not be used.
        target: ActorId,
    },
}

impl std::fmt::Display for TanglerFieldRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => {
                write!(f, "session {found} is not this ledger's session {expected}")
            }
            Self::NonMonotonicTick { at, found } => {
                write!(f, "tick {found:?} is before the ledger's tick {at:?}")
            }
            Self::NonPositiveLifetime { seconds } => {
                write!(f, "a field lifetime of {seconds} s covers no whole tick")
            }
            Self::NonFiniteRadius { metres } => {
                write!(f, "a field radius of {metres} m is not a positive length")
            }
            Self::NonFinitePosition { target } => {
                write!(f, "{target}'s position is not finite")
            }
        }
    }
}

impl std::error::Error for TanglerFieldRefusal {}

/// Why a session refused a measured tangler field at registration.
///
/// A field is only expressible when it is the measured shape: created at fire
/// time by the component's own launch, and re-applied to every actor inside on
/// every tick. Anything else is refused here rather than flown as a field the
/// measurement does not describe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TanglerFieldRegistrationError {
    /// The session has been closed and accepts nothing.
    Closed,
    /// The declaration's field is not created at fire time.
    NotCreatedAtFireTime {
        /// The record that declared it.
        record: String,
    },
    /// The declaration's field is not re-applied every tick.
    NotReappliedEveryTick {
        /// The record that declared it.
        record: String,
    },
}

impl std::fmt::Display for TanglerFieldRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => f.write_str("the session is closed"),
            Self::NotCreatedAtFireTime { record } => write!(
                f,
                "{record}'s field is not created at fire time, so no launch order can open it"
            ),
            Self::NotReappliedEveryTick { record } => write!(
                f,
                "{record}'s field is not re-applied every tick, which is not the measured field"
            ),
        }
    }
}

impl std::error::Error for TanglerFieldRegistrationError {}

/// The session's open tangler fields and the tangle timers they leave behind.
///
/// Every field is **bounded**: it carries an expiry tick of
/// `opened_at + lifetime_ticks`, [`advance_to`](Self::advance_to) removes
/// exactly the fields whose boundary was reached and reports each once, and
/// the ledger is confined to one session generation, so a restarted session (a
/// new ledger) inherits nothing. A tangle outlives the field that applied it:
/// it is the victim's own timer and counts down on its own expiry tick.
#[derive(Clone, Debug)]
pub struct TanglerLedger {
    session: SessionId,
    tick: Tick,
    rate: TickRate,
    next_serial: u64,
    fields: BTreeMap<TanglerFieldId, OpenTanglerField>,
    tangles: BTreeMap<ActorId, TangleState>,
}

impl TanglerLedger {
    /// Opens an empty ledger for one session generation, positioned at `tick`.
    #[must_use]
    pub fn new(session: SessionId, tick: Tick, rate: TickRate) -> Self {
        Self {
            session,
            tick,
            rate,
            next_serial: 0,
            fields: BTreeMap::new(),
            tangles: BTreeMap::new(),
        }
    }

    /// The session generation this ledger is confined to.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The tick the ledger is positioned at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The declared tick rate whole-tick durations are converted with.
    #[must_use]
    pub const fn rate(&self) -> TickRate {
        self.rate
    }

    /// How many fields are open.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether no field is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// One open field by id.
    #[must_use]
    pub fn get(&self, instance: &TanglerFieldId) -> Option<&OpenTanglerField> {
        self.fields.get(instance)
    }

    /// Every open field, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &OpenTanglerField> {
        self.fields.values()
    }

    /// The tangle timer on one actor, when it is tangled at the ledger's tick.
    #[must_use]
    pub fn tangled(&self, target: &ActorId) -> Option<&TangleState> {
        self.tangles
            .get(target)
            .filter(|state| state.expires_at > self.tick)
    }

    /// Every tangle timer, in ascending actor order.
    pub fn tangles(&self) -> impl Iterator<Item = &TangleState> {
        self.tangles
            .values()
            .filter(|state| state.expires_at > self.tick)
    }

    /// Opens one field from a declaration the fire path already lowered.
    ///
    /// The centre is the caller's, not a default and not derived from a pose
    /// the ledger does not own ([`T452_REG_TANGLER_FIELD_ORIGIN_CLAIM`]).
    /// The lifetime is the declaration's `TIME` in whole ticks, rounded up as
    /// the original's step is (`seconds_to_ticks`).
    ///
    /// # Errors
    ///
    /// [`TanglerFieldRefusal::ForeignSession`] for another generation,
    /// [`TanglerFieldRefusal::NonMonotonicTick`] for a tick before the
    /// ledger's, or [`TanglerFieldRefusal::NonPositiveLifetime`] /
    /// [`TanglerFieldRefusal::NonFiniteRadius`] for a declaration with no
    /// positive finite lifetime or radius.
    pub fn open(
        &mut self,
        session: SessionId,
        tick: Tick,
        order: &TanglerFieldOrder,
    ) -> Result<TanglerFieldId, TanglerFieldRefusal> {
        self.check(session, tick)?;
        if order.owner.session != self.session {
            return Err(TanglerFieldRefusal::ForeignSession {
                expected: self.session.get(),
                found: order.owner.session.get(),
            });
        }
        let declaration = &order.declaration;
        if !declaration.radius_m.is_finite() || declaration.radius_m <= 0.0 {
            return Err(TanglerFieldRefusal::NonFiniteRadius {
                metres: declaration.radius_m,
            });
        }
        if !declaration.lifetime_s.is_finite() || declaration.lifetime_s <= 0.0 {
            return Err(TanglerFieldRefusal::NonPositiveLifetime {
                seconds: declaration.lifetime_s,
            });
        }
        let lifetime_ticks = seconds_to_ticks(declaration.lifetime_s, self.rate.ticks_per_second());
        let instance = TanglerFieldId {
            session: self.session.get(),
            serial: self.next_serial,
        };
        self.next_serial += 1;
        self.fields.insert(
            instance,
            OpenTanglerField {
                instance,
                record: declaration.record.clone(),
                owner: order.owner,
                centre: order.centre,
                radius_m: declaration.radius_m,
                lifetime_ticks,
                opened_at: tick,
                expires_at: Tick(tick.0 + lifetime_ticks),
                status: declaration.status.clone(),
            },
        );
        Ok(instance)
    }

    /// Advances the ledger to `tick` and reports every field that reached its
    /// expiry boundary, each exactly once, boundary tick first then id.
    ///
    /// # Errors
    ///
    /// [`TanglerFieldRefusal::ForeignSession`] or
    /// [`TanglerFieldRefusal::NonMonotonicTick`].
    pub fn advance_to(
        &mut self,
        session: SessionId,
        tick: Tick,
    ) -> Result<Vec<ExpiredTanglerField>, TanglerFieldRefusal> {
        self.check(session, tick)?;
        let mut expired = Vec::new();
        self.fields.retain(|instance, field| {
            if field.is_live_at(tick) {
                return true;
            }
            expired.push(ExpiredTanglerField {
                instance: *instance,
                record: field.record.clone(),
                expired_at: field.expires_at,
            });
            false
        });
        expired.sort_by(|a, b| {
            a.expired_at
                .cmp(&b.expired_at)
                .then(a.instance.cmp(&b.instance))
        });
        self.tick = tick;
        for state in self.tangles.values_mut() {
            state.remaining_ticks = state.expires_at.0.saturating_sub(tick.0);
        }
        Ok(expired)
    }

    /// Runs one tick's application pass: every live field re-applies its tangle
    /// to every supplied actor inside its radius, then the tangle timers that
    /// reached their boundary are reported and cleared.
    ///
    /// `targets` is the same [`TargetPath`] list the proximity fuse and the
    /// area pass read, so the pass adds no input and owns no pose. One victim
    /// is tangled once per field per tick (a repeated path is read once, at its
    /// first position), in ascending actor order. The field's owner is not
    /// excluded: an owner the caller lists is tangled like any other actor.
    ///
    /// The list is the whole actor set the measured tick walks
    /// (`0x4b9590` walks the actor list and excludes nobody), so a caller that
    /// omits an actor leaves it untangled. In particular M01's rocket target
    /// list omits the player's own actor: a caller that wants the player
    /// tangled by a field it fired has to list it, which is the flight
    /// consumer's wiring (task T452-REG-TANGLER-EFFECT).
    ///
    /// The order is deliberate. Applying before expiring means a victim still
    /// inside a live field is re-tangled on the tick its timer would otherwise
    /// clear, which is exactly the measured re-hit rule ("raise to max, never
    /// shorten"); an actor that has left every field is reported expired once,
    /// on its own boundary tick.
    ///
    /// # Errors
    ///
    /// [`TanglerFieldRefusal::ForeignSession`] for another generation or a
    /// victim from one, [`TanglerFieldRefusal::NonMonotonicTick`] for a tick
    /// before the ledger's, or [`TanglerFieldRefusal::NonFinitePosition`] for
    /// a victim whose squared distance is not finite.
    pub fn apply(
        &mut self,
        session: SessionId,
        tick: Tick,
        targets: &[TargetPath],
    ) -> Result<TanglerTick, TanglerFieldRefusal> {
        self.check(session, tick)?;
        let mut victims: BTreeMap<ActorId, WorldPosition> = BTreeMap::new();
        for path in targets {
            if path.actor().session != self.session {
                return Err(TanglerFieldRefusal::ForeignSession {
                    expected: self.session.get(),
                    found: path.actor().session.get(),
                });
            }
            victims.entry(path.actor()).or_insert(path.current());
        }

        // Read the fields out before the pass so the tangle map can be written
        // while each field's radius and rule are still borrowed.
        let fields: Vec<OpenTanglerField> = self.fields.values().cloned().collect();
        // Validate every distance first: a refusal below must change nothing,
        // so nothing is written until each pair is known to be finite.
        for field in &fields {
            for (target, position) in &victims {
                if !squared_distance(field, *position).is_finite() {
                    return Err(TanglerFieldRefusal::NonFinitePosition { target: *target });
                }
            }
        }
        let mut passes = Vec::with_capacity(fields.len());
        let mut started = Vec::new();
        for field in &fields {
            let mut applied = Vec::new();
            for (target, position) in &victims {
                let d2 = squared_distance(field, *position);
                if !field.contains_d2(d2) {
                    continue;
                }
                let duration_ticks =
                    seconds_to_ticks(field.status.duration_for(d2), self.rate.ticks_per_second());
                let raised = Tick(tick.0 + duration_ticks);
                let current = self
                    .tangles
                    .get(target)
                    .filter(|state| state.expires_at > tick);
                let expires_at = current.map_or(raised, |state| state.expires_at.max(raised));
                let ceiling = current.map_or(duration_ticks, |state| {
                    state.duration_ticks.max(duration_ticks)
                });
                if current.is_none() {
                    started.push(TangleTransition {
                        target: *target,
                        field: field.instance,
                        expires_at,
                    });
                }
                let state = TangleState {
                    target: *target,
                    remaining_ticks: expires_at.0 - tick.0,
                    duration_ticks: ceiling,
                    expires_at,
                    applied_at: tick,
                    field: field.instance,
                };
                self.tangles.insert(*target, state);
                applied.push(TangleApplication {
                    target: *target,
                    distance_squared: d2,
                    duration_ticks,
                    remaining_ticks: state.remaining_ticks,
                    expires_at,
                });
            }
            passes.push(TanglerFieldTick {
                field: field.instance,
                applied,
            });
        }

        let mut expired = Vec::new();
        for state in self.tangles.values_mut() {
            // The timer is read at the ledger's tick, so an actor that no
            // field re-applied still reports the ticks it has left.
            state.remaining_ticks = state.expires_at.0.saturating_sub(tick.0);
        }
        self.tangles.retain(|_, state| {
            if state.expires_at > tick {
                return true;
            }
            expired.push(ExpiredTangle {
                target: state.target,
                field: state.field,
                expired_at: state.expires_at,
                duration_ticks: state.duration_ticks,
            });
            false
        });
        expired.sort_by(|a, b| {
            a.expired_at
                .cmp(&b.expired_at)
                .then(a.target.cmp(&b.target))
        });
        self.tick = tick;
        Ok(TanglerTick {
            passes,
            started,
            expired,
        })
    }

    fn check(&self, session: SessionId, tick: Tick) -> Result<(), TanglerFieldRefusal> {
        if session != self.session {
            return Err(TanglerFieldRefusal::ForeignSession {
                expected: self.session.get(),
                found: session.get(),
            });
        }
        if tick < self.tick {
            return Err(TanglerFieldRefusal::NonMonotonicTick {
                at: self.tick,
                found: tick,
            });
        }
        Ok(())
    }
}

/// The squared distance from a field's centre to `position`, square metres:
/// the value the measured duration formula divides and the containment test
/// compares with `RADIUS²`.
fn squared_distance(field: &OpenTanglerField, position: WorldPosition) -> f64 {
    let dx = position.x() - field.centre.x();
    let dy = position.y() - field.centre.y();
    let dz = position.z() - field.centre.z();
    dx * dx + dy * dy + dz * dz
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
/// Status expiry, then areas, then tangler fields, then orders, then the
/// tangler application pass, then guidance, then motion, then the fuse
/// decisions, then the mirror pass. An item launched on this tick is moved by
/// the same step, an item that triggers on this tick applies its effects
/// on this tick — before the next advance retires it — and a component
/// registered with a measured tangler field opens that field inside its own
/// launch order, so the field exists on the tick the weapon fired. A guided item that loses
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
    match session.runtime.advance_status(session.session_id, step.at) {
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

    // Areas expire on the same rule as statuses: on their boundary tick,
    // reported once, before this tick's detonations open new ones.
    match session.runtime.advance_areas(session.session_id, step.at) {
        Ok(expired) => tick.areas_expired = expired,
        Err(source) => tick.areas_refused = Some(source),
    }

    // Tangler fields expire on that same rule too, before this tick's orders
    // can open new ones. A field opened at tick T with a `TIME` of N ticks is
    // live at T..T+N-1 and reported here at T+N, so it applies exactly N times.
    match session.tangler.advance_to(session.session_id, step.at) {
        Ok(expired) => tick.tangler_expired = expired,
        Err(source) => tick.tangler_refused = Some(source),
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
                if session
                    .stack_remaining(&launch.shooter, &launch.ordnance)
                    .is_some_and(|stock| stock.remaining_units == 0)
                {
                    tick.orders_refused
                        .push(OrdnanceOrderRefusal::StackExhausted {
                            shooter: launch.shooter,
                            ordnance: launch.ordnance.clone(),
                        });
                    continue;
                }

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
                        *session
                            .launched_units
                            .entry((launch.shooter, launch.ordnance.clone()))
                            .or_insert(0) += 1;
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
                            sound: component.media().sound().cloned(),
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
                        // A measured tangler component opens its field on the
                        // tick it fires (T452-R.02 §1: the fire path builds the
                        // field object, `0x4b94e0`), from the pose the fire
                        // path itself read. The centre is the launch origin the
                        // effect above reports, never the launcher's actor
                        // record or the world origin:
                        // `T452_REG_TANGLER_FIELD_ORIGIN_CLAIM` keeps the part
                        // the image does not pin.
                        if let Some(declaration) =
                            session.tangler_fields.get(&launch.ordnance).cloned()
                        {
                            let order = TanglerFieldOrder {
                                declaration,
                                owner: launch.shooter,
                                centre: transform.origin,
                            };
                            match session.tangler.open(session.session_id, step.at, &order) {
                                Ok(field) => tick.tangler_opened.push(field),
                                Err(source) => tick.tangler_refused = Some(source),
                            }
                        }
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
            OrdnanceOrder::Tangler(order) => {
                if order.owner.session.get() != session.session {
                    tick.orders_refused.push(OrdnanceOrderRefusal::Runtime(
                        OrdnanceRuntimeError::ForeignSession {
                            expected: session.session,
                            found: order.owner.session.get(),
                        },
                    ));
                    continue;
                }
                match session.tangler.open(session.session_id, step.at, order) {
                    Ok(field) => tick.tangler_opened.push(field),
                    Err(source) => tick
                        .orders_refused
                        .push(OrdnanceOrderRefusal::Tangler(source)),
                }
            }
        }
    }

    // The tangler pass runs after the orders so a field opened at fire time on
    // this tick applies on this tick, and it reads the same `TargetPath` list
    // the proximity fuse and the area pass read, so it adds no input. Its own
    // `apply` expires the tangle timers after applying, so a victim still
    // inside a live field is re-tangled rather than reported expired. Only a
    // transition is announced — a tangle beginning or clearing — never the
    // per-tick re-application the measured field performs.
    match session
        .tangler
        .apply(session.session_id, step.at, step.targets)
    {
        Ok(outcome) => {
            for transition in &outcome.started {
                let event = push_network_event(
                    &mut session.events,
                    &mut session.next_event_sequence,
                    session.session_id,
                    step.at,
                    session.producer,
                    OrdnanceEventKind::TangleStarted {
                        target: transition.target,
                        field: transition.field,
                        expires_at: transition.expires_at,
                    },
                );
                tick.events.push(event);
            }
            for expired in &outcome.expired {
                let event = push_network_event(
                    &mut session.events,
                    &mut session.next_event_sequence,
                    session.session_id,
                    step.at,
                    session.producer,
                    OrdnanceEventKind::TangleExpired {
                        target: expired.target,
                        field: expired.field,
                        expired_at: expired.expired_at,
                    },
                );
                tick.events.push(event);
            }
            tick.tangler = outcome.passes;
            tick.tangles_started = outcome.started;
            tick.tangles_expired = outcome.expired;
        }
        Err(source) => tick.tangler_refused = Some(source),
    }

    session.runtime.retarget_marker_seekers(session.session_id);
    if let Some(observation) = step.guidance {
        let guidance = session
            .runtime
            .guidance_tick(session.session_id, observation);
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
                area: None,
                area_refused: None,
            };
            let routed = match (detonation.area_effect(), step.blast_recipients) {
                (Some(_), Some(recipients)) => session.runtime.route_detonation_area(
                    session.session_id,
                    step.at,
                    detonation,
                    recipients,
                ),
                _ => session.runtime.route_detonation(
                    session.session_id,
                    step.at,
                    detonation,
                    engagement.damage_target,
                    engagement.node.clone(),
                ),
            };
            match routed {
                Ok(hits) => {
                    blast.hits.clone_from(&hits);
                    match damage.resolve(step.at, &hits) {
                        Ok(resolution) => blast.damage = Some(resolution),
                        Err(source) => blast.damage_refused = Some(source),
                    }
                    match session.runtime.apply_detonation_statuses(
                        session.session_id,
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
                    if detonation.area_effect().is_some() {
                        match session.runtime.open_detonation_area(
                            session.session_id,
                            step.at,
                            detonation,
                            step.targets,
                        ) {
                            Ok(instance) => blast.area = Some(instance),
                            Err(source) => blast.area_refused = Some(source),
                        }
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
                    Ok(FuseDecision::Consumed { range_m }) => {
                        // The measured end of the record's `RANGE`. The item is
                        // consumed: it detonates nothing, no area opens and no
                        // damage is routed, so this branch deliberately reaches
                        // neither `route_trigger` nor `resolve`. Removing the
                        // item here (rather than leaving it for the next
                        // advance) is what makes the consumption the item's
                        // last tick, exactly as a detonation is.
                        session.runtime.remove(&projectile);
                        session.engagements.remove(&projectile);
                        tick.consumed.push(OrdnanceConsumption {
                            projectile,
                            range_m,
                        });
                        let event = push_network_event(
                            &mut session.events,
                            &mut session.next_event_sequence,
                            session.session_id,
                            step.at,
                            session.producer,
                            OrdnanceEventKind::Consumed {
                                projectile,
                                range_m,
                            },
                        );
                        tick.events.push(event);
                        continue;
                    }
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
                    area: None,
                    area_refused: None,
                };

                // A fuse `Impact`/`Proximity` trigger names the actor it
                // actually reached; with a producer victim table its hits land
                // on that actor's own registered node — never defaulted to
                // the engagement's designated target, which may be a
                // different actor. A `Timed` trigger names no victim and
                // still routes through the engagement.
                let (victim, victim_node) = match (detonation.trigger, step.victim_nodes) {
                    (
                        FuseTrigger::Impact { target, .. } | FuseTrigger::Proximity { target, .. },
                        Some(nodes),
                    ) => match nodes.get(&target) {
                        Some(node) => (target, node.clone()),
                        None => {
                            tick.routing_refused
                                .push(OrdnanceRoutingRefusal::UnboundVictim { projectile, target });
                            continue;
                        }
                    },
                    _ => (engagement.damage_target, engagement.node.clone()),
                };
                let status_recipient = match engagement.status_recipient.system() {
                    Some(system) => StatusEffectTarget::actor_system(victim, system),
                    None => StatusEffectTarget::whole_actor(victim),
                };
                let declares_area = session
                    .runtime
                    .get(&projectile)
                    .is_some_and(|item| item.definition().area_effect().is_some());
                let routed = match (declares_area, step.blast_recipients) {
                    (true, Some(recipients)) => session.runtime.route_trigger_area(
                        session.session_id,
                        step.at,
                        &projectile,
                        recipients,
                    ),
                    _ => session.runtime.route_trigger(
                        session.session_id,
                        step.at,
                        &projectile,
                        victim,
                        victim_node,
                    ),
                };
                match routed {
                    Ok(hits) => {
                        detonation.hits.clone_from(&hits);
                        match damage.resolve(step.at, &hits) {
                            Ok(resolution) => detonation.damage = Some(resolution),
                            Err(source) => detonation.damage_refused = Some(source),
                        }
                        match session.runtime.apply_statuses(
                            session.session_id,
                            step.at,
                            &projectile,
                            status_recipient,
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

                        if declares_area {
                            match session.runtime.open_area(
                                session.session_id,
                                step.at,
                                &projectile,
                                step.targets,
                            ) {
                                Ok(instance) => detonation.area = Some(instance),
                                Err(source) => detonation.area_refused = Some(source),
                            }
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
                                target: victim,
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

// ------------------------------------------------------- the session audit ----

/// One registered component's audited row: what this session can actually do
/// with the component it lowered.
///
/// This is the **runtime** half of F28-D's audit. `cs_content::ordnance`'s
/// audit walks the *declared* records and can see their provenance; the
/// lowering boundary drops provenance on purpose, so nothing here can claim a
/// record is original data. What this half can do — and the content half
/// cannot — is report which channels a registered component actually routes and
/// whether any production path consumes each of them.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionOrdnanceRow {
    shooter: ActorId,
    ordnance: OrdnanceId,
    family: OrdnanceFamily,
    booster: bool,
    launcher: Option<DamageNodeKey>,
    damage_channels: usize,
    status_effects: usize,
    area_declared: bool,
    area_applied: bool,
    unconsumed_fields: Vec<&'static str>,
}

impl SessionOrdnanceRow {
    /// The actor this component is registered for.
    #[must_use]
    pub const fn shooter(&self) -> ActorId {
        self.shooter
    }

    /// The lowered catalog id.
    #[must_use]
    pub const fn ordnance(&self) -> &OrdnanceId {
        &self.ordnance
    }

    /// The lowered behavior family.
    #[must_use]
    pub const fn family(&self) -> OrdnanceFamily {
        self.family
    }

    /// Whether this component is a nitro booster rather than a launcher.
    #[must_use]
    pub const fn is_booster(&self) -> bool {
        self.booster
    }

    /// The damage-node mount a launched item occupies, `None` for a booster.
    #[must_use]
    pub const fn launcher(&self) -> Option<&DamageNodeKey> {
        self.launcher.as_ref()
    }

    /// Whether this session can launch it: a booster is driven, not launched,
    /// so it has no mount.
    #[must_use]
    pub fn is_launchable(&self) -> bool {
        !self.booster && self.launcher.is_some()
    }

    /// How many declared damage channels the item routes when its fuse fires.
    #[must_use]
    pub const fn damage_channels(&self) -> usize {
        self.damage_channels
    }

    /// How many timed status effects the item applies when its fuse fires.
    #[must_use]
    pub const fn status_effects(&self) -> usize {
        self.status_effects
    }

    /// Whether the component declares a bounded area effect.
    #[must_use]
    pub const fn declares_area(&self) -> bool {
        self.area_declared
    }

    /// Whether the session applies the declared area's radius and lifetime.
    ///
    /// `true` for a component that declares an area: a trigger or a
    /// guidance-loss detonation opens it through
    /// [`cs_sim::weapons::ordnance::OrdnanceRuntime::open_area`], which reaches
    /// every actor inside the radius once and expires on the lifetime
    /// boundary. It says the path exists, not that the original behaves this
    /// way; see `docs/findings/2026-10-11-f28-ae1-area-effect-reach.md`.
    #[must_use]
    pub const fn area_applied(&self) -> bool {
        self.area_applied
    }

    /// The declared fields this component carries that no production path
    /// reads.
    #[must_use]
    pub fn unconsumed_fields(&self) -> &[&'static str] {
        &self.unconsumed_fields
    }

    /// Whether this component delivers any gameplay effect at all.
    #[must_use]
    pub const fn delivers_effect(&self) -> bool {
        self.booster || self.damage_channels > 0 || self.status_effects > 0
    }
}

/// One actor's booster, as this session holds it.
///
/// Every number here is the **declared** one the ledger was opened with, plus
/// the ledger's live capacity. There is no position, no velocity and no
/// duration: AC04's "boost changes thrust/consumption but never directly
/// teleports or scales render dt" is enforced by the shape of the record as
/// much as by its values, and `consumption_per_tick` is the only rate the
/// ledger converts with.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionNitroRow {
    shooter: ActorId,
    capacity_units: f64,
    capacity_fraction: f64,
    declared_capacity_units: f64,
    consumption_per_s: f64,
    consumption_per_tick: f64,
    extra_thrust_n: f64,
    authority_multiplier: f64,
    activation: NitroActivationRule,
    burn_running: bool,
}

impl SessionNitroRow {
    /// The actor whose booster this is.
    #[must_use]
    pub const fn shooter(&self) -> ActorId {
        self.shooter
    }

    /// Capacity remaining, in the component's own capacity units.
    #[must_use]
    pub const fn capacity_units(&self) -> f64 {
        self.capacity_units
    }

    /// Capacity remaining as a fraction of the declared maximum.
    #[must_use]
    pub const fn capacity_fraction(&self) -> f64 {
        self.capacity_fraction
    }

    /// The capacity the declared record carries.
    #[must_use]
    pub const fn declared_capacity_units(&self) -> f64 {
        self.declared_capacity_units
    }

    /// The declared consumption rate, in capacity units per second.
    #[must_use]
    pub const fn consumption_per_s(&self) -> f64 {
        self.consumption_per_s
    }

    /// What one tick of running nitro costs at the session's declared rate.
    ///
    /// This is the number a [`NitroTick`]'s `consumed_units` must equal, and
    /// it is derived from the declared [`TickRate`] — never from a render
    /// frame's elapsed time.
    #[must_use]
    pub const fn consumption_per_tick(&self) -> f64 {
        self.consumption_per_tick
    }

    /// The extra thrust an accepted activation adds, in newtons.
    #[must_use]
    pub const fn extra_thrust_n(&self) -> f64 {
        self.extra_thrust_n
    }

    /// The control-authority multiplier an accepted activation reports.
    #[must_use]
    pub const fn authority_multiplier(&self) -> f64 {
        self.authority_multiplier
    }

    /// The declared activation rule.
    #[must_use]
    pub const fn activation(&self) -> NitroActivationRule {
        self.activation
    }

    /// Whether a fixed-duration burn is running right now.
    #[must_use]
    pub const fn burn_running(&self) -> bool {
        self.burn_running
    }
}

/// One gap the session audit found, named.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionOrdnanceFinding {
    /// A registered launched item the session can fire that delivers nothing:
    /// no damage channel and no status effect. It flies, sounds and lands for
    /// no gameplay effect at all.
    ComponentDeliversNothing {
        /// The actor carrying it.
        shooter: ActorId,
        /// The component nothing consumes.
        ordnance: OrdnanceId,
    },
    /// A designed behavior family no registered component carries.
    ///
    /// The sheet's non-negotiable 1 — "do not substitute every rocket with one
    /// homing missile" — is only checkable in this direction: a catalogue of
    /// one family is otherwise indistinguishable from a complete one.
    FamilyWithoutAComponent {
        /// The family with no component.
        family: OrdnanceFamily,
    },
}

impl SessionOrdnanceFinding {
    /// The stable machine-readable label of this finding.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ComponentDeliversNothing { .. } => "component_delivers_nothing",
            Self::FamilyWithoutAComponent { .. } => "family_without_a_component",
        }
    }
}

impl std::fmt::Display for SessionOrdnanceFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ComponentDeliversNothing { shooter, ordnance } => write!(
                f,
                "{shooter} can fire {ordnance}, which routes no damage channel and applies \
                 no status effect"
            ),
            Self::FamilyWithoutAComponent { family } => {
                write!(f, "the {family} family has no registered component")
            }
        }
    }
}

/// The result of auditing one live ordnance session.
///
/// A closed session audits to nothing: [`OrdnanceSession::close`] releases the
/// loadout's lowered components along with the runtime, so there is nothing
/// left to audit and no stale row to read out of a session that no longer
/// exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionOrdnanceAudit {
    rows: Vec<SessionOrdnanceRow>,
    families: Vec<(OrdnanceFamily, usize)>,
    nitro: Vec<SessionNitroRow>,
    findings: Vec<SessionOrdnanceFinding>,
}

impl SessionOrdnanceAudit {
    /// One row per registered component, in ascending actor then id order.
    #[must_use]
    pub fn rows(&self) -> &[SessionOrdnanceRow] {
        &self.rows
    }

    /// The row for one component, if it is registered.
    #[must_use]
    pub fn row(&self, shooter: &ActorId, ordnance: &OrdnanceId) -> Option<&SessionOrdnanceRow> {
        self.rows
            .iter()
            .find(|row| row.shooter == *shooter && row.ordnance == *ordnance)
    }

    /// How many components each designed family carries, in family order and
    /// including the families that carry none.
    #[must_use]
    pub fn families(&self) -> &[(OrdnanceFamily, usize)] {
        &self.families
    }

    /// One row per actor's booster, in ascending actor order.
    #[must_use]
    pub fn nitro(&self) -> &[SessionNitroRow] {
        &self.nitro
    }

    /// The booster row for one actor, if it has one.
    #[must_use]
    pub fn nitro_for(&self, shooter: &ActorId) -> Option<&SessionNitroRow> {
        self.nitro.iter().find(|row| row.shooter == *shooter)
    }

    /// Every gap found, in report order.
    #[must_use]
    pub fn findings(&self) -> &[SessionOrdnanceFinding] {
        &self.findings
    }

    /// The findings of one label, so a caller can name one gap at a time.
    #[must_use]
    pub fn findings_of(&self, label: &str) -> Vec<&SessionOrdnanceFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.label() == label)
            .collect()
    }

    /// How many components the session holds.
    #[must_use]
    pub fn component_count(&self) -> usize {
        self.rows.len()
    }

    /// How many launchable components the session holds.
    #[must_use]
    pub fn launchable_count(&self) -> usize {
        self.rows.iter().filter(|row| row.is_launchable()).count()
    }

    /// Whether the session holds nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.nitro.is_empty()
    }

    /// Whether the audit found no gap at all.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Audits what one live ordnance session can actually fire.
///
/// The runtime counterpart of `cs_content::ordnance::OrdnanceAudit`: that one
/// walks declared records and can read their provenance, this one walks the
/// components a session **lowered and registered** and reports which channels
/// each of them reaches a consumer through. Neither repairs a gap. The
/// declared area effect is consumed since F28-AE1, but only as a radius from the
/// detonation: the original's area semantics for a fuse-triggered item are
/// unmeasured and not claimed.
#[must_use]
pub fn session_ordnance_audit(session: &OrdnanceSession) -> SessionOrdnanceAudit {
    let mut audit = SessionOrdnanceAudit::default();
    // A closed session holds no components and no nitro ledger, so the family
    // occupancy walk below would report every designed family as uncovered —
    // a finding about a loadout that no longer exists. Teardown released the
    // loadout; there is nothing left to audit and nothing to complain about.
    if session.is_closed() {
        return audit;
    }

    for (shooter, components) in &session.components {
        for (ordnance, component) in components {
            let booster = component.as_nitro().is_some();
            let launcher = component
                .as_projectile()
                .map(|projectile| projectile.launch().mount().clone());
            let damage_channels = component.as_projectile().map_or(0, |projectile| {
                WEAPON_DAMAGE_CHANNELS
                    .iter()
                    .filter(|channel| projectile.channels().amount_on(**channel) > 0.0)
                    .count()
            });
            let status_effects = component
                .as_projectile()
                .map_or(0, |projectile| projectile.status().len());
            let area_declared = component
                .as_projectile()
                .is_some_and(|projectile| projectile.area_effect().is_some());
            let row = SessionOrdnanceRow {
                shooter: *shooter,
                ordnance: ordnance.clone(),
                family: component.family(),
                booster,
                launcher,
                damage_channels,
                status_effects,
                area_declared,
                area_applied: area_declared,
                unconsumed_fields: cs_content::ordnance::DECLARED_FIELDS_WITHOUT_CONSUMER
                    .iter()
                    .map(|(field, _)| *field)
                    .collect(),
            };
            if !row.delivers_effect() {
                audit
                    .findings
                    .push(SessionOrdnanceFinding::ComponentDeliversNothing {
                        shooter: *shooter,
                        ordnance: ordnance.clone(),
                    });
            }
            audit.rows.push(row);
        }
    }

    audit.families = OrdnanceFamily::ALL
        .iter()
        .map(|family| {
            let count = audit
                .rows
                .iter()
                .filter(|row| row.family == *family)
                .count();
            (*family, count)
        })
        .collect();
    for (family, count) in &audit.families {
        if *count == 0 {
            audit
                .findings
                .push(SessionOrdnanceFinding::FamilyWithoutAComponent { family: *family });
        }
    }

    audit.nitro = session
        .runtime
        .nitro_actors()
        .map(|shooter| {
            let ledger = session
                .runtime
                .nitro(&shooter)
                .expect("nitro_actors names the runtime's own ledgers");
            let parameters = ledger.parameters();
            let tradeoffs = parameters.tradeoffs();
            SessionNitroRow {
                shooter,
                capacity_units: ledger.capacity_units(),
                capacity_fraction: ledger.capacity_fraction(),
                declared_capacity_units: parameters.capacity_units(),
                consumption_per_s: parameters.consumption_per_s(),
                consumption_per_tick: parameters.consumption_per_s() * session.rate().dt_seconds(),
                extra_thrust_n: parameters.extra_thrust_n(),
                authority_multiplier: tradeoffs.authority_multiplier(),
                activation: parameters.activation(),
                burn_running: ledger.burn_running(),
            }
        })
        .collect();

    audit
}

// ----------------------------------------- measured original catalogue ----

/// The installation declares no `wep_` record for its nitro booster: the
/// booster is actor state on an airframe whose plane record sets `nitro`
/// (task T452.03). This is the count, stated as a number so an installation
/// that ever ships a record for it is an event and not a silent change.
pub const ORIGINAL_NITRO_COMPONENT_RECORDS: usize = 0;

/// The original's nitro activation cannot be expressed by
/// [`NitroActivationRule`]: the measured rule is a **charge-bounded run** —
/// a start needs a nearly full tank and the control, and the burn then
/// continues until the charge falls below a fraction (task T452.03). Neither
/// `WhileHeld` nor `FixedTicks` is that rule, so the catalogue records the
/// measured fractions and names the vocabulary gap instead of picking one.
pub const T452_NITRO_ACTIVATION_CLAIM: &str = "t452.limit.nitro_activation_rule";

/// The original's nitro extra thrust, authority tradeoff and any drag or
/// handling penalty are unmeasured: the measured terms replace or scale
/// quantities the finding did not identify (task T452.03), so no newton value
/// and no authority multiplier is asserted.
pub const T452_NITRO_TRADEOFF_CLAIM: &str = "t452.limit.nitro_tradeoff";

/// A record whose end-of-flight trigger no file states — the two area-denial
/// records declare an effect and no detonation key.
pub const T452_FUSE_CLAIM: &str = "t452.limit.fuse";

/// A guided record whose lost-target behavior has not been measured. The
/// measured rule covers the `ROCKET`-flagged records (task #454); the
/// glidebomb `wep_25` declares `LOCK_ON` and no `ROCKET` flag, so it is outside
/// that measurement.
pub const T452_LOST_TARGET_CLAIM: &str = "t452.limit.lost_target";

/// Why a catalogued component is not also a registered runtime component.
/// T452-R.03 measured what earlier sessions had not: a slot's stack count is
/// the record's `CLUSTER_SIZE` (T452-R.03-B), the media ids resolve through
/// the measured `FIRE`/`FLYOUT`/`IMPACT`/`DESTROY_ANIMATION` blocks
/// (T452-R.03-C), and none of the sixteen declares a requirement or
/// permission key so the equipment rule set is measured empty (T452-R.03-D).
/// Mass is measured too, as an absence: the purchase surface prices each
/// enabled rocket box a flat 480 weight / 410 cost and no path reads a
/// type-dependent value (T452-R.03). What still stands between a catalogued
/// and a registered component is the lowering step itself, which
/// T452-RUNTIME-REGISTER (#2156) owns.
pub const T452_RUNTIME_LOWERING_CLAIM: &str = "t452.limit.runtime_lowering";

/// One hardpoint component the installation declares, catalogued with the
/// family and behavior its data states.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalCatalogueEntry {
    /// The record name (`wep_04`).
    pub record: String,
    /// The catalogued entry: identity, family and declared behavior.
    pub entry: CataloguedOrdnance,
}

/// The original's nitro booster as measured, with the rule its vocabulary
/// cannot express and the tradeoffs nobody measured named rather than chosen.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalNitroCatalogue {
    /// The control the resource header declares, with its id.
    pub control: (&'static str, u32),
    /// How many `wep_` records the member declares for the booster.
    pub component_records: usize,
    /// `capacity` (`0x41f00000`), capacity units.
    pub capacity: f64,
    /// `consumption_per_dt` (`0x40800000`), units per engine delta.
    pub consumption_per_dt: f64,
    /// `recovery_per_dt` (`0x3f800000`), units per delta, applied on every
    /// step, not only idle ones.
    pub recovery_per_dt: f64,
    /// A start needs at least this fraction of the capacity (`0x3f7d70a4`).
    pub start_min_fraction: f64,
    /// The booster is cut when the charge falls below this fraction of the
    /// capacity (`0x3d4ccccd`).
    pub stop_below_fraction: f64,
    /// While running, the engine-record value is replaced by this
    /// (`0x3fe66666`).
    pub running_engine_value: f64,
    /// While running, the thrust step's result is multiplied by this
    /// (`0x3f4ccccd`).
    pub running_result_factor: f64,
    /// Every value no measurement states, named by claim.
    pub unmeasured: Vec<CatalogueUnknown>,
}

impl OriginalNitroCatalogue {
    /// Whether the measured activation is "the control is held", which the
    /// runtime vocabulary can express.
    ///
    /// Always `false`: the measured rule starts on a full tank and ends on a
    /// low one, so a held control neither starts it nor keeps it going.
    #[must_use]
    pub const fn activation_is_while_held(&self) -> bool {
        false
    }

    /// The extra thrust, when a measurement states one in newtons.
    ///
    /// `None`: the measured `1.8` replaces an engine-record value whose unit
    /// the finding did not decode.
    #[must_use]
    pub const fn extra_thrust_n(&self) -> Option<f64> {
        None
    }

    /// Whether a claim id is recorded against the booster.
    #[must_use]
    pub fn names_claim(&self, claim: &str) -> bool {
        self.unmeasured.iter().any(|unknown| unknown.claim == claim)
    }
}

/// The whole measured catalogue: every hardpoint component the installation's
/// weapon member classifies, the records no family rule reaches (named, never
/// folded), the measured mount layout, and the nitro booster.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalCatalogue {
    /// One entry per classified record, in member order.
    pub entries: Vec<OriginalCatalogueEntry>,
    /// The records no T452.01 rule classifies, by name (the guns and the
    /// cannonball).
    pub excluded: Vec<String>,
    /// The measured nitro booster.
    pub nitro: OriginalNitroCatalogue,
    /// The rocket slots the ordnance screen declares.
    pub rocket_slots: u32,
    /// The gun/ammunition slots the ordnance screen declares.
    pub ammunition_slots: u32,
    /// The hardpoint points the hardpoint screen declares.
    pub hardpoint_points: u32,
    /// Values no single record owns (which airframe fills which slot).
    pub unmeasured: Vec<CatalogueUnknown>,
}

impl OriginalCatalogue {
    /// One entry by record name.
    #[must_use]
    pub fn entry(&self, record: &str) -> Option<&OriginalCatalogueEntry> {
        self.entries.iter().find(|entry| entry.record == record)
    }

    /// The entries of one family.
    #[must_use]
    pub fn of_family(&self, family: OrdnanceFamily) -> Vec<&OriginalCatalogueEntry> {
        self.entries
            .iter()
            .filter(|entry| entry.entry.family() == family)
            .collect()
    }

    /// Every distinct claim id the catalogue records, sorted.
    #[must_use]
    pub fn claims(&self) -> Vec<String> {
        let mut claims: Vec<String> = self
            .entries
            .iter()
            .flat_map(|entry| {
                entry
                    .entry
                    .unmeasured()
                    .iter()
                    .map(|unknown| unknown.claim.clone())
            })
            .chain(self.unmeasured.iter().map(|unknown| unknown.claim.clone()))
            .chain(
                self.nitro
                    .unmeasured
                    .iter()
                    .map(|unknown| unknown.claim.clone()),
            )
            .collect();
        claims.sort();
        claims.dedup();
        claims
    }
}

/// Why the measured catalogue could not be read from an installation.
#[derive(Clone, Debug, PartialEq)]
pub enum OriginalCatalogueError {
    /// The per-component table failed to read.
    Table {
        /// The reader's own refusal.
        detail: String,
    },
    /// The `ROCKET` blast records failed to read.
    Blasts {
        /// The reader's own refusal.
        detail: String,
    },
    /// A record name could not form a canonical weapon id.
    Id {
        /// The record name.
        record: String,
        /// Why the id was refused.
        source: cs_types::content::ContentIdError,
    },
    /// A runtime ordnance id could not be formed.
    Ordnance {
        /// The record name.
        record: String,
        /// Why the id was refused.
        source: OrdnanceIdError,
    },
    /// The catalogue entry contradicted its own family.
    Catalogue(CatalogueError),
    /// An unknown was recorded without a field, claim or reason.
    Unknown {
        /// The record the unknown belongs to.
        record: String,
        /// Why the unknown was refused.
        source: CatalogueError,
    },
}

impl std::fmt::Display for OriginalCatalogueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Table { detail } => write!(f, "the component table cannot be read: {detail}"),
            Self::Blasts { detail } => {
                write!(f, "the rocket blast records cannot be read: {detail}")
            }
            Self::Id { record, source } => {
                write!(
                    f,
                    "record {record:?} is not a canonical weapon id: {source}"
                )
            }
            Self::Ordnance { record, source } => {
                write!(
                    f,
                    "record {record:?} is not a runtime ordnance id: {source}"
                )
            }
            Self::Catalogue(source) => write!(f, "the catalogue entry is refused: {source}"),
            Self::Unknown { record, source } => {
                write!(f, "record {record:?} records a refused unknown: {source}")
            }
        }
    }
}

impl std::error::Error for OriginalCatalogueError {}

impl From<CatalogueError> for OriginalCatalogueError {
    fn from(source: CatalogueError) -> Self {
        Self::Catalogue(source)
    }
}

/// The catalogue field a claim id belongs to.
///
/// A claim says *what* is unknown; the catalogue field says *where* in a
/// component it would have been used. Claim ids that already name their field
/// map one-to-one, and a claim this stage did not write is carried verbatim so
/// it is still visible rather than dropped.
#[must_use]
pub fn catalogue_field(claim: &str) -> String {
    let field = match claim {
        cs_content::ordnance::T452_04_VELOCITY_UNIT => "launch.velocity",
        cs_content::ordnance::T452_04_RELEASE_DELAY => "launch.release_delay",
        cs_content::ordnance::T452_04_NO_EXPIRES => "lifetime_ticks",
        cs_content::ordnance::T452_04_STATUS_KIND => "status.kind",
        cs_content::ordnance::T452_04_STATUS_DURATION => "status.duration",
        cs_content::ordnance::T452_04_AREA_LIFETIME => "area_effect.lifetime",
        cs_content::ordnance::T452_04_AREA_RADIUS => "area_effect.radius",
        cs_content::ordnance::T452_04_MOUNT => "mount",
        cs_content::ordnance::T452_04_CLUSTER => "stack.sub_munitions",
        cs_content::ordnance::T452_04_PER_AIRFRAME => "airframe.per_slot",
        T452_FUSE_CLAIM => "fuse",
        T452_LOST_TARGET_CLAIM => "guidance.lost_target",
        T452_RUNTIME_LOWERING_CLAIM => "runtime_component",
        T452_NITRO_ACTIVATION_CLAIM => "nitro.activation",
        T452_NITRO_TRADEOFF_CLAIM => "nitro.tradeoff",
        other => other,
    };
    field.to_owned()
}

/// One measured record's named unknowns, in catalogue vocabulary.
fn catalogue_unknowns(
    record: &str,
    unknowns: &[cs_content::ordnance::OriginalComponentUnknown],
) -> Result<Vec<CatalogueUnknown>, OriginalCatalogueError> {
    let mut named = Vec::with_capacity(unknowns.len());
    for unknown in unknowns {
        let named_unknown = CatalogueUnknown::try_new(
            catalogue_field(&unknown.claim),
            unknown.claim.clone(),
            unknown.reason.clone(),
        )
        .map_err(|source| OriginalCatalogueError::Unknown {
            record: record.to_owned(),
            source,
        })?;
        named.push(named_unknown);
    }
    Ok(named)
}

/// The measured lost-target rule of one guided record, if it was measured.
///
/// The blast reader is of `ROCKET`-flagged records (task #454), so a guided
/// record without that flag — the glidebomb `wep_25` — has no measured rule and
/// the catalogue names [`T452_LOST_TARGET_CLAIM`] instead of assuming it.
///
/// The rule is measured per record by task #454, and what the end of `RANGE`
/// does is part of it: [`OriginalRangeEnd::Detonates`] is the measured
/// coast-then-detonate rule, and no retail rocket declares `EXPIRES`. A record
/// that *did* declare `EXPIRES` without `DETONATE_AT_RANGE` would not detonate
/// at the end of its range, and what removes it then is unmeasured
/// (`t454.limit.range_end_removal`), so it keeps the claim rather than being
/// folded into the measured rule.
fn measured_lost_target(
    record: &str,
    blasts: &[cs_content::ordnance::OriginalRocketBlast],
) -> Option<LostTargetBehavior> {
    let blast = blasts.iter().find(|blast| blast.record == record)?;
    match blast.lost_target_rule() {
        Some(rule) if rule.range_end == cs_content::ordnance::OriginalRangeEnd::Detonates => {
            Some(LostTargetBehavior::CoastThenDetonate)
        }
        _ => None,
    }
}

/// Builds the measured catalogue from an already-read component table, the
/// measured rocket blast records and the measured nitro booster.
///
/// Pure: no installation, no clock, no default. Every row becomes an entry;
/// every value the row does not state becomes a named claim on that entry; and
/// a row whose behavior contradicts its own family fails rather than being
/// repaired. The measured family/behaviour mapping is:
///
/// * a guided rocket or an aerial torpedo is a targeted seeker whose
///   lost-target rule is the measured `CoastThenDetonate` when task #454
///   measured it, and an unknown otherwise;
/// * an area-denial component and a proximity flak shell are unguided;
/// * the proximity fuse is the record's `DETONATION_DISTANCE` sphere, but only
///   for a seeker: the measured proximity path requires `LOCK_ON` (T452.02), so
///   an unguided record's `DETONATION_DISTANCE` never reaches it. It carries the
///   measured trigger shape ([`ProximityShape::Sphere`]) and sampling
///   ([`ProximitySampling::StartOfStepSample`]). A record with a stated duration
///   and no usable proximity key ends on that timer; anything else names
///   [`T452_FUSE_CLAIM`];
/// * arming has no delay at all beyond the fuse's own live conditions, which is
///   the measured rule (task T452.02) and is not expressible as an
///   [`ArmingRule`], whose `Disarmed` never arms.
///
/// # Errors
///
/// [`OriginalCatalogueError`] naming the record whose id or behavior is
/// invalid.
pub fn original_catalogue(
    table: &cs_content::ordnance::OriginalComponentTable,
    blasts: &[cs_content::ordnance::OriginalRocketBlast],
    nitro: &cs_content::ordnance::OriginalNitro,
) -> Result<OriginalCatalogue, OriginalCatalogueError> {
    let rocket_slots = table.layout.rocket_slots.slots;
    let ammunition_slots = table.layout.ammunition_slots.slots;
    let hardpoint_points = table.layout.hardpoint_points.slots;
    let mut entries = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let content =
            ContentId::from_source(ContentKind::Weapon, &row.record).map_err(|source| {
                OriginalCatalogueError::Id {
                    record: row.record.clone(),
                    source,
                }
            })?;
        let ordnance =
            OrdnanceId::try_new(content).map_err(|source| OriginalCatalogueError::Ordnance {
                record: row.record.clone(),
                source,
            })?;
        let family = lower_family(row.family);
        let mount = match &row.mount {
            cs_content::ordnance::OriginalMountDeclaration::RocketSlot { slots } => {
                CataloguedMount::RocketSlot { slots: *slots }
            }
            cs_content::ordnance::OriginalMountDeclaration::Unknown { claim, .. } => {
                CataloguedMount::Unknown {
                    claim: claim.clone(),
                }
            }
        };
        let mut unknowns = catalogue_unknowns(&row.record, &row.unknowns)?;
        let guidance = match family {
            OrdnanceFamily::GuidedRocket | OrdnanceFamily::AerialTorpedo
                if is_marker_seeker(row) =>
            {
                CataloguedGuidance::MarkerSeeker
            }
            OrdnanceFamily::GuidedRocket | OrdnanceFamily::AerialTorpedo => {
                let lost_target = measured_lost_target(&row.record, blasts);
                if lost_target.is_none() {
                    unknowns.push(named_unknown(
                        &row.record,
                        T452_LOST_TARGET_CLAIM,
                        "the measured rule (task #454) is coast-then-detonate at the end of \
                         a RANGE that detonates, and nothing measures this record's behavior \
                         after it loses its target, so no rule is assumed",
                    )?);
                }
                CataloguedGuidance::Targeted { lost_target }
            }
            _ => CataloguedGuidance::Unguided,
        };
        let fuse = match row
            .proximity_trigger_m
            .filter(|radius| *radius > 0.0)
            .filter(|_| guidance.is_targeted())
        {
            Some(radius) => {
                let fuse = ProximityFuse::try_new(radius)
                    .map_err(|_| CatalogueError::InvalidTriggerRadius { radius_m: radius })
                    .map_err(OriginalCatalogueError::Catalogue)?
                    .with_sampling(ProximitySampling::StartOfStepSample);
                CataloguedFuse::Proximity {
                    fuse,
                    shape: ProximityShape::Sphere,
                }
            }
            None => match row.lifetime {
                cs_content::ordnance::OriginalLifetime::Declared { seconds, .. } => {
                    CataloguedFuse::TimedSeconds { seconds }
                }
                cs_content::ordnance::OriginalLifetime::Undeclared => {
                    // A record can declare `DETONATION_DISTANCE` without being a
                    // seeker (`wep_12` does). The measured proximity path needs
                    // `LOCK_ON` (T452.02), so an unguided record's value never
                    // reaches it and is not catalogued as a fuse.
                    unknowns.push(named_unknown(
                        &row.record,
                        T452_FUSE_CLAIM,
                        "the record states no trigger the measured engine path reaches: it \
                         declares no duration, and a DETONATION_DISTANCE on an unguided \
                         record never reaches the proximity burst",
                    )?);
                    CataloguedFuse::Undeclared {
                        claim: T452_FUSE_CLAIM.to_owned(),
                    }
                }
            },
        };
        unknowns.push(named_unknown(
            &row.record,
            T452_RUNTIME_LOWERING_CLAIM,
            "the component is catalogued, not registered: its stack count is measured \
             (CLUSTER_SIZE, T452-R.03-B), its mass is measured as flat per enabled \
             rocket box with no type-dependent term (480 weight / 410 cost, \
             T452-R.03), its media ids are measured (T452-R.03-C) and its equipment \
             rules are measured empty (T452-R.03-D); the lowering step that \
             registers it is T452-RUNTIME-REGISTER's",
        )?);
        let entry = CataloguedOrdnance::try_new(
            ordnance,
            family,
            mount,
            guidance,
            fuse,
            CataloguedArming::OnRelease,
            unknowns,
        )?;
        entries.push(OriginalCatalogueEntry {
            record: row.record.clone(),
            entry,
        });
    }

    let mut table_unknowns = Vec::with_capacity(table.unknowns.len());
    for unknown in &table.unknowns {
        table_unknowns.push(
            CatalogueUnknown::try_new(
                catalogue_field(&unknown.claim),
                unknown.claim.clone(),
                unknown.reason.clone(),
            )
            .map_err(|source| OriginalCatalogueError::Unknown {
                record: "table".to_owned(),
                source,
            })?,
        );
    }

    let mut nitro_unknowns = vec![
        named_unknown(
            "nitro",
            T452_NITRO_ACTIVATION_CLAIM,
            "the measured rule starts a burn only at a nearly full charge and cuts it \
             below a fraction of capacity, which NitroActivationRule cannot express; the \
             fractions below are recorded and the rule is not lowered to a variant it is \
             not",
        )?,
        named_unknown(
            "nitro",
            T452_NITRO_TRADEOFF_CLAIM,
            "the measured 1.8 replaces an engine-record value and 0.8 scales an \
             unidentified result, so no extra thrust in newtons, no authority multiplier \
             and no drag penalty is asserted",
        )?,
    ];
    nitro_unknowns.push(named_unknown(
        "nitro",
        cs_content::ordnance::T452_04_NO_EXPIRES,
        "the booster is actor state, not a projectile: it has no fuse, no lifetime and \
         no `wep_` record at all",
    )?);

    Ok(OriginalCatalogue {
        entries,
        excluded: table.excluded.clone(),
        nitro: OriginalNitroCatalogue {
            control: cs_content::ordnance::ORIGINAL_NITRO_CONTROL,
            component_records: ORIGINAL_NITRO_COMPONENT_RECORDS,
            capacity: nitro.capacity,
            consumption_per_dt: nitro.consumption_per_dt,
            recovery_per_dt: nitro.recovery_per_dt,
            start_min_fraction: nitro.start_min_fraction,
            stop_below_fraction: nitro.stop_below_fraction,
            running_engine_value: nitro.running_engine_value,
            running_result_factor: nitro.running_result_factor,
            unmeasured: nitro_unknowns,
        },
        rocket_slots,
        ammunition_slots,
        hardpoint_points,
        unmeasured: table_unknowns,
    })
}

/// One named unknown, with the record it belongs to in the refusal.
fn named_unknown(
    record: &str,
    claim: &str,
    reason: &str,
) -> Result<CatalogueUnknown, OriginalCatalogueError> {
    CatalogueUnknown::try_new(catalogue_field(claim), claim, reason).map_err(|source| {
        OriginalCatalogueError::Unknown {
            record: record.to_owned(),
            source,
        }
    })
}

/// Reads the installation's measured ordnance catalogue.
///
/// The three readers are production code: `read_original_component_table` for
/// the per-component rows and the screens' layout, `read_original_rocket_blasts`
/// for the measured `ROCKET` blast/lost-target records, and
/// [`cs_content::ordnance::ORIGINAL_NITRO`] for the engine-image constants the
/// T452.03 finding recorded.
///
/// # Errors
///
/// [`OriginalCatalogueError`] when a reader fails or a row cannot be
/// catalogued.
pub fn read_original_catalogue(
    install_root: &std::path::Path,
) -> Result<OriginalCatalogue, OriginalCatalogueError> {
    let table =
        cs_content::ordnance::read_original_component_table(install_root).map_err(|error| {
            OriginalCatalogueError::Table {
                detail: error.to_string(),
            }
        })?;
    let blasts =
        cs_content::ordnance::read_original_rocket_blasts(install_root).map_err(|error| {
            OriginalCatalogueError::Blasts {
                detail: error.to_string(),
            }
        })?;
    original_catalogue(&table, &blasts, &cs_content::ordnance::ORIGINAL_NITRO)
}

/// Catalogues every entry of the measured catalogue in a registry.
///
/// This is the whole catalogue side of F28-D: after it, each id the
/// installation declares is queryable with its family and behavior, and the
/// values nobody measured are still named on the entry. Registering a *runtime*
/// component is a separate, stricter step ([`register`](OrdnanceRegistry::register)),
/// and no original entry is registered yet: the stack, mass, media and
/// equipment-rule measurements exist (T452-R.03), but the lowering step that
/// registers them is T452-RUNTIME-REGISTER's ([`T452_RUNTIME_LOWERING_CLAIM`]).
///
/// # Errors
///
/// [`OrdnanceRegistryError::DuplicateOrdnance`] when the registry already has a
/// catalogued entry for an id.
pub fn catalogue_measured_ordnance(
    catalogue: &OriginalCatalogue,
    registry: &mut OrdnanceRegistry,
) -> Result<usize, OrdnanceRegistryError> {
    let mut added = 0;
    for entry in &catalogue.entries {
        registry.catalogue(entry.entry.clone())?;
        added += 1;
    }
    Ok(added)
}

/// Whether every id the installation declares is catalogued in the registry,
/// and by which family.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalCatalogueCoverage {
    /// How many entries the measured catalogue holds.
    pub declared: usize,
    /// How many of them the registry has.
    pub catalogued: usize,
    /// The ids the registry does not have, by record name.
    pub missing: Vec<String>,
    /// Per family, how many ids the installation declares.
    pub families: Vec<(OrdnanceFamily, usize)>,
    /// The distinct claim ids the catalogue records.
    pub claims: Vec<String>,
}

impl OriginalCatalogueCoverage {
    /// Whether every declared id is catalogued.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.missing.is_empty() && self.declared == self.catalogued
    }
}

/// Measures registry coverage of a catalogue: every declared id, its family and
/// its recorded claims, without registering anything.
#[must_use]
pub fn original_catalogue_coverage(
    catalogue: &OriginalCatalogue,
    registry: &OrdnanceRegistry,
) -> OriginalCatalogueCoverage {
    let mut missing = Vec::new();
    let mut catalogued = 0;
    for entry in &catalogue.entries {
        if registry.catalogued(entry.entry.ordnance()).is_some() {
            catalogued += 1;
        } else {
            missing.push(entry.record.clone());
        }
    }
    let families = OrdnanceFamily::ALL
        .iter()
        .map(|family| (*family, catalogue.of_family(*family).len()))
        .collect();
    OriginalCatalogueCoverage {
        declared: catalogue.entries.len(),
        catalogued,
        missing,
        families,
        claims: catalogue.claims(),
    }
}

// ------------------- T452-RUNTIME-REGISTER: the measured lowerings ----------
//
// Task #2156/#2171. The catalogue above answers *which* components the
// installation declares and *what* their data states; this layer performs the
// step it deliberately left open, `t452.limit.runtime_lowering`: it maps the
// measured values of each catalogued record into a
// [`DeclaredOrdnance`] and lowers that through [`lower_ordnance`] into the
// runtime [`OrdnanceComponent`] a session registers.
//
// Every value comes from a measurement, and where a measurement cannot be
// expressed by the runtime vocabulary the record is **blocked** with a named
// claim, its affected content and the task that resolves it — never defaulted.
// The measurements this layer reads are tasks T452-R.01 (launch geometry),
// T452-R.02 (effects, fuses, swarm), T452-R.03 (stack, media, equipment,
// per-box weight), T452-R.04 (flight duration, release binding) and T452-R.05
// (slot reach, incapacitation); every one of them is `static` analysis of the
// owner-supplied engine image or a read of the owner's installation, never
// `verified_original`.

/// The claim id of the `weapons.zrd` member every declared record comes from:
/// the provenance of a field read out of the owner's installation.
pub const T452_ORIGINAL_WEAPONS_CLAIM: &str = "t452_register.original_weapons_member";

/// Claim: the launch speed, inheritance and range are T452-R.01's measurement.
pub const T452_MEASURED_LAUNCH_CLAIM: &str = "t452_register.launch_geometry";

/// Claim: the per-slot stack capacity is the record's `CLUSTER_SIZE`
/// (T452-R.03-B) and the named item counts one unit per launch (T452-R.02).
pub const T452_MEASURED_STACK_CLAIM: &str = "t452_register.stack_load";

/// Claim: the fuse, lost-target rule, area and swarm are T452-R.02's
/// measurement.
pub const T452_MEASURED_BEHAVIOUR_CLAIM: &str = "t452_register.behavior";

/// Claim: the visual and launch sound are the record's own `FLYOUT` `MODEL`
/// and `FIRE` `SOUND[0]` (T452-R.03-C).
pub const T452_MEASURED_MEDIA_CLAIM: &str = "t452_register.media";

/// Claim: the damage channels are the record's own `ARMOR_DAMAGE` /
/// `HEALTH_DAMAGE` keys, and a `DAMAGE`-only record with no measured channel
/// split is refused rather than assigned one.
pub const T452_MEASURED_DAMAGE_CLAIM: &str = "t452_register.damage";

/// Claim: a status effect's kind and `TIME` duration are T452-R.02's and
/// T452-R.05's measurement.
pub const T452_MEASURED_STATUS_CLAIM: &str = "t452_register.status";

/// Provenance class of the two **designed** fields this lowering has to fill:
/// the hardpoint kind and the mount key. Both vocabularies are engine design
/// (`cs_content::ordnance::DeclaredHardpointKind` is documented as designed,
/// and a `DamageNodeKey` is a project key, not an original name), so the
/// *value* they carry — a measured mount id — is what is measured, and the
/// vocabulary is not.
pub const T452_DESIGNED_LAUNCH_NAMES_CLAIM: &str = "t452_register.designed_launch_names";

/// Claim: the measured end of flight is a silent expiry at the record's
/// `RANGE`, which the runtime fuse vocabulary has no variant for.
///
/// **Resolved** by [`T452_SILENT_EXPIRY_TASK`]: `FuseRule::ExpiresAtRange` now
/// expresses it and no record is blocked on this claim. The id is kept, as a
/// resolved claim id is elsewhere in this crate, because the earlier stage's
/// evidence report names it; the catalogue layer it came from is unchanged.
pub const T452_SILENT_EXPIRY_BLOCKED: &str = "t452_register.limit.silent_expiry";

/// Recorded (not blocking): the measured per-box weight and cost have no
/// runtime consumer, and the weight unit's conversion to kilograms is
/// unmeasured. Resolving task: [`T452_PER_BOX_WEIGHT_TASK`].
pub const T452_PER_BOX_WEIGHT_CLAIM: &str = "t452_register.limit.per_box_weight";

/// The task that added the measured silent range-end expiry to the runtime fuse
/// vocabulary (`FuseRule::ExpiresAtRange`, this stage). No record is blocked on
/// it any more; the unmeasured "this record's flight never ends" branch still
/// names it, because expressing a flight nothing measured ends is the same
/// vocabulary question.
pub const T452_SILENT_EXPIRY_TASK: &str = "T452-REG-SILENT-EXPIRY";

/// The task that gives the measured per-box weight and cost a consumer.
pub const T452_PER_BOX_WEIGHT_TASK: &str = "T452-REG-PER-BOX-WEIGHT";

/// The hardpoint kind this lowering assigns to every measured rocket-slot
/// component.
///
/// **Designed, on purpose.** No `weapons.zrd` record and no screen states where
/// on the airframe a slot sits: the measured layout is one eight-entry rocket
/// slot array plus two hardpoint box counts
/// (`cs_content::ordnance::OriginalMountLayout`, T452.04), which places nothing.
/// `HardpointKind` is documented as designed vocabulary that F28-D maps onto,
/// and the ordnance screen's rocket array is the airframe's underside rack, so
/// every measured component gets the same kind rather than an invented
/// per-record position.
pub const ORIGINAL_ORDNANCE_HARDPOINT: DeclaredHardpointKind = DeclaredHardpointKind::Underslung;

/// The unit mass a measured original component's stack carries.
///
/// **Measured as an absence**, which is why it is zero and not a tonnage: the
/// airframe weight total adds a flat `480` weight per *enabled rocket box* and
/// reads no weapon record field at all (`0x40558e`; T452-R.03,
/// `docs/findings/2026-10-28-t452-r03-hardpoint-price-and-gate-table.md` §2), so
/// a loaded type contributes **no** per-type mass term. The flat 480/410 is
/// charged per box by the purchase surface, not per round, and the weight
/// unit's conversion to kilograms is unmeasured — both are named by
/// [`T452_PER_BOX_WEIGHT_CLAIM`] rather than converted into a kilogram figure
/// here.
pub const ORIGINAL_ORDNANCE_UNIT_MASS_KG: f64 = 0.0;

/// The lifetime the runtime gives an instantaneous blast's area.
///
/// A blast applies its damage once, at detonation (T1318), and leaves no area
/// behind (`MeasuredAreaSource::InstantaneousBlast`, T452-R.02 §2). The
/// runtime's [`AreaEffect`] refuses a zero lifetime and applies its damage once
/// when it opens, so the shortest bounded lifetime it has — one tick — is the
/// faithful stand-in for "applied once, gone".
pub const ORIGINAL_BLAST_AREA_TICKS: u64 = 1;

/// One value that keeps a measured original id out of the runtime registry.
///
/// A blocker is *not* a failure of the reading: it names a measured value the
/// runtime cannot express, the content it affects and the task that resolves
/// it, so an unregistered id is never mistaken for a component nobody measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalRegistrationBlocker {
    /// The record (`wep_04`).
    pub record: String,
    /// The declared field the value would have filled.
    pub field: &'static str,
    /// The claim id the gap is recorded under.
    pub claim: String,
    /// The content the gap affects.
    pub affects: String,
    /// The task that resolves it.
    pub resolving_task: &'static str,
}

impl OriginalRegistrationBlocker {
    fn new(
        record: &str,
        field: &'static str,
        claim: &str,
        affects: &str,
        resolving_task: &'static str,
    ) -> Self {
        Self {
            record: record.to_owned(),
            field,
            claim: claim.to_owned(),
            affects: affects.to_owned(),
            resolving_task,
        }
    }
}

impl std::fmt::Display for OriginalRegistrationBlocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} is unexpressible ({}: {}) — resolving task {}",
            self.record, self.field, self.claim, self.affects, self.resolving_task
        )
    }
}

/// The outcome of lowering and registering one measured original id.
#[derive(Clone, Debug, PartialEq)]
pub enum OriginalRegistrationOutcome {
    /// The measured record lowered into a runtime component and the registry
    /// accepted it.
    Registered {
        /// The record (`wep_05`).
        record: String,
        /// The runtime id it was registered under.
        ordnance: OrdnanceId,
        /// The family it registered as.
        family: OrdnanceFamily,
    },
    /// The record's measured values cannot be expressed yet: nothing was
    /// registered and every reason is named.
    Blocked {
        /// The record (`wep_12`).
        record: String,
        /// One entry per unexpressible value.
        blockers: Vec<OriginalRegistrationBlocker>,
    },
}

impl OriginalRegistrationOutcome {
    /// The record this outcome is about.
    #[must_use]
    pub fn record(&self) -> &str {
        match self {
            Self::Registered { record, .. } | Self::Blocked { record, .. } => record,
        }
    }

    /// Whether the id reached the runtime registry.
    #[must_use]
    pub const fn is_registered(&self) -> bool {
        matches!(self, Self::Registered { .. })
    }
}

/// What one measured catalogue produced when it was lowered and registered.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalRegistrationReport {
    /// How many ids the installation declares.
    pub declared: usize,
    /// One outcome per declared id, in member order.
    pub outcomes: Vec<OriginalRegistrationOutcome>,
    /// The claims this stage keeps, per record, because the runtime cannot
    /// express the measured value behind them.
    pub claims_kept: Vec<(String, Vec<String>)>,
    /// The measured tangler fields the registered records open at fire time,
    /// one per registered record that declares a `TANGLER` field (task #2186).
    ///
    /// They are the producer's input to
    /// [`OrdnanceSession::register_tangler_field`]: a record's field is
    /// measured alongside the component, so the report carries both answers
    /// and a caller never has to re-read the record to find the field.
    pub tangler_fields: Vec<TanglerFieldDeclaration>,
}

impl OriginalRegistrationReport {
    /// The records that reached the registry, in member order.
    #[must_use]
    pub fn registered_records(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter(|outcome| outcome.is_registered())
            .map(OriginalRegistrationOutcome::record)
            .collect()
    }

    /// The records that did not, in member order.
    #[must_use]
    pub fn blocked_records(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter(|outcome| !outcome.is_registered())
            .map(OriginalRegistrationOutcome::record)
            .collect()
    }

    /// Every blocker, in member order.
    #[must_use]
    pub fn blockers(&self) -> Vec<&OriginalRegistrationBlocker> {
        self.outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                OriginalRegistrationOutcome::Blocked { blockers, .. } => Some(blockers.iter()),
                OriginalRegistrationOutcome::Registered { .. } => None,
            })
            .flatten()
            .collect()
    }

    /// Whether every declared id reached the registry.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self.outcomes.is_empty() && self.blocked_records().is_empty()
    }
}

/// Why the measured catalogue could not be read for registration.
#[derive(Clone, Debug, PartialEq)]
pub enum OriginalRegistrationError {
    /// The component table failed to read.
    Table {
        /// The reader's own refusal.
        detail: String,
    },
    /// The weapon records failed to read.
    Records {
        /// The reader's own refusal.
        detail: String,
    },
    /// The media declarations failed to read.
    Media {
        /// The reader's own refusal.
        detail: String,
    },
    /// The ammunition declarations failed to read.
    Ammo {
        /// The reader's own refusal.
        detail: String,
    },
    /// The record member's provenance could not be measured.
    Provenance {
        /// The reader's own refusal.
        detail: String,
    },
}

impl std::fmt::Display for OriginalRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Table { detail } => write!(f, "the component table cannot be read: {detail}"),
            Self::Records { detail } => write!(f, "the weapon records cannot be read: {detail}"),
            Self::Media { detail } => write!(f, "the media declarations cannot be read: {detail}"),
            Self::Ammo { detail } => {
                write!(f, "the ammunition declarations cannot be read: {detail}")
            }
            Self::Provenance { detail } => {
                write!(
                    f,
                    "the record member's provenance cannot be measured: {detail}"
                )
            }
        }
    }
}

impl std::error::Error for OriginalRegistrationError {}

/// A measured field, stamped with the provenance of the record it was read
/// from and the claim id of the measurement that read it.
fn measured_field<T>(provenance: &Provenance, claim: &str, value: T) -> Resolved<T> {
    let claim_id = ClaimId::new(claim).expect("an engine-authored claim id is valid");
    Resolved::Known(Known::new(
        value,
        Provenance {
            claim_id,
            ..provenance.clone()
        },
    ))
}

/// A **designed** field, carrying the measured value it names under a designed
/// provenance: the vocabulary is the project's, the value is not.
fn designed_field<T>(claim: &str, value: T) -> Resolved<T> {
    let claim_id = ClaimId::new(claim).expect("an engine-authored claim id is valid");
    Resolved::Known(Known::new(value, Provenance::designed(claim_id)))
}

/// Whole ticks a measured duration covers, rounding up as the original's step
/// does (T452-R.04: a flight ends on the first step at or after the continuous
/// time).
fn seconds_to_ticks(seconds: f64, ticks_per_second: u32) -> u64 {
    let ticks = (seconds * f64::from(ticks_per_second)).ceil();
    if ticks < 1.0 { 1 } else { ticks as u64 }
}

/// The damage channels a measured record states.
///
/// A record that declares `ARMOR_DAMAGE` and `HEALTH_DAMAGE` states both. One
/// that declares only `DAMAGE` states a single channel whose split into the
/// runtime's two is unmeasured: a **zero** value is therefore read as zero on
/// both (the record declares no damage at all) and any other value is refused
/// rather than halved, duplicated or assigned to a guessed channel.
fn measured_damage(
    row: &cs_content::ordnance::OriginalComponentLaunch,
) -> Result<(f64, f64), &'static str> {
    match (row.damage.armor, row.damage.health, row.damage.single) {
        (Some(armor), Some(health), _) => Ok((armor, health)),
        (None, None, Some(0.0)) => Ok((0.0, 0.0)),
        (None, None, Some(_)) => {
            Err("the record declares one DAMAGE amount and the measured channel split is unknown")
        }
        (None, None, None) => Ok((0.0, 0.0)),
        (armor, health, _) => Ok((armor.unwrap_or(0.0), health.unwrap_or(0.0))),
    }
}

/// Whether a measured record declares the `TANGLER` effect, whose measured
/// area is a persistent per-tick field rather than a one-shot blast.
fn is_tangler(row: &OriginalComponentLaunch) -> bool {
    row.statuses.iter().any(|status| status.key == "TANGLER")
}

/// The measured `TANGLER` field a record declares, through the production
/// lowering of its vocabulary.
///
/// The field exists only for a record whose measured area has a lifetime — a
/// persistent field — and `None` for every other record: a blast applies once
/// and has no field, and a record with no measured area opens nothing. The
/// radius is the image's own default (T452-R.02 §1: no retail `wep_` record
/// states `RADIUS`, so the record key is `None` and the lowering labels the
/// radius `ImageDefault`), never a value chosen here.
///
/// The declaration is the producer's answer to "does this component open a
/// field, and which one"; it is handed to
/// [`OrdnanceSession::register_tangler_field`], which opens it from the launch
/// order's pose at fire time.
#[must_use]
pub fn measured_tangler_field(row: &OriginalComponentLaunch) -> Option<TanglerFieldDeclaration> {
    let area = cs_content::ordnance::measure_component_behavior(row).area?;
    // A field persists for its measured `TIME`; a blast's area has none.
    area.lifetime_s?;
    cs_content::ordnance::lower_tangler_field(row, None)
}

/// Lowers **one** measured original record into a declared component.
///
/// The inputs are the production readers' outputs: the T452.04 component row,
/// the record itself (for its launch geometry, T452-R.01/R.04), its media
/// declarations (T452-R.03-C), its ammunition declaration (T452-R.03-B) and the
/// provenance of the member they all came from. `ticks_per_second` converts the
/// measurements' seconds into the runtime's whole ticks; it is the session's
/// declared rate, not a constant.
///
/// Every field is filled from a measurement. Where the measured value cannot be
/// expressed the record is refused with one [`OriginalRegistrationBlocker`] per
/// such value, so a caller never sees a partially measured component.
///
/// # Errors
///
/// The blockers, one per unexpressible measured value.
pub fn measured_ordnance_declaration(
    row: &cs_content::ordnance::OriginalComponentLaunch,
    record: &cs_content::ordnance::OriginalWeaponRecord,
    media: &cs_content::ordnance::OriginalWeaponMedia,
    ammo: &cs_content::ordnance::OriginalAmmoDeclaration,
    provenance: &Provenance,
    ticks_per_second: u32,
) -> Result<DeclaredOrdnance, Vec<OriginalRegistrationBlocker>> {
    let name = row.record.as_str();
    let mut blockers = Vec::new();
    let geometry = cs_content::ordnance::original_launch_geometry(record);
    let behavior = cs_content::ordnance::measure_component_behavior(row);

    let target = ContentId::from_source(ContentKind::Weapon, name)
        .expect("a record name is a valid weapon key");
    let origin = Origin::Installation {
        source: provenance
            .source
            .clone()
            .expect("the record member's provenance carries its span"),
    };

    // Launch geometry: the measured speed, inheritance, acceleration and
    // (measured) absence of a release delay. The refire interval the mount
    // row states delays the *next* release, never the current one
    // (T452-R.04 §1), so the component's own release delay is the measured
    // zero. The `ACCELERATION` field is carried only for the rule that runs
    // it: for a `Full` or `INSTANT` record the parser's `+0x38` plays no
    // part in the measured step, so `None` is the measured value, not a
    // dropped declaration.
    let inheritance = match geometry.inheritance {
        cs_content::ordnance::OriginalInheritance::Full => {
            designed_field(T452_MEASURED_LAUNCH_CLAIM, DeclaredInheritanceRule::Full)
        }
        cs_content::ordnance::OriginalInheritance::LateralRemainder => designed_field(
            T452_MEASURED_LAUNCH_CLAIM,
            DeclaredInheritanceRule::LateralRemainder,
        ),
        cs_content::ordnance::OriginalInheritance::Instant => {
            designed_field(T452_MEASURED_LAUNCH_CLAIM, DeclaredInheritanceRule::Instant)
        }
    };
    let acceleration_mps2 = geometry.acceleration_mps2.filter(|_| {
        geometry.inheritance == cs_content::ordnance::OriginalInheritance::LateralRemainder
    });
    let mount_id = cs_content::ordnance::original_mount_id(name);
    let mount = designed_field(
        T452_DESIGNED_LAUNCH_NAMES_CLAIM,
        DamageNodeKey::new(&format!("weapon_mount_{mount_id:02}"))
            .expect("the mount key shape is valid"),
    );
    let launch = DeclaredLaunchGeometry {
        mount: known_or_refuse("launch.mount", &mount).expect("a designed mount key is known"),
        hardpoint: designed_field(
            T452_DESIGNED_LAUNCH_NAMES_CLAIM,
            ORIGINAL_ORDNANCE_HARDPOINT,
        ),
        launch_speed_mps: measured_field(
            provenance,
            T452_MEASURED_LAUNCH_CLAIM,
            geometry.speed.metres_per_second(),
        ),
        inheritance: match inheritance {
            Resolved::Known(known) => Resolved::Known(known),
            Resolved::Unknown { claim_id, reason } => Resolved::Unknown { claim_id, reason },
        },
        acceleration_mps2: measured_field(
            provenance,
            T452_MEASURED_LAUNCH_CLAIM,
            acceleration_mps2,
        ),
        // The `RANGE` field is carried only for the rule that reads it: for
        // a `Full` or `LateralRemainder` record the parser's `+0x1c` is the
        // flight's end bound, not a step displacement, so `None` is the
        // measured value, not a dropped declaration.
        instant_range_m: measured_field(
            provenance,
            T452_MEASURED_LAUNCH_CLAIM,
            (geometry.inheritance == cs_content::ordnance::OriginalInheritance::Instant)
                .then(|| geometry.range.metres()),
        ),
        release_delay_ticks: measured_field(provenance, T452_MEASURED_LAUNCH_CLAIM, 0),
    };

    // Stack: the measured per-slot capacity is `CLUSTER_SIZE` (T452-R.03-B) and
    // one launch spends one unit of it (T452-R.02 §5, one projectile per
    // sub-munition).
    let capacity = match row.cluster {
        cs_content::ordnance::OriginalClusterShape::Volley { munitions } => {
            // The ammunition declaration is a second reader of the same key:
            // a disagreement means the two readings are not the same record.
            if ammo.cluster_size != Some(f64::from(munitions)) {
                blockers.push(OriginalRegistrationBlocker::new(
                    name,
                    "stack.capacity_units",
                    T452_MEASURED_STACK_CLAIM,
                    "the record's CLUSTER_SIZE: the table and the ammunition declaration \
                     disagree about it",
                    "a #2156 follow-up: two readings of one key disagree",
                ));
            }
            u64::from(munitions)
        }
        cs_content::ordnance::OriginalClusterShape::AmmunitionCount { .. }
        | cs_content::ordnance::OriginalClusterShape::Undeclared => {
            blockers.push(OriginalRegistrationBlocker::new(
                name,
                "stack.capacity_units",
                T452_MEASURED_STACK_CLAIM,
                "the per-slot stack this record's launcher holds: the record states no \
                 CLUSTER_SIZE, so no file states its slot count",
                "a #2156 follow-up that measures the per-slot count of a record without \
                 CLUSTER_SIZE",
            ));
            1
        }
    };
    let stack = DeclaredStackLoad {
        capacity_units: measured_field(provenance, T452_MEASURED_STACK_CLAIM, capacity),
        // Measured as an absence: see `ORIGINAL_ORDNANCE_UNIT_MASS_KG`.
        unit_mass_kg: measured_field(
            provenance,
            T452_MEASURED_STACK_CLAIM,
            ORIGINAL_ORDNANCE_UNIT_MASS_KG,
        ),
    };

    // Lifetime: the measured rule is `RANGE` over the flown speed (T452-R.04
    // §2), which depends on the launcher's velocity at release. The runtime
    // field is a fixed cap, so it carries the record's own rule evaluated at a
    // stationary launcher, rounded up to a tick.
    let duration = geometry.flight_duration([0.0; 3], [0.0, 0.0, 1.0]);
    let lifetime_ticks = match duration.seconds {
        Some(seconds) => measured_field(
            provenance,
            T452_MEASURED_BEHAVIOUR_CLAIM,
            seconds_to_ticks(seconds, ticks_per_second),
        ),
        None => {
            blockers.push(OriginalRegistrationBlocker::new(
                name,
                "lifetime_ticks",
                T452_MEASURED_BEHAVIOUR_CLAIM,
                "how long this record's item lives: its measured flight end never ends it",
                T452_SILENT_EXPIRY_TASK,
            ));
            measured_field(provenance, T452_MEASURED_BEHAVIOUR_CLAIM, 1)
        }
    };
    let lifetime_value = match &lifetime_ticks {
        Resolved::Known(known) => known.value,
        Resolved::Unknown { .. } => 1,
    };

    // Fuse: the record's own `DETONATION_DISTANCE` sphere for a seeker (the
    // measured proximity path needs `LOCK_ON`, T452-R.02 §3), a declared
    // `DETONATION_TIME` as a timed fuse, and otherwise the measured end of the
    // record's flight at `RANGE` (T452-R.02 §3). That last end has two measured
    // shapes, and the difference is the whole reason this task exists:
    //
    // * `ExpiresSilently` — the item is consumed at the end of `RANGE` with no
    //   detonation (`0x5b0318`). `FuseRule::ExpiresAtRange` expresses it, with
    //   the record's own bound.
    // * `Detonates` — the end is a detonation, gated on `LOCK_ON` without
    //   `EXPIRES` or on `DETONATE_AT_RANGE`. A guided record's detonation is
    //   its lost-target rule (`CoastThenDetonate`), whose end is the lifetime
    //   cap — T452-R.02 §4 measured that rule over exactly the `LOCK_ON`
    //   records — so a clock at that cap is the end. No measured record is
    //   both unguided and range-end detonating (T548 measured `DETONATE_AT_RANGE`
    //   on none), so the unguided case is not asserted by this mapping.
    let targeted = matches!(
        row.family,
        DeclaredOrdnanceFamily::GuidedRocket | DeclaredOrdnanceFamily::AerialTorpedo
    );
    let fuse = match row
        .proximity_trigger_m
        .filter(|radius| *radius > 0.0)
        .filter(|_| targeted)
    {
        Some(radius) => DeclaredFuseRule::Proximity(DeclaredProximityFuse {
            trigger_radius_m: measured_field(provenance, T452_MEASURED_BEHAVIOUR_CLAIM, radius),
        }),
        None => match row.lifetime {
            cs_content::ordnance::OriginalLifetime::Declared { seconds, .. } => {
                DeclaredFuseRule::Timed {
                    ticks: measured_field(
                        provenance,
                        T452_MEASURED_BEHAVIOUR_CLAIM,
                        seconds_to_ticks(seconds, ticks_per_second),
                    ),
                }
            }
            cs_content::ordnance::OriginalLifetime::Undeclared => match behavior.fuse.range_end {
                cs_content::ordnance::MeasuredRangeEnd::ExpiresSilently => {
                    DeclaredFuseRule::ExpiresAtRange {
                        range_m: measured_field(
                            provenance,
                            T452_MEASURED_BEHAVIOUR_CLAIM,
                            geometry.range.metres(),
                        ),
                    }
                }
                cs_content::ordnance::MeasuredRangeEnd::Detonates => DeclaredFuseRule::Timed {
                    ticks: measured_field(
                        provenance,
                        T452_MEASURED_BEHAVIOUR_CLAIM,
                        lifetime_value,
                    ),
                },
            },
        },
    };

    // Guidance: measured, including the lost-target rule task T452-R.02 §4
    // measured over every `LOCK_ON` record.
    let guidance = match (targeted, behavior.lost_target) {
        // `BEEPER_SEEKER` replaces the one-target homing (T452-R.02 §1): the
        // item re-selects the newest live marker every step and has no
        // lost-target rule of its own.
        (true, _) if is_marker_seeker(row) => DeclaredGuidanceRule::MarkerSeeker,
        (true, Some(_)) => DeclaredGuidanceRule::Targeted {
            lost_target: designed_field(
                T452_MEASURED_BEHAVIOUR_CLAIM,
                DeclaredLostTargetBehavior::CoastThenDetonate,
            ),
        },
        (true, None) => {
            blockers.push(OriginalRegistrationBlocker::new(
                name,
                "guidance.lost_target",
                T452_LOST_TARGET_CLAIM,
                "what this seeker does when it loses its target",
                "a #454 follow-up that measures this record's lost-target behaviour",
            ));
            DeclaredGuidanceRule::Targeted {
                lost_target: Resolved::unknown(
                    ClaimId::new(T452_LOST_TARGET_CLAIM).expect("a valid claim id"),
                    "no measurement states this record's behaviour after a lost target",
                )
                .expect("the reason is non-empty"),
            }
        }
        (false, _) => DeclaredGuidanceRule::Unguided,
    };

    // Area: a measured radius with no measured lifetime is an instantaneous
    // blast (T1318, T452-R.02 §2); a measured lifetime is the field's `TIME`.
    let area_effect = match behavior.area {
        None => None,
        Some(area) => match (area.radius_m, area.lifetime_s) {
            (Some(radius), None) => Some(DeclaredAreaEffect {
                radius_m: measured_field(provenance, T452_MEASURED_BEHAVIOUR_CLAIM, radius),
                lifetime_ticks: measured_field(
                    provenance,
                    T452_MEASURED_BEHAVIOUR_CLAIM,
                    ORIGINAL_BLAST_AREA_TICKS,
                ),
            }),
            (Some(radius), Some(seconds)) => {
                // A `TANGLER` record's measured area is not the runtime's
                // bounded blast area: it is a field the original creates at
                // fire time whose tick re-applies the weapon's hit to every
                // actor inside it (T452-R.02 §1). That shape has its own
                // expression — [`measured_tangler_field`] and the session's
                // [`TanglerLedger`] — so no one-shot `AreaEffect` is declared
                // here and the field is never mis-flown as a blast that
                // applies once at detonation.
                if is_tangler(row) {
                    None
                } else {
                    let ticks = seconds_to_ticks(seconds, ticks_per_second);
                    // `measured_area` yields a timed area **with a radius**
                    // only for `TANGLER`, which the branch above expresses as
                    // its own field; this arm is the fallback for any future
                    // measurement of another persistent record area. The
                    // runtime's bounded area applies once and cannot outlive
                    // its item, so a record whose measured area is longer than
                    // its own flight cannot be expressed as one and is blocked
                    // rather than flown as a blast.
                    if ticks > lifetime_value {
                        blockers.push(OriginalRegistrationBlocker::new(
                            name,
                            "area_effect.lifetime_ticks",
                            T452_MEASURED_BEHAVIOUR_CLAIM,
                            "this record's bounded area: it is measured to last longer than \
                             the item that creates it, which the runtime's one-shot area \
                             record refuses",
                            "a #452 follow-up that measures a persistent area the runtime \
                             can express",
                        ));
                    }
                    Some(DeclaredAreaEffect {
                        radius_m: measured_field(provenance, T452_MEASURED_BEHAVIOUR_CLAIM, radius),
                        lifetime_ticks: measured_field(
                            provenance,
                            T452_MEASURED_BEHAVIOUR_CLAIM,
                            ticks,
                        ),
                    })
                }
            }
            (None, _) => {
                // A measured lifetime with no measured radius (the smoke
                // emitter) cannot form a bounded area: the claim stays named on
                // the catalogue entry and no area is asserted.
                None
            }
        },
    };

    let (armor, internal) = match measured_damage(row) {
        Ok(channels) => channels,
        Err(reason) => {
            blockers.push(OriginalRegistrationBlocker::new(
                name,
                "damage",
                T452_MEASURED_DAMAGE_CLAIM,
                reason,
                "a #452 follow-up that measures the single DAMAGE channel's split",
            ));
            (0.0, 0.0)
        }
    };

    // Status: only a measured effect whose runtime kind fits becomes one. A
    // `SONIC`/`FLASH` exposure is pilot incapacitation whose duration is
    // computed per hit (`cs_sim::weapons::incapacitation`), so it declares no
    // fixed-duration status here; the kind is `Incapacitation` (task #2172).
    let mut status = Vec::new();
    for effect in &behavior.effects {
        let Some(kind) = effect.runtime_kind else {
            continue;
        };
        let seconds = match effect.duration {
            cs_content::ordnance::MeasuredEffectDuration::RecordTime => row
                .statuses
                .iter()
                .find(|status| status.key == effect.key)
                .and_then(|status| status.duration_s),
            cs_content::ordnance::MeasuredEffectDuration::ImageDefaultSeconds(seconds) => {
                Some(seconds)
            }
            cs_content::ordnance::MeasuredEffectDuration::Exposure { .. } => continue,
            cs_content::ordnance::MeasuredEffectDuration::None => None,
        };
        let Some(seconds) = seconds else {
            blockers.push(OriginalRegistrationBlocker::new(
                name,
                "status.duration_ticks",
                T452_MEASURED_STATUS_CLAIM,
                "the duration of this record's measured effect",
                "a #452 follow-up that measures the effect's duration",
            ));
            continue;
        };
        status.push(DeclaredStatusEffect {
            kind,
            duration_ticks: measured_field(
                provenance,
                T452_MEASURED_STATUS_CLAIM,
                seconds_to_ticks(seconds, ticks_per_second),
            ),
            // A marker's magnitude is ignored by its own kind, and the record
            // states none (T452-R.02 §1).
            strength: measured_field(provenance, T452_MEASURED_STATUS_CLAIM, 0.0),
        });
    }

    // Media: the record's own `FLYOUT` model and `FIRE` sound. A record that
    // declares no launch sound is measured silent: the sound stays absent.
    let media_claim = ClaimId::new(T452_MEASURED_MEDIA_CLAIM).expect("a valid claim id");
    let declared_media = media.measured_ordnance_media(&media_claim);
    if !declared_media.visual.is_known() {
        blockers.push(OriginalRegistrationBlocker::new(
            name,
            "media",
            T452_MEASURED_MEDIA_CLAIM,
            "the launch presentation: the record declares no FLYOUT MODEL, so the round \
             has no visual of its own (no retail hardpoint record is in this state)",
            "a #452 follow-up, only if a record without a FLYOUT MODEL is ever measured",
        ));
    }

    if !blockers.is_empty() {
        return Err(blockers);
    }

    let projectile = DeclaredProjectile {
        launch,
        stack,
        arming: DeclaredArmingRule::AfterTicks(measured_field(
            provenance,
            T452_MEASURED_BEHAVIOUR_CLAIM,
            0,
        )),
        fuse,
        guidance,
        lifetime_ticks,
        area_effect,
        smoke_stun: behavior.smoke_stun.clone(),
        armor_damage: measured_field(provenance, T452_MEASURED_DAMAGE_CLAIM, armor),
        internal_damage: measured_field(provenance, T452_MEASURED_DAMAGE_CLAIM, internal),
        status,
        media: declared_media,
        // The measured equipment rule set is empty (T452-R.03-D).
        equipment_rules: DeclaredEquipmentRules::default(),
    };
    DeclaredOrdnance::try_new(
        target,
        origin,
        row.family,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        provenance.clone(),
    )
    .map_err(|error| {
        vec![OriginalRegistrationBlocker::new(
            name,
            "component",
            T452_MEASURED_BEHAVIOUR_CLAIM,
            &error.to_string(),
            "a #2171 follow-up: the measured record contradicts its own family",
        )]
    })
}

/// Lowers and registers every measured original component the runtime
/// vocabulary can express.
///
/// The table, the weapon records, the media declarations, the ammunition
/// declarations and the member's provenance are all production readers'
/// outputs. Each record is lowered by [`measured_ordnance_declaration`],
/// converted by [`lower_ordnance`] and inserted with
/// [`OrdnanceRegistry::register`]; a record whose measured values the runtime
/// cannot express is reported as
/// [`OriginalRegistrationOutcome::Blocked`] with its named claims and registers
/// nothing.
pub fn register_measured_ordnance(
    table: &cs_content::ordnance::OriginalComponentTable,
    records: &[cs_content::ordnance::OriginalWeaponRecord],
    media: &[cs_content::ordnance::OriginalWeaponMedia],
    ammo: &[cs_content::ordnance::OriginalAmmoDeclaration],
    provenance: &Provenance,
    registry: &mut OrdnanceRegistry,
    ticks_per_second: u32,
) -> OriginalRegistrationReport {
    let mut outcomes = Vec::with_capacity(table.rows.len());
    let mut claims_kept = Vec::new();
    let mut tangler_fields = Vec::new();
    for row in &table.rows {
        let name = row.record.as_str();
        let Some(record) = records.iter().find(|record| record.record == name) else {
            outcomes.push(OriginalRegistrationOutcome::Blocked {
                record: row.record.clone(),
                blockers: vec![OriginalRegistrationBlocker::new(
                    name,
                    "record",
                    T452_MEASURED_LAUNCH_CLAIM,
                    "the record itself: the installation's weapon member has no record by \
                     this name",
                    "a #2156 follow-up: the component table and the record list disagree",
                )],
            });
            continue;
        };
        let Some(declared_media) = media.iter().find(|media| media.record == name) else {
            outcomes.push(OriginalRegistrationOutcome::Blocked {
                record: row.record.clone(),
                blockers: vec![OriginalRegistrationBlocker::new(
                    name,
                    "media",
                    T452_MEASURED_MEDIA_CLAIM,
                    "the record's media declarations, which could not be read",
                    "a #2156 follow-up: the media reader found no record by this name",
                )],
            });
            continue;
        };
        let Some(declared_ammo) = ammo.iter().find(|ammo| ammo.record == name) else {
            outcomes.push(OriginalRegistrationOutcome::Blocked {
                record: row.record.clone(),
                blockers: vec![OriginalRegistrationBlocker::new(
                    name,
                    "ammo",
                    T452_MEASURED_STACK_CLAIM,
                    "the record's ammunition declaration, which could not be read",
                    "a #2156 follow-up: the ammunition reader found no record by this name",
                )],
            });
            continue;
        };

        let behavior = cs_content::ordnance::measure_component_behavior(row);
        let mut kept: Vec<String> = behavior
            .claims
            .iter()
            .map(|claim| claim.claim.clone())
            .collect();
        for effect in &behavior.effects {
            if let Some(claim) = effect.claim
                && !kept.iter().any(|existing| existing == claim)
            {
                kept.push(claim.to_owned());
            }
        }
        kept.push(T452_PER_BOX_WEIGHT_CLAIM.to_owned());

        match measured_ordnance_declaration(
            row,
            record,
            declared_media,
            declared_ammo,
            provenance,
            ticks_per_second,
        ) {
            Ok(declared) => match lower_ordnance(&declared) {
                Ok(component) => {
                    let family = component.family();
                    let ordnance = component.ordnance().clone();
                    match registry.register(component) {
                        Ok(()) => {
                            // The field is measured with the component, so a
                            // registered `TANGLER` record reports the field the
                            // fire path will open along with the id, and the
                            // two parts the measurement does not pin stay named
                            // on it.
                            if let Some(field) = measured_tangler_field(row) {
                                for claim in
                                    [field.status.claim, T452_REG_TANGLER_FIELD_ORIGIN_CLAIM]
                                {
                                    if !kept.iter().any(|existing| existing == claim) {
                                        kept.push(claim.to_owned());
                                    }
                                }
                                tangler_fields.push(field);
                            }
                            outcomes.push(OriginalRegistrationOutcome::Registered {
                                record: row.record.clone(),
                                ordnance,
                                family,
                            });
                        }
                        Err(error) => outcomes.push(OriginalRegistrationOutcome::Blocked {
                            record: row.record.clone(),
                            blockers: vec![OriginalRegistrationBlocker::new(
                                name,
                                "registry",
                                T452_MEASURED_BEHAVIOUR_CLAIM,
                                &error.to_string(),
                                "a #2156 follow-up: the registry refused the measured id",
                            )],
                        }),
                    }
                }
                Err(error) => outcomes.push(OriginalRegistrationOutcome::Blocked {
                    record: row.record.clone(),
                    blockers: vec![OriginalRegistrationBlocker::new(
                        name,
                        "lowering",
                        T452_MEASURED_BEHAVIOUR_CLAIM,
                        &error.to_string(),
                        "a #2156 follow-up: the declared record did not lower",
                    )],
                }),
            },
            Err(blockers) => outcomes.push(OriginalRegistrationOutcome::Blocked {
                record: row.record.clone(),
                blockers,
            }),
        }
        claims_kept.push((row.record.clone(), kept));
    }

    OriginalRegistrationReport {
        declared: table.rows.len(),
        outcomes,
        claims_kept,
        tangler_fields,
    }
}

/// Reads the installation's measured catalogue and registers every component
/// its measurements fully describe.
///
/// # Errors
///
/// [`OriginalRegistrationError`] when a production reader fails.
pub fn read_and_register_measured_ordnance(
    install_root: &std::path::Path,
    registry: &mut OrdnanceRegistry,
    ticks_per_second: u32,
) -> Result<OriginalRegistrationReport, OriginalRegistrationError> {
    let table =
        cs_content::ordnance::read_original_component_table(install_root).map_err(|error| {
            OriginalRegistrationError::Table {
                detail: error.to_string(),
            }
        })?;
    let records =
        cs_content::ordnance::read_original_weapon_records(install_root).map_err(|error| {
            OriginalRegistrationError::Records {
                detail: error.to_string(),
            }
        })?;
    let media =
        cs_content::ordnance::read_original_weapon_media(install_root).map_err(|error| {
            OriginalRegistrationError::Media {
                detail: error.to_string(),
            }
        })?;
    let ammo =
        cs_content::ordnance::read_original_ammo_declarations(install_root).map_err(|error| {
            OriginalRegistrationError::Ammo {
                detail: error.to_string(),
            }
        })?;
    let provenance = cs_content::ordnance::read_original_weapons_provenance(
        install_root,
        T452_ORIGINAL_WEAPONS_CLAIM,
    )
    .map_err(|error| OriginalRegistrationError::Provenance {
        detail: error.to_string(),
    })?;
    Ok(register_measured_ordnance(
        &table,
        &records,
        &media,
        &ammo,
        &provenance,
        registry,
        ticks_per_second,
    ))
}

/// What a claim this stage keeps names, and the task that resolves it.
///
/// A registered component can still carry a measured value the runtime has no
/// field for. This table is the one place that says, per claim id, what content
/// the gap affects and which task removes it, so a reader of the registration
/// report never has to guess. `None` for a claim this stage does not know.
#[must_use]
pub fn original_kept_claim_context(claim: &str) -> Option<(&'static str, &'static str)> {
    let context = match claim {
        cs_content::ordnance::T452_R02_STATUS_INCAPACITATION => (
            "the pilot incapacitation this record's SONIC or FLASH effect applies to the \
             actor it strikes",
            cs_content::ordnance::T452_R05_STATUS_KIND_FOLLOW_UP,
        ),
        cs_content::ordnance::T452_REG_TANGLER_FORCE_SCALAR => (
            "what the force scalar [ebp-8] that a tangle zeroes is in physical terms \
             (the measured effect is \"the scalar is zero while tangled\", not \"thrust \
             is zero\")",
            "T452-REG-TANGLER-EFFECT",
        ),
        T452_REG_TANGLER_FIELD_ORIGIN_CLAIM => (
            "which world position the original's fire path hands the field constructor \
             0x4b94e0 (the field's own +0x08 slot is measured, not the caller's source)",
            "T452-REG-TANGLER-RUNTIME",
        ),
        T452_PER_BOX_WEIGHT_CLAIM => (
            "the flat per-enabled-box weight and cost this component's purchase charges, \
             which the runtime's per-unit mass has no field for",
            T452_PER_BOX_WEIGHT_TASK,
        ),
        _ => return None,
    };
    Some(context)
}
