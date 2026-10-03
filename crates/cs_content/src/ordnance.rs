//! The declared ordnance schema: provenance-carrying hardpoint component,
//! fuse, guidance, area-effect and nitro records (F28-A), plus the measured
//! original ordnance surface and the catalogue audit that compares against it
//! (F28-D).
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stages `### F28-A` and `### F28-D`. Shared contract:
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
//! The original PC ordnance catalogue's *behavior* — which families exist, their
//! trigger radii, arming delays, lifetimes, blast radii, damage numbers,
//! status effects, nitro capacity, consumption, recovery, thrust, duration and
//! tradeoffs — is **all unmeasured**. F28's "Research boundary" says the
//! public manual establishes no ordnance table, and nothing in the
//! installation's files does either. Every family name, rule name and fixture
//! value here is **newly authored project design** carrying
//! `Origin::SyntheticFixture` or designed provenance, recorded in
//! `docs/findings/2026-10-01-f28-a-ordnance-behavior-and-effect-registry.md`.
//!
//! # What the installation does declare (F28-D)
//!
//! The installation's files *do* declare the **shape** of the ordnance
//! surface, and that is what [`OriginalOrdnanceSurface`] measures: eleven
//! rocket ordnance types (from two independent selection screens, not from the
//! string-block gaps), eight rocket slots per airframe, two hardpoint points,
//! and a nitro control. What it does **not** declare is any mapping from a
//! declared record to one of those eleven types, so [`OrdnanceAudit`] reports
//! every component as unattributed and a six-component synthetic catalogue is
//! correctly reported as incomplete. The measurements are re-derived from the
//! owner's installation by
//! `crates/cs_content/tests/accept_f28_d_retail_ordnance_catalogue.rs` on
//! every run, and the details are in
//! `docs/findings/2026-10-03-f28-d-original-ordnance-catalogue.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::damage::DamageNodeKey;
use crate::scene::SceneNodeId;
use crate::weapons::{ORIGINAL_HARDPOINT_POINTS, ORIGINAL_ROCKET_SLOTS};

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
    /// A known damage amount was NaN or infinite.
    NonFiniteDamage {
        /// Which declared damage channel was corrupt.
        field: &'static str,
    },
    /// A known damage amount was negative.
    NegativeDamage {
        /// Which declared damage channel was corrupt.
        field: &'static str,
        /// The rejected value.
        damage: f64,
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
            Self::NonFiniteDamage { field } => {
                write!(f, "the declared {field} amount must be finite")
            }
            Self::NegativeDamage { field, damage } => {
                write!(f, "the declared {field} is negative: {damage}")
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
    for (field, amount) in [
        ("armor_damage", &projectile.armor_damage),
        ("internal_damage", &projectile.internal_damage),
    ] {
        if let Resolved::Known(known) = amount {
            if !known.value.is_finite() {
                return Err(OrdnanceSchemaError::NonFiniteDamage { field });
            }
            if known.value < 0.0 {
                return Err(OrdnanceSchemaError::NegativeDamage {
                    field,
                    damage: known.value,
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
    // design — but it may not be negative. Each entry names its own declared
    // field, so a refusal points at the number that is corrupt rather than at
    // the record as a whole.
    for (field, resolved, must_be_positive) in [
        ("capacity_units", &parameters.capacity_units, true),
        ("consumption_per_s", &parameters.consumption_per_s, true),
        ("recovery_per_s", &parameters.recovery_per_s, false),
        ("extra_thrust_n", &parameters.extra_thrust_n, true),
    ] {
        let Resolved::Known(known) = resolved else {
            continue;
        };
        if !known.value.is_finite() {
            return Err(OrdnanceSchemaError::CorruptNitroValue {
                field,
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
                field,
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

// ------------------------------------------------- measured original surface ----

// Everything in this section was read out of retail members of
// `GOSDATA/ASSETS/crimson.rof` and is re-measured by
// `crates/cs_content/tests/accept_f28_d_retail_ordnance_catalogue.rs` on every
// run, so a stale constant here fails rather than passing. **It is a
// measurement of what shipped files declare — ids and counts — and says
// nothing about how the game behaves.** `retail` is read access, not evidence
// that the original executable ran.

/// The rocket identifier blocks the original's own resource header declares,
/// with the macro that declares each.
///
/// **Measured** in `ASSETS/SCRIPTS/RESOURCE.H`. The bases are gaps of **15**
/// apart and the first block the header declares after them is
/// [`ORIGINAL_NEXT_ROCKET_BLOCK`] at `3425`, so the rocket run is bounded at
/// 15 ids per block. The *type count* is **not** that width: it is
/// [`ORIGINAL_ROCKET_ORDNANCE_TYPES`], read from the selection screens.
pub const ORIGINAL_ROCKET_NAME_BLOCKS: [(u32, &str); 3] = [
    (3380, "IDS_ROCKETLONGNAME"),
    (3395, "IDS_ROCKETSHORTNAME"),
    (3410, "IDS_ROCKETDESCRIPTION"),
];

/// The first string-id block the resource header declares **after** the three
/// rocket blocks, with the macro that declares it.
///
/// **Measured**: this is what bounds the rocket run at 15 ids per block, and
/// it is the reason the blocks are not read as a type count — the gap from
/// `IDS_ROCKETDESCRIPTION 3410` is 15, which would give fifteen types, and
/// the screens say eleven.
pub const ORIGINAL_NEXT_ROCKET_BLOCK: (u32, &str) = (3425, "IDS_PAINTLONGNAME");

/// The rocket ordnance types the original's selection screens offer.
///
/// **Measured twice, independently**, and *not* from the block gaps:
///
/// * `ASSETS/SCRIPTS/MULTIPLAYER_AMMOR.SCRIPT` declares `string DIA[11]` and
///   asks the engine to fill it (`callback($$E$$,5058,DIA[0])`), builds a
///   header row plus eleven rows (`for(AIA=0; AIA < 11 + 1; AIA++)`), tests
///   eleven selectable types (`for(ZHA = 0; ZHA < 11; ZHA++)`) and indexes
///   the description block as `3410 + selection - 1`, so the descriptions
///   occupy `3410..=3420`;
/// * `ASSETS/SCRIPTS/MULTIPLAYER_OUTLAWROC.SCRIPT` asks for the same eleven
///   rocket names through a different callback on a different screen
///   (`for(int RX=0; RX < 11; RX++)` then `callback($$E$$,5019,(RX),YIA[RX])`).
pub const ORIGINAL_ROCKET_ORDNANCE_TYPES: u32 = 11;

/// The nitro control the original's resource header declares, with its id.
///
/// **Measured** in `ASSETS/SCRIPTS/RESOURCE.H`, and corroborated by the
/// engine's own dictionary, which names `fnitroout = SIA`. This is what makes
/// non-negotiable 2 checkable: the PC build ships a nitro control, so a
/// catalogue with no booster record is a gap rather than a complete design.
/// What the control *does* — capacity, burn, recovery, thrust, tradeoffs — is
/// not in any file and stays unmeasured.
pub const ORIGINAL_NITRO_CONTROL: (&str, u32) = ("MPOUT_CHK_NITRO", 10135);

/// The rocket slots one airframe offers, re-exported from
/// [`crate::weapons::ORIGINAL_ROCKET_SLOTS`].
///
/// **Measured**: `object EIA[8]` in `MULTIPLAYER_AMMOR.SCRIPT` and
/// `object RKA[8]` in `ORDINANCELAYOUT.SCRIPT`. One number, one place.
pub const ORIGINAL_ORDNANCE_ROCKET_SLOTS: u32 = ORIGINAL_ROCKET_SLOTS;

/// The hardpoint points one airframe offers, re-exported from
/// [`crate::weapons::ORIGINAL_HARDPOINT_POINTS`].
///
/// **Measured**: `object DT[2]`, `for (int R=0; R < 2; R++)` and the
/// `callback($$E$$, 2245, 0, (R), AT[R])` per-point read in
/// `HARDPOINTS.SCRIPT`. The same member shows the hardpoint's **weight** and
/// **cost** are named by the engine dictionary (`ohardpointweight`,
/// `ohardpointcost`) — the numbers are in the executable, not in a file.
pub const ORIGINAL_ORDNANCE_HARDPOINT_POINTS: u32 = ORIGINAL_HARDPOINT_POINTS;

/// The declared fields that lower into a runtime definition and that **no
/// production path reads**.
///
/// **Measured 2026-10-03 (F28-D)**, and recorded here so a field that is
/// declared, lowered and then read by nothing is a *named* gap rather than a
/// silent one. Each entry is `(declared field, the lowered field it reaches)`.
///
/// The area effect is the case: `cs_app::ordnance::lower_ordnance` lowers both
/// of its values into `cs_sim::weapons::ordnance::AreaEffect`, and the only
/// reader of `ProjectileOrdnance::area_effect` in the workspace is that
/// accessor. F28-C applies the record's declared *status effects* to one
/// stable recipient, which is what the timed engine-status path consumes; the
/// radius and the area's own bounded lifetime reach no gameplay code, so
/// non-negotiable 3's "bounded lifetimes and stable recipient ids" is enforced
/// for the status ledger and **not** for the area's reach. Follow-up task #552
/// (F28-AE1) owns implementing it; this list is where it is recorded until
/// then.
pub const DECLARED_FIELDS_WITHOUT_CONSUMER: [(&str, &str); 2] = [
    (
        "area_effect.radius_m",
        "cs_sim::weapons::ordnance::ProjectileOrdnance::area_effect",
    ),
    (
        "area_effect.lifetime_ticks",
        "cs_sim::weapons::ordnance::ProjectileOrdnance::area_effect",
    ),
];

/// The measured counts of one installation's ordnance surface.
///
/// Every count is nonzero except [`nitro_named`](Self::nitro_named), which is
/// a fact about the installation rather than a magnitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginalOrdnanceCounts {
    rocket_types: u32,
    rocket_slots: u32,
    hardpoint_points: u32,
    nitro_named: bool,
}

/// Why a measured ordnance count was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalOrdnanceSurfaceError {
    /// The installation was measured as offering no rocket ordnance type,
    /// which cannot be audited against.
    NoRocketTypes,
    /// The installation was measured as offering no rocket slot.
    NoRocketSlots,
    /// The installation was measured as offering no hardpoint point.
    NoHardpointPoints,
}

impl fmt::Display for OriginalOrdnanceSurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRocketTypes => {
                write!(
                    f,
                    "an installation must offer at least one rocket ordnance type"
                )
            }
            Self::NoRocketSlots => {
                write!(f, "an installation must offer at least one rocket slot")
            }
            Self::NoHardpointPoints => {
                write!(f, "an installation must offer at least one hardpoint point")
            }
        }
    }
}

impl std::error::Error for OriginalOrdnanceSurfaceError {}

impl OriginalOrdnanceCounts {
    /// Assembles the measured counts, refusing the magnitudes that cannot be
    /// audited against.
    ///
    /// # Errors
    ///
    /// [`OriginalOrdnanceSurfaceError`] on a zero type, slot or hardpoint
    /// count.
    pub const fn try_new(
        rocket_types: u32,
        rocket_slots: u32,
        hardpoint_points: u32,
        nitro_named: bool,
    ) -> Result<Self, OriginalOrdnanceSurfaceError> {
        if rocket_types == 0 {
            return Err(OriginalOrdnanceSurfaceError::NoRocketTypes);
        }
        if rocket_slots == 0 {
            return Err(OriginalOrdnanceSurfaceError::NoRocketSlots);
        }
        if hardpoint_points == 0 {
            return Err(OriginalOrdnanceSurfaceError::NoHardpointPoints);
        }
        Ok(Self {
            rocket_types,
            rocket_slots,
            hardpoint_points,
            nitro_named,
        })
    }

    /// How many rocket ordnance types the installation offers.
    #[must_use]
    pub const fn rocket_types(&self) -> u32 {
        self.rocket_types
    }

    /// How many rocket slots one airframe offers.
    #[must_use]
    pub const fn rocket_slots(&self) -> u32 {
        self.rocket_slots
    }

    /// How many hardpoint points one airframe offers.
    #[must_use]
    pub const fn hardpoint_points(&self) -> u32 {
        self.hardpoint_points
    }

    /// Whether the installation names a nitro control at all.
    #[must_use]
    pub const fn nitro_named(&self) -> bool {
        self.nitro_named
    }
}

/// One installation's measured ordnance surface: what its files declare, with
/// the span the measurement came from.
///
/// This is the closure target the audit needs. Without a number the
/// installation itself declares, "every original rocket type" is unfalsifiable
/// and a catalogue holding only the synthetic fixture's components would
/// satisfy any check there was — which is the failure AC04 names.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalOrdnanceSurface {
    origin: Origin,
    counts: OriginalOrdnanceCounts,
    provenance: Provenance,
}

impl OriginalOrdnanceSurface {
    /// Assembles a measured surface.
    #[must_use]
    pub const fn new(
        origin: Origin,
        counts: OriginalOrdnanceCounts,
        provenance: Provenance,
    ) -> Self {
        Self {
            origin,
            counts,
            provenance,
        }
    }

    /// The span the measurement came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The measured counts.
    #[must_use]
    pub const fn counts(&self) -> &OriginalOrdnanceCounts {
        &self.counts
    }

    /// How many rocket ordnance types the installation offers.
    #[must_use]
    pub const fn rocket_types(&self) -> u32 {
        self.counts.rocket_types()
    }

    /// How many rocket slots one airframe offers.
    #[must_use]
    pub const fn rocket_slots(&self) -> u32 {
        self.counts.rocket_slots()
    }

    /// How many hardpoint points one airframe offers.
    #[must_use]
    pub const fn hardpoint_points(&self) -> u32 {
        self.counts.hardpoint_points()
    }

    /// Whether the installation names a nitro control at all.
    #[must_use]
    pub const fn nitro_named(&self) -> bool {
        self.counts.nitro_named()
    }

    /// The provenance of the measurement itself.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// --------------------------------------------------------- the audit itself ----

/// How many of a record's load-bearing values are declared, how many of those
/// are explicit unknowns, and how many carry `verified_original` provenance.
///
/// The three numbers are what separate "the record has a number" from "the
/// number is the original's": the synthetic fixture fills every field, so only
/// the third one can tell a measured catalogue from a designed one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FieldTally {
    total: usize,
    unknown: usize,
    verified: usize,
}

impl FieldTally {
    /// How many load-bearing values the record declares.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    /// How many of them are explicit unknowns with a reason.
    #[must_use]
    pub const fn unknown(&self) -> usize {
        self.unknown
    }

    /// How many of them carry `verified_original` provenance.
    #[must_use]
    pub const fn verified(&self) -> usize {
        self.verified
    }

    /// Whether every declared value is an original measurement.
    #[must_use]
    pub const fn is_fully_measured(&self) -> bool {
        self.total > 0 && self.verified == self.total
    }

    /// Whether every declared value carries `verified_original` provenance.
    #[must_use]
    pub const fn is_verified(&self) -> bool {
        self.verified == self.total
    }
}

/// One walk of a declared record's load-bearing values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RecordWalk {
    tally: FieldTally,
    unknown_fields: Vec<&'static str>,
}

impl RecordWalk {
    fn value<T>(&mut self, name: &'static str, resolved: &Resolved<T>) {
        self.tally.total += 1;
        match resolved {
            Resolved::Unknown { .. } => {
                self.tally.unknown += 1;
                self.unknown_fields.push(name);
            }
            Resolved::Known(Known { provenance, .. }) => {
                if provenance.class == ClaimStatus::VerifiedOriginal {
                    self.tally.verified += 1;
                }
            }
        }
    }
}

/// Walks one declared record's load-bearing values, by their declared names.
///
/// The names are the ones the audit reports, so they are the ones a reader can
/// grep for. A field the walk does not visit is a field no audit can report,
/// which is why the list is spelled out here rather than derived.
fn walk_record(record: &DeclaredOrdnance) -> RecordWalk {
    let mut walk = RecordWalk::default();
    match record.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => {
            walk.value("launch.hardpoint", &projectile.launch.hardpoint);
            walk.value(
                "launch.launch_speed_mps",
                &projectile.launch.launch_speed_mps,
            );
            walk.value("launch.inheritance", &projectile.launch.inheritance);
            walk.value(
                "launch.release_delay_ticks",
                &projectile.launch.release_delay_ticks,
            );
            walk.value("stack.capacity_units", &projectile.stack.capacity_units);
            walk.value("stack.unit_mass_kg", &projectile.stack.unit_mass_kg);
            match &projectile.arming {
                DeclaredArmingRule::Disarmed => {}
                DeclaredArmingRule::AfterTicks(ticks) => {
                    walk.value("arming.after_ticks", ticks);
                }
                DeclaredArmingRule::AfterTravelMetres(metres) => {
                    walk.value("arming.after_travel_m", metres);
                }
            }
            match &projectile.fuse {
                DeclaredFuseRule::Impact => {}
                DeclaredFuseRule::Proximity(fuse) => {
                    walk.value("fuse.trigger_radius_m", &fuse.trigger_radius_m);
                }
                DeclaredFuseRule::Timed { ticks } => walk.value("fuse.ticks", ticks),
            }
            match &projectile.guidance {
                DeclaredGuidanceRule::Unguided => {}
                DeclaredGuidanceRule::Targeted { lost_target } => {
                    walk.value("guidance.lost_target", lost_target);
                }
            }
            walk.value("lifetime_ticks", &projectile.lifetime_ticks);
            if let Some(area) = &projectile.area_effect {
                walk.value("area_effect.radius_m", &area.radius_m);
                walk.value("area_effect.lifetime_ticks", &area.lifetime_ticks);
            }
            walk.value("armor_damage", &projectile.armor_damage);
            walk.value("internal_damage", &projectile.internal_damage);
            for (index, status) in projectile.status.iter().enumerate() {
                // The index keeps two same-named status fields distinguishable
                // in a report; the field name stays greppable.
                let _ = index;
                walk.value("status.duration_ticks", &status.duration_ticks);
                walk.value("status.strength", &status.strength);
            }
            walk.value("media.visual", &projectile.media.visual);
            walk.value("media.sound", &projectile.media.sound);
            if let Some(particles) = &projectile.media.particles {
                walk.value("media.particles", particles);
            }
            walk_equipment(&mut walk, &projectile.equipment_rules);
        }
        DeclaredOrdnanceDetails::Nitro(nitro) => {
            let parameters = &nitro.parameters;
            walk.value("nitro.capacity_units", &parameters.capacity_units);
            walk.value("nitro.consumption_per_s", &parameters.consumption_per_s);
            walk.value("nitro.recovery_per_s", &parameters.recovery_per_s);
            walk.value("nitro.extra_thrust_n", &parameters.extra_thrust_n);
            match &parameters.activation {
                Resolved::Known(Known {
                    value: DeclaredNitroActivationRule::FixedTicks { ticks },
                    ..
                }) => walk.value("nitro.activation.ticks", ticks),
                _ => walk.value("nitro.activation", &parameters.activation),
            }
            walk.value(
                "nitro.authority_multiplier",
                &parameters.authority_multiplier,
            );
            walk.value("media.visual", &nitro.media.visual);
            walk.value("media.sound", &nitro.media.sound);
            if let Some(particles) = &nitro.media.particles {
                walk.value("media.particles", particles);
            }
            walk_equipment(&mut walk, &nitro.equipment_rules);
        }
    }
    walk
}

fn walk_equipment(walk: &mut RecordWalk, rules: &DeclaredEquipmentRules) {
    if let Some(requires) = &rules.requires {
        walk.value("equipment_rules.requires", requires);
    }
    for forbids in &rules.forbids {
        walk.value("equipment_rules.forbids", forbids);
    }
}

/// The known, nonzero damage channels a declared launched item routes.
///
/// A channel that is an explicit unknown is **not** counted: it delivers
/// nothing that can be applied, which is the gap this number exists to expose.
fn known_damage_channels(record: &DeclaredOrdnance) -> usize {
    let Some(projectile) = record.projectile() else {
        return 0;
    };
    [&projectile.armor_damage, &projectile.internal_damage]
        .into_iter()
        .filter_map(|channel| match channel {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        })
        .filter(|amount| *amount > 0.0)
        .count()
}

/// One declared component's audited row.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceAuditRow {
    ordnance: ContentId,
    family: DeclaredOrdnanceFamily,
    booster: bool,
    tally: FieldTally,
    damage_channels: usize,
    status_effects: usize,
    area_declared: bool,
    origin: Origin,
    provenance: Provenance,
}

impl OrdnanceAuditRow {
    /// The catalog id this row audits.
    #[must_use]
    pub const fn ordnance(&self) -> &ContentId {
        &self.ordnance
    }

    /// The declared behavior family.
    #[must_use]
    pub const fn family(&self) -> DeclaredOrdnanceFamily {
        self.family
    }

    /// Whether this component is a booster rather than a launched item.
    #[must_use]
    pub const fn is_booster(&self) -> bool {
        self.booster
    }

    /// How many load-bearing values the record declares, and how many of them
    /// are original measurements.
    #[must_use]
    pub const fn tally(&self) -> FieldTally {
        self.tally
    }

    /// How many known, nonzero damage channels the item routes.
    #[must_use]
    pub const fn damage_channels(&self) -> usize {
        self.damage_channels
    }

    /// How many timed status effects the item applies.
    #[must_use]
    pub const fn status_effects(&self) -> usize {
        self.status_effects
    }

    /// Whether the record declares a bounded area effect.
    #[must_use]
    pub const fn declares_area(&self) -> bool {
        self.area_declared
    }

    /// Whether this component delivers **any** gameplay effect: a damage
    /// channel, a status effect, a bounded area, or the boost itself.
    #[must_use]
    pub const fn delivers_effect(&self) -> bool {
        self.booster || self.damage_channels > 0 || self.status_effects > 0 || self.area_declared
    }

    /// Whether every load-bearing value this record declares is an original
    /// measurement, and the record itself was measured from the installation.
    #[must_use]
    pub fn is_measured(&self) -> bool {
        self.tally.is_fully_measured() && self.provenance.class == ClaimStatus::VerifiedOriginal
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Where the record itself was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One gap the ordnance catalogue audit found, named.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceAuditFinding {
    /// The installation offers more rocket ordnance types than the declared
    /// catalogue enumerates, so at least one type has no row at all.
    UndeclaredRocketType {
        /// How many types the installation offers.
        observed: u32,
        /// How many the catalogue enumerates.
        declared: u32,
    },
    /// The catalogue enumerates as many or more components as the installation
    /// offers, but none of them is *attributed* to an original type.
    ///
    /// The rocket blocks in the resource header are a **count**, not a
    /// mapping: no shipped file says which of the eleven types a given record
    /// is, so a record can only be attributed by a measurement of that
    /// mapping. Without it, a catalogue of eleven invented names would
    /// satisfy the count check above and still name nothing the original has.
    UnattributedRocketType {
        /// How many types the installation offers.
        observed: u32,
        /// How many declared components are original measurements.
        attributed: u32,
    },
    /// The declared airframe layout offers fewer rocket slots than the
    /// installation's screens build.
    UnsupportedRocketSlots {
        /// How many slots the declared layout offers.
        declared: u32,
        /// How many the installation offers.
        observed: u32,
    },
    /// The declared airframe layout offers fewer hardpoint points than the
    /// installation's hardpoint screen builds.
    UnsupportedHardpointPoints {
        /// How many points the declared layout offers.
        declared: u32,
        /// How many the installation offers.
        observed: u32,
    },
    /// The installation ships a nitro control and the catalogue declares no
    /// booster record at all.
    MissingNitroRecord,
    /// A declared load-bearing value is an explicit unknown, so the lowering
    /// boundary refuses the record rather than flying it.
    UnmeasuredField {
        /// The record with the unknown field.
        ordnance: ContentId,
        /// The declared field that is unknown.
        field: &'static str,
    },
    /// A record whose values are not original measurements.
    ///
    /// The synthetic fixture fills every field, so this is what it reports for
    /// each of its components: the numbers are *declared*, never measured.
    UnmeasuredRecord {
        /// The record nothing measured.
        ordnance: ContentId,
        /// The claim status its provenance actually carries.
        class: &'static str,
    },
    /// A designed behavior family no declared record uses.
    ///
    /// The six families are leads from the sheet, not a catalogue. An unused
    /// family is a lead nothing confirmed — and the sheet's non-negotiable 1,
    /// "do not substitute every rocket with one homing missile", is only
    /// checkable in this direction.
    UnusedFamily {
        /// The family with no component.
        family: DeclaredOrdnanceFamily,
    },
    /// A declared field that lowers into the runtime and that no production
    /// path reads.
    ///
    /// See [`DECLARED_FIELDS_WITHOUT_CONSUMER`]. The field reaches a runtime
    /// definition and then stops: nothing applies it, so the rule it was
    /// written for is not enforced at runtime.
    UnconsumedField {
        /// The record declaring the field.
        ordnance: ContentId,
        /// The declared field nothing reads.
        field: &'static str,
    },
    /// A launched item that delivers nothing: no known nonzero damage channel,
    /// no status effect and no bounded area. It flies, sounds and lands for
    /// no gameplay effect at all.
    DeliversNoEffect {
        /// The record nothing consumes.
        ordnance: ContentId,
    },
}

impl OrdnanceAuditFinding {
    /// The stable machine-readable label of this finding.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::UndeclaredRocketType { .. } => "undeclared_rocket_type",
            Self::UnattributedRocketType { .. } => "unattributed_rocket_type",
            Self::UnsupportedRocketSlots { .. } => "unsupported_rocket_slots",
            Self::UnsupportedHardpointPoints { .. } => "unsupported_hardpoint_points",
            Self::MissingNitroRecord => "missing_nitro_record",
            Self::UnmeasuredField { .. } => "unmeasured_field",
            Self::UnmeasuredRecord { .. } => "unmeasured_record",
            Self::UnusedFamily { .. } => "unused_family",
            Self::UnconsumedField { .. } => "unconsumed_field",
            Self::DeliversNoEffect { .. } => "delivers_no_effect",
        }
    }
}

impl fmt::Display for OrdnanceAuditFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredRocketType { observed, declared } => write!(
                f,
                "the installation offers {observed} rocket ordnance types but the \
                 catalogue enumerates only {declared}"
            ),
            Self::UnattributedRocketType {
                observed,
                attributed,
            } => write!(
                f,
                "the installation offers {observed} rocket ordnance types and no shipped \
                 file maps any of them to a declared record, so {attributed} are attributed"
            ),
            Self::UnsupportedRocketSlots { declared, observed } => write!(
                f,
                "the declared airframe layout offers {declared} rocket slots, but the \
                 installation's screens build {observed}"
            ),
            Self::UnsupportedHardpointPoints { declared, observed } => write!(
                f,
                "the declared airframe layout offers {declared} hardpoint points, but the \
                 installation's hardpoint screen builds {observed}"
            ),
            Self::MissingNitroRecord => write!(
                f,
                "the installation names the nitro control {}, and the catalogue declares \
                 no booster",
                ORIGINAL_NITRO_CONTROL.0
            ),
            Self::UnmeasuredField { ordnance, field } => {
                write!(f, "{ordnance} leaves its {field} unmeasured")
            }
            Self::UnmeasuredRecord { ordnance, class } => write!(
                f,
                "{ordnance} is {class} content: none of its declared values is an \
                 original measurement"
            ),
            Self::UnusedFamily { family } => {
                write!(f, "the {family} family has no declared component")
            }
            Self::UnconsumedField { ordnance, field } => write!(
                f,
                "{ordnance} declares {field}, which lowers into the runtime and that no \
                 production path reads"
            ),
            Self::DeliversNoEffect { ordnance } => write!(
                f,
                "{ordnance} declares no damage, no status effect and no area, so a round \
                 of it costs a round and lands for nothing"
            ),
        }
    }
}

/// The result of an ordnance catalogue audit.
///
/// `complete` is the only verdict, and it is deliberately hard to reach: the
/// declared catalogue must enumerate at least as many launched components as
/// the installation offers, *attribute* them to original types, cover the
/// installation's rocket slots and hardpoint points, declare a booster when the
/// installation names a nitro control, use every designed family, carry no
/// unmeasured value and deliver something for every component. Anything else is
/// a named finding.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceAuditReport {
    rows: Vec<OrdnanceAuditRow>,
    findings: Vec<OrdnanceAuditFinding>,
}

impl OrdnanceAuditReport {
    /// One row per distinct declared component, in ascending id order.
    #[must_use]
    pub fn rows(&self) -> &[OrdnanceAuditRow] {
        &self.rows
    }

    /// The row for one component, if it is declared.
    #[must_use]
    pub fn row(&self, ordnance: &ContentId) -> Option<&OrdnanceAuditRow> {
        self.rows.iter().find(|row| row.ordnance == *ordnance)
    }

    /// Every gap found, in report order.
    #[must_use]
    pub fn findings(&self) -> &[OrdnanceAuditFinding] {
        &self.findings
    }

    /// The findings of one label, so a caller can name one gap at a time.
    #[must_use]
    pub fn findings_of(&self, label: &str) -> Vec<&OrdnanceAuditFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.label() == label)
            .collect()
    }

    /// How many distinct launched components the catalogue declares.
    #[must_use]
    pub fn declared_rocket_types(&self) -> usize {
        self.rows.iter().filter(|row| !row.is_booster()).count()
    }

    /// How many declared components are original measurements.
    #[must_use]
    pub fn attributed_rocket_types(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| !row.is_booster() && row.is_measured())
            .count()
    }

    /// How many boosters the catalogue declares.
    #[must_use]
    pub fn declared_boosters(&self) -> usize {
        self.rows.iter().filter(|row| row.is_booster()).count()
    }

    /// Whether the audit found no gap at all.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.findings.is_empty()
    }
}

/// The declared ordnance catalogue audit (F28-D).
///
/// The audit walks one installation's declared components and compares them
/// against the [`OriginalOrdnanceSurface`] its own files declare. It answers,
/// per component: which family it is, how many load-bearing values it declares
/// and how many of them are original measurements, whether it delivers any
/// gameplay effect at all, and whether it declares a field nothing reads.
///
/// # Why it reports rather than repairs
///
/// Nothing here fills a gap. A rocket type the installation offers and no
/// record enumerates, an unattributed component, an unused family and an
/// unconsumed field all stay gaps and are named, because the alternatives —
/// inventing an eleventh rocket, or assigning a family to a record nothing
/// measured — are exactly the guesses F28's "Research boundary" and
/// non-negotiable 1 forbid. An audit that always passed would be worse than
/// none: it would let a six-component synthetic catalogue stand in for the
/// original's eleven.
#[derive(Clone, Debug, Default)]
pub struct OrdnanceAudit {
    ordnance: Vec<DeclaredOrdnance>,
    rocket_slots: u32,
    hardpoint_points: u32,
}

impl OrdnanceAudit {
    /// An empty audit with no declared airframe layout, so every slot and
    /// hardpoint finding fires until [`with_layout`](Self::with_layout) is
    /// called.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty audit over a declared airframe layout.
    #[must_use]
    pub const fn with_layout(rocket_slots: u32, hardpoint_points: u32) -> Self {
        Self {
            ordnance: Vec::new(),
            rocket_slots,
            hardpoint_points,
        }
    }

    /// Adds one declared component record.
    pub fn add(&mut self, record: DeclaredOrdnance) -> &mut Self {
        self.ordnance.push(record);
        self
    }

    /// How many declared records the audit holds, duplicates included.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.ordnance.len()
    }

    /// The declared rocket slots per airframe the audit compares against.
    #[must_use]
    pub const fn declared_rocket_slots(&self) -> u32 {
        self.rocket_slots
    }

    /// The declared hardpoint points per airframe the audit compares against.
    #[must_use]
    pub const fn declared_hardpoint_points(&self) -> u32 {
        self.hardpoint_points
    }

    /// Runs the audit against a measured installation surface.
    #[must_use]
    pub fn run(&self, original: &OriginalOrdnanceSurface) -> OrdnanceAuditReport {
        let mut rows = Vec::new();
        let mut findings = Vec::new();

        // One row per distinct component, in ascending id order, so two
        // records for the same id cannot inflate the type count past the
        // closure check. The first in insertion order is the one reported; the
        // disagreement between two records for one id is the importer's
        // business, not something to average away here.
        let mut distinct: BTreeMap<&str, &DeclaredOrdnance> = BTreeMap::new();
        for record in &self.ordnance {
            distinct.entry(record.ordnance().as_str()).or_insert(record);
        }

        let mut families_in_use: BTreeSet<DeclaredOrdnanceFamily> = BTreeSet::new();
        let mut launched = 0u32;
        let mut attributed = 0u32;
        let mut boosters = 0u32;

        for record in distinct.values() {
            let walk = walk_record(record);
            let ordnance = record.ordnance().clone();
            let booster = record.nitro().is_some();
            let damage_channels = known_damage_channels(record);
            let status_effects = record.projectile().map_or(0, |item| item.status.len());
            let area_declared = record
                .projectile()
                .is_some_and(|item| item.area_effect.is_some());
            let row = OrdnanceAuditRow {
                ordnance: ordnance.clone(),
                family: record.family(),
                booster,
                tally: walk.tally,
                damage_channels,
                status_effects,
                area_declared,
                origin: record.origin().clone(),
                provenance: record.provenance().clone(),
            };
            families_in_use.insert(record.family());
            if booster {
                boosters += 1;
            } else {
                launched += 1;
            }
            if row.is_measured() && !booster {
                attributed += 1;
            }

            for field in &walk.unknown_fields {
                findings.push(OrdnanceAuditFinding::UnmeasuredField {
                    ordnance: ordnance.clone(),
                    field,
                });
            }
            if record.provenance().class != ClaimStatus::VerifiedOriginal {
                findings.push(OrdnanceAuditFinding::UnmeasuredRecord {
                    ordnance: ordnance.clone(),
                    class: record.provenance().class.label(),
                });
            }
            if area_declared {
                for (field, _) in DECLARED_FIELDS_WITHOUT_CONSUMER {
                    findings.push(OrdnanceAuditFinding::UnconsumedField {
                        ordnance: ordnance.clone(),
                        field,
                    });
                }
            }
            if !row.delivers_effect() {
                findings.push(OrdnanceAuditFinding::DeliversNoEffect { ordnance });
            }
            rows.push(row);
        }

        // The closure checks against the installation, in both directions.
        if launched < original.rocket_types() {
            findings.push(OrdnanceAuditFinding::UndeclaredRocketType {
                observed: original.rocket_types(),
                declared: launched,
            });
        }
        if attributed < original.rocket_types() {
            findings.push(OrdnanceAuditFinding::UnattributedRocketType {
                observed: original.rocket_types(),
                attributed,
            });
        }
        if self.rocket_slots < original.rocket_slots() {
            findings.push(OrdnanceAuditFinding::UnsupportedRocketSlots {
                declared: self.rocket_slots,
                observed: original.rocket_slots(),
            });
        }
        if self.hardpoint_points < original.hardpoint_points() {
            findings.push(OrdnanceAuditFinding::UnsupportedHardpointPoints {
                declared: self.hardpoint_points,
                observed: original.hardpoint_points(),
            });
        }
        if original.nitro_named() && boosters == 0 {
            findings.push(OrdnanceAuditFinding::MissingNitroRecord);
        }
        for family in DeclaredOrdnanceFamily::ALL {
            if !families_in_use.contains(family) {
                findings.push(OrdnanceAuditFinding::UnusedFamily { family: *family });
            }
        }

        OrdnanceAuditReport { rows, findings }
    }
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
