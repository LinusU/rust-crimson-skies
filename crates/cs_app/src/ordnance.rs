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

use crate::scene::SceneGeneration;

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
