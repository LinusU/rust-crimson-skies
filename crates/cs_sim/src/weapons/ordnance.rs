//! Rockets, special ordnance, counter-effects and nitro: the typed
//! behavior and effect registry (F28-A) and the per-tick ordnance runtime
//! (F28-B).
//!
//! Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
//! stages `### F28-A` and `### F28-B`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`, sections "Boost and special models"
//! and "Collision and ballistic tests".
//!
//! This module is the **runtime** half of the ordnance contract: the records
//! one session resolves against, with no Bevy, Avian, renderer or file
//! dependency (`docs/01-ARCHITECTURE.md`). The declared,
//! provenance-carrying half is `cs_content::ordnance`; the conversion
//! boundary is `cs_app::ordnance`.
//!
//! # What is defined here and what is not
//!
//! Stage F28-A defines the *typed* registry, its exhaustive shape and the
//! pure queries a session runs against it — never a schedule, a body or an
//! effect consumer:
//!
//! * [`OrdnanceComponent`] — the sum of the two kinds of hardpoint
//!   component the sheet names: a launched [`ProjectileOrdnance`] with a
//!   fuse, a lifetime and a guidance rule, and a [`NitroOrdnance`]
//!   booster with capacity, consumption and thrust. Nitro is **not** an
//!   Xbox-only feature (non-negotiable 2): the PC manual lists a nitro
//!   activation control, so it is a first-class registry entry.
//! * [`ProjectileOrdnance`] declares every field the sheet's deliverable
//!   names, and keeps them separate: launch geometry, stack capacity and
//!   mass, arming, fuse, guidance, lifetime, area effect, damage and status
//!   channels, and media.
//! * [`OrdnanceFamily`] — the six families the sheet names as *discovery
//!   leads*, and [`ProjectileOrdnance::try_new`] refuses a family whose
//!   guidance or fuse contradicts it. That refusal is how non-negotiable 1
//!   ("Do not substitute every rocket with one homing missile") becomes
//!   executable rather than a comment: a direct explosive or flak shell
//!   cannot be declared as a seeker, and an area-denial item cannot be
//!   declared as a guided one.
//! * [`OrdnanceRegistry`] — the exhaustive registry of installed hardpoint
//!   components, with [`OrdnanceRegistry::resolve_installation`] refusing
//!   an equipment id it does not know, which is the import-side half of
//!   non-negotiable 5 ("no unsupported custom plane can bypass the shop
//!   through an import").
//! * [`closest_approach`] and [`OrdnanceState::fuse_decision`] — the swept
//!   relative-distance proximity query and the arming-respecting fuse
//!   decision. This is AC01's minimum scenario at this stage.
//! * [`GuidanceTracker`] — the lost-target contract of non-negotiable 4: a
//!   tracker that cannot re-acquire a destroyed target or cross a session.
//! * [`StatusEffectLedger`] — bounded, tick-counted status effects over
//!   stable recipient ids, separate from any visual particle.
//! * [`NitroLedger`] — capacity, accepted activation, thrust modification
//!   and consumption, all measured in whole ticks of the declared
//!   [`crate::time::TickRate`]. No method takes a [`std::time::Duration`],
//!   and no output carries a pose, so no caller can move an airframe or
//!   scale a render frame through nitro (AC04).
//! * [`OrdnanceRuntime`] — the F28-B per-tick production path that owns the
//!   live items and drives every mechanism above: launch geometry and swept
//!   motion, the direct and proximity fuse decisions, the once-only routing
//!   of a triggered item into [`crate::damage::HitEvent`]s, the
//!   destruct/session-safe guidance loss, the status ledger and nitro. It is
//!   session-confined, and dropping it is the whole teardown.
//!
//! What F28-C and F28-D still own, and what is therefore deliberately
//! absent: the launch gesture and hardpoint firing order in the cockpit, the
//! ECS/Avian binding of an in-flight item, the status-effect consumer in the
//! flight model, the nitro consumer of
//! [`crate::flight::BoostParameters`], the loadout/shop validation that
//! consumes [`EquipmentRules`], and the original ordnance catalogue audit.
//!
//! # Designed vocabulary, not original data
//!
//! Every family name, hardpoint kind, arming rule, fuse geometry, guidance
//! rule, lost-target behavior, status effect, media namespace, capacity,
//! consumption, thrust and tradeoff here is **newly authored project
//! design**, carried by synthetic fixture values. The original PC ordnance
//! catalogue, its trigger distances, arming delays, lifetimes, blast radii,
//! nitro capacity and every original rule behind them are **unmeasured** —
//! F28's "Research boundary" and this stage having no `CS_GAME_DIR` at all.
//! Nothing here claims `verified_original`; the catalogue audit is F28-D.
//! The unknowns are listed in
//! `docs/findings/2026-10-01-f28-a-ordnance-behavior-and-effect-registry.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::{SpaceError, WorldPosition};

use crate::damage::{ActorId, DamageNodeKey, HitEvent, HitEventError, HitEventId, SystemKind};
use crate::environment::{air_relative_velocity_m_s, world_velocity_from_air_m_s};
use crate::time::TickRate;
use crate::weapons::guns::{
    InheritanceRule, MountTransform, ProjectileId, ProjectileSegment, SweptHit,
    WEAPON_DAMAGE_CHANNELS, WeaponDamage,
};

// ---------------------------------------------------------------- identity ----

/// One hardpoint ordnance component's catalog identifier.
///
/// A `weapon`-namespace [`ContentId`], deliberately **not** an enum. The
/// sheet's families are *discovery leads* ("may include direct explosive,
/// proximity/flak, guided or tagged-target weapons, area-denial/engine
/// effects, aerial torpedoes and nitro boosters") and the catalogue F28-D
/// audits is unmeasured, so a closed `Ord...Kind` enum here would be a
/// fabricated roster that a later original id could not enter.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrdnanceId(ContentId);

impl OrdnanceId {
    /// Wraps an ordnance content id, validating its namespace.
    ///
    /// # Errors
    ///
    /// [`OrdnanceIdError::KindMismatch`] when the id is not in the `weapon`
    /// namespace.
    pub fn try_new(id: ContentId) -> Result<Self, OrdnanceIdError> {
        if id.kind() != ContentKind::Weapon {
            return Err(OrdnanceIdError::KindMismatch { id });
        }
        Ok(Self(id))
    }

    /// The catalog id of the component.
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.0
    }

    /// The `namespace/key` text of the component.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for OrdnanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// Why an [`OrdnanceId`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrdnanceIdError {
    /// The content id is not in the `weapon` namespace.
    KindMismatch {
        /// The offending id.
        id: ContentId,
    },
}

impl fmt::Display for OrdnanceIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindMismatch { id } => {
                write!(f, "ordnance id {id} is not in the weapon namespace")
            }
        }
    }
}

impl std::error::Error for OrdnanceIdError {}

/// The behavior family one component belongs to.
///
/// **Designed**, from the sheet's list of families to discover. It is a
/// *label with teeth* rather than documentation: [`ProjectileOrdnance`]
/// validates the family's rules against the component's declared guidance
/// and fuse, so the six leads cannot collapse into one homing missile
/// (non-negotiable 1). A family the original turns out not to have simply
/// stays unused, and one it has that is not here is added when F28-D's audit
/// finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OrdnanceFamily {
    /// A shot that flies a declared line and bursts on contact.
    DirectExplosive,
    /// A shell that bursts when a target comes close enough, unguided.
    ProximityFlak,
    /// A seeker that tracks one designated target.
    GuidedRocket,
    /// An item whose effect is a bounded area — smoke, flame, denial —
    /// rather than a hit.
    AreaDenialEngine,
    /// A long-lived aerodynamic weapon dropped or launched at a target.
    AerialTorpedo,
    /// A continuous booster: capacity, consumption, extra thrust, no fuse.
    NitroBooster,
}

impl OrdnanceFamily {
    /// Every family, in a stable order.
    pub const ALL: &'static [OrdnanceFamily] = &[
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

impl fmt::Display for OrdnanceFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where a launcher sits on the airframe.
///
/// **Designed**, mirroring the F27 per-mount discipline: a weapon-mount
/// damage node disables the component on that mount and no other, so a
/// destroyed wing launcher cannot silence a fuselage one (F27
/// non-negotiable 2, applied to ordnance here).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HardpointKind {
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

impl HardpointKind {
    /// Every hardpoint kind, in a stable order.
    pub const ALL: &'static [HardpointKind] = &[
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

    /// Whether this hardpoint sits on a wing.
    #[must_use]
    pub const fn is_wing(self) -> bool {
        matches!(self, Self::WingLeft | Self::WingRight | Self::Underslung)
    }
}

impl fmt::Display for HardpointKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// ----------------------------------------------------------- launch geometry ----

/// The declared numbers a launch leaves a hardpoint with.
///
/// The **pose** is not here: it is a [`MountTransform`] supplied by the
/// live aircraft hierarchy, exactly as for a gun, so this stage invents no
/// launch offset and no center-screen origin. What the geometry *does*
/// declare is the mount the component occupies, where that mount is, how
/// fast the item leaves, how much of the launcher's velocity it inherits and
/// how many ticks after the launch intent it actually releases.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchGeometry {
    mount: DamageNodeKey,
    hardpoint: HardpointKind,
    launch_speed_mps: f64,
    inheritance: InheritanceRule,
    release_delay_ticks: u64,
}

impl LaunchGeometry {
    /// Assembles a launch geometry.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError::NonFiniteLaunchSpeed`] or
    /// [`OrdnanceDefinitionError::NonPositiveLaunchSpeed`].
    pub fn try_new(
        mount: DamageNodeKey,
        hardpoint: HardpointKind,
        launch_speed_mps: f64,
        inheritance: InheritanceRule,
        release_delay_ticks: u64,
    ) -> Result<Self, OrdnanceDefinitionError> {
        if !launch_speed_mps.is_finite() {
            return Err(OrdnanceDefinitionError::NonFiniteLaunchSpeed);
        }
        if launch_speed_mps <= 0.0 {
            return Err(OrdnanceDefinitionError::NonPositiveLaunchSpeed { launch_speed_mps });
        }
        if let InheritanceRule::Fraction { share } = inheritance
            && (!share.is_finite() || !(0.0..=1.0).contains(&share))
        {
            return Err(OrdnanceDefinitionError::InvalidInheritanceShare { share });
        }
        Ok(Self {
            mount,
            hardpoint,
            launch_speed_mps,
            inheritance,
            release_delay_ticks,
        })
    }

    /// The weapon-mount damage node this component occupies.
    #[must_use]
    pub const fn mount(&self) -> &DamageNodeKey {
        &self.mount
    }

    /// Where on the airframe that mount sits.
    #[must_use]
    pub const fn hardpoint(&self) -> HardpointKind {
        self.hardpoint
    }

    /// The speed the item leaves the hardpoint at, in meters per second.
    #[must_use]
    pub const fn launch_speed_mps(&self) -> f64 {
        self.launch_speed_mps
    }

    /// The declared share of the launcher's velocity the item inherits.
    #[must_use]
    pub const fn inheritance(&self) -> InheritanceRule {
        self.inheritance
    }

    /// Ticks between the launch intent and the actual release.
    #[must_use]
    pub const fn release_delay_ticks(&self) -> u64 {
        self.release_delay_ticks
    }

    /// The item's world velocity at release, composed from the supplied
    /// mount pose and this geometry's declared rule.
    ///
    /// The composition is F27's, not a second one: the declared inheritance
    /// share of the launcher's world velocity plus the declared launch speed
    /// along the mount's forward axis. No drag, no gravity and no release
    /// impulse beyond those two terms is applied here.
    #[must_use]
    pub fn release_velocity_mps(&self, transform: &MountTransform) -> [f64; 3] {
        transform.world_velocity_mps(self.inheritance, self.launch_speed_mps)
    }
}

/// How much ordnance one hardpoint carries and what it weighs.
///
/// Capacity is *per launcher* in units, not a rate: it is the number the
/// launcher's stack holds before a reload, which is what loadout validation
/// and the shop's ammo count read (F28 non-negotiable 5). The mass is per
/// unit, because that is the term the F24 loadout-mass record integrates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StackLoad {
    capacity_units: u64,
    unit_mass_kg: f64,
}

impl StackLoad {
    /// Assembles a stack load, refusing a zero capacity or a corrupt mass.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError::ZeroStackCapacity`],
    /// [`OrdnanceDefinitionError::NonFiniteUnitMass`] or
    /// [`OrdnanceDefinitionError::NegativeUnitMass`].
    pub fn try_new(
        capacity_units: u64,
        unit_mass_kg: f64,
    ) -> Result<Self, OrdnanceDefinitionError> {
        if capacity_units == 0 {
            return Err(OrdnanceDefinitionError::ZeroStackCapacity);
        }
        if !unit_mass_kg.is_finite() {
            return Err(OrdnanceDefinitionError::NonFiniteUnitMass);
        }
        if unit_mass_kg < 0.0 {
            return Err(OrdnanceDefinitionError::NegativeUnitMass { unit_mass_kg });
        }
        Ok(Self {
            capacity_units,
            unit_mass_kg,
        })
    }

    /// How many units the launcher's stack holds.
    #[must_use]
    pub const fn capacity_units(&self) -> u64 {
        self.capacity_units
    }

    /// The mass of one unit, in kilograms.
    #[must_use]
    pub const fn unit_mass_kg(&self) -> f64 {
        self.unit_mass_kg
    }

    /// The declared total mass of a full stack, in kilograms.
    #[must_use]
    pub fn loaded_mass_kg(&self) -> f64 {
        self.unit_mass_kg * self.capacity_units as f64
    }
}

// ------------------------------------------------------------------ arming ----

/// When an item's fuse becomes live.
///
/// A proximity fuse that triggers on the launch tick detonates the item in
/// its own launcher's lap. Arming is therefore a *separate declared field*
/// from the fuse geometry, and [`OrdnanceState::fuse_decision`] refuses
/// every trigger before it — AC01's "triggers for a fast near-pass **but
/// not before arming**".
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ArmingRule {
    /// Never arms. The item flies inert: a stowed or inertially released
    /// component that cannot detonate, whatever its fuse geometry says.
    Disarmed,
    /// Arms once the item has lived this many ticks after launch.
    AfterTicks(u64),
    /// Arms once the item has travelled this many meters from its release
    /// point. The distance is accumulated from the item's own swept
    /// segments, not from a speed, so a launch that never leaves the rail
    /// never arms.
    AfterTravelMetres(f64),
}

impl ArmingRule {
    /// Every arming rule, in a stable order.
    pub const ALL: &'static [ArmingRule] = &[
        Self::Disarmed,
        Self::AfterTicks(1),
        Self::AfterTravelMetres(1.0),
    ];

    /// The stable label used in reports.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Disarmed => "disarmed".to_owned(),
            Self::AfterTicks(ticks) => format!("after_ticks({ticks})"),
            Self::AfterTravelMetres(metres) => format!("after_travel_m({metres})"),
        }
    }

    /// Whether the item is armed after `ticks_live` ticks and
    /// `travelled_m` meters.
    ///
    /// A rule whose threshold is unknown or unusable refuses to arm: an
    /// arming rule is never satisfied by accident.
    #[must_use]
    pub fn is_armed(&self, ticks_live: u64, travelled_m: f64) -> bool {
        match *self {
            Self::Disarmed => false,
            Self::AfterTicks(ticks) => ticks_live >= ticks,
            Self::AfterTravelMetres(metres) => {
                metres.is_finite() && travelled_m.is_finite() && travelled_m >= metres
            }
        }
    }
}

impl fmt::Display for ArmingRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

// -------------------------------------------------------------------- fuse ----

/// A proximity fuse's trigger geometry.
///
/// One number: the radius, in meters, within which the closest approach to
/// a target's swept path counts as a trigger. The original's trigger
/// geometry — a sphere, a shaped zone, a per-axis box, or a sensor cone that
/// ignores some approaches — is **unmeasured**, so no shape is chosen here
/// and the radius is the declared effective trigger radius. `FLIGHT-PHYSICS`
/// ("For interaction triggers use a swept center/shape appropriate to the
/// original rule") is satisfied by the sweep: the distance is computed over
/// the whole tick from relative motion, not from either endpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProximityFuse {
    trigger_radius_m: f64,
}

impl ProximityFuse {
    /// Assembles a proximity fuse, refusing a non-finite or
    /// non-positive radius.
    ///
    /// A zero-radius fuse is refused rather than treated as "triggers on
    /// contact": that is what [`FuseRule::Impact`] is for, and two spellings
    /// of the same behavior would let a catalogue contradict itself.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError::NonFiniteTriggerRadius`] or
    /// [`OrdnanceDefinitionError::NonPositiveTriggerRadius`].
    pub fn try_new(trigger_radius_m: f64) -> Result<Self, OrdnanceDefinitionError> {
        if !trigger_radius_m.is_finite() {
            return Err(OrdnanceDefinitionError::NonFiniteTriggerRadius);
        }
        if trigger_radius_m <= 0.0 {
            return Err(OrdnanceDefinitionError::NonPositiveTriggerRadius { trigger_radius_m });
        }
        Ok(Self { trigger_radius_m })
    }

    /// The declared effective trigger radius, in meters.
    #[must_use]
    pub const fn trigger_radius_m(self) -> f64 {
        self.trigger_radius_m
    }
}

/// How an item ends.
///
/// Three distinct behaviors, all **designed**:
///
/// * [`Impact`](FuseRule::Impact) — a swept contact ends the item. The
///   swept test itself is [`crate::weapons::guns::Ballistics`], whose
///   ledger already applies one hit at most once per projectile.
/// * [`Proximity`](FuseRule::Proximity) — the closest approach to a
///   target's swept path reaches [`ProximityFuse::trigger_radius_m`].
/// * [`Timed`](FuseRule::Timed) — the item's own clock reaches the
///   declared tick. This is the only end an area-denial item may have: its
///   effect is the bounded area, so a contact end would be meaningless.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FuseRule {
    /// Ends on swept contact.
    Impact,
    /// Ends when a target comes within the declared radius.
    Proximity(ProximityFuse),
    /// Ends after this many ticks, which may not exceed the item's own
    /// lifetime.
    Timed { ticks: u64 },
}

impl FuseRule {
    /// The stable label used in reports.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Impact => "impact".to_owned(),
            Self::Proximity(fuse) => format!("proximity({})", fuse.trigger_radius_m),
            Self::Timed { ticks } => format!("timed({ticks})"),
        }
    }

    /// Whether this fuse reacts to swept contact.
    #[must_use]
    pub const fn is_impact(&self) -> bool {
        matches!(self, Self::Impact)
    }
}

impl fmt::Display for FuseRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

// ---------------------------------------------------------------- guidance ----

/// What an item does when it can no longer follow its target.
///
/// Non-negotiable 4: "Guidance cannot track destroyed or invalid targets
/// forever; lost-target behavior is specified." Every guidance rule that
/// names a target therefore *names* what happens when the target is gone —
/// there is no default and no "keep tracking".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LostTargetBehavior {
    /// Detonate where the item is. A tagged-target weapon whose tag is
    /// dropped detonates instead of vanishing, which is the only behavior
    /// that leaves a visible outcome.
    Detonate,
    /// Stop tracking and fly the last velocity until the item's own
    /// lifetime expires. Bounded by construction: the lifetime is the
    /// item's, so coasting cannot continue forever.
    Coast,
    /// Stop being live: no detonation, no damage, no effect. The item
    /// remains a visible dud until its lifetime expires.
    Disarm,
}

impl LostTargetBehavior {
    /// Every lost-target behavior, in a stable order.
    pub const ALL: &'static [LostTargetBehavior] = &[Self::Detonate, Self::Coast, Self::Disarm];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Detonate => "detonate",
            Self::Coast => "coast",
            Self::Disarm => "disarm",
        }
    }
}

impl fmt::Display for LostTargetBehavior {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How an item finds its target — or the explicit fact that it does not.
///
/// The two variants are the point of non-negotiable 1: `Unguided` is a
/// declared, first-class state, so a direct explosive or a flak shell is not
/// a seeker with its guidance quietly defaulted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidanceRule {
    /// Flies the declared launch line. It has no target to lose.
    Unguided,
    /// Tracks one designated target until it is lost, then does exactly what
    /// `lost_target` says.
    Targeted { lost_target: LostTargetBehavior },
}

impl GuidanceRule {
    /// Whether this item tracks a target at all.
    #[must_use]
    pub const fn is_targeted(&self) -> bool {
        matches!(self, Self::Targeted { .. })
    }

    /// The declared lost-target behavior, for a rule that has one.
    #[must_use]
    pub const fn lost_target(&self) -> Option<LostTargetBehavior> {
        match self {
            Self::Unguided => None,
            Self::Targeted { lost_target } => Some(*lost_target),
        }
    }
}

// -------------------------------------------------------------- area effect ----

/// The bounded area one item leaves behind.
///
/// "Bounded" is enforced, not documented: the area's own lifetime may not
/// exceed the item's lifetime, so an area effect cannot outlive the thing
/// that created it or persist into a later encounter. The radius and the
/// lifetime are separate fields for the same reason the damage channels are:
/// they are separately measurable original quantities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaEffect {
    radius_m: f64,
    lifetime_ticks: u64,
}

impl AreaEffect {
    /// Assembles an area effect, refusing non-finite or non-positive
    /// geometry and a zero lifetime.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError::NonFiniteAreaRadius`],
    /// [`OrdnanceDefinitionError::NonPositiveAreaRadius`] or
    /// [`OrdnanceDefinitionError::ZeroAreaLifetime`].
    pub fn try_new(radius_m: f64, lifetime_ticks: u64) -> Result<Self, OrdnanceDefinitionError> {
        if !radius_m.is_finite() {
            return Err(OrdnanceDefinitionError::NonFiniteAreaRadius);
        }
        if radius_m <= 0.0 {
            return Err(OrdnanceDefinitionError::NonPositiveAreaRadius { radius_m });
        }
        if lifetime_ticks == 0 {
            return Err(OrdnanceDefinitionError::ZeroAreaLifetime);
        }
        Ok(Self {
            radius_m,
            lifetime_ticks,
        })
    }

    /// The declared area radius, in meters.
    #[must_use]
    pub const fn radius_m(&self) -> f64 {
        self.radius_m
    }

    /// The declared area lifetime, in whole ticks.
    #[must_use]
    pub const fn lifetime_ticks(&self) -> u64 {
        self.lifetime_ticks
    }
}

// ---------------------------------------------------------- status channels ----

/// What a status effect does to its recipient.
///
/// `Damage`, the choking/stall pair and `Marker` are separate kinds, never a
/// combined "effect level". The kinds are the *gameplay* vocabulary; the
/// visual particle that may accompany one is a separate
/// [`OrdnanceMedia`] field and reaches no decision (non-negotiable 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StatusEffectKind {
    /// Damage over time, on the F29 damage channels.
    Damage,
    /// Loss of engine output — the choke an engine-denial item inflicts.
    Choke,
    /// Loss of the ability to hold flight — the stall.
    Stall,
    /// A marker for other systems and the pilot; no damage, no loss.
    Marker,
}

impl StatusEffectKind {
    /// Every status effect kind, in a stable order.
    pub const ALL: &'static [StatusEffectKind] =
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

    /// Whether this kind degrades a capability rather than marking one.
    #[must_use]
    pub const fn is_degrading(self) -> bool {
        matches!(self, Self::Damage | Self::Choke | Self::Stall)
    }
}

impl fmt::Display for StatusEffectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Who a status effect is applied to.
///
/// A recipient is a **stable id**, never a scene entity reference: the actor
/// is a session-qualified [`ActorId`], and an effect aimed at a *system*
/// names the F29 [`SystemKind`] as well, so "the engine of actor 4" is a
/// value that survives a reload. AC03's engine-status effect is exactly
/// `StatusEffectTarget { actor, system: Some(SystemKind::Propulsion) }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatusEffectTarget {
    actor: ActorId,
    system: Option<SystemKind>,
}

impl StatusEffectTarget {
    /// An effect on the actor as a whole.
    #[must_use]
    pub const fn whole_actor(actor: ActorId) -> Self {
        Self {
            actor,
            system: None,
        }
    }

    /// An effect on one of the actor's damage-graph systems.
    #[must_use]
    pub const fn actor_system(actor: ActorId, system: SystemKind) -> Self {
        Self {
            actor,
            system: Some(system),
        }
    }

    /// The recipient actor.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The recipient system, when the effect names one.
    #[must_use]
    pub const fn system(&self) -> Option<SystemKind> {
        self.system
    }

    /// Whether two targets name the same recipient.
    ///
    /// Two targets match only when both the actor **and** the system match,
    /// so an actor-wide choke never silently reads as a satisfaction of the
    /// engine's own choke.
    #[must_use]
    pub fn is_same_recipient(&self, other: &Self) -> bool {
        self.actor == other.actor && self.system == other.system
    }
}

/// One timed status effect an item applies to a recipient.
///
/// A zero duration is refused: an instant effect is not a status effect, it
/// is a hit, and routing it through the ledger would give it a duration the
/// original may never have declared.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrdnanceStatusEffect {
    kind: StatusEffectKind,
    duration_ticks: u64,
    strength: f64,
}

impl OrdnanceStatusEffect {
    /// Assembles a status effect.
    ///
    /// `strength` is in the kind's own unmeasured unit: damage amount for
    /// [`StatusEffectKind::Damage`], a fraction of engine output for
    /// [`StatusEffectKind::Choke`], a stall severity for
    /// [`StatusEffectKind::Stall`] and an ignored magnitude for
    /// [`StatusEffectKind::Marker`].
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError::ZeroStatusDuration`],
    /// [`OrdnanceDefinitionError::NonFiniteStatusStrength`] or
    /// [`OrdnanceDefinitionError::NegativeStatusStrength`].
    pub fn try_new(
        kind: StatusEffectKind,
        duration_ticks: u64,
        strength: f64,
    ) -> Result<Self, OrdnanceDefinitionError> {
        if duration_ticks == 0 {
            return Err(OrdnanceDefinitionError::ZeroStatusDuration);
        }
        if !strength.is_finite() {
            return Err(OrdnanceDefinitionError::NonFiniteStatusStrength);
        }
        if strength < 0.0 {
            return Err(OrdnanceDefinitionError::NegativeStatusStrength { strength });
        }
        Ok(Self {
            kind,
            duration_ticks,
            strength,
        })
    }

    /// What this effect does.
    #[must_use]
    pub const fn kind(&self) -> StatusEffectKind {
        self.kind
    }

    /// How long the effect lasts, in whole ticks.
    #[must_use]
    pub const fn duration_ticks(&self) -> u64 {
        self.duration_ticks
    }

    /// The effect's strength, in its kind's own unit.
    #[must_use]
    pub const fn strength(&self) -> f64 {
        self.strength
    }
}

// -------------------------------------------------------------------- media ----

/// The resources a component plays: what is seen and what is heard.
///
/// Media is deliberately **not** a gameplay channel. `particles` is the
/// visual smoke or flame of an area-denial item; non-negotiable 3 requires
/// it to be separate from the damage and status effects, so it lives here
/// and reaches no decision: no accessor in this module, and no output record
/// below, carries it into a damage, fuse or ledger path. A component whose
/// effect media is missing loses a picture and a sound, never a detonation.
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceMedia {
    visual: ContentId,
    sound: ContentId,
    particles: Option<ContentId>,
}

impl OrdnanceMedia {
    /// Assembles a media record, validating every namespace it names.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError::MediaKindMismatch`] naming the offending
    /// id and the namespace it needed.
    pub fn try_new(
        visual: ContentId,
        sound: ContentId,
        particles: Option<ContentId>,
    ) -> Result<Self, OrdnanceDefinitionError> {
        let check =
            |id: &ContentId, expected: ContentKind| -> Result<(), OrdnanceDefinitionError> {
                if id.kind() == expected {
                    Ok(())
                } else {
                    Err(OrdnanceDefinitionError::MediaKindMismatch {
                        id: id.clone(),
                        expected,
                    })
                }
            };
        check(&visual, ContentKind::HardpointEquipment)?;
        check(&sound, ContentKind::Sound)?;
        if let Some(particles) = &particles {
            check(particles, ContentKind::HardpointEquipment)?;
        }
        Ok(Self {
            visual,
            sound,
            particles,
        })
    }

    /// The effect resource a launch or detonation plays.
    #[must_use]
    pub const fn visual(&self) -> &ContentId {
        &self.visual
    }

    /// The sound resource a launch or detonation plays.
    #[must_use]
    pub const fn sound(&self) -> &ContentId {
        &self.sound
    }

    /// The presentation-only particle resource, when the component declares
    /// one. No gameplay decision reads it.
    #[must_use]
    pub fn particles(&self) -> Option<&ContentId> {
        self.particles.as_ref()
    }
}

// -------------------------------------------------------- equipment rules ----

/// What else an airframe must, or must not, carry for this component to be
/// installed.
///
/// F28 non-negotiable 5 makes hardpoint equipment compatibility *shared
/// with loadout validation*: this record is the ordnance side of that
/// shared rule, and it is a pure check so the shop (F44) and an import
/// (F28-C) read the same verdict rather than each inventing one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EquipmentRules {
    requires: Option<ContentId>,
    forbids: BTreeSet<ContentId>,
}

impl EquipmentRules {
    /// Assembles an equipment rule.
    #[must_use]
    pub fn new(requires: Option<ContentId>, forbids: BTreeSet<ContentId>) -> Self {
        Self { requires, forbids }
    }

    /// The equipment the airframe must already carry, when one is required.
    #[must_use]
    pub const fn requires(&self) -> Option<&ContentId> {
        self.requires.as_ref()
    }

    /// The equipment the airframe must not carry.
    #[must_use]
    pub const fn forbids(&self) -> &BTreeSet<ContentId> {
        &self.forbids
    }

    /// Checks one installation against the declared equipment.
    ///
    /// The required item is checked first, so a verdict names one cause in a
    /// stable order rather than whichever set happened to be smaller.
    #[must_use]
    pub fn check(&self, installed: &BTreeSet<ContentId>) -> CompatibilityVerdict {
        if let Some(required) = &self.requires
            && !installed.contains(required)
        {
            return CompatibilityVerdict::MissingRequired {
                required: required.clone(),
            };
        }
        match self.forbids.iter().find(|id| installed.contains(*id)) {
            Some(forbidden) => CompatibilityVerdict::Forbidden {
                forbidden: forbidden.clone(),
            },
            None => CompatibilityVerdict::Compatible,
        }
    }
}

/// The result of checking one installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompatibilityVerdict {
    /// The airframe carries what the component needs and nothing it forbids.
    Compatible,
    /// A required piece of equipment is absent.
    MissingRequired {
        /// The absent equipment.
        required: ContentId,
    },
    /// A forbidden piece of equipment is present.
    Forbidden {
        /// The offending equipment.
        forbidden: ContentId,
    },
}

impl CompatibilityVerdict {
    /// Whether the installation is allowed.
    #[must_use]
    pub const fn is_compatible(&self) -> bool {
        matches!(self, Self::Compatible)
    }

    /// The stable label used in reports.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Compatible => "compatible".to_owned(),
            Self::MissingRequired { required } => {
                format!("missing_required({required})")
            }
            Self::Forbidden { forbidden } => format!("forbidden({forbidden})"),
        }
    }
}

impl fmt::Display for CompatibilityVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}
// ------------------------------------------------------ projectile ordnance ----

/// The declared behavior of one launched hardpoint component.
///
/// Every field the sheet's deliverable names is present and separate:
/// `launch` geometry, `stack` capacity and mass, `arming`, `fuse`,
/// `guidance`, `lifetime_ticks`, `area_effect`, the damage `channels` and
/// the `status` effects, and `media`.
///
/// The family is **validated against** those fields rather than trusted.
/// [`OrdnanceDefinitionError::IncoherentFamily`] is the mechanism behind
/// non-negotiable 1: a [`OrdnanceFamily::DirectExplosive`] or
/// [`OrdnanceFamily::ProximityFlak`] component may not be declared as a
/// seeker, a [`OrdnanceFamily::GuidedRocket`] may not be declared
/// unguided, and an [`OrdnanceFamily::AreaDenialEngine`] component may be
/// neither guided nor impact-fused — its effect is the bounded area, so it
/// must end on its own clock. Nothing stops a *torpedo* from being guided or
/// unguided, because that is exactly the kind of fact F28-D has to measure
/// rather than assume.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectileOrdnance {
    ordnance: OrdnanceId,
    family: OrdnanceFamily,
    launch: LaunchGeometry,
    stack: StackLoad,
    arming: ArmingRule,
    fuse: FuseRule,
    guidance: GuidanceRule,
    lifetime_ticks: u64,
    area_effect: Option<AreaEffect>,
    channels: WeaponDamage,
    status: Vec<OrdnanceStatusEffect>,
    media: OrdnanceMedia,
    equipment_rules: EquipmentRules,
}

impl ProjectileOrdnance {
    /// Assembles and validates a launched component.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError`] on a zero lifetime, a timed fuse longer
    /// than the item's own lifetime, an area effect outliving the item, a
    /// media id in the wrong namespace, or a
    /// [`OrdnanceDefinitionError::IncoherentFamily`] — a family whose
    /// declared guidance or fuse contradicts what that family *is*.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        ordnance: OrdnanceId,
        family: OrdnanceFamily,
        launch: LaunchGeometry,
        stack: StackLoad,
        arming: ArmingRule,
        fuse: FuseRule,
        guidance: GuidanceRule,
        lifetime_ticks: u64,
        area_effect: Option<AreaEffect>,
        channels: WeaponDamage,
        status: Vec<OrdnanceStatusEffect>,
        media: OrdnanceMedia,
        equipment_rules: EquipmentRules,
    ) -> Result<Self, OrdnanceDefinitionError> {
        if !family.is_projectile() {
            return Err(OrdnanceDefinitionError::NotAProjectile { family });
        }
        if lifetime_ticks == 0 {
            return Err(OrdnanceDefinitionError::ZeroLifetime);
        }
        if let FuseRule::Timed { ticks } = fuse
            && ticks > lifetime_ticks
        {
            return Err(OrdnanceDefinitionError::FuseOutlivesItem {
                fuse_ticks: ticks,
                lifetime_ticks,
            });
        }
        if let Some(area) = area_effect
            && area.lifetime_ticks() > lifetime_ticks
        {
            return Err(OrdnanceDefinitionError::AreaOutlivesItem {
                area_ticks: area.lifetime_ticks(),
                lifetime_ticks,
            });
        }
        check_family_rules(family, fuse, guidance)?;
        Ok(Self {
            ordnance,
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
        })
    }

    /// The catalog id of this component.
    #[must_use]
    pub const fn ordnance(&self) -> &OrdnanceId {
        &self.ordnance
    }

    /// The behavior family this component belongs to.
    #[must_use]
    pub const fn family(&self) -> OrdnanceFamily {
        self.family
    }

    /// The declared launch geometry.
    #[must_use]
    pub const fn launch(&self) -> &LaunchGeometry {
        &self.launch
    }

    /// The declared stack capacity and unit mass.
    #[must_use]
    pub const fn stack(&self) -> &StackLoad {
        &self.stack
    }

    /// When this component's fuse becomes live.
    #[must_use]
    pub const fn arming(&self) -> ArmingRule {
        self.arming
    }

    /// How this component ends.
    #[must_use]
    pub const fn fuse(&self) -> FuseRule {
        self.fuse
    }

    /// How this component finds — or does not find — a target.
    #[must_use]
    pub const fn guidance(&self) -> GuidanceRule {
        self.guidance
    }

    /// How many whole ticks the item may live after release.
    #[must_use]
    pub const fn lifetime_ticks(&self) -> u64 {
        self.lifetime_ticks
    }

    /// The bounded area this component leaves behind, when it leaves one.
    #[must_use]
    pub const fn area_effect(&self) -> Option<AreaEffect> {
        self.area_effect
    }

    /// The immediate damage channels this component delivers.
    #[must_use]
    pub const fn channels(&self) -> &WeaponDamage {
        &self.channels
    }

    /// The timed status effects this component applies.
    #[must_use]
    pub fn status(&self) -> &[OrdnanceStatusEffect] {
        &self.status
    }

    /// The resources this component plays.
    #[must_use]
    pub const fn media(&self) -> &OrdnanceMedia {
        &self.media
    }

    /// The equipment rules an installation of this component obeys.
    #[must_use]
    pub const fn equipment_rules(&self) -> &EquipmentRules {
        &self.equipment_rules
    }
}

/// Enforces the family rules that make non-negotiable 1 executable.
///
/// The three refusals, and why each is not a guess about the original:
///
/// * A **direct explosive** or a **flak shell** may not be declared as
///   tracking a target. Both families are named by what they *do* in their
///   names; if either could carry a seeker, the registry could describe one
///   weapon two ways and the "one homing missile substituted for every
///   rocket" failure would be undetectable.
/// * A **guided rocket** may not be declared unguided. The family is
///   defined by tracking a tag, so an unguided member is a mislabelled
///   component rather than a distinct one.
/// * An **area-denial** component may be neither targeted nor
///   impact-fused. Its effect is the bounded area, so a contact end would
///   mean the area never happened and a target would mean it is not the
///   family it claims.
fn check_family_rules(
    family: OrdnanceFamily,
    fuse: FuseRule,
    guidance: GuidanceRule,
) -> Result<(), OrdnanceDefinitionError> {
    match family {
        OrdnanceFamily::DirectExplosive | OrdnanceFamily::ProximityFlak => {
            if guidance.is_targeted() {
                return Err(OrdnanceDefinitionError::IncoherentFamily {
                    family,
                    field: "guidance",
                    detail: "this family is unguided by definition".to_owned(),
                });
            }
        }
        OrdnanceFamily::GuidedRocket => {
            if !guidance.is_targeted() {
                return Err(OrdnanceDefinitionError::IncoherentFamily {
                    family,
                    field: "guidance",
                    detail: "this family tracks a tag by definition".to_owned(),
                });
            }
        }
        OrdnanceFamily::AreaDenialEngine => {
            if guidance.is_targeted() {
                return Err(OrdnanceDefinitionError::IncoherentFamily {
                    family,
                    field: "guidance",
                    detail: "this family denies an area; it does not seek".to_owned(),
                });
            }
            if fuse.is_impact() {
                return Err(OrdnanceDefinitionError::IncoherentFamily {
                    family,
                    field: "fuse",
                    detail: "this family ends on its own clock, not on contact".to_owned(),
                });
            }
        }
        OrdnanceFamily::AerialTorpedo | OrdnanceFamily::NitroBooster => {}
    }
    Ok(())
}

/// Why an ordnance record was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceDefinitionError {
    /// A launch speed was NaN or infinite.
    NonFiniteLaunchSpeed,
    /// A launch speed was zero or negative.
    NonPositiveLaunchSpeed {
        /// The rejected value.
        launch_speed_mps: f64,
    },
    /// An inheritance share was corrupt.
    InvalidInheritanceShare {
        /// The rejected share.
        share: f64,
    },
    /// A launcher declared no stack capacity, so it can never be loaded.
    ZeroStackCapacity,
    /// A unit mass was NaN or infinite.
    NonFiniteUnitMass,
    /// A unit mass was negative.
    NegativeUnitMass {
        /// The rejected value.
        unit_mass_kg: f64,
    },
    /// A proximity trigger radius was NaN or infinite.
    NonFiniteTriggerRadius,
    /// A proximity trigger radius was zero or negative.
    NonPositiveTriggerRadius {
        /// The rejected value.
        trigger_radius_m: f64,
    },
    /// An area radius was NaN or infinite.
    NonFiniteAreaRadius,
    /// An area radius was zero or negative.
    NonPositiveAreaRadius {
        /// The rejected value.
        radius_m: f64,
    },
    /// An area effect declared no lifetime, so it is not bounded.
    ZeroAreaLifetime,
    /// A status effect declared no duration, so it is not a status effect.
    ZeroStatusDuration,
    /// A status strength was NaN or infinite.
    NonFiniteStatusStrength,
    /// A status strength was negative.
    NegativeStatusStrength {
        /// The rejected value.
        strength: f64,
    },
    /// The item's own lifetime was zero, so it exists for no ticks.
    ZeroLifetime,
    /// A timed fuse is scheduled to fire after the item has expired.
    FuseOutlivesItem {
        /// The fuse's declared tick.
        fuse_ticks: u64,
        /// The item's own lifetime.
        lifetime_ticks: u64,
    },
    /// An area effect would outlive the item that created it.
    AreaOutlivesItem {
        /// The area's declared lifetime.
        area_ticks: u64,
        /// The item's own lifetime.
        lifetime_ticks: u64,
    },
    /// A media id named the wrong catalog namespace.
    MediaKindMismatch {
        /// The offending id.
        id: ContentId,
        /// The namespace it needed.
        expected: ContentKind,
    },
    /// A booster family was declared as a launched projectile, or the other
    /// way round.
    NotAProjectile {
        /// The family that was declared.
        family: OrdnanceFamily,
    },
    /// The family and its declared rules contradict each other.
    IncoherentFamily {
        /// The family that was declared.
        family: OrdnanceFamily,
        /// Which declared field contradicted it.
        field: &'static str,
        /// Why the combination is incoherent.
        detail: String,
    },
    /// A nitro capacity was NaN or infinite.
    NonFiniteNitroCapacity,
    /// A nitro capacity was negative.
    NegativeNitroCapacity,
    /// A nitro capacity was zero, so the booster could never run.
    UnusableNitroCapacity,
    /// A nitro consumption rate was NaN or infinite.
    NonFiniteNitroConsumption,
    /// A nitro consumption rate was negative.
    NegativeNitroConsumption,
    /// A nitro consumption rate was zero, so activation would be free and
    /// therefore unbounded — a booster that cannot run out is not a
    /// capacity at all.
    UnusableNitroConsumption,
    /// A nitro recovery rate was NaN or infinite.
    NonFiniteNitroRecovery,
    /// A nitro recovery rate was negative.
    NegativeNitroRecovery,
    /// A nitro thrust was NaN or infinite.
    NonFiniteNitroThrust,
    /// A nitro thrust was negative: a booster that removes thrust is not a
    /// booster, and a negative extra thrust with a positive consumption
    /// would be a trap rather than a tradeoff.
    NegativeNitroThrust,
    /// A fixed nitro burn declared zero ticks, which is not an activation.
    ZeroNitroBurn,
    /// A nitro authority multiplier was NaN or infinite.
    NonFiniteAuthorityMultiplier,
    /// A nitro authority multiplier was outside `(0, 1]`.
    AuthorityMultiplierOutOfRange {
        /// The rejected value.
        authority_multiplier: f64,
    },
}

impl fmt::Display for OrdnanceDefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
            Self::ZeroAreaLifetime => {
                write!(f, "an area effect must declare a bounded lifetime")
            }
            Self::ZeroStatusDuration => {
                write!(f, "a status effect must last at least one tick")
            }
            Self::NonFiniteStatusStrength => write!(f, "a status strength must be finite"),
            Self::NegativeStatusStrength { strength } => {
                write!(f, "a status strength is negative: {strength}")
            }
            Self::ZeroLifetime => write!(f, "ordnance must live for at least one tick"),
            Self::FuseOutlivesItem {
                fuse_ticks,
                lifetime_ticks,
            } => write!(
                f,
                "the fuse fires after {fuse_ticks} tick(s) but the item expires after {lifetime_ticks}"
            ),
            Self::AreaOutlivesItem {
                area_ticks,
                lifetime_ticks,
            } => write!(
                f,
                "the area lasts {area_ticks} tick(s) but the item that made it expires after {lifetime_ticks}"
            ),
            Self::MediaKindMismatch { id, expected } => write!(
                f,
                "media resource {id} must be in the {} namespace",
                expected.label()
            ),
            Self::NotAProjectile { family } => write!(
                f,
                "{family} is not a launched item, so it cannot carry a fuse, a lifetime and a guidance rule"
            ),
            Self::IncoherentFamily {
                family,
                field,
                detail,
            } => write!(f, "{family} cannot declare {field}: {detail}"),
            Self::NonFiniteNitroCapacity => write!(f, "the nitro capacity must be finite"),
            Self::NegativeNitroCapacity => write!(f, "the nitro capacity is negative"),
            Self::UnusableNitroCapacity => write!(
                f,
                "a nitro booster needs a capacity above zero, or it can never be accepted"
            ),
            Self::NonFiniteNitroConsumption => {
                write!(f, "the nitro consumption rate must be finite")
            }
            Self::NegativeNitroConsumption => {
                write!(f, "the nitro consumption rate is negative")
            }
            Self::UnusableNitroConsumption => write!(
                f,
                "a nitro booster needs a consumption above zero, or its capacity never runs down"
            ),
            Self::NonFiniteNitroRecovery => write!(f, "the nitro recovery rate must be finite"),
            Self::NegativeNitroRecovery => write!(f, "the nitro recovery rate is negative"),
            Self::NonFiniteNitroThrust => write!(f, "the nitro extra thrust must be finite"),
            Self::NegativeNitroThrust => {
                write!(f, "the nitro extra thrust is negative")
            }
            Self::ZeroNitroBurn => write!(f, "a fixed nitro burn must last at least one tick"),
            Self::NonFiniteAuthorityMultiplier => {
                write!(f, "the nitro authority multiplier must be finite")
            }
            Self::AuthorityMultiplierOutOfRange {
                authority_multiplier,
            } => write!(
                f,
                "the nitro authority multiplier must be within (0, 1]: {authority_multiplier}"
            ),
        }
    }
}

impl std::error::Error for OrdnanceDefinitionError {}

// -------------------------------------------------------------------- nitro ----

/// How a nitro activation begins and how long it may run.
///
/// Nitro is **not** an Xbox-only feature (non-negotiable 2): the PC manual
/// lists a nitro activation control, so an activation rule is part of the
/// contract. Which rule the original used — a held control or a fixed burn —
/// is **unmeasured**, so both are declared vocabulary and the original's
/// choice is F28-D's to record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NitroActivationRule {
    /// Active on every tick the control is held and capacity remains.
    WhileHeld,
    /// Each accepted activation runs for this many whole ticks.
    FixedTicks { ticks: u64 },
}

impl NitroActivationRule {
    /// Every activation rule, in a stable order.
    pub const ALL: &'static [NitroActivationRule] =
        &[Self::WhileHeld, Self::FixedTicks { ticks: 1 }];

    /// The stable label used in reports.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::WhileHeld => "while_held".to_owned(),
            Self::FixedTicks { ticks } => format!("fixed_ticks({ticks})"),
        }
    }
}

impl fmt::Display for NitroActivationRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// What running nitro costs besides capacity.
///
/// The original's tradeoffs — if it has any, and what they are — are
/// **unknown** (non-negotiable 2), so this stage ships
/// [`NitroTradeoffs::UNMEASURED`]: no invented authority penalty and no
/// invented weapon interlock. The field exists so a measured value can be
/// installed later without changing any consumer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NitroTradeoffs {
    authority_multiplier: f64,
}

impl NitroTradeoffs {
    /// No measured tradeoff: authority is untouched and no capability is
    /// interlocked.
    pub const UNMEASURED: Self = Self {
        authority_multiplier: 1.0,
    };

    /// Assumes a declared control-authority penalty.
    ///
    /// `authority_multiplier` is a fraction of normal authority: `1.0` is
    /// none, `0.5` is half. It must be finite and within `(0, 1]` — a zero
    /// or negative authority would make an airframe unflyable, which is a
    /// different failure than a tradeoff and is refused here.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError::NonFiniteAuthorityMultiplier`] or
    /// [`OrdnanceDefinitionError::AuthorityMultiplierOutOfRange`].
    pub fn try_new(authority_multiplier: f64) -> Result<Self, OrdnanceDefinitionError> {
        if !authority_multiplier.is_finite() {
            return Err(OrdnanceDefinitionError::NonFiniteAuthorityMultiplier);
        }
        if !(0.0..=1.0).contains(&authority_multiplier) || authority_multiplier == 0.0 {
            return Err(OrdnanceDefinitionError::AuthorityMultiplierOutOfRange {
                authority_multiplier,
            });
        }
        Ok(Self {
            authority_multiplier,
        })
    }

    /// The declared fraction of normal control authority while nitro runs.
    #[must_use]
    pub const fn authority_multiplier(self) -> f64 {
        self.authority_multiplier
    }

    /// Whether no tradeoff has been measured, so nothing is being invented.
    #[must_use]
    pub const fn is_unmeasured(self) -> bool {
        self.authority_multiplier == 1.0
    }
}

/// The declared numbers a nitro booster carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NitroParameters {
    capacity_units: f64,
    consumption_per_s: f64,
    recovery_per_s: f64,
    extra_thrust_n: f64,
    activation: NitroActivationRule,
    tradeoffs: NitroTradeoffs,
}

impl NitroParameters {
    /// Assembles nitro parameters, refusing corrupt numbers.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError`] on a non-finite or negative capacity,
    /// consumption, recovery or thrust; on a booster that can never be
    /// activated; and on a zero-length fixed burn.
    pub fn try_new(
        capacity_units: f64,
        consumption_per_s: f64,
        recovery_per_s: f64,
        extra_thrust_n: f64,
        activation: NitroActivationRule,
        tradeoffs: NitroTradeoffs,
    ) -> Result<Self, OrdnanceDefinitionError> {
        for (amount, non_finite, negative) in [
            (
                capacity_units,
                OrdnanceDefinitionError::NonFiniteNitroCapacity,
                OrdnanceDefinitionError::NegativeNitroCapacity,
            ),
            (
                consumption_per_s,
                OrdnanceDefinitionError::NonFiniteNitroConsumption,
                OrdnanceDefinitionError::NegativeNitroConsumption,
            ),
            (
                recovery_per_s,
                OrdnanceDefinitionError::NonFiniteNitroRecovery,
                OrdnanceDefinitionError::NegativeNitroRecovery,
            ),
            (
                extra_thrust_n,
                OrdnanceDefinitionError::NonFiniteNitroThrust,
                OrdnanceDefinitionError::NegativeNitroThrust,
            ),
        ] {
            if !amount.is_finite() {
                return Err(non_finite);
            }
            if amount < 0.0 {
                return Err(negative);
            }
        }
        if capacity_units == 0.0 {
            return Err(OrdnanceDefinitionError::UnusableNitroCapacity);
        }
        if consumption_per_s == 0.0 {
            return Err(OrdnanceDefinitionError::UnusableNitroConsumption);
        }
        if let NitroActivationRule::FixedTicks { ticks } = activation
            && ticks == 0
        {
            return Err(OrdnanceDefinitionError::ZeroNitroBurn);
        }
        Ok(Self {
            capacity_units,
            consumption_per_s,
            recovery_per_s,
            extra_thrust_n,
            activation,
            tradeoffs,
        })
    }

    /// The declared capacity, in capacity units.
    #[must_use]
    pub const fn capacity_units(&self) -> f64 {
        self.capacity_units
    }

    /// Capacity consumed per second of running nitro.
    #[must_use]
    pub const fn consumption_per_s(&self) -> f64 {
        self.consumption_per_s
    }

    /// Capacity recovered per second while nitro is not running.
    #[must_use]
    pub const fn recovery_per_s(&self) -> f64 {
        self.recovery_per_s
    }

    /// The extra thrust nitro adds, in newtons.
    #[must_use]
    pub const fn extra_thrust_n(&self) -> f64 {
        self.extra_thrust_n
    }

    /// The declared activation rule.
    #[must_use]
    pub const fn activation(&self) -> NitroActivationRule {
        self.activation
    }

    /// The declared tradeoffs.
    #[must_use]
    pub const fn tradeoffs(&self) -> NitroTradeoffs {
        self.tradeoffs
    }
}

/// The declared behavior of one nitro booster.
///
/// A booster is the second kind of hardpoint component and it is
/// deliberately **not** a [`ProjectileOrdnance`]: it has no fuse, no
/// lifetime, no blast, no guidance and no launch geometry. Forcing it into
/// the projectile record would invent a detonation the original does not
/// have and hide the capacity arithmetic that AC04 is actually about.
#[derive(Clone, Debug, PartialEq)]
pub struct NitroOrdnance {
    ordnance: OrdnanceId,
    parameters: NitroParameters,
    media: OrdnanceMedia,
    equipment_rules: EquipmentRules,
}

impl NitroOrdnance {
    /// Assembles a nitro booster record.
    ///
    /// # Errors
    ///
    /// [`OrdnanceDefinitionError`] when the parameters or the media record
    /// are refused. The id's namespace is already validated by
    /// [`OrdnanceId::try_new`].
    pub fn try_new(
        ordnance: OrdnanceId,
        parameters: NitroParameters,
        media: OrdnanceMedia,
        equipment_rules: EquipmentRules,
    ) -> Result<Self, OrdnanceDefinitionError> {
        Ok(Self {
            ordnance,
            parameters,
            media,
            equipment_rules,
        })
    }

    /// The catalog id of this component.
    #[must_use]
    pub const fn ordnance(&self) -> &OrdnanceId {
        &self.ordnance
    }

    /// The declared capacity, consumption, recovery, thrust and activation
    /// rule.
    #[must_use]
    pub const fn parameters(&self) -> &NitroParameters {
        &self.parameters
    }

    /// The resources this component plays.
    #[must_use]
    pub const fn media(&self) -> &OrdnanceMedia {
        &self.media
    }

    /// The equipment rules an installation of this component obeys.
    #[must_use]
    pub const fn equipment_rules(&self) -> &EquipmentRules {
        &self.equipment_rules
    }
}

/// One hardpoint ordnance component: a launched item or a booster.
///
/// Both arms are boxed: either record on its own is large enough that
/// carrying it inline would make every registry entry as big as the larger
/// of the two, for no gain.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceComponent {
    /// A launched item with a fuse, a lifetime and a guidance rule.
    Projectile(Box<ProjectileOrdnance>),
    /// A nitro booster: capacity, consumption, extra thrust, no fuse.
    Nitro(Box<NitroOrdnance>),
}

impl OrdnanceComponent {
    /// The behavior family of this component.
    #[must_use]
    pub fn family(&self) -> OrdnanceFamily {
        match self {
            Self::Projectile(projectile) => projectile.family(),
            Self::Nitro(_) => OrdnanceFamily::NitroBooster,
        }
    }

    /// The catalog id of this component.
    #[must_use]
    pub fn ordnance(&self) -> &OrdnanceId {
        match self {
            Self::Projectile(projectile) => projectile.ordnance(),
            Self::Nitro(nitro) => nitro.ordnance(),
        }
    }

    /// The launched half, when this is a launched item.
    #[must_use]
    pub fn as_projectile(&self) -> Option<&ProjectileOrdnance> {
        match self {
            Self::Projectile(projectile) => Some(projectile),
            Self::Nitro(_) => None,
        }
    }

    /// The booster half, when this is a nitro booster.
    #[must_use]
    pub fn as_nitro(&self) -> Option<&NitroOrdnance> {
        match self {
            Self::Nitro(nitro) => Some(nitro),
            Self::Projectile(_) => None,
        }
    }

    /// The equipment rules an installation of this component obeys.
    #[must_use]
    pub const fn equipment_rules(&self) -> &EquipmentRules {
        match self {
            Self::Projectile(projectile) => projectile.equipment_rules(),
            Self::Nitro(nitro) => nitro.equipment_rules(),
        }
    }

    /// The resources this component plays.
    #[must_use]
    pub const fn media(&self) -> &OrdnanceMedia {
        match self {
            Self::Projectile(projectile) => projectile.media(),
            Self::Nitro(nitro) => nitro.media(),
        }
    }
}

/// Why an [`OrdnanceRegistry`] operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrdnanceRegistryError {
    /// Two components claim the same catalog id.
    DuplicateOrdnance {
        /// The repeated id.
        ordnance: OrdnanceId,
        /// The family already registered under it.
        family: OrdnanceFamily,
    },
    /// The registry has no component with this id.
    UnknownOrdnance {
        /// The id that was named.
        ordnance: OrdnanceId,
    },
    /// The same id was installed twice in one loadout.
    DuplicateInstallation {
        /// The repeated id.
        ordnance: OrdnanceId,
    },
}

impl fmt::Display for OrdnanceRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateOrdnance { ordnance, family } => {
                write!(
                    f,
                    "{ordnance} is already registered as a {family} component"
                )
            }
            Self::UnknownOrdnance { ordnance } => {
                write!(f, "no ordnance component is registered as {ordnance}")
            }
            Self::DuplicateInstallation { ordnance } => {
                write!(f, "{ordnance} is installed more than once")
            }
        }
    }
}

impl std::error::Error for OrdnanceRegistryError {}

/// The verdict for one installed component.
#[derive(Clone, Debug, PartialEq)]
pub struct InstallationVerdict {
    /// The component this verdict is about.
    pub ordnance: OrdnanceId,
    /// The family it belongs to.
    pub family: OrdnanceFamily,
    /// Whether it may be installed on this airframe.
    pub compatibility: CompatibilityVerdict,
}

/// The exhaustive registry of hardpoint ordnance components.
///
/// "Exhaustive" is a property of *this* registry only: it holds every
/// component the project has declared, refuses a duplicate id, and refuses
/// to resolve an installation that names an id it does not have. That last
/// refusal is the import-side half of non-negotiable 5 — a custom plane
/// naming an equipment id this registry does not know is **refused**, not
/// skipped, so it cannot reach a session by way of an import. Whether the
/// registry is complete with respect to the original installation is F28-D's
/// catalogue audit, and nothing here claims it.
#[derive(Clone, Debug, Default)]
pub struct OrdnanceRegistry {
    components: BTreeMap<OrdnanceId, OrdnanceComponent>,
}

impl OrdnanceRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one component.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRegistryError::DuplicateOrdnance`] when the id is already
    /// registered. Registering never replaces: a second component with the
    /// same id is a catalogue contradiction, and the registry refuses it
    /// instead of letting the later one win.
    pub fn register(&mut self, component: OrdnanceComponent) -> Result<(), OrdnanceRegistryError> {
        let ordnance = component.ordnance().clone();
        if let Some(existing) = self.components.get(&ordnance) {
            return Err(OrdnanceRegistryError::DuplicateOrdnance {
                ordnance,
                family: existing.family(),
            });
        }
        self.components.insert(ordnance, component);
        Ok(())
    }

    /// One component, if the registry has it.
    #[must_use]
    pub fn get(&self, ordnance: &OrdnanceId) -> Option<&OrdnanceComponent> {
        self.components.get(ordnance)
    }

    /// One component, or a refusal naming the missing id.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRegistryError::UnknownOrdnance`].
    pub fn require(
        &self,
        ordnance: &OrdnanceId,
    ) -> Result<&OrdnanceComponent, OrdnanceRegistryError> {
        self.get(ordnance)
            .ok_or_else(|| OrdnanceRegistryError::UnknownOrdnance {
                ordnance: ordnance.clone(),
            })
    }

    /// Every registered id, in the stable id order.
    pub fn ids(&self) -> impl Iterator<Item = &OrdnanceId> {
        self.components.keys()
    }

    /// How many components are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.components.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.components.is_empty()
    }

    /// The audit rows of a catalogue walk: every registered family with the
    /// ids that carry it, in the stable family order and then id order.
    ///
    /// This is what F28-D walks against the installation: a family present
    /// here with no original counterpart, or a family the original has that
    /// this map does not list, is a catalogue gap, not a rounding error.
    #[must_use]
    pub fn families(&self) -> Vec<(OrdnanceFamily, Vec<&OrdnanceId>)> {
        OrdnanceFamily::ALL
            .iter()
            .map(|family| {
                let ids = self
                    .components
                    .iter()
                    .filter(|(_, component)| component.family() == *family)
                    .map(|(id, _)| id)
                    .collect();
                (*family, ids)
            })
            .collect()
    }

    /// Resolves one airframe's installed components against the registry.
    ///
    /// Every id is checked, in the order given, and the **whole** loadout is
    /// refused the moment one id is unknown: a loadout is not usable with a
    /// hole in it, and an import that smuggles in an unsupported component
    /// is refused here rather than quietly carried (non-negotiable 5).
    /// A repeated id is refused the same way, since two components claiming
    /// one mount cannot both be installed.
    ///
    /// The verdicts are per component and include the equipment check, so
    /// the shop and an import read one answer rather than each applying its
    /// own idea of compatibility.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRegistryError::UnknownOrdnance`] for the first id the
    /// registry does not have, or
    /// [`OrdnanceRegistryError::DuplicateInstallation`] for the first id
    /// that appears twice.
    pub fn resolve_installation(
        &self,
        installed: &[OrdnanceId],
        equipment: &BTreeSet<ContentId>,
    ) -> Result<Vec<InstallationVerdict>, OrdnanceRegistryError> {
        let mut seen = BTreeSet::new();
        let mut verdicts = Vec::with_capacity(installed.len());
        for ordnance in installed {
            let component = self.require(ordnance)?;
            if !seen.insert(ordnance.clone()) {
                return Err(OrdnanceRegistryError::DuplicateInstallation {
                    ordnance: ordnance.clone(),
                });
            }
            verdicts.push(InstallationVerdict {
                ordnance: ordnance.clone(),
                family: component.family(),
                compatibility: component.equipment_rules().check(equipment),
            });
        }
        Ok(verdicts)
    }
}

// -------------------------------------------------- proximity geometry ----

/// One target's swept path over a single tick: where its center was and
/// where it is now.
///
/// A path, not a box: the proximity distance is computed over the whole tick
/// from **relative** motion, so a target that crosses the item's path
/// between two ticks is still tested — the case an endpoint-only
/// implementation misses (`FLIGHT-PHYSICS`, "Check relative movement: a
/// target can cross the projectile path between ticks").
///
/// The original's fuse *shape* is unmeasured. This stage therefore tests the
/// swept center against the declared trigger radius and does not invent a
/// shape; a spherical fuse and a directional sensor are different rules and
/// only F28-D's original evidence can say which the game used.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetPath {
    actor: ActorId,
    previous: WorldPosition,
    current: WorldPosition,
}

impl TargetPath {
    /// Assembles a target path.
    ///
    /// # Errors
    ///
    /// [`cs_types::space::SpaceError`] when a position component is not
    /// finite. The caller builds the positions with
    /// [`WorldPosition::try_new`], so this exists for direct construction.
    pub fn try_new(
        actor: ActorId,
        previous: [f64; 3],
        current: [f64; 3],
    ) -> Result<Self, SpaceError> {
        Ok(Self {
            actor,
            previous: WorldPosition::try_new(previous)?,
            current: WorldPosition::try_new(current)?,
        })
    }

    /// The actor this path belongs to.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The actor's center at the start of the tick.
    #[must_use]
    pub const fn previous(&self) -> WorldPosition {
        self.previous
    }

    /// The actor's center at the end of the tick.
    #[must_use]
    pub const fn current(&self) -> WorldPosition {
        self.current
    }
}

/// The closest a moving item came to a moving target over one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClosestApproach {
    /// The smallest distance reached, in meters.
    pub distance_m: f64,
    /// When it was reached, as a fraction of the tick: `0.0` is the start of
    /// the tick and `1.0` its end.
    pub time: f64,
}

impl ClosestApproach {
    /// Whether the two paths came within `radius_m`.
    #[must_use]
    pub fn is_within(&self, radius_m: f64) -> bool {
        self.distance_m <= radius_m
    }
}

/// The closest approach of an item's swept segment to a target's swept path,
/// over relative motion.
///
/// Both moves are expressed in one frame first: the target's own motion over
/// the tick is subtracted from the item's, so the test becomes a segment
/// against a static segment — the standard point-to-segment distance. That
/// is exact for two linear motions and, unlike an endpoint test, it cannot
/// miss a pass that happens entirely inside the tick.
///
/// The returned `time` is the parameter of that static segment's closest
/// point, which is the same normalized position along the *item's* motion
/// once the relative motion has been formed — so it stays inside `[0, 1]`
/// because both endpoints are clamped.
pub fn closest_approach(segment: &ProjectileSegment, path: &TargetPath) -> ClosestApproach {
    let p0 = segment.previous.to_array();
    let p1 = segment.current.to_array();
    let q0 = path.previous.to_array();
    let q1 = path.current.to_array();

    // Relative motion: the target's displacement removed from the item's.
    let mut d = [0.0f64; 3];
    for axis in 0..3 {
        d[axis] = (p1[axis] - p0[axis]) - (q1[axis] - q0[axis]);
    }
    let mut r = [0.0f64; 3];
    for axis in 0..3 {
        r[axis] = p0[axis] - q0[axis];
    }

    let a = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let e = r[0] * d[0] + r[1] * d[1] + r[2] * d[2];

    // `a == 0.0` is the exactly-degenerate case: the relative displacement is
    // zero, so the distance is constant over the tick and there is no
    // meaningful `t`. It is tested exactly rather than with an epsilon, per
    // `FLIGHT-PHYSICS`: the epsilon exists to avoid singularities, not to
    // invent a motion.
    let time = if a == 0.0 {
        0.0
    } else {
        (-e / a).clamp(0.0, 1.0)
    };

    let mut closest = [0.0f64; 3];
    for axis in 0..3 {
        closest[axis] = r[axis] + d[axis] * time;
    }
    ClosestApproach {
        distance_m: (closest[0] * closest[0] + closest[1] * closest[1] + closest[2] * closest[2])
            .sqrt(),
        time,
    }
}

// --------------------------------------------------------- instance state ----

/// Why an in-flight item ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FuseTrigger {
    /// A swept contact ended the item.
    Impact {
        /// The actor that was hit.
        target: ActorId,
        /// Where in the tick the contact happened, as a fraction.
        time: f64,
    },
    /// A target came within the declared trigger radius.
    Proximity {
        /// The target whose path was crossed.
        target: ActorId,
        /// The closest distance reached, in meters.
        distance_m: f64,
        /// When it happened, as a fraction of the tick.
        time: f64,
    },
    /// The item's own clock reached the declared tick.
    Timed {
        /// Ticks after launch the fuse was scheduled for.
        ticks: u64,
    },
}

impl FuseTrigger {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Impact { .. } => "impact",
            Self::Proximity { .. } => "proximity",
            Self::Timed { .. } => "timed",
        }
    }
}

/// Why an in-flight item did **not** end.
///
/// The reasons are named, not collapsed into a bool: "not armed yet" and
/// "armed but nothing was within the trigger radius" are different states of
/// the world and a test must be able to tell them apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FuseInert {
    /// The fuse is not live yet. `ticks_live` is how long the item has
    /// lived and `travelled_m` how far it has gone, so a caller can see how
    /// much more of the arming condition is outstanding.
    NotArmed {
        /// Ticks the item has lived.
        ticks_live: u64,
        /// Meters the item has travelled.
        travelled_m: f64,
    },
    /// The fuse is live and saw nothing.
    NothingTriggered,
    /// The fuse is live but the nearest path stayed outside the radius.
    OutOfRange {
        /// The closest distance reached, in meters.
        closest_m: f64,
        /// The declared trigger radius, in meters.
        trigger_radius_m: f64,
    },
    /// The item's own lifetime ran out without its fuse ending it. A dud:
    /// no detonation and no area effect.
    Expired,
    /// The item already ended earlier; a second call reports nothing.
    AlreadyTriggered,
}

impl FuseInert {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::NotArmed { .. } => "not_armed",
            Self::NothingTriggered => "nothing_triggered",
            Self::OutOfRange { .. } => "out_of_range",
            Self::Expired => "expired",
            Self::AlreadyTriggered => "already_triggered",
        }
    }
}

/// The result of asking one in-flight item whether it ended this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FuseDecision {
    /// The item ended, with the cause.
    Triggered(FuseTrigger),
    /// The item is still live, with the reason it did not end.
    Inert(FuseInert),
}

impl FuseDecision {
    /// The trigger, when the item ended.
    #[must_use]
    pub const fn trigger(&self) -> Option<FuseTrigger> {
        match self {
            Self::Triggered(trigger) => Some(*trigger),
            Self::Inert(_) => None,
        }
    }

    /// Whether the item ended.
    #[must_use]
    pub const fn is_triggered(&self) -> bool {
        matches!(self, Self::Triggered(_))
    }

    /// Whether the item is still live because it has not armed.
    #[must_use]
    pub const fn is_not_armed(&self) -> bool {
        matches!(self, Self::Inert(FuseInert::NotArmed { .. }))
    }
}

/// One in-flight ordnance item's fuse and lifetime state.
///
/// The authority for *when an item ends*: it holds the launch tick, the
/// travelled distance the arming rule needs, the fuse rule, and whether the
/// fuse has already fired. It is a record, not a system — no schedule drives
/// it here and it holds no Avian body; F28-B's per-tick system is what calls
/// [`advance`](Self::advance) and [`fuse_decision`](Self::fuse_decision).
///
/// [`fuse_decision`](Self::fuse_decision) latches its trigger: a second
/// call for the same item reports [`FuseInert::AlreadyTriggered`] and names
/// nothing new. That is what stops one item from detonating twice when two
/// collision features, or a proximity sweep and a contact sweep, both
/// report it in the same tick (`FLIGHT-PHYSICS`: "Apply damage once even if
/// several collision features report the same hit").
#[derive(Clone, Debug, PartialEq)]
pub struct OrdnanceState {
    projectile: ProjectileId,
    ordnance: OrdnanceId,
    family: OrdnanceFamily,
    arming: ArmingRule,
    fuse: FuseRule,
    lifetime_ticks: u64,
    launched_at: Tick,
    ticks_live: u64,
    travelled_m: f64,
    triggered: Option<FuseTrigger>,
    destroyed: bool,
}

impl OrdnanceState {
    /// Opens the state of one item released at `launched_at`.
    #[must_use]
    pub fn launch(
        projectile: ProjectileId,
        ordnance: OrdnanceId,
        family: OrdnanceFamily,
        arming: ArmingRule,
        fuse: FuseRule,
        lifetime_ticks: u64,
        launched_at: Tick,
    ) -> Self {
        Self {
            projectile,
            ordnance,
            family,
            arming,
            fuse,
            lifetime_ticks,
            launched_at,
            ticks_live: 0,
            travelled_m: 0.0,
            triggered: None,
            destroyed: false,
        }
    }

    /// The in-flight item's stable id.
    #[must_use]
    pub const fn projectile(&self) -> ProjectileId {
        self.projectile
    }

    /// The catalog id of the component this item is.
    #[must_use]
    pub const fn ordnance(&self) -> &OrdnanceId {
        &self.ordnance
    }

    /// The family of the component this item is.
    #[must_use]
    pub const fn family(&self) -> OrdnanceFamily {
        self.family
    }

    /// The tick the item was released on.
    #[must_use]
    pub const fn launched_at(&self) -> Tick {
        self.launched_at
    }

    /// How many whole ticks the item may live after release.
    #[must_use]
    pub const fn lifetime_ticks(&self) -> u64 {
        self.lifetime_ticks
    }

    /// The declared arming rule this item was launched with.
    #[must_use]
    pub const fn arming(&self) -> ArmingRule {
        self.arming
    }

    /// The declared fuse rule this item was launched with.
    #[must_use]
    pub const fn fuse(&self) -> FuseRule {
        self.fuse
    }

    /// Whole ticks the item has been in flight.
    #[must_use]
    pub const fn ticks_live(&self) -> u64 {
        self.ticks_live
    }

    /// Meters the item has travelled along its swept segments.
    #[must_use]
    pub const fn travelled_m(&self) -> f64 {
        self.travelled_m
    }

    /// Whether the fuse is live at this moment.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.arming.is_armed(self.ticks_live, self.travelled_m)
    }

    /// Whether the item's own lifetime is exhausted.
    #[must_use]
    pub const fn is_expired(&self) -> bool {
        self.ticks_live >= self.lifetime_ticks
    }

    /// Whether the item has already ended.
    #[must_use]
    pub const fn is_triggered(&self) -> bool {
        self.triggered.is_some()
    }

    /// The cause of the item's end, when it has ended.
    #[must_use]
    pub const fn triggered(&self) -> Option<FuseTrigger> {
        self.triggered
    }

    /// Advances the item by one tick along `segment`, accumulating the
    /// distance the arming rule needs.
    ///
    /// The distance is the swept segment's own length, so it is the path the
    /// item actually flew rather than a speed multiplied by a nominal tick
    /// length.
    pub fn advance(&mut self, segment: &ProjectileSegment) {
        if self.destroyed || self.is_triggered() {
            return;
        }
        let from = segment.previous.to_array();
        let to = segment.current.to_array();
        let dx = to[0] - from[0];
        let dy = to[1] - from[1];
        let dz = to[2] - from[2];
        self.travelled_m += (dx * dx + dy * dy + dz * dz).sqrt();
        self.ticks_live += 1;
    }

    /// Decides whether the item ends on this tick.
    ///
    /// The order is the contract, and each step is a refusal the next one
    /// cannot override:
    ///
    /// 1. already triggered → [`FuseInert::AlreadyTriggered`];
    /// 2. **not armed** → [`FuseInert::NotArmed`], whatever else this tick
    ///    reported. This is AC01's "not before arming";
    /// 3. a swept contact in `impacts` → [`FuseTrigger::Impact`];
    /// 4. a [`FuseRule::Timed`] clock that has run out → [`FuseTrigger::Timed`];
    /// 5. a [`FuseRule::Proximity`] radius that a path crossed →
    ///    [`FuseTrigger::Proximity`], the **nearest** path, with the actor id
    ///    as the stable tie-breaker;
    /// 6. otherwise the named inert reason.
    ///
    /// The first trigger that applies is latched, so calling this twice for
    /// one item yields one detonation.
    ///
    /// `impacts` is expected in the order
    /// [`crate::weapons::guns::Ballistics::sweep`] produces — ascending time
    /// of impact, with the actor id as the tie-breaker — so `impacts.first()`
    /// is the earliest contact rather than whichever report a caller happened
    /// to collect first. `paths` carries no such requirement: the nearest
    /// in-range path is chosen here, with the same tie-breaker.
    ///
    /// One consequence of the order above is worth stating: a
    /// [`FuseRule::Proximity`] item that has run out of lifetime still
    /// detonates on a path inside its radius, and reports
    /// [`FuseInert::OutOfRange`] rather than [`FuseInert::Expired`] when
    /// paths are presented but none is in range. Whether an expired item
    /// should still be able to trigger is an unmeasured original rule, so it
    /// is left to F28-B to decide with evidence rather than chosen here.
    pub fn fuse_decision(
        &mut self,
        segment: &ProjectileSegment,
        paths: &[TargetPath],
        impacts: &[SweptHit],
    ) -> FuseDecision {
        if self.destroyed {
            // A retired item has ended: it is a dud, not a live fuse. This
            // makes `retire`'s own contract real — it reports
            // `AlreadyTriggered` rather than pretending a detonation happened.
            return FuseDecision::Inert(FuseInert::AlreadyTriggered);
        }
        if self.triggered.is_some() {
            return FuseDecision::Inert(FuseInert::AlreadyTriggered);
        }
        if !self.is_armed() {
            return FuseDecision::Inert(FuseInert::NotArmed {
                ticks_live: self.ticks_live,
                travelled_m: self.travelled_m,
            });
        }
        if let Some(hit) = impacts.first() {
            return self.trigger(FuseTrigger::Impact {
                target: hit.target,
                time: hit.time_of_impact,
            });
        }
        if let FuseRule::Timed { ticks } = self.fuse
            && self.ticks_live >= ticks
        {
            return self.trigger(FuseTrigger::Timed { ticks });
        }
        if let FuseRule::Proximity(fuse) = self.fuse {
            // The nearest path wins, and equal distances are ordered by
            // actor id, so two equally close targets produce the same answer
            // on every host regardless of the order the caller collected them
            // in — the stable tie-breaker `FLIGHT-PHYSICS` requires.
            let mut best: Option<(f64, ActorId, f64)> = None;
            for path in paths {
                let approach = closest_approach(segment, path);
                if !approach.is_within(fuse.trigger_radius_m()) {
                    continue;
                }
                let candidate = (approach.distance_m, path.actor(), approach.time);
                best = match best {
                    Some(current) if (current.0, current.1) <= (candidate.0, candidate.1) => {
                        Some(current)
                    }
                    _ => Some(candidate),
                };
            }
            if let Some((distance_m, target, time)) = best {
                return self.trigger(FuseTrigger::Proximity {
                    target,
                    distance_m,
                    time,
                });
            }
            let closest_m = paths
                .iter()
                .map(|path| closest_approach(segment, path).distance_m)
                .fold(f64::INFINITY, f64::min);
            if closest_m.is_finite() {
                return FuseDecision::Inert(FuseInert::OutOfRange {
                    closest_m,
                    trigger_radius_m: fuse.trigger_radius_m(),
                });
            }
        }
        if self.is_expired() {
            return FuseDecision::Inert(FuseInert::Expired);
        }
        FuseDecision::Inert(FuseInert::NothingTriggered)
    }

    /// Latches a trigger and returns the decision.
    fn trigger(&mut self, trigger: FuseTrigger) -> FuseDecision {
        self.triggered = Some(trigger);
        FuseDecision::Triggered(trigger)
    }

    /// Retires the item — a bounce-off, a dud, or a session teardown —
    /// without it having ended.
    ///
    /// Idempotent, and it does not fabricate a cause: after this the item
    /// reports [`FuseInert::AlreadyTriggered`] rather than pretending a
    /// detonation happened.
    pub fn retire(&mut self) {
        self.destroyed = true;
    }

    /// Whether the item has been retired.
    #[must_use]
    pub const fn is_retired(&self) -> bool {
        self.destroyed
    }
}

// ----------------------------------------------------------- guidance track ----

/// Why a guided item lost its target.
///
/// Every reason is terminal. Non-negotiable 4: "Guidance cannot track
/// destroyed or invalid targets forever." There is no "retry" and no
/// "re-acquire": once a reason has been reported the tracker is spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LostTargetReason {
    /// The target was destroyed while the item was in flight.
    Destroyed,
    /// The target left the world — despawned, or out of the mission's
    /// actor roster for any reason.
    Despawned,
    /// The item was launched with no target assigned, so there was never
    /// anything to track. This is a different fact from
    /// [`Despawned`](LostTargetReason::Despawned) and is reported as one:
    /// an item that never had a target did not lose one.
    Unassigned,
    /// The tracker was asked about a tick belonging to another session
    /// generation. The item cannot carry a target across a restart, so a
    /// stale id is a lost target, not a lookup that might succeed.
    ForeignSession {
        /// The generation the item was launched in.
        expected: u64,
        /// The generation the caller presented.
        found: u64,
    },
}

impl LostTargetReason {
    /// The stable label used in reports.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Destroyed => "destroyed".to_owned(),
            Self::Despawned => "despawned".to_owned(),
            Self::Unassigned => "unassigned".to_owned(),
            Self::ForeignSession { expected, found } => {
                format!("foreign_session({expected}->{found})")
            }
        }
    }
}

impl fmt::Display for LostTargetReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// The outcome of asking a tracker about its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidanceUpdate {
    /// The item has no target to track, by declaration.
    Unguided,
    /// The item is tracking this target.
    Tracked {
        /// The target being tracked.
        target: ActorId,
    },
    /// The item lost its target, with the cause and what it now does. The
    /// cause is reported **once**; every later tick reports
    /// [`GuidanceUpdate::Coast`] or [`GuidanceUpdate::Disarmed`] without
    /// re-naming it.
    Lost {
        /// Why the target was lost, on the tick it was lost.
        reason: LostTargetReason,
        /// The declared behavior that loss invokes.
        behavior: LostTargetBehavior,
    },
    /// The item is coasting on its last vector until its lifetime expires.
    Coast,
    /// The item is a dud: it will not detonate and deals nothing.
    Disarmed,
}

impl GuidanceUpdate {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Unguided => "unguided",
            Self::Tracked { .. } => "tracked",
            Self::Lost { .. } => "lost",
            Self::Coast => "coast",
            Self::Disarmed => "disarmed",
        }
    }

    /// Whether the item lost its target on this update.
    #[must_use]
    pub const fn is_lost(&self) -> bool {
        matches!(self, Self::Lost { .. })
    }

    /// Whether the item is still following a target.
    #[must_use]
    pub const fn is_tracked(&self) -> bool {
        matches!(self, Self::Tracked { .. })
    }
}

/// Why a guidance query was refused outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuidanceError {
    /// The caller's actor is not this tracker's.
    UnknownProjectile {
        /// The item that was named.
        projectile: ProjectileId,
    },
}

impl fmt::Display for GuidanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownProjectile { projectile } => {
                write!(f, "no guided item {projectile} is registered here")
            }
        }
    }
}

impl std::error::Error for GuidanceError {}

/// One guided item's target, and what happened to it.
///
/// The tracker is the enforcement point for non-negotiable 4. It holds at
/// most one target, reports a loss **once** with its cause, and never
/// re-acquires: after a [`GuidanceUpdate::Lost`], every later query reports
/// the continuing consequence (`Coast` or `Disarmed`) and the target is
/// `None` forever. A session change is a loss, not a lookup that might
/// happen to match an id in the next session.
#[derive(Clone, Debug, PartialEq)]
pub struct GuidanceTracker {
    session: u64,
    rule: GuidanceRule,
    projectile: ProjectileId,
    target: Option<ActorId>,
    lost: Option<LostTargetReason>,
}

impl GuidanceTracker {
    /// Opens a tracker for one item in one session generation.
    ///
    /// `target` is `None` for an item whose target is not yet assigned, or
    /// for an item launched with no target — which is a launch that cannot
    /// be tracked at all, not a target waiting to be found.
    #[must_use]
    pub fn new(
        session: u64,
        projectile: ProjectileId,
        rule: GuidanceRule,
        target: Option<ActorId>,
    ) -> Self {
        Self {
            session,
            rule,
            projectile,
            target,
            lost: None,
        }
    }

    /// The session generation this tracker is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The in-flight item this tracker belongs to.
    #[must_use]
    pub const fn projectile(&self) -> ProjectileId {
        self.projectile
    }

    /// The declared guidance rule.
    #[must_use]
    pub const fn rule(&self) -> GuidanceRule {
        self.rule
    }

    /// The target being tracked, or `None` once the target is lost or was
    /// never assigned.
    #[must_use]
    pub const fn target(&self) -> Option<ActorId> {
        self.target
    }

    /// Why the target was lost, when it was.
    #[must_use]
    pub const fn lost(&self) -> Option<LostTargetReason> {
        self.lost
    }

    /// Reports that the tracked target no longer exists.
    ///
    /// This is the **only** way a target is lost. It is idempotent: the
    /// first call names the cause, and every later call reports only the
    /// continuing consequence, so a destroyed target cannot be announced on
    /// every tick and a stale lookup cannot resurrect it.
    pub fn lose(&mut self, reason: LostTargetReason) -> GuidanceUpdate {
        if self.lost.is_some() {
            // Already spent. Reporting the consequence here rather than
            // below matters for the `Detonate` behavior: an item that
            // detonated on the tick its target died has already ended, so a
            // later call must not re-announce a cause for an event that
            // happened once.
            return self.hold();
        }
        self.lost = Some(reason);
        self.target = None;
        match self.rule {
            GuidanceRule::Unguided => GuidanceUpdate::Unguided,
            GuidanceRule::Targeted { lost_target } => match lost_target {
                LostTargetBehavior::Detonate => GuidanceUpdate::Lost {
                    reason,
                    behavior: LostTargetBehavior::Detonate,
                },
                LostTargetBehavior::Coast => GuidanceUpdate::Coast,
                LostTargetBehavior::Disarm => GuidanceUpdate::Disarmed,
            },
        }
    }

    /// Reports that a target is still alive, which is what keeps tracking.
    ///
    /// A tracker that has already lost its target ignores this: the item
    /// cannot re-acquire, so a live actor arriving later is not adopted.
    ///
    /// A **targeted** item that holds no target is the one case this
    /// resolves rather than merely reports: [`new`](Self::new) documents a
    /// `None` target as a launch that cannot be tracked at all, not one
    /// waiting to be found, so the lack of a target is passed through
    /// [`lose`](Self::lose) exactly once. That way the reported behavior is
    /// the rule's own declared `lost_target` rather than a default, and the
    /// cause is named on the tick it is discovered and never again.
    pub fn hold(&mut self) -> GuidanceUpdate {
        match self.rule {
            GuidanceRule::Unguided => GuidanceUpdate::Unguided,
            GuidanceRule::Targeted { .. } => match (self.lost, self.target) {
                (None, Some(target)) => GuidanceUpdate::Tracked { target },
                (None, None) => self.lose(LostTargetReason::Unassigned),
                (Some(_), _) => match self.rule.lost_target() {
                    Some(LostTargetBehavior::Coast) => GuidanceUpdate::Coast,
                    Some(LostTargetBehavior::Disarm) => GuidanceUpdate::Disarmed,
                    // A detonating item has already ended on the tick its
                    // target was lost; holding it afterwards reports the
                    // consequence, not a new loss.
                    _ => GuidanceUpdate::Disarmed,
                },
            },
        }
    }
}

/// What a guided item's target is doing, as the session's authority sees it.
///
/// One observation covers **every** tracked item on the tick, because the
/// answer is a property of the target rather than of the item: a target that
/// was destroyed at tick `t` is destroyed for every item tracking it, and
/// two items must not be told different stories about the same actor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetObservation {
    /// The tick being resolved.
    pub tick: Tick,
    /// The tracked target, when one is assigned.
    pub target: Option<ActorId>,
    /// Why the target is gone. `None` means it is still alive.
    pub lost: Option<LostTargetReason>,
}

impl TargetObservation {
    /// The target is alive at `tick`.
    #[must_use]
    pub const fn alive(target: ActorId, tick: Tick) -> Self {
        Self {
            tick,
            target: Some(target),
            lost: None,
        }
    }

    /// The target was destroyed at `tick`.
    #[must_use]
    pub const fn destroyed(target: ActorId, tick: Tick) -> Self {
        Self {
            tick,
            target: Some(target),
            lost: Some(LostTargetReason::Destroyed),
        }
    }
}

/// The per-session registry of guided items.
///
/// It owns the trackers and applies the one rule a caller cannot bypass:
/// [`session_tick`](Self::session_tick) compares the generation every
/// observation carries, and a mismatch **loses the target** with
/// [`LostTargetReason::ForeignSession`] instead of looking it up in the next
/// session's roster. That is the "session change" half of AC02, made
/// structural rather than a convention every caller has to remember.
#[derive(Clone, Debug, Default)]
pub struct GuidanceSet {
    session: u64,
    trackers: BTreeMap<ProjectileId, GuidanceTracker>,
}

impl GuidanceSet {
    /// Opens an empty set for one session generation.
    #[must_use]
    pub const fn new(session: u64) -> Self {
        Self {
            session,
            trackers: BTreeMap::new(),
        }
    }

    /// The session generation this set is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// Registers one guided item's tracker.
    pub fn insert(&mut self, tracker: GuidanceTracker) {
        self.trackers.insert(tracker.projectile(), tracker);
    }

    /// One item's tracker.
    #[must_use]
    pub fn tracker(&self, projectile: &ProjectileId) -> Option<&GuidanceTracker> {
        self.trackers.get(projectile)
    }

    /// One item's tracker, or a refusal naming the missing item.
    ///
    /// The counterpart of [`OrdnanceRegistry::require`]: a caller that is
    /// about to update a guided item's target needs to know that the item is
    /// registered, because an absent tracker read as `None` would let a
    /// missing item be *skipped* rather than refused.
    ///
    /// # Errors
    ///
    /// [`GuidanceError::UnknownProjectile`].
    pub fn require(&self, projectile: &ProjectileId) -> Result<&GuidanceTracker, GuidanceError> {
        self.trackers
            .get(projectile)
            .ok_or(GuidanceError::UnknownProjectile {
                projectile: *projectile,
            })
    }

    /// One item's tracker, mutably.
    pub fn tracker_mut(&mut self, projectile: &ProjectileId) -> Option<&mut GuidanceTracker> {
        self.trackers.get_mut(projectile)
    }

    /// How many trackers are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.trackers.len()
    }

    /// Whether no tracker is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.trackers.is_empty()
    }

    /// Applies one tick's observation to every registered tracker.
    ///
    /// The session check runs first and applies to the whole set: an
    /// observation stamped with another generation loses every target,
    /// because no id from one session may be resolved in another.
    pub fn session_tick(
        &mut self,
        session: u64,
        observation: TargetObservation,
    ) -> BTreeMap<ProjectileId, GuidanceUpdate> {
        let foreign = session != self.session;
        self.trackers
            .iter_mut()
            .map(|(projectile, tracker)| {
                let update = if foreign {
                    tracker.lose(LostTargetReason::ForeignSession {
                        expected: self.session,
                        found: session,
                    })
                } else {
                    match observation.lost {
                        Some(reason) => tracker.lose(reason),
                        // The session reports a different actor than the item
                        // tracks: the tracked actor has left the roster. An
                        // observation carrying *no* target is "no news", not
                        // a vanished one, so it falls through to `hold`.
                        None => match (observation.target, tracker.target()) {
                            (Some(observed), Some(own)) if observed != own => {
                                tracker.lose(LostTargetReason::Despawned)
                            }
                            _ => tracker.hold(),
                        },
                    }
                };
                (*projectile, update)
            })
            .collect()
    }

    /// Drops one item's tracker, as a detonation or a despawn does.
    ///
    /// Idempotent: removing an absent tracker is not an error, because the
    /// caller cannot know whether the previous tick's teardown already did.
    pub fn remove(&mut self, projectile: &ProjectileId) -> Option<GuidanceTracker> {
        self.trackers.remove(projectile)
    }
}

// ------------------------------------------------------- status effect ledger ----

/// One live status effect, addressed by a stable instance id.
#[derive(Clone, Debug, PartialEq)]
pub struct ActiveStatusEffect {
    /// The effect's stable id: session generation plus a session-local
    /// serial. It never names a scene entity, so a reload cannot make it
    /// point at something else.
    pub instance: StatusEffectInstanceId,
    /// Who it applies to.
    pub target: StatusEffectTarget,
    /// What it does.
    pub kind: StatusEffectKind,
    /// How strong it is, in its kind's own unit.
    pub strength: f64,
    /// The tick it started on.
    pub started_at: Tick,
    /// The tick it expires on, inclusive: it is still active *at* this tick
    /// and gone at the next one. Naming one boundary rather than two is what
    /// makes "expires on the correct simulation tick" a testable claim.
    pub expires_at: Tick,
    /// Which component applied it.
    pub source: OrdnanceId,
}

impl ActiveStatusEffect {
    /// Whether the effect is still live at `tick`.
    #[must_use]
    pub fn is_live_at(&self, tick: Tick) -> bool {
        tick < self.expires_at
    }

    /// How many whole ticks remain at `tick`, saturating at zero.
    #[must_use]
    pub const fn ticks_remaining_at(&self, tick: Tick) -> u64 {
        match self.expires_at.0.checked_sub(tick.0) {
            Some(remaining) => remaining,
            None => 0,
        }
    }
}

/// A status effect's stable identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatusEffectInstanceId {
    /// The session generation the effect belongs to.
    pub session: u64,
    /// The session-local serial. Serials are never recycled inside a
    /// session, so an instance id always names exactly one effect.
    pub serial: u64,
}

impl fmt::Display for StatusEffectInstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "status/{}#{}", self.session, self.serial)
    }
}

/// A status effect that reached its expiry tick.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpiredStatusEffect {
    /// The effect that expired.
    pub instance: StatusEffectInstanceId,
    /// Who it had been applied to.
    pub target: StatusEffectTarget,
    /// What it was doing.
    pub kind: StatusEffectKind,
    /// The tick it expired on.
    pub expired_at: Tick,
}

/// Why a status-effect operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusEffectError {
    /// The caller presented a tick from another session generation. An
    /// effect from one session cannot be advanced, queried or applied inside
    /// another: the actor ids and the tick numbering both belong to the
    /// session that created them.
    ForeignSession {
        /// The ledger's generation.
        expected: u64,
        /// The generation the caller presented.
        found: u64,
    },
    /// A caller asked about a tick the ledger has already passed. Time in a
    /// session does not run backwards, and a re-entered schedule step must
    /// not resurrect an expired effect.
    NonMonotonicTick {
        /// The tick the ledger is at.
        at: Tick,
        /// The tick the caller presented.
        found: Tick,
    },
}

impl fmt::Display for StatusEffectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "status effects belong to session {expected}, but the caller is session {found}"
            ),
            Self::NonMonotonicTick { at, found } => write!(
                f,
                "this status ledger is at tick {} and cannot go back to tick {}",
                at.0, found.0
            ),
        }
    }
}

impl std::error::Error for StatusEffectError {}

/// The session's live status effects, over stable recipient ids.
///
/// Every effect is **bounded**: it carries an expiry tick derived from its
/// declared duration in whole ticks, and [`advance_to`](Self::advance_to)
/// removes exactly the effects whose boundary has been reached and reports
/// each once. A recipient is an [`ActorId`] plus an optional
/// [`SystemKind`], so an engine's choke and the aircraft's own stall are
/// different records even on the same frame.
///
/// The ledger holds **no** visual particle. Non-negotiable 3 requires the
/// damage and status effects to be separate from the particles that show
/// them, and the separation is structural: an entry in this ledger has no
/// media field at all, so no consumer of gameplay state can reach a
/// particle from it.
///
/// It is also the restart boundary: a new session gets a new ledger, and
/// [`StatusEffectLedger::session`] is checked by every operation, so a
/// session that restarts cannot inherit a previous session's effects.
#[derive(Clone, Debug)]
pub struct StatusEffectLedger {
    session: u64,
    tick: Tick,
    next_serial: u64,
    live: BTreeMap<StatusEffectInstanceId, ActiveStatusEffect>,
}

impl StatusEffectLedger {
    /// Opens an empty ledger for one session generation, positioned at `tick`.
    #[must_use]
    pub const fn new(session: u64, tick: Tick) -> Self {
        Self {
            session,
            tick,
            next_serial: 0,
            live: BTreeMap::new(),
        }
    }

    /// The session generation this ledger is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The tick this ledger is positioned at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// How many effects are live.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether no effect is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// One live effect by id.
    #[must_use]
    pub fn get(&self, instance: &StatusEffectInstanceId) -> Option<&ActiveStatusEffect> {
        self.live.get(instance)
    }

    /// Whether this recipient is under this effect kind right now.
    #[must_use]
    pub fn is_under(&self, target: &StatusEffectTarget, kind: StatusEffectKind) -> bool {
        self.live.values().any(|effect| {
            effect.kind == kind
                && effect.target.is_same_recipient(target)
                && effect.is_live_at(self.tick)
        })
    }

    /// The live effects on one recipient, in the stable id order.
    #[must_use]
    pub fn effects_on(&self, target: &StatusEffectTarget) -> Vec<&ActiveStatusEffect> {
        self.live
            .values()
            .filter(|effect| effect.target.is_same_recipient(target))
            .collect()
    }

    /// Applies one declared status effect to a recipient at `tick`.
    ///
    /// The expiry tick is `tick + duration_ticks`, computed in whole ticks:
    /// nothing here reads a wall clock, so no render frame rate can lengthen
    /// or shorten an effect.
    ///
    /// # Errors
    ///
    /// [`StatusEffectError::ForeignSession`] when `session` is not this
    /// ledger's, and [`StatusEffectError::NonMonotonicTick`] when `tick` is
    /// behind the ledger.
    pub fn apply(
        &mut self,
        session: u64,
        tick: Tick,
        target: StatusEffectTarget,
        source: &OrdnanceId,
        effect: &OrdnanceStatusEffect,
    ) -> Result<StatusEffectInstanceId, StatusEffectError> {
        self.check_session_and_tick(session, tick)?;
        let instance = StatusEffectInstanceId {
            session: self.session,
            serial: self.next_serial,
        };
        self.next_serial += 1;
        self.live.insert(
            instance,
            ActiveStatusEffect {
                instance,
                target,
                kind: effect.kind(),
                strength: effect.strength(),
                started_at: tick,
                expires_at: Tick(tick.0 + effect.duration_ticks()),
                source: source.clone(),
            },
        );
        Ok(instance)
    }

    /// Advances the ledger to `tick` and reports every effect that reached
    /// its expiry boundary.
    ///
    /// Each expired effect is reported **exactly once**: the returned list is
    /// the ledger's removal record, so a re-entered schedule step that calls
    /// `advance_to` again with the same tick sees nothing and cannot
    /// re-apply an expiry. Advancing to the current tick is a no-op that
    /// returns nothing; going *backwards* is refused.
    ///
    /// # Errors
    ///
    /// [`StatusEffectError::ForeignSession`] or
    /// [`StatusEffectError::NonMonotonicTick`].
    pub fn advance_to(
        &mut self,
        session: u64,
        tick: Tick,
    ) -> Result<Vec<ExpiredStatusEffect>, StatusEffectError> {
        self.check_session_and_tick(session, tick)?;
        if tick == self.tick {
            return Ok(Vec::new());
        }
        let mut expired = Vec::new();
        self.live.retain(|instance, effect| {
            if effect.is_live_at(tick) {
                return true;
            }
            expired.push(ExpiredStatusEffect {
                instance: *instance,
                target: effect.target,
                kind: effect.kind,
                expired_at: effect.expires_at,
            });
            false
        });
        // Stable order: the boundary tick first, then the instance id, so two
        // hosts report the same list whatever order the map iterated in.
        expired.sort_by(|a, b| {
            a.expired_at
                .cmp(&b.expired_at)
                .then(a.instance.cmp(&b.instance))
        });
        self.tick = tick;
        Ok(expired)
    }

    /// Removes one effect before it expires, as a repair or a mission reset
    /// does.
    ///
    /// Returns whether the effect was live, so a caller can tell a removal
    /// from a no-op.
    pub fn remove(
        &mut self,
        session: u64,
        instance: &StatusEffectInstanceId,
    ) -> Result<bool, StatusEffectError> {
        if session != self.session {
            return Err(StatusEffectError::ForeignSession {
                expected: self.session,
                found: session,
            });
        }
        Ok(self.live.remove(instance).is_some())
    }

    fn check_session_and_tick(
        &mut self,
        session: u64,
        tick: Tick,
    ) -> Result<(), StatusEffectError> {
        if session != self.session {
            return Err(StatusEffectError::ForeignSession {
                expected: self.session,
                found: session,
            });
        }
        if tick < self.tick {
            return Err(StatusEffectError::NonMonotonicTick {
                at: self.tick,
                found: tick,
            });
        }
        Ok(())
    }
}

// -------------------------------------------------------------- nitro ledger ----

/// Why a nitro activation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NitroRefusal {
    /// No capacity is left, so pressing the control consumes nothing.
    ///
    /// `FLIGHT-PHYSICS`: "Pressing a button while boost is unavailable does
    /// not consume capacity."
    CapacityExhausted,
    /// A fixed-duration burn is already running.
    BurnAlreadyRunning {
        /// The tick the running burn ends on.
        until: Tick,
    },
}

impl NitroRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::CapacityExhausted => "capacity_exhausted".to_owned(),
            Self::BurnAlreadyRunning { until } => {
                format!("burn_already_running(until {})", until.0)
            }
        }
    }
}

impl fmt::Display for NitroRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label())
    }
}

/// What nitro did on one tick.
///
/// The record is deliberately small and deliberately **not** a force or a
/// pose. It carries the *thrust modifier* and the *capacity consumed*, and
/// those are the only two things AC04 allows nitro to do: "Boost changes
/// thrust/consumption but never directly teleports or scales render dt."
/// There is no position, no velocity and no `dt_seconds` field on this
/// record, so no consumer can move an airframe or scale a render frame
/// through it — and no method on [`NitroLedger`] takes a
/// [`std::time::Duration`], so a caller cannot express "boost for this
/// frame's elapsed time" in the first place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NitroTick {
    /// The tick this record is for.
    pub tick: Tick,
    /// Whether the booster ran on this tick.
    pub active: bool,
    /// The extra thrust to add along the body forward axis, in newtons.
    pub extra_thrust_n: f64,
    /// Capacity consumed on this tick, in capacity units.
    pub consumed_units: f64,
    /// The control-authority multiplier in force on this tick. `1.0` is
    /// normal authority; a measured tradeoff may lower it.
    pub authority_multiplier: f64,
    /// Why the requested activation was refused, when it was.
    pub refused: Option<NitroRefusal>,
}

impl NitroTick {
    /// Whether the booster ran.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Whether a requested activation was refused.
    #[must_use]
    pub const fn is_refused(&self) -> bool {
        self.refused.is_some()
    }
}

/// Why a nitro operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NitroError {
    /// The caller's session generation is not this ledger's.
    ForeignSession {
        /// The ledger's generation.
        expected: u64,
        /// The generation the caller presented.
        found: u64,
    },
    /// A caller asked about a tick the ledger has passed.
    NonMonotonicTick {
        /// The tick the ledger is at.
        at: Tick,
        /// The tick the caller presented.
        found: Tick,
    },
}

impl fmt::Display for NitroError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "nitro belongs to session {expected}, but the caller is session {found}"
            ),
            Self::NonMonotonicTick { at, found } => write!(
                f,
                "this nitro ledger is at tick {} and cannot go back to tick {}",
                at.0, found.0
            ),
        }
    }
}

impl std::error::Error for NitroError {}

/// One actor's nitro capacity, consumption and burn state.
///
/// Capacity is in the component's own **capacity units** — the unit the
/// declared [`NitroParameters`] use — and every conversion to seconds goes
/// through the declared [`TickRate`], never through a caller-supplied
/// duration. A tick therefore consumes exactly
/// `consumption_per_s / ticks_per_second`, which is the same number on every
/// host and at every frame rate.
///
/// [`request`](Self::request) is the authority on accepted activation:
/// it is the only place capacity moves, and a refused activation moves
/// nothing. No method takes a [`std::time::Duration`], so there is no way to
/// express a render-frame-scaled boost burn.
#[derive(Clone, Debug)]
pub struct NitroLedger {
    session: u64,
    tick: Tick,
    rate: TickRate,
    capacity_units: f64,
    parameters: NitroParameters,
    burn_until: Option<Tick>,
}

impl NitroLedger {
    /// Opens a full-capacity ledger for one actor in one session.
    #[must_use]
    pub const fn new(
        session: u64,
        tick: Tick,
        rate: TickRate,
        parameters: NitroParameters,
    ) -> Self {
        Self {
            session,
            tick,
            rate,
            capacity_units: parameters.capacity_units(),
            parameters,
            burn_until: None,
        }
    }

    /// The session generation this ledger is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The tick this ledger is positioned at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The declared tick rate capacity is converted with.
    #[must_use]
    pub const fn rate(&self) -> TickRate {
        self.rate
    }

    /// The declared parameters.
    #[must_use]
    pub const fn parameters(&self) -> &NitroParameters {
        &self.parameters
    }

    /// Capacity remaining, in capacity units.
    #[must_use]
    pub fn capacity_units(&self) -> f64 {
        self.capacity_units
    }

    /// Capacity as a fraction of the declared maximum, in `[0, 1]`.
    #[must_use]
    pub fn capacity_fraction(&self) -> f64 {
        let maximum = self.parameters.capacity_units();
        if maximum <= 0.0 {
            return 0.0;
        }
        (self.capacity_units / maximum).clamp(0.0, 1.0)
    }

    /// Whether a fixed-duration burn is still running.
    #[must_use]
    pub fn burn_active_at(&self, tick: Tick) -> bool {
        self.burn_until.is_some_and(|until| tick < until)
    }

    /// Resolves one tick's activation request.
    ///
    /// `requested` is whether the activation control is being asserted this
    /// tick — the held state, never an elapsed duration. The ledger decides
    /// what that means under the declared [`NitroActivationRule`]:
    ///
    /// * a refused request — no capacity, or a burn already running —
    ///   consumes **nothing** and reports the refusal
    ///   (`FLIGHT-PHYSICS`, "Pressing a button while boost is unavailable
    ///   does not consume capacity");
    /// * an accepted request consumes one tick's worth of capacity and
    ///   reports the extra thrust and the authority multiplier.
    ///
    /// A [`NitroActivationRule::FixedTicks`] burn runs for its whole
    /// declared length whatever the control does afterwards: the
    /// [`NitroRefusal::BurnAlreadyRunning`] refusal is about *starting* a
    /// second activation, not about the one already accepted, so holding the
    /// control through a burn neither shortens it nor lets the idle-only
    /// recovery refill the tank underneath it.
    ///
    /// Recovery is applied first when the booster is idle, so a pilot who
    /// let go of the control gets capacity back before the next request.
    ///
    /// # Errors
    ///
    /// [`NitroError::ForeignSession`] or [`NitroError::NonMonotonicTick`].
    pub fn request(
        &mut self,
        session: u64,
        tick: Tick,
        requested: bool,
    ) -> Result<NitroTick, NitroError> {
        if session != self.session {
            return Err(NitroError::ForeignSession {
                expected: self.session,
                found: session,
            });
        }
        if tick < self.tick {
            return Err(NitroError::NonMonotonicTick {
                at: self.tick,
                found: tick,
            });
        }

        let elapsed = tick.0.saturating_sub(self.tick.0);
        let dt_s = elapsed as f64 * self.rate.dt_seconds();
        let burn_running = self.burn_active_at(tick);

        // Whether the booster runs on this tick is decided **before** any
        // capacity moves, because recovery is an idle-only effect: a tick
        // that consumes capacity must not also recover it, or a held control
        // would drift upward and a jumped tick would not — and ten ticks
        // walked would not cost what ten ticks jumped cost.
        let mut refused = None;
        if requested && !burn_running {
            if self.capacity_units <= 0.0 {
                // The rule `FLIGHT-PHYSICS` states: an unavailable boost
                // consumes nothing. Capacity is already empty, so this is a
                // refusal and not a silent zero-consumption activation.
                refused = Some(NitroRefusal::CapacityExhausted);
            } else {
                match self.parameters.activation() {
                    NitroActivationRule::WhileHeld => {}
                    NitroActivationRule::FixedTicks { ticks } => {
                        self.burn_until = Some(Tick(tick.0 + ticks));
                    }
                }
            }
        } else if requested && burn_running {
            refused = Some(NitroRefusal::BurnAlreadyRunning {
                until: self.burn_until.unwrap_or(tick),
            });
        }

        // An already-running fixed burn stays active even though this tick's
        // request is refused: the refusal is about starting *another*
        // activation, so gating `active` on it would stop the accepted burn
        // one tick in, and — because recovery is idle-only — would then pay
        // out capacity on every tick of a burn the pilot is still paying for.
        let active = burn_running || (requested && refused.is_none());

        // Idle recovery, for the elapsed seconds of a tick the booster was
        // not running.
        if !active && self.parameters.recovery_per_s() > 0.0 && dt_s > 0.0 {
            let recovered = self.parameters.recovery_per_s() * dt_s;
            self.capacity_units =
                (self.capacity_units + recovered).min(self.parameters.capacity_units());
        }
        // Consumption covers every elapsed tick, not just the last one, so a
        // caller that walks ten ticks and one that jumps ten reach the same
        // capacity. Both the recovery above and the consumption here convert
        // through the declared tick rate, never through a wall clock.
        let per_tick = self.parameters.consumption_per_s() * self.rate.dt_seconds();
        let consumed = if active {
            (per_tick * elapsed as f64).min(self.capacity_units)
        } else {
            0.0
        };
        self.capacity_units = (self.capacity_units - consumed).max(0.0);

        self.tick = tick;
        Ok(NitroTick {
            tick,
            active,
            extra_thrust_n: if active {
                self.parameters.extra_thrust_n()
            } else {
                0.0
            },
            consumed_units: consumed,
            authority_multiplier: if active {
                self.parameters.tradeoffs().authority_multiplier()
            } else {
                1.0
            },
            refused,
        })
    }

    /// Whether a fixed burn has finished and the booster is idle again.
    ///
    /// The burn's end is observed by the *next* [`request`](Self::request):
    /// this method reports the state without moving the ledger.
    #[must_use]
    pub fn burn_running(&self) -> bool {
        self.burn_active_at(self.tick)
    }
}

// ------------------------------------------------------------ ordnance runtime ----
//
// F28-A defined the records and the pure decisions above. This is the F28-B
// per-tick production path that owns the live items and drives those
// decisions in one session-confined runtime: launch geometry, swept motion,
// fuse resolution, guidance loss, status application and nitro. It is
// deliberately platform-independent — no Bevy, no Avian, no renderer — so the
// geometry and the accounting stay in the simulation and an ECS body can only
// mirror them (the same split F27-B made for gun rounds).

/// Why an [`OrdnanceRuntime`] operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum OrdnanceRuntimeError {
    /// The item, observation or event belongs to another session generation.
    ForeignSession {
        /// The runtime's session.
        expected: u64,
        /// The generation the caller presented.
        found: u64,
    },
    /// An item with this identity is already live. An item is launched once.
    DuplicateProjectile {
        /// The repeated projectile.
        projectile: ProjectileId,
    },
    /// The runtime holds no live item with this identity.
    UnknownProjectile {
        /// The item that was named.
        projectile: ProjectileId,
    },
    /// The runtime holds no booster for this actor.
    UnknownBooster {
        /// The actor that was named.
        shooter: ActorId,
    },
    /// The item is live but its fuse has not triggered, so it has no damage
    /// to route. Only a fired item delivers damage.
    NotTriggered {
        /// The item whose damage was asked for.
        projectile: ProjectileId,
    },
    /// This item's damage has already been routed. A fuse trigger latches
    /// once, and routing follows the same rule.
    AlreadyRouted {
        /// The already-routed item.
        projectile: ProjectileId,
    },
    /// A wind velocity had a non-finite component.
    NonFiniteWind {
        /// The offending axis: `0` X, `1` Y, `2` Z.
        component: usize,
    },
    /// A launch velocity had a non-finite component.
    NonFiniteVelocity {
        /// The offending axis.
        component: usize,
    },
    /// The tick length was negative, NaN or infinite.
    NonFiniteExtent {
        /// The field that was refused (`dt_s`).
        field: &'static str,
    },
    /// Advancing an item produced a non-finite position.
    NonFinitePosition {
        /// The item whose position left the representable range.
        projectile: ProjectileId,
    },
    /// An item declared a zero-tick lifetime, so it could never exist as a
    /// projectile.
    ZeroLifetime {
        /// The item with no life.
        projectile: ProjectileId,
    },
    /// The status ledger refused the application or the advance.
    Status(StatusEffectError),
    /// A routed damage amount could not form a [`HitEvent`].
    Hit(HitEventError),
    /// The nitro ledger refused the request.
    Nitro(NitroError),
    /// The runtime is session generation zero, which is not a session a
    /// [`HitEvent`] can be stamped into.
    NoSession,
}

impl OrdnanceRuntimeError {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ForeignSession { .. } => "foreign_session",
            Self::DuplicateProjectile { .. } => "duplicate_projectile",
            Self::UnknownProjectile { .. } => "unknown_projectile",
            Self::UnknownBooster { .. } => "unknown_booster",
            Self::NotTriggered { .. } => "not_triggered",
            Self::AlreadyRouted { .. } => "already_routed",
            Self::NonFiniteWind { .. } => "non_finite_wind",
            Self::NonFiniteVelocity { .. } => "non_finite_velocity",
            Self::NonFiniteExtent { .. } => "non_finite_extent",
            Self::NonFinitePosition { .. } => "non_finite_position",
            Self::ZeroLifetime { .. } => "zero_lifetime",
            Self::Status(_) => "status",
            Self::Hit(_) => "hit",
            Self::Nitro(_) => "nitro",
            Self::NoSession => "no_session",
        }
    }
}

impl fmt::Display for OrdnanceRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "this ordnance belongs to session {expected}, but the caller presented session {found}"
            ),
            Self::DuplicateProjectile { projectile } => {
                write!(f, "{projectile} is already live")
            }
            Self::UnknownProjectile { projectile } => {
                write!(f, "no ordnance item {projectile} is live in this runtime")
            }
            Self::UnknownBooster { shooter } => {
                write!(f, "{shooter} has no nitro booster registered")
            }
            Self::NotTriggered { projectile } => {
                write!(
                    f,
                    "{projectile} has not triggered, so it has no damage to route"
                )
            }
            Self::AlreadyRouted { projectile } => {
                write!(f, "{projectile}'s triggered damage has already been routed")
            }
            Self::NonFiniteWind { component } => {
                write!(f, "a wind component {component} must be finite")
            }
            Self::NonFiniteVelocity { component } => {
                write!(f, "a launch velocity component {component} must be finite")
            }
            Self::NonFiniteExtent { field } => write!(f, "{field} must be finite and non-negative"),
            Self::NonFinitePosition { projectile } => {
                write!(f, "{projectile} left the representable position range")
            }
            Self::ZeroLifetime { projectile } => {
                write!(f, "{projectile} was declared with a zero-tick lifetime")
            }
            Self::Status(source) => write!(f, "the status ledger refused the call: {source}"),
            Self::Hit(source) => write!(f, "a routed damage amount was refused: {source}"),
            Self::Nitro(source) => write!(f, "the nitro ledger refused the request: {source}"),
            Self::NoSession => write!(
                f,
                "this runtime is session generation zero, which is not a session a hit can be stamped into"
            ),
        }
    }
}

impl std::error::Error for OrdnanceRuntimeError {}

impl From<StatusEffectError> for OrdnanceRuntimeError {
    fn from(source: StatusEffectError) -> Self {
        Self::Status(source)
    }
}

impl From<NitroError> for OrdnanceRuntimeError {
    fn from(source: NitroError) -> Self {
        Self::Nitro(source)
    }
}

/// One live ordnance item's authoritative motion and behavior state.
///
/// The declared [`ProjectileOrdnance`] travels with the item so the runtime
/// can answer what the item *is* (its guidance, its status effects, its
/// damage channels) without a second registry lookup, while the moving
/// [`OrdnanceState`] keeps the fuse, arming and lifetime accounting F28-A
/// defined. `previous` and `current` are the authoritative swept segment of
/// the current tick; an ECS body is a mirror, never a second integrator.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveOrdnance {
    projectile: ProjectileId,
    shooter: ActorId,
    definition: ProjectileOrdnance,
    state: OrdnanceState,
    previous: WorldPosition,
    current: WorldPosition,
    air_velocity_m_s: [f64; 3],
}

impl LiveOrdnance {
    /// The item's stable identity.
    #[must_use]
    pub const fn projectile(&self) -> ProjectileId {
        self.projectile
    }

    /// The actor that launched it.
    #[must_use]
    pub const fn shooter(&self) -> ActorId {
        self.shooter
    }

    /// The declared component this item is.
    #[must_use]
    pub const fn definition(&self) -> &ProjectileOrdnance {
        &self.definition
    }

    /// The item's fuse, arming and lifetime state.
    #[must_use]
    pub const fn state(&self) -> &OrdnanceState {
        &self.state
    }

    /// Where the item was at the start of the current tick.
    #[must_use]
    pub const fn previous(&self) -> WorldPosition {
        self.previous
    }

    /// Where the item is now.
    #[must_use]
    pub const fn current(&self) -> WorldPosition {
        self.current
    }

    /// The constant air-relative velocity the item flies with.
    #[must_use]
    pub const fn air_velocity_m_s(&self) -> [f64; 3] {
        self.air_velocity_m_s
    }

    /// The swept segment the item covered over the current tick.
    #[must_use]
    pub const fn segment(&self) -> ProjectileSegment {
        ProjectileSegment {
            projectile: self.projectile,
            previous: self.previous,
            current: self.current,
        }
    }

    /// The world velocity the item has in `wind`: the shared conversion of
    /// its constant air-relative velocity into the world frame. The wind is
    /// subtracted exactly once, at launch, and added back here.
    #[must_use]
    pub fn world_velocity_m_s(&self, wind_velocity_m_s: [f64; 3]) -> [f64; 3] {
        world_velocity_from_air_m_s(self.air_velocity_m_s, wind_velocity_m_s)
    }
}

/// One tick's accounting of the live ordnance items.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OrdnanceTick {
    /// The swept segment each live item covered over the tick, in ascending
    /// id order (the map's own stable order).
    pub segments: Vec<ProjectileSegment>,
    /// Items whose declared lifetime ended this tick, *after* their final
    /// segment was produced.
    pub expired: Vec<ProjectileId>,
    /// Items removed because their fuse had already triggered on an earlier
    /// tick and their damage had a tick to be routed.
    pub triggered: Vec<ProjectileId>,
}

impl OrdnanceTick {
    /// Whether the tick moved no item and retired nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() && self.expired.is_empty() && self.triggered.is_empty()
    }
}

/// The outcome of one guidance tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuidanceTick {
    /// Every tracker's update, in ascending item id order.
    pub updates: BTreeMap<ProjectileId, GuidanceUpdate>,
    /// The items that ended by losing their target with a `Detonate`
    /// behavior. Their tracker and their live record are already gone; the
    /// caller applies the blast at the last recorded position.
    pub detonated: Vec<ProjectileId>,
}

impl GuidanceTick {
    /// The update for one item, if it was tracked by this runtime.
    #[must_use]
    pub fn update_for(&self, projectile: &ProjectileId) -> Option<GuidanceUpdate> {
        self.updates.get(projectile).copied()
    }
}

/// The per-session runtime of live ordnance.
///
/// This is the production path F28-A left unowned: [`launch`](Self::launch)
/// turns a declared [`ProjectileOrdnance`] and a supplied [`MountTransform`]
/// into a moving item, [`advance`](Self::advance) moves every live item and
/// produces the swept segments the direct and proximity fuses test,
/// [`decide`](Self::decide) runs the item's own fuse decision,
/// [`guidance_tick`](Self::guidance_tick) drives the lost-target contract,
/// [`apply_statuses`](Self::apply_statuses) and
/// [`advance_status`](Self::advance_status) drive the bounded status ledger,
/// and [`register_nitro`](Self::register_nitro) /
/// [`request_nitro`](Self::request_nitro) drive one actor's booster.
///
/// Every piece of durable state is session-confined and keyed by a stable id:
/// a [`ProjectileId`] for an item, an [`ActorId`] for a booster, a
/// [`StatusEffectInstanceId`] for an effect. Teardown is dropping the
/// runtime. There is no pose on any output and no method takes a
/// [`std::time::Duration`], so nothing here can teleport an airframe or scale
/// a render frame.
#[derive(Clone, Debug)]
pub struct OrdnanceRuntime {
    session: u64,
    tick: Tick,
    rate: TickRate,
    producer: u32,
    next_hit_sequence: u32,
    live: BTreeMap<ProjectileId, LiveOrdnance>,
    routed: BTreeSet<ProjectileId>,
    guidance: GuidanceSet,
    status: StatusEffectLedger,
    nitro: BTreeMap<ActorId, NitroLedger>,
}

impl OrdnanceRuntime {
    /// Opens a runtime for one session generation, positioned at `tick` and
    /// stamping the [`HitEvent`]s it routes with producer serial `producer`.
    #[must_use]
    pub fn new(session: u64, tick: Tick, rate: TickRate, producer: u32) -> Self {
        Self {
            session,
            tick,
            rate,
            producer,
            next_hit_sequence: 0,
            live: BTreeMap::new(),
            routed: BTreeSet::new(),
            guidance: GuidanceSet::new(session),
            status: StatusEffectLedger::new(session, tick),
            nitro: BTreeMap::new(),
        }
    }

    /// The session generation this runtime is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The tick this runtime is positioned at.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The declared tick rate capacity and time are converted with.
    #[must_use]
    pub const fn rate(&self) -> TickRate {
        self.rate
    }

    /// How many items are live.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether no item is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// One live item, if it is still flying.
    #[must_use]
    pub fn get(&self, projectile: &ProjectileId) -> Option<&LiveOrdnance> {
        self.live.get(projectile)
    }

    /// Every live item, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &LiveOrdnance> {
        self.live.values()
    }

    /// Every live item's current swept segment, in ascending id order.
    #[must_use]
    pub fn segments(&self) -> Vec<ProjectileSegment> {
        self.live.values().map(LiveOrdnance::segment).collect()
    }

    /// The session's guidance trackers.
    #[must_use]
    pub const fn guidance(&self) -> &GuidanceSet {
        &self.guidance
    }

    /// The session's bounded status-effect ledger.
    #[must_use]
    pub const fn status(&self) -> &StatusEffectLedger {
        &self.status
    }

    /// One actor's nitro ledger, if a booster is registered for it.
    #[must_use]
    pub fn nitro(&self, shooter: &ActorId) -> Option<&NitroLedger> {
        self.nitro.get(shooter)
    }

    /// Launches one declared item from a supplied mount pose.
    ///
    /// The world release velocity is composed by the *declared*
    /// [`LaunchGeometry`] through F27's [`MountTransform::world_velocity_mps`]
    /// (never a second composition rule), and the wind is removed from it
    /// once by the shared [`air_relative_velocity_m_s`], exactly as F27-B does
    /// for a gun round. A targeted component registers its guidance tracker at
    /// launch, with `target` as the designated target; `None` is a launch that
    /// cannot be tracked and is resolved by the rule's own lost-target
    /// behavior on the first guidance tick (F28-A).
    ///
    /// # Errors
    ///
    /// [`OrdnanceRuntimeError`] when the item or the shooter is from another
    /// session, a projectile with the same id is already live, a release
    /// velocity or wind component is non-finite, or the declared lifetime is
    /// zero.
    pub fn launch(
        &mut self,
        shooter: ActorId,
        projectile: ProjectileId,
        definition: &ProjectileOrdnance,
        transform: &MountTransform,
        target: Option<ActorId>,
        wind_velocity_m_s: [f64; 3],
    ) -> Result<ProjectileId, OrdnanceRuntimeError> {
        if projectile.session != self.session {
            return Err(OrdnanceRuntimeError::ForeignSession {
                expected: self.session,
                found: projectile.session,
            });
        }
        if shooter.session.get() != self.session {
            return Err(OrdnanceRuntimeError::ForeignSession {
                expected: self.session,
                found: shooter.session.get(),
            });
        }
        if self.live.contains_key(&projectile) {
            return Err(OrdnanceRuntimeError::DuplicateProjectile { projectile });
        }
        if definition.lifetime_ticks() == 0 {
            return Err(OrdnanceRuntimeError::ZeroLifetime { projectile });
        }
        check_finite_wind(wind_velocity_m_s)?;
        let release = definition.launch().release_velocity_mps(transform);
        for (component, value) in release.iter().enumerate() {
            if !value.is_finite() {
                return Err(OrdnanceRuntimeError::NonFiniteVelocity { component });
            }
        }
        let air_velocity_m_s = air_relative_velocity_m_s(release, wind_velocity_m_s);
        let origin = transform.origin;
        self.live.insert(
            projectile,
            LiveOrdnance {
                projectile,
                shooter,
                definition: definition.clone(),
                state: OrdnanceState::launch(
                    projectile,
                    definition.ordnance().clone(),
                    definition.family(),
                    definition.arming(),
                    definition.fuse(),
                    definition.lifetime_ticks(),
                    self.tick,
                ),
                previous: origin,
                current: origin,
                air_velocity_m_s,
            },
        );
        if definition.guidance().is_targeted() {
            self.guidance.insert(GuidanceTracker::new(
                self.session,
                projectile,
                definition.guidance(),
                target,
            ));
        }
        Ok(projectile)
    }

    /// Advances every live item by one tick of `dt_s` seconds.
    ///
    /// The returned [`OrdnanceTick`] names each item's swept segment, the
    /// items whose lifetime ended, and the items removed because their fuse
    /// had already triggered. Expired and triggered items are removed from the
    /// runtime *after* their final segment is recorded, and their guidance
    /// trackers are dropped with them, so no stale tracker can outlive its
    /// item.
    ///
    /// The tick is **all-or-nothing**: the next positions are validated
    /// before any item moves, so an item whose next position is not
    /// representable refuses the whole tick and leaves every item exactly
    /// where it was.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRuntimeError::NonFiniteExtent`] when `dt_s` is NaN, infinite
    /// or negative, [`OrdnanceRuntimeError::NonFiniteWind`] when a wind
    /// component is not finite, and
    /// [`OrdnanceRuntimeError::NonFinitePosition`] when an item's next
    /// position is not representable.
    pub fn advance(
        &mut self,
        dt_s: f64,
        wind_velocity_m_s: [f64; 3],
    ) -> Result<OrdnanceTick, OrdnanceRuntimeError> {
        if !dt_s.is_finite() || dt_s < 0.0 {
            return Err(OrdnanceRuntimeError::NonFiniteExtent { field: "dt_s" });
        }
        check_finite_wind(wind_velocity_m_s)?;
        let mut moved = Vec::with_capacity(self.live.len());
        for live in self.live.values() {
            let velocity = world_velocity_from_air_m_s(live.air_velocity_m_s, wind_velocity_m_s);
            let mut position = live.current.to_array();
            for axis in 0..3 {
                position[axis] += velocity[axis] * dt_s;
            }
            let current = WorldPosition::try_new(position).map_err(|_| {
                OrdnanceRuntimeError::NonFinitePosition {
                    projectile: live.projectile,
                }
            })?;
            moved.push((live.projectile, current));
        }
        let mut tick = OrdnanceTick::default();
        for (projectile, current) in moved {
            let Some(live) = self.live.get_mut(&projectile) else {
                continue;
            };
            live.previous = live.current;
            live.current = current;
            let segment = live.segment();
            live.state.advance(&segment);
            tick.segments.push(segment);
        }
        for live in self.live.values() {
            if live.state.is_expired() {
                tick.expired.push(live.projectile);
            } else if live.state.is_triggered() {
                tick.triggered.push(live.projectile);
            }
        }
        for projectile in tick.expired.iter().chain(tick.triggered.iter()) {
            self.remove_internal(projectile);
        }
        self.tick = Tick(self.tick.0.saturating_add(1));
        Ok(tick)
    }

    /// Runs one live item's fuse decision against this tick's geometry.
    ///
    /// `paths` are the eligible targets' swept paths for the proximity test
    /// and `impacts` are the swept contacts the caller's ballistics query
    /// reported, in ascending time of impact (F27-C's ordering). The item's
    /// own current segment is the geometry; the arming gate and the trigger
    /// latch are [`OrdnanceState::fuse_decision`]'s, unchanged.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRuntimeError::UnknownProjectile`] when the item is not live.
    pub fn decide(
        &mut self,
        projectile: &ProjectileId,
        paths: &[TargetPath],
        impacts: &[SweptHit],
    ) -> Result<FuseDecision, OrdnanceRuntimeError> {
        let live =
            self.live
                .get_mut(projectile)
                .ok_or(OrdnanceRuntimeError::UnknownProjectile {
                    projectile: *projectile,
                })?;
        let segment = live.segment();
        Ok(live.state.fuse_decision(&segment, paths, impacts))
    }

    /// Drives every guidance tracker with one tick's target observation.
    ///
    /// The session check runs first: an observation stamped with another
    /// generation loses every target, because no id from one session may be
    /// resolved in another. A `Coast` loss leaves the item flying its last
    /// vector; a `Disarm` loss retires the item so it deals nothing; a
    /// `Detonate` loss ends the item here — it is removed from the live set
    /// and reported in [`GuidanceTick::detonated`] so the caller applies the
    /// blast — and its spent tracker is dropped either way. There is no
    /// re-acquisition and no second announcement.
    pub fn guidance_tick(&mut self, session: u64, observation: TargetObservation) -> GuidanceTick {
        let updates = self.guidance.session_tick(session, observation);
        let mut detonated = Vec::new();
        for (projectile, update) in &updates {
            match update {
                GuidanceUpdate::Lost {
                    behavior: LostTargetBehavior::Detonate,
                    ..
                } => detonated.push(*projectile),
                GuidanceUpdate::Disarmed => {
                    if let Some(live) = self.live.get_mut(projectile) {
                        live.state.retire();
                    }
                }
                _ => {}
            }
        }
        for projectile in &detonated {
            self.remove_internal(projectile);
        }
        GuidanceTick { updates, detonated }
    }

    /// Applies one live item's declared status effects to a recipient.
    ///
    /// This is the bridge from a component's declared
    /// [`OrdnanceStatusEffect`] list to the session's bounded ledger: the
    /// recipient is a stable [`StatusEffectTarget`] and the source is the
    /// item's own [`OrdnanceId`], so an effect survives a reload and names
    /// what applied it. The item stays live; a caller that triggered it
    /// decides whether it also ends.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRuntimeError::UnknownProjectile`] when the item is not live,
    /// or [`OrdnanceRuntimeError::Status`] when the ledger refuses the call.
    pub fn apply_statuses(
        &mut self,
        session: u64,
        tick: Tick,
        projectile: &ProjectileId,
        target: StatusEffectTarget,
    ) -> Result<Vec<StatusEffectInstanceId>, OrdnanceRuntimeError> {
        let Some(live) = self.live.get(projectile) else {
            return Err(OrdnanceRuntimeError::UnknownProjectile {
                projectile: *projectile,
            });
        };
        let source = live.definition.ordnance().clone();
        let effects = live.definition.status().to_vec();
        let mut applied = Vec::with_capacity(effects.len());
        for effect in &effects {
            applied.push(self.status.apply(session, tick, target, &source, effect)?);
        }
        Ok(applied)
    }

    /// Advances the status ledger to `tick`, reporting every expiry once.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRuntimeError::Status`] when the
    /// ledger refuses the call.
    pub fn advance_status(
        &mut self,
        session: u64,
        tick: Tick,
    ) -> Result<Vec<ExpiredStatusEffect>, OrdnanceRuntimeError> {
        Ok(self.status.advance_to(session, tick)?)
    }

    /// Routes one already-triggered item's declared damage into
    /// [`HitEvent`]s on one damage node.
    ///
    /// Only a triggered item routes, and each item routes **once**: a second
    /// call is [`OrdnanceRuntimeError::AlreadyRouted`], which is how "apply
    /// damage once even if several collision features report the same hit"
    /// becomes structural at the ordnance layer too. The caller supplies the
    /// target actor and the damage node its part geometry reached; this
    /// function invents no node and no multiplier, and emits one hit per
    /// non-zero declared channel.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRuntimeError`] for a foreign session, an unknown or
    /// un-triggered item, an item already routed, a hit amount the damage
    /// layer refuses, or a session generation zero that cannot stamp a hit.
    pub fn route_trigger(
        &mut self,
        session: u64,
        tick: Tick,
        projectile: &ProjectileId,
        target: ActorId,
        node: DamageNodeKey,
    ) -> Result<Vec<HitEvent>, OrdnanceRuntimeError> {
        if session != self.session {
            return Err(OrdnanceRuntimeError::ForeignSession {
                expected: self.session,
                found: session,
            });
        }
        let Some(live) = self.live.get(projectile) else {
            return Err(OrdnanceRuntimeError::UnknownProjectile {
                projectile: *projectile,
            });
        };
        if !live.state.is_triggered() {
            return Err(OrdnanceRuntimeError::NotTriggered {
                projectile: *projectile,
            });
        }
        if !self.routed.insert(*projectile) {
            return Err(OrdnanceRuntimeError::AlreadyRouted {
                projectile: *projectile,
            });
        }
        let Some(session_id) = SessionId::new(self.session) else {
            return Err(OrdnanceRuntimeError::NoSession);
        };
        let attacker = live.shooter;
        let damage = *live.definition.channels();
        let mut hits = Vec::new();
        for channel in WEAPON_DAMAGE_CHANNELS {
            let amount = damage.amount_on(channel);
            if amount == 0.0 {
                continue;
            }
            let id = self.next_hit_id(session_id, tick);
            hits.push(
                HitEvent::try_new(id, Some(attacker), target, node.clone(), channel, amount)
                    .map_err(OrdnanceRuntimeError::Hit)?,
            );
        }
        Ok(hits)
    }

    /// Registers one actor's booster, starting it at full capacity.
    pub fn register_nitro(&mut self, shooter: ActorId, parameters: NitroParameters) {
        self.nitro.insert(
            shooter,
            NitroLedger::new(self.session, self.tick, self.rate, parameters),
        );
    }

    /// Resolves one actor's nitro activation request for one tick.
    ///
    /// The ledger is the authority on accepted activation; a refused request
    /// consumes nothing. `requested` is the held control, never an elapsed
    /// duration, so no render-frame-scaled burn can be expressed here.
    ///
    /// # Errors
    ///
    /// [`OrdnanceRuntimeError::UnknownBooster`] when the actor has no booster
    /// registered, or [`OrdnanceRuntimeError::Nitro`] when the ledger refuses
    /// the session or the tick.
    pub fn request_nitro(
        &mut self,
        shooter: &ActorId,
        tick: Tick,
        requested: bool,
    ) -> Result<NitroTick, OrdnanceRuntimeError> {
        let session = self.session;
        let ledger = self
            .nitro
            .get_mut(shooter)
            .ok_or(OrdnanceRuntimeError::UnknownBooster { shooter: *shooter })?;
        Ok(ledger.request(session, tick, requested)?)
    }

    /// Removes one item, dropping its guidance tracker with it.
    ///
    /// Idempotent, and a removed id is never reissued: the item is gone and
    /// its tracker is gone, so a later guidance query cannot resurrect it.
    pub fn remove(&mut self, projectile: &ProjectileId) -> Option<LiveOrdnance> {
        self.guidance.remove(projectile);
        self.routed.remove(projectile);
        self.live.remove(projectile)
    }

    /// Removes an item and its tracker without returning it.
    fn remove_internal(&mut self, projectile: &ProjectileId) {
        self.guidance.remove(projectile);
        self.routed.remove(projectile);
        self.live.remove(projectile);
    }

    /// Allocates the next hit id of this session.
    fn next_hit_id(&mut self, session: SessionId, tick: Tick) -> HitEventId {
        let id = HitEventId {
            session,
            tick,
            producer: self.producer,
            sequence: self.next_hit_sequence,
        };
        self.next_hit_sequence = self.next_hit_sequence.wrapping_add(1);
        id
    }
}

/// Refuses a non-finite wind component with the runtime's own error type.
fn check_finite_wind(wind_velocity_m_s: [f64; 3]) -> Result<(), OrdnanceRuntimeError> {
    for (component, value) in wind_velocity_m_s.iter().enumerate() {
        if !value.is_finite() {
            return Err(OrdnanceRuntimeError::NonFiniteWind { component });
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ fixture ----

/// Catalog key of the synthetic direct-explosive component.
pub const SYNTHETIC_DIRECT_KEY: &str = "synthetic.fixture_direct_explosive";
/// Catalog key of the synthetic proximity-flak component.
pub const SYNTHETIC_FLAK_KEY: &str = "synthetic.fixture_proximity_flak";
/// Catalog key of the synthetic guided-rocket component.
pub const SYNTHETIC_GUIDED_KEY: &str = "synthetic.fixture_guided_rocket";
/// Catalog key of the synthetic area-denial component.
pub const SYNTHETIC_AREA_DENIAL_KEY: &str = "synthetic.fixture_area_denial";
/// Catalog key of the synthetic aerial-torpedo component.
pub const SYNTHETIC_TORPEDO_KEY: &str = "synthetic.fixture_aerial_torpedo";
/// Catalog key of the synthetic nitro booster.
pub const SYNTHETIC_NITRO_KEY: &str = "synthetic.fixture_nitro";

/// The synthetic launcher mount key.
pub const SYNTHETIC_LAUNCHER_MOUNT: &str = "ordnance_mount_1";
/// The synthetic weapon-mount node key a destroyed launcher disables.
pub const SYNTHETIC_LAUNCHER_NODE: &str = "weapon_mount_3";
/// The synthetic effect resource.
pub const SYNTHETIC_ORDNANCE_EFFECT_KEY: &str = "synthetic.fixture_ordnance_effect";
/// The synthetic particle resource.
pub const SYNTHETIC_ORDNANCE_PARTICLE_KEY: &str = "synthetic.fixture_ordnance_particles";
/// The synthetic sound resource.
pub const SYNTHETIC_ORDNANCE_SOUND_KEY: &str = "synthetic.fixture_ordnance_sound";

/// Synthetic launch speed, in meters per second.
pub const SYNTHETIC_LAUNCH_SPEED_MPS: f64 = 210.0;
/// Synthetic stack capacity, in units.
pub const SYNTHETIC_STACK_CAPACITY: u64 = 6;
/// Synthetic unit mass, in kilograms.
pub const SYNTHETIC_UNIT_MASS_KG: f64 = 2.4;
/// Ticks after launch before the synthetic proximity fuse arms.
pub const SYNTHETIC_ARMING_TICKS: u64 = 3;
/// The synthetic proximity fuse's trigger radius, in meters.
pub const SYNTHETIC_TRIGGER_RADIUS_M: f64 = 12.0;
/// The synthetic proximity-flak item's lifetime, in ticks.
pub const SYNTHETIC_FLAK_LIFETIME_TICKS: u64 = 45;
/// The synthetic guided item's lifetime, in ticks.
pub const SYNTHETIC_GUIDED_LIFETIME_TICKS: u64 = 240;
/// The synthetic area-denial item's area radius, in meters.
pub const SYNTHETIC_AREA_RADIUS_M: f64 = 55.0;
/// The synthetic area-denial item's area lifetime, in ticks.
pub const SYNTHETIC_AREA_LIFETIME_TICKS: u64 = 90;
/// Armor damage the synthetic direct-explosive component delivers.
pub const SYNTHETIC_ORDNANCE_ARMOR_DAMAGE: f64 = 22.0;
/// Internal damage the synthetic direct-explosive component delivers.
pub const SYNTHETIC_ORDNANCE_INTERNAL_DAMAGE: f64 = 9.0;
/// The synthetic area-denial choke duration, in ticks.
pub const SYNTHETIC_CHOKE_TICKS: u64 = 60;
/// The synthetic area-denial choke strength.
pub const SYNTHETIC_CHOKE_STRENGTH: f64 = 0.4;
/// The synthetic nitro capacity, in capacity units.
pub const SYNTHETIC_NITRO_CAPACITY: f64 = 12.0;
/// The synthetic nitro consumption, in capacity units per second.
pub const SYNTHETIC_NITRO_CONSUMPTION_PER_S: f64 = 3.0;
/// The synthetic nitro recovery, in capacity units per second.
pub const SYNTHETIC_NITRO_RECOVERY_PER_S: f64 = 1.0;
/// The synthetic nitro extra thrust, in newtons.
pub const SYNTHETIC_NITRO_EXTRA_THRUST_N: f64 = 4200.0;

/// The claim the synthetic fixture's values are recorded under.
///
/// The synthetic claim, so nothing here can be mistaken for an observed
/// original value.
#[must_use]
pub fn synthetic_ordnance_fixture_claim() -> ClaimId {
    ClaimId::new("f28.fixture.synthetic").expect("the fixture claim id is valid")
}

/// The synthetic ordnance catalog id for `key`.
#[must_use]
pub fn synthetic_ordnance(key: &str) -> OrdnanceId {
    OrdnanceId::try_new(ContentId::from_source(ContentKind::Weapon, key).expect("valid id"))
        .expect("the weapon namespace is what synthetic_ordnance builds")
}

/// The synthetic launcher mount key, as a damage-node key.
#[must_use]
pub fn synthetic_launcher_mount() -> DamageNodeKey {
    DamageNodeKey::new(SYNTHETIC_LAUNCHER_MOUNT).expect("the fixture mount key is valid")
}

/// The synthetic weapon-mount node a destroyed launcher disables.
#[must_use]
pub fn synthetic_launcher_node() -> DamageNodeKey {
    DamageNodeKey::new(SYNTHETIC_LAUNCHER_NODE).expect("the fixture node key is valid")
}

/// The synthetic media record.
#[must_use]
pub fn synthetic_media() -> OrdnanceMedia {
    OrdnanceMedia::try_new(
        ContentId::from_source(
            ContentKind::HardpointEquipment,
            SYNTHETIC_ORDNANCE_EFFECT_KEY,
        )
        .expect("valid id"),
        ContentId::from_source(ContentKind::Sound, SYNTHETIC_ORDNANCE_SOUND_KEY).expect("valid id"),
        Some(
            ContentId::from_source(
                ContentKind::HardpointEquipment,
                SYNTHETIC_ORDNANCE_PARTICLE_KEY,
            )
            .expect("valid id"),
        ),
    )
    .expect("the fixture media namespaces are valid")
}

/// The synthetic launch geometry.
#[must_use]
pub fn synthetic_launch_geometry() -> LaunchGeometry {
    LaunchGeometry::try_new(
        synthetic_launcher_mount(),
        HardpointKind::WingLeft,
        SYNTHETIC_LAUNCH_SPEED_MPS,
        InheritanceRule::None,
        0,
    )
    .expect("the fixture launch geometry is valid")
}

/// The synthetic stack load.
#[must_use]
pub fn synthetic_stack_load() -> StackLoad {
    StackLoad::try_new(SYNTHETIC_STACK_CAPACITY, SYNTHETIC_UNIT_MASS_KG)
        .expect("the fixture stack load is valid")
}

/// The synthetic proximity fuse.
#[must_use]
pub fn synthetic_proximity_fuse() -> ProximityFuse {
    ProximityFuse::try_new(SYNTHETIC_TRIGGER_RADIUS_M).expect("the fixture radius is valid")
}

/// The synthetic damage channels.
#[must_use]
pub fn synthetic_channels() -> WeaponDamage {
    WeaponDamage::try_new(
        SYNTHETIC_ORDNANCE_ARMOR_DAMAGE,
        SYNTHETIC_ORDNANCE_INTERNAL_DAMAGE,
    )
    .expect("the fixture damage profile is valid")
}

/// The synthetic choke status effect.
#[must_use]
pub fn synthetic_choke() -> OrdnanceStatusEffect {
    OrdnanceStatusEffect::try_new(
        StatusEffectKind::Choke,
        SYNTHETIC_CHOKE_TICKS,
        SYNTHETIC_CHOKE_STRENGTH,
    )
    .expect("the fixture choke is valid")
}

/// The synthetic nitro parameters.
///
/// `NitroTradeoffs::UNMEASURED`, because the original's nitro tradeoff is
/// unknown: this fixture therefore declares **no** authority penalty rather
/// than an invented one.
#[must_use]
pub fn synthetic_nitro_parameters() -> NitroParameters {
    NitroParameters::try_new(
        SYNTHETIC_NITRO_CAPACITY,
        SYNTHETIC_NITRO_CONSUMPTION_PER_S,
        SYNTHETIC_NITRO_RECOVERY_PER_S,
        SYNTHETIC_NITRO_EXTRA_THRUST_N,
        NitroActivationRule::WhileHeld,
        NitroTradeoffs::UNMEASURED,
    )
    .expect("the fixture nitro parameters are valid")
}

/// The synthetic area-denial area effect.
#[must_use]
pub fn synthetic_area_effect() -> AreaEffect {
    AreaEffect::try_new(SYNTHETIC_AREA_RADIUS_M, SYNTHETIC_AREA_LIFETIME_TICKS)
        .expect("the fixture area effect is valid")
}

/// The synthetic proximity-flak projectile component.
///
/// The AC01 minimum scenario's component: a proximity fuse that arms after
/// [`SYNTHETIC_ARMING_TICKS`], triggers within
/// [`SYNTHETIC_TRIGGER_RADIUS_M`] meters of a target's swept path, and is
/// unguided because a flak shell is unguided by definition.
#[must_use]
pub fn synthetic_proximity_flak() -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance(SYNTHETIC_FLAK_KEY),
        OrdnanceFamily::ProximityFlak,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(SYNTHETIC_ARMING_TICKS),
        FuseRule::Proximity(synthetic_proximity_fuse()),
        GuidanceRule::Unguided,
        SYNTHETIC_FLAK_LIFETIME_TICKS,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture flak component is valid")
}

/// The synthetic direct-explosive projectile component.
#[must_use]
pub fn synthetic_direct_explosive() -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance(SYNTHETIC_DIRECT_KEY),
        OrdnanceFamily::DirectExplosive,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(SYNTHETIC_ARMING_TICKS),
        FuseRule::Impact,
        GuidanceRule::Unguided,
        SYNTHETIC_GUIDED_LIFETIME_TICKS,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture direct component is valid")
}

/// The synthetic guided-rocket projectile component.
///
/// Its lost-target behavior is [`LostTargetBehavior::Detonate`]: a
/// tagged-target weapon whose tag is dropped detonates rather than vanishing
/// with no outcome.
#[must_use]
pub fn synthetic_guided_rocket() -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance(SYNTHETIC_GUIDED_KEY),
        OrdnanceFamily::GuidedRocket,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTravelMetres(60.0),
        FuseRule::Impact,
        GuidanceRule::Targeted {
            lost_target: LostTargetBehavior::Detonate,
        },
        SYNTHETIC_GUIDED_LIFETIME_TICKS,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture guided component is valid")
}

/// The synthetic area-denial projectile component.
///
/// Unguided and clock-ended, because its effect is the bounded area.
#[must_use]
pub fn synthetic_area_denial() -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance(SYNTHETIC_AREA_DENIAL_KEY),
        OrdnanceFamily::AreaDenialEngine,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(SYNTHETIC_ARMING_TICKS),
        FuseRule::Timed {
            ticks: SYNTHETIC_AREA_LIFETIME_TICKS,
        },
        GuidanceRule::Unguided,
        SYNTHETIC_AREA_LIFETIME_TICKS,
        Some(synthetic_area_effect()),
        synthetic_channels(),
        vec![synthetic_choke()],
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture area-denial component is valid")
}

/// The synthetic aerial-torpedo projectile component.
///
/// A proximity fuse *and* a proximity-flak fuse on one item would be the same
/// behavior twice, so the torpedo uses the timed fuse its family needs and
/// stays unguided: whether the original's torpedo was a seeker is exactly
/// the kind of fact F28-D has to measure.
#[must_use]
pub fn synthetic_aerial_torpedo() -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance(SYNTHETIC_TORPEDO_KEY),
        OrdnanceFamily::AerialTorpedo,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(SYNTHETIC_ARMING_TICKS),
        FuseRule::Timed {
            ticks: SYNTHETIC_GUIDED_LIFETIME_TICKS,
        },
        GuidanceRule::Unguided,
        SYNTHETIC_GUIDED_LIFETIME_TICKS,
        None,
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture torpedo is valid")
}

/// The synthetic nitro booster component.
#[must_use]
pub fn synthetic_nitro() -> NitroOrdnance {
    NitroOrdnance::try_new(
        synthetic_ordnance(SYNTHETIC_NITRO_KEY),
        synthetic_nitro_parameters(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture nitro component is valid")
}

/// A registry holding one synthetic component of every family.
///
/// The registry is what F28-D's catalogue audit walks: six families, six
/// distinct ids, six distinct declared behaviors. This stage's registry is
/// complete with respect to **this fixture only**.
#[must_use]
pub fn synthetic_registry() -> OrdnanceRegistry {
    let mut registry = OrdnanceRegistry::new();
    for projectile in [
        synthetic_direct_explosive(),
        synthetic_proximity_flak(),
        synthetic_guided_rocket(),
        synthetic_area_denial(),
        synthetic_aerial_torpedo(),
    ] {
        registry
            .register(OrdnanceComponent::Projectile(Box::new(projectile)))
            .expect("the fixture ids are distinct");
    }
    registry
        .register(OrdnanceComponent::Nitro(Box::new(synthetic_nitro())))
        .expect("the fixture nitro id is distinct");
    registry
}
