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
//! them apart is the same split the runtime `OrdnanceComponent`
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
//!
//! # The per-component launch table (T452.04)
//!
//! T452.01 classified every `wep_` record to a family. This module now also
//! reads what those records *declare* per component —
//! [`read_original_component_table`]: the launch speed (`VELOCITY`), the
//! declared release rate (`FIRE_RATE`), any stated lifetime, the area radius
//! and lifetime, the damage channels, every status/effect flag with its stated
//! duration, and what `CLUSTER_SIZE` states — against the mount layout the two
//! ordnance screens declare ([`read_original_mount_layout`]: eight rocket
//! slots, four gun/ammunition slots, two hardpoint points, each read from its
//! own `object <array>[n]` declaration and fill loop through the base retail
//! archive). Every value no file states is a named [`OriginalComponentUnknown`],
//! never a default: the `VELOCITY` unit, what `FIRE_RATE` delays, the absent
//! `EXPIRES` lifetime, a status effect's gameplay kind, the area-denial
//! effect's radius and lifetime, which slot holds a record with no `ROCKET`
//! flag, what each clustered sub-munition does, and which component fills which
//! slot on a given airframe. Details and measurements:
//! `docs/findings/2026-10-25-t452-04-component-hardpoints-launch-speeds-and-lifetimes.md`.

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
    /// The measured lateral-remainder rule
    /// ([`OriginalInheritance::LateralRemainder`]): the launch-direction part
    /// of the launcher's velocity is removed, a speed scalar carries it from
    /// `|launcher|` and ramps toward `launch speed + |launcher|` at the
    /// declared `acceleration_mps2`.
    LateralRemainder,
}

impl fmt::Display for DeclaredInheritanceRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Full => f.write_str("full"),
            Self::Fraction { share } => write!(f, "fraction({share})"),
            Self::None => f.write_str("none"),
            Self::LateralRemainder => f.write_str("lateral_remainder"),
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
    /// The per-second gain the launch-direction scalar runs at for
    /// [`DeclaredInheritanceRule::LateralRemainder`], in metres per second
    /// squared — the record's `ACCELERATION` (`+0x38`). `None` for every
    /// rule that runs no ramp.
    pub acceleration_mps2: Resolved<Option<f64>>,
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
///
/// `ExpiresAtRange` is the original's **measured** end of flight
/// (`docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md`
/// §3): the item is consumed, with no detonation, once the path it has flown
/// reaches its record's `RANGE`. Its bound is a [`Resolved`] distance for the
/// same reason a `Timed` fuse's ticks are — a default range would fly a
/// fabricated distance.
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
    /// Ends — without detonating — once the path flown reaches this many
    /// meters, the record's own `RANGE`.
    ExpiresAtRange {
        /// The declared `RANGE` bound, in meters.
        range_m: Resolved<f64>,
    },
}

impl fmt::Display for DeclaredFuseRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Impact => f.write_str("impact"),
            Self::Proximity(_) => f.write_str("proximity"),
            Self::Timed { .. } => f.write_str("timed"),
            Self::ExpiresAtRange { .. } => f.write_str("expires_at_range"),
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
    /// Fly on like `Coast`, but detonate at the end of the item's lifetime
    /// (the original's measured lost-target rule).
    CoastThenDetonate,
    /// Stop being live: no detonation, no damage, no effect.
    Disarm,
}

impl fmt::Display for DeclaredLostTargetBehavior {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Detonate => "detonate",
            Self::Coast => "coast",
            Self::CoastThenDetonate => "coast-then-detonate",
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
    /// Re-selects its target every step: the newest live marker, or none.
    /// Losing a target is not terminal, so there is no lost-target behavior
    /// to declare.
    MarkerSeeker,
}

impl fmt::Display for DeclaredGuidanceRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unguided => f.write_str("unguided"),
            Self::Targeted { .. } => f.write_str("targeted"),
            Self::MarkerSeeker => f.write_str("marker_seeker"),
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
    /// The sound resource a launch or detonation plays; `None` when the
    /// record declares none, which is a measured silence and not an unknown.
    pub sound: Option<Resolved<ContentId>>,
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
    /// The stun the item's smoke emitter applies to the actors it reaches
    /// (`SMOKE_SCREEN`), when the record declares one. Carries keys, not
    /// resolved globals: the registration is told the installation's values by
    /// [`SmokeStunRule::resolve`].
    pub smoke_stun: Option<SmokeStunRule>,
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
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredNitroActivationRule {
    /// Active on every tick the control is held and capacity remains.
    WhileHeld,
    /// Each accepted activation runs for this many whole ticks.
    FixedTicks {
        /// The declared burn length.
        ticks: Resolved<u64>,
    },
    /// The measured original rule: a start needs the control and at least
    /// `start_min_fraction` of capacity; a running burn latches and ends only
    /// when charge falls below `stop_below_fraction` (releasing the control
    /// does not stop it).
    UntilCharge {
        /// Fraction of capacity a start needs.
        start_min_fraction: Resolved<f64>,
        /// Fraction of capacity below which a running burn is cut.
        stop_below_fraction: Resolved<f64>,
    },
}

impl fmt::Display for DeclaredNitroActivationRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WhileHeld => f.write_str("while_held"),
            Self::FixedTicks { .. } => f.write_str("fixed_ticks"),
            Self::UntilCharge { .. } => f.write_str("until_charge"),
        }
    }
}

/// Whether a nitro booster recovers capacity on a tick whose activation was
/// refused because the tank was empty.
///
/// The original's rule is unmeasured (`f28.d.limit.nitro_recovery`), so this
/// is a [`Resolved`] option with no default: an unresolved one refuses to
/// lower rather than becoming either choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredNitroRefusedRecovery {
    /// A refused tick recovers like any other tick the booster did not run.
    Recovers,
    /// A refused tick recovers nothing.
    Withheld,
}

impl fmt::Display for DeclaredNitroRefusedRecovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Recovers => "recovers",
            Self::Withheld => "withheld",
        })
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
    /// Whether capacity recovers on a tick whose activation was refused.
    pub recovery_on_refused: Resolved<DeclaredNitroRefusedRecovery>,
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
                validate_media(visual, sound.as_ref(), particles.as_ref())?;
            }
            DeclaredOrdnanceDetails::Nitro(nitro) => {
                let DeclaredOrdnanceMedia {
                    visual,
                    sound,
                    particles,
                } = &nitro.media;
                validate_media(visual, sound.as_ref(), particles.as_ref())?;
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
    /// A known launch acceleration was NaN or infinite.
    NonFiniteAcceleration,
    /// A known launch acceleration was zero or negative.
    NonPositiveAcceleration {
        /// The rejected value.
        acceleration_mps2: f64,
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
    /// A known `RANGE` fuse bound was NaN or infinite.
    NonFiniteFuseRange,
    /// A known `RANGE` fuse bound was zero or negative.
    NonPositiveFuseRange {
        /// The rejected value.
        range_m: f64,
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
            Self::NonFiniteAcceleration => {
                write!(f, "the launch acceleration must be finite")
            }
            Self::NonPositiveAcceleration { acceleration_mps2 } => {
                write!(
                    f,
                    "the launch acceleration must be positive: {acceleration_mps2}"
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
            Self::NonFiniteFuseRange => write!(f, "the RANGE fuse bound must be finite"),
            Self::NonPositiveFuseRange { range_m } => {
                write!(f, "the RANGE fuse bound must be positive: {range_m}")
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
    if let Resolved::Known(known) = &projectile.launch.acceleration_mps2
        && let Some(acceleration_mps2) = known.value
    {
        if !acceleration_mps2.is_finite() {
            return Err(OrdnanceSchemaError::NonFiniteAcceleration);
        }
        if acceleration_mps2 <= 0.0 {
            return Err(OrdnanceSchemaError::NonPositiveAcceleration { acceleration_mps2 });
        }
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
        DeclaredFuseRule::ExpiresAtRange {
            range_m: Resolved::Known(known),
        } => {
            if !known.value.is_finite() {
                return Err(OrdnanceSchemaError::NonFiniteFuseRange);
            }
            if known.value <= 0.0 {
                return Err(OrdnanceSchemaError::NonPositiveFuseRange {
                    range_m: known.value,
                });
            }
        }
        DeclaredFuseRule::Impact | DeclaredFuseRule::Timed { .. } => {}
        DeclaredFuseRule::ExpiresAtRange {
            range_m: Resolved::Unknown { .. },
        } => {}
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
    if let Resolved::Known(known) = &parameters.activation
        && let DeclaredNitroActivationRule::UntilCharge {
            start_min_fraction,
            stop_below_fraction,
        } = &known.value
        && let (Resolved::Known(start), Resolved::Known(stop)) =
            (start_min_fraction, stop_below_fraction)
        && !(start.value.is_finite()
            && stop.value.is_finite()
            && start.value > 0.0
            && start.value <= 1.0
            && stop.value >= 0.0
            && stop.value < start.value)
    {
        return Err(OrdnanceSchemaError::CorruptNitroValue {
            field: "activation",
            reason: "charge fractions must satisfy 0 <= stop < start <= 1",
        });
    }
    Ok(())
}

/// The known-value sanity checks on a declared media record.
fn validate_media(
    visual: &Resolved<ContentId>,
    sound: Option<&Resolved<ContentId>>,
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
    if let Some(sound) = sound {
        check(sound, ContentKind::Sound)?;
    }
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
/// Each entry is `(declared field, the lowered field it reaches)`. **Empty
/// since F28-AE1**: the area effect's radius and lifetime are read by
/// `cs_sim::weapons::ordnance::OrdnanceRuntime::open_area` (and its
/// guidance-loss counterpart), which applies the area once to every actor
/// inside the radius and expires it on its boundary tick. The list stays as
/// the place a future declared-but-unread field is recorded, and the audit
/// that walks it keeps reporting `UnconsumedField` for any entry.
///
/// Reading the field is not a claim about the original: the radius shape is
/// the one measured for the guidance-loss blast, and the original's area
/// semantics for a fuse-triggered item are unmeasured (see
/// `docs/findings/2026-10-11-f28-ae1-area-effect-reach.md`).
pub const DECLARED_FIELDS_WITHOUT_CONSUMER: [(&str, &str); 0] = [];

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
    ///
    /// A record that declares no load-bearing value at all is **not** fully
    /// measured: an empty tally cannot tell "nothing to measure" from
    /// "everything measured", and the audit must not read a bare variant as an
    /// original rocket.
    #[must_use]
    pub const fn is_fully_measured(&self) -> bool {
        self.total > 0 && self.verified == self.total
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
                DeclaredFuseRule::ExpiresAtRange { range_m } => {
                    walk.value("fuse.range_m", range_m);
                }
            }
            match &projectile.guidance {
                DeclaredGuidanceRule::Unguided | DeclaredGuidanceRule::MarkerSeeker => {}
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
            // Two same-named status fields are counted separately but reported
            // under the same name, so a report names the field rather than which
            // status of a record it came from. The count is exact; the label is
            // not, and a reader who must tell two statuses apart reads the record.
            for status in &projectile.status {
                walk.value("status.duration_ticks", &status.duration_ticks);
                walk.value("status.strength", &status.strength);
            }
            walk.value("media.visual", &projectile.media.visual);
            if let Some(sound) = &projectile.media.sound {
                walk.value("media.sound", sound);
            }
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
            walk.value("nitro.recovery_on_refused", &parameters.recovery_on_refused);
            walk.value("nitro.extra_thrust_n", &parameters.extra_thrust_n);
            match &parameters.activation {
                Resolved::Known(Known {
                    value: DeclaredNitroActivationRule::FixedTicks { ticks },
                    ..
                }) => walk.value("nitro.activation.ticks", ticks),
                Resolved::Known(Known {
                    value:
                        DeclaredNitroActivationRule::UntilCharge {
                            start_min_fraction,
                            stop_below_fraction,
                        },
                    ..
                }) => {
                    walk.value("nitro.activation.start_min_fraction", start_min_fraction);
                    walk.value("nitro.activation.stop_below_fraction", stop_below_fraction);
                }
                _ => walk.value("nitro.activation", &parameters.activation),
            }
            walk.value(
                "nitro.authority_multiplier",
                &parameters.authority_multiplier,
            );
            walk.value("media.visual", &nitro.media.visual);
            if let Some(sound) = &nitro.media.sound {
                walk.value("media.sound", sound);
            }
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
        sound: Some(declared_known(
            ContentId::from_source(ContentKind::Sound, DECLARED_SYNTHETIC_SOUND_KEY)
                .expect("valid id"),
        )),
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
        acceleration_mps2: declared_known(None),
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
        smoke_stun: None,
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
                recovery_on_refused: declared_known(DeclaredNitroRefusedRecovery::Recovers),
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

// ------------------------------------------------ the measured original blast ----

/// The `ZBD/zrdr.zbd` member the original's weapon records live in.
pub const ORIGINAL_WEAPONS_MEMBER: &str = "weapons.zrd";

/// The key whose presence marks a weapon record as a rocket.
pub const ORIGINAL_ROCKET_FLAG: &str = "ROCKET";

/// One retail rocket record's blast-relevant declarations, exactly as stored.
///
/// **Measured** from `weapons.zrd` (task #548,
/// `docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`). Every
/// distance is in metres: task #1318 measured in the engine image that the
/// record values reach the world-space distance comparisons with no unit
/// conversion, and the world unit is the metre (the F16-E calibration
/// landmarks). `docs/findings/2026-10-18-t1318-blast-unknowns-measured.md`
/// carries the byte windows.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalRocketBlast {
    /// The record name (`wep_04`).
    pub record: String,
    /// `IMPACT_PROXIMITY`: the blast radius, metres.
    pub impact_proximity: Option<f64>,
    /// `DETONATION_DISTANCE`: the lock-on proximity trigger, metres.
    pub detonation_distance: Option<f64>,
    /// `DETONATION_TIME`: the timer trigger, seconds.
    pub detonation_time_s: Option<f64>,
    /// `ARMOR_DAMAGE` at the blast centre.
    pub armor_damage: Option<f64>,
    /// `HEALTH_DAMAGE` at the blast centre.
    pub health_damage: Option<f64>,
    /// `LOCK_ON`, the first value.
    pub lock_on: Option<f64>,
    /// `RANGE`, the first value: the distance at which the projectile's
    /// flight ends, metres (task #454).
    pub range: Option<f64>,
    /// `RANGE_MINIMUM`, the first value: the arming distance, metres. Measured
    /// (task #1318): an impact or detonation inside it is governed by
    /// [`Self::range_minimum_blast_scale`].
    pub range_minimum: Option<f64>,
    /// `RANGE_MINIMUM`, the second value: inside the arming window the
    /// projectile's blast-and-damage scale is multiplied by it — `0.0`
    /// suppresses the detonation outright (the torpedo's `300, 0` pair).
    pub range_minimum_blast_scale: Option<f64>,
    /// Every key the record declares, at its top level, sorted.
    pub keys: BTreeSet<String>,
}

/// What a locked-on rocket does at the end of its `RANGE` after it lost its
/// target, read from the weapon flags (task #454).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OriginalRangeEnd {
    /// The projectile detonates: locked-on and no `EXPIRES` key.
    Detonates,
    /// `EXPIRES` is declared: it detonates only with `DETONATE_AT_RANGE`.
    DetonatesOnlyWithDetonateAtRange,
}

/// What one retail guided rocket does once its target pointer is zero.
///
/// **Measured** (task #454,
/// `docs/findings/2026-10-25-f28-guidance-loss-per-weapon-audit.md`): a lost
/// target is a zero pointer at projectile offset `+0x70`; the steering call
/// (`0x5af7f9`) and the proximity branch (`0x5b021b`) both skip on it, so the
/// rocket keeps its last heading and only the timer and the end of `RANGE` can
/// detonate it. Nothing re-acquires.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalLostTargetRule {
    /// The record name (`wep_04`).
    pub record: String,
    /// `DETONATION_DISTANCE`, metres: the proximity trigger a lost target no
    /// longer reaches.
    pub proximity_trigger_lost_m: Option<f64>,
    /// `DETONATION_TIME`, seconds, when positive: the surviving timer trigger.
    pub timer_s: Option<f64>,
    /// `RANGE`, metres: where the flight ends.
    pub range_m: Option<f64>,
    /// What the end of `RANGE` does.
    pub range_end: OriginalRangeEnd,
}

impl OriginalRocketBlast {
    /// The lost-target rule of a guided (`LOCK_ON`) rocket; `None` for an
    /// unguided one, which has no target to lose.
    #[must_use]
    pub fn lost_target_rule(&self) -> Option<OriginalLostTargetRule> {
        self.lock_on?;
        let range_end = if !self.keys.contains("EXPIRES") || self.keys.contains("DETONATE_AT_RANGE")
        {
            OriginalRangeEnd::Detonates
        } else {
            OriginalRangeEnd::DetonatesOnlyWithDetonateAtRange
        };
        Some(OriginalLostTargetRule {
            record: self.record.clone(),
            proximity_trigger_lost_m: self.detonation_distance.filter(|d| *d > 0.0),
            timer_s: self.detonation_time_s.filter(|t| *t > 0.0),
            range_m: self.range,
            range_end,
        })
    }
}

/// How the original samples the proximity fuse against the target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OriginalProximitySampling {
    /// One point sample per projectile step: the squared distance between the
    /// projectile's position at the start of the step and the target's
    /// current position. No segment is swept.
    PointSamplePerStep,
}

/// One retail guided rocket's proximity fuse, read from its record.
///
/// **Measured** statically (task #2128,
/// `docs/findings/2026-10-25-t452-02-proximity-fuse-shape-and-arming.md`): the
/// trigger is a sphere of radius `DETONATION_DISTANCE` around the target
/// position (squared distance `<=` the squared radius, boundary inclusive),
/// sampled once per step ([`OriginalProximitySampling`]). It is live only
/// while the distance flown is below `RANGE`, the weapon is `LOCK_ON`, a
/// target is held and `DETONATION_DISTANCE` is positive. `RANGE_MINIMUM` is
/// not tested on this path.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalProximityFuse {
    /// The record name (`wep_04`).
    pub record: String,
    /// `DETONATION_DISTANCE`, metres: the sphere radius.
    pub trigger_radius_m: f64,
    /// `RANGE`, metres: the fuse is live only below it.
    pub live_below_range_m: Option<f64>,
    /// How the fuse is sampled.
    pub sampling: OriginalProximitySampling,
    /// `RANGE_MINIMUM`, metres, when declared: it gates impact detonations
    /// (`0x5b063d`), **not** this fuse. Carried so a catalogue can show the
    /// record declares an arming window the proximity burst ignores.
    pub range_minimum_not_tested_m: Option<f64>,
}

impl OriginalRocketBlast {
    /// The proximity fuse of a guided rocket with a positive
    /// `DETONATION_DISTANCE`; `None` otherwise (no lock, or no radius).
    #[must_use]
    pub fn proximity_fuse(&self) -> Option<OriginalProximityFuse> {
        self.lock_on?;
        let radius = self.detonation_distance.filter(|d| *d > 0.0)?;
        Some(OriginalProximityFuse {
            record: self.record.clone(),
            trigger_radius_m: radius,
            live_below_range_m: self.range,
            sampling: OriginalProximitySampling::PointSamplePerStep,
            range_minimum_not_tested_m: self.range_minimum,
        })
    }
}

/// The original's nitro booster, as measured in the engine image.
///
/// **Static** measurement (task #2129,
/// `docs/findings/2026-10-25-t452-03-nitro-activation-capacity-and-tradeoffs.md`):
/// nothing here is `verified_original`. The charge is one pool shared by every
/// airframe whose plane record sets `nitro`: the constants are stored once, at
/// actor construction (`0x4b02c4`), and no later store was found. Times are
/// the engine's frame delta (`0x9ad744`); that it counts seconds is assumed
/// from the status timers that subtract it, not proven.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OriginalNitro {
    /// The pool size (`0x41f00000`).
    pub capacity: f64,
    /// Charge consumed per delta while the booster runs (`0x40800000`).
    pub consumption_per_dt: f64,
    /// Charge recovered per delta on **every** step, running or not
    /// (`0x3f800000`).
    pub recovery_per_dt: f64,
    /// A start needs at least this fraction of the capacity (`0x3f7d70a4`).
    pub start_min_fraction: f64,
    /// The booster is cut when the charge falls below this fraction of the
    /// capacity (`0x3d4ccccd`).
    pub stop_below_fraction: f64,
    /// While running, the engine-record value `actor+0x128` is replaced by this
    /// (`0x3fe66666`) in the speed/thrust step `0x48fc40`.
    pub running_engine_value: f64,
    /// While running, the thrust step's result is multiplied by this
    /// (`0x3f4ccccd`); otherwise by `1.0`.
    pub running_result_factor: f64,
}

/// The measured nitro booster. See [`OriginalNitro`].
pub const ORIGINAL_NITRO: OriginalNitro = OriginalNitro {
    capacity: 30.0,
    consumption_per_dt: 4.0,
    recovery_per_dt: 1.0,
    start_min_fraction: 0.99,
    stop_below_fraction: 0.05,
    running_engine_value: 1.8,
    running_result_factor: 0.8,
};

impl OriginalNitro {
    /// One step of the charge (`0x49f810..0x49f882`): minus consumption when
    /// running, plus recovery always, clamped into `[0, capacity]`.
    #[must_use]
    pub fn step_charge(&self, charge: f64, running: bool, dt: f64) -> f64 {
        let mut charge = charge;
        if running {
            charge -= dt * self.consumption_per_dt;
        }
        charge += dt * self.recovery_per_dt;
        charge.clamp(0.0, self.capacity)
    }

    /// The player's start/stop decision (`0x487e91..0x487efc`), as the argument
    /// the original passes to its set-running routine (`0x4b2110`): `Some(false)`
    /// cuts the booster, `Some(true)` runs it, `None` leaves it alone.
    ///
    /// Releasing the control does not appear: a running booster is cut only by
    /// the charge falling below [`Self::stop_below_fraction`] (or by the
    /// callers listed in the finding), never by the control.
    #[must_use]
    pub fn player_command(
        &self,
        charge: f64,
        running: bool,
        control_pressed: bool,
    ) -> Option<bool> {
        if self.capacity * self.stop_below_fraction > charge {
            Some(false)
        } else if self.capacity * self.start_min_fraction > charge {
            running.then_some(true)
        } else if control_pressed {
            Some(true)
        } else {
            None
        }
    }
}

/// Decodes the `ZBD/zrdr.zbd` member `member` of the installation at
/// `install_root` into a [`SpannedDocument`], install and member digests
/// attached.
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] for an undiscoverable
/// installation, an unreadable archive, a missing or undecodable member or a
/// member whose shape is not the record list.
fn read_member_document(
    install_root: &std::path::Path,
    member: &str,
) -> Result<crate::original_airframe::SpannedDocument, crate::original_airframe::OriginalImportError>
{
    use crate::original_airframe::{OriginalImportError, READER_ARCHIVE, SpannedDocument};
    use cs_formats::io::ParseContext;
    use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};

    let found = cs_assets::install::discover(install_root)
        .map_err(|error| OriginalImportError::Io(error.to_string()))?;
    let install_sha256 = cs_assets::install::fingerprint(&found.manifest);
    let bytes = std::fs::read(install_root.join(READER_ARCHIVE))
        .map_err(|error| OriginalImportError::Io(error.to_string()))?;
    let spelling = cs_types::install::RelativePath::new(READER_ARCHIVE)
        .map_err(|error| OriginalImportError::Io(error.to_string()))?;
    let mut context = ParseContext::with_defaults("original_blast");
    let decision = dispatch(ZbdProbe::new(READER_ARCHIVE, &spelling, &bytes))
        .map_err(|error| OriginalImportError::Archive(error.to_string()))?;
    let index = read_version_one_index(&mut context, decision, &bytes)
        .map_err(|error| OriginalImportError::Archive(error.to_string()))?;
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data())
        .map_err(|error| OriginalImportError::Archive(error.to_string()))?;
    let entry = archive
        .entries()
        .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
        .ok_or_else(|| OriginalImportError::MemberMissing {
            member: member.to_owned(),
        })?;
    let content = entry.content().to_vec();
    let member_sha256 = cs_assets::install::sha256(&content);
    SpannedDocument::decode(
        install_sha256,
        member,
        &content,
        entry.span().offset,
        member_sha256,
    )
}

/// The provenance of the member every `wep_` record is declared in.
///
/// The span is the installation's own record member — the reader archive
/// `ZBD/zrdr.zbd`, member [`ORIGINAL_WEAPONS_MEMBER`] — with the installation
/// and member digests the production reader measured, at
/// [`ClaimStatus::ObservedTool`] strength. Task T452-RUNTIME-REGISTER's lowering
/// needs a location for the declared records it registers; a component read
/// from the owner's files must not be stamped `Origin::SyntheticFixture`.
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] for an undiscoverable
/// installation, an unreadable archive, a missing or undecodable member, or a
/// claim id that cannot be formed.
pub fn read_original_weapons_provenance(
    install_root: &std::path::Path,
    claim: &str,
) -> Result<Provenance, crate::original_airframe::OriginalImportError> {
    let document = read_member_document(install_root, ORIGINAL_WEAPONS_MEMBER)?;
    document.provenance(document.root(), claim)
}

/// Reads every `ROCKET` record of the installation's `weapons.zrd`.
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] for an undiscoverable
/// installation, an unreadable archive, a missing or undecodable member or a
/// member whose shape is not the record list.
pub fn read_original_rocket_blasts(
    install_root: &std::path::Path,
) -> Result<Vec<OriginalRocketBlast>, crate::original_airframe::OriginalImportError> {
    let document = read_member_document(install_root, ORIGINAL_WEAPONS_MEMBER)?;
    let mut records = Vec::new();
    collect_wep_records(document.root(), &mut records);
    let mut blasts = Vec::new();
    for (name, body) in records {
        if let Some(blast) = rocket_blast(name, body) {
            blasts.push(blast);
        }
    }
    if blasts.is_empty() {
        return Err(crate::original_airframe::OriginalImportError::Shape {
            detail: "no `wep_` record with a ROCKET key was found".to_owned(),
        });
    }
    Ok(blasts)
}

/// Walks the decoded member for `(name, body)` pairs whose name begins `wep_`.
///
/// The member's root also carries a table of integers and a header of global
/// keys (`VERSION`, `BALLISTICS`, ...), so the records are found by their own
/// shape rather than by a position the member does not promise.
fn collect_wep_records<'a>(
    node: &'a crate::original_airframe::SpannedValue,
    out: &mut Vec<(&'a str, &'a [crate::original_airframe::SpannedValue])>,
) {
    let Some(children) = node.as_list() else {
        return;
    };
    let mut index = 0;
    while index < children.len() {
        let record = children[index]
            .as_text()
            .filter(|name| name.starts_with("wep_"))
            .zip(children.get(index + 1).and_then(|body| body.as_list()));
        if let Some((name, body)) = record {
            out.push((name, body));
            index += 2;
        } else {
            collect_wep_records(&children[index], out);
            index += 1;
        }
    }
}

/// One record's declarations, when it is a rocket.
fn rocket_blast(
    name: &str,
    body: &[crate::original_airframe::SpannedValue],
) -> Option<OriginalRocketBlast> {
    let mut keys = BTreeSet::new();
    let mut scalar = BTreeMap::new();
    let mut second = BTreeMap::new();
    for field in body.chunks(2) {
        let [key, value] = field else { continue };
        let Some(key) = key.as_text() else { continue };
        keys.insert(key.to_owned());
        let Some(list) = value.as_list() else {
            continue;
        };
        if let Some(number) = list
            .first()
            .and_then(crate::original_airframe::SpannedValue::as_scalar_f64)
        {
            scalar.insert(key.to_owned(), number);
        }
        if let Some(number) = list
            .get(1)
            .and_then(crate::original_airframe::SpannedValue::as_scalar_f64)
        {
            second.insert(key.to_owned(), number);
        }
    }
    if !keys.contains(ORIGINAL_ROCKET_FLAG) {
        return None;
    }
    let number = |key: &str| scalar.get(key).copied();
    Some(OriginalRocketBlast {
        record: name.to_owned(),
        impact_proximity: number("IMPACT_PROXIMITY"),
        detonation_distance: number("DETONATION_DISTANCE"),
        detonation_time_s: number("DETONATION_TIME"),
        armor_damage: number("ARMOR_DAMAGE"),
        health_damage: number("HEALTH_DAMAGE"),
        lock_on: number("LOCK_ON"),
        range: number("RANGE"),
        range_minimum: number("RANGE_MINIMUM"),
        range_minimum_blast_scale: second.get("RANGE_MINIMUM").copied(),
        keys,
    })
}

// ------------------------------------------------ the full weapon census (T452.01) ----

/// One retail `wep_` record of `weapons.zrd`, enumerated whole (task T452.01).
///
/// [`read_original_rocket_blasts`] returns only the `ROCKET` records and only
/// the blast fields; this record is the same member walked without the
/// filter: every gun, rocket, torpedo, flare and effect record, its complete
/// top-level key set, and the first (and second) declared value of every key
/// — the values the family rules read (`ROCKET`, `LOCK_ON`, `TORPEDO`,
/// `CLUSTER_SIZE`, `EXPIRES`, `DETONATE_AT_RANGE`, `IMPACT_PROXIMITY`,
/// `DETONATION_*`, `RANGE*`, `TANGLER`, `FLASH`, `SONIC`, `BEEPER*`,
/// `SMOKE_SCREEN` and the damage channels) plus everything else, so a key no
/// rule knows is visible rather than silently dropped.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalWeaponRecord {
    /// The record name (`wep_04`).
    pub record: String,
    /// Every top-level key the record declares, sorted.
    pub keys: BTreeSet<String>,
    /// The first scalar of each key whose declared list opens with one.
    pub first_values: BTreeMap<String, f64>,
    /// The second scalar of each key whose declared list holds one (the
    /// `RANGE_MINIMUM` blast scale is the known second value).
    pub second_values: BTreeMap<String, f64>,
    /// The first text of each key whose declared list opens with text
    /// (`NAME`, `DESC`, ...).
    pub first_texts: BTreeMap<String, String>,
}

impl OriginalWeaponRecord {
    /// The record's first declared value of `key`, when it is a scalar.
    #[must_use]
    pub fn first(&self, key: &str) -> Option<f64> {
        self.first_values.get(key).copied()
    }

    /// Whether the record declares `key` at its top level.
    #[must_use]
    pub fn declares(&self, key: &str) -> bool {
        self.keys.contains(key)
    }
}

/// What a record's own keys state about its behavior family (T452.01).
///
/// `Classified` names the [`DeclaredOrdnanceFamily`] the stated rule assigns
/// and the rule id that assigned it (citable in findings).
/// `cs_sim::weapons::ordnance::OrdnanceFamily` carries the same six variants
/// in the same order — `cs_content` may not link `cs_sim`, so the
/// classification lands on the declared counterpart and the
/// `accept_t452_01_` tests pin the two enums variant-for-variant on the
/// test-only edge where both crates are visible.
///
/// `Limitation` is the recorded unknown for a record no stated rule
/// classifies: the claim id `t452_01.limit.<record>` and the reason, so an
/// unclassified record can never silently pass as classified (F28 non-
/// negotiable 1 — nothing is substituted, nothing is defaulted).
#[derive(Clone, Debug, PartialEq)]
pub enum OriginalWeaponClass {
    /// The record's keys state this family, by the named rule.
    Classified {
        /// The family the keys state.
        family: DeclaredOrdnanceFamily,
        /// The stated rule that assigned it (`t452_01.rule.*`).
        rule: &'static str,
    },
    /// No stated rule classifies the record; recorded as a limitation.
    Limitation {
        /// `t452_01.limit.<record>`.
        claim: String,
        /// Why no stated rule applies.
        reason: String,
    },
}

impl OriginalWeaponClass {
    /// The family, when the record classified.
    #[must_use]
    pub fn family(&self) -> Option<DeclaredOrdnanceFamily> {
        match self {
            Self::Classified { family, .. } => Some(*family),
            Self::Limitation { .. } => None,
        }
    }
}

/// One census finding that is not a per-record classification
/// (T452.01): the comparison the task asks for — the `ROCKET`-flagged
/// record count against the measured selectable type count
/// [`ORIGINAL_ROCKET_ORDNANCE_TYPES`] (eleven, from the two selection
/// screens; the record count is what `weapons.zrd` actually carries).
#[derive(Clone, Debug, PartialEq)]
pub enum OriginalWeaponFinding {
    /// The `ROCKET`-flagged record count differs from the selectable type
    /// count the screens declare. Recorded, not reconciled: the screens
    /// enumerate the *selectable* types and the member carries every record
    /// the engine loads, so a difference is a fact about the two sources,
    /// not an error in either.
    RocketTypeCountMismatch {
        /// `ROCKET`-flagged records in the member.
        rocket_records: usize,
        /// The measured selectable type count.
        screen_types: u32,
    },
}

impl fmt::Display for OriginalWeaponFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RocketTypeCountMismatch {
                rocket_records,
                screen_types,
            } => write!(
                f,
                "rocket_type_count_mismatch: {rocket_records} ROCKET records vs \
                 {screen_types} selectable types"
            ),
        }
    }
}

/// One census row: a record and what its own keys say about its family.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalWeaponRow {
    /// The enumerated record.
    pub record: OriginalWeaponRecord,
    /// Its classification.
    pub class: OriginalWeaponClass,
}

/// The full `weapons.zrd` census: every `wep_` record, its classification
/// and the census-level findings (T452.01).
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalWeaponCensus {
    /// Every `wep_` record of the member, in member order.
    pub records: Vec<OriginalWeaponRecord>,
    /// One row per record: the family its keys state or the limitation.
    pub rows: Vec<OriginalWeaponRow>,
    /// `ROCKET`-flagged record count.
    pub rocket_records: usize,
    /// Census-level findings (`rocket_type_count_mismatch` when the counts
    /// disagree).
    pub findings: Vec<OriginalWeaponFinding>,
}

impl OriginalWeaponCensus {
    /// The records that classified to `family`.
    #[must_use]
    pub fn records_of(&self, family: DeclaredOrdnanceFamily) -> Vec<&OriginalWeaponRecord> {
        self.rows
            .iter()
            .filter(|row| row.class.family() == Some(family))
            .map(|row| &row.record)
            .collect()
    }

    /// The rows recorded as limitations (a record no stated rule
    /// classifies).
    #[must_use]
    pub fn limitations(&self) -> Vec<&OriginalWeaponRow> {
        self.rows
            .iter()
            .filter(|row| matches!(row.class, OriginalWeaponClass::Limitation { .. }))
            .collect()
    }

    /// One record by name.
    #[must_use]
    pub fn record(&self, name: &str) -> Option<&OriginalWeaponRecord> {
        self.records.iter().find(|record| record.record == name)
    }
}

/// The flag keys that mark an effect record rather than a damaging hit —
/// the "declared and are not damage" shapes of the F28-C.2 audit
/// (`docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`).
const ORIGINAL_EFFECT_KEYS: [&str; 6] = [
    "TANGLER",
    "SMOKE_SCREEN",
    "FLASH",
    "SONIC",
    "BEEPER",
    "BEEPER_SEEKER",
];

/// The damage channels a record can declare (`ARMOR_DAMAGE`/`HEALTH_DAMAGE`
/// pair, or the single `DAMAGE` of the flash records — the same census the
/// F28-C.2 audit made).
pub const ORIGINAL_DAMAGE_KEYS: [&str; 3] = ["ARMOR_DAMAGE", "HEALTH_DAMAGE", "DAMAGE"];

/// The stated rules of T452.01, applied in order — the first rule whose
/// conditions the record's keys satisfy assigns the family. A record no
/// rule reaches is a limitation; nothing is guessed.
///
/// | rule | conditions (keys state) | family |
/// | --- | --- | --- |
/// | `torpedo` | `TORPEDO` | `aerial_torpedo` |
/// | `guided` | `LOCK_ON` | `guided_rocket` |
/// | `effect` | an [`ORIGINAL_EFFECT_KEYS`] flag | `area_denial_engine` |
/// | `airburst` | `IMPACT_PROXIMITY` or `DETONATION_DISTANCE`, unguided | `proximity_flak` |
/// | `direct` | `ROCKET` | `direct_explosive` |
/// | — | anything else | `t452_01.limit.<record>` |
fn classify_weapon_record(record: &OriginalWeaponRecord) -> OriginalWeaponClass {
    let classified = |family, rule| OriginalWeaponClass::Classified { family, rule };
    if record.declares("TORPEDO") {
        return classified(
            DeclaredOrdnanceFamily::AerialTorpedo,
            "t452_01.rule.torpedo",
        );
    }
    if record.declares("LOCK_ON") {
        return classified(DeclaredOrdnanceFamily::GuidedRocket, "t452_01.rule.guided");
    }
    if ORIGINAL_EFFECT_KEYS
        .iter()
        .any(|flag| record.declares(flag))
    {
        return classified(
            DeclaredOrdnanceFamily::AreaDenialEngine,
            "t452_01.rule.effect",
        );
    }
    if record.declares("IMPACT_PROXIMITY") || record.declares("DETONATION_DISTANCE") {
        return classified(
            DeclaredOrdnanceFamily::ProximityFlak,
            "t452_01.rule.airburst",
        );
    }
    if record.declares(ORIGINAL_ROCKET_FLAG) {
        return classified(
            DeclaredOrdnanceFamily::DirectExplosive,
            "t452_01.rule.direct",
        );
    }
    OriginalWeaponClass::Limitation {
        claim: format!("t452_01.limit.{}", record.record),
        reason: if record.declares("CANNON") {
            "a machine-gun record (CANNON): the gun vocabulary is F27's \
             domain, not one of the ordnance families; the record is \
             enumerated whole but is not lowered as ordnance here"
                .to_owned()
        } else {
            "not an ordnance record shape: no ROCKET, TORPEDO, LOCK_ON, \
             effect flag or proximity key, so no stated rule applies"
                .to_owned()
        },
    }
}

/// Classifies already-read records by the stated rules — the pure half of
/// the census, callable on records that need no installation.
#[must_use]
pub fn classify_original_weapons(records: Vec<OriginalWeaponRecord>) -> OriginalWeaponCensus {
    let rocket_records = records
        .iter()
        .filter(|record| record.declares(ORIGINAL_ROCKET_FLAG))
        .count();
    let rows = records
        .iter()
        .map(|record| OriginalWeaponRow {
            record: record.clone(),
            class: classify_weapon_record(record),
        })
        .collect();
    let mut findings = Vec::new();
    if rocket_records as u32 != ORIGINAL_ROCKET_ORDNANCE_TYPES {
        findings.push(OriginalWeaponFinding::RocketTypeCountMismatch {
            rocket_records,
            screen_types: ORIGINAL_ROCKET_ORDNANCE_TYPES,
        });
    }
    OriginalWeaponCensus {
        records,
        rows,
        rocket_records,
        findings,
    }
}

/// One record's declarations, whole.
fn weapon_record(
    name: &str,
    body: &[crate::original_airframe::SpannedValue],
) -> OriginalWeaponRecord {
    let mut keys = BTreeSet::new();
    let mut first_values = BTreeMap::new();
    let mut second_values = BTreeMap::new();
    let mut first_texts = BTreeMap::new();
    for field in body.chunks(2) {
        let [key, value] = field else { continue };
        let Some(key) = key.as_text() else { continue };
        keys.insert(key.to_owned());
        let Some(list) = value.as_list() else {
            continue;
        };
        if let Some(number) = list
            .first()
            .and_then(crate::original_airframe::SpannedValue::as_scalar_f64)
        {
            first_values.insert(key.to_owned(), number);
        }
        if let Some(text) = list
            .first()
            .and_then(crate::original_airframe::SpannedValue::as_scalar_text)
        {
            first_texts.insert(key.to_owned(), text.to_owned());
        }
        if let Some(number) = list
            .get(1)
            .and_then(crate::original_airframe::SpannedValue::as_scalar_f64)
        {
            second_values.insert(key.to_owned(), number);
        }
    }
    OriginalWeaponRecord {
        record: name.to_owned(),
        keys,
        first_values,
        second_values,
        first_texts,
    }
}

/// Reads every `wep_` record of the installation's `weapons.zrd` — guns,
/// rockets, torpedo, flare and effect records alike (T452.01).
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] for an undiscoverable
/// installation, an unreadable archive, a missing or undecodable member or a
/// member carrying no `wep_` record.
pub fn read_original_weapon_records(
    install_root: &std::path::Path,
) -> Result<Vec<OriginalWeaponRecord>, crate::original_airframe::OriginalImportError> {
    let document = read_member_document(install_root, ORIGINAL_WEAPONS_MEMBER)?;
    let mut found = Vec::new();
    collect_wep_records(document.root(), &mut found);
    if found.is_empty() {
        return Err(crate::original_airframe::OriginalImportError::Shape {
            detail: "no `wep_` record was found".to_owned(),
        });
    }
    Ok(found
        .into_iter()
        .map(|(name, body)| weapon_record(name, body))
        .collect())
}

/// The full census: every `wep_` record of `weapons.zrd`, read and
/// classified (T452.01).
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] as
/// [`read_original_weapon_records`].
pub fn read_original_weapon_census(
    install_root: &std::path::Path,
) -> Result<OriginalWeaponCensus, crate::original_airframe::OriginalImportError> {
    Ok(classify_original_weapons(read_original_weapon_records(
        install_root,
    )?))
}

// ------------------- the rocket type -> weapon record table (T452-R.03-A) ----

/// The virtual address of the engine function that turns a plane-record rocket
/// id into a `weapons.zrd` record index (range check, then a jump through
/// [`ORIGINAL_ROCKET_ID_JUMP_TABLE_VA`]).
///
/// **Measured, `static`** (never `verified_original`): the loadout binder
/// calls it for each `record + 0xa8 + 4 * slot` rocket id.
pub const ORIGINAL_ROCKET_ID_FUNCTION_VA: u64 = 0x44_40f0;

/// The virtual address of the dword jump table [`ORIGINAL_ROCKET_ID_FUNCTION_VA`]
/// indexes; each of its eleven targets is `mov eax, N; ret`.
pub const ORIGINAL_ROCKET_ID_JUMP_TABLE_VA: u64 = 0x44_4148;

/// The `weapons.zrd` record index each rocket id `0..=10` resolves to
/// (`wep_05`..`wep_15`, in this non-monotonic order).
///
/// **Measured, `static`**, at [`ORIGINAL_ROCKET_ID_FUNCTION_VA`]: the `N` of
/// each jump target. Any other id, the "none" id 11 included, answers -1 and
/// selects no record.
pub const ORIGINAL_ROCKET_ID_WEAPON_INDEX: [u32; 11] = [5, 6, 7, 8, 9, 15, 13, 12, 10, 11, 14];

/// The `weapons.zrd` record name rocket `id` resolves to (`wep_05`), or `None`
/// for an id the engine maps to no record (11 and above).
#[must_use]
pub fn original_rocket_weapon_record(id: u32) -> Option<String> {
    let index = ORIGINAL_ROCKET_ID_WEAPON_INDEX.get(usize::try_from(id).ok()?)?;
    Some(format!("wep_{index:02}"))
}

/// One selectable rocket type: its id, the record it resolves to and what that
/// record declares as its short `NAME`.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalRocketTypeRow {
    /// The plane-record rocket id (`0..=10`).
    pub rocket_id: u32,
    /// The record index the engine maps the id to.
    pub weapon_index: u32,
    /// The record name (`wep_05`).
    pub record: String,
    /// The record's first declared `NAME` text, when it declares one.
    pub short_name: Option<String>,
}

/// The selectable rocket types, resolved through `weapons.zrd`, and the
/// catalogued records no rocket id reaches (T452-R.03-A).
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalRocketTypeTable {
    /// One row per rocket id, in id order.
    pub rows: Vec<OriginalRocketTypeRow>,
    /// Ids whose mapped record the member does not carry (empty for retail).
    pub missing_records: Vec<String>,
    /// The classified (T452.01) records no rocket id maps to, sorted by name:
    /// spawn-only as far as the player's selection is concerned.
    pub spawn_only: Vec<String>,
}

/// Resolves [`ORIGINAL_ROCKET_ID_WEAPON_INDEX`] against a read census.
#[must_use]
pub fn original_rocket_type_table(census: &OriginalWeaponCensus) -> OriginalRocketTypeTable {
    let mut rows = Vec::new();
    let mut missing_records = Vec::new();
    for (id, index) in (0_u32..).zip(ORIGINAL_ROCKET_ID_WEAPON_INDEX) {
        let record = format!("wep_{index:02}");
        match census.record(&record) {
            Some(found) => rows.push(OriginalRocketTypeRow {
                rocket_id: id,
                weapon_index: index,
                short_name: found.first_texts.get("NAME").cloned(),
                record,
            }),
            None => missing_records.push(record),
        }
    }
    let mut spawn_only: Vec<String> = census
        .rows
        .iter()
        .filter(|row| row.class.family().is_some())
        .map(|row| row.record.record.clone())
        .filter(|name| {
            !rows.iter().any(|row| &row.record == name) && !missing_records.contains(name)
        })
        .collect();
    spawn_only.sort();
    OriginalRocketTypeTable {
        rows,
        missing_records,
        spawn_only,
    }
}

/// Reads the installation's `weapons.zrd` and resolves the rocket type table.
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] as
/// [`read_original_weapon_records`].
pub fn read_original_rocket_type_table(
    install_root: &std::path::Path,
) -> Result<OriginalRocketTypeTable, crate::original_airframe::OriginalImportError> {
    Ok(original_rocket_type_table(&read_original_weapon_census(
        install_root,
    )?))
}

// ------------------------------------------- AMMO_LIMIT and the slot count ----

/// The virtual address of the `weapons.zrd` record parser (`zwep_init.c`
/// loader) whose `AMMO_LIMIT` read is [`ORIGINAL_AMMO_LIMIT_READ_VA`].
///
/// **Measured, `static`** (task T452-R.03-B, never `verified_original`).
pub const ORIGINAL_WEAPON_RECORD_PARSER_VA: u64 = 0x5a_d630;

/// The call that reads `AMMO_LIMIT` (`0x57a130`, number read as a float) into
/// the record's own float at `record + 0x18`.
pub const ORIGINAL_AMMO_LIMIT_READ_VA: u64 = 0x5a_d925;

/// The store that defaults `record + 0x18` to [`ORIGINAL_AMMO_LIMIT_DEFAULT`]
/// before any key is read, so a record without `AMMO_LIMIT` keeps that value.
pub const ORIGINAL_AMMO_LIMIT_DEFAULT_VA: u64 = 0x5a_d805;

/// The default of the `AMMO_LIMIT` field (`0x42480000` as an `f32`).
///
/// **Measured, `static`**. No reader of the field was found (see
/// [`ORIGINAL_AMMO_LIMIT_CONSUMER_NOTE`]), so what the default means for play is
/// unknown.
pub const ORIGINAL_AMMO_LIMIT_DEFAULT: f32 = 50.0;

/// The byte offset of the parsed `AMMO_LIMIT` float inside a `weapons.zrd`
/// record (stride `0x214`, list at `0xa1d78c`).
pub const ORIGINAL_AMMO_LIMIT_RECORD_OFFSET: u32 = 0x18;

/// The per-type callback the record parser invokes (`0x5ae0f7`); it allocates
/// the 0x38-byte object stored at `record + 0x210`.
pub const ORIGINAL_WEAPON_TYPE_CALLBACK_VA: u64 = 0x4b_a6f0;

/// The store of `CLUSTER_SIZE` at `(record + 0x210) + 0xc`, the dword the slot
/// installer takes as a slot's count (`0x4bad90`). The object is zero-filled
/// on allocation, so a record without `CLUSTER_SIZE` has count 0.
///
/// **Measured, `static`**: this, not `AMMO_LIMIT`, fills the stack count.
pub const ORIGINAL_SLOT_COUNT_STORE_VA: u64 = 0x4b_aa17;

/// What the static search for a reader of the `AMMO_LIMIT` float established.
pub const ORIGINAL_AMMO_LIMIT_CONSUMER_NOTE: &str = "no instruction reading record+0x18 as AMMO_LIMIT was found: the zweapon module (0x5ac000..0x5af000) only writes it, and the callers of the record lookups 0x5abfd0 and 0x5ac050 read other structures at +0x18; the slot count (per-slot stack, applied to every slot holding the record) is CLUSTER_SIZE";

/// What one `weapons.zrd` record declares about its ammunition, and what the
/// engine's two parsers make of it.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalAmmoDeclaration {
    /// The record name (`wep_04`).
    pub record: String,
    /// The declared `AMMO_LIMIT`, or `None` (the parser field then keeps
    /// [`ORIGINAL_AMMO_LIMIT_DEFAULT`]).
    pub ammo_limit: Option<f64>,
    /// The declared `CLUSTER_SIZE`, or `None`.
    pub cluster_size: Option<f64>,
}

impl OriginalAmmoDeclaration {
    /// The stack count the slot installer reads for this record: the declared
    /// `CLUSTER_SIZE`, and 0 when it is absent (the zero-filled object).
    #[must_use]
    pub fn slot_count(&self) -> u32 {
        self.cluster_size.map_or(0, whole)
    }

    /// The value of the parsed `AMMO_LIMIT` field: the declaration, else the
    /// parser default.
    #[must_use]
    pub fn parsed_ammo_limit(&self) -> f64 {
        self.ammo_limit
            .unwrap_or_else(|| f64::from(ORIGINAL_AMMO_LIMIT_DEFAULT))
    }
}

/// The ammunition declaration of every record in the census, in member order.
#[must_use]
pub fn original_ammo_declarations(census: &OriginalWeaponCensus) -> Vec<OriginalAmmoDeclaration> {
    census
        .records
        .iter()
        .map(|record| OriginalAmmoDeclaration {
            record: record.record.clone(),
            ammo_limit: record.first("AMMO_LIMIT"),
            cluster_size: record.first("CLUSTER_SIZE"),
        })
        .collect()
}

/// Reads the installation's `weapons.zrd` and returns every record's ammunition
/// declaration.
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] as
/// [`read_original_weapon_records`].
pub fn read_original_ammo_declarations(
    install_root: &std::path::Path,
) -> Result<Vec<OriginalAmmoDeclaration>, crate::original_airframe::OriginalImportError> {
    Ok(original_ammo_declarations(&read_original_weapon_census(
        install_root,
    )?))
}

// ------------------- the equipment-rule census (T452-R.03-D) ----------------

/// Fragments of a key name that would state a requirement or permission on
/// another piece of equipment. A catalogued record that declares a key
/// containing one of these would carry an equipment rule the importer must read;
/// the census reports every such key rather than assuming none exists.
pub const ORIGINAL_EQUIPMENT_RULE_KEY_FRAGMENTS: [&str; 8] = [
    "REQUIRE", "PERMIT", "ALLOW", "FORBID", "PROHIBIT", "EXCLUDE", "NEED", "EQUIP",
];

/// What the catalogued `weapons.zrd` records declare about equipment rules.
///
/// **Measured, `static`** (a read of the installation's data, not a run of the
/// original): the union of top-level keys over the records, and which of them
/// state a requirement or permission.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalEquipmentRuleCensus {
    /// The records the census covers, in table order.
    pub records: Vec<String>,
    /// The union of every top-level key those records declare.
    pub keys: BTreeSet<String>,
    /// `(record, key)` for every declared key that names a requirement or
    /// permission ([`ORIGINAL_EQUIPMENT_RULE_KEY_FRAGMENTS`]).
    pub rule_keys: Vec<(String, String)>,
}

impl OriginalEquipmentRuleCensus {
    /// Whether any covered record states an equipment rule; when it is false
    /// the empty rule set is the measured value for every one of them.
    #[must_use]
    pub fn declares_rules(&self) -> bool {
        !self.rule_keys.is_empty()
    }
}

/// Census of the equipment-rule keys over the named records of `census`.
///
/// A name the census does not hold is skipped from `records` (and so shows as a
/// short record count, which the caller pins).
#[must_use]
pub fn original_equipment_rule_census(
    census: &OriginalWeaponCensus,
    records: &[String],
) -> OriginalEquipmentRuleCensus {
    let mut covered = Vec::new();
    let mut keys = BTreeSet::new();
    let mut rule_keys = Vec::new();
    for name in records {
        let Some(record) = census.records.iter().find(|record| &record.record == name) else {
            continue;
        };
        covered.push(record.record.clone());
        for key in &record.keys {
            keys.insert(key.clone());
            if ORIGINAL_EQUIPMENT_RULE_KEY_FRAGMENTS
                .iter()
                .any(|fragment| key.contains(fragment))
            {
                rule_keys.push((record.record.clone(), key.clone()));
            }
        }
    }
    OriginalEquipmentRuleCensus {
        records: covered,
        keys,
        rule_keys,
    }
}

/// Reads the installation and counts the equipment-rule keys over the records
/// of the component table (the catalogued records).
///
/// # Errors
///
/// [`OriginalComponentError`] when the records or a screen cannot be read.
pub fn read_original_equipment_rule_census(
    install_root: &std::path::Path,
) -> Result<OriginalEquipmentRuleCensus, OriginalComponentError> {
    let census =
        read_original_weapon_census(install_root).map_err(OriginalComponentError::Records)?;
    let layout = read_original_mount_layout(install_root)?;
    let table = original_component_table(&census, &layout);
    let records: Vec<String> = table.rows.iter().map(|row| row.record.clone()).collect();
    Ok(original_equipment_rule_census(&census, &records))
}

// ------------- the hardpoint box price and the gate table (T452-R.03) --------

/// The handler the purchase screen's hardpoint rows dispatch to (callbacks
/// `E 2259..2261`): `-1` counts the non-zero box counts, a row formats the
/// side's count through string `1176`/`1177`, and the weight/cost row computes
/// [`ORIGINAL_HARDPOINT_WEIGHT_PER_BOX`] / [`ORIGINAL_HARDPOINT_COST_PER_BOX`]
/// times the count.
///
/// **Measured, `static`** (never `verified_original`).
pub const ORIGINAL_HARDPOINT_ROW_CALLBACK_VA: u64 = 0x40_b2b4;

/// The instruction window inside [`ORIGINAL_HARDPOINT_ROW_CALLBACK_VA`] that
/// computes `price * count`: it picks the side's box count (`0x64cbac` or
/// `0x64cbb0`), then `and edx, 0xffffffba; add edx, 0x1e0; imul edx, ecx` —
/// `480` for `E 2260` (weight) and `480 - 70 = 410` for `E 2261` (cost).
pub const ORIGINAL_HARDPOINT_PRICE_VA: u64 = 0x40_b2ff;

/// The purchase-weight totalizer: it adds `480 * (record + 0x34 + record +
/// 0x38)` — the composite `(count sum) * 15 * 8 * 4` — to the airframe, engine,
/// gun and armor terms. It does not read `record + 0xa8` (the rocket slots).
pub const ORIGINAL_PLANE_WEIGHT_TOTAL_VA: u64 = 0x40_5550;

/// The purchase-cost totalizer: it adds `410 * (record + 0x34 + record +
/// 0x38)` — the composite `(count sum) * 41 * 5 * 2` — to the same terms. It
/// does not read `record + 0xa8` either.
pub const ORIGINAL_PLANE_COST_TOTAL_VA: u64 = 0x40_5680;

/// The hardpoint point get/set handler (`E 2245`): get reads the draft
/// record's box count (`0x64cbac` point 0, `0x64cbb0` point 1 — the draft
/// record at `0x64cb78` plus the same offsets the committed record uses,
/// [`ORIGINAL_HARDPOINT_BOX_COUNT_OFFSETS`]); set writes the count and fills
/// the side's four draft rocket slots (`0x64cc20 + 16 * point`, the draft
/// record's `+0xa8` array) with a random `0`/`1` (the two starting rocket ids)
/// or `11` (the none id) when the count is 0.
pub const ORIGINAL_HARDPOINT_POINT_CALLBACK_VA: u64 = 0x40_ad0f;

/// The Instant Action rocket-name handler (`E 2031`): it reports a slot's
/// rocket id through `0x40ffa0`, and answers `-1` when the slot index is not
/// below the box count — `record + 0x34` for slots 0..4, `record + 0x38` for
/// slots 4..8 — proving the two record fields are per-side counts of enabled
/// rocket boxes.
pub const ORIGINAL_ROCKET_SLOT_NAME_CALLBACK_VA: u64 = 0x40_9aec;

/// The record offsets (of the 204-byte plane record) holding the enabled
/// rocket-box counts of the two hardpoint sides (`DT[2]`).
///
/// **Measured, `static`**: read as counts by
/// [`ORIGINAL_ROCKET_SLOT_NAME_CALLBACK_VA`] and priced by
/// [`ORIGINAL_PLANE_WEIGHT_TOTAL_VA`] / [`ORIGINAL_PLANE_COST_TOTAL_VA`].
pub const ORIGINAL_HARDPOINT_BOX_COUNT_OFFSETS: [u32; 2] = [0x34, 0x38];

/// The weight the purchase surface charges per enabled rocket box, in the
/// engine's weight unit (the same unit the airframe, engine, gun and armor
/// rows price; its conversion to kilograms is not measured here).
///
/// **Measured, `static`**: [`ORIGINAL_HARDPOINT_PRICE_VA`] multiplies the
/// side's box count by this for `E 2260`, and
/// [`ORIGINAL_PLANE_WEIGHT_TOTAL_VA`] adds it per counted box. The loaded
/// rocket type contributes nothing — no purchase path reads a type-dependent
/// value.
pub const ORIGINAL_HARDPOINT_WEIGHT_PER_BOX: u32 = 480;

/// The cost the purchase surface charges per enabled rocket box.
///
/// **Measured, `static`**: the same instruction selects `410` for `E 2261`,
/// and [`ORIGINAL_PLANE_COST_TOTAL_VA`] adds it per counted box.
pub const ORIGINAL_HARDPOINT_COST_PER_BOX: u32 = 410;

/// The Instant Action rocket availability table: twelve 8-byte records the
/// E 2028 list enumerates through `0x40ff00` (count), `0x40ff50` (nth passing
/// row's index) and `0x40ffa0` (a record index's row). Each record's first
/// dword gates it: a row passes when any of the flag cells `0x647b5c`,
/// `0x647b68`, `0x647b6c` is set, or when `first <= progress + 1` against the
/// progress cell `0x64b678`.
///
/// **Measured, `static`** (T452-R.03; the row values were first pinned by
/// F2066-B's `[ordinance_feed]` facts).
pub const ORIGINAL_ROCKET_GATE_TABLE_VA: u64 = 0x61_9efc;

/// The availability thresholds of the twelve gate-table rows (the first
/// dword of each). Row `n` is the rocket id `n`: the same ids
/// [`ORIGINAL_ROCKET_ID_WEAPON_INDEX`] maps (0..10 selectable, 11 the none
/// entry the Instant Action list shows last).
pub const ORIGINAL_ROCKET_GATE_THRESHOLDS: [u32; 12] = [1, 1, 2, 8, 7, 12, 7, 7, 17, 17, 20, 1];

/// The second dwords of the gate-table rows (`4, 4, 2, 2, 2, 2, 2, 4, 4, 1, 0`
/// for rows 0..10): **measured to be unread** — every code reference to the
/// table is one of the three accessors, and each compares `[ecx]` only. Row
/// 11's second dword is not table data at all: `0x619f58` is the base of the
/// 204-byte plane-record table (`lea esi, [ecx * 4 + 0x619f58]` copies
/// `0x33` dwords, e.g. at `0x40ab16`). What the column means is unknown;
/// nothing in the image reads it.
pub const ORIGINAL_ROCKET_GATE_AUX: [u32; 11] = [4, 4, 2, 2, 2, 2, 2, 4, 4, 1, 0];

/// The base of the static plane-record table (204-byte records, the source
/// the live `0x64b78c` table mirrors). It overlaps what would be the last
/// gate-table row's second dword.
pub const ORIGINAL_PLANE_RECORD_TABLE_VA: u64 = 0x61_9f58;

/// The byte stride of a plane record.
pub const ORIGINAL_PLANE_RECORD_STRIDE: u32 = 204;

/// What the purchase surface charges for hardpoints — the measured answer to
/// "the hardpoint weight and cost".
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalHardpointPrice {
    /// Weight units charged per enabled rocket box
    /// ([`ORIGINAL_HARDPOINT_WEIGHT_PER_BOX`]).
    pub weight_per_box: u32,
    /// Cost charged per enabled rocket box
    /// ([`ORIGINAL_HARDPOINT_COST_PER_BOX`]).
    pub cost_per_box: u32,
    /// The record fields holding the two box counts
    /// ([`ORIGINAL_HARDPOINT_BOX_COUNT_OFFSETS`]).
    pub box_count_offsets: [u32; 2],
    /// How many hardpoint sides the record carries (`DT[2]`).
    pub sides: u32,
    /// Rocket boxes each side holds at most (the `RKA[8]` slots split
    /// 4 + 4).
    pub boxes_per_side: u32,
}

/// The measured hardpoint price: the purchase surface charges
/// [`ORIGINAL_HARDPOINT_WEIGHT_PER_BOX`] weight and
/// [`ORIGINAL_HARDPOINT_COST_PER_BOX`] cost for every enabled rocket box, and
/// the rocket type loaded into a box changes neither — no instruction in the
/// purchase rows or the totals reads the rocket slots (`record + 0xa8`) or
/// any per-type field. There is no per-type hardpoint weight or cost table in
/// the image to pin; the measured per-row values are the flat per-box price
/// and this absence.
#[must_use]
pub const fn original_hardpoint_price() -> OriginalHardpointPrice {
    OriginalHardpointPrice {
        weight_per_box: ORIGINAL_HARDPOINT_WEIGHT_PER_BOX,
        cost_per_box: ORIGINAL_HARDPOINT_COST_PER_BOX,
        box_count_offsets: ORIGINAL_HARDPOINT_BOX_COUNT_OFFSETS,
        sides: 2,
        boxes_per_side: 4,
    }
}

impl OriginalHardpointPrice {
    /// The weight `box_count` enabled boxes add on one side.
    #[must_use]
    pub const fn weight(&self, box_count: u32) -> u32 {
        self.weight_per_box * box_count
    }

    /// The cost `box_count` enabled boxes add on one side.
    #[must_use]
    pub const fn cost(&self, box_count: u32) -> u32 {
        self.cost_per_box * box_count
    }
}

/// The composite factors the two totalizers multiply the summed box counts by:
/// `15 * 8 * 4` in [`ORIGINAL_PLANE_WEIGHT_TOTAL_VA`] (`0x40558e`) and
/// `41 * 5 * 2` in [`ORIGINAL_PLANE_COST_TOTAL_VA`] (`0x4056aa`..`0x4056ef`).
/// They are the instructions' own constants, and their products are
/// [`ORIGINAL_HARDPOINT_WEIGHT_PER_BOX`] and [`ORIGINAL_HARDPOINT_COST_PER_BOX`].
///
/// **Measured, `static`.**
pub const ORIGINAL_PLANE_TOTAL_BOX_FACTORS: ([u32; 3], [u32; 3]) = ([15, 8, 4], [41, 5, 2]);

/// The claim that the weight unit's conversion to kilograms is **not
/// measured**: [`OriginalWeightUnits`] deliberately has no kilogram accessor,
/// and nothing may present its value as kilograms until a task measures the
/// conversion.
pub const ORIGINAL_WEIGHT_UNIT_KG_CLAIM: &str = "t452_register.limit.weight_unit_kg";

/// A weight in the engine's own purchase unit. Its conversion to kilograms is
/// unmeasured ([`ORIGINAL_WEIGHT_UNIT_KG_CLAIM`]), so the type offers no
/// kilogram view.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct OriginalWeightUnits(pub u32);

/// The two enabled-rocket-box counts of an airframe record (`record + 0x34`
/// and `record + 0x38`, [`ORIGINAL_HARDPOINT_BOX_COUNT_OFFSETS`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OriginalHardpointBoxes {
    counts: [u32; 2],
}

/// A box count above what one hardpoint side holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OriginalBoxCountError {
    /// The side (0 = `+0x34`, 1 = `+0x38`).
    pub side: usize,
    /// The refused count.
    pub count: u32,
}

impl OriginalHardpointBoxes {
    /// Assembles the two counts, refusing one above the four boxes a side holds
    /// (`RKA[8]` splits 4 + 4).
    ///
    /// # Errors
    ///
    /// [`OriginalBoxCountError`] naming the side.
    pub fn try_new(counts: [u32; 2]) -> Result<Self, OriginalBoxCountError> {
        let max = original_hardpoint_price().boxes_per_side;
        for (side, &count) in counts.iter().enumerate() {
            if count > max {
                return Err(OriginalBoxCountError { side, count });
            }
        }
        Ok(Self { counts })
    }

    /// The enabled boxes over both sides.
    #[must_use]
    pub const fn total_boxes(&self) -> u32 {
        self.counts[0] + self.counts[1]
    }

    /// The hardpoint term of the airframe weight total
    /// (`0x40558e`): the summed counts times `15 * 8 * 4`. The loaded rocket
    /// type contributes nothing. In the engine's own unit, not kilograms.
    #[must_use]
    pub const fn weight(&self) -> OriginalWeightUnits {
        let [a, b, c] = ORIGINAL_PLANE_TOTAL_BOX_FACTORS.0;
        OriginalWeightUnits(self.total_boxes() * a * b * c)
    }

    /// The hardpoint term of the airframe cost total (`0x4056aa`): the summed
    /// counts times `41 * 5 * 2`.
    #[must_use]
    pub const fn cost(&self) -> u32 {
        let [a, b, c] = ORIGINAL_PLANE_TOTAL_BOX_FACTORS.1;
        self.total_boxes() * a * b * c
    }
}

// ------------------------------- the per-component launch table (T452.04) ----

/// The retail container that holds the two ordnance screens (task T452.04).
pub const ORIGINAL_SCREENS_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The screen that lays out an airframe's hardpoint points.
pub const ORIGINAL_HARDPOINTS_SCREEN: &str = "ASSETS/SCRIPTS/HARDPOINTS.SCRIPT";

/// The screen that lays out an airframe's gun/ammunition and rocket slots.
pub const ORIGINAL_ORDNANCE_LAYOUT_SCREEN: &str = "ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT";

/// The hardpoint screen's own array of points (`object DT[2]`).
pub const ORIGINAL_HARDPOINT_POINT_ARRAY: &str = "DT";

/// The ordnance screen's own array of rocket slots (`object RKA[8]`).
pub const ORIGINAL_ROCKET_SLOT_ARRAY: &str = "RKA";

/// The ordnance screen's own array of gun/ammunition slots (`object QKA[4]`).
pub const ORIGINAL_AMMUNITION_SLOT_ARRAY: &str = "QKA";

/// The claim id for "`VELOCITY`'s unit is not measured" — resolved by task
/// T452-R.01 (metres per second, static); kept so older findings and claim
/// lists stay parseable.
pub const T452_04_VELOCITY_UNIT: &str = "t452_04.limit.velocity_unit";

/// The claim id for "what `FIRE_RATE` delays, and in which direction" —
/// narrowed by T452-R.01 and resolved by T452-R.04 for every record some
/// airframe's `weapons` row mounts: the interval a mount adds to the shared
/// clock is that row's third element ([`OriginalMountRow`]), not record
/// `+0x14`. It stays on a record no airframe row names.
pub const T452_04_RELEASE_DELAY: &str = "t452_04.limit.release_delay";

/// The claim id for "no record states a projectile lifetime" — resolved by
/// T452-R.04: the step ends a flight by path length, a timer or never
/// ([`OriginalLaunchGeometry::flight_duration`]); kept so older findings and
/// the nitro row stay parseable.
pub const T452_04_NO_EXPIRES: &str = "t452_04.limit.no_expires_key";

/// The claim id for "a status effect's gameplay kind is not stated".
pub const T452_04_STATUS_KIND: &str = "t452_04.limit.status_effect_kind";

/// The claim id for "a status effect carries no duration".
pub const T452_04_STATUS_DURATION: &str = "t452_04.limit.status_effect_duration";

/// The claim id for "an effect area states no lifetime".
pub const T452_04_AREA_LIFETIME: &str = "t452_04.limit.area_lifetime";

/// The claim id for "an area-denial record states an effect but no radius".
pub const T452_04_AREA_RADIUS: &str = "t452_04.limit.area_radius";

/// The claim id for "an ordnance-shaped record declares no `ROCKET` flag, so
/// no screen states which slot holds it".
pub const T452_04_MOUNT: &str = "t452_04.limit.rocket_flag_missing";

/// The carried claim id for "what each sub-munition of a clustered launch
/// does" (T452.01's `t454.limit.swarm`).
pub const T452_04_CLUSTER: &str = "t454.limit.swarm";

/// The claim id for "which component sits in which slot on a given airframe".
pub const T452_04_PER_AIRFRAME: &str = "t452_04.limit.per_airframe_layout";

/// Why one screen could not be read or does not declare its array.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginalScreenError {
    /// The installation, container or member could not be read.
    Read {
        /// The member that was being read.
        member: String,
        /// Why it failed.
        reason: String,
    },
    /// The screen does not declare the slot array the layout is read from.
    SlotArrayMissing {
        /// The member that was read.
        member: String,
        /// The array name that was not found.
        array: String,
    },
}

impl fmt::Display for OriginalScreenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { member, reason } => {
                write!(f, "{member} could not be read: {reason}")
            }
            Self::SlotArrayMissing { member, array } => write!(
                f,
                "{member} declares no `object {array}[n]` slot array, so its layout cannot be read"
            ),
        }
    }
}

impl std::error::Error for OriginalScreenError {}

/// Why the per-component launch table could not be produced.
#[derive(Debug)]
pub enum OriginalComponentError {
    /// The [`OriginalWeaponCensus`] could not be read.
    Records(crate::original_airframe::OriginalImportError),
    /// A screen the layout is read from could not be read.
    Screen(OriginalScreenError),
}

impl fmt::Display for OriginalComponentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Records(error) => write!(f, "the weapon records could not be read: {error}"),
            Self::Screen(error) => write!(f, "the mount layout could not be read: {error}"),
        }
    }
}

impl std::error::Error for OriginalComponentError {}

impl From<OriginalScreenError> for OriginalComponentError {
    fn from(error: OriginalScreenError) -> Self {
        Self::Screen(error)
    }
}

/// One slot array one screen declares, and the loop that fills it.
///
/// **Measured** (task T452.04) from the two screens' own text: the declaration
/// (`object RKA[8]`) and the fill loop (`for (BA = 0; BA < 8; BA++)`) are two
/// independent statements of the same count, so [`Self::is_consistent`] is a
/// real check rather than a restatement. `deactivates_empty_slots` is the
/// screen's own handling of a slot the engine reports as `-1`: the ordnance
/// screen calls `deactivate(...)` on an empty slot of each of its two arrays,
/// the hardpoint screen has no `deactivate` at all and always shows both
/// points. The call is matched against *this* array's name, so a screen that
/// hid one list's empty slots and not another's would state that difference
/// rather than a single screen-wide flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalSlotArray {
    /// The member the array is declared in.
    pub member: String,
    /// The screen's own array name (`RKA`, `QKA`, `DT`).
    pub array: String,
    /// The bound the declaration states.
    pub slots: u32,
    /// The bound the loop that fills it states.
    pub loop_bound: u32,
    /// Whether the screen calls `deactivate(...)` on an empty slot of this
    /// array.
    pub deactivates_empty_slots: bool,
}

impl OriginalSlotArray {
    /// Reads one slot array out of a screen's text.
    ///
    /// Returns `None` when the screen declares no `object <array>[n]` or no
    /// following fill loop states a bound: nothing is defaulted, because a
    /// layout this project guessed is a layout no airframe was measured to
    /// have.
    #[must_use]
    pub fn read(member: &str, screen: &str, array: &str) -> Option<Self> {
        let declaration = format!("object {array}[");
        let start = screen.find(&declaration)? + declaration.len();
        let rest = &screen[start..];
        let close = rest.find(']')?;
        let slots = rest[..close].trim().parse::<u32>().ok()?;
        // The fill loop is the next `for (...)` after the declaration; its
        // header states the same bound a second time.
        let after = &rest[close..];
        let loop_start = after.find("for (")?;
        let loop_bound = loop_bound(&after[loop_start..])?;
        Some(Self {
            member: member.to_owned(),
            array: array.to_owned(),
            slots,
            loop_bound,
            deactivates_empty_slots: deactivates_empty_slot(screen, array),
        })
    }

    /// Whether the declaration and its fill loop state the same count.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.slots == self.loop_bound
    }
}

/// Whether the screen calls `deactivate` on an empty slot of `array`.
///
/// The screen's own call is `deactivate(<array>[<index>])`. The array name is
/// matched so two lists in one screen report their empty-slot handling
/// separately, and the spacing after the call is skipped because it is the
/// original build's, not this reader's, to decide.
fn deactivates_empty_slot(screen: &str, array: &str) -> bool {
    const CALL: &str = "deactivate";
    let mut rest = screen;
    while let Some(position) = rest.find(CALL) {
        let after_call = rest[position + CALL.len()..].trim_start();
        if let Some(after_open) = after_call.strip_prefix('(') {
            let after_open = after_open.trim_start();
            if let Some(after_array) = after_open.strip_prefix(array)
                && after_array.trim_start().starts_with('[')
            {
                return true;
            }
        }
        rest = &rest[position + CALL.len()..];
    }
    false
}

/// The `< n` bound of a `for (...)` header, as the header spells it.
fn loop_bound(loop_text: &str) -> Option<u32> {
    let header = &loop_text[..loop_text.find(')')?];
    let after = &header[header.find("< ")? + 2..];
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// One way the two screens disagree with themselves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginalLayoutFinding {
    /// An array's declared bound and its fill loop state different counts.
    SlotBoundDisagrees {
        /// The member the array is declared in.
        member: String,
        /// The array name.
        array: String,
        /// The bound the declaration states.
        slots: u32,
        /// The bound the fill loop states.
        loop_bound: u32,
    },
}

impl fmt::Display for OriginalLayoutFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlotBoundDisagrees {
                member,
                array,
                slots,
                loop_bound,
            } => write!(
                f,
                "{member}: `object {array}[{slots}]` but the fill loop counts {loop_bound}"
            ),
        }
    }
}

/// The mount layout the two screens declare (task T452.04).
///
/// **Measured**: eight rocket slots, four gun/ammunition slots and two
/// hardpoint points, each with the screen's array bound and its fill-loop bound
/// agreeing, and the ordnance screen deactivating an empty slot while the
/// hardpoint screen does not. The layout is per airframe in the original — the
/// screens size their controls once, and which slot a given plane fills is in
/// its plane record, not in a file (see
/// [`T452_04_PER_AIRFRAME`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalMountLayout {
    /// `ORDNANCELAYOUT.SCRIPT`'s `object RKA[8]`.
    pub rocket_slots: OriginalSlotArray,
    /// `ORDNANCELAYOUT.SCRIPT`'s `object QKA[4]`.
    pub ammunition_slots: OriginalSlotArray,
    /// `HARDPOINTS.SCRIPT`'s `object DT[2]`.
    pub hardpoint_points: OriginalSlotArray,
    /// Every disagreement the screens state about themselves.
    pub findings: Vec<OriginalLayoutFinding>,
}

impl OriginalMountLayout {
    /// Reads the layout out of the two screens' text.
    ///
    /// # Errors
    ///
    /// [`OriginalScreenError::SlotArrayMissing`] when either screen does not
    /// declare its array — a layout with a missing half is refused rather than
    /// filled in.
    pub fn from_screens(
        hardpoints: &str,
        ordnance_layout: &str,
    ) -> Result<Self, OriginalScreenError> {
        let read = |member: &'static str, screen: &str, array: &'static str| {
            OriginalSlotArray::read(member, screen, array).ok_or_else(|| {
                OriginalScreenError::SlotArrayMissing {
                    member: member.to_owned(),
                    array: array.to_owned(),
                }
            })
        };
        let hardpoint_points = read(
            ORIGINAL_HARDPOINTS_SCREEN,
            hardpoints,
            ORIGINAL_HARDPOINT_POINT_ARRAY,
        )?;
        let rocket_slots = read(
            ORIGINAL_ORDNANCE_LAYOUT_SCREEN,
            ordnance_layout,
            ORIGINAL_ROCKET_SLOT_ARRAY,
        )?;
        let ammunition_slots = read(
            ORIGINAL_ORDNANCE_LAYOUT_SCREEN,
            ordnance_layout,
            ORIGINAL_AMMUNITION_SLOT_ARRAY,
        )?;
        let mut findings = Vec::new();
        for array in [&hardpoint_points, &rocket_slots, &ammunition_slots] {
            if !array.is_consistent() {
                findings.push(OriginalLayoutFinding::SlotBoundDisagrees {
                    member: array.member.clone(),
                    array: array.array.clone(),
                    slots: array.slots,
                    loop_bound: array.loop_bound,
                });
            }
        }
        Ok(Self {
            rocket_slots,
            ammunition_slots,
            hardpoint_points,
            findings,
        })
    }

    /// Whether the screens agree with themselves.
    #[must_use]
    pub fn is_consistent(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Reads one screen out of the base retail archive's ROF container as text.
fn read_screen_member(
    install_root: &std::path::Path,
    member: &str,
) -> Result<String, OriginalScreenError> {
    use cs_assets::rof::mount_rof_into;
    use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
    use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};

    let read_error = |reason: String| OriginalScreenError::Read {
        member: member.to_owned(),
        reason,
    };
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| read_error(error.to_string()))?;
    let install = cs_assets::install::fingerprint(&found.manifest);
    let id: String = format!("rof-{}", ORIGINAL_SCREENS_CONTAINER.to_ascii_lowercase())
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '.' {
                character
            } else {
                '-'
            }
        })
        .collect();
    let mut builder = SessionBuilder::new(ResolveContext::new(install));
    let source = mount_rof_into(
        &mut builder,
        MountBuilder::new(
            MountId::new(&id).map_err(|error| read_error(error.to_string()))?,
            MountNamespace::new(INSTALL_NAMESPACE)
                .map_err(|error| read_error(error.to_string()))?,
            PrecedenceClass::Shared,
            ORIGINAL_SCREENS_CONTAINER,
        )
        .retail(),
        &install_root.join(ORIGINAL_SCREENS_CONTAINER),
    )
    .map_err(|error| OriginalScreenError::Read {
        member: ORIGINAL_SCREENS_CONTAINER.to_owned(),
        reason: format!("{error:?}"),
    })?;
    let session = builder.open();
    let key = AssetKey::from_spelling(INSTALL_NAMESPACE, member, "default")
        .map_err(|error| read_error(error.to_string()))?;
    session
        .resolve(&key)
        .map_err(|error| read_error(format!("{error:?}")))?;
    let bytes = source
        .read(&key)
        .map_err(|error| read_error(format!("{error:?}")))?
        .data;
    String::from_utf8(bytes).map_err(|_| read_error("the screen is not UTF-8 text".to_owned()))
}

/// The mount layout of the installation's two ordnance screens (T452.04).
///
/// # Errors
///
/// [`OriginalScreenError`] when the installation, the archive or a member
/// cannot be read, or a screen declares no slot array.
pub fn read_original_mount_layout(
    install_root: &std::path::Path,
) -> Result<OriginalMountLayout, OriginalScreenError> {
    let hardpoints = read_screen_member(install_root, ORIGINAL_HARDPOINTS_SCREEN)?;
    let ordnance_layout = read_screen_member(install_root, ORIGINAL_ORDNANCE_LAYOUT_SCREEN)?;
    OriginalMountLayout::from_screens(&hardpoints, &ordnance_layout)
}

/// Where a component mounts, as the screens' own slot arrays state it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginalMountDeclaration {
    /// The record declares `ROCKET`, so it is a rocket-slot component: one of
    /// the measured `slots` slots.
    RocketSlot {
        /// The slot count the ordnance screen declares.
        slots: u32,
    },
    /// An ordnance-shaped record with no `ROCKET` flag — `wep_25`, the
    /// glidebomb — for which no screen states a slot.
    Unknown {
        /// The claim id the unknown is recorded under.
        claim: String,
        /// Why no screen states a slot.
        reason: String,
    },
}

impl OriginalMountDeclaration {
    /// The slot count, or `None` when no screen states one.
    #[must_use]
    pub fn slots(&self) -> Option<u32> {
        match self {
            Self::RocketSlot { slots } => Some(*slots),
            Self::Unknown { .. } => None,
        }
    }
}

/// What a record's `CLUSTER_SIZE` states — and it is not always a volley.
///
/// **Measured** (task T452.04): on every one of the thirty-one `CANNON` records
/// `CLUSTER_SIZE` equals `AMMO_LIMIT` exactly (2800 = 2800, 1200 = 1200, ...),
/// so there it is the ammunition count; on every ordnance record the two keys
/// differ and `CLUSTER_SIZE` is a small whole number (1, 2, 3, 4, 6 or 7), the
/// only shape in the member that can be a launch volley. What each sub-munition
/// then does is unknown ([`T452_04_CLUSTER`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalClusterShape {
    /// `CLUSTER_SIZE` of an ordnance record: the sub-munitions one launch fires.
    Volley {
        /// How many sub-munitions the record launches.
        munitions: u32,
    },
    /// A record whose `CLUSTER_SIZE` is its `AMMO_LIMIT`: rounds carried.
    AmmunitionCount {
        /// The declared count (`CLUSTER_SIZE`, which equals `AMMO_LIMIT`).
        rounds: u32,
    },
    /// The record declares no `CLUSTER_SIZE`.
    Undeclared,
}

/// The lifetime a record states, if it states one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OriginalLifetime {
    /// A record key states a duration, in seconds.
    Declared {
        /// The duration.
        seconds: f64,
        /// The key it came from.
        key: &'static str,
    },
    /// No record key states a duration. The member carries no `EXPIRES` key at
    /// all (measured by `accept_t452_01_`), so there is nothing to read rather
    /// than a value this project declined to copy.
    Undeclared,
}

/// The `RANGE` the parser leaves in the weapon record's `+0x1c`, metres
/// (task T452-R.01 — static analysis, `checked` provenance at most).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OriginalRangeBound {
    /// The record declares `RANGE`; store `0x5adfb5`, square to `+0x20` at
    /// `0x5adfb8`.
    Declared {
        /// The declared metres.
        m: f64,
    },
    /// The record declares no `RANGE`: the parser writes the default `500.0`
    /// at `0x5ad80b` and no key store runs (`wep_13`, `wep_15`).
    Defaulted,
}

impl OriginalRangeBound {
    /// The metres the record ends up with.
    #[must_use]
    pub fn metres(&self) -> f64 {
        match self {
            Self::Declared { m } => *m,
            Self::Defaulted => ORIGINAL_RANGE_DEFAULT_M,
        }
    }
}

/// The `RANGE` the parser supplies when a record declares none (`mov
/// [record+0x1c], 0x43fa0000` at `0x5ad80b`).
pub const ORIGINAL_RANGE_DEFAULT_M: f64 = 500.0;

/// The `VELOCITY` the parser supplies when a record declares none (`mov
/// [record+0x2c], 0x42f00000` at `0x5ad812`).
pub const ORIGINAL_VELOCITY_DEFAULT_MPS: f64 = 120.0;

/// The launch speed the weapon record's `+0x2c` holds once the parser's rule
/// order has run, in metres per second along the launch direction (task
/// T452-R.01).
///
/// Parse order matters and is measured: `BEAM` first writes `+0x2c = 10000.0`
/// or the reciprocal of `TIME_TO_MAX_RANGE` (`0x5ad950..0x5ada08`) and sets
/// flag bit `0x2` of `+0x74`; `VELOCITY` stores later at `0x5ae0b8` and is
/// skipped when that bit is set (`0x5ae0ac`); then, only when `GRAVITY` parsed
/// to a non-zero `+0x54` (`jne` at `0x5ae4ed` skips the block when it is `0`),
/// `+0x2c` is overwritten with `sqrt(2·GRAVITY·RANGE)` (`0x5ae4ef..0x5ae520`,
/// a bit-level square root). The `GRAVITY` block is the post-parse tail and
/// runs unconditionally, so a declared non-zero `GRAVITY` loses to nothing —
/// it overwrites a `BEAM` rate just as it overwrites a declared `VELOCITY`.
///
/// Every retail record that declares `GRAVITY` at all declares `GRAVITY 0`
/// (`wep_04`, `wep_25`–`wep_28`): the override is engine-real but unexercised
/// by the retail data, so those records fly at their declared `VELOCITY` and
/// the step's `dt·+0x54` pull is zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OriginalLaunchSpeed {
    /// `VELOCITY` declared and not overridden: stored verbatim.
    Declared {
        /// The metres per second.
        mps: f64,
    },
    /// `VELOCITY` absent: the parser default `120.0` stands.
    Defaulted,
    /// `GRAVITY` declared non-zero: `+0x2c = sqrt(2·GRAVITY·RANGE)` — the
    /// speed a 45° lob needs to reach `RANGE`. `+0x200` separately keeps
    /// `VELOCITY²/(2·GRAVITY)`, the height the declared speed could climb to.
    /// No retail record reaches this arm (all declare `GRAVITY 0`).
    Ballistic {
        /// The speed the parser computes, metres per second.
        mps: f64,
        /// The declared gravity, metres per second squared.
        gravity_mps2: f64,
        /// The range the computation used (declared or the `500.0` default).
        range_m: f64,
    },
    /// `BEAM` declared: `+0x2c = 1/TIME_TO_MAX_RANGE`, or `10000.0` when that
    /// key is present and not positive; with the key absent the reciprocal
    /// runs on the `10000.0` default and yields `1e-4` — transcribed as
    /// measured, not "corrected". The value is a rate, not metres per second.
    /// A declared non-zero `GRAVITY` still overwrites it — see the enum doc.
    Beam {
        /// The value `+0x2c` holds, per second.
        per_second: f64,
    },
}

impl OriginalLaunchSpeed {
    /// The scalar the spawn copies to the node's target speed (`+0x644`).
    /// For [`Self::Beam`] that is the stored rate, which the engine uses as
    /// a speed all the same; no retail record declares `BEAM`.
    #[must_use]
    pub fn metres_per_second(&self) -> f64 {
        match self {
            Self::Declared { mps } | Self::Ballistic { mps, .. } => *mps,
            Self::Defaulted => ORIGINAL_VELOCITY_DEFAULT_MPS,
            Self::Beam { per_second } => *per_second,
        }
    }
}

/// How a spawned item's velocity relates to the launcher's velocity (task
/// T452-R.01): the spawn `0x5aef40` writes the projectile's `+0x60` vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OriginalInheritance {
    /// `ACCELERATION` (`+0x38`) zero or undeclared and `INSTANT` clear:
    /// `+0x640 = +0x644 = VELOCITY` and `+0x60 = shooter_velocity +
    /// launch_dir·VELOCITY` (`0x5af144..0x5af15a`, via the multiply-accumulate
    /// `0x5389a0`) — the launcher's whole velocity is added, fraction 1.0, and
    /// the item flies at that constant vector (no speed ramp runs, because
    /// `+0x640 == +0x644`). Every gun and every unaccelerated rocket.
    Full,
    /// `ACCELERATION` non-zero and `INSTANT` clear (`0x5af315..0x5af3b5`):
    /// `+0x640 = |shooter_velocity| + 1e-4`, `+0x644 = VELOCITY +
    /// |shooter_velocity|`, and `+0x60 = shooter_velocity −
    /// launch_dir·|shooter_velocity|` — the component of the launcher's
    /// velocity along the launch direction is removed and the speed scalar
    /// takes its place, then ramps toward `+0x644` at `ACCELERATION` per
    /// second (`0x5afd70`). The item starts at the launcher's speed and
    /// accelerates along the launch direction.
    LateralRemainder,
    /// `INSTANT` set: `+0x60` is computed as `LateralRemainder` but unused —
    /// the per-step displacement for the flag is `RANGE·launch_dir` instead.
    Instant,
}

/// One launched component's measured launch geometry (task T452-R.01,
/// `checked` provenance — static analysis of the owner image).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OriginalLaunchGeometry {
    /// The speed `+0x2c` holds after parsing.
    pub speed: OriginalLaunchSpeed,
    /// `1/FIRE_RATE` — the interval the parser writes to record `+0x14` in
    /// seconds (`0x5adcc9..0x5adcd2`). `None` when the record declares no
    /// `FIRE_RATE`; the field then stays `0`, which the mount's refire
    /// arithmetic would read as "always ready".
    pub refire_interval_s: Option<f64>,
    /// `ACCELERATION` (`+0x38`): metres per second the projectile's speed
    /// scalar `+0x640` gains per second toward its target `+0x644`
    /// (`0x5afd70..0x5afdaf`). `None` when undeclared — the zero-initialized
    /// field then means "no acceleration", which selects the
    /// [`OriginalInheritance::Full`] spawn path.
    pub acceleration_mps2: Option<f64>,
    /// The `RANGE` bound the parser leaves in `+0x1c`.
    pub range: OriginalRangeBound,
    /// How the spawned item's velocity is formed from the launcher's.
    pub inheritance: OriginalInheritance,
    /// `DETONATION_TIME` when positive: the step detonates the item once its
    /// age exceeds it (`0x5b029a`). A zero, negative or absent value is no
    /// timer (the parser default is `-1.0`).
    pub detonation_timer_s: Option<f64>,
    /// `MINE` (record flag `0x4000`): the speed ramp is skipped and the path
    /// length flown is halved every step (`0x5b01cc`), so `RANGE` is never
    /// reached by a step shorter than `RANGE/2`.
    pub mine: bool,
}

/// Derives the launch geometry the parser leaves in one record (T452-R.01).
///
/// Pure: mirrors the parser's measured rule order — `BEAM` before `VELOCITY`
/// before `GRAVITY`, the `BEAM` flag suppressing the `VELOCITY` store, and the
/// ballistic overwrite using the effective (declared-or-default) `RANGE`. The
/// `GRAVITY` block is the post-parse tail (`0x5ae4df`), so it overwrites a
/// `BEAM` rate too: the override precedence is `GRAVITY != 0` > `BEAM` >
/// `VELOCITY` > the `120.0` default.
#[must_use]
pub fn original_launch_geometry(record: &OriginalWeaponRecord) -> OriginalLaunchGeometry {
    let range = record
        .first("RANGE")
        .map_or(OriginalRangeBound::Defaulted, |m| {
            OriginalRangeBound::Declared { m }
        });
    let speed =
        if let Some(gravity_mps2) = record.first("GRAVITY").filter(|gravity| *gravity != 0.0) {
            let range_m = range.metres();
            OriginalLaunchSpeed::Ballistic {
                mps: (2.0 * gravity_mps2 * range_m).sqrt(),
                gravity_mps2,
                range_m,
            }
        } else if record.declares("BEAM") {
            let per_second = match record.first("TIME_TO_MAX_RANGE") {
                Some(t) if t > 0.0 => 1.0 / t,
                Some(_) => 10_000.0,
                None => 1.0e-4,
            };
            OriginalLaunchSpeed::Beam { per_second }
        } else {
            record
                .first("VELOCITY")
                .map_or(OriginalLaunchSpeed::Defaulted, |mps| {
                    OriginalLaunchSpeed::Declared { mps }
                })
        };
    let acceleration_mps2 = record.first("ACCELERATION");
    let inheritance = if record.declares("INSTANT") {
        OriginalInheritance::Instant
    } else if acceleration_mps2.is_some_and(|a| a != 0.0) {
        OriginalInheritance::LateralRemainder
    } else {
        OriginalInheritance::Full
    };
    OriginalLaunchGeometry {
        speed,
        refire_interval_s: record.first("FIRE_RATE").map(|rate| 1.0 / rate),
        acceleration_mps2,
        range,
        inheritance,
        detonation_timer_s: record
            .first("DETONATION_TIME")
            .filter(|seconds| *seconds > 0.0),
        mine: record.declares("MINE"),
    }
}

/// What ends one item's flight and when (task T452-R.04).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OriginalFlightEnd {
    /// The path length flown reaches `RANGE` (`0x5b01ec`).
    Range,
    /// The age exceeds `DETONATION_TIME` first (`0x5b02b0`).
    Timer,
    /// `INSTANT`: the first step displaces by `RANGE·dir`, so the flight is
    /// over after one step.
    Instant,
    /// `MINE` with no timer: nothing in the step ends the flight.
    Never,
}

/// How long one item flies, derived from the step's arithmetic (task
/// T452-R.04). Continuous seconds; the original ends the flight on the first
/// step at or after this time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OriginalFlightDuration {
    /// Seconds from release; `None` when [`OriginalFlightEnd::Never`] or the
    /// item does not move.
    pub seconds: Option<f64>,
    /// The rule that ends the flight.
    pub end: OriginalFlightEnd,
}

impl OriginalLaunchGeometry {
    /// The flight duration for an **unguided straight flight** released with
    /// `shooter_velocity` (metres per second) along unit vector
    /// `launch_direction` (task T452-R.04).
    ///
    /// The step adds `|dt·velocity|` to a path-length counter every step and
    /// ends the flight when it reaches `RANGE` (`0x5b0160`, `0x5b01e6`) or
    /// the age passes `DETONATION_TIME` (`0x5b029a`):
    ///
    /// - [`OriginalInheritance::Full`]: the velocity is the constant vector
    ///   `shooter_velocity + launch_direction·VELOCITY`, so the time is
    ///   `RANGE / |that|`.
    /// - [`OriginalInheritance::LateralRemainder`]: the speed is `s + min(a·t,
    ///   VELOCITY)` with `s = |shooter_velocity| + 1e-4` and `a =
    ///   ACCELERATION` (`0x5af315`, `0x5afd70`), integrated to `RANGE`.
    ///
    /// `LOCK_ON` steering with a live target and `RANDOM_DEVIATION` change
    /// direction, not modeled here. The result depends on the launcher's
    /// velocity at release, a runtime input: the data states no duration, it
    /// states the rule.
    #[must_use]
    pub fn flight_duration(
        &self,
        shooter_velocity: [f64; 3],
        launch_direction: [f64; 3],
    ) -> OriginalFlightDuration {
        let timer = self.detonation_timer_s;
        let finish = |range_s: Option<f64>| match (range_s, timer) {
            (Some(range), Some(timer)) if range <= timer => OriginalFlightDuration {
                seconds: Some(range),
                end: OriginalFlightEnd::Range,
            },
            (_, Some(timer)) => OriginalFlightDuration {
                seconds: Some(timer),
                end: OriginalFlightEnd::Timer,
            },
            (Some(range), None) => OriginalFlightDuration {
                seconds: Some(range),
                end: OriginalFlightEnd::Range,
            },
            (None, None) => OriginalFlightDuration {
                seconds: None,
                end: OriginalFlightEnd::Never,
            },
        };
        if self.inheritance == OriginalInheritance::Instant {
            return OriginalFlightDuration {
                seconds: Some(0.0),
                end: OriginalFlightEnd::Instant,
            };
        }
        if self.mine {
            return finish(None);
        }
        let range_m = self.range.metres();
        let speed = self.speed.metres_per_second();
        let shooter_speed = norm(shooter_velocity);
        match self.inheritance {
            OriginalInheritance::Full => {
                let flown = norm([
                    shooter_velocity[0] + launch_direction[0] * speed,
                    shooter_velocity[1] + launch_direction[1] * speed,
                    shooter_velocity[2] + launch_direction[2] * speed,
                ]);
                finish((flown > 0.0).then(|| range_m / flown))
            }
            OriginalInheritance::LateralRemainder => {
                let start = shooter_speed + RAMP_START_SPEED_MPS;
                let accel = self.acceleration_mps2.unwrap_or(0.0);
                let ramp_s = speed / accel;
                let at_ramp_end = start * ramp_s + 0.5 * accel * ramp_s * ramp_s;
                let seconds = if at_ramp_end >= range_m {
                    (-start + (start * start + 2.0 * accel * range_m).sqrt()) / accel
                } else {
                    ramp_s + (range_m - at_ramp_end) / (start + speed)
                };
                finish(Some(seconds))
            }
            OriginalInheritance::Instant => unreachable!("handled above"),
        }
    }
}

/// The scalar a ramping spawn adds to the launcher's speed (`0x5af318`).
const RAMP_START_SPEED_MPS: f64 = 1.0e-4;

fn norm(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// The launch geometry of every record in a census, keyed by record name
/// (T452-R.01). Pure: the same derivation as [`original_launch_geometry`].
#[must_use]
pub fn original_launch_geometry_table(
    census: &OriginalWeaponCensus,
) -> BTreeMap<String, OriginalLaunchGeometry> {
    census
        .records
        .iter()
        .map(|record| (record.record.clone(), original_launch_geometry(record)))
        .collect()
}

/// The area a record declares, in metres and seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OriginalAreaDeclaration {
    /// `IMPACT_PROXIMITY`, metres: task #1318 measured that the record value
    /// reaches the world-space distance comparison with no conversion, and
    /// F16-E measured the world unit is the metre.
    pub radius_m: Option<f64>,
    /// How long the area lasts, seconds: the only measured declaration is
    /// `SMOKE_SCREEN`'s own second value (8 s). No other record states one.
    pub lifetime_s: Option<f64>,
}

/// The damage channels a record declares.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OriginalDamageDeclaration {
    /// `ARMOR_DAMAGE`, the declared amount.
    pub armor: Option<f64>,
    /// `HEALTH_DAMAGE`, the declared amount.
    pub health: Option<f64>,
    /// `DAMAGE`, the single channel the flash records declare instead.
    pub single: Option<f64>,
}

impl OriginalDamageDeclaration {
    /// How many channels the record declares a value for.
    #[must_use]
    pub fn channels(&self) -> usize {
        usize::from(self.armor.is_some())
            + usize::from(self.health.is_some())
            + usize::from(self.single.is_some())
    }
}

/// One status/effect flag a record declares, with the duration it states.
///
/// The flag names the effect (the engine's own key name); **which gameplay
/// status kind it drives is not stated by any file** ([`T452_04_STATUS_KIND`]),
/// so no [`DeclaredStatusEffectKind`] is assigned here.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalStatusEffectDeclaration {
    /// The record key that states the effect (`TANGLER`, `SMOKE_SCREEN`, ...).
    pub key: &'static str,
    /// The duration the key states, seconds, when it carries one.
    pub duration_s: Option<f64>,
    /// The unit word the key states with the duration (`TIME`), when present.
    pub duration_unit: Option<String>,
}

/// One value this stage could not measure, named by the claim it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalComponentUnknown {
    /// The claim id (`t452_04.limit.*`, `t454.limit.swarm`).
    pub claim: String,
    /// Why the value is not known, and what it affects.
    pub reason: String,
}

impl OriginalComponentUnknown {
    fn new(claim: &str, reason: impl Into<String>) -> Self {
        Self {
            claim: claim.to_owned(),
            reason: reason.into(),
        }
    }
}

/// One launched component's measured declaration (task T452.04).
///
/// Every field is what a file states, exactly as stored; a value no file states
/// is `None`/`Undeclared` **and** an entry in [`Self::unknowns`], never a
/// default. The record's own name is the identity, so two records that disagree
/// stay two rows.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalComponentLaunch {
    /// The record name (`wep_04`).
    pub record: String,
    /// The family the record's keys state (T452.01's rules).
    pub family: DeclaredOrdnanceFamily,
    /// Where the screens mount it.
    pub mount: OriginalMountDeclaration,
    /// `VELOCITY`, the first value, exactly as stored. Task T452-R.01
    /// measured it statically as metres per second along the launch
    /// direction: the spawn adds `dir·VELOCITY` to a velocity vector that the
    /// step integrates as `displacement = dt·v`, and F16-E measured the world
    /// unit is the metre. A declared `GRAVITY` or `BEAM` makes the parser
    /// overwrite the field — see [`original_launch_geometry`].
    pub velocity: Option<f64>,
    /// `FIRE_RATE`, the first value, exactly as stored. Task T452-R.01
    /// measured that the parser stores `1/FIRE_RATE` in record `+0x14` (an
    /// interval, seconds). Task T452-R.04 measured that no mount takes it:
    /// the mount's interval is the airframe `weapons` row's third element
    /// ([`OriginalMountRow`]); the record's `+0x14` only sizes the projectile
    /// pool. [`T452_04_RELEASE_DELAY`] stays only on a record no row names.
    pub fire_rate: Option<f64>,
    /// `DETONATION_DISTANCE`, metres: the proximity fuse's trigger radius, and
    /// the radius of the sphere around the target the fuse tests (task T452.02,
    /// statically measured; the record value reaches the squared-distance
    /// comparison with no conversion). `None` when the record declares no
    /// proximity trigger, which is not the same as a zero radius.
    pub proximity_trigger_m: Option<f64>,
    /// The lifetime the record states, if any.
    pub lifetime: OriginalLifetime,
    /// The area the record declares.
    pub area: OriginalAreaDeclaration,
    /// The damage channels the record declares.
    pub damage: OriginalDamageDeclaration,
    /// Every status/effect flag the record declares.
    pub statuses: Vec<OriginalStatusEffectDeclaration>,
    /// What its `CLUSTER_SIZE` states.
    pub cluster: OriginalClusterShape,
    /// Every value this row could not measure.
    pub unknowns: Vec<OriginalComponentUnknown>,
}

impl OriginalComponentLaunch {
    /// Whether the record declares any status/effect flag at all.
    #[must_use]
    pub fn declares_status(&self) -> bool {
        !self.statuses.is_empty()
    }

    /// Whether a claim id is recorded against this row.
    #[must_use]
    pub fn names_claim(&self, claim: &str) -> bool {
        self.unknowns.iter().any(|unknown| unknown.claim == claim)
    }
}

/// Every launched component's measured declaration, plus the layout it was read
/// against (task T452.04).
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalComponentTable {
    /// The mount layout the screens declare.
    pub layout: OriginalMountLayout,
    /// One row per record a T452.01 rule classifies to a family.
    pub rows: Vec<OriginalComponentLaunch>,
    /// The records no rule classifies (the guns and the cannonball), by name —
    /// visible rather than silently dropped.
    pub excluded: Vec<String>,
    /// Values no single record owns (which airframe fills which slot).
    pub unknowns: Vec<OriginalComponentUnknown>,
}

impl OriginalComponentTable {
    /// One row by record name.
    #[must_use]
    pub fn row(&self, record: &str) -> Option<&OriginalComponentLaunch> {
        self.rows.iter().find(|row| row.record == record)
    }

    /// The rows whose family is `family`.
    #[must_use]
    pub fn rows_of(&self, family: DeclaredOrdnanceFamily) -> Vec<&OriginalComponentLaunch> {
        self.rows
            .iter()
            .filter(|row| row.family == family)
            .collect()
    }

    /// Every distinct claim id the whole table records, sorted.
    #[must_use]
    pub fn claims(&self) -> BTreeSet<String> {
        let mut claims: BTreeSet<String> = self
            .rows
            .iter()
            .flat_map(|row| row.unknowns.iter().map(|unknown| unknown.claim.clone()))
            .collect();
        claims.extend(self.unknowns.iter().map(|unknown| unknown.claim.clone()));
        claims
    }
}

/// Builds the per-component launch table from an already-read census and layout.
///
/// Pure: no installation, no clock, no preferred default. A row is produced for
/// every record a T452.01 rule classifies; a record no rule reaches goes into
/// [`OriginalComponentTable::excluded`] by name.
#[must_use]
pub fn original_component_table(
    census: &OriginalWeaponCensus,
    layout: &OriginalMountLayout,
) -> OriginalComponentTable {
    original_component_table_with_mounts(census, layout, &BTreeSet::new())
}

/// [`original_component_table`] with the set of record names some airframe's
/// `weapons` row mounts (task T452-R.04, [`mounted_original_records`]).
///
/// A record in `mounted` has its refire interval stated by those rows, so the
/// [`T452_04_RELEASE_DELAY`] claim is not recorded against it; a record no row
/// names keeps the claim. The lifetime claim [`T452_04_NO_EXPIRES`] is never
/// recorded: every record's flight end is derivable from its own keys
/// ([`OriginalLaunchGeometry::flight_duration`]).
#[must_use]
pub fn original_component_table_with_mounts(
    census: &OriginalWeaponCensus,
    layout: &OriginalMountLayout,
    mounted: &BTreeSet<String>,
) -> OriginalComponentTable {
    let mut rows = Vec::new();
    let mut excluded = Vec::new();
    for row in &census.rows {
        let Some(family) = row.class.family() else {
            excluded.push(row.record.record.clone());
            continue;
        };
        rows.push(component_launch(&row.record, family, layout, mounted));
    }
    OriginalComponentTable {
        layout: layout.clone(),
        rows,
        excluded,
        unknowns: vec![OriginalComponentUnknown::new(
            T452_04_PER_AIRFRAME,
            "which of the declared slots a given airframe fills with which \
             component is in that plane's record in the engine image, not in \
             any file; the measured fresh-profile default names rocket ids per \
             slot (data/owner-static/engine-facts.toml, [flight_check_default]) \
             and no file maps a rocket id to a `wep_` record",
        )],
    }
}

/// One record's row.
fn component_launch(
    record: &OriginalWeaponRecord,
    family: DeclaredOrdnanceFamily,
    layout: &OriginalMountLayout,
    mounted: &BTreeSet<String>,
) -> OriginalComponentLaunch {
    let velocity = record.first("VELOCITY");
    let fire_rate = record.first("FIRE_RATE");

    let mount = if record.declares(ORIGINAL_ROCKET_FLAG) {
        OriginalMountDeclaration::RocketSlot {
            slots: layout.rocket_slots.slots,
        }
    } else {
        OriginalMountDeclaration::Unknown {
            claim: T452_04_MOUNT.to_owned(),
            reason: "the record reaches an ordnance family but declares no \
                     ROCKET flag, so no screen states which slot holds it"
                .to_owned(),
        }
    };

    let statuses: Vec<OriginalStatusEffectDeclaration> = ORIGINAL_EFFECT_KEYS
        .iter()
        .filter(|key| record.declares(key))
        .map(|key| OriginalStatusEffectDeclaration {
            key,
            duration_s: record.second_values.get(*key).copied(),
            duration_unit: record.first_texts.get(*key).cloned(),
        })
        .collect();

    let lifetime =
        record
            .first("DETONATION_TIME")
            .map_or(OriginalLifetime::Undeclared, |seconds| {
                OriginalLifetime::Declared {
                    seconds,
                    key: "DETONATION_TIME",
                }
            });

    let area = OriginalAreaDeclaration {
        radius_m: record.first("IMPACT_PROXIMITY"),
        lifetime_s: record.second_values.get("SMOKE_SCREEN").copied(),
    };

    let damage = OriginalDamageDeclaration {
        armor: record.first("ARMOR_DAMAGE"),
        health: record.first("HEALTH_DAMAGE"),
        single: record.first("DAMAGE"),
    };

    let cluster = match record.first("CLUSTER_SIZE") {
        Some(cluster_size) if record.first("AMMO_LIMIT") == Some(cluster_size) => {
            OriginalClusterShape::AmmunitionCount {
                rounds: whole(cluster_size),
            }
        }
        Some(cluster_size) if cluster_size.fract() == 0.0 && cluster_size >= 0.0 => {
            OriginalClusterShape::Volley {
                munitions: whole(cluster_size),
            }
        }
        _ => OriginalClusterShape::Undeclared,
    };

    let mut unknowns = Vec::new();
    if fire_rate.is_some() && !mounted.contains(&record.record) {
        unknowns.push(OriginalComponentUnknown::new(
            T452_04_RELEASE_DELAY,
            "the mount's refire interval is the third element of an airframe's \
             `weapons` row (0x4b5a79), not record +0x14, and no airframe row \
             names this record, so no file states the interval its mount would use",
        ));
    }
    // An area-denial record's whole effect is an area, and it declares the
    // effect flag without stating that area's size. `wep_12` (TANGLER) states
    // a duration for the effect and no area lifetime either; `wep_13`'s
    // SMOKE_SCREEN states 8 s, which the row keeps as the area's lifetime.
    let area_effect = family == DeclaredOrdnanceFamily::AreaDenialEngine;
    if area_effect && area.radius_m.is_none() {
        unknowns.push(OriginalComponentUnknown::new(
            T452_04_AREA_RADIUS,
            "the record's whole effect is an area and it declares no \
             IMPACT_PROXIMITY, so the area's radius is stated by no file",
        ));
    }
    if area.lifetime_s.is_none() && (area.radius_m.is_some() || area_effect) {
        unknowns.push(OriginalComponentUnknown::new(
            T452_04_AREA_LIFETIME,
            if area_effect {
                "the record's whole effect is an area and no file states how long \
                 the area lasts; its effect duration is the effect's, not stated \
                 to be the area's"
            } else {
                "the record declares a blast radius and no lifetime for the area"
            },
        ));
    }
    if !statuses.is_empty() {
        unknowns.push(OriginalComponentUnknown::new(
            T452_04_STATUS_KIND,
            "no file states which gameplay status kind the declared effect flag drives",
        ));
    }
    let undated: Vec<&str> = statuses
        .iter()
        .filter(|status| status.duration_s.is_none())
        .map(|status| status.key)
        .collect();
    if !undated.is_empty() {
        unknowns.push(OriginalComponentUnknown::new(
            T452_04_STATUS_DURATION,
            format!(
                "the declared effect flag {} carries no duration",
                undated.join(", ")
            ),
        ));
    }
    if matches!(cluster, OriginalClusterShape::Volley { munitions } if munitions > 1) {
        unknowns.push(OriginalComponentUnknown::new(
            T452_04_CLUSTER,
            "CLUSTER_SIZE states how many sub-munitions a launch fires; no file \
             states what each sub-munition does",
        ));
    }
    if matches!(mount, OriginalMountDeclaration::Unknown { .. }) {
        unknowns.push(OriginalComponentUnknown::new(
            T452_04_MOUNT,
            "no screen states which slot holds a record without a ROCKET flag",
        ));
    }

    OriginalComponentLaunch {
        record: record.record.clone(),
        family,
        mount,
        velocity,
        fire_rate,
        proximity_trigger_m: record.first("DETONATION_DISTANCE"),
        lifetime,
        area,
        damage,
        statuses,
        cluster,
        unknowns,
    }
}

/// A declared count as a whole number. The member stores every `CLUSTER_SIZE`
/// and `AMMO_LIMIT` as an integer f64, so the truncation is exact; a value that
/// was not whole would be visible through the equality check that chose this
/// arm.
fn whole(value: f64) -> u32 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        value as u32
    }
}

/// Reads the installation's weapon census and its screen layout, and builds the
/// per-component launch table (task T452.04).
///
/// # Errors
///
/// [`OriginalComponentError`] when the records or a screen cannot be read.
pub fn read_original_component_table(
    install_root: &std::path::Path,
) -> Result<OriginalComponentTable, OriginalComponentError> {
    let census =
        read_original_weapon_census(install_root).map_err(OriginalComponentError::Records)?;
    let layout = read_original_mount_layout(install_root)?;
    let rows = read_original_mount_rows(install_root).map_err(OriginalComponentError::Records)?;
    Ok(original_component_table_with_mounts(
        &census,
        &layout,
        &mounted_original_records(&rows),
    ))
}

// ------------------- T452.R04: the weapon-to-mount release binding ------------
//
// Task #2169 measured where a mount entry's refire interval (`+0x14`) comes
// from. The mount builder `0x4b59b0` walks the `weapons` key of an airframe's
// vehicle record; each row is `(wep_NN count interval value value)`, and the
// entry takes the row's third element as `+0x14` (`0x4b5a79`), not weapon
// record `+0x14` (`1/FIRE_RATE`), whose only reader sizes the projectile pool
// (`0x5ae564`). The fire loop adds `+0x14` to the shared clock to schedule the
// mount's (and, for a flag-clear mount, the launcher's) next release.

/// The vehicle-record key the mount builder walks (string `0x62b128`).
pub const ORIGINAL_MOUNT_ROW_KEY: &str = "weapons";

/// One `weapons` row of one airframe: a mount entry as the builder fills it.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalMountRow {
    /// The vehicle record the row resolves under (`kind_of` chain applied).
    pub airframe: String,
    /// The weapon record the row names (`wep_04`).
    pub record: String,
    /// The mount id the builder derives from the name: `name[4]*10 + name[5]
    /// - 0x210` (`0x4b5a43..0x4b5a4e`), i.e. the two digits of `wep_NN` — so
    /// `wep_130` is mount 13, the same id as `wep_13`.
    pub mount_id: i32,
    /// Row element 1 → entry `+0x08`, the count the fire loop tests `> 0`.
    pub count: f64,
    /// Row element 2 → entry `+0x14`: the seconds the fire loop adds to the
    /// shared clock after a release (`0x4b6b2d`).
    pub release_interval_s: f64,
    /// Row element 3; the entry stores its square at `+0x18`. Its meaning is
    /// not measured.
    pub third_value: f64,
    /// Row element 4; the entry stores its square at `+0x1c`. Its meaning is
    /// not measured.
    pub fourth_value: f64,
}

/// The mount id the builder derives from a record name (`0x4b5a43..0x4b5a4e`).
#[must_use]
pub fn original_mount_id(record: &str) -> i32 {
    let bytes = record.as_bytes();
    let digit = |index: usize| i32::from(bytes.get(index).copied().unwrap_or(0));
    digit(4) * 10 + digit(5) - 0x210
}

/// Every `weapons` row of every airframe in a decoded `vehicle.zrd`, in file
/// order, each airframe resolved through its `kind_of` chain.
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] when the document is
/// malformed or a row is not `(name count interval value value)`.
pub fn original_mount_rows(
    document: &crate::original_airframe::SpannedDocument,
) -> Result<Vec<OriginalMountRow>, crate::original_airframe::OriginalImportError> {
    use crate::original_airframe::{OriginalImportError, resolve_vehicle_record};
    let shape = |detail: String| OriginalImportError::Shape { detail };
    let outer = document
        .root()
        .as_list()
        .and_then(|outer| outer.first())
        .and_then(|inner| inner.as_list())
        .ok_or_else(|| shape("the vehicle document has no record list".to_owned()))?;
    let mut rows = Vec::new();
    for pair in outer.chunks(2) {
        let airframe = pair[0]
            .as_text()
            .ok_or_else(|| shape("a record name is not text".to_owned()))?;
        let resolved = resolve_vehicle_record(document, airframe)?;
        let Some(list) = resolved
            .value(ORIGINAL_MOUNT_ROW_KEY)
            .and_then(|v| v.as_list())
        else {
            continue;
        };
        for row in list {
            let cell = |index: usize| {
                row.element(index)
                    .and_then(|node| node.as_scalar_f64())
                    .ok_or_else(|| {
                        shape(format!("{airframe}: a weapons row lacks element {index}"))
                    })
            };
            let record = row
                .element(0)
                .and_then(|node| node.as_scalar_text())
                .ok_or_else(|| shape(format!("{airframe}: a weapons row has no record name")))?;
            rows.push(OriginalMountRow {
                airframe: airframe.to_owned(),
                record: record.to_owned(),
                mount_id: original_mount_id(record),
                count: cell(1)?,
                release_interval_s: cell(2)?,
                third_value: cell(3)?,
                fourth_value: cell(4)?,
            });
        }
    }
    Ok(rows)
}

/// Reads the installation's `vehicle.zrd` and returns its mount rows.
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] when the archive, the
/// member or a row cannot be read.
pub fn read_original_mount_rows(
    install_root: &std::path::Path,
) -> Result<Vec<OriginalMountRow>, crate::original_airframe::OriginalImportError> {
    let documents = crate::original_airframe::OriginalDocuments::read(install_root)?;
    original_mount_rows(documents.vehicle())
}

/// The names of the weapon records some airframe row mounts.
#[must_use]
pub fn mounted_original_records(rows: &[OriginalMountRow]) -> BTreeSet<String> {
    rows.iter().map(|row| row.record.clone()).collect()
}

// ------------------- T452.R02: measured effect, fuse and swarm behavior ------
//
// Task #2160 measured the remaining original ordnance behavior statically in
// the owner-supplied decrypted engine image (`$CS_ENGINE_IMAGE`, image base
// `0x400000`): what each effect flag does, the duration and radius an area
// field uses, how a projectile ends its flight, whether a lost target needs a
// `ROCKET` flag, and what `CLUSTER_SIZE` launches. Every value below carries a
// [`StaticProvenance`] window that the `accept_t452_r02_` suite re-reads from
// the image, so a claim of measurement cannot drift from the bytes.
//
// This layer is *measured behavior*, not a lowering: the runtime components
// live in `cs_sim` and this crate may not link it. It resolves the T452.04
// claims whose value is now known and keeps a named claim wherever the runtime
// vocabulary cannot express what the image does.

/// Claim: `SONIC`/`FLASH` incapacitate the struck actor, and
/// `DeclaredStatusEffectKind` has no incapacitation variant.
pub const T452_R02_STATUS_INCAPACITATION: &str = "t452_r02.limit.status_incapacitation";

/// Claim: what a zero-damage `TANGLER` field does to a flight model. The field
/// exists and re-applies the weapon's per-hit routine every tick, but `wep_12`
/// declares no damage and no per-hit status flag, so the gameplay consequence
/// is not visible in the measured path.
pub const T452_R02_TANGLER_EFFECT: &str = "t452_r02.limit.tangler_flight_effect";

/// Claim (T452.R02, resolved by T452-REG-SMOKE-RADIUS): `SMOKE_SCREEN` has no
/// gameplay radius. It does: the emitter's per-step update (`0x4b8fd0`) reads
/// the `smokescreen_stun_range` global, so the radius is measured as that key
/// ([`MeasuredAreaSource::GlobalKey`]) and no measured record keeps this claim.
pub const T452_R02_SMOKE_RADIUS: &str = "t452_r02.limit.smoke_radius";

/// Claim (resolved by T452-REG-SMOKE-STUN): the smoke emitter stuns actors
/// inside the `smokescreen_stun_*` cone (`0x4200d0` for an AI actor, `0x42e9d0`
/// for the player). [`SmokeStunRule`] expresses both, so no measured record
/// keeps this claim any more.
pub const T452_R02_SMOKE_STUN_EFFECT: &str = "t452_r02.limit.smoke_stun_effect";

/// Claim: `BEEPER_SEEKER` retargets the projectile onto a live beeper each
/// step, which `GuidanceRule::Targeted` cannot express (it names one target).
/// Resolved by task T452-REG-BEEPER-SEEKER (`GuidanceRule::MarkerSeeker`): no
/// measured record keeps it any more.
pub const T452_R02_BEEPER_SEEKER: &str = "t452_r02.limit.beeper_seeker_guidance";

/// The T452.04 fuse claim (`t452.limit.fuse`), resolved for a record whose
/// end of flight the measured gate now states.
pub const T452_R02_FUSE_CLAIM: &str = "t452.limit.fuse";

/// The T452.04 lost-target claim (`t452.limit.lost_target`), resolved for the
/// one targeted record #454's `ROCKET`-record measurement did not cover.
pub const T452_R02_LOST_TARGET_CLAIM: &str = "t452.limit.lost_target";

/// A byte window of the engine image that a measured value was read from.
///
/// `va` is the virtual address (image base `0x400000`) and `len` the number of
/// bytes; a test re-reads the window from `$CS_ENGINE_IMAGE` and compares it
/// with the code site it claims. Static analysis only: never
/// `verified_original`, and no original executable ran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticProvenance {
    /// Virtual address in the image.
    pub va: u32,
    /// Window length in bytes.
    pub len: u16,
}

impl StaticProvenance {
    /// A pin at `va` covering `len` bytes.
    #[must_use]
    pub const fn new(va: u32, len: u16) -> Self {
        Self { va, len }
    }
}

/// What an effect flag does, as the image's own consumer of the flag shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredEffectAction {
    /// The flag places a timed marker on the struck actor for another system to
    /// consume (`BEEPER`). This is the runtime `Marker` kind.
    TimedMarker,
    /// The flag is an area field created at the shooter, not a per-victim
    /// status (`TANGLER`, `SMOKE_SCREEN`).
    AreaField,
    /// The flag runs the per-hit exposure at `0x42e840`, which incapacitates
    /// the struck actor for a computed duration (`SONIC`, `FLASH`).
    ExposureIncapacitation,
    /// The flag installs a guidance callback rather than applying a status
    /// (`BEEPER_SEEKER`); the runtime expresses it as
    /// `GuidanceRule::MarkerSeeker`.
    GuidanceModifier,
}

/// How long a measured effect lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeasuredEffectDuration {
    /// The record's own `TIME` value, in seconds.
    RecordTime,
    /// The image's default when the record states no `TIME`, in seconds.
    ImageDefaultSeconds(f64),
    /// The per-hit exposure formula: a linear falloff from full intensity at
    /// `full_intensity_ratio` or below to zero at `zero_intensity_ratio`, times
    /// `max_seconds`, optionally scaled by the victim's facing.
    Exposure {
        /// Full-intensity duration ceiling, seconds.
        max_seconds: f64,
        /// Ratio at or below which intensity is 1.0.
        full_intensity_ratio: f64,
        /// Ratio at or above which intensity is 0.0.
        zero_intensity_ratio: f64,
        /// Whether the facing dot product further scales the duration.
        facing_scaled: bool,
    },
    /// The flag carries no duration of its own.
    None,
}

/// One effect flag, measured.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredEffect {
    /// The record key (`BEEPER`, `TANGLER`, ...).
    pub key: &'static str,
    /// What the flag does.
    pub action: MeasuredEffectAction,
    /// How long it lasts.
    pub duration: MeasuredEffectDuration,
    /// The runtime status kind when one is measured to fit, `None` when the
    /// vocabulary cannot express the measured effect.
    pub runtime_kind: Option<DeclaredStatusEffectKind>,
    /// The claim kept when no runtime kind fits.
    pub claim: Option<&'static str>,
    /// Where the action and duration were measured.
    pub pin: StaticProvenance,
}

/// Where a measured area value came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeasuredAreaSource {
    /// The record's own key states the value.
    RecordKey {
        /// The key (`TIME`, `IMPACT_PROXIMITY`, ...).
        key: &'static str,
    },
    /// The image applies a default because the record states no key.
    ImageDefault {
        /// The default the image stores.
        value: f64,
    },
    /// The image reads the value from a named global key of `player.zrd`, not
    /// from the record; the key's retail value is read by
    /// `OriginalDocuments::smokescreen_stun`, and `image_default` is what the
    /// image keeps when the key is absent.
    GlobalKey {
        /// The `player.zrd` key.
        key: &'static str,
        /// The image's own default for the key.
        image_default: f64,
    },
    /// A blast is instantaneous: it has no area lifetime to measure (T1318).
    InstantaneousBlast,
    /// No measured value exists; the claim names what is missing.
    Unmeasured {
        /// The claim id.
        claim: &'static str,
    },
}

/// A measured area effect: radius, lifetime and where each came from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredArea {
    /// The radius in metres, when measured.
    pub radius_m: Option<f64>,
    /// Where the radius came from.
    pub radius_source: MeasuredAreaSource,
    /// The lifetime in seconds, when measured.
    pub lifetime_s: Option<f64>,
    /// Where the lifetime came from.
    pub lifetime_source: MeasuredAreaSource,
    /// The window the area's consumer was read from.
    pub pin: StaticProvenance,
}

/// How a projectile's flight ends at the end of `RANGE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredRangeEnd {
    /// The step calls the detonation path at the end of `RANGE`.
    Detonates,
    /// The step marks the projectile dead without detonating.
    ExpiresSilently,
}

/// The measured end of a projectile's flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredFuse {
    /// What the end of `RANGE` does.
    pub range_end: MeasuredRangeEnd,
    /// The gate the step reads: `LOCK_ON` without `EXPIRES`, or
    /// `DETONATE_AT_RANGE`.
    pub gate: &'static str,
    /// Whether the projectile is marked dead (`+0x20 = 0`) on that path.
    pub consumed: bool,
    /// The window the gate was read from.
    pub gate_pin: StaticProvenance,
    /// The window the consumption store was read from.
    pub consume_pin: StaticProvenance,
}

/// A measured lost-target rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredLostTarget {
    /// Whether the rule keys on `LOCK_ON` rather than `ROCKET`.
    pub requires_lock_on: bool,
    /// The measured behavior (`coast_then_detonate_at_range_end`).
    pub behavior: &'static str,
    /// The window the steering guard was read from.
    pub pin: StaticProvenance,
}

/// A measured clustered launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredSwarm {
    /// The record's `CLUSTER_SIZE`.
    pub munitions: u32,
    /// How many projectiles one launch step spawns: measured as one.
    pub projectiles_per_launch: u32,
    /// The window the per-launch countdown was read from; the single spawn it
    /// counts down to is pinned separately (`0x4b7289` calls `0x5aef40` once).
    pub pin: StaticProvenance,
}

/// The measured behavior of one original component (task T452.R02).
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalBehaviorMeasurement {
    /// Every effect flag the record declares, measured.
    pub effects: Vec<MeasuredEffect>,
    /// The area effect, when the record has one.
    pub area: Option<MeasuredArea>,
    /// How its flight ends.
    pub fuse: MeasuredFuse,
    /// The lost-target rule, for a targeted record.
    pub lost_target: Option<MeasuredLostTarget>,
    /// The clustered launch, for a volley record.
    pub swarm: Option<MeasuredSwarm>,
    /// What the smoke emitter does to the actors it reaches, for `SMOKE_SCREEN`.
    pub smoke_stun: Option<SmokeStunRule>,
    /// Values this measurement could not resolve, named.
    pub claims: Vec<OriginalComponentUnknown>,
    /// The T452.04 claim ids this measurement resolves for this row.
    pub resolved: Vec<&'static str>,
}

impl OriginalBehaviorMeasurement {
    /// Whether the row names `claim`.
    #[must_use]
    pub fn names_claim(&self, claim: &str) -> bool {
        self.claims.iter().any(|unknown| unknown.claim == claim)
    }

    /// Whether this measurement resolves `claim`.
    #[must_use]
    pub fn resolves_claim(&self, claim: &str) -> bool {
        self.resolved.contains(&claim)
    }
}

/// The image's default for a `TIME` word a record does not state: `0x4ba869`
/// (`TANGLER`) and `0x4ba779` (`BEEPER`) store `5.0` through the same parser,
/// and `0x4ba806` does it for `SMOKE_SCREEN`.
const IMAGE_DEFAULT_TIME_S: f64 = 5.0;

/// How long an effect whose duration is the record's `TIME` lasts, and where
/// that duration came from: the record when it states one, else the image's
/// `5.0 s` default.
fn measured_time(duration_s: Option<f64>) -> MeasuredEffectDuration {
    match duration_s {
        Some(_) => MeasuredEffectDuration::RecordTime,
        None => MeasuredEffectDuration::ImageDefaultSeconds(IMAGE_DEFAULT_TIME_S),
    }
}

/// Where an area lifetime came from: the record's `TIME` when it states one,
/// else the image's `5.0 s` default.
fn measured_time_source(duration_s: Option<f64>) -> MeasuredAreaSource {
    match duration_s {
        Some(_) => MeasuredAreaSource::RecordKey { key: "TIME" },
        None => MeasuredAreaSource::ImageDefault {
            value: IMAGE_DEFAULT_TIME_S,
        },
    }
}

/// Measures one component's effect, fuse, lost-target and swarm behavior from
/// the values already measured in the image.
///
/// The input is the T452.04 row: its declared status keys and durations, its
/// area, its family and its `CLUSTER_SIZE`. The output names the code window
/// each measured rule was read from and keeps a claim wherever the runtime
/// vocabulary cannot express the original.
#[must_use]
pub fn measure_component_behavior(row: &OriginalComponentLaunch) -> OriginalBehaviorMeasurement {
    let mut effects = Vec::new();
    let mut claims = Vec::new();
    let mut resolved = Vec::new();

    for status in &row.statuses {
        let effect = match status.key {
            "BEEPER" => {
                resolved.push(T452_04_STATUS_KIND);
                resolved.push(T452_04_STATUS_DURATION);
                MeasuredEffect {
                    key: "BEEPER",
                    action: MeasuredEffectAction::TimedMarker,
                    duration: measured_time(status.duration_s),
                    runtime_kind: Some(DeclaredStatusEffectKind::Marker),
                    claim: None,
                    pin: StaticProvenance::new(0x004b_9d95, 14),
                }
            }
            "TANGLER" => {
                resolved.push(T452_04_STATUS_KIND);
                resolved.push(T452_04_STATUS_DURATION);
                claims.push(OriginalComponentUnknown::new(
                    T452_R02_TANGLER_EFFECT,
                    "the tangler field re-applies the weapon's per-hit routine every \
                     tick to actors inside RADIUS; wep_12 declares no damage and no \
                     per-hit status flag, so what the field does to a flight model is \
                     not visible in the measured path",
                ));
                MeasuredEffect {
                    key: "TANGLER",
                    action: MeasuredEffectAction::AreaField,
                    duration: measured_time(status.duration_s),
                    runtime_kind: None,
                    claim: Some(T452_R02_TANGLER_EFFECT),
                    pin: StaticProvenance::new(0x004b_9482, 18),
                }
            }
            "SMOKE_SCREEN" => {
                resolved.push(T452_04_STATUS_KIND);
                resolved.push(T452_04_STATUS_DURATION);
                MeasuredEffect {
                    key: "SMOKE_SCREEN",
                    action: MeasuredEffectAction::AreaField,
                    duration: measured_time(status.duration_s),
                    runtime_kind: None,
                    // Expressed by `SmokeStunRule`, not by a status kind.
                    claim: None,
                    pin: StaticProvenance::new(0x004b_71b1, 5),
                }
            }
            "SONIC" | "FLASH" => {
                resolved.push(T452_04_STATUS_DURATION);
                claims.push(OriginalComponentUnknown::new(
                    T452_R02_STATUS_INCAPACITATION,
                    "SONIC and FLASH run the per-hit exposure at 0x42e840, which \
                     stores mode 4 and zeroes the struck actor's controls until a \
                     computed timestamp; StatusEffectKind has no pilot-incapacitation \
                     variant, so no variant is forced",
                ));
                MeasuredEffect {
                    key: if status.key == "SONIC" {
                        "SONIC"
                    } else {
                        "FLASH"
                    },
                    action: MeasuredEffectAction::ExposureIncapacitation,
                    duration: MeasuredEffectDuration::Exposure {
                        max_seconds: 5.0,
                        full_intensity_ratio: 0.6,
                        zero_intensity_ratio: 1.0,
                        facing_scaled: status.key == "FLASH",
                    },
                    runtime_kind: None,
                    claim: Some(T452_R02_STATUS_INCAPACITATION),
                    pin: StaticProvenance::new(0x0042_e85b, 53),
                }
            }
            "BEEPER_SEEKER" => MeasuredEffect {
                key: "BEEPER_SEEKER",
                action: MeasuredEffectAction::GuidanceModifier,
                duration: MeasuredEffectDuration::None,
                runtime_kind: None,
                claim: None,
                pin: StaticProvenance::new(0x0044_1877, 10),
            },
            _ => continue,
        };
        effects.push(effect);
    }

    let targeted = matches!(
        row.family,
        DeclaredOrdnanceFamily::GuidedRocket | DeclaredOrdnanceFamily::AerialTorpedo
    );
    let fuse = MeasuredFuse {
        range_end: if targeted {
            MeasuredRangeEnd::Detonates
        } else {
            // The range-end gate is now measured for every unguided record: a
            // declared `DETONATION_DISTANCE` cannot reach the proximity branch
            // without `LOCK_ON` (`0x5b0216`), and none of the fifteen `ROCKET`
            // records declares `DETONATE_AT_RANGE` (T548's 41-key census), so
            // the only measured end of flight is the silent removal at
            // `0x5b0318`. That resolves the claim for every such row, not only
            // the two the task named.
            resolved.push(T452_R02_FUSE_CLAIM);
            MeasuredRangeEnd::ExpiresSilently
        },
        gate: "LOCK_ON without EXPIRES detonates at the end of RANGE; otherwise \
               DETONATE_AT_RANGE detonates and a record with neither expires",
        consumed: true,
        gate_pin: StaticProvenance::new(0x005b_01ec, 42),
        consume_pin: StaticProvenance::new(0x005b_0318, 7),
    };

    let lost_target = targeted.then(|| {
        if matches!(row.mount, OriginalMountDeclaration::Unknown { .. }) {
            // The glidebomb is the one targeted record with no ROCKET flag, and
            // therefore the one #454's ROCKET-only measurement did not reach.
            resolved.push(T452_R02_LOST_TARGET_CLAIM);
        }
        MeasuredLostTarget {
            requires_lock_on: true,
            behavior: "coast_then_detonate_at_range_end",
            pin: StaticProvenance::new(0x005a_f7f4, 12),
        }
    });

    let swarm = match row.cluster {
        OriginalClusterShape::Volley { munitions } => {
            resolved.push(T452_04_CLUSTER);
            Some(MeasuredSwarm {
                munitions,
                projectiles_per_launch: 1,
                pin: StaticProvenance::new(0x004b_72be, 11),
            })
        }
        OriginalClusterShape::AmmunitionCount { .. } | OriginalClusterShape::Undeclared => None,
    };

    let area = measured_area(row, &mut resolved);

    OriginalBehaviorMeasurement {
        effects,
        area,
        fuse,
        lost_target,
        swarm,
        smoke_stun: row
            .statuses
            .iter()
            .any(|status| status.key == "SMOKE_SCREEN")
            .then(SmokeStunRule::measured),
        claims,
        resolved,
    }
}

/// The area a row's effect creates, measured from the image's consumers.
fn measured_area(
    row: &OriginalComponentLaunch,
    resolved: &mut Vec<&'static str>,
) -> Option<MeasuredArea> {
    let tangler = row.statuses.iter().find(|s| s.key == "TANGLER");
    if let Some(tangler) = tangler {
        // `RADIUS` defaults to 10 at 0x4ba870 and `TIME` to 5 at 0x4ba869;
        // wep_12 states TIME 2 and no RADIUS, so the default applies. No retail
        // `wep_` record declares `RADIUS` at all (T452.01's measured 46-key
        // union in `accept_t452_01_weapon_census`), and the T452.04 row does
        // not carry it, so there is no record value this default could shadow.
        let radius_m = 10.0;
        let lifetime_s = tangler.duration_s.unwrap_or(IMAGE_DEFAULT_TIME_S);
        resolved.push(T452_04_AREA_RADIUS);
        resolved.push(T452_04_AREA_LIFETIME);
        return Some(MeasuredArea {
            radius_m: Some(radius_m),
            radius_source: MeasuredAreaSource::ImageDefault { value: radius_m },
            lifetime_s: Some(lifetime_s),
            lifetime_source: measured_time_source(tangler.duration_s),
            pin: StaticProvenance::new(0x004b_950f, 26),
        });
    }
    let smoke = row.statuses.iter().find(|s| s.key == "SMOKE_SCREEN");
    if let Some(smoke) = smoke {
        let lifetime_s = smoke.duration_s.unwrap_or(IMAGE_DEFAULT_TIME_S);
        resolved.push(T452_04_AREA_RADIUS);
        resolved.push(T452_04_AREA_LIFETIME);
        return Some(MeasuredArea {
            radius_m: None,
            radius_source: MeasuredAreaSource::GlobalKey {
                key: "smokescreen_stun_range",
                image_default: 200.0,
            },
            lifetime_s: Some(lifetime_s),
            lifetime_source: measured_time_source(smoke.duration_s),
            pin: StaticProvenance::new(0x004b_71b1, 5),
        });
    }
    if row.area.radius_m.is_some() {
        // A blast applies its damage once per detonation (T1318), so it has no
        // area lifetime to measure: the radius is the instantaneous burst.
        resolved.push(T452_04_AREA_LIFETIME);
        return Some(MeasuredArea {
            radius_m: row.area.radius_m,
            radius_source: MeasuredAreaSource::RecordKey {
                key: "IMPACT_PROXIMITY",
            },
            lifetime_s: None,
            lifetime_source: MeasuredAreaSource::InstantaneousBlast,
            pin: StaticProvenance::new(0x005a_c3a0, 16),
        });
    }
    None
}

// ------------------- T452.R05: the record-to-slot loadout map and the ----
// ------------------- SONIC/FLASH incapacitation status -------------------
//
// Task #2170 measured two things statically in the owner-supplied decrypted
// engine image (`$CS_ENGINE_IMAGE`, base `0x400000`):
//
// 1. how a `wep_` record reaches a loaded slot, and therefore what slot the
//    glidebomb `wep_25` occupies; and
// 2. what the two effect bits `SONIC|FLASH` (`test ch, 0x0a` at `0x4b9d84`) do
//    to the actor they strike: its kind, its duration and which actors it
//    reaches.
//
// Every statement below cites a virtual address; the `accept_t452_r05_` suite
// re-reads each byte window from the image, so a stale address fails rather
// than passes. Nothing here is `verified_original`: no original executable ran,
// and `retail` is read access to the owner's files.

/// The plane-record loadout binder, `0x443de0`.
///
/// **Measured, `static`** (task T452-R.05): it takes the local plane record
/// (the `0x198`-byte airframe record the loadout screen edits), walks its four
/// gun ids at `record + 0x88` through the gun mapper `0x443d70` into
/// [`ORIGINAL_GUN_SLOT_TABLE_VA`], then walks its eight rocket ids at
/// `record + 0xa8` through [`ORIGINAL_ROCKET_ID_FUNCTION_VA`] into
/// [`ORIGINAL_ROCKET_SLOT_TABLE_VA`] (`0x443f08`..`0x443f2c`), and finally
/// calls [`ORIGINAL_SLOT_INSTALL_FUNCTION_VA`] (`0x443f35`).
pub const ORIGINAL_LOADOUT_BINDER_VA: u64 = 0x44_3de0;

/// The preset loadout binder, `0x444180` (task T452-R.05).
///
/// **Measured, `static`**: the second path that fills the same slot arrays,
/// from the preset's four gun ids at `0x643058`, its four flag words at
/// `0x643068` and its eight rocket ids at `0x643078`. Every rocket id goes
/// through the same [`ORIGINAL_ROCKET_ID_FUNCTION_VA`]
/// (`0x444269`..`0x444284`), so this path cannot select a record the id map
/// does not name either.
pub const ORIGINAL_LOADOUT_PRESET_BINDER_VA: u64 = 0x44_4180;

/// The four gun slots of the engine's loadout table, `0x62ae28`.
///
/// **Measured, `static`** (task T452-R.05): entry `i` is the `weapons.zrd`
/// record index the airframe's gun slot `i` holds, or `-1` for an empty slot.
pub const ORIGINAL_GUN_SLOT_TABLE_VA: u64 = 0x62_ae28;

/// The eight rocket slots of the engine's loadout table, `0x62ae38`.
///
/// **Measured, `static`** (task T452-R.05): entry `j` is the `weapons.zrd`
/// record index the airframe's rocket slot `j` holds, or `-1` for an empty
/// slot. The table runs to [`ORIGINAL_SLOT_FLAG_TABLE_VA`], so it holds
/// exactly [`ORIGINAL_ROCKET_SLOT_COUNT`] dwords.
pub const ORIGINAL_ROCKET_SLOT_TABLE_VA: u64 = 0x62_ae38;

/// The slot-enable flag bytes, `0x62ae58`: one byte per gun slot (read by the
/// binder and stored by the installer), followed by the rocket slots'
/// implicit "enabled" flag the installer passes as `1`.
pub const ORIGINAL_SLOT_FLAG_TABLE_VA: u64 = 0x62_ae58;

/// How many gun slots the loadout table carries. The binder's first loop ends
/// at [`ORIGINAL_ROCKET_SLOT_TABLE_VA`] (`0x443ef8`).
pub const ORIGINAL_GUN_SLOT_COUNT: u32 = 4;

/// How many rocket slots the loadout table carries. The binder's second loop
/// ends at [`ORIGINAL_SLOT_FLAG_TABLE_VA`] (`0x443f26`).
pub const ORIGINAL_ROCKET_SLOT_COUNT: u32 = 8;

/// The offset of the four gun ids in the plane record (`0x443e4a`:
/// `lea ebx, [ecx + 0x88]`).
pub const ORIGINAL_PLANE_RECORD_GUN_IDS_OFFSET: u32 = 0x88;

/// The offset of the eight rocket ids in the plane record (`0x443f0d`:
/// `lea edi, [edx + 0xa8]`). Each id is one of the eleven rocket types
/// [`ORIGINAL_ROCKET_ID_WEAPON_INDEX`] maps, or the "none" id `11`.
pub const ORIGINAL_PLANE_RECORD_ROCKET_IDS_OFFSET: u32 = 0xa8;

/// The slot installer `0x4b24d0`: installs all twelve slots of the table
/// through `0x4b2550(entity, slot, weapon_index, -1, flag)`.
///
/// **Measured, `static`** (task T452-R.05): the slot index is the table index,
/// `0..=3` the gun slots and `4..=11` the eight rocket slots (`0x4b250b`:
/// `cmp esi, 0xc`; `0x4b2571`: `cmp esi, 4` selects the rocket branch).
pub const ORIGINAL_SLOT_INSTALL_FUNCTION_VA: u64 = 0x4b_24d0;

/// The slot's record lookup `0x4bad90`, which turns a slot's weapon index into
/// the `wep_` record it names.
///
/// **Measured, `static`** (task T452-R.05; T452-R.03 measured the lookup, this
/// task pins it as the link between a slot index and a record name): given the
/// index a slot holds, it formats the engine's own
/// [`ORIGINAL_SLOT_RECORD_NAME_FORMAT_VA`] with it (`0x4bada7`) and answers the
/// record from the global record list through `0x5abfd0` (`0x4badb8`). A slot's
/// weapon index `N` is therefore exactly the record named `wep_NN`, which is
/// what [`original_weapon_record_index`] reads back from the name: `wep_25` is
/// index 25, the index no rocket id produces.
pub const ORIGINAL_SLOT_RECORD_LOOKUP_VA: u64 = 0x4b_ad90;

/// The engine's `wep_%02d` record-name format, `0x62b394` (pushed at
/// `0x4bada7`). Eight bytes, no NUL inside the cited window.
pub const ORIGINAL_SLOT_RECORD_NAME_FORMAT_VA: u64 = 0x62_b394;

/// The one-slot script writer `0x43e553`, inside the command handler that also
/// answers the engine's revive command.
///
/// **Measured, `static`** (task T452-R.05): the two-argument form stores its
/// second argument, a raw `weapons.zrd` record index, at
/// `slot_table[first_argument]` and then calls the installer. The handler's
/// command name is the five-byte compare at `0x43e530`; its argument reader is
/// `0x5b7980`, which applies no range check, so this is the one writer that can
/// name a record index the rocket id map does not produce. It installs a record
/// *at a slot the mission names*; it is not a slot assignment by any airframe.
pub const ORIGINAL_SCRIPT_AMMO_SLOT_WRITE_VA: u64 = 0x43_e553;

/// The all-rocket-slot script writer `0x43e56a`, the one-argument form of the
/// same command: `mov ecx, 8; mov edi, 0x62ae38; rep stosd` fills all eight
/// rocket slots with the raw record index.
pub const ORIGINAL_SCRIPT_AMMO_ROCKET_FILL_VA: u64 = 0x43_e56a;

/// The loadout screen's rocket-type count: eleven ids (`0`..`=10`), the
/// [`ORIGINAL_ROCKET_ID_WEAPON_INDEX`] table's length.
pub const ORIGINAL_ROCKET_TYPE_COUNT: u32 = 11;

/// Claim kept when a record's slot is measured but the record can still be
/// installed by a mission script: `t452_r05.limit.script_ammo_use`.
///
/// The loadout map is measured and answers "no slot"; what a *mission* does
/// with the two script writers above is script content, not record content, and
/// no script was read for this task (follow-up task `T452-R05-AMMO-SCRIPT`,
/// #2173). That follow-up has since landed on `main`: its census of
/// `ZBD/interp.zbd` found no `ammo` command in 98 scripts, so no retail script
/// installs any of these records by index either.
pub const T452_R05_SCRIPT_AMMO_USE: &str = "t452_r05.limit.script_ammo_use";

/// The `cs_sim` status vocabulary's measured gap: the effect the two bits apply
/// is pilot incapacitation (mode `4`, control words zeroed until a clock
/// timestamp for every actor but the player; a screen/audio exposure for the
/// player), and none of `StatusEffectKind`'s `Damage`/`Choke`/`Stall`/`Marker`
/// variants expresses it.
///
/// The missing runtime variant and its application path are follow-up task
/// `T452-R05-STATUS-KIND` (#2172). No variant is forced here.
pub const T452_R05_STATUS_KIND_FOLLOW_UP: &str = "T452-R05-STATUS-KIND";

/// The claim kept for the measured effect's missing runtime variant:
/// `t452_r02.limit.status_incapacitation` (task T452-R.02's id).
pub const T452_R05_STATUS_INCAPACITATION: &str = T452_R02_STATUS_INCAPACITATION;

/// How a classified record reaches a loaded slot, measured (task T452-R.05).
///
/// A plane record does **not** name a record directly: each of its eight rocket
/// slots holds a rocket *type id*, and
/// [`ORIGINAL_ROCKET_ID_FUNCTION_VA`] turns that id into a `weapons.zrd` record
/// index. `wep_05`..`wep_15` are selectable that way; the other classified
/// records are not selectable by any airframe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginalSlotReach {
    /// A plane record's rocket slot can hold the record: any of `slots` slots,
    /// on any airframe, by holding one of `rocket_ids`.
    RocketSlot {
        /// The rocket-slot count the loadout table carries.
        slots: u32,
        /// The rocket type ids that select this record, in id order.
        rocket_ids: Vec<u32>,
    },
    /// No plane record's slot selects the record. `script_index_writer` says
    /// whether the mission-script writers above can still install it by naming
    /// its raw index — true for every record that exists, which is exactly why
    /// [`T452_R05_SCRIPT_AMMO_USE`] stays a claim.
    NoRocketSlot {
        /// Whether the measured script writers can install the record by index.
        script_index_writer: bool,
    },
}

/// One classified record's measured slot reach (task T452-R.05).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalSlotMeasurement {
    /// The record name (`wep_25`).
    pub record: String,
    /// The `weapons.zrd` record index the name carries (`wep_25` -> `25`), or
    /// `None` when the name carries no number.
    pub weapon_index: Option<u32>,
    /// How the loadout map reaches the record.
    pub reach: OriginalSlotReach,
    /// The claim ids this measurement resolves for the row.
    pub resolved: Vec<&'static str>,
    /// Values the measurement does not resolve, named.
    pub claims: Vec<OriginalComponentUnknown>,
    /// The binder window the reach was measured at.
    pub pin: StaticProvenance,
}

impl OriginalSlotMeasurement {
    /// Whether this measurement resolves `claim`.
    #[must_use]
    pub fn resolves_claim(&self, claim: &str) -> bool {
        self.resolved.contains(&claim)
    }

    /// Whether this measurement names `claim` as unresolved.
    #[must_use]
    pub fn names_claim(&self, claim: &str) -> bool {
        self.claims.iter().any(|unknown| unknown.claim == claim)
    }
}

/// The `weapons.zrd` record index a `wep_` record name carries.
///
/// `wep_25` -> `Some(25)`; a name outside the `wep_<number>` shape -> `None`.
/// The number is the index the engine's slot table holds because the slot's own
/// lookup [`ORIGINAL_SLOT_RECORD_LOOKUP_VA`] formats
/// [`ORIGINAL_SLOT_RECORD_NAME_FORMAT_VA`] with that index to find the record
/// by name (`0x4bada7`), so name and index are two spellings of one value.
#[must_use]
pub fn original_weapon_record_index(record: &str) -> Option<u32> {
    record.strip_prefix("wep_")?.parse().ok()
}

/// The rocket type ids that select record `weapon_index`, in id order.
///
/// Measured from [`ORIGINAL_ROCKET_ID_WEAPON_INDEX`]: an empty answer means no
/// plane-record rocket slot can hold the record.
#[must_use]
pub fn original_rocket_ids_for_weapon_index(weapon_index: u32) -> Vec<u32> {
    ORIGINAL_ROCKET_ID_WEAPON_INDEX
        .iter()
        .enumerate()
        .filter(|(_, index)| **index == weapon_index)
        .map(|(id, _)| u32::try_from(id).unwrap_or(u32::MAX))
        .collect()
}

/// Measures one classified component row's slot reach (task T452-R.05).
///
/// The row's own mount is left exactly as T452.04 recorded it: this layer is
/// the measurement that resolves the claim, and the T452.04 row keeps its
/// pre-existing entry until the runtime-registration task consumes this layer
/// (the same split T452-R.02 used). For a record the loadout map cannot select,
/// the mount claim `t452_04.limit.rocket_flag_missing` is resolved here and the
/// script question is named separately.
#[must_use]
pub fn measure_loadout_slot(row: &OriginalComponentLaunch) -> OriginalSlotMeasurement {
    let weapon_index = original_weapon_record_index(&row.record);
    let rocket_ids = weapon_index.map_or_else(Vec::new, original_rocket_ids_for_weapon_index);
    let mut resolved = Vec::new();
    let mut claims = Vec::new();
    let reach = if rocket_ids.is_empty() {
        if matches!(row.mount, OriginalMountDeclaration::Unknown { .. }) {
            resolved.push(T452_04_MOUNT);
        }
        claims.push(OriginalComponentUnknown::new(
            T452_R05_SCRIPT_AMMO_USE,
            "no airframe's rocket slot can hold this record (no rocket type id selects \
             its index), but the mission-script command at 0x43e553/0x43e56a stores a raw \
             record index into the slot table with no range check, so a mission could \
             install it at a slot it names; whether any retail mission does is script \
             content and was not read (follow-up task T452-R05-AMMO-SCRIPT)",
        ));
        OriginalSlotReach::NoRocketSlot {
            script_index_writer: true,
        }
    } else {
        OriginalSlotReach::RocketSlot {
            slots: ORIGINAL_ROCKET_SLOT_COUNT,
            rocket_ids,
        }
    };
    OriginalSlotMeasurement {
        record: row.record.clone(),
        weapon_index,
        reach,
        resolved,
        claims,
        pin: StaticProvenance::new(0x0044_3f13, 10),
    }
}

/// Measures every classified row's slot reach, in table order.
#[must_use]
pub fn measure_loadout_slots(table: &OriginalComponentTable) -> Vec<OriginalSlotMeasurement> {
    table.rows.iter().map(measure_loadout_slot).collect()
}

/// What the two effect bits do to the actor they strike, as the image's code
/// shows it (task T452-R.05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeasuredStatusKind {
    /// The struck actor loses control of its aircraft: an AI/flight actor's
    /// mode word becomes `4` and its control words are zeroed for a computed
    /// duration; the player gets the exposure's screen and sound effect
    /// instead. Not damage, not engine-output loss, not a stall, not a marker.
    PilotIncapacitation,
}

/// How long the measured status lasts (task T452-R.05).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasuredStatusDuration {
    /// The duration ceiling in seconds (`intensity * 5.0`, `0x42e9b1`).
    pub max_seconds: f64,
    /// The ratio at or below which the intensity is full (`0.6`, `0x60411c`).
    pub full_intensity_ratio: f64,
    /// The ratio at or above which the intensity is zero (`1.0`, `0x6032dc`).
    pub zero_intensity_ratio: f64,
    /// Whether the victim's facing further scales the intensity (`FLASH` only,
    /// `0x42e8ed`..`0x42e966`).
    pub facing_scaled: bool,
    /// What the ratio is measured over: the squared distance from the blast to
    /// the victim over the record's `IMPACT_PROXIMITY` squared (parsed into
    /// `record + 0x3c`/`+0x40` at `0x5add6a`, squared again at the call site
    /// `0x4ba590` and divided at `0x42e862`).
    pub ratio_denominator: &'static str,
    /// The formula window.
    pub pin: StaticProvenance,
}

/// What the measured status does to the player actor (task T452-R.05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredPlayerExposure {
    /// The intensity drives the screen effect (`0x42e9d0`, `0x4ba61d`).
    pub screen_effect: bool,
    /// A sound name is resolved for the effect (`0x4ba603`; `SONIC` and
    /// `FLASH` name different sounds, `0x62b1d0`/`0x62b1dc`).
    pub sound: bool,
    /// The player's controls are **not** zeroed: the branch returns to the
    /// common tail after the screen-effect call (`0x4ba625`).
    pub control_loss: bool,
}

/// What the measured status does to every other struck actor (task
/// T452-R.05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredActorIncapacitation {
    /// The mode word the path stores at `+0x358` (`0x420127`).
    pub mode: u32,
    /// How many control words the path zeroes (`+0x100`, `+0x108`, `+0x10c`,
    /// `+0x114`, `+0x11c`, `+0x120`, `0x42015a`..`0x420185`).
    pub control_words_zeroed: u32,
    /// The end of the effect: the clock (`0x71c470`) reaching `clock +
    /// duration` at `+0xc0` clears mode `4` (`0x489acd`..`0x489ae9`).
    pub ends_at: &'static str,
    /// The gate window (`0x4200ea`..) and the store window.
    pub gate_pin: StaticProvenance,
    pub store_pin: StaticProvenance,
}

/// Which actors the measured status reaches, and through which gate (task
/// T452-R.05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredStatusVictims {
    /// The player actor.
    pub player: MeasuredPlayerExposure,
    /// Every other actor.
    pub other_actor: MeasuredActorIncapacitation,
    /// The gate an "other actor" victim must pass: not the player
    /// (`0x71c298`), `+0x91d == 0`, `+0xf8 == 0`, and `+0x67c` in `{0, 4}`.
    pub gate: &'static str,
}

/// One `SONIC`/`FLASH` effect, measured (task T452-R.05).
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalStatusMeasurement {
    /// The record name.
    pub record: String,
    /// The effect key this row measures (`SONIC` or `FLASH`).
    pub key: &'static str,
    /// What the effect does to the struck actor.
    pub kind: MeasuredStatusKind,
    /// How long it lasts.
    pub duration: MeasuredStatusDuration,
    /// Who it reaches and what it does to each.
    pub victims: MeasuredStatusVictims,
    /// The shot is consumed by the exposure: both result floats are zeroed
    /// (`0x4ba632`).
    pub shot_consumed: bool,
    /// The runtime status kind when one is measured to fit. `None` here: no
    /// `StatusEffectKind` variant expresses pilot incapacitation
    /// ([`T452_R05_STATUS_KIND_FOLLOW_UP`]).
    pub runtime_kind: Option<DeclaredStatusEffectKind>,
    /// The claim kept for the missing runtime variant.
    pub claim: Option<&'static str>,
    /// The follow-up that adds the variant, when the runtime cannot express it.
    pub follow_up_task: Option<&'static str>,
    /// The T452.04 claim ids this measurement resolves for the row.
    pub resolved: Vec<&'static str>,
    /// The per-hit test window (`test ch, 0x0a`).
    pub pin: StaticProvenance,
}

impl OriginalStatusMeasurement {
    /// Whether this measurement resolves `claim`.
    #[must_use]
    pub fn resolves_claim(&self, claim: &str) -> bool {
        self.resolved.contains(&claim)
    }

    /// Whether this measurement names `claim` as unresolved.
    #[must_use]
    pub fn names_claim(&self, claim: &str) -> bool {
        self.claim == Some(claim)
    }
}

/// Measures every `SONIC`/`FLASH` effect a classified row declares (task
/// T452-R.05).
///
/// One row per declared bit, in the record's own key order. A record that
/// declares neither gets an empty answer. The duration claim
/// `t452_04.limit.status_effect_duration` is resolved (the formula is
/// measured); the kind claim `t452_r02.limit.status_incapacitation` is kept
/// with the follow-up that will add the runtime variant.
#[must_use]
pub fn measure_status_effects(row: &OriginalComponentLaunch) -> Vec<OriginalStatusMeasurement> {
    row.statuses
        .iter()
        .filter(|status| matches!(status.key, "SONIC" | "FLASH"))
        .map(|status| {
            let flash = status.key == "FLASH";
            OriginalStatusMeasurement {
                record: row.record.clone(),
                key: if flash { "FLASH" } else { "SONIC" },
                kind: MeasuredStatusKind::PilotIncapacitation,
                duration: MeasuredStatusDuration {
                    max_seconds: 5.0,
                    full_intensity_ratio: 0.6,
                    zero_intensity_ratio: 1.0,
                    facing_scaled: flash,
                    ratio_denominator: "the squared distance from the detonation to the \
                                        victim over the record's IMPACT_PROXIMITY squared",
                    pin: StaticProvenance::new(0x0042_e85b, 10),
                },
                victims: MeasuredStatusVictims {
                    player: MeasuredPlayerExposure {
                        screen_effect: true,
                        sound: true,
                        control_loss: false,
                    },
                    other_actor: MeasuredActorIncapacitation {
                        mode: 4,
                        control_words_zeroed: 6,
                        ends_at: "the shared clock (0x71c470) reaching the stored \
                                  clock + duration at the victim's +0xc0 clears mode 4",
                        gate_pin: StaticProvenance::new(0x0042_00ea, 24),
                        store_pin: StaticProvenance::new(0x0042_0154, 54),
                    },
                    gate: "not the player, victim +0x91d == 0, victim +0xf8 == 0, \
                           victim +0x67c in {0, 4}",
                },
                shot_consumed: true,
                runtime_kind: None,
                claim: Some(T452_R05_STATUS_INCAPACITATION),
                follow_up_task: Some(T452_R05_STATUS_KIND_FOLLOW_UP),
                resolved: vec![T452_04_STATUS_DURATION],
                pin: StaticProvenance::new(0x004b_9d84, 16),
            }
        })
        .collect()
}

// ------------------- the measured effect-class media declarations (T452-R.03-C) ----

/// The virtual address of the engine's shared effect-block reader.
///
/// **Measured, `static`** (never `verified_original`): it fills one `0x64`
/// byte block from a keyed zrd list. The key-to-offset map it writes:
///
/// | block offset | zrd key | resolution |
/// | --- | --- | --- |
/// | `+0x00` bit 0 | `RANDOM_ROTATE` | flag |
/// | `+0x00` bit 1 | an `int` child inside `ANIMATION` | flag |
/// | `+0x04`, `+0x08` | `ANIMATION` | two name handles via `0x523820`, further entries dropped |
/// | `+0x14` | `ANIMATION_ATTACHED` | name via `0x523820` |
/// | `+0x18` | `MODEL_ANIMATION` | name via `0x523820` |
/// | `+0x1c` | `SURFACE_ANIMATION` | name via `0x523820` |
/// | `+0x20` | `ANIMATION_MIN_RANGE` | name via `0x523820` |
/// | `+0x24` | `EFFECT` | name via the effect resolver `0x526160` |
/// | `+0x28` | `MODEL` | name via the model-table lookup `0x5aec00` |
/// | `+0x2c`, `+0x30`.. | `SOUND` | count, then names via `0x596120` |
/// | `+0x40`, `+0x44`.. | `BOUNCE_SOUND` | count, then names via `0x596120` |
///
/// `weapons.zrd` declares no other key inside these blocks.
pub const ORIGINAL_EFFECT_BLOCK_READER_VA: u64 = 0x5a_e990;

/// The stride of one engine effect block: `0x64` bytes, used both for the
/// `FIRE`/`FLYOUT` members and for every `IMPACT` surface row.
pub const ORIGINAL_EFFECT_BLOCK_STRIDE: u64 = 0x64;

/// The virtual address of the `FIRE` key parse site: the weapon parser pushes
/// `"FIRE"` (`0x63cfd8`) and `weapon + 0x94`, then calls
/// [`ORIGINAL_EFFECT_BLOCK_READER_VA`].
pub const ORIGINAL_WEAPON_FIRE_BLOCK_VA: u64 = 0x5a_e0fc;

/// The virtual address of the `FLYOUT` key parse site: the parser pushes
/// `"FLYOUT"` (`0x63cfd0`) and `weapon + 0xf8`, then calls
/// [`ORIGINAL_EFFECT_BLOCK_READER_VA`]. The spawned round attaches the
/// block's `MODEL` (`weapon + 0x120`) — and starts `MODEL_ANIMATION`
/// (`weapon + 0x110`) when present — at `0x5aecbf` in the round allocator.
pub const ORIGINAL_WEAPON_FLYOUT_BLOCK_VA: u64 = 0x5a_e10e;

/// The virtual address of the `IMPACT` key parse site: the parser looks up
/// `"IMPACT"` (`0x63cf98`), parses its `default` child into row 0 at
/// `weapon + 0x15c`, then walks the soil-name table (`0x637b10`, six
/// registered names: `default`, `water`, `seafloor`, `quicksand`, `lava`,
/// `fire`, plus any the mission appended) parsing each declared row at
/// `row * 0x64`. An undeclared soil's row is a verbatim copy of row 0.
pub const ORIGINAL_WEAPON_IMPACT_BLOCK_VA: u64 = 0x5a_e1cf;

/// The virtual address of the `DESTROY_ANIMATION` key parse site: the parser
/// resolves the name through `0x523820` into `weapon + 0x188`
/// (`0x5adc33..0x5adc54`).
pub const ORIGINAL_WEAPON_DESTROY_ANIMATION_VA: u64 = 0x5a_dc33;

/// The virtual address where the fire routine reads `weapon + 0xe8`, the
/// launch sound slot: `-1` plays nothing, otherwise
/// `[weapon + 0xc4 + 4 * slot]` — entry `slot` of the `FIRE` block's `SOUND`
/// array (`0x94 + 0x30`) — is played through `0x593b80` at `0x5af3de`. The
/// `FIRE` block's `EFFECT` (`0x94 + 0x24` = `weapon + 0xb8`) then takes
/// precedence over its `ANIMATION` slots (`0x5af3e6`); with no effect the
/// slot `weapon + 0xec` selects among `ANIMATION` handles.
pub const ORIGINAL_WEAPON_LAUNCH_SOUND_VA: u64 = 0x5a_f3c0;

/// The virtual address of the launch-sound slot selector
/// `(weapon, index) -> bool`: writes `weapon + 0xe8` when `index` is below
/// the `FIRE` `SOUND` count (`weapon + 0xc0`); the callers observed pass `0`
/// (first declared entry) or `-1` (silence — the compare is signed, so `-1`
/// always writes). A slot at or above the count writes nothing and returns
/// `0`.
pub const ORIGINAL_WEAPON_LAUNCH_SLOT_VA: u64 = 0x5a_c120;

/// The virtual address of the sound playback gate: it returns early on a
/// null sound descriptor (`0x593ba4`), so a record with no declared sound
/// plays **nothing** — there is no shared default and nothing
/// mount-specific falls in.
pub const ORIGINAL_SOUND_GATE_VA: u64 = 0x59_3b80;

/// The virtual address of the `IMPACT` row sound picker `(weapon, position,
/// flags, surface_row)`: a uniform `rand * count >> 15` index into the row's
/// `SOUND` array (`+0x30`), played through `0x593b80`. The row's `EFFECT`
/// (`+0x24`) plays via `0x525d90` at `0x5aca01`, its `ANIMATION` slots
/// (`+0x04`/`+0x08`) at `0x5ac942`, and `SURFACE_ANIMATION` (`+0x1c`)
/// surface-aligned at `0x5ac9eb`.
pub const ORIGINAL_WEAPON_IMPACT_SOUND_VA: u64 = 0x5a_d100;

/// The first of the two `DESTROY_ANIMATION` consumer sites: a round ending
/// in flight plays `weapon + 0x188` (`0x5af78b`, through `0x4edda0` or
/// `0x4edc10`); the second, `0x5b06b8`, selects it as the detonation
/// animation when the surface row's `ANIMATION_MIN_RANGE` field is absent.
pub const ORIGINAL_WEAPON_DESTROY_USE_VA: u64 = 0x5a_f78b;

/// The virtual address where the round allocator attaches the `FLYOUT`
/// block's `MODEL` (`weapon + 0x120` = block `+0x28`) to the round's scene
/// node through `0x4d1390`; when `weapon + 0x110` (`MODEL_ANIMATION`,
/// block `+0x18`) is nonzero the animated form is built first via
/// `0x5ad290`.
pub const ORIGINAL_WEAPON_FLYOUT_MODEL_ATTACH_VA: u64 = 0x5a_ecbf;

/// The virtual address of the `LOOPED_SOUND_NAME` key parse site in the
/// weapon's mount-descriptor filler: the name resolves through
/// `0x59cd40(0x13, name)` and the registered callback lands on the
/// descriptor's `+0x24` field. It is the gun loop — a mount-level sound,
/// not an effect-block entry, and no `weapons.zrd` ordnance record declares
/// it.
pub const ORIGINAL_MOUNT_LOOPED_SOUND_VA: u64 = 0x4b_aa7b;

/// The stride of one `wep_` record in the engine's weapon table, `0x214`
/// bytes, confirmed at the table iteration `0x5ac108`.
pub const ORIGINAL_WEAPON_RECORD_STRIDE: u64 = 0x214;

/// The declared contents of one `FIRE`/`FLYOUT`/`IMPACT`-row effect block —
/// the fields the engine's shared block reader at
/// [`ORIGINAL_EFFECT_BLOCK_READER_VA`] fills, kept as the *names* the member
/// declares rather than the engine's resolved handles.
///
/// Names absent from the record stay `None`/`empty`: the engine zeroes the
/// block before filling it, so an absent key means "no resource" — and for
/// sounds the gate at [`ORIGINAL_SOUND_GATE_VA`] turns that into silence,
/// never a substitute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OriginalEffectBlock {
    /// Every key the block declares, sorted — including any the engine's
    /// reader does not name, so nothing is silently dropped.
    pub keys: BTreeSet<String>,
    /// `ANIMATION` entries in declaration order. The engine resolves at
    /// most two into block `+0x04`/`+0x08`; extra entries are dropped by
    /// the engine but kept here.
    pub animations: Vec<String>,
    /// `ANIMATION_ATTACHED` (block `+0x14`).
    pub animation_attached: Option<String>,
    /// `MODEL_ANIMATION` (block `+0x18`) — the looping animation of the
    /// `MODEL`, e.g. `torpedo_trail` for `a_torpedo`.
    pub model_animation: Option<String>,
    /// `SURFACE_ANIMATION` (block `+0x1c`) — played surface-aligned on
    /// impact rows.
    pub surface_animation: Option<String>,
    /// `ANIMATION_MIN_RANGE` (block `+0x20`): the impact row's detonation
    /// animation override; a row without one inherits row 0's, then the
    /// weapon's `DESTROY_ANIMATION` (see
    /// [`ORIGINAL_WEAPON_DESTROY_USE_VA`]).
    pub animation_min_range: Option<String>,
    /// `EFFECT` (block `+0x24`) — resolved through the engine's effect
    /// resolver, not the animation one.
    pub effect: Option<String>,
    /// `MODEL` (block `+0x28`) — a model-table name; on `FLYOUT` it is the
    /// projectile's own visual ([`ORIGINAL_WEAPON_FLYOUT_MODEL_ATTACH_VA`]).
    pub model: Option<String>,
    /// `SOUND` entries in declaration order (block `+0x2c` count, `+0x30`
    /// array). Retail `weapons.zrd` declares at most one per `FIRE` block;
    /// `IMPACT` rows declare several and the picker at
    /// [`ORIGINAL_WEAPON_IMPACT_SOUND_VA`] chooses one at random.
    pub sounds: Vec<String>,
    /// `BOUNCE_SOUND` entries (block `+0x40` count, `+0x44` array).
    pub bounce_sounds: Vec<String>,
    /// The declared `RANDOM_ROTATE` flag value (block `+0x00` bit 0), when
    /// the key is present.
    pub random_rotate: Option<bool>,
}

impl OriginalEffectBlock {
    /// Whether the block declares no key at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// One `IMPACT` row: the surface name the record declares and its block.
///
/// The engine keys rows by registered soil name (`default` is row 0 at
/// `weapon + 0x15c`); `enemy`, `player` and `buildings` are soils the
/// mission registers, so retail records can and do declare them.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalImpactRow {
    /// The surface name as declared (`default`, `water`, `enemy`, ...).
    pub surface: String,
    /// The row's block.
    pub block: OriginalEffectBlock,
}

/// The measured effect-class declarations of one `wep_` record
/// (task T452-R.03-C).
///
/// This is the member-level half of the ordnance media contract: the names
/// each effect-class key declares, with the engine sites that consume them
/// pinned on the constants above. `SOUND_NAME` exists only as the tail of
/// `LOOPED_SOUND_NAME` — there is no literal `SOUND_NAME` key in the image;
/// the launch one-shot is `FIRE`'s `SOUND` list, the flying loop is
/// `FLYOUT`'s, and the mount loop is `LOOPED_SOUND_NAME`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OriginalWeaponMedia {
    /// The record name (`wep_14`).
    pub record: String,
    /// Every top-level key the record declares, sorted.
    pub keys: BTreeSet<String>,
    /// The `FIRE` block at `weapon + 0x94` ([`ORIGINAL_WEAPON_FIRE_BLOCK_VA`]):
    /// the muzzle presentation — `SOUND` is the launch one-shot, `EFFECT`
    /// and `ANIMATION` the muzzle visual.
    pub fire: Option<OriginalEffectBlock>,
    /// The `FLYOUT` block at `weapon + 0xf8`
    /// ([`ORIGINAL_WEAPON_FLYOUT_BLOCK_VA`]): the flying projectile's own
    /// presentation — `MODEL`/`MODEL_ANIMATION` attach to the round at
    /// spawn, `SOUND` is the declared flight loop.
    pub flyout: Option<OriginalEffectBlock>,
    /// The `IMPACT` rows ([`ORIGINAL_WEAPON_IMPACT_BLOCK_VA`]), in declared
    /// order — the detonation presentation, keyed by surface.
    pub impact_rows: Vec<OriginalImpactRow>,
    /// `DESTROY_ANIMATION` (`weapon + 0x188`,
    /// [`ORIGINAL_WEAPON_DESTROY_ANIMATION_VA`]): the end-of-life animation,
    /// e.g. `torpedo_destroy_effect` on `wep_14`.
    pub destroy_animation: Option<String>,
    /// `LOOPED_SOUND_NAME` ([`ORIGINAL_MOUNT_LOOPED_SOUND_VA`]): the mount's
    /// gun loop — declared by `CANNON` records, not by ordnance.
    pub looped_sound_name: Option<String>,
}

impl OriginalWeaponMedia {
    /// The launch one-shot this record declares: the first `FIRE` `SOUND`
    /// entry, which is what the fire routine plays at the muzzle
    /// ([`ORIGINAL_WEAPON_LAUNCH_SOUND_VA`]).
    ///
    /// `None` means the launch is silent — the slot selector at
    /// [`ORIGINAL_WEAPON_LAUNCH_SLOT_VA`] writes the caller's index (`0`
    /// selects the first declared entry, `-1` silence) and the playback
    /// gate at [`ORIGINAL_SOUND_GATE_VA`] returns early on a missing
    /// handle. There is **no** shared rocket sound and **no**
    /// mount-specific fallback: absent means silent, which is what the
    /// static callers prove — a firing caller must pick a declared slot or
    /// explicitly pass `-1`.
    #[must_use]
    pub fn launch_sound(&self) -> Option<&str> {
        self.fire
            .as_ref()
            .and_then(|block| block.sounds.first())
            .map(String::as_str)
    }

    /// Every launch sound entry the record declares (retail: at most one).
    #[must_use]
    pub fn launch_sounds(&self) -> &[String] {
        self.fire
            .as_ref()
            .map_or(&[], |block| block.sounds.as_slice())
    }

    /// The muzzle visual that takes precedence at launch: `FIRE`'s
    /// `EFFECT`. When declared, the engine plays it and skips the
    /// `ANIMATION` slots entirely (`0x5af3e6`).
    #[must_use]
    pub fn launch_effect(&self) -> Option<&str> {
        self.fire.as_ref().and_then(|block| block.effect.as_deref())
    }

    /// The launch animation the engine plays when no `EFFECT` is declared:
    /// the `FIRE` `ANIMATION` slot index `weapon + 0xec` selects — slot `0`
    /// after every ordinary shot.
    #[must_use]
    pub fn launch_animation(&self) -> Option<&str> {
        self.fire
            .as_ref()
            .and_then(|block| block.animations.first())
            .map(String::as_str)
    }

    /// The projectile's own visual: `FLYOUT`'s `MODEL`, attached to the
    /// spawned round's scene node at [`ORIGINAL_WEAPON_FLYOUT_MODEL_ATTACH_VA`].
    #[must_use]
    pub fn flyout_model(&self) -> Option<&str> {
        self.flyout
            .as_ref()
            .and_then(|block| block.model.as_deref())
    }

    /// The looping animation of the `FLYOUT` `MODEL`
    /// (`FLYOUT>MODEL_ANIMATION`, block `+0x18`).
    #[must_use]
    pub fn flyout_model_animation(&self) -> Option<&str> {
        self.flyout
            .as_ref()
            .and_then(|block| block.model_animation.as_deref())
    }

    /// The declared flight loop: `FLYOUT`'s `SOUND` entries. They are
    /// parsed into the block (`+0x124` count, `+0x128` array); the weapon
    /// module's consumer for them was not located statically (measured gap,
    /// recorded in the finding).
    #[must_use]
    pub fn flyout_sounds(&self) -> &[String] {
        self.flyout
            .as_ref()
            .map_or(&[], |block| block.sounds.as_slice())
    }

    /// The mount's `LOOPED_SOUND_NAME` — the gun loop, which ordnance
    /// records never declare.
    #[must_use]
    pub fn mount_looped_sound(&self) -> Option<&str> {
        self.looped_sound_name.as_deref()
    }

    /// The `IMPACT` row for `surface`, or the `default` row when the
    /// record declares none under that name — the engine copies row 0 into
    /// undeclared soils verbatim ([`ORIGINAL_WEAPON_IMPACT_BLOCK_VA`]).
    #[must_use]
    pub fn impact_row(&self, surface: &str) -> Option<&OriginalImpactRow> {
        self.impact_rows
            .iter()
            .find(|row| row.surface == surface)
            .or_else(|| self.default_impact_row())
    }

    /// The `default` row of `IMPACT` — row 0 of the engine's table.
    #[must_use]
    pub fn default_impact_row(&self) -> Option<&OriginalImpactRow> {
        self.impact_rows.iter().find(|row| row.surface == "default")
    }

    /// The measured launch-presentation ids for the runtime
    /// [`cs_sim::weapons::ordnance::OrdnanceMedia`] contract: the declared
    /// `FLYOUT` `MODEL` as the visual and the declared `FIRE` `SOUND`
    /// entry as the sound.
    ///
    /// The ids are built from the declared names — measured, never
    /// invented — at [`ObservedTool`] strength with `claim_id` as their
    /// claim: `hardpoint_equipment/<model>` and `sound/<name>`. A record
    /// that declares no `FLYOUT` `MODEL` or no `FIRE` `SOUND` entry keeps
    /// the field `Unknown` with the reason — the boundary at
    /// `cs_app::ordnance::lower_media` then refuses it rather than
    /// fabricating a resource, which is the measured behavior anyway: no
    /// `FIRE` `SOUND` means a silent launch.
    #[must_use]
    pub fn measured_ordnance_media(&self, claim_id: &ClaimId) -> DeclaredOrdnanceMedia {
        let known = |kind: ContentKind, name: &str| {
            Resolved::Known(Known::new(
                ContentId::from_source(kind, name)
                    .expect("a declared media name is a valid content key"),
                Provenance::new(claim_id.clone(), ClaimStatus::ObservedTool, None)
                    .expect("an ObservedTool provenance needs no source span"),
            ))
        };
        let visual = match self.flyout_model() {
            Some(model) => known(ContentKind::HardpointEquipment, model),
            None => Resolved::unknown(
                claim_id.clone(),
                "the record declares no FLYOUT MODEL, so the spawned round carries no \
                 visual of its own",
            )
            .expect("the reason is non-empty"),
        };
        // No `FIRE` `SOUND` entry is a measured silence (playback gate
        // 0x593b80 early-outs on a missing handle), carried as `None` and
        // never defaulted.
        let sound = self
            .launch_sound()
            .map(|name| known(ContentKind::Sound, name));
        DeclaredOrdnanceMedia {
            visual,
            sound,
            particles: None,
        }
    }
}

/// Decodes one keyed effect block (`FIRE`, `FLYOUT`, or one `IMPACT` row)
/// into its declared names.
fn effect_block(children: &[crate::original_airframe::SpannedValue]) -> OriginalEffectBlock {
    let mut block = OriginalEffectBlock::default();
    for field in children.chunks(2) {
        let [key, value] = field else { continue };
        let Some(key) = key.as_text() else { continue };
        block.keys.insert(key.to_owned());
        let Some(list) = value.as_list() else {
            continue;
        };
        let texts = || {
            list.iter()
                .filter_map(|child| child.as_text().map(str::to_owned))
        };
        match key {
            "ANIMATION" => block.animations = texts().collect(),
            "SOUND" => block.sounds = texts().collect(),
            "BOUNCE_SOUND" => block.bounce_sounds = texts().collect(),
            "ANIMATION_ATTACHED" => block.animation_attached = texts().next(),
            "MODEL_ANIMATION" => block.model_animation = texts().next(),
            "SURFACE_ANIMATION" => block.surface_animation = texts().next(),
            "ANIMATION_MIN_RANGE" => block.animation_min_range = texts().next(),
            "EFFECT" => block.effect = texts().next(),
            "MODEL" => block.model = texts().next(),
            "RANDOM_ROTATE" => {
                block.random_rotate = list
                    .first()
                    .and_then(crate::original_airframe::SpannedValue::as_scalar_u32)
                    .map(|value| value != 0);
            }
            _ => {}
        }
    }
    block
}

/// One record's measured effect-class declarations — the pure half of the
/// reader, callable on any decoded record body.
#[must_use]
pub fn weapon_media(
    name: &str,
    body: &[crate::original_airframe::SpannedValue],
) -> OriginalWeaponMedia {
    let mut media = OriginalWeaponMedia {
        record: name.to_owned(),
        ..OriginalWeaponMedia::default()
    };
    for field in body.chunks(2) {
        let [key, value] = field else { continue };
        let Some(key) = key.as_text() else { continue };
        media.keys.insert(key.to_owned());
        match key {
            "FIRE" | "FLYOUT" => {
                let block = value
                    .as_list()
                    .map_or_else(OriginalEffectBlock::default, effect_block);
                if key == "FIRE" {
                    media.fire = Some(block);
                } else {
                    media.flyout = Some(block);
                }
            }
            "IMPACT" => {
                let Some(rows) = value.as_list() else {
                    continue;
                };
                for pair in rows.chunks(2) {
                    let [surface, row] = pair else { continue };
                    let Some(surface) = surface.as_text() else {
                        continue;
                    };
                    media.impact_rows.push(OriginalImpactRow {
                        surface: surface.to_owned(),
                        block: row
                            .as_list()
                            .map_or_else(OriginalEffectBlock::default, effect_block),
                    });
                }
            }
            "DESTROY_ANIMATION" => {
                media.destroy_animation = value
                    .as_list()
                    .and_then(|list| list.first())
                    .and_then(crate::original_airframe::SpannedValue::as_text)
                    .map(str::to_owned);
            }
            "LOOPED_SOUND_NAME" => {
                media.looped_sound_name = value
                    .as_list()
                    .and_then(|list| list.first())
                    .and_then(crate::original_airframe::SpannedValue::as_text)
                    .map(str::to_owned);
            }
            _ => {}
        }
    }
    media
}

/// Reads the measured effect-class declarations of every `wep_` record in
/// the installation's `weapons.zrd` (task T452-R.03-C).
///
/// # Errors
///
/// [`crate::original_airframe::OriginalImportError`] as
/// [`read_original_weapon_records`].
pub fn read_original_weapon_media(
    install_root: &std::path::Path,
) -> Result<Vec<OriginalWeaponMedia>, crate::original_airframe::OriginalImportError> {
    let document = read_member_document(install_root, ORIGINAL_WEAPONS_MEMBER)?;
    let mut found = Vec::new();
    collect_wep_records(document.root(), &mut found);
    if found.is_empty() {
        return Err(crate::original_airframe::OriginalImportError::Shape {
            detail: "no `wep_` record was found".to_owned(),
        });
    }
    Ok(found
        .into_iter()
        .map(|(name, body)| weapon_media(name, body))
        .collect())
}

// ------------------- T452-REG-TANGLER-VOCAB: the tangler field -----------
//
// Task #2179 measured statically in the decrypted engine image
// (`docs/findings/2026-10-28-t452-tangler-effect.md`, rows `[[tangler_fact]]`
// in `data/owner-static/engine-facts.toml`) what a zero-damage `TANGLER` does:
// the weapon creates a persistent area field at fire time whose tick re-applies
// a per-hit routine to every live actor inside it, owner included, and that
// routine tangles the victim for a distance-dependent duration. This section
// is the pure-data vocabulary for that measurement; the runtime that applies
// it per tick is a separate task.

/// Claim: what the force scalar `[ebp-8]` of the actor force routine `0x48fc40`
/// is in physical terms. The tangle zeroes it; whether that is thrust, drag or
/// something else is not decoded, so the status is named after the store only.
pub const T452_REG_TANGLER_FORCE_SCALAR: &str = "t452_reg.limit.tangler_force_scalar_meaning";

/// The name of the tangled status: exactly what is measured, no more.
pub const TANGLE_STATUS_NAME: &str = "force scalar [ebp-8] of 0x48fc40 suppressed";

/// The image's default `RADIUS` when a record states none, metres (`0x4ba870`).
pub const TANGLER_IMAGE_DEFAULT_RADIUS_M: f64 = 10.0;

/// The constant `1.0` of the duration formula, at `0x6032dc`.
pub const TANGLE_UNIT: f64 = 1.0;

/// The multiplier `10.0` of the duration formula, seconds, at `0x62b124`.
pub const TANGLE_FALLOFF_SCALE_S: f64 = 10.0;

/// The duration floor `2.0`, seconds, at `0x62b120`.
pub const TANGLE_FLOOR_S: f64 = 2.0;

/// Where a re-hit of an already tangled actor leaves the timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TangleReHit {
    /// The timer becomes the larger of its current value and the new duration;
    /// it is never shortened (`0x4b16f6..0x4b170b`).
    RaiseToMax,
}

/// What advances the tangle timer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TangleCountdown {
    /// The timer loses the frame time each update and the status clears when it
    /// is no longer positive (`0x48fda5..0x48fdcb`).
    FrameTime,
}

/// The per-victim timed status a tangler field applies.
#[derive(Clone, Debug, PartialEq)]
pub struct TangleStatus {
    /// [`TANGLE_STATUS_NAME`].
    pub name: &'static str,
    /// `RADIUS`, unsquared, metres: the divisor of the duration formula.
    pub radius_m: f64,
    /// The duration floor, seconds.
    pub floor_s: f64,
    /// The `10.0` multiplier, seconds.
    pub falloff_scale_s: f64,
    /// The `1.0` the ratio is subtracted from.
    pub unit: f64,
    /// What a re-hit does.
    pub on_rehit: TangleReHit,
    /// What counts the timer down.
    pub countdown: TangleCountdown,
    /// The claim kept for the unresolved physical meaning of the scalar.
    pub claim: &'static str,
    /// The window the duration formula was read from.
    pub pin: StaticProvenance,
}

impl TangleStatus {
    /// The duration a hit at squared distance `d2` (square metres, as the field
    /// tick passes it) gives: `max(floor, 10 * (1 - d2 / RADIUS))`, `RADIUS`
    /// unsquared.
    #[must_use]
    pub fn duration_for(&self, d2: f64) -> f64 {
        self.floor_s
            .max(self.falloff_scale_s * (self.unit - d2 / self.radius_m))
    }

    /// The timer after a hit at squared distance `d2` on an actor whose timer
    /// reads `current_s` (zero when not tangled): never shorter than before.
    #[must_use]
    pub fn after_hit(&self, current_s: f64, d2: f64) -> f64 {
        match self.on_rehit {
            TangleReHit::RaiseToMax => current_s.max(self.duration_for(d2)),
        }
    }
}

/// The persistent area field a `TANGLER` record declares, measured.
#[derive(Clone, Debug, PartialEq)]
pub struct TanglerFieldDeclaration {
    /// The record name.
    pub record: String,
    /// The field's radius, metres.
    pub radius_m: f64,
    /// Where the radius came from.
    pub radius_source: MeasuredAreaSource,
    /// How long the field lives, seconds.
    pub lifetime_s: f64,
    /// Where the lifetime came from.
    pub lifetime_source: MeasuredAreaSource,
    /// The field is created at fire time at the record's flight position.
    pub created_at_fire_time: bool,
    /// The field re-applies its hit to every live actor inside it every tick.
    pub reapplied_every_tick: bool,
    /// The owner is hit like any other actor.
    pub includes_owner: bool,
    /// The status each victim receives.
    pub status: TangleStatus,
    /// The window the field's tick was read from.
    pub pin: StaticProvenance,
}

impl TanglerFieldDeclaration {
    /// Whether an actor at squared distance `d2` is inside the field
    /// (`d2 < RADIUS^2`).
    #[must_use]
    pub fn contains(&self, d2: f64) -> bool {
        d2 < self.radius_m * self.radius_m
    }
}

/// Lowers a `TANGLER` record into its measured field and status declaration.
///
/// `record_radius_m` is the `RADIUS` the record states, `None` when it states
/// none (the image then applies [`TANGLER_IMAGE_DEFAULT_RADIUS_M`]; no retail
/// `wep_` record states one). Returns `None` for a record without `TANGLER`.
#[must_use]
pub fn lower_tangler_field(
    row: &OriginalComponentLaunch,
    record_radius_m: Option<f64>,
) -> Option<TanglerFieldDeclaration> {
    let tangler = row.statuses.iter().find(|s| s.key == "TANGLER")?;
    let (radius_m, radius_source) = match record_radius_m {
        Some(radius) => (radius, MeasuredAreaSource::RecordKey { key: "RADIUS" }),
        None => (
            TANGLER_IMAGE_DEFAULT_RADIUS_M,
            MeasuredAreaSource::ImageDefault {
                value: TANGLER_IMAGE_DEFAULT_RADIUS_M,
            },
        ),
    };
    Some(TanglerFieldDeclaration {
        record: row.record.clone(),
        radius_m,
        radius_source,
        lifetime_s: tangler.duration_s.unwrap_or(IMAGE_DEFAULT_TIME_S),
        lifetime_source: measured_time_source(tangler.duration_s),
        created_at_fire_time: true,
        reapplied_every_tick: true,
        includes_owner: true,
        status: TangleStatus {
            name: TANGLE_STATUS_NAME,
            radius_m,
            floor_s: TANGLE_FLOOR_S,
            falloff_scale_s: TANGLE_FALLOFF_SCALE_S,
            unit: TANGLE_UNIT,
            on_rehit: TangleReHit::RaiseToMax,
            countdown: TangleCountdown::FrameTime,
            claim: T452_REG_TANGLER_FORCE_SCALAR,
            pin: StaticProvenance::new(0x004b_9dfa, 36),
        },
        pin: StaticProvenance::new(0x004b_950f, 26),
    })
}

// ------------------- T452-REG-SMOKE-STUN: what the smoke emitter's stun does -----
//
// Task #2187 measured statically in the owner-supplied decrypted engine image
// (`$CS_ENGINE_IMAGE`, base `0x400000`) the two routines the emitter's per-step
// update (`0x4b8fd0`) calls for an actor inside the stun range and cone
// (`docs/findings/2026-10-28-t452-reg-smoke-radius.md`, `docs/findings/
// 2026-10-28-t452-reg-smoke-stun.md`). Nothing here is `verified_original`.
//
// * An AI actor: `0x4200d0` is the same incapacitation `SONIC`/`FLASH` apply
//   ([`MeasuredActorIncapacitation`]): mode word `4` at `+0x358`, six control
//   words zeroed, and the stored clock + the argument at `+0xc0`. The argument
//   is `smokescreen_stun_interval`.
// * The player: `0x42e9d0` starts a screen tint of a fixed colour for `2.0 s`;
//   the player's controls are not touched. The emitter re-arms it through its
//   own timer at node `+0x14`.

/// The window in which the emitter picks its victims (`0x4b90a6..0x4b9178`).
const SMOKE_STUN_PICK_PIN: StaticProvenance = StaticProvenance::new(0x004b_90a6, 210);

/// How the emitter treats the player, measured (`0x4b917f..0x4b91da`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmokeStunPlayerScreen {
    /// How long the tint lasts, seconds (`push 0x40000000`, `0x4b91cc`).
    pub tint_seconds: f64,
    /// The emitter's own re-arm timer is stored as this at each trigger,
    /// seconds (`0x4b91d3`).
    pub rearm_seconds: f64,
    /// The player is tinted only while the re-arm timer is below this
    /// (`0x4b9182`, constant at `0x6032e0`).
    pub rearm_gate_seconds: f64,
    /// The tint colour (`0x4b9192..0x4b91a6`), red, green, blue.
    pub colour: [f64; 3],
    /// The tint scalar when the re-arm timer is positive (`0x4b91ad`).
    pub scalar_rearmed: f64,
    /// The tint scalar when the re-arm timer is not positive (`0x4b91bb`).
    pub scalar_first: f64,
    /// The player's controls are not zeroed by this path.
    pub control_loss: bool,
    /// The window of the trigger.
    pub trigger_pin: StaticProvenance,
    /// The window of the screen routine's state stores.
    pub routine_pin: StaticProvenance,
}

/// How the emitter treats every other actor, measured (`0x4b915e..0x4b9178`,
/// `0x4200d0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmokeStunAiEffect {
    /// The incapacitation it applies; its duration is the stun interval.
    pub incapacitation: MeasuredActorIncapacitation,
    /// The gate the victim passes in the emitter and in `0x4200d0`.
    pub gate: &'static str,
}

/// The smoke emitter's stun rule: who it reaches and what it does, with the
/// three `player.zrd` globals it reads left as keys (their retail values come
/// from `OriginalDocuments::smokescreen_stun`).
#[derive(Clone, Debug, PartialEq)]
pub struct SmokeStunRule {
    /// The range key (`0x71c3dc`).
    pub range_key: &'static str,
    /// The image's range when the key is absent, metres.
    pub range_image_default_m: f64,
    /// The cone-angle key (`0x71c3e0`, stored as `cos(angle * pi / 180 * 0.5)`).
    pub angle_key: &'static str,
    /// The image's cosine when the key is absent.
    pub cone_cosine_image_default: f64,
    /// The interval key (`0x71c3e4`).
    pub interval_key: &'static str,
    /// The image's interval when the key is absent, seconds.
    pub interval_image_default_s: f64,
    /// The owner (the node's owner actor) is never stunned (`0x4b90bc`).
    pub excludes_owner: bool,
    /// The emitter acts on every step it is alive, not once.
    pub every_step: bool,
    /// What the player gets.
    pub player: SmokeStunPlayerScreen,
    /// What every other actor gets.
    pub ai: SmokeStunAiEffect,
    /// The window in which victims are picked.
    pub pin: StaticProvenance,
}

impl SmokeStunRule {
    /// The rule as measured.
    #[must_use]
    pub fn measured() -> Self {
        let f = |bits: u32| f64::from(f32::from_bits(bits));
        Self {
            range_key: "smokescreen_stun_range",
            range_image_default_m: f(0x4348_0000),
            angle_key: "smokescreen_stun_angle",
            cone_cosine_image_default: 0.8,
            interval_key: "smokescreen_stun_interval",
            interval_image_default_s: 3.0,
            excludes_owner: true,
            every_step: true,
            player: SmokeStunPlayerScreen {
                tint_seconds: 2.0,
                rearm_seconds: 2.0,
                rearm_gate_seconds: 0.5,
                colour: [f(0x3e4c_cccd), f(0x3e94_7ae1), f(0x3e14_7ae1)],
                scalar_rearmed: f(0x3f66_6666),
                scalar_first: f(0x3f78_51ec),
                control_loss: false,
                trigger_pin: StaticProvenance::new(0x004b_917f, 92),
                routine_pin: StaticProvenance::new(0x0042_e9d0, 16),
            },
            ai: SmokeStunAiEffect {
                incapacitation: MeasuredActorIncapacitation {
                    mode: 4,
                    control_words_zeroed: 6,
                    ends_at: "the shared clock (0x71c470) reaching the stored clock + \
                              smokescreen_stun_interval at the victim's +0xc0 clears mode 4",
                    gate_pin: StaticProvenance::new(0x0042_00ea, 24),
                    store_pin: StaticProvenance::new(0x0042_0154, 54),
                },
                gate: "not the owner, victim +0x91d == 0, not the player, victim +0xf8 == 0, \
                       and in 0x4200d0 victim +0x67c in {0, 4}",
            },
            pin: SMOKE_STUN_PICK_PIN,
        }
    }

    /// The rule with the three globals read from the installation.
    #[must_use]
    pub fn resolve(
        &self,
        globals: &crate::original_airframe::OriginalSmokescreenStun,
    ) -> ResolvedSmokeStun {
        ResolvedSmokeStun {
            range_m: globals.range_m,
            cone_cosine: (globals.cone_angle_deg.to_radians() * 0.5).cos(),
            interval_s: globals.interval_s,
            player: self.player,
        }
    }

    /// The rule with every global at the image's own default.
    #[must_use]
    pub fn at_image_defaults(&self) -> ResolvedSmokeStun {
        ResolvedSmokeStun {
            range_m: self.range_image_default_m,
            cone_cosine: self.cone_cosine_image_default,
            interval_s: self.interval_image_default_s,
            player: self.player,
        }
    }
}

/// The smoke stun rule with its globals resolved to numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedSmokeStun {
    /// The stun range, metres.
    pub range_m: f64,
    /// The cosine the facing dot product must exceed.
    pub cone_cosine: f64,
    /// How long a struck AI actor is incapacitated, seconds.
    pub interval_s: f64,
    /// What the player gets.
    pub player: SmokeStunPlayerScreen,
}

/// What one emitter step does to the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmokeStunPlayerTint {
    /// The tint duration, seconds.
    pub seconds: f64,
    /// The tint scalar.
    pub scalar: f64,
    /// The new value of the emitter's re-arm timer, seconds.
    pub rearm_seconds: f64,
}

impl ResolvedSmokeStun {
    /// Whether an actor at `distance_m` from the owner, whose direction from
    /// the owner has cosine `facing_dot` with the owner's forward axis, is
    /// reached: strictly inside the range and strictly inside the cone.
    #[must_use]
    pub fn reaches(&self, distance_m: f64, facing_dot: f64) -> bool {
        distance_m < self.range_m && facing_dot > self.cone_cosine
    }

    /// The tint a reached player gets when the emitter's re-arm timer reads
    /// `rearm_s`, `None` while it has not run down below the gate.
    #[must_use]
    pub fn player_tint(&self, rearm_s: f64) -> Option<SmokeStunPlayerTint> {
        (rearm_s < self.player.rearm_gate_seconds).then_some(SmokeStunPlayerTint {
            seconds: self.player.tint_seconds,
            scalar: if rearm_s > 0.0 {
                self.player.scalar_rearmed
            } else {
                self.player.scalar_first
            },
            rearm_seconds: self.player.rearm_seconds,
        })
    }

    /// The clock value a reached AI actor's mode-4 stun ends at, given the
    /// shared clock `now_s`.
    #[must_use]
    pub fn ai_stun_ends_at(&self, now_s: f64) -> f64 {
        now_s + self.interval_s
    }
}
