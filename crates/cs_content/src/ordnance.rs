//! The declared ordnance schema: provenance-carrying hardpoint component,
//! fuse, guidance, area-effect and nitro records (F28-A).
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stage `### F28-A`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! This module is the **content half** of the ordnance contract — the
//! normalized record an importer produces and the catalogue consumes. Its
//! runtime counterpart is `cs_sim::weapons::ordnance` (the definitions a
//! session resolves against and the ledgers it keeps); the conversion
//! boundary between them is `cs_app::ordnance`. The split mirrors
//! `weapons` ↔ `cs_sim::weapons`: this crate cannot depend on `cs_sim`, so
//! the declared record keeps its own typed vocabulary — families, hardpoint
//! kinds, arming and fuse rules, guidance and lost-target rules, status
//! kinds, nitro activation rules — and the boundary maps it field-wise.
//!
//! # Records
//!
//! A [`DeclaredOrdnance`] names its `ordnance` — the `weapon` catalog id it
//! describes — carries an [`Origin`] and a [`Provenance`], and holds one
//! [`DeclaredOrdnanceDetails`]: either a [`DeclaredProjectile`] with a fuse,
//! a lifetime and a guidance rule, or a [`DeclaredNitro`] booster. Keeping
//! them apart is the same split the runtime [`OrdnanceComponent`]
//! counterpart makes, and for the same reason: a booster has no fuse, no
//! lifetime, no blast and no launch geometry, so a record that carried both
//! would invent a detonation the original does not have.
//!
//! Every load-bearing value is a [`Resolved`]: an unmeasured trigger
//! radius, arming delay, launch speed, blast radius, damage amount, nitro
//! capacity or tradeoff is *either* known with [`Provenance`] *or* an
//! explicit unknown with its claim id and reason — never a silent default
//! (F14 non-negotiable behavior 3). The lowering boundary refuses an
//! unknown rather than inventing a component, because a session must not fly
//! a rocket whose fuse geometry was guessed.
//!
//! `scene_binding` ties the launcher to its visual [`SceneNodeId`] in the
//! live aircraft hierarchy. The scene binding is for the presentation and
//! F28-B's transform consumer; ordnance decisions never read it.
//!
//! # Designed vocabulary, not original data
//!
//! The original PC ordnance catalogue, its families, trigger radii, arming
//! delays, lifetimes, blast radii, damage numbers, status effects, nitro
//! capacity, consumption, recovery, thrust, duration and tradeoffs are
//! **all unmeasured** — F28's "Research boundary" says the public manual
//! establishes no ordnance table, and this stage had no `CS_GAME_DIR` at
//! all. Every family name, rule name and fixture value here is **newly
//! authored project design** carrying `Origin::SyntheticFixture` or designed
//! provenance, recorded in
//! `docs/findings/2026-10-01-f28-a-ordnance-behavior-and-effect-registry.md`.

use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use crate::damage::DamageNodeKey;
use crate::scene::SceneNodeId;

/// Where a launcher sits on the airframe — the declared counterpart of
/// `cs_sim::weapons::ordnance::HardpointKind`.
///
/// The list is **designed**: the original hardpoint layout is unmeasured and
/// F28-D maps it onto this set. The boundary lowers it variant-wise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredHardpointKind {
    /// A nose or forward-fuselage launcher.
    Nose,
    /// A launcher on the airframe's left side.
    WingLeft,
    /// A launcher on the airframe's right side.
    WingRight,
    /// A fuselage, gondola or keel launcher.
    Fuselage,
    /// A pylon under a wing.
    Underslung,
}

impl DeclaredHardpointKind {
    /// Every hardpoint kind, in a stable order.
    pub const ALL: &'static [DeclaredHardpointKind] = &[
        Self::Nose,
        Self::WingLeft,
        Self::WingRight,
        Self::Fuselage,
        Self::Underslung,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Nose => "nose",
            Self::WingLeft => "wing_left",
            Self::WingRight => "wing_right",
            Self::Fuselage => "fuselage",
            Self::Underslung => "underslung",
        }
    }
}

impl fmt::Display for DeclaredHardpointKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How an item inherits the launcher's velocity — the declared counterpart
/// of `cs_sim::weapons::InheritanceRule`.
///
/// The original's rule is unmeasured, so this is a declared [`Resolved`]
/// option the boundary refuses to guess.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclaredInheritanceRule {
    /// The item keeps the launcher's whole world velocity.
    Full,
    /// The item keeps a declared fraction of the launcher's velocity.
    Fraction {
        /// The declared share, which must be finite and within `[0, 1]`.
        share: f64,
    },
    /// The item keeps none of the launcher's velocity.
    None,
}

impl fmt::Display for DeclaredInheritanceRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("full"),
            Self::Fraction { share } => write!(f, "fraction({share})"),
            Self::None => f.write_str("none"),
        }
    }
}

/// The declared launch geometry of one component.
///
/// The numbers are declared; the **pose** is not. A launch pose is a
/// supplied transform read from the live aircraft hierarchy by F28-B, so
/// this record carries no launch offset and no default origin.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredLaunchGeometry {
    /// The weapon-mount damage node this component occupies, so the mount a
    /// destroyed weapon node disables is literally the launcher that stops
    /// firing.
    pub mount: DamageNodeKey,
    /// Where on the airframe that mount sits.
    pub hardpoint: Resolved<DeclaredHardpointKind>,
    /// The speed the item leaves the hardpoint at, in meters per second.
    pub launch_speed_mps: Resolved<f64>,
    /// The declared share of the launcher's velocity the item keeps.
    pub inheritance: Resolved<DeclaredInheritanceRule>,
    /// Ticks between the launch intent and the actual release.
    pub release_delay_ticks: Resolved<u64>,
}

/// The declared stack load of one launcher: capacity per launcher and the
/// mass of one unit.
///
/// Capacity is in units, not a rate: it is what the launcher's stack holds
/// before a reload, which is the number the shop's ammo count and the F44
/// loadout validator read.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredStackLoad {
    /// How many units the launcher's stack holds.
    pub capacity_units: Resolved<u64>,
    /// The mass of one unit, in kilograms.
    pub unit_mass_kg: Resolved<f64>,
}

/// When a declared component's fuse becomes live.
///
/// A proximity fuse that triggers on the launch tick detonates the item in
/// its own launcher's lap, so arming is a separate declared field from the
/// fuse geometry and the boundary refuses a record whose fuse is armed by
/// something it did not declare.
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredArmingRule {
    /// Never arms: the component is declared inert.
    Disarmed,
    /// Arms once the item has lived this many ticks after launch.
    AfterTicks(Resolved<u64>),
    /// Arms once the item has travelled this many meters from release.
    AfterTravelMetres(Resolved<f64>),
}

impl fmt::Display for DeclaredArmingRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disarmed => f.write_str("disarmed"),
            Self::AfterTicks(_) => f.write_str("after_ticks"),
            Self::AfterTravelMetres(_) => f.write_str("after_travel_m"),
        }
    }
}

/// A declared proximity fuse's trigger geometry.
///
/// One number: the effective radius within which a target's swept path
/// counts as a trigger. The original's fuse *shape* — a sphere, a shaped
/// zone, a directional sensor — is unmeasured, so this record does not
/// choose one; only the radius is declared, and only the radius lowers.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredProximityFuse {
    /// The declared effective trigger radius, in meters.
    pub trigger_radius_m: Resolved<f64>,
}

/// How a declared component ends.
///
/// A `Timed` fuse's tick count is a [`Resolved`] value: the original's burn
/// or burst delay is unmeasured, and a default tick count would silently
/// give every area-denial component the same lifetime.
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredFuseRule {
    /// Ends on swept contact.
    Impact,
    /// Ends when a target comes within the declared radius.
    Proximity(DeclaredProximityFuse),
    /// Ends after this many ticks.
    Timed {
        /// The declared tick count.
        ticks: Resolved<u64>,
    },
}

impl fmt::Display for DeclaredFuseRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Impact => f.write_str("impact"),
            Self::Proximity(_) => f.write_str("proximity"),
            Self::Timed { .. } => f.write_str("timed"),
        }
    }
}

/// What a declared guided component does when it can no longer follow its
/// target.
///
/// F28 non-negotiable 4 requires the lost-target behavior to be *specified*,
/// so a declared targeted rule always names one. An unmeasured behavior is a
/// [`Resolved`] unknown and refuses to lower — there is no "keep tracking"
/// default, and there is no silent coast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredLostTargetBehavior {
    /// Detonate where the item is.
    Detonate,
    /// Stop tracking and fly the last velocity until the item expires.
    Coast,
    /// Stop being live: no detonation, no damage, no effect.
    Disarm,
}

impl fmt::Display for DeclaredLostTargetBehavior {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Detonate => "detonate",
            Self::Coast => "coast",
            Self::Disarm => "disarm",
        })
    }
}

/// How a declared component finds its target — or the explicit fact that it
/// does not.
///
/// `Unguided` is a declared, first-class state, so a direct explosive or a
/// flak shell is not a seeker with its guidance quietly defaulted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredGuidanceRule {
    /// Flies the declared launch line; it has no target to lose.
    Unguided,
    /// Tracks one designated target, and on loss does exactly what
    /// `lost_target` names.
    Targeted {
        /// The declared lost-target behavior.
        lost_target: Resolved<DeclaredLostTargetBehavior>,
    },
}

impl fmt::Display for DeclaredGuidanceRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unguided => f.write_str("unguided"),
            Self::Targeted { .. } => f.write_str("targeted"),
        }
    }
}

/// A declared bounded area one component leaves behind.
///
/// Both the radius and the lifetime are [`Resolved`]: the original's blast
/// or denial area and its duration are unmeasured, and either one defaulted
/// would change the gameplay outcome.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredAreaEffect {
    /// The declared area radius, in meters.
    pub radius_m: Resolved<f64>,
    /// The declared area lifetime, in whole ticks.
    pub lifetime_ticks: Resolved<u64>,
}

/// What a declared status effect does to its recipient.
///
/// The gameplay kinds, never a combined "effect level", and always separate
/// from the visual particle that may accompany them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredStatusEffectKind {
    /// Damage over time.
    Damage,
    /// Loss of engine output.
    Choke,
    /// Loss of the ability to hold flight.
    Stall,
    /// A marker for other systems and the pilot.
    Marker,
}

impl DeclaredStatusEffectKind {
    /// Every status effect kind, in a stable order.
    pub const ALL: &'static [DeclaredStatusEffectKind] =
        &[Self::Damage, Self::Choke, Self::Stall, Self::Marker];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Damage => "damage",
            Self::Choke => "choke",
            Self::Stall => "stall",
            Self::Marker => "marker",
        }
    }
}

impl fmt::Display for DeclaredStatusEffectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One declared timed status effect.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredStatusEffect {
    /// What it does.
    pub kind: DeclaredStatusEffectKind,
    /// How long it lasts, in whole ticks.
    pub duration_ticks: Resolved<u64>,
    /// How strong it is, in the kind's own unmeasured unit.
    pub strength: Resolved<f64>,
}

/// The declared media a component plays.
///
/// `particles` is the presentation-only visual of an area effect. F28
/// non-negotiable 3 requires it to be separate from the damage and status
/// effects, so it lives in the media record and reaches no gameplay field:
/// dropping the particle costs a picture, never a detonation.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredOrdnanceMedia {
    /// The effect resource a launch or detonation plays.
    pub visual: Resolved<ContentId>,
    /// The sound resource a launch or detonation plays.
    pub sound: Resolved<ContentId>,
    /// The presentation-only particle resource, when the record declares
    /// one.
    pub particles: Option<Resolved<ContentId>>,
}

/// The declared equipment rules an installation of a component obeys.
///
/// F28 non-negotiable 5 makes hardpoint equipment compatibility *shared with
/// loadout validation*, so this record is the ordnance side of that shared
/// rule and the boundary lowers it for both the shop and an import to read.
///
/// The prohibitions are a `Vec` in declared order rather than a set,
/// because a `Resolved` value is not `Ord`: a prohibition's order is
/// whatever the importer read, and the lowering boundary reports the first
/// unresolved one in that order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeclaredEquipmentRules {
    /// The equipment the airframe must already carry.
    pub requires: Option<Resolved<ContentId>>,
    /// The equipment the airframe must not carry, in declared order.
    pub forbids: Vec<Resolved<ContentId>>,
}

/// The behavior family one declared component belongs to.
///
/// **Designed**, from the sheet's list of families to discover. The six
/// names are leads, not a catalogue: a family the original turns out not to
/// have simply stays unused, and one it has that is not here is added when
/// F28-D's audit finds it. The runtime refuses a family whose declared
/// guidance or fuse contradicts it, which is how non-negotiable 1 — "do
/// not substitute every rocket with one homing missile" — becomes
/// executable rather than a comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredOrdnanceFamily {
    /// A shot that flies a declared line and bursts on contact.
    DirectExplosive,
    /// A shell that bursts when a target comes close enough, unguided.
    ProximityFlak,
    /// A seeker that tracks one designated target.
    GuidedRocket,
    /// An item whose effect is a bounded area rather than a hit.
    AreaDenialEngine,
    /// A long-lived aerodynamic weapon dropped or launched at a target.
    AerialTorpedo,
    /// A continuous booster: capacity, consumption, extra thrust, no fuse.
    NitroBooster,
}

impl DeclaredOrdnanceFamily {
    /// Every family, in a stable order.
    pub const ALL: &'static [DeclaredOrdnanceFamily] = &[
        Self::DirectExplosive,
        Self::ProximityFlak,
        Self::GuidedRocket,
        Self::AreaDenialEngine,
        Self::AerialTorpedo,
        Self::NitroBooster,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::DirectExplosive => "direct_explosive",
            Self::ProximityFlak => "proximity_flak",
            Self::GuidedRocket => "guided_rocket",
            Self::AreaDenialEngine => "area_denial_engine",
            Self::AerialTorpedo => "aerial_torpedo",
            Self::NitroBooster => "nitro_booster",
        }
    }

    /// Whether this family is a launched item rather than a booster.
    #[must_use]
    pub const fn is_projectile(self) -> bool {
        !matches!(self, Self::NitroBooster)
    }
}

impl fmt::Display for DeclaredOrdnanceFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A declared launched component: a rocket, a shell, a torpedo, a denial
/// canister.
///
/// Every load-bearing value is a [`Resolved`] so an unmeasured parameter is
/// recorded rather than defaulted.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredProjectile {
    /// The declared launch geometry.
    pub launch: DeclaredLaunchGeometry,
    /// The declared stack capacity and unit mass.
    pub stack: DeclaredStackLoad,
    /// When the fuse becomes live.
    pub arming: DeclaredArmingRule,
    /// How the item ends.
    pub fuse: DeclaredFuseRule,
    /// How the item finds — or does not find — a target.
    pub guidance: DeclaredGuidanceRule,
    /// How many whole ticks the item may live after release.
    pub lifetime_ticks: Resolved<u64>,
    /// The bounded area left behind, when the record declares one.
    pub area_effect: Option<DeclaredAreaEffect>,
    /// The immediate damage amount per channel.
    pub armor_damage: Resolved<f64>,
    /// The immediate internal damage amount.
    pub internal_damage: Resolved<f64>,
    /// The timed status effects applied to recipients.
    pub status: Vec<DeclaredStatusEffect>,
    /// The resources played.
    pub media: DeclaredOrdnanceMedia,
    /// The equipment rules an installation obeys.
    pub equipment_rules: DeclaredEquipmentRules,
}

/// How a declared nitro activation begins and how long it may run.
///
/// Nitro is **not** an Xbox-only feature (non-negotiable 2): the PC manual
/// lists a nitro activation control, so the activation rule is part of the
/// declared record. Which rule the original used is unmeasured, so it is a
/// [`Resolved`] option and an unresolved one refuses to lower rather than
/// becoming "while held".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredNitroActivationRule {
    /// Active on every tick the control is held and capacity remains.
    WhileHeld,
    /// Each accepted activation runs for this many whole ticks.
    FixedTicks {
        /// The declared burn length.
        ticks: Resolved<u64>,
    },
}

impl fmt::Display for DeclaredNitroActivationRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WhileHeld => f.write_str("while_held"),
            Self::FixedTicks { .. } => f.write_str("fixed_ticks"),
        }
    }
}

/// The declared numbers one nitro booster carries.
///
/// Every value is [`Resolved`]. In particular the tradeoff is: the original's
/// nitro tradeoffs are unknown, so an unmeasured tradeoff refuses to lower
/// and the record can never substitute an invented authority penalty.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredNitroParameters {
    /// The declared capacity, in capacity units.
    pub capacity_units: Resolved<f64>,
    /// Capacity consumed per second of running nitro.
    pub consumption_per_s: Resolved<f64>,
    /// Capacity recovered per second while idle.
    pub recovery_per_s: Resolved<f64>,
    /// The extra thrust nitro adds, in newtons.
    pub extra_thrust_n: Resolved<f64>,
    /// The declared activation rule.
    pub activation: Resolved<DeclaredNitroActivationRule>,
    /// The declared fraction of normal control authority while running, as
    /// a [`Resolved<f64>`].
    pub authority_multiplier: Resolved<f64>,
}

/// A declared nitro booster.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredNitro {
    /// The declared capacity, consumption, recovery, thrust, activation rule
    /// and authority tradeoff.
    pub parameters: DeclaredNitroParameters,
    /// The resources played.
    pub media: DeclaredOrdnanceMedia,
    /// The equipment rules an installation obeys.
    pub equipment_rules: DeclaredEquipmentRules,
}

/// One declared component's details: a launched item or a booster.
///
/// The split is the same one the runtime makes, for the same reason: a
/// booster has no fuse, no lifetime, no blast and no launch geometry.
///
/// Both arms are boxed. Either record carries several `Resolved` values, and
/// either one inline would make every declared record as large as the larger
/// of the two — which for a catalogue that will hold every component the
/// original has is the wrong trade.
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredOrdnanceDetails {
    /// A launched item with a fuse, a lifetime and a guidance rule.
    Projectile(Box<DeclaredProjectile>),
    /// A nitro booster: capacity, consumption, extra thrust, no fuse.
    Nitro(Box<DeclaredNitro>),
}

/// The declared definition of one hardpoint ordnance component.
///
/// `ordnance` is the `weapon` catalog id the record describes. The `family`
/// names the behavior class and must agree with `details`: a
/// [`DeclaredOrdnanceFamily::NitroBooster`] record carries
/// [`DeclaredOrdnanceDetails::Nitro`] and a projectile family carries
/// [`DeclaredOrdnanceDetails::Projectile`], which is the first of the
/// family-coherence refusals F28 non-negotiable 1 needs.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredOrdnance {
    ordnance: ContentId,
    origin: Origin,
    family: DeclaredOrdnanceFamily,
    details: DeclaredOrdnanceDetails,
    scene_binding: Option<Resolved<SceneNodeId>>,
    provenance: Provenance,
}

impl DeclaredOrdnance {
    /// Assembles and validates a declared component record.
    ///
    /// Validation covers identity, the family/`details` agreement, and the
    /// sanity of every **known** value. It never decides an unknown: an
    /// unresolved field is a legal record, and the lowering boundary is what
    /// refuses it.
    ///
    /// # Errors
    ///
    /// [`OrdnanceSchemaError`] on a wrong-namespace id, a family that
    /// disagrees with its details, or a corrupt known value.
    pub fn try_new(
        ordnance: ContentId,
        origin: Origin,
        family: DeclaredOrdnanceFamily,
        details: DeclaredOrdnanceDetails,
        scene_binding: Option<Resolved<SceneNodeId>>,
        provenance: Provenance,
    ) -> Result<Self, OrdnanceSchemaError> {
        if ordnance.kind() != ContentKind::Weapon {
            return Err(OrdnanceSchemaError::OrdnanceKindMismatch { id: ordnance });
        }
        match (&family, &details) {
            (DeclaredOrdnanceFamily::NitroBooster, DeclaredOrdnanceDetails::Nitro(_)) => {}
            (family, DeclaredOrdnanceDetails::Projectile(projectile)) if family.is_projectile() => {
                validate_projectile(projectile)?;
            }
            (family, _) => {
                return Err(OrdnanceSchemaError::FamilyDetailsMismatch { family: *family });
            }
        }
        // Both halves validate their own known values, including the media
        // namespaces. An `Unknown` value is legal here and is the lowering
        // boundary's business: repairing one into a plausible number is
        // exactly what F14 non-negotiable behavior 3 forbids.
        match &details {
            DeclaredOrdnanceDetails::Projectile(projectile) => {
                let DeclaredOrdnanceMedia {
                    visual,
                    sound,
                    particles,
                } = &projectile.media;
                validate_media(visual, sound, particles.as_ref())?;
            }
            DeclaredOrdnanceDetails::Nitro(nitro) => {
                let DeclaredOrdnanceMedia {
                    visual,
                    sound,
                    particles,
                } = &nitro.media;
                validate_media(visual, sound, particles.as_ref())?;
                validate_nitro(&nitro.parameters)?;
            }
        }
        Ok(Self {
            ordnance,
            origin,
            family,
            details,
            scene_binding,
            provenance,
        })
    }

    /// The `weapon` catalog id this record describes.
    #[must_use]
    pub const fn ordnance(&self) -> &ContentId {
        &self.ordnance
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared behavior family.
    #[must_use]
    pub const fn family(&self) -> DeclaredOrdnanceFamily {
        self.family
    }

    /// The declared details.
    #[must_use]
    pub const fn details(&self) -> &DeclaredOrdnanceDetails {
        &self.details
    }

    /// The declared projectile half, when this is a launched item.
    #[must_use]
    pub fn projectile(&self) -> Option<&DeclaredProjectile> {
        match &self.details {
            DeclaredOrdnanceDetails::Projectile(projectile) => Some(projectile),
            DeclaredOrdnanceDetails::Nitro(_) => None,
        }
    }

    /// The declared booster half, when this is a nitro booster.
    #[must_use]
    pub fn nitro(&self) -> Option<&DeclaredNitro> {
        match &self.details {
            DeclaredOrdnanceDetails::Nitro(nitro) => Some(nitro),
            DeclaredOrdnanceDetails::Projectile(_) => None,
        }
    }

    /// The declared media, whichever half carries them.
    #[must_use]
    pub const fn media(&self) -> &DeclaredOrdnanceMedia {
        match &self.details {
            DeclaredOrdnanceDetails::Projectile(projectile) => &projectile.media,
            DeclaredOrdnanceDetails::Nitro(nitro) => &nitro.media,
        }
    }

    /// The declared equipment rules, whichever half carries them.
    #[must_use]
    pub const fn equipment_rules(&self) -> &DeclaredEquipmentRules {
        match &self.details {
            DeclaredOrdnanceDetails::Projectile(projectile) => &projectile.equipment_rules,
            DeclaredOrdnanceDetails::Nitro(nitro) => &nitro.equipment_rules,
        }
    }

    /// The visual launcher binding into the live aircraft hierarchy, when the
    /// record declares one. Presentation and F28-B transforms read it;
    /// ordnance decisions never do.
    #[must_use]
    pub const fn scene_binding(&self) -> Option<&Resolved<SceneNodeId>> {
        self.scene_binding.as_ref()
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why a declared ordnance record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceSchemaError {
    /// The ordnance id is not in the `weapon` namespace.
    OrdnanceKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The declared family and the declared details disagree: a booster
    /// family with projectile details, or a projectile family with booster
    /// details.
    FamilyDetailsMismatch {
        /// The family that was declared.
        family: DeclaredOrdnanceFamily,
    },
    /// A known launch speed was NaN or infinite.
    NonFiniteLaunchSpeed,
    /// A known launch speed was zero or negative.
    NonPositiveLaunchSpeed {
        /// The rejected value.
        launch_speed_mps: f64,
    },
    /// A known inheritance share was corrupt.
    InvalidInheritanceShare {
        /// The rejected share.
        share: f64,
    },
    /// A known stack capacity was zero, so the launcher can never be loaded.
    ZeroStackCapacity,
    /// A known unit mass was NaN or infinite.
    NonFiniteUnitMass,
    /// A known unit mass was negative.
    NegativeUnitMass {
        /// The rejected value.
        unit_mass_kg: f64,
    },
    /// A known trigger radius was NaN or infinite.
    NonFiniteTriggerRadius,
    /// A known trigger radius was zero or negative.
    NonPositiveTriggerRadius {
        /// The rejected value.
        trigger_radius_m: f64,
    },
    /// A known area radius was NaN or infinite.
    NonFiniteAreaRadius,
    /// A known area radius was zero or negative.
    NonPositiveAreaRadius {
        /// The rejected value.
        radius_m: f64,
    },
    /// A known area lifetime was zero, so the area is not bounded.
    ZeroAreaLifetime,
    /// A known status duration was zero, so the effect is not timed.
    ZeroStatusDuration,
    /// A known status strength was NaN or infinite.
    NonFiniteStatusStrength,
    /// A known status strength was negative.
    NegativeStatusStrength {
        /// The rejected value.
        strength: f64,
    },
    /// A known lifetime was zero, so the item exists for no ticks.
    ZeroLifetime,
    /// A known fuse tick count was zero.
    ZeroFuseTicks,
    /// A known media id named the wrong catalog namespace.
    MediaKindMismatch {
        /// The offending id.
        id: ContentId,
        /// The namespace it needed.
        expected: ContentKind,
    },
    /// A known nitro capacity, consumption, recovery or thrust was corrupt.
    CorruptNitroValue {
        /// Which declared field was corrupt.
        field: &'static str,
        /// Why it was refused.
        reason: &'static str,
    },
    /// A known authority multiplier was outside `(0, 1]`.
    AuthorityMultiplierOutOfRange {
        /// The rejected value.
        authority_multiplier: f64,
    },
    /// A known fixed nitro burn was zero ticks, which is not an activation.
    ZeroNitroBurn,
}

impl fmt::Display for OrdnanceSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OrdnanceKindMismatch { id } => {
                write!(f, "ordnance id {id} is not in the weapon namespace")
            }
            Self::FamilyDetailsMismatch { family } => write!(
                f,
                "{family} does not agree with the declared details for this record"
            ),
            Self::NonFiniteLaunchSpeed => write!(f, "the launch speed must be finite"),
            Self::NonPositiveLaunchSpeed { launch_speed_mps } => {
                write!(f, "the launch speed must be positive: {launch_speed_mps}")
            }
            Self::InvalidInheritanceShare { share } => {
                write!(
                    f,
                    "the inherited velocity share must be within [0, 1]: {share}"
                )
            }
            Self::ZeroStackCapacity => {
                write!(
                    f,
                    "a launcher must declare a stack capacity of at least one unit"
                )
            }
            Self::NonFiniteUnitMass => write!(f, "the unit mass must be finite"),
            Self::NegativeUnitMass { unit_mass_kg } => {
                write!(f, "the unit mass is negative: {unit_mass_kg}")
            }
            Self::NonFiniteTriggerRadius => {
                write!(f, "the proximity trigger radius must be finite")
            }
            Self::NonPositiveTriggerRadius { trigger_radius_m } => {
                write!(
                    f,
                    "the proximity trigger radius must be positive: {trigger_radius_m}"
                )
            }
            Self::NonFiniteAreaRadius => write!(f, "the area radius must be finite"),
            Self::NonPositiveAreaRadius { radius_m } => {
                write!(f, "the area radius must be positive: {radius_m}")
            }
            Self::ZeroAreaLifetime => write!(f, "an area effect must declare a bounded lifetime"),
            Self::ZeroStatusDuration => {
                write!(f, "a status effect must last at least one tick")
            }
            Self::NonFiniteStatusStrength => write!(f, "a status strength must be finite"),
            Self::NegativeStatusStrength { strength } => {
                write!(f, "a status strength is negative: {strength}")
            }
            Self::ZeroLifetime => write!(f, "ordnance must live for at least one tick"),
            Self::ZeroFuseTicks => write!(f, "a timed fuse must fire after at least one tick"),
            Self::MediaKindMismatch { id, expected } => write!(
                f,
                "media resource {id} must be in the {} namespace",
                expected.label()
            ),
            Self::CorruptNitroValue { field, reason } => {
                write!(f, "the declared nitro {field} is unusable: {reason}")
            }
            Self::AuthorityMultiplierOutOfRange {
                authority_multiplier,
            } => write!(
                f,
                "the nitro authority multiplier must be within (0, 1]: {authority_multiplier}"
            ),
            Self::ZeroNitroBurn => write!(f, "a fixed nitro burn must last at least one tick"),
        }
    }
}

impl std::error::Error for OrdnanceSchemaError {}

/// The known-value sanity checks on a declared projectile.
///
/// Every check is on a **known** value only: an `Unknown` is a legal
/// declared record and is the lowering boundary's business, not this
/// function's. Repairing an unknown into a plausible number here is exactly
/// the failure F14 non-negotiable behavior 3 forbids.
fn validate_projectile(projectile: &DeclaredProjectile) -> Result<(), OrdnanceSchemaError> {
    if let Resolved::Known(known) = &projectile.launch.launch_speed_mps {
        if !known.value.is_finite() {
            return Err(OrdnanceSchemaError::NonFiniteLaunchSpeed);
        }
        if known.value <= 0.0 {
            return Err(OrdnanceSchemaError::NonPositiveLaunchSpeed {
                launch_speed_mps: known.value,
            });
        }
    }
    if let Resolved::Known(known) = &projectile.launch.inheritance
        && let DeclaredInheritanceRule::Fraction { share } = known.value
        && (!share.is_finite() || !(0.0..=1.0).contains(&share))
    {
        return Err(OrdnanceSchemaError::InvalidInheritanceShare { share });
    }
    if let Resolved::Known(known) = &projectile.stack.capacity_units
        && known.value == 0
    {
        return Err(OrdnanceSchemaError::ZeroStackCapacity);
    }
    if let Resolved::Known(known) = &projectile.stack.unit_mass_kg {
        if !known.value.is_finite() {
            return Err(OrdnanceSchemaError::NonFiniteUnitMass);
        }
        if known.value < 0.0 {
            return Err(OrdnanceSchemaError::NegativeUnitMass {
                unit_mass_kg: known.value,
            });
        }
    }
    match &projectile.fuse {
        DeclaredFuseRule::Proximity(fuse) => {
            if let Resolved::Known(known) = &fuse.trigger_radius_m {
                if !known.value.is_finite() {
                    return Err(OrdnanceSchemaError::NonFiniteTriggerRadius);
                }
                if known.value <= 0.0 {
                    return Err(OrdnanceSchemaError::NonPositiveTriggerRadius {
                        trigger_radius_m: known.value,
                    });
                }
            }
        }
        DeclaredFuseRule::Timed {
            ticks: Resolved::Known(known),
        } => {
            if known.value == 0 {
                return Err(OrdnanceSchemaError::ZeroFuseTicks);
            }
        }
        DeclaredFuseRule::Impact | DeclaredFuseRule::Timed { .. } => {}
    }
    if let Some(area) = &projectile.area_effect {
        if let Resolved::Known(known) = &area.radius_m {
            if !known.value.is_finite() {
                return Err(OrdnanceSchemaError::NonFiniteAreaRadius);
            }
            if known.value <= 0.0 {
                return Err(OrdnanceSchemaError::NonPositiveAreaRadius {
                    radius_m: known.value,
                });
            }
        }
        if let Resolved::Known(known) = &area.lifetime_ticks
            && known.value == 0
        {
            return Err(OrdnanceSchemaError::ZeroAreaLifetime);
        }
    }
    for effect in &projectile.status {
        if let Resolved::Known(known) = &effect.duration_ticks
            && known.value == 0
        {
            return Err(OrdnanceSchemaError::ZeroStatusDuration);
        }
        if let Resolved::Known(known) = &effect.strength {
            if !known.value.is_finite() {
                return Err(OrdnanceSchemaError::NonFiniteStatusStrength);
            }
            if known.value < 0.0 {
                return Err(OrdnanceSchemaError::NegativeStatusStrength {
                    strength: known.value,
                });
            }
        }
    }
    if let Resolved::Known(known) = &projectile.lifetime_ticks
        && known.value == 0
    {
        return Err(OrdnanceSchemaError::ZeroLifetime);
    }
    for amount in [&projectile.armor_damage, &projectile.internal_damage] {
        if let Resolved::Known(known) = amount {
            if !known.value.is_finite() {
                return Err(OrdnanceSchemaError::NonFiniteStatusStrength);
            }
            if known.value < 0.0 {
                return Err(OrdnanceSchemaError::NegativeStatusStrength {
                    strength: known.value,
                });
            }
        }
    }
    Ok(())
}

/// The known-value sanity checks on a declared nitro booster.
///
/// Every one of the five numbers is checked, because a booster with a
/// negative thrust or a zero consumption is not a weaker nitro — it is a
/// different component, and letting it through would be a silent
/// substitution rather than a refusal.
fn validate_nitro(parameters: &DeclaredNitroParameters) -> Result<(), OrdnanceSchemaError> {
    // Capacity, consumption and thrust must each be strictly positive: a
    // booster with none of them is a different component, not a weaker one.
    // Recovery may be zero — a booster that never recharges is a legitimate
    // design — but it may not be negative.
    for (resolved, must_be_positive) in [
        (&parameters.capacity_units, true),
        (&parameters.consumption_per_s, true),
        (&parameters.recovery_per_s, false),
        (&parameters.extra_thrust_n, true),
    ] {
        let Resolved::Known(known) = resolved else {
            continue;
        };
        if !known.value.is_finite() {
            return Err(OrdnanceSchemaError::CorruptNitroValue {
                field: "nitro",
                reason: "capacity, consumption, recovery and thrust must all be finite",
            });
        }
        let unusable = if must_be_positive {
            known.value <= 0.0
        } else {
            known.value < 0.0
        };
        if unusable {
            return Err(OrdnanceSchemaError::CorruptNitroValue {
                field: "nitro",
                reason: if must_be_positive {
                    "capacity, consumption and thrust must each be above zero"
                } else {
                    "recovery cannot be negative"
                },
            });
        }
    }
    if let Resolved::Known(known) = &parameters.authority_multiplier {
        if !known.value.is_finite() {
            return Err(OrdnanceSchemaError::CorruptNitroValue {
                field: "authority_multiplier",
                reason: "it must be finite",
            });
        }
        if known.value <= 0.0 || known.value > 1.0 {
            return Err(OrdnanceSchemaError::AuthorityMultiplierOutOfRange {
                authority_multiplier: known.value,
            });
        }
    }
    if let Resolved::Known(known) = &parameters.activation
        && let DeclaredNitroActivationRule::FixedTicks { ticks } = &known.value
        && let Resolved::Known(ticks) = ticks
        && ticks.value == 0
    {
        return Err(OrdnanceSchemaError::ZeroNitroBurn);
    }
    Ok(())
}

/// The known-value sanity checks on a declared media record.
fn validate_media(
    visual: &Resolved<ContentId>,
    sound: &Resolved<ContentId>,
    particles: Option<&Resolved<ContentId>>,
) -> Result<(), OrdnanceSchemaError> {
    let check = |resolved: &Resolved<ContentId>, expected: ContentKind| {
        if let Resolved::Known(known) = resolved
            && known.value.kind() != expected
        {
            return Err(OrdnanceSchemaError::MediaKindMismatch {
                id: known.value.clone(),
                expected,
            });
        }
        Ok(())
    };
    check(visual, ContentKind::HardpointEquipment)?;
    check(sound, ContentKind::Sound)?;
    if let Some(particles) = particles {
        check(particles, ContentKind::HardpointEquipment)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- fixture ----

/// Catalog key of the synthetic declared direct-explosive component.
pub const DECLARED_SYNTHETIC_DIRECT_KEY: &str = "synthetic.fixture_direct_explosive";
/// Catalog key of the synthetic declared proximity-flak component.
pub const DECLARED_SYNTHETIC_FLAK_KEY: &str = "synthetic.fixture_proximity_flak";
/// Catalog key of the synthetic declared guided-rocket component.
pub const DECLARED_SYNTHETIC_GUIDED_KEY: &str = "synthetic.fixture_guided_rocket";
/// Catalog key of the synthetic declared area-denial component.
pub const DECLARED_SYNTHETIC_AREA_DENIAL_KEY: &str = "synthetic.fixture_area_denial";
/// Catalog key of the synthetic declared aerial-torpedo component.
pub const DECLARED_SYNTHETIC_TORPEDO_KEY: &str = "synthetic.fixture_aerial_torpedo";
/// Catalog key of the synthetic declared nitro booster.
pub const DECLARED_SYNTHETIC_NITRO_KEY: &str = "synthetic.fixture_nitro";
/// The synthetic declared launcher mount key.
pub const DECLARED_SYNTHETIC_LAUNCHER_MOUNT: &str = "ordnance_mount_1";
/// The synthetic declared effect resource.
pub const DECLARED_SYNTHETIC_EFFECT_KEY: &str = "synthetic.fixture_ordnance_effect";
/// The synthetic declared particle resource.
pub const DECLARED_SYNTHETIC_PARTICLE_KEY: &str = "synthetic.fixture_ordnance_particles";
/// The synthetic declared sound resource.
pub const DECLARED_SYNTHETIC_SOUND_KEY: &str = "synthetic.fixture_ordnance_sound";

/// The claim the synthetic declared fixture's values are recorded under.
#[must_use]
pub fn declared_synthetic_claim() -> ClaimId {
    ClaimId::new("f28.fixture.declared").expect("the fixture claim id is valid")
}

/// Designed provenance for a synthetic declared value.
#[must_use]
pub fn declared_synthetic_provenance() -> Provenance {
    Provenance::designed(declared_synthetic_claim())
}

/// A known synthetic value, carrying designed provenance.
#[must_use]
pub fn declared_known<T>(value: T) -> Resolved<T> {
    Resolved::Known(cs_types::content::Known::new(
        value,
        declared_synthetic_provenance(),
    ))
}

/// An explicit unknown, with the fixture claim and a stated reason.
#[must_use]
pub fn declared_unknown<T>(reason: &str) -> Resolved<T> {
    Resolved::unknown(declared_synthetic_claim(), reason).expect("the fixture reason is non-empty")
}

/// The synthetic declared media record, with a declared particle.
#[must_use]
pub fn declared_synthetic_media() -> DeclaredOrdnanceMedia {
    DeclaredOrdnanceMedia {
        visual: declared_known(
            ContentId::from_source(
                ContentKind::HardpointEquipment,
                DECLARED_SYNTHETIC_EFFECT_KEY,
            )
            .expect("valid id"),
        ),
        sound: declared_known(
            ContentId::from_source(ContentKind::Sound, DECLARED_SYNTHETIC_SOUND_KEY)
                .expect("valid id"),
        ),
        particles: Some(declared_known(
            ContentId::from_source(
                ContentKind::HardpointEquipment,
                DECLARED_SYNTHETIC_PARTICLE_KEY,
            )
            .expect("valid id"),
        )),
    }
}

/// The synthetic declared launch geometry.
#[must_use]
pub fn declared_synthetic_launch() -> DeclaredLaunchGeometry {
    DeclaredLaunchGeometry {
        mount: DamageNodeKey::new(DECLARED_SYNTHETIC_LAUNCHER_MOUNT)
            .expect("the fixture mount key is valid"),
        hardpoint: declared_known(DeclaredHardpointKind::WingLeft),
        launch_speed_mps: declared_known(210.0),
        inheritance: declared_known(DeclaredInheritanceRule::None),
        release_delay_ticks: declared_known(0),
    }
}

/// The synthetic declared stack load.
#[must_use]
pub fn declared_synthetic_stack() -> DeclaredStackLoad {
    DeclaredStackLoad {
        capacity_units: declared_known(6),
        unit_mass_kg: declared_known(2.4),
    }
}

/// A synthetic declared projectile with the given id, family and rules.
///
/// The shared body of the fixture's projectile components: everything is
/// `Known` with **designed** provenance, because the synthetic fixture is
/// not original data and must never be able to claim it is.
#[must_use]
pub fn declared_synthetic_projectile(
    key: &str,
    family: DeclaredOrdnanceFamily,
    arming: DeclaredArmingRule,
    fuse: DeclaredFuseRule,
    guidance: DeclaredGuidanceRule,
    lifetime_ticks: u64,
) -> DeclaredOrdnance {
    let mut projectile = DeclaredProjectile {
        launch: declared_synthetic_launch(),
        stack: declared_synthetic_stack(),
        arming,
        fuse,
        guidance,
        lifetime_ticks: declared_known(lifetime_ticks),
        area_effect: None,
        armor_damage: declared_known(22.0),
        internal_damage: declared_known(9.0),
        status: Vec::new(),
        media: declared_synthetic_media(),
        equipment_rules: DeclaredEquipmentRules::default(),
    };
    if matches!(family, DeclaredOrdnanceFamily::AreaDenialEngine) {
        projectile.area_effect = Some(DeclaredAreaEffect {
            radius_m: declared_known(55.0),
            lifetime_ticks: declared_known(90),
        });
        projectile.status = vec![DeclaredStatusEffect {
            kind: DeclaredStatusEffectKind::Choke,
            duration_ticks: declared_known(60),
            strength: declared_known(0.4),
        }];
    }
    DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Weapon, key).expect("valid id"),
        Origin::SyntheticFixture,
        family,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the synthetic declared projectile is valid")
}

/// The synthetic declared proximity-flak component.
///
/// The AC01 minimum scenario's declared component: a proximity fuse with a
/// declared trigger radius and a declared arming delay.
#[must_use]
pub fn declared_synthetic_flak() -> DeclaredOrdnance {
    declared_synthetic_projectile(
        DECLARED_SYNTHETIC_FLAK_KEY,
        DeclaredOrdnanceFamily::ProximityFlak,
        DeclaredArmingRule::AfterTicks(declared_known(3)),
        DeclaredFuseRule::Proximity(DeclaredProximityFuse {
            trigger_radius_m: declared_known(12.0),
        }),
        DeclaredGuidanceRule::Unguided,
        45,
    )
}

/// The synthetic declared direct-explosive component.
#[must_use]
pub fn declared_synthetic_direct() -> DeclaredOrdnance {
    declared_synthetic_projectile(
        DECLARED_SYNTHETIC_DIRECT_KEY,
        DeclaredOrdnanceFamily::DirectExplosive,
        DeclaredArmingRule::AfterTicks(declared_known(3)),
        DeclaredFuseRule::Impact,
        DeclaredGuidanceRule::Unguided,
        240,
    )
}

/// The synthetic declared guided-rocket component.
#[must_use]
pub fn declared_synthetic_guided() -> DeclaredOrdnance {
    declared_synthetic_projectile(
        DECLARED_SYNTHETIC_GUIDED_KEY,
        DeclaredOrdnanceFamily::GuidedRocket,
        DeclaredArmingRule::AfterTravelMetres(declared_known(60.0)),
        DeclaredFuseRule::Impact,
        DeclaredGuidanceRule::Targeted {
            lost_target: declared_known(DeclaredLostTargetBehavior::Detonate),
        },
        240,
    )
}

/// The synthetic declared area-denial component.
#[must_use]
pub fn declared_synthetic_area_denial() -> DeclaredOrdnance {
    declared_synthetic_projectile(
        DECLARED_SYNTHETIC_AREA_DENIAL_KEY,
        DeclaredOrdnanceFamily::AreaDenialEngine,
        DeclaredArmingRule::AfterTicks(declared_known(3)),
        DeclaredFuseRule::Timed {
            ticks: declared_known(90),
        },
        DeclaredGuidanceRule::Unguided,
        90,
    )
}

/// The synthetic declared aerial-torpedo component.
#[must_use]
pub fn declared_synthetic_torpedo() -> DeclaredOrdnance {
    declared_synthetic_projectile(
        DECLARED_SYNTHETIC_TORPEDO_KEY,
        DeclaredOrdnanceFamily::AerialTorpedo,
        DeclaredArmingRule::AfterTicks(declared_known(3)),
        DeclaredFuseRule::Timed {
            ticks: declared_known(240),
        },
        DeclaredGuidanceRule::Unguided,
        240,
    )
}

/// The synthetic declared nitro booster.
///
/// Its `authority_multiplier` is a **known 1.0 with designed provenance**,
/// which says "no tradeoff has been measured" rather than claiming an
/// observed value.
#[must_use]
pub fn declared_synthetic_nitro() -> DeclaredOrdnance {
    DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_NITRO_KEY)
            .expect("valid id"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::NitroBooster,
        DeclaredOrdnanceDetails::Nitro(Box::new(DeclaredNitro {
            parameters: DeclaredNitroParameters {
                capacity_units: declared_known(12.0),
                consumption_per_s: declared_known(3.0),
                recovery_per_s: declared_known(1.0),
                extra_thrust_n: declared_known(4200.0),
                activation: declared_known(DeclaredNitroActivationRule::WhileHeld),
                authority_multiplier: declared_known(1.0),
            },
            media: declared_synthetic_media(),
            equipment_rules: DeclaredEquipmentRules::default(),
        })),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the synthetic declared nitro is valid")
}
