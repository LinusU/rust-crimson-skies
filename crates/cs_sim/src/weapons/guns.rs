//! Guns, ammunition, hardpoints and swept ballistic hits (F27-A).
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stages
//! `### F27-A` and `### F27-C`. Shared contract:
//! `docs/contracts/FLIGHT-PHYSICS.md`, "Collision and ballistic tests".
//!
//! This module is the **runtime** half of the weapon contract: the records
//! one session resolves against, with no Bevy, Avian, renderer or file
//! dependency (`docs/01-ARCHITECTURE.md`). The declared,
//! provenance-carrying half is `cs_content::weapons`; the conversion
//! boundary is `cs_app::weapons`.
//!
//! # What is defined here and what is not
//!
//! Stage F27-A defined the *typed* contract and a minimal synthetic fixture:
//!
//! * [`GunDefinition`] — one gun's declared behavior, with every field the
//!   sheet's deliverable names kept separate: mount, caliber, ammunition
//!   type, rate, muzzle velocity, lifetime, spread, damage channels,
//!   effects and sound.
//! * [`WeaponState`] — one actor's live weapon state: the selected
//!   [`GunBank`], per-mount cooldown in ticks, per-mount remaining rounds
//!   and the disabled mounts.
//! * [`FireIntent`] in, [`FireResolution`] out. Fire intents are resolved
//!   **once** by the authoritative [`FireResolver`], which is the only
//!   thing that consumes a round, starts a cooldown, spawns a projectile
//!   or names a sound.
//! * [`Ballistics::sweep`] — the swept-segment query with relative motion
//!   and a once-per-projectile ledger. This is AC01's minimum scenario at
//!   this stage.
//!
//! Stage F27-C adds the **query stage** that turns a sweep into damage, and
//! decides where the seam between declared policy and geometry runs — see
//! `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`:
//!
//! * [`SweepCandidate`] — one world candidate: a swept box, the damage node a
//!   contact with it lands on, and the declared relation the rules filter it
//!   under.
//! * [`WeaponRules::admitted`] — the declared-policy half of the query, and
//!   [`Ballistics::sweep_with_sources`] its geometry half, kept as two
//!   functions so neither can grow the other's rules.
//! * [`GunHitRouter`] — the conversion: one accepted shot's contacts become
//!   [`crate::damage::HitEvent`]s carrying the gun definition's own
//!   per-channel damage amounts.
//!
//! What is therefore still absent, and who owns it: the per-tick cadence loop
//! and the mount transforms read out of the *live* aircraft hierarchy
//! (F27-B), an Avian body or collider for a projectile and the collision
//! features that report a part's swept box (F27-B), the audio and
//! muzzle-effect consumers and the player's bank-selection input (F27-C's
//! ECS half), and every original weapon/ammunition measurement (F27-D).
//!
//! # Designed vocabulary, not original data
//!
//! Every mount kind, caliber, rate, spread model, damage number and
//! interaction rule here is **newly authored project design**, carried by
//! synthetic fixture values. The original ammunition catalogue — slug,
//! armor-piercing, dum-dum and explosive are *discovery leads* per F27
//! non-negotiable 1, not measurements — is deliberately **not** enumerated;
//! see `docs/findings/2026-10-01-f27-a-weapon-ammo-schemas-and-fire-events.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::{UnitVec3, WorldPosition};

use crate::damage::{ActorId, DamageChannel, DamageNodeKey, HitEvent, HitEventError, HitEventId};
use crate::environment::{air_relative_velocity_m_s, world_velocity_from_air_m_s};
use crate::targeting::Allegiance;

// ---------------------------------------------------------------- identity ----

/// One ammunition **type** identifier.
///
/// Ammunition is deliberately *not* an enum here. The original catalogue's
/// actual types are unmeasured (F27 "Research boundary"; the public manual
/// establishes no ammunition table) and F27 non-negotiable behavior 1 names
/// slug, armor-piercing, dum-dum and explosive as *leads* while forbidding an
/// unverified multiplier table. So a type is an opaque [`ContentId`] in the
/// `ammo` namespace: an importer that has read real ammunition data supplies
/// real ids, and nothing is defaulted in its absence.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AmmunitionId(ContentId);

impl AmmunitionId {
    /// Wraps an ammunition content id.
    ///
    /// # Errors
    ///
    /// [`AmmunitionIdError::KindMismatch`] when the id is not in the `ammo`
    /// namespace.
    pub fn try_new(id: ContentId) -> Result<Self, AmmunitionIdError> {
        if id.kind() != ContentKind::Ammo {
            return Err(AmmunitionIdError::KindMismatch { id });
        }
        Ok(Self(id))
    }

    /// The catalog id of the ammunition type.
    #[must_use]
    pub const fn id(&self) -> &ContentId {
        &self.0
    }

    /// The `namespace/key` text of the ammunition type.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for AmmunitionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.as_str())
    }
}

/// Why an [`AmmunitionId`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AmmunitionIdError {
    /// The content id is not in the `ammo` namespace.
    KindMismatch {
        /// The offending id.
        id: ContentId,
    },
}

impl fmt::Display for AmmunitionIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KindMismatch { id } => {
                write!(f, "ammunition id {id} is not in the ammo namespace")
            }
        }
    }
}

impl std::error::Error for AmmunitionIdError {}

/// Where a weapon sits on the airframe.
///
/// The list is **designed**, not measured: it is the smallest set that
/// states AC02 ("a disabled *wing* gun") and the per-mount discipline of
/// non-negotiable 2. F27-D maps the original hardpoint set onto it, and a
/// kind the original does not have simply stays unused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GunMountKind {
    /// A nose or forward-fuselage gun.
    Nose,
    /// A wing gun on the airframe's left side.
    WingLeft,
    /// A wing gun on the airframe's right side.
    WingRight,
    /// A tail gun.
    Tail,
    /// A fuselage or gondola gun.
    Gondola,
}

impl GunMountKind {
    /// Every mount kind, in a stable order.
    pub const ALL: &'static [GunMountKind] = &[
        Self::Nose,
        Self::WingLeft,
        Self::WingRight,
        Self::Tail,
        Self::Gondola,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Nose => "nose",
            Self::WingLeft => "wing_left",
            Self::WingRight => "wing_right",
            Self::Tail => "tail",
            Self::Gondola => "gondola",
        }
    }

    /// Whether this mount is on a wing. AC02's "disabled wing gun" is the
    /// discriminating case: a wing gun can be destroyed independently of
    /// the fuselage, so the gate has to be per mount, not per airframe.
    #[must_use]
    pub const fn is_wing(self) -> bool {
        matches!(self, Self::WingLeft | Self::WingRight)
    }
}

impl fmt::Display for GunMountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// -------------------------------------------------------------- definition ----

/// How fast a gun may fire, in the simulation's own tick unit.
///
/// A gun's rate is *ticks between shots*, never wall-clock seconds or a
/// frame-time approximation (`FLIGHT-PHYSICS`: commands belong to one
/// simulation tick).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GunRate {
    ticks_between_shots: u32,
}

impl GunRate {
    /// Builds a rate, refusing a zero interval — even a fastest-firing gun
    /// needs one tick between two accepted shots.
    ///
    /// # Errors
    ///
    /// [`GunDefinitionError::ZeroRateInterval`] when the interval is zero.
    pub fn try_new(ticks_between_shots: u32) -> Result<Self, GunDefinitionError> {
        if ticks_between_shots == 0 {
            return Err(GunDefinitionError::ZeroRateInterval);
        }
        Ok(Self {
            ticks_between_shots,
        })
    }

    /// Ticks the gun must wait between two accepted shots.
    #[must_use]
    pub const fn ticks_between_shots(self) -> u32 {
        self.ticks_between_shots
    }
}

/// How a round inherits the firing airframe's velocity.
///
/// F27 non-negotiable 2 makes inherited velocity an *explicit verified
/// rule*, so it is declared data with its own type rather than an assumption
/// buried in the spawn math. The original's rule is unmeasured — the public
/// manual establishes no ballistic parameter — so which variant the original
/// used is F27-D's audit, and the content schema carries this field
/// `Resolved`.
///
/// Convergence is deliberately **not** modelled here: the distance at which
/// a wing pair's barrels meet is unmeasured, so [`MountTransform::forward`]
/// already carries the *resolved* direction and this stage invents no
/// convergence geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InheritanceRule {
    /// The round keeps the airframe's whole world velocity; the muzzle
    /// velocity is added to it.
    Full,
    /// The round keeps a declared fraction of the airframe's world velocity.
    Fraction {
        /// The declared share of the airframe's velocity.
        share: f64,
    },
    /// The round keeps none of the airframe's velocity: its world velocity is
    /// the muzzle velocity alone.
    None,
}

impl InheritanceRule {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Fraction { .. } => "fraction",
            Self::None => "none",
        }
    }
}

impl fmt::Display for InheritanceRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fraction { share } => write!(f, "fraction({share})"),
            other => f.write_str(other.label()),
        }
    }
}

/// The damage channels one round delivers.
///
/// A gun delivers a *profile*, not a scalar: the same round may do more to
/// structure than to armor. What the gun declares is the amount **per
/// channel**; how a target's armor intercepts a channel is the damage
/// resolver's declared rule, not the gun's, so no multiplier is invented
/// here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeaponDamage {
    /// Damage routed on [`DamageChannel::Armor`].
    pub armor: f64,
    /// Damage routed on [`DamageChannel::Internal`].
    pub internal: f64,
}

/// The channels a gun's damage profile routes on, in the order the F27-C
/// routing emits them: armor first, then internal.
///
/// The order is a declared property of the routing rather than of the damage
/// graph: it is what makes the emitted [`HitEventId`]s deterministic, because
/// the two channels of one contact take consecutive sequence numbers in this
/// order. `DamageChannel` is the damage layer's vocabulary and this crate
/// states the weapon-side order explicitly instead of borrowing a list from
/// a module F27 does not own.
pub const WEAPON_DAMAGE_CHANNELS: [DamageChannel; 2] =
    [DamageChannel::Armor, DamageChannel::Internal];

impl WeaponDamage {
    /// Builds a damage profile, refusing non-finite or negative amounts.
    ///
    /// # Errors
    ///
    /// [`GunDefinitionError::NonFiniteDamage`] or
    /// [`GunDefinitionError::NegativeDamage`].
    pub fn try_new(armor: f64, internal: f64) -> Result<Self, GunDefinitionError> {
        for amount in [armor, internal] {
            if !amount.is_finite() {
                return Err(GunDefinitionError::NonFiniteDamage);
            }
            if amount < 0.0 {
                return Err(GunDefinitionError::NegativeDamage { amount });
            }
        }
        Ok(Self { armor, internal })
    }

    /// The amount this round delivers on `channel`.
    #[must_use]
    pub const fn amount_on(&self, channel: DamageChannel) -> f64 {
        match channel {
            DamageChannel::Armor => self.armor,
            DamageChannel::Internal => self.internal,
        }
    }
}

/// How a gun's rounds scatter around its declared direction.
///
/// F27 non-negotiable 2 makes convergence and inherited velocity *explicit
/// verified rules*; spread is the third such knob. At this stage spread is
/// only a declared cone half-angle in radians — the sampling model the
/// original used is unmeasured, so no distribution is chosen here and no
/// sample is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpreadCone {
    half_angle_radians: f64,
}

impl SpreadCone {
    /// Builds a cone, refusing a non-finite or out-of-range half-angle.
    ///
    /// # Errors
    ///
    /// [`GunDefinitionError::NonFiniteSpread`] or
    /// [`GunDefinitionError::SpreadOutOfRange`].
    pub fn try_new(half_angle_radians: f64) -> Result<Self, GunDefinitionError> {
        if !half_angle_radians.is_finite() {
            return Err(GunDefinitionError::NonFiniteSpread);
        }
        if !(0.0..=std::f64::consts::FRAC_PI_2).contains(&half_angle_radians) {
            return Err(GunDefinitionError::SpreadOutOfRange { half_angle_radians });
        }
        Ok(Self { half_angle_radians })
    }

    /// The declared cone half-angle, in radians.
    #[must_use]
    pub const fn half_angle_radians(self) -> f64 {
        self.half_angle_radians
    }
}

/// The declared behavior of one gun.
///
/// Every field the sheet's deliverable names is present and separate:
/// `mount`, `caliber`, `ammunition`, `rate`, `muzzle_velocity_mps`,
/// `lifetime_ticks`, `spread`, `damage` (the channels), `effect` and
/// `sound`.
///
/// `mount` is a [`DamageNodeKey`] — the *same* identity discipline the F29
/// damage graph applies to its `WeaponMount` nodes. Reusing the graph's own
/// key is what makes non-negotiable 2's "mount transforms come from the live
/// aircraft hierarchy/damage state" concrete: a mount is disabled by exactly
/// the damage transition that destroyed it, and no gun can be silenced
/// through an unrelated node.
#[derive(Clone, Debug, PartialEq)]
pub struct GunDefinition {
    mount: DamageNodeKey,
    kind: GunMountKind,
    caliber: String,
    ammunition: AmmunitionId,
    rate: GunRate,
    muzzle_velocity_mps: f64,
    lifetime_ticks: u64,
    spread: SpreadCone,
    damage: WeaponDamage,
    inheritance: InheritanceRule,
    effect: ContentId,
    sound: ContentId,
}

impl GunDefinition {
    /// Assembles a gun definition.
    ///
    /// # Errors
    ///
    /// [`GunDefinitionError`] on an empty caliber, a non-finite or
    /// non-positive muzzle velocity, a zero round lifetime, an effect id
    /// outside the `hardpoint_equipment` namespace or a sound id outside the
    /// `sound` namespace.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        mount: DamageNodeKey,
        kind: GunMountKind,
        caliber: impl Into<String>,
        ammunition: AmmunitionId,
        rate: GunRate,
        muzzle_velocity_mps: f64,
        lifetime_ticks: u64,
        spread: SpreadCone,
        damage: WeaponDamage,
        inheritance: InheritanceRule,
        effect: ContentId,
        sound: ContentId,
    ) -> Result<Self, GunDefinitionError> {
        let caliber = caliber.into();
        if caliber.trim().is_empty() {
            return Err(GunDefinitionError::EmptyCaliber);
        }
        if !muzzle_velocity_mps.is_finite() {
            return Err(GunDefinitionError::NonFiniteMuzzleVelocity);
        }
        if muzzle_velocity_mps <= 0.0 {
            return Err(GunDefinitionError::NonPositiveMuzzleVelocity {
                muzzle_velocity_mps,
            });
        }
        if lifetime_ticks == 0 {
            return Err(GunDefinitionError::ZeroLifetime);
        }
        if let InheritanceRule::Fraction { share } = inheritance
            && (!share.is_finite() || !(0.0..=1.0).contains(&share))
        {
            return Err(GunDefinitionError::InvalidInheritanceShare { share });
        }
        if effect.kind() != ContentKind::HardpointEquipment {
            return Err(GunDefinitionError::EffectKindMismatch { id: effect });
        }
        if sound.kind() != ContentKind::Sound {
            return Err(GunDefinitionError::SoundKindMismatch { id: sound });
        }
        Ok(Self {
            mount,
            kind,
            caliber,
            ammunition,
            rate,
            muzzle_velocity_mps,
            lifetime_ticks,
            spread,
            damage,
            inheritance,
            effect,
            sound,
        })
    }

    /// The mount this gun occupies, by damage-node key.
    #[must_use]
    pub const fn mount(&self) -> &DamageNodeKey {
        &self.mount
    }

    /// Where on the airframe this mount sits.
    #[must_use]
    pub const fn kind(&self) -> GunMountKind {
        self.kind
    }

    /// The declared caliber text. Free text, not an enum: the original
    /// caliber vocabulary is unmeasured, so a closed set would be a
    /// fabrication.
    #[must_use]
    pub fn caliber(&self) -> &str {
        &self.caliber
    }

    /// The ammunition type this gun fires.
    #[must_use]
    pub const fn ammunition(&self) -> &AmmunitionId {
        &self.ammunition
    }

    /// The declared rate, in ticks between shots.
    #[must_use]
    pub const fn rate(&self) -> GunRate {
        self.rate
    }

    /// The declared muzzle velocity, in meters per second.
    #[must_use]
    pub const fn muzzle_velocity_mps(&self) -> f64 {
        self.muzzle_velocity_mps
    }

    /// How many ticks one round lives after it leaves the mount.
    #[must_use]
    pub const fn lifetime_ticks(&self) -> u64 {
        self.lifetime_ticks
    }

    /// The declared spread cone.
    #[must_use]
    pub const fn spread(&self) -> SpreadCone {
        self.spread
    }

    /// The damage channels one round delivers.
    #[must_use]
    pub const fn damage(&self) -> &WeaponDamage {
        &self.damage
    }

    /// The declared rule for how much of the firing airframe's velocity a
    /// round inherits.
    #[must_use]
    pub const fn inheritance(&self) -> InheritanceRule {
        self.inheritance
    }

    /// The muzzle/hit effect resource id.
    #[must_use]
    pub const fn effect(&self) -> &ContentId {
        &self.effect
    }

    /// The sound resource id an accepted shot plays.
    #[must_use]
    pub const fn sound(&self) -> &ContentId {
        &self.sound
    }
}

impl fmt::Display for GunDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} gun on the {} mount",
            self.caliber, self.ammunition, self.kind
        )
    }
}

/// Why a [`GunDefinition`] or one of its sub-records was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum GunDefinitionError {
    /// The rate interval was zero; even the fastest gun needs one tick
    /// between two accepted shots.
    ZeroRateInterval,
    /// The caliber text was empty or whitespace.
    EmptyCaliber,
    /// The muzzle velocity was NaN or infinite.
    NonFiniteMuzzleVelocity,
    /// The muzzle velocity was zero or negative.
    NonPositiveMuzzleVelocity {
        /// The rejected value.
        muzzle_velocity_mps: f64,
    },
    /// The round lifetime was zero: a round that dies on the muzzle never
    /// exists as a projectile.
    ZeroLifetime,
    /// A damage amount was NaN or infinite.
    NonFiniteDamage,
    /// A damage amount was negative.
    NegativeDamage {
        /// The rejected value.
        amount: f64,
    },
    /// The spread half-angle was NaN or infinite.
    NonFiniteSpread,
    /// The spread half-angle fell outside `[0, π/2]`.
    SpreadOutOfRange {
        /// The rejected value.
        half_angle_radians: f64,
    },
    /// An [`InheritanceRule::Fraction`] share was NaN, infinite or outside
    /// `[0, 1]`.
    InvalidInheritanceShare {
        /// The rejected share.
        share: f64,
    },
    /// The effect id is not in the `hardpoint_equipment` namespace.
    EffectKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The sound id is not in the `sound` namespace.
    SoundKindMismatch {
        /// The offending id.
        id: ContentId,
    },
}

impl fmt::Display for GunDefinitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroRateInterval => {
                write!(f, "a gun rate must be at least one tick between shots")
            }
            Self::EmptyCaliber => write!(f, "a gun caliber must not be empty"),
            Self::NonFiniteMuzzleVelocity => write!(f, "muzzle velocity must be finite"),
            Self::NonPositiveMuzzleVelocity {
                muzzle_velocity_mps,
            } => {
                write!(
                    f,
                    "muzzle velocity must be positive, got {muzzle_velocity_mps}"
                )
            }
            Self::ZeroLifetime => write!(f, "a round's lifetime must be at least one tick"),
            Self::NonFiniteDamage => write!(f, "gun damage must be finite"),
            Self::NegativeDamage { amount } => {
                write!(f, "gun damage must not be negative, got {amount}")
            }
            Self::NonFiniteSpread => write!(f, "spread half-angle must be finite"),
            Self::SpreadOutOfRange { half_angle_radians } => {
                write!(
                    f,
                    "spread half-angle {half_angle_radians} rad is outside [0, π/2]"
                )
            }
            Self::InvalidInheritanceShare { share } => {
                write!(
                    f,
                    "an inherited-velocity share must be finite and within [0, 1], got {share}"
                )
            }
            Self::EffectKindMismatch { id } => {
                write!(
                    f,
                    "gun effect id {id} is not in the hardpoint_equipment namespace"
                )
            }
            Self::SoundKindMismatch { id } => {
                write!(f, "gun sound id {id} is not in the sound namespace")
            }
        }
    }
}

impl std::error::Error for GunDefinitionError {}

// ------------------------------------------------------------ interaction ----

/// Whether a gun's own rounds may hit the airframe that fired them.
///
/// F27 non-negotiable 4 requires this to be defined by evidence or marked
/// unknown; it is a **declared** rule, never an implicit exclusion. The
/// content schema carries it `Resolved`, so an unmeasured rule stays
/// unknown and refuses to lower.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelfHitRule {
    /// A round never damages the airframe that fired it.
    Excluded,
    /// A round may damage its own airframe, subject to the damage graph.
    Allowed,
}

impl SelfHitRule {
    /// Every rule, in a stable order.
    pub const ALL: &'static [SelfHitRule] = &[Self::Excluded, Self::Allowed];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Excluded => "excluded",
            Self::Allowed => "allowed",
        }
    }
}

impl fmt::Display for SelfHitRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which declared relations a gun's rounds may damage.
///
/// The relations are the F30-A [`Allegiance`] vocabulary, so a gun never
/// re-derives hostility: an undeclared pair carries no relation and is
/// admitted only by [`FriendlyFireRule::Everyone`], never by a rule that
/// asserts hostility it does not have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FriendlyFireRule {
    /// Only a declared hostile relation may be damaged.
    HostileOnly,
    /// A declared hostile *or neutral* relation may be damaged; an ally
    /// never is.
    NonFriendly,
    /// Every actor except the shooter may be damaged, including one with no
    /// declared relation at all.
    Everyone,
}

impl FriendlyFireRule {
    /// Every rule, in a stable order.
    pub const ALL: &'static [FriendlyFireRule] =
        &[Self::HostileOnly, Self::NonFriendly, Self::Everyone];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HostileOnly => "hostile_only",
            Self::NonFriendly => "non_friendly",
            Self::Everyone => "everyone",
        }
    }

    /// Whether this rule admits a target with the given declared relation.
    /// `None` is an *undeclared* pair, which is not the same statement as
    /// "friendly": only [`FriendlyFireRule::Everyone`] admits it.
    #[must_use]
    pub const fn allows(self, relation: Option<Allegiance>) -> bool {
        matches!(
            (self, relation),
            (Self::Everyone, _)
                | (Self::HostileOnly, Some(Allegiance::Hostile))
                | (
                    Self::NonFriendly,
                    Some(Allegiance::Hostile | Allegiance::Neutral)
                )
        )
    }
}

impl fmt::Display for FriendlyFireRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared interaction rules one gun's rounds run under.
///
/// `self_hit` and `friendly_fire` are load-bearing here:
/// [`WeaponRules::admit_candidates`] is the query that assembles the
/// [`Ballistics::sweep`] candidate list — [`WeaponRules::eligible`] is that same
/// decision with the nodes and relations dropped — so a gun cannot hit its own
/// shooter or an ally unless the *declared* rule admits it
/// (non-negotiable 4).
///
/// `penetration`, `ricochet` and `ammo_switching` are declared and carried
/// but **read by no production code**, at this stage or after F27-C's routing.
/// They are behaviors a content schema must be able to state before F27-D
/// audits what the original actually did; inventing a penetration or
/// ricochet *model* here would be exactly the "simulator features
/// unsupported by game content" F27 non-negotiable 4 forbids, and in-flight
/// ammunition switching needs a multi-type per-mount inventory whose
/// selection rule is unmeasured.
///
/// The gap is therefore **deferred to F27-D, not silently left open**, and
/// the deferral is visible in the declared schema rather than only in prose:
/// `cs_content::weapons::InteractionOption::applied_by` names the production
/// path that applies an option, and
/// `cs_content::weapons::InteractionOption::deferred_to` returns the stage
/// that must resolve an unapplied one together with the reason nothing reads
/// it yet, so an audit asks the content contract instead of trusting a
/// findings file. Those names are prose rather than links because `cs_sim` does
/// not depend on `cs_content`. F27-D closes the gap against the
/// installation's ammunition data, or the options stay unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponRules {
    /// Whether a round may hit the airframe that fired it.
    pub self_hit: SelfHitRule,
    /// Which declared relations a round may damage.
    pub friendly_fire: FriendlyFireRule,
    /// Whether this ammunition type is declared to penetrate what it hits.
    pub penetration: bool,
    /// Whether this ammunition type is declared to ricochet.
    pub ricochet: bool,
    /// Whether a pilot may change ammunition type in flight.
    pub ammo_switching: bool,
}

impl WeaponRules {
    /// Whether this gun's rounds may damage `target` at all, given the
    /// declared relation of `shooter` toward it.
    #[must_use]
    pub fn admits(&self, shooter: ActorId, target: ActorId, relation: Option<Allegiance>) -> bool {
        if target == shooter && self.self_hit == SelfHitRule::Excluded {
            return false;
        }
        self.friendly_fire.allows(relation)
    }

    /// The subset of `candidates` this gun's rules admit, paired with the
    /// relation each one was offered under. This is the candidate list a
    /// [`Ballistics::sweep`] consumes: the sweep itself is pure geometry,
    /// and eligibility is decided here by declared rules rather than by
    /// proximity.
    #[must_use]
    pub fn eligible(
        &self,
        shooter: ActorId,
        candidates: impl IntoIterator<Item = (SweepTarget, Option<Allegiance>)>,
    ) -> Vec<SweepTarget> {
        candidates
            .into_iter()
            .filter(|(target, relation)| self.admits(shooter, target.actor, *relation))
            .map(|(target, _)| target)
            .collect()
    }

    /// The F27-C candidate query's **declared-policy half**: which of these
    /// world candidates this gun's rules admit, keeping each admitted
    /// candidate whole — its swept box, the damage node a contact with it
    /// lands on and the relation it was offered under.
    ///
    /// This is where the boundary decision recorded in
    /// `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`
    /// lives. Filtering is a *rules query*, not geometry and not damage:
    /// [`Ballistics::sweep`] stays a pure slab test over an already-filtered
    /// list, and `cs_sim::damage::DamageResolver` consumes typed hits and
    /// knows nothing about allegiance or gun rules. So a declared rule is
    /// applied exactly here, and never by proximity or inside the sweep.
    ///
    /// The admitted order is the caller's candidate order, unchanged: the
    /// filter never reorders, so a caller's stable candidate list keeps its
    /// own tie-breaking.
    ///
    /// [`WeaponRules::eligible`] is this query with the nodes and relations
    /// dropped; both decide through the same [`WeaponRules::admits`].
    #[must_use]
    pub fn admit_candidates(
        &self,
        shooter: ActorId,
        candidates: impl IntoIterator<Item = SweepCandidate>,
    ) -> Vec<SweepCandidate> {
        candidates
            .into_iter()
            .filter(|candidate| self.admits(shooter, candidate.target.actor, candidate.relation))
            .collect()
    }
}

// ----------------------------------------------------------------- state ----

/// A gun bank: the group of mounts that fire together when selected.
///
/// The *bank vocabulary* is designed, not measured. The original cockpit's
/// bank names and cycle order (nose / wings / tail / all are the common
/// community reading, not evidence) are unmeasured, so this stage models a
/// bank as a set of [`DamageNodeKey`]s and leaves naming and cycling to
/// F27-C, which owns the player's selection input.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct GunBank {
    mounts: BTreeSet<DamageNodeKey>,
}

impl GunBank {
    /// Builds a bank from its mount keys.
    ///
    /// # Errors
    ///
    /// [`GunBankError::Empty`] when no mount is named: a bank with no mounts
    /// could never fire, so it is a malformed declaration rather than an
    /// inert one.
    pub fn try_new(mounts: impl IntoIterator<Item = DamageNodeKey>) -> Result<Self, GunBankError> {
        let mounts: BTreeSet<DamageNodeKey> = mounts.into_iter().collect();
        if mounts.is_empty() {
            return Err(GunBankError::Empty);
        }
        Ok(Self { mounts })
    }

    /// The bank's mounts, in the stable key order.
    #[must_use]
    pub fn mounts(&self) -> &BTreeSet<DamageNodeKey> {
        &self.mounts
    }

    /// Whether the bank names `mount`.
    #[must_use]
    pub fn contains(&self, mount: &DamageNodeKey) -> bool {
        self.mounts.contains(mount)
    }

    /// How many mounts the bank names.
    #[must_use]
    pub fn len(&self) -> usize {
        self.mounts.len()
    }

    /// Whether the bank names no mount. A [`GunBank`] cannot be *built*
    /// empty; this reports the state of a *cleared* selection, which is how
    /// "no guns selected" is represented.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }
}

/// Why a [`GunBank`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GunBankError {
    /// A bank must name at least one mount.
    Empty,
}

impl fmt::Display for GunBankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a gun bank must name at least one mount"),
        }
    }
}

impl std::error::Error for GunBankError {}

/// One actor's live weapon state: the selected bank, per-mount cooldown in
/// ticks, per-mount remaining rounds and the disabled mounts.
///
/// The cooldown is stored in *ticks remaining*, not as a wall-clock
/// deadline, so it counts down exactly one tick per simulation tick and can
/// never depend on a render frame rate (`FLIGHT-PHYSICS`: integer
/// simulation ticks).
#[derive(Clone, Debug, PartialEq)]
pub struct WeaponState {
    selected: GunBank,
    cooldown_ticks: BTreeMap<DamageNodeKey, u64>,
    ammunition: BTreeMap<DamageNodeKey, u64>,
    disabled_mounts: BTreeSet<DamageNodeKey>,
}

impl WeaponState {
    /// Builds the state of one actor's mounted guns.
    ///
    /// `definitions` are the actor's mounted [`GunDefinition`]s; each starts
    /// loaded with `starting_rounds` and off cooldown.
    ///
    /// # Errors
    ///
    /// [`GunStateError::InvalidStartingRounds`] when `starting_rounds` is
    /// zero, [`GunStateError::DuplicateMount`] when two definitions share a
    /// mount key, and [`GunStateError::UnknownSelection`] when the selected
    /// bank names a mount the actor does not carry.
    pub fn try_new(
        definitions: &[GunDefinition],
        selected: GunBank,
        starting_rounds: u64,
    ) -> Result<Self, GunStateError> {
        if starting_rounds == 0 {
            return Err(GunStateError::InvalidStartingRounds);
        }
        let mut cooldown_ticks = BTreeMap::new();
        let mut ammunition = BTreeMap::new();
        for definition in definitions {
            let mount = definition.mount();
            if ammunition.insert(mount.clone(), starting_rounds).is_some() {
                return Err(GunStateError::DuplicateMount {
                    mount: mount.clone(),
                });
            }
            cooldown_ticks.insert(mount.clone(), 0);
        }
        for mount in selected.mounts() {
            if !ammunition.contains_key(mount) {
                return Err(GunStateError::UnknownSelection {
                    mount: mount.clone(),
                });
            }
        }
        Ok(Self {
            selected,
            cooldown_ticks,
            ammunition,
            disabled_mounts: BTreeSet::new(),
        })
    }

    /// The currently selected bank.
    #[must_use]
    pub const fn selected(&self) -> &GunBank {
        &self.selected
    }

    /// Replaces the selected bank.
    ///
    /// Selection changes *nothing else*: it never refills ammunition and
    /// never resets a cooldown, so switching bank mid-cooldown can neither
    /// duplicate a shot nor hand the pilot free rounds (AC03's state half).
    pub fn select(&mut self, bank: GunBank) {
        self.selected = bank;
    }

    /// Ticks the named mount must still wait before it may fire.
    #[must_use]
    pub fn cooldown_ticks(&self, mount: &DamageNodeKey) -> u64 {
        self.cooldown_ticks.get(mount).copied().unwrap_or(0)
    }

    /// Rounds remaining in the named mount.
    #[must_use]
    pub fn ammunition(&self, mount: &DamageNodeKey) -> u64 {
        self.ammunition.get(mount).copied().unwrap_or(0)
    }

    /// Whether the named mount is disabled.
    #[must_use]
    pub fn is_disabled(&self, mount: &DamageNodeKey) -> bool {
        self.disabled_mounts.contains(mount)
    }

    /// The currently disabled mounts, in the stable key order.
    #[must_use]
    pub fn disabled_mounts(&self) -> &BTreeSet<DamageNodeKey> {
        &self.disabled_mounts
    }

    /// Disables the named mount — the weapon-side effect of a destroyed
    /// weapon-mount damage node.
    ///
    /// Idempotent: the damage resolver may report the same destruction more
    /// than once, and disabling twice is not a second event.
    pub fn disable(&mut self, mount: &DamageNodeKey) {
        self.disabled_mounts.insert(mount.clone());
    }

    /// Re-enables the named mount, as an aircraft repair does.
    pub fn enable(&mut self, mount: &DamageNodeKey) {
        self.disabled_mounts.remove(mount);
    }

    /// Advances every mount's cooldown by one simulation tick.
    pub fn tick_cooldowns(&mut self) {
        for remaining in self.cooldown_ticks.values_mut() {
            *remaining = remaining.saturating_sub(1);
        }
    }

    /// Consumes one round from the named mount and starts its cooldown.
    ///
    /// This is the only place a round is ever consumed, and it is reached
    /// only from an accepted shot.
    ///
    /// # Errors
    ///
    /// [`GunStateError::UnknownMount`] when the actor does not carry the
    /// mount, [`GunStateError::Empty`] when it has no rounds left.
    pub fn consume_round(
        &mut self,
        mount: &DamageNodeKey,
        rate: GunRate,
    ) -> Result<(), GunStateError> {
        let rounds = self
            .ammunition
            .get_mut(mount)
            .ok_or_else(|| GunStateError::UnknownMount {
                mount: mount.clone(),
            })?;
        if *rounds == 0 {
            return Err(GunStateError::Empty {
                mount: mount.clone(),
            });
        }
        *rounds -= 1;
        self.cooldown_ticks
            .insert(mount.clone(), u64::from(rate.ticks_between_shots()));
        Ok(())
    }
}

/// Why a [`WeaponState`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GunStateError {
    /// Two mounted guns share one mount key.
    DuplicateMount {
        /// The repeated mount.
        mount: DamageNodeKey,
    },
    /// The selected bank names a mount the actor does not carry.
    UnknownSelection {
        /// The mount the bank named.
        mount: DamageNodeKey,
    },
    /// Starting ammunition was zero, so no gun could ever fire.
    InvalidStartingRounds,
    /// The actor does not carry the named mount.
    UnknownMount {
        /// The unnamed mount.
        mount: DamageNodeKey,
    },
    /// The named mount has no rounds left.
    Empty {
        /// The empty mount.
        mount: DamageNodeKey,
    },
}

impl fmt::Display for GunStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateMount { mount } => {
                write!(f, "mount {mount} carries more than one gun")
            }
            Self::UnknownSelection { mount } => {
                write!(
                    f,
                    "the selected bank names mount {mount}, which is not carried"
                )
            }
            Self::InvalidStartingRounds => {
                write!(f, "a weapon must start with at least one round")
            }
            Self::UnknownMount { mount } => write!(f, "mount {mount} is not carried"),
            Self::Empty { mount } => write!(f, "mount {mount} has no rounds left"),
        }
    }
}

impl std::error::Error for GunStateError {}

// ------------------------------------------------------------------ fire ----

/// The identity of one [`FireIntent`]: `EventId(session, tick, producer,
/// sequence)`.
///
/// `producer` is the serial of the firing system (a player's craft, a
/// scripted wingman, a replayed network packet) and `sequence` orders that
/// producer's intents. The whole id is the *once* key: a duplicate packet
/// carrying an already-resolved id is refused whole and changes no state
/// (non-negotiable 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FireIntentId {
    /// The session generation the intent belongs to.
    pub session: u64,
    /// The simulation tick the intent belongs to.
    pub tick: Tick,
    /// The firing system's serial.
    pub producer: u32,
    /// The intent's sequence within its producer.
    pub sequence: u32,
}

impl fmt::Display for FireIntentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "fire intent {}:{}#{}.{}",
            self.session, self.tick.0, self.producer, self.sequence
        )
    }
}

/// The identity of one accepted [`FireEvent`]: the same `EventId` shape,
/// stamped by the resolver.
///
/// Unlike [`FireIntentId`], whose `producer` is a wire-level system serial,
/// the event's `producer` is the firing actor's own [`ActorId::serial`] and
/// therefore keeps its full width: `ActorId` serials are never recycled
/// inside a session (`docs/01-ARCHITECTURE.md`), so narrowing it to a `u32`
/// could give two actors the same producer serial and make two distinct shots
/// share one event id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FireEventId {
    /// The session generation the event was produced in.
    pub session: u64,
    /// The simulation tick the shot belongs to.
    pub tick: Tick,
    /// The firing actor's serial within the session.
    pub producer: u64,
    /// The event's sequence within the shooting actor's own arsenal: how many
    /// shots that actor had already accepted, at full width so a long session
    /// cannot wrap it onto an id already used.
    pub sequence: u64,
}

/// The identity of one projectile: a session-qualified, never-recycled
/// serial.
///
/// The [`Ballistics`] ledger is keyed by this id, which is how "one
/// projectile applies a hit at most once" is enforced across ticks and
/// across several collision features reporting the same contact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectileId {
    /// The session generation the projectile belongs to.
    pub session: u64,
    /// The projectile's serial within that session.
    pub serial: u64,
}

impl fmt::Display for ProjectileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "projectile {}:{}", self.session, self.serial)
    }
}

/// A mount's pose for one tick: where the muzzle is, where it points and
/// what velocity the airframe already has.
///
/// This is the whole of F27 non-negotiable 2's "mount transforms come from
/// the live aircraft hierarchy/damage state, not a fixed center-screen
/// origin": a spawn's pose is *supplied*, never defaulted, and a mount with
/// no transform cannot fire at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MountTransform {
    /// The muzzle's canonical world position.
    pub origin: WorldPosition,
    /// The mount's canonical forward direction.
    pub forward: UnitVec3,
    /// The world velocity the firing airframe already has. F27
    /// non-negotiable 2 makes inherited velocity an *explicit verified
    /// rule*: it is a supplied vector, and this stage defines no rule for
    /// how much of it a round inherits.
    pub inherited_velocity_mps: [f64; 3],
}

impl MountTransform {
    /// Assembles a transform, refusing a non-finite inherited velocity.
    ///
    /// # Errors
    ///
    /// [`MountTransformError::NonFiniteInheritedVelocity`] when a component
    /// of `inherited_velocity_mps` is NaN or infinite.
    pub fn try_new(
        origin: WorldPosition,
        forward: UnitVec3,
        inherited_velocity_mps: [f64; 3],
    ) -> Result<Self, MountTransformError> {
        if inherited_velocity_mps
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(MountTransformError::NonFiniteInheritedVelocity);
        }
        Ok(Self {
            origin,
            forward,
            inherited_velocity_mps,
        })
    }

    /// The round's world velocity at the muzzle: the declared share of the
    /// airframe's world velocity plus the muzzle velocity along the mount's
    /// forward axis.
    ///
    /// Both terms are explicit, and the share is the declared
    /// [`InheritanceRule`] rather than an assumption. No drag, no
    /// convergence and no gravity are applied here — those are F27-B's
    /// ballistics and F27-D's measurements.
    #[must_use]
    pub fn world_velocity_mps(
        &self,
        inheritance: InheritanceRule,
        muzzle_velocity_mps: f64,
    ) -> [f64; 3] {
        let share = match inheritance {
            InheritanceRule::Full => 1.0,
            InheritanceRule::Fraction { share } => share,
            InheritanceRule::None => 0.0,
        };
        let forward = self.forward.to_array();
        [
            self.inherited_velocity_mps[0] * share + forward[0] * muzzle_velocity_mps,
            self.inherited_velocity_mps[1] * share + forward[1] * muzzle_velocity_mps,
            self.inherited_velocity_mps[2] * share + forward[2] * muzzle_velocity_mps,
        ]
    }
}

/// Why a [`MountTransform`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MountTransformError {
    /// The inherited airframe velocity had a non-finite component.
    NonFiniteInheritedVelocity,
}

impl fmt::Display for MountTransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteInheritedVelocity => {
                write!(f, "the inherited airframe velocity must be finite")
            }
        }
    }
}

impl std::error::Error for MountTransformError {}

/// A request to fire: the currently selected bank of one actor, on one
/// tick.
///
/// An intent carries no damage, no spawn and no consequence — it is the
/// *request*. Only the authoritative [`FireResolver`] turns it into a
/// [`FireEvent`], and only an accepted event consumes anything.
#[derive(Clone, Debug, PartialEq)]
pub struct FireIntent {
    /// The intent's once-only identity.
    pub id: FireIntentId,
    /// The actor whose guns are being asked to fire.
    pub shooter: ActorId,
}

/// The spawned projectile of one accepted shot.
///
/// The pose and velocity come from the supplied mount transform; the
/// lifetime and spread come from the gun definition. What the round *does*
/// on arrival — the damage, the effects — is F27-C's wiring into
/// [`crate::damage`].
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectileSpawn {
    /// The projectile's stable identity.
    pub projectile: ProjectileId,
    /// The world position the round leaves the muzzle at.
    pub origin: WorldPosition,
    /// The world velocity the round starts with.
    pub velocity_mps: [f64; 3],
    /// The declared cone the round is scattered in.
    pub spread: SpreadCone,
    /// How many further ticks the round lives.
    pub lifetime_ticks: u64,
}

/// One accepted shot.
///
/// An event exists **only** for a shot that happened. Its presence is the
/// authority for consuming a round, starting a cooldown, spawning a
/// projectile and naming a sound and muzzle effect (non-negotiable 5:
/// "Consumption, sound and muzzle effects derive from accepted fire events;
/// no ammo drain from a denied input or duplicate network packet").
#[derive(Clone, Debug, PartialEq)]
pub struct FireEvent {
    /// The event's stable identity and ordering key. Its `producer` is the
    /// firing actor's own [`ActorId::serial`] at full width, so two actors
    /// that both fire on one tick never share an id.
    pub id: FireEventId,
    /// The intent this shot answers.
    pub intent: FireIntentId,
    /// The actor that fired.
    pub shooter: ActorId,
    /// The mount that fired.
    pub mount: DamageNodeKey,
    /// Where on the airframe the mount sits.
    pub mount_kind: GunMountKind,
    /// The gun that fired, by declared caliber text.
    pub caliber: String,
    /// The ammunition type that was consumed.
    pub ammunition: AmmunitionId,
    /// The spawned projectile.
    pub projectile: ProjectileSpawn,
    /// The damage channels this round delivers.
    pub damage: WeaponDamage,
    /// The effect resource an accepted shot plays.
    pub effect: ContentId,
    /// The sound resource an accepted shot plays.
    pub sound: ContentId,
}

/// Why one mount of a bank did not fire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FireDenialReason {
    /// The actor does not carry the mount the bank names.
    UnmountedBankMember {
        /// The mount the bank named.
        mount: DamageNodeKey,
    },
    /// The mount is disabled: its weapon-mount damage node was destroyed.
    MountDisabled {
        /// The disabled mount.
        mount: DamageNodeKey,
    },
    /// The mount is still cooling down from its last accepted shot.
    Cooldown {
        /// The cooling mount.
        mount: DamageNodeKey,
        /// Ticks it must still wait.
        remaining_ticks: u64,
    },
    /// The mount has no rounds left.
    OutOfAmmunition {
        /// The empty mount.
        mount: DamageNodeKey,
    },
    /// The resolver has no transform for the mount, so it has no pose to
    /// fire from. It refuses rather than firing from a default origin.
    MissingMountTransform {
        /// The mount with no transform.
        mount: DamageNodeKey,
    },
}

impl FireDenialReason {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::UnmountedBankMember { .. } => "unmounted_bank_member",
            Self::MountDisabled { .. } => "mount_disabled",
            Self::Cooldown { .. } => "cooldown",
            Self::OutOfAmmunition { .. } => "out_of_ammunition",
            Self::MissingMountTransform { .. } => "missing_mount_transform",
        }
    }

    /// The mount this denial is about.
    #[must_use]
    pub const fn mount(&self) -> &DamageNodeKey {
        match self {
            Self::UnmountedBankMember { mount }
            | Self::MountDisabled { mount }
            | Self::Cooldown { mount, .. }
            | Self::OutOfAmmunition { mount }
            | Self::MissingMountTransform { mount } => mount,
        }
    }
}

impl fmt::Display for FireDenialReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnmountedBankMember { mount } => {
                write!(
                    f,
                    "the bank names mount {mount}, which this actor does not carry"
                )
            }
            Self::MountDisabled { mount } => write!(f, "mount {mount} is disabled"),
            Self::Cooldown {
                mount,
                remaining_ticks,
            } => write!(
                f,
                "mount {mount} is cooling down for {remaining_ticks} more tick(s)"
            ),
            Self::OutOfAmmunition { mount } => {
                write!(f, "mount {mount} has no rounds left")
            }
            Self::MissingMountTransform { mount } => {
                write!(f, "mount {mount} has no transform to fire from")
            }
        }
    }
}

/// Why a whole fire intent was refused, changing no state at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntentRefusal {
    /// The intent names a session generation other than the resolver's.
    ForeignSession {
        /// The resolver's session.
        expected: u64,
        /// The session the intent carried.
        found: u64,
    },
    /// The intent names a tick other than the one being resolved.
    ForeignTick {
        /// The tick being resolved.
        expected: Tick,
        /// The tick the intent carried.
        found: Tick,
    },
    /// The intent's id was already resolved. A duplicate network packet must
    /// not fire a second time (non-negotiable 5).
    DuplicateIntent {
        /// The repeated intent id.
        id: FireIntentId,
    },
    /// The intent names an actor this resolver has no weapons for.
    UnknownShooter {
        /// The actor that was named.
        shooter: ActorId,
    },
    /// The actor has an empty selection: nothing is selected, so there is
    /// nothing to fire.
    NoSelectedBank {
        /// The actor with the empty selection.
        shooter: ActorId,
    },
}

impl IntentRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ForeignSession { .. } => "foreign_session",
            Self::ForeignTick { .. } => "foreign_tick",
            Self::DuplicateIntent { .. } => "duplicate_intent",
            Self::UnknownShooter { .. } => "unknown_shooter",
            Self::NoSelectedBank { .. } => "no_selected_bank",
        }
    }
}

impl fmt::Display for IntentRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => {
                write!(
                    f,
                    "fire intent is for session {found}, but this resolver is session {expected}"
                )
            }
            Self::ForeignTick { expected, found } => {
                write!(
                    f,
                    "fire intent is for tick {found:?}, but tick {expected:?} is resolving"
                )
            }
            Self::DuplicateIntent { id } => {
                write!(f, "fire intent {id} was already resolved")
            }
            Self::UnknownShooter { shooter } => {
                write!(f, "{shooter} has no weapons registered here")
            }
            Self::NoSelectedBank { shooter } => {
                write!(f, "{shooter} has no gun bank selected")
            }
        }
    }
}

impl std::error::Error for IntentRefusal {}

/// One resolved intent: the accepted shots and the refused mounts.
///
/// The two lists partition the selected bank's mounts. Exactly the accepted
/// list consumed anything, so a bank whose every member is disabled resolves
/// to an empty `accepted` and a full `refused` — no projectile, no sound, no
/// ammunition decrement (AC02).
#[derive(Clone, Debug, PartialEq)]
pub struct FireResolution {
    /// The intent that was resolved.
    pub intent: FireIntentId,
    /// The shots that happened, in the bank's mount order.
    pub accepted: Vec<FireEvent>,
    /// The mounts that did not fire, with the reason.
    pub refused: Vec<FireDenialReason>,
}

impl FireResolution {
    /// Whether nothing fired.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.accepted.is_empty()
    }
}

/// Why a [`FireResolver`] operation was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FireError {
    /// The actor was already registered.
    DuplicateShooter {
        /// The actor that exists already.
        shooter: ActorId,
    },
    /// Two mounted guns share one mount key.
    DuplicateMount {
        /// The actor being registered.
        shooter: ActorId,
        /// The repeated mount.
        mount: DamageNodeKey,
    },
    /// The actor is not registered with this resolver.
    UnknownShooter {
        /// The actor that was named.
        shooter: ActorId,
    },
    /// A [`WeaponState`] operation was refused.
    State(GunStateError),
}

impl fmt::Display for FireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateShooter { shooter } => {
                write!(f, "{shooter} is already registered with this resolver")
            }
            Self::DuplicateMount { shooter, mount } => {
                write!(f, "{shooter} already carries a gun on mount {mount}")
            }
            Self::UnknownShooter { shooter } => {
                write!(f, "{shooter} has no weapons registered here")
            }
            Self::State(source) => {
                write!(f, "weapon state refused the operation: {source}")
            }
        }
    }
}

impl std::error::Error for FireError {}

/// One actor's registered guns and live weapon state.
#[derive(Clone, Debug)]
struct ActorArsenal {
    definitions: BTreeMap<DamageNodeKey, GunDefinition>,
    state: WeaponState,
    /// How many shots this actor has already accepted. It is the event
    /// sequence's low half — the firing actor's own serial is the high half —
    /// so it is `u64` for the same reason: a long session must not wrap the
    /// counter and reissue an id it already used.
    next_event_sequence: u64,
}

/// One session generation's authority over fire intents and weapon state.
///
/// The resolver is the *only* place a round is consumed, a cooldown is
/// started, a projectile is spawned and a sound is named
/// (non-negotiable 5). Everything a consumer needs to apply those effects
/// is on the [`FireEvent`]s it emits, and a refused intent emits none.
///
/// It is deliberately **not** a tick loop: it resolves one tick's intents on
/// demand, and the schedule that calls it every tick — and that supplies the
/// mount transforms from the live aircraft hierarchy — is F27-B's runtime.
#[derive(Clone, Debug)]
pub struct FireResolver {
    session: u64,
    tick: Tick,
    actors: BTreeMap<ActorId, ActorArsenal>,
    next_projectile_serial: u64,
    resolved_intents: BTreeSet<FireIntentId>,
}

impl FireResolver {
    /// Opens a resolver for one session generation, positioned at `tick`.
    #[must_use]
    pub fn new(session: u64, tick: Tick) -> Self {
        Self {
            session,
            tick,
            actors: BTreeMap::new(),
            next_projectile_serial: 0,
            resolved_intents: BTreeSet::new(),
        }
    }

    /// The session generation this resolver is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The tick this resolver is currently resolving.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// Advances the resolver to `tick`, ticking every cooldown down once per
    /// elapsed tick.
    ///
    /// Time never runs backwards: a `tick` at or before the current one is
    /// ignored, so a re-entered schedule step cannot double-decrement or
    /// rewind a cooldown.
    ///
    /// Every elapsed tick is visited, because a cooldown that is still
    /// counting must decrement once per tick: a caller that skips ticks must
    /// reach the same state as one that walks them. A caller seeking across a
    /// very long gap should clamp the seek rather than rely on this being
    /// cheap — F27-B owns the cadence schedule that drives it.
    pub fn advance_to(&mut self, tick: Tick) {
        if tick <= self.tick {
            return;
        }
        for _ in self.tick.0..tick.0 {
            for arsenal in self.actors.values_mut() {
                arsenal.state.tick_cooldowns();
            }
        }
        self.tick = tick;
    }

    /// Registers one actor's mounted guns and its weapon state.
    ///
    /// # Errors
    ///
    /// [`FireError::DuplicateShooter`] when the actor is registered already
    /// and [`FireError::DuplicateMount`] when two of the definitions share a
    /// mount key.
    pub fn register(
        &mut self,
        shooter: ActorId,
        definitions: Vec<GunDefinition>,
        state: WeaponState,
    ) -> Result<(), FireError> {
        if self.actors.contains_key(&shooter) {
            return Err(FireError::DuplicateShooter { shooter });
        }
        let mut by_mount: BTreeMap<DamageNodeKey, GunDefinition> = BTreeMap::new();
        for definition in definitions {
            let mount = definition.mount().clone();
            if by_mount.insert(mount.clone(), definition).is_some() {
                return Err(FireError::DuplicateMount { shooter, mount });
            }
        }
        self.actors.insert(
            shooter,
            ActorArsenal {
                definitions: by_mount,
                state,
                next_event_sequence: 0,
            },
        );
        Ok(())
    }

    /// One actor's weapon state, if registered.
    #[must_use]
    pub fn state(&self, shooter: &ActorId) -> Option<&WeaponState> {
        self.actors.get(shooter).map(|arsenal| &arsenal.state)
    }

    /// One actor's mutable weapon state, if registered.
    ///
    /// This is how the damage system's
    /// [`crate::damage::DamageEventKind::SystemDisabled`] reaches the weapon
    /// gate: its caller disables the named mount here.
    pub fn state_mut(&mut self, shooter: &ActorId) -> Option<&mut WeaponState> {
        self.actors
            .get_mut(shooter)
            .map(|arsenal| &mut arsenal.state)
    }

    /// One actor's mounted gun on `mount`, if any.
    #[must_use]
    pub fn definition(&self, shooter: &ActorId, mount: &DamageNodeKey) -> Option<&GunDefinition> {
        self.actors
            .get(shooter)
            .and_then(|arsenal| arsenal.definitions.get(mount))
    }

    /// Allocates the next [`ProjectileId`] of this session.
    ///
    /// Serials are never recycled inside a session, so a projectile id
    /// always names exactly one projectile — which is what makes the
    /// [`Ballistics`] ledger's once-per-projectile guarantee sound.
    pub fn next_projectile_id(&mut self) -> ProjectileId {
        let id = ProjectileId {
            session: self.session,
            serial: self.next_projectile_serial,
        };
        self.next_projectile_serial += 1;
        id
    }

    /// Resolves one intent for the resolver's current tick.
    ///
    /// Returns [`Err`] with an [`IntentRefusal`] when the intent is refused
    /// **whole** — a foreign session or tick, an already-resolved id, an
    /// unknown shooter or an empty selection. In every one of those cases no
    /// state changed at all: nothing fired, nothing was consumed, no
    /// cooldown started and no sound was named.
    ///
    /// Otherwise returns [`Ok`] with a [`FireResolution`] whose `accepted`
    /// and `refused` lists partition the selected bank's mounts.
    pub fn resolve(
        &mut self,
        intent: &FireIntent,
        transforms: &BTreeMap<DamageNodeKey, MountTransform>,
    ) -> Result<FireResolution, IntentRefusal> {
        if intent.id.session != self.session {
            return Err(IntentRefusal::ForeignSession {
                expected: self.session,
                found: intent.id.session,
            });
        }
        if intent.id.tick != self.tick {
            return Err(IntentRefusal::ForeignTick {
                expected: self.tick,
                found: intent.id.tick,
            });
        }
        if self.resolved_intents.contains(&intent.id) {
            return Err(IntentRefusal::DuplicateIntent { id: intent.id });
        }
        if !self.actors.contains_key(&intent.shooter) {
            return Err(IntentRefusal::UnknownShooter {
                shooter: intent.shooter,
            });
        }
        if self
            .actors
            .get(&intent.shooter)
            .is_some_and(|arsenal| arsenal.state.selected().is_empty())
        {
            return Err(IntentRefusal::NoSelectedBank {
                shooter: intent.shooter,
            });
        }

        let arsenal = self
            .actors
            .get_mut(&intent.shooter)
            .expect("the shooter was checked above");
        let mut context = ShotContext {
            session: self.session,
            tick: self.tick,
            next_projectile_serial: &mut self.next_projectile_serial,
            intent: intent.id,
            shooter: intent.shooter,
            transforms,
        };
        let mut accepted = Vec::new();
        let mut refused = Vec::new();
        for mount in arsenal
            .state
            .selected()
            .mounts()
            .iter()
            .cloned()
            .collect::<Vec<_>>()
        {
            match fire_one_mount(&mut context, &mount, arsenal) {
                Ok(event) => accepted.push(event),
                Err(reason) => refused.push(reason),
            }
        }
        self.resolved_intents.insert(intent.id);
        Ok(FireResolution {
            intent: intent.id,
            accepted,
            refused,
        })
    }
}

/// Resolves one mount of a bank into an accepted shot or a named refusal.
///
/// A free function rather than a method so the resolver's own borrows stay
/// disjoint: it needs the arsenal mutably, the projectile serial cursor
/// mutably, and the caller's transforms immutably.
struct ShotContext<'a> {
    session: u64,
    tick: Tick,
    next_projectile_serial: &'a mut u64,
    intent: FireIntentId,
    shooter: ActorId,
    transforms: &'a BTreeMap<DamageNodeKey, MountTransform>,
}

fn fire_one_mount(
    context: &mut ShotContext<'_>,
    mount: &DamageNodeKey,
    arsenal: &mut ActorArsenal,
) -> Result<FireEvent, FireDenialReason> {
    let session = context.session;
    let tick = context.tick;
    let shooter = context.shooter;
    let intent = context.intent;
    let next_projectile_serial = &mut *context.next_projectile_serial;
    let transforms = context.transforms;
    let Some(definition) = arsenal.definitions.get(mount) else {
        return Err(FireDenialReason::UnmountedBankMember {
            mount: mount.clone(),
        });
    };
    if arsenal.state.is_disabled(mount) {
        return Err(FireDenialReason::MountDisabled {
            mount: mount.clone(),
        });
    }
    let remaining_ticks = arsenal.state.cooldown_ticks(mount);
    if remaining_ticks > 0 {
        return Err(FireDenialReason::Cooldown {
            mount: mount.clone(),
            remaining_ticks,
        });
    }
    if arsenal.state.ammunition(mount) == 0 {
        return Err(FireDenialReason::OutOfAmmunition {
            mount: mount.clone(),
        });
    }
    // The mount's pose is supplied, never defaulted: without a transform
    // there is no muzzle position and nothing fires.
    let Some(transform) = transforms.get(mount) else {
        return Err(FireDenialReason::MissingMountTransform {
            mount: mount.clone(),
        });
    };

    // The shot is accepted. From here a round is consumed and the cooldown
    // starts — the only place either happens.
    let rate = definition.rate();
    arsenal
        .state
        .consume_round(mount, rate)
        .map_err(|source| match source {
            GunStateError::Empty { .. } => FireDenialReason::OutOfAmmunition {
                mount: mount.clone(),
            },
            // The emptiness and the membership were both checked above, so
            // this arm is unreachable today; it is mapped rather than
            // unwrapped so a future state change can never take the
            // simulation down mid-fire.
            _ => FireDenialReason::UnmountedBankMember {
                mount: mount.clone(),
            },
        })?;

    let projectile = ProjectileId {
        session,
        serial: *next_projectile_serial,
    };
    *next_projectile_serial += 1;
    let event = FireEvent {
        id: FireEventId {
            session,
            tick,
            producer: shooter.serial,
            sequence: arsenal.next_event_sequence,
        },
        intent,
        shooter,
        mount: mount.clone(),
        mount_kind: definition.kind(),
        caliber: definition.caliber().to_owned(),
        ammunition: definition.ammunition().clone(),
        projectile: ProjectileSpawn {
            projectile,
            origin: transform.origin,
            velocity_mps: transform
                .world_velocity_mps(definition.inheritance(), definition.muzzle_velocity_mps()),
            spread: definition.spread(),
            lifetime_ticks: definition.lifetime_ticks(),
        },
        damage: *definition.damage(),
        effect: definition.effect().clone(),
        sound: definition.sound().clone(),
    };
    arsenal.next_event_sequence += 1;
    Ok(event)
}

// -------------------------------------------------------------- ballistics ----

/// One projectile's motion across a single tick: where it was and where it
/// is now.
///
/// The swept segment, not the two endpoints, is the geometry of record
/// (`FLIGHT-PHYSICS`: "For high-speed tests choose speed*dt larger than the
/// obstacle thickness so a discrete endpoint-only implementation provably
/// fails").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileSegment {
    /// The projectile this segment belongs to.
    pub projectile: ProjectileId,
    /// The world position at the start of the tick.
    pub previous: WorldPosition,
    /// The world position at the end of the tick.
    pub current: WorldPosition,
}

/// One target's swept box over a single tick.
///
/// A box, not a point: an aircraft is not a mathematical point, and the
/// original's collision shapes are unmeasured (F27-D). The box is swept
/// between `previous` and `current`, so a target that crosses the
/// projectile's path *between* ticks is still tested (`FLIGHT-PHYSICS`:
/// "Check relative movement: a target can cross the projectile path between
/// ticks").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweepTarget {
    /// The actor the box belongs to.
    pub actor: ActorId,
    /// The box centre at the start of the tick.
    pub previous: WorldPosition,
    /// The box centre at the end of the tick.
    pub current: WorldPosition,
    /// The axis-aligned half extents of the box, in meters.
    pub half_extents_m: [f64; 3],
}

impl SweepTarget {
    /// Assembles a swept target, refusing non-finite geometry and negative
    /// extents.
    ///
    /// # Errors
    ///
    /// [`SweepTargetError`] on a non-finite position or half extent, or on a
    /// negative half extent. A negative extent would silently mirror the box
    /// and make the slab test answer nonsense.
    pub fn try_new(
        actor: ActorId,
        previous: [f64; 3],
        current: [f64; 3],
        half_extents_m: [f64; 3],
    ) -> Result<Self, SweepTargetError> {
        let previous = WorldPosition::try_new(previous).map_err(SweepTargetError::Position)?;
        let current = WorldPosition::try_new(current).map_err(SweepTargetError::Position)?;
        for (axis, extent) in half_extents_m.iter().enumerate() {
            if !extent.is_finite() {
                return Err(SweepTargetError::NonFiniteHalfExtent { axis });
            }
            if *extent < 0.0 {
                return Err(SweepTargetError::NegativeHalfExtent {
                    axis,
                    value: *extent,
                });
            }
        }
        Ok(Self {
            actor,
            previous,
            current,
            half_extents_m,
        })
    }
}

/// Why a [`SweepTarget`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum SweepTargetError {
    /// A centre position had a non-finite component.
    Position(cs_types::space::SpaceError),
    /// A half extent was NaN or infinite.
    NonFiniteHalfExtent {
        /// The offending axis index: `0` X, `1` Y, `2` Z.
        axis: usize,
    },
    /// A half extent was negative.
    NegativeHalfExtent {
        /// The offending axis index: `0` X, `1` Y, `2` Z.
        axis: usize,
        /// The rejected value.
        value: f64,
    },
}

impl fmt::Display for SweepTargetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Position(source) => {
                write!(f, "a swept target position is not finite: {source}")
            }
            Self::NonFiniteHalfExtent { axis } => {
                let axis = match axis {
                    0 => "x",
                    1 => "y",
                    _ => "z",
                };
                write!(f, "the swept target's {axis} half extent must be finite")
            }
            Self::NegativeHalfExtent { axis, value } => {
                let axis = match axis {
                    0 => "x",
                    1 => "y",
                    _ => "z",
                };
                write!(
                    f,
                    "the swept target's {axis} half extent is negative: {value}"
                )
            }
        }
    }
}

impl std::error::Error for SweepTargetError {}

/// One candidate a sweep is handed: a swept box, the damage node a contact
/// with it lands on, and the declared relation the rules filter it under.
///
/// The **node** is what the F27-C routing needs and a [`SweepTarget`] cannot
/// supply: a swept hit names an *actor* and a time of impact, while a
/// [`crate::damage::HitEvent`] names the damage-graph node the round damaged,
/// and which of an aircraft's parts a round reached is not derivable from one
/// actor-level box — the original's part collision shapes are unmeasured
/// (F27-D) and no part is chosen here by proximity or by a default node. So
/// the collision feature that owns part geometry reports the node *with* the
/// box it reports, and the contact that wins the sweep routes the node of
/// the candidate it came from.
///
/// A candidate is a `(box, node)` pair, so a target whose parts are reported
/// separately contributes one candidate per part; the sweep's
/// once-per-`(projectile, actor)` ledger is what keeps that from applying one
/// round several times to one aircraft.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepCandidate {
    /// The swept box over the tick.
    pub target: SweepTarget,
    /// The damage-graph node a contact with this box damages.
    pub node: DamageNodeKey,
    /// The declared relation of the firing actor to `target.actor`, in the
    /// F30-A [`Allegiance`] vocabulary. `None` is an *undeclared* pair, which
    /// is not the same statement as "friendly": only
    /// [`FriendlyFireRule::Everyone`] admits it.
    pub relation: Option<Allegiance>,
}

impl SweepCandidate {
    /// Assembles a candidate.
    ///
    /// Infallible by construction: the box was validated by
    /// [`SweepTarget::try_new`], and the node and the relation are typed
    /// values that carry their own validity.
    #[must_use]
    pub const fn new(
        target: SweepTarget,
        node: DamageNodeKey,
        relation: Option<Allegiance>,
    ) -> Self {
        Self {
            target,
            node,
            relation,
        }
    }

    /// The candidate's damage node.
    #[must_use]
    pub const fn node(&self) -> &DamageNodeKey {
        &self.node
    }

    /// The candidate's swept box.
    #[must_use]
    pub const fn target(&self) -> &SweepTarget {
        &self.target
    }

    /// The candidate's declared relation.
    #[must_use]
    pub const fn relation(&self) -> Option<Allegiance> {
        self.relation
    }
}

impl fmt::Display for SweepCandidate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} on {}", self.target.actor, self.node)
    }
}

/// One hit a swept segment produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweptHit {
    /// The projectile that hit.
    pub projectile: ProjectileId,
    /// The actor that was hit.
    pub target: ActorId,
    /// The hit's normalized time of impact along the tick, in `[0, 1]`:
    /// `0.0` is the start of the tick, `1.0` its end. A hit at `0.4`
    /// happened four tenths of the way through the tick, *before* the
    /// projectile's end position.
    pub time_of_impact: f64,
}

/// One swept hit together with the supplied candidate it came from.
///
/// [`Ballistics::sweep_with_sources`] returns these; a caller that only needs
/// to know *that* an actor was hit takes [`SweptHit`]s from
/// [`Ballistics::sweep`] instead, which is the same test with the index
/// dropped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SweptContact {
    /// The hit itself: which projectile, which actor, when.
    pub hit: SweptHit,
    /// The index into the target slice the sweep was given. The candidate it
    /// names is the one whose box the round reached first — which, for a
    /// target whose parts were reported as separate candidates, is the part
    /// the hit landed on.
    pub candidate: usize,
}

impl SweptContact {
    /// The actor this contact hit.
    #[must_use]
    pub const fn target(&self) -> ActorId {
        self.hit.target
    }

    /// The projectile that made this contact.
    #[must_use]
    pub const fn projectile(&self) -> ProjectileId {
        self.hit.projectile
    }

    /// The index of the candidate this contact came from.
    #[must_use]
    pub const fn candidate(&self) -> usize {
        self.candidate
    }
}

/// The per-session swept-ballistics query.
///
/// [`Ballistics::sweep`] is the authoritative swept test: it takes a
/// projectile's tick segment and the eligible targets, computes relative
/// motion, runs a slab test and returns the hits **in ascending time of
/// impact**, with the actor id as the stable tie-breaker — the ordering
/// `FLIGHT-PHYSICS` requires ("A closest-hit policy must choose earliest
/// time-of-impact and stable tie-breakers").
///
/// The ledger keyed by `(ProjectileId, ActorId)` is what enforces "one
/// projectile applies a hit at most once" (non-negotiable 3): several
/// collision features reporting the same contact — whether they arrive as
/// repeated candidates in one sweep or as a second sweep of the same segment
/// — still yield one hit.
#[derive(Clone, Debug, Default)]
pub struct Ballistics {
    applied: BTreeSet<(ProjectileId, ActorId)>,
}

impl Ballistics {
    /// An empty ledger: no projectile has applied a hit yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The swept test for one projectile segment against the eligible
    /// targets.
    ///
    /// Targets arrive already filtered by the caller: this function is
    /// geometry. Eligibility — allegiance, self-hit exclusion, layer rules —
    /// belongs to the query that assembles the candidate list,
    /// [`WeaponRules::admit_candidates`], which is F27-C's wiring. What the
    /// sweep owns
    /// is the segment-vs-box test, the deterministic ordering and the
    /// once-per-`(projectile, actor)` guarantee.
    ///
    /// A candidate list may name the same actor more than once: several
    /// collision features reporting one contact is a documented case
    /// (`FLIGHT-PHYSICS`, "Apply damage once even if several collision
    /// features report the same hit"). Duplicates are collapsed to the
    /// earliest time of impact rather than each producing a hit.
    ///
    /// This is [`Ballistics::sweep_with_sources`] with the source index
    /// dropped: the geometry, the ordering and the ledger are one test, so a
    /// caller that needs the winning candidate asks for it here rather than
    /// re-deriving it.
    pub fn sweep(&mut self, segment: &ProjectileSegment, targets: &[SweepTarget]) -> Vec<SweptHit> {
        self.sweep_with_sources(segment, targets)
            .into_iter()
            .map(|contact| contact.hit)
            .collect()
    }

    /// The swept test that also reports which supplied candidate each hit
    /// came from.
    ///
    /// Same test as [`Ballistics::sweep`] — same relative motion, same slab
    /// test, same ascending time of impact with the actor id as the
    /// tie-breaker, same once-per-`(projectile, actor)` ledger — returning
    /// [`SweptContact`]s that additionally name the candidate the contact
    /// came from.
    ///
    /// The source index is what makes the F27-C routing possible without a
    /// second geometry test: several part boxes of one actor are separate
    /// candidates, and the contact that wins the sweep is the one whose box
    /// the round reached first, so its index names the damage node the hit
    /// routes to. Duplicate candidates for one actor still collapse to the
    /// earliest contact *and its source* — several collision features
    /// reporting one contact yield one hit naming the part that was reached
    /// first.
    pub fn sweep_with_sources(
        &mut self,
        segment: &ProjectileSegment,
        targets: &[SweepTarget],
    ) -> Vec<SweptContact> {
        let mut candidates: Vec<(f64, ActorId, usize)> = targets
            .iter()
            .enumerate()
            .filter(|(_, target)| !self.has_hit(segment.projectile, target.actor))
            .filter_map(|(index, target)| {
                earliest_time_of_impact(segment, target).map(|t| (t, target.actor, index))
            })
            .collect();
        // Ascending time of impact, actor id as the stable tie-breaker, and
        // the supplied index as the last one: a total order that does not
        // depend on the caller's target order. The index only decides between
        // two candidates of the *same* actor at the same time, where either
        // answer applies one hit — it makes the choice explicit instead of
        // leaving it to the sort's stability.
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let mut contacts = Vec::with_capacity(candidates.len());
        // One entry in `reported` per actor this call has already emitted, so
        // a candidate list that names the same actor twice — several collision
        // features reporting one contact in the *same* sweep, which the
        // ledger filter above cannot see because nothing has been applied yet
        // — still yields one hit. Ordering decides which: the first, i.e. the
        // earliest time of impact.
        let mut reported: BTreeSet<ActorId> = BTreeSet::new();
        for (time_of_impact, actor, candidate) in candidates {
            if !reported.insert(actor) {
                continue;
            }
            self.applied.insert((segment.projectile, actor));
            contacts.push(SweptContact {
                hit: SweptHit {
                    projectile: segment.projectile,
                    target: actor,
                    time_of_impact,
                },
                candidate,
            });
        }
        contacts
    }

    /// Whether this projectile has already applied a hit on this actor.
    #[must_use]
    pub fn has_hit(&self, projectile: ProjectileId, target: ActorId) -> bool {
        self.applied.contains(&(projectile, target))
    }

    /// How many `(projectile, actor)` hits this ledger has recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.applied.len()
    }

    /// Whether the ledger records no hit at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.applied.is_empty()
    }
}

/// The earliest time in `[0, 1]` at which a projectile's swept segment
/// enters a target's swept box, or `None` if it never does.
///
/// The two moves are put in one frame first: the box's own motion over the
/// tick is subtracted from the projectile's, so the test is a static segment
/// against a static box and a target that crosses the path between ticks is
/// caught. The tick is normalized to `[0, 1]`, so the result is a fraction
/// of the tick rather than a time — the caller owns the tick length.
///
/// A degenerate axis (`delta == 0.0`) is handled by a containment test
/// rather than a division. No numerical epsilon is introduced: a near-zero
/// axis produces enormous `t` values that the slab bounds reject correctly,
/// and `FLIGHT-PHYSICS` reserves an epsilon for avoiding singularities, not
/// for silently widening a shape.
fn earliest_time_of_impact(segment: &ProjectileSegment, target: &SweepTarget) -> Option<f64> {
    let p0 = segment.previous.to_array();
    let p1 = segment.current.to_array();
    let t0 = target.previous.to_array();
    let t1 = target.current.to_array();

    let mut enter: f64 = 0.0;
    let mut exit: f64 = 1.0;
    for axis in 0..3 {
        let start = p0[axis] - t0[axis];
        let delta = (p1[axis] - p0[axis]) - (t1[axis] - t0[axis]);
        let half = target.half_extents_m[axis];
        if delta == 0.0 {
            if start.abs() > half {
                return None;
            }
            continue;
        }
        let first = (-half - start) / delta;
        let second = (half - start) / delta;
        let (near, far) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        enter = enter.max(near);
        exit = exit.min(far);
        if enter > exit {
            return None;
        }
    }
    Some(enter)
}

// ------------------------------------------------- projectile runtime ----
//
// F27-B. The swept query above answers "does this segment cross this box?" for
// one tick. This section is the *runtime* that owns the rounds themselves: the
// per-tick cadence that turns accepted fire events into moving projectiles,
// keeps their swept segments, applies the shared wind conversion and retires a
// round when its declared lifetime is spent.
//
// It is deliberately platform-independent: no Bevy, no Avian, no renderer. The
// ECS side (`cs_app::weapons`) mirrors these positions onto Avian bodies and
// reads the live mount transforms; the geometry and the accounting live here.

/// One live projectile's authoritative motion state.
///
/// A round carries a **constant air-relative velocity**. It is spawned from
/// the accepted fire event's own world velocity with the wind removed once, by
/// the shared [`air_relative_velocity_m_s`], and every tick's world velocity
/// is the shared [`world_velocity_from_air_m_s`] of that air velocity and the
/// tick's wind. No drag, no gravity and no wind shear are applied: F27's
/// research boundary leaves them unmeasured and F19-B deliberately modelled
/// none of them (`docs/findings/2026-09-30-f19-wind-conversion-ownership.md`).
///
/// `previous` is where the round was at the start of the tick and `current`
/// where it is now; together they are the swept segment the query above tests.
/// The position is the *authoritative* one: an ECS body is a mirror, not a
/// second integrator.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveProjectile {
    projectile: ProjectileId,
    shooter: ActorId,
    previous: WorldPosition,
    current: WorldPosition,
    air_velocity_m_s: [f64; 3],
    ticks_remaining: u64,
}

impl LiveProjectile {
    /// The projectile's stable identity.
    #[must_use]
    pub const fn projectile(&self) -> ProjectileId {
        self.projectile
    }

    /// The actor that fired it.
    #[must_use]
    pub const fn shooter(&self) -> ActorId {
        self.shooter
    }

    /// Where the round was at the start of the current tick.
    #[must_use]
    pub const fn previous(&self) -> WorldPosition {
        self.previous
    }

    /// Where the round is now.
    #[must_use]
    pub const fn current(&self) -> WorldPosition {
        self.current
    }

    /// The constant air-relative velocity the round flies with.
    #[must_use]
    pub const fn air_velocity_m_s(&self) -> [f64; 3] {
        self.air_velocity_m_s
    }

    /// How many further ticks the round lives.
    #[must_use]
    pub const fn ticks_remaining(&self) -> u64 {
        self.ticks_remaining
    }

    /// The swept segment the round covered over the current tick.
    #[must_use]
    pub const fn segment(&self) -> ProjectileSegment {
        ProjectileSegment {
            projectile: self.projectile,
            previous: self.previous,
            current: self.current,
        }
    }

    /// The world velocity the round has in `wind`: the shared conversion of
    /// its constant air-relative velocity into the world frame. The wind is
    /// therefore subtracted exactly once, at spawn, and added back here.
    #[must_use]
    pub fn world_velocity_m_s(&self, wind_velocity_m_s: [f64; 3]) -> [f64; 3] {
        world_velocity_from_air_m_s(self.air_velocity_m_s, wind_velocity_m_s)
    }
}

/// One tick's accounting of the live projectiles.
///
/// `segments` is one swept segment per round that was live for the tick, in
/// ascending [`ProjectileId`] order (the map's own stable order). `expired`
/// names the rounds whose declared lifetime was spent on this tick, *after*
/// their final segment was produced: a caller sweeps the segment, then
/// retires the body the id names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectileTick {
    /// The swept segment each live round covered over the tick.
    pub segments: Vec<ProjectileSegment>,
    /// The rounds that reached the end of their declared lifetime this tick.
    pub expired: Vec<ProjectileId>,
}

impl ProjectileTick {
    /// Whether the tick produced no segment and retired no round.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() && self.expired.is_empty()
    }
}

/// Why a [`ProjectileRuntime`] operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectileRuntimeError {
    /// The fire event belongs to another session generation.
    ForeignSession {
        /// The runtime's session.
        expected: u64,
        /// The session the event carried.
        found: u64,
    },
    /// A round with this identity is already live. A projectile is spawned
    /// once; a duplicate event must not give one id two bodies.
    DuplicateProjectile {
        /// The repeated projectile.
        projectile: ProjectileId,
    },
    /// A spawn world velocity had a non-finite component.
    NonFiniteVelocity {
        /// The offending axis: `0` X, `1` Y, `2` Z.
        component: usize,
    },
    /// A wind velocity had a non-finite component.
    NonFiniteWind {
        /// The offending axis: `0` X, `1` Y, `2` Z.
        component: usize,
    },
    /// The tick length was negative, NaN or infinite.
    NonFiniteExtent {
        /// The field that was refused (`dt_s`).
        field: &'static str,
    },
    /// Advancing a round produced a non-finite position.
    NonFinitePosition {
        /// The round whose position left the representable range.
        projectile: ProjectileId,
    },
    /// A fire event declared a zero-tick lifetime, so the round could never
    /// exist as a projectile.
    ZeroLifetime {
        /// The round with no life.
        projectile: ProjectileId,
    },
}

impl ProjectileRuntimeError {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ForeignSession { .. } => "foreign_session",
            Self::DuplicateProjectile { .. } => "duplicate_projectile",
            Self::NonFiniteVelocity { .. } => "non_finite_velocity",
            Self::NonFiniteWind { .. } => "non_finite_wind",
            Self::NonFiniteExtent { .. } => "non_finite_extent",
            Self::NonFinitePosition { .. } => "non_finite_position",
            Self::ZeroLifetime { .. } => "zero_lifetime",
        }
    }
}

impl fmt::Display for ProjectileRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "a projectile belongs to session {found}, but this runtime is session {expected}"
            ),
            Self::DuplicateProjectile { projectile } => {
                write!(f, "{projectile} is already live")
            }
            Self::NonFiniteVelocity { component } => {
                write!(f, "a spawn velocity component {component} must be finite")
            }
            Self::NonFiniteWind { component } => {
                write!(f, "a wind component {component} must be finite")
            }
            Self::NonFiniteExtent { field } => write!(f, "{field} must be finite and non-negative"),
            Self::NonFinitePosition { projectile } => {
                write!(f, "{projectile} left the representable position range")
            }
            Self::ZeroLifetime { projectile } => {
                write!(f, "{projectile} was declared with a zero-tick lifetime")
            }
        }
    }
}

impl std::error::Error for ProjectileRuntimeError {}

/// The per-session set of live projectiles.
///
/// This is the F27-B runtime half the swept query above was missing: a fire
/// event spawns a round here, each tick advances every round along its
/// constant air-relative velocity (converted to the world frame through the
/// shared wind functions), and a round whose declared lifetime is spent is
/// retired after its final segment. The swept query stays a pure function of
/// one segment and a target list; this type is what produces the segments and
/// the identities they belong to.
///
/// It is keyed by [`ProjectileId`], so ids are never recycled: a round can be
/// removed and its id never reissued (`FireResolver::next_projectile_id`).
#[derive(Clone, Debug)]
pub struct ProjectileRuntime {
    session: u64,
    live: BTreeMap<ProjectileId, LiveProjectile>,
}

impl ProjectileRuntime {
    /// Opens a runtime for one session generation.
    #[must_use]
    pub const fn new(session: u64) -> Self {
        Self {
            session,
            live: BTreeMap::new(),
        }
    }

    /// The session generation this runtime is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// One live round, if it is still flying.
    #[must_use]
    pub fn get(&self, projectile: ProjectileId) -> Option<&LiveProjectile> {
        self.live.get(&projectile)
    }

    /// Every live round, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &LiveProjectile> {
        self.live.values()
    }

    /// How many rounds are live.
    #[must_use]
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether no round is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Every live round's current swept segment, in ascending id order.
    #[must_use]
    pub fn segments(&self) -> Vec<ProjectileSegment> {
        self.live.values().map(LiveProjectile::segment).collect()
    }

    /// Spawns one round from an accepted fire event.
    ///
    /// The event's projectile world velocity has the wind removed once, by the
    /// shared [`air_relative_velocity_m_s`], and the result is what the round
    /// keeps: `world_velocity_from_air_m_s` reconstructs the world velocity
    /// every tick from the current wind. This is the only conversion site, so
    /// the subtraction is never written out again
    /// (`docs/findings/2026-09-30-f19-wind-conversion-ownership.md`).
    ///
    /// # Errors
    ///
    /// [`ProjectileRuntimeError`] when the event belongs to another session, a
    /// round with the same id is already live, a velocity or wind component is
    /// non-finite, or the declared lifetime is zero.
    pub fn spawn(
        &mut self,
        event: &FireEvent,
        wind_velocity_m_s: [f64; 3],
    ) -> Result<ProjectileId, ProjectileRuntimeError> {
        if event.id.session != self.session {
            return Err(ProjectileRuntimeError::ForeignSession {
                expected: self.session,
                found: event.id.session,
            });
        }
        let projectile = event.projectile.projectile;
        if self.live.contains_key(&projectile) {
            return Err(ProjectileRuntimeError::DuplicateProjectile { projectile });
        }
        for (component, value) in event.projectile.velocity_mps.iter().enumerate() {
            if !value.is_finite() {
                return Err(ProjectileRuntimeError::NonFiniteVelocity { component });
            }
        }
        check_finite_wind(wind_velocity_m_s)?;
        if event.projectile.lifetime_ticks == 0 {
            return Err(ProjectileRuntimeError::ZeroLifetime { projectile });
        }
        let air_velocity_m_s =
            air_relative_velocity_m_s(event.projectile.velocity_mps, wind_velocity_m_s);
        let origin = event.projectile.origin;
        self.live.insert(
            projectile,
            LiveProjectile {
                projectile,
                shooter: event.shooter,
                previous: origin,
                current: origin,
                air_velocity_m_s,
                ticks_remaining: event.projectile.lifetime_ticks,
            },
        );
        Ok(projectile)
    }

    /// Advances every live round by one tick of `dt_s` seconds.
    ///
    /// The returned [`ProjectileTick`] names each round's swept segment and
    /// the rounds whose lifetime ended this tick. Expired rounds are removed
    /// from the runtime *after* their final segment is recorded, so one caller
    /// pass can sweep the last segment and then retire the body it names.
    ///
    /// The tick is **all-or-nothing**: the next state is built beside the live
    /// one and installed only once every round has advanced, so a round whose
    /// next position is not representable (an arithmetic overflow from
    /// otherwise finite inputs) refuses the whole tick and leaves every round
    /// exactly where it was.
    ///
    /// # Errors
    ///
    /// [`ProjectileRuntimeError::NonFiniteExtent`] when `dt_s` is NaN,
    /// infinite or negative, [`ProjectileRuntimeError::NonFiniteWind`] when a
    /// wind component is not finite, and
    /// [`ProjectileRuntimeError::NonFinitePosition`] when a round's next
    /// position is not representable.
    pub fn advance(
        &mut self,
        dt_s: f64,
        wind_velocity_m_s: [f64; 3],
    ) -> Result<ProjectileTick, ProjectileRuntimeError> {
        if !dt_s.is_finite() || dt_s < 0.0 {
            return Err(ProjectileRuntimeError::NonFiniteExtent { field: "dt_s" });
        }
        check_finite_wind(wind_velocity_m_s)?;
        let mut next = self.live.clone();
        let mut tick = ProjectileTick::default();
        for live in next.values_mut() {
            live.previous = live.current;
            let velocity = world_velocity_from_air_m_s(live.air_velocity_m_s, wind_velocity_m_s);
            let mut moved = live.current.to_array();
            for axis in 0..3 {
                moved[axis] += velocity[axis] * dt_s;
            }
            let current = WorldPosition::try_new(moved).map_err(|_| {
                ProjectileRuntimeError::NonFinitePosition {
                    projectile: live.projectile,
                }
            })?;
            live.current = current;
            tick.segments.push(live.segment());
            live.ticks_remaining = live.ticks_remaining.saturating_sub(1);
            if live.ticks_remaining == 0 {
                tick.expired.push(live.projectile);
            }
        }
        for projectile in &tick.expired {
            next.remove(projectile);
        }
        self.live = next;
        Ok(tick)
    }

    /// Removes one round, as a hit or a teardown does, returning it.
    ///
    /// A removed id is never reissued by this runtime; the id cursor belongs to
    /// [`FireResolver`].
    pub fn remove(&mut self, projectile: ProjectileId) -> Option<LiveProjectile> {
        self.live.remove(&projectile)
    }
}

/// Refuses a non-finite wind component.
fn check_finite_wind(wind_velocity_m_s: [f64; 3]) -> Result<(), ProjectileRuntimeError> {
    for (component, value) in wind_velocity_m_s.iter().enumerate() {
        if !value.is_finite() {
            return Err(ProjectileRuntimeError::NonFiniteWind { component });
        }
    }
    Ok(())
}

/// Why the per-tick gun cadence refused to fire.
#[derive(Clone, Debug, PartialEq)]
pub enum CadenceRefusal {
    /// The whole intent was refused; nothing fired and nothing was spawned.
    Intent(IntentRefusal),
    /// The intent resolved into shots, but a spawned round was refused by the
    /// projectile runtime.
    ///
    /// The caller-supplied wind is checked **before** the intent is resolved,
    /// so a corrupt wind refuses with no state change. A normally resolved
    /// event cannot otherwise reach this arm — the resolver allocates a fresh
    /// id, refuses non-finite geometry at the mount, and never declares a zero
    /// lifetime — but it is mapped rather than unwrapped so a future event
    /// cannot take the simulation down mid-fire.
    Projectile(ProjectileRuntimeError),
}

impl fmt::Display for CadenceRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Intent(source) => write!(f, "the fire intent was refused: {source}"),
            Self::Projectile(source) => {
                write!(f, "an accepted shot's projectile was refused: {source}")
            }
        }
    }
}

impl std::error::Error for CadenceRefusal {}

/// The per-tick gun cadence: one session's resolver and live projectiles,
/// advanced as one step.
///
/// This is the loop F27-A left unowned (`docs/findings/2026-10-01-f27-a-weapon-ammo-schemas-and-fire-events.md`):
/// [`GunCadence::advance_to`] walks the cooldowns exactly like
/// [`FireResolver::advance_to`], [`GunCadence::fire`] resolves one intent and
/// spawns every accepted shot's projectile in the same call, and
/// [`GunCadence::advance_projectiles`] moves the rounds. A caller therefore
/// cannot resolve a shot and forget to spawn its round, and a refused intent
/// (`IntentRefusal`) spawns nothing because the resolver produced no event.
///
/// It owns no ECS state: the live mount transforms are supplied per intent, and
/// the ECS mirror of the projectile positions is `cs_app::weapons`'s. The
/// cadence is where the *simulation* authority over the rounds lives.
#[derive(Clone, Debug)]
pub struct GunCadence {
    resolver: FireResolver,
    projectiles: ProjectileRuntime,
}

impl GunCadence {
    /// Opens a cadence for one session generation, positioned at `tick`.
    #[must_use]
    pub fn new(session: u64, tick: Tick) -> Self {
        Self {
            resolver: FireResolver::new(session, tick),
            projectiles: ProjectileRuntime::new(session),
        }
    }

    /// The session generation this cadence is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.resolver.session()
    }

    /// The tick this cadence is currently resolving.
    #[must_use]
    pub const fn tick(&self) -> Tick {
        self.resolver.tick()
    }

    /// Advances the cadence to `tick`, ticking every cooldown down once per
    /// elapsed tick. Time never runs backwards.
    pub fn advance_to(&mut self, tick: Tick) {
        self.resolver.advance_to(tick);
    }

    /// Registers one actor's guns, exactly as [`FireResolver::register`] does.
    ///
    /// # Errors
    ///
    /// [`FireError`] when the actor is registered already or two definitions
    /// share a mount.
    pub fn register(
        &mut self,
        shooter: ActorId,
        definitions: Vec<GunDefinition>,
        state: WeaponState,
    ) -> Result<(), FireError> {
        self.resolver.register(shooter, definitions, state)
    }

    /// One actor's weapon state, if registered.
    #[must_use]
    pub fn state(&self, shooter: &ActorId) -> Option<&WeaponState> {
        self.resolver.state(shooter)
    }

    /// One actor's mutable weapon state, if registered.
    pub fn state_mut(&mut self, shooter: &ActorId) -> Option<&mut WeaponState> {
        self.resolver.state_mut(shooter)
    }

    /// One actor's mounted gun on `mount`, if any.
    #[must_use]
    pub fn definition(&self, shooter: &ActorId, mount: &DamageNodeKey) -> Option<&GunDefinition> {
        self.resolver.definition(shooter, mount)
    }

    /// The live projectiles this cadence owns.
    #[must_use]
    pub const fn projectiles(&self) -> &ProjectileRuntime {
        &self.projectiles
    }

    /// The resolver this cadence drives, for callers that need a query it
    /// already exposes but the cadence does not re-export.
    #[must_use]
    pub const fn resolver(&self) -> &FireResolver {
        &self.resolver
    }

    /// Resolves one fire intent and spawns every accepted shot's projectile.
    ///
    /// A refused intent (`CadenceRefusal::Intent`) changes no state: no round
    /// is consumed, no cooldown starts and no projectile exists. That is the
    /// AC02 gate seen from the cadence: a disabled mount is refused per mount,
    /// so its shot is absent from `accepted`, spawns nothing here, and an
    /// accepted sibling shot on the same intent still fires.
    ///
    /// A non-finite wind is refused (`CadenceRefusal::Projectile`) **before**
    /// the intent is resolved, so it too changes no state rather than draining
    /// a round for a projectile that cannot spawn.
    ///
    /// # Errors
    ///
    /// [`CadenceRefusal::Intent`] for a whole-intent refusal and
    /// [`CadenceRefusal::Projectile`] for a spawn the runtime refused.
    pub fn fire(
        &mut self,
        intent: &FireIntent,
        transforms: &BTreeMap<DamageNodeKey, MountTransform>,
        wind_velocity_m_s: [f64; 3],
    ) -> Result<FireResolution, CadenceRefusal> {
        // The resolver consumes a round and starts a cooldown the moment it
        // accepts a shot, so the caller-supplied wind is validated before it
        // runs: a refused fire must not leave a round spent on a projectile
        // that could not spawn. `ProjectileRuntime::spawn` re-checks the same
        // input; this boundary check is what makes the refusal state-free.
        check_finite_wind(wind_velocity_m_s).map_err(CadenceRefusal::Projectile)?;
        let resolution = self
            .resolver
            .resolve(intent, transforms)
            .map_err(CadenceRefusal::Intent)?;
        for event in &resolution.accepted {
            self.projectiles
                .spawn(event, wind_velocity_m_s)
                .map_err(CadenceRefusal::Projectile)?;
        }
        Ok(resolution)
    }

    /// Advances every live projectile by one cadence tick of `dt_s` seconds.
    ///
    /// # Errors
    ///
    /// [`ProjectileRuntimeError`] as [`ProjectileRuntime::advance`].
    pub fn advance_projectiles(
        &mut self,
        dt_s: f64,
        wind_velocity_m_s: [f64; 3],
    ) -> Result<ProjectileTick, ProjectileRuntimeError> {
        self.projectiles.advance(dt_s, wind_velocity_m_s)
    }

    /// Removes one round, as a hit despawn does.
    pub fn remove_projectile(&mut self, projectile: ProjectileId) -> Option<LiveProjectile> {
        self.projectiles.remove(projectile)
    }
}

// ------------------------------------------------- sweep → damage routing ----

/// One accepted shot's swept hit, routed into the damage inputs it became.
///
/// This is the record F27-C produces: the hit the geometry found, the damage
/// node whose box was reached first, and the [`HitEvent`]s the round's
/// **declared per-channel damage amounts** became. Nothing here invents a
/// multiplier, a penetration or a ricochet: each channel carries the gun
/// definition's own amount, and which node a contact lands on is the
/// candidate's, not a proximity guess.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutedHit {
    /// The hit the sweep reported: which projectile, which actor, when.
    pub hit: SweptHit,
    /// The damage node the contact landed on, taken from the winning
    /// candidate.
    pub node: DamageNodeKey,
    /// The damage inputs this contact became, in
    /// [`WEAPON_DAMAGE_CHANNELS`] order: one per channel whose declared
    /// amount is above zero.
    ///
    /// A channel whose declared amount is zero produces **no** hit. That is
    /// not an optimization: a zero-amount hit would be a damage record
    /// carrying no damage, and the resolver would have to treat "applied 0"
    /// and "never routed" as the same thing. Emitting nothing keeps "this
    /// round does no internal damage" a statement about the gun's declared
    /// profile rather than a distinction a consumer must rediscover.
    ///
    /// Channels are *not* merged: a round with a nonzero armor amount and a
    /// nonzero internal amount produces two [`HitEvent`]s on the same node,
    /// because the resolver routes each channel through its own declared
    /// armor interception and overflow chain.
    pub damage: Vec<HitEvent>,
}

impl RoutedHit {
    /// Whether this contact produced no damage input at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.damage.is_empty()
    }

    /// The amount this contact routed on `channel`, summed over every hit
    /// routed on it. One hit per channel, so this is that hit's amount, or
    /// `0.0` when the channel declared nothing.
    #[must_use]
    pub fn amount_on(&self, channel: DamageChannel) -> f64 {
        self.damage
            .iter()
            .filter(|hit| hit.channel == channel)
            .map(|hit| hit.damage)
            .sum()
    }
}

/// Why a swept hit could not be routed into a damage input.
///
/// Every arm is a *named* refusal rather than a dropped hit: a contact that
/// produced no damage must say which of these it was, because a silently
/// missing hit looks exactly like a projectile that missed.
#[derive(Clone, Debug, PartialEq)]
pub enum SweepRefusal {
    /// The routing produced no candidate for a hit the sweep reported.
    ///
    /// Defensive: the candidate index a
    /// [`Ballistics::sweep_with_sources`] reports names an element of the
    /// very slice it was given, and this routing hands it the admitted
    /// candidates unchanged. The arm keeps the join total rather than
    /// indexing on a condition the sweep already guarantees.
    UnknownCandidate {
        /// The projectile that hit.
        projectile: ProjectileId,
        /// The actor that was hit.
        target: ActorId,
        /// The candidate index the sweep reported.
        candidate: usize,
    },
    /// A per-channel damage amount could not form a [`HitEvent`].
    ///
    /// Defensive: [`WeaponDamage::try_new`] refuses a non-finite or negative
    /// amount where the gun definition is built, and the shot carries that
    /// profile verbatim, so this arm is unreachable today. It is mapped
    /// rather than unwrapped so a future damage source cannot take the
    /// simulation down mid-routing.
    InvalidDamage {
        /// The actor that was hit.
        target: ActorId,
        /// The channel whose amount was refused.
        channel: DamageChannel,
        /// Why the runtime refused it.
        source: HitEventError,
    },
    /// The swept hit names a projectile this shot did not spawn.
    ///
    /// The routing is per shot: it applies the shot's declared damage, so a
    /// segment belonging to a different round must be refused rather than
    /// damage one aircraft with another round's profile.
    ForeignProjectile {
        /// The projectile the accepted shot spawned.
        expected: ProjectileId,
        /// The projectile the swept segment belongs to.
        found: ProjectileId,
    },
    /// The accepted shot belongs to another session generation.
    ForeignSession {
        /// The routing's session generation.
        expected: u64,
        /// The session the shot carried.
        found: u64,
    },
    /// This router was opened on session generation zero.
    ///
    /// The routed [`HitEvent`]s carry the shared `cs_types::net::EventId`,
    /// whose session is a nonzero `SessionId` — zero *is* "no session" in
    /// that type — while this module's own ids still carry a `u64`
    /// generation (F27-A; the migration is task #442). A router on zero
    /// therefore has no session to stamp a hit into, and refuses whole rather
    /// than routing damage into a generation that does not exist.
    ///
    /// Defensive for a session opened normally: `FireResolver::new` and this
    /// constructor take the generation the session allocated, and
    /// `SessionAllocator` issues from 1. The arm exists so a caller that
    /// passed 0 is refused by name instead of getting a hit whose id cannot be
    /// expressed.
    NoSession,
}

impl SweepRefusal {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::UnknownCandidate { .. } => "unknown_candidate",
            Self::InvalidDamage { .. } => "invalid_damage",
            Self::ForeignProjectile { .. } => "foreign_projectile",
            Self::ForeignSession { .. } => "foreign_session",
            Self::NoSession => "no_session",
        }
    }
}

impl fmt::Display for SweepRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCandidate {
                projectile,
                target,
                candidate,
            } => write!(
                f,
                "{projectile} hit {target} on candidate {candidate}, which this routing did not supply"
            ),
            Self::InvalidDamage {
                target,
                channel,
                source,
            } => write!(
                f,
                "the {} damage routed onto {target} was refused: {source}",
                channel.label()
            ),
            Self::ForeignProjectile { expected, found } => write!(
                f,
                "the swept segment belongs to {found}, but this shot spawned {expected}"
            ),
            Self::ForeignSession { expected, found } => write!(
                f,
                "the accepted shot belongs to session {found}, but this routing is session {expected}"
            ),
            Self::NoSession => write!(
                f,
                "this routing is session generation zero, which is not a session a hit can be stamped into"
            ),
        }
    }
}

impl std::error::Error for SweepRefusal {}

/// One routing pass's whole result: the candidates the rules admitted, the
/// hits they produced, and every refusal by name.
///
/// The three lists together are the whole outcome, so a caller cannot read
/// "hits" without also being able to see what was admitted and what was
/// refused. A pass that admitted nothing and refused nothing means the shot
/// crossed nothing — an empty [`SweepOutcome::hits`] is a *miss*, which is
/// why refusals are reported rather than dropped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SweepOutcome {
    /// The candidates the declared rules admitted, in the order the sweep
    /// received them.
    pub admitted: Vec<SweepCandidate>,
    /// The hits the sweep produced, in ascending time of impact, each with
    /// the damage inputs it became.
    pub hits: Vec<RoutedHit>,
    /// The contacts that produced no damage input, with the reason.
    pub refused: Vec<SweepRefusal>,
}

impl SweepOutcome {
    /// Whether the pass produced no hit at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.hits.is_empty()
    }

    /// Every [`HitEvent`] the pass produced, in hit order.
    #[must_use]
    pub fn damage(&self) -> Vec<HitEvent> {
        self.hits
            .iter()
            .flat_map(|hit| hit.damage.iter().cloned())
            .collect()
    }
}

/// The per-session stage that turns one accepted shot's swept contacts into
/// damage inputs.
///
/// This is the F27-C decision, recorded in
/// `docs/findings/2026-10-02-f27-c-candidate-filtering-and-hit-damage-routing.md`:
/// **the declared-rule filter stays in
/// [`WeaponRules::admit_candidates`], the geometry stays in
/// [`Ballistics::sweep_with_sources`], and the conversion into damage lives
/// here** — neither inside the sweep (which would make declared policy
/// reachable from geometry) nor inside `cs_sim::damage` (which consumes typed
/// hits and must know nothing about allegiance, gun banks or projectiles).
///
/// The three steps of one [`GunHitRouter::route`] call are exactly that
/// order, and each is observable: the admitted candidates, the swept hits
/// with their sources, and the routed [`HitEvent`]s.
///
/// It is session-confined: a shot from another generation is refused whole,
/// so a restarted session's projectile can never apply its damage to this
/// one's actors.
///
/// It **owns** the [`Ballistics`] ledger rather than borrowing one, because
/// the ledger's lifetime has to be the session's: the once-per-`(projectile,
/// actor)` guarantee only holds if the same ledger is consulted on every tick
/// a round is live, and a caller that supplied a fresh one per pass could
/// defeat it by accident. Holding it makes teardown *drop the router* — there
/// is nothing else to unwind and no retry path that has to restore a ledger
/// someone else owns. [`Ballistics`] stays public and usable on its own for
/// callers that only want the geometry.
#[derive(Clone, Debug)]
pub struct GunHitRouter {
    session: u64,
    producer: u32,
    next_sequence: u32,
    ballistics: Ballistics,
}

impl GunHitRouter {
    /// Opens a router for one session generation, stamping the [`HitEvent`]s
    /// it routes with producer serial `producer`.
    ///
    /// The producer is the *routing system's* serial, not the shooter's: a
    /// [`HitEventId`] carries a `u32` producer while an [`ActorId`] serial is
    /// never recycled inside a session, so narrowing a shooter's serial into
    /// it could give two shooters the same producer. The session's schedule
    /// allocates one serial for this stage, exactly as it does for
    /// `DamageResolver::new`'s producer.
    #[must_use]
    pub fn new(session: u64, producer: u32) -> Self {
        Self {
            session,
            producer,
            next_sequence: 0,
            ballistics: Ballistics::new(),
        }
    }

    /// The session's swept-hit ledger, for a caller that needs to ask what
    /// this router has already applied — and for the geometry-only uses that
    /// never route damage at all.
    #[must_use]
    pub const fn ballistics(&self) -> &Ballistics {
        &self.ballistics
    }

    /// The session generation this router is confined to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The producer serial stamped into the hit ids it routes.
    #[must_use]
    pub const fn producer(&self) -> u32 {
        self.producer
    }

    /// How many [`HitEvent`]s this router has routed.
    #[must_use]
    pub const fn routed(&self) -> u32 {
        self.next_sequence
    }

    /// Routes one accepted shot's swept contacts into damage inputs.
    ///
    /// `segment` is the projectile's motion across the tick being resolved and
    /// `candidates` are the world candidates the collision features reported
    /// for it. The pass:
    ///
    /// 1. filters the candidates through the gun's declared rules
    ///    ([`WeaponRules::admit_candidates`]) — self-hit exclusion and
    ///    friendly fire are decided by declaration, never by proximity;
    /// 2. sweeps the survivors against the segment, keeping which candidate
    ///    each hit came from;
    /// 3. converts every hit into one [`HitEvent`] per non-zero declared
    ///    damage channel, on the node the winning candidate named.
    ///
    /// `shot` supplies the attacker, the projectile identity and the declared
    /// per-channel damage amounts; the *node* comes from the candidate, never
    /// from the shot, because the shot describes the shooter and not the
    /// target's parts.
    ///
    /// `at` is the tick being resolved, and it — **not** `shot.id.tick` — is
    /// what the routed hits are stamped with. A projectile is fired on one
    /// tick and can land several ticks later, so the hit belongs to the tick
    /// its contact happened on: `DamageResolver::resolve` refuses a batch
    /// whose hits carry any other tick, and stamping a landing with its
    /// muzzle's tick would make every travelling round unresolvable.
    ///
    /// Nothing here applies the damage: the [`HitEvent`]s are inputs the
    /// authoritative `DamageResolver` resolves, so this stage cannot destroy
    /// anything. A retried route is caught by this router's own
    /// once-per-`(projectile, actor)` ledger rather than by a refusal, and a
    /// refused contact is reported in [`SweepOutcome::refused`] rather than
    /// dropped — an empty [`SweepOutcome::hits`] must be readable as "this
    /// round crossed nothing" rather than "this round hit something and the
    /// damage was lost".
    ///
    /// Teardown is dropping the router: the ledger and the hit-identity
    /// cursor are its own state, so there is nothing to unwind and a fresh
    /// session starts both again.
    pub fn route(
        &mut self,
        shot: &FireEvent,
        segment: &ProjectileSegment,
        candidates: impl IntoIterator<Item = SweepCandidate>,
        rules: &WeaponRules,
        at: Tick,
    ) -> SweepOutcome {
        let mut outcome = SweepOutcome::default();
        if shot.id.session != self.session {
            outcome.refused.push(SweepRefusal::ForeignSession {
                expected: self.session,
                found: shot.id.session,
            });
            return outcome;
        }
        // A routed hit carries the shared `cs_types::net::EventId`, whose
        // session is a nonzero `SessionId`, while this module's own ids still
        // carry a `u64` generation (F27-A, and the `cs_types` migration task
        // #442). Generation zero cannot become a `SessionId` — it *is* "no
        // session" in that type — so a router opened on zero refuses whole
        // rather than stamping a hit into a session that does not exist.
        let Some(session) = SessionId::new(self.session) else {
            outcome.refused.push(SweepRefusal::NoSession);
            return outcome;
        };
        if segment.projectile != shot.projectile.projectile {
            outcome.refused.push(SweepRefusal::ForeignProjectile {
                expected: shot.projectile.projectile,
                found: segment.projectile,
            });
            return outcome;
        }

        outcome.admitted = rules.admit_candidates(shot.shooter, candidates);
        let admitted: Vec<SweepTarget> = outcome.admitted.iter().map(|c| c.target).collect();
        for contact in self.ballistics.sweep_with_sources(segment, &admitted) {
            let Some(candidate) = outcome.admitted.get(contact.candidate) else {
                outcome.refused.push(SweepRefusal::UnknownCandidate {
                    projectile: contact.hit.projectile,
                    target: contact.hit.target,
                    candidate: contact.candidate,
                });
                continue;
            };
            let mut damage = Vec::new();
            let mut refused = None;
            for channel in WEAPON_DAMAGE_CHANNELS {
                let amount = shot.damage.amount_on(channel);
                if amount == 0.0 {
                    continue;
                }
                match HitEvent::try_new(
                    self.next_hit_id(session, at),
                    Some(shot.shooter),
                    contact.hit.target,
                    candidate.node.clone(),
                    channel,
                    amount,
                ) {
                    Ok(hit) => damage.push(hit),
                    Err(source) => {
                        refused = Some(SweepRefusal::InvalidDamage {
                            target: contact.hit.target,
                            channel,
                            source,
                        });
                        break;
                    }
                }
            }
            // A contact whose *second* channel is refused discards the first:
            // a half-applied contact would be damage that exists with no rule
            // behind it. Nothing is consumed either way beyond the id
            // sequence, which advances whether or not the hit is kept, so a
            // retried route never reissues an id.
            if let Some(refusal) = refused {
                outcome.refused.push(refusal);
                continue;
            }
            outcome.hits.push(RoutedHit {
                hit: contact.hit,
                node: candidate.node.clone(),
                damage,
            });
        }
        outcome
    }

    /// Allocates the next hit id of this session.
    ///
    /// The cursor is `u32` because a [`HitEventId`]'s sequence is, and it
    /// only ever increases: a wrap would reissue an id a batch already
    /// resolved. A session that routed more than `u32::MAX` hits would need a
    /// new producer serial, which is the schedule's decision and not a
    /// silent wrap here.
    fn next_hit_id(&mut self, session: SessionId, tick: Tick) -> HitEventId {
        let id = HitEventId {
            session,
            tick,
            producer: self.producer,
            sequence: self.next_sequence,
        };
        self.next_sequence = self.next_sequence.wrapping_add(1);
        id
    }
}

// ---------------------------------------------------------------- fixture ----

/// The fixture ammunition type's catalog key.
pub const SYNTHETIC_AMMO_KEY: &str = "synthetic.fixture_slug";
/// The fixture gun's mount key, matching the synthetic airframe damage
/// graph's weapon-mount node.
pub const SYNTHETIC_GUN_MOUNT: &str = "gun_mount_1";
/// The fixture gun's muzzle effect catalog key.
pub const SYNTHETIC_EFFECT_KEY: &str = "synthetic.fixture_muzzle";
/// The fixture gun's shot sound catalog key.
pub const SYNTHETIC_SOUND_KEY: &str = "synthetic.fixture_shot";
/// The fixture gun's caliber text.
pub const SYNTHETIC_CALIBER: &str = "synthetic fixture caliber";
/// The fixture gun's muzzle velocity, in meters per second.
pub const SYNTHETIC_MUZZLE_VELOCITY_MPS: f64 = 640.0;
/// The fixture gun's round lifetime, in ticks.
pub const SYNTHETIC_LIFETIME_TICKS: u64 = 90;
/// The fixture gun's spread cone half-angle, in radians.
pub const SYNTHETIC_SPREAD_HALF_ANGLE_RAD: f64 = 0.004;
/// The fixture gun's armor-channel damage per round.
pub const SYNTHETIC_ARMOR_DAMAGE: f64 = 6.0;
/// The fixture gun's internal-channel damage per round.
pub const SYNTHETIC_INTERNAL_DAMAGE: f64 = 3.0;
/// The fixture gun's rate, in ticks between shots.
pub const SYNTHETIC_TICKS_BETWEEN_SHOTS: u32 = 4;
/// Rounds a [`WeaponState`] starts with in the fixture.
pub const SYNTHETIC_STARTING_ROUNDS: u64 = 250;

/// The claim the fixture's synthetic values carry.
#[must_use]
pub fn synthetic_claim() -> ClaimId {
    ClaimId::new("f27a.synthetic-fixture").expect("the fixture claim id is valid")
}

/// The fixture's ammunition type: one synthetic `ammo` id.
///
/// Deliberately *not* a claim about the original catalogue. The word "slug"
/// appears only inside a fixture key; the real ammunition set is unknown
/// and F27-D audits it.
#[must_use]
pub fn synthetic_ammunition() -> AmmunitionId {
    AmmunitionId::try_new(
        ContentId::from_source(ContentKind::Ammo, SYNTHETIC_AMMO_KEY)
            .expect("the fixture ammunition id is valid"),
    )
    .expect("the fixture ammunition id is in the ammo namespace")
}

/// The fixture's mount key — the same weapon-mount node the synthetic
/// airframe damage graph declares, so a damage-driven disable and a weapon
/// gate are the same key.
#[must_use]
pub fn synthetic_mount() -> DamageNodeKey {
    DamageNodeKey::new(SYNTHETIC_GUN_MOUNT).expect("the fixture mount key is valid")
}

/// The fixture gun's muzzle effect id.
#[must_use]
pub fn synthetic_effect() -> ContentId {
    ContentId::from_source(ContentKind::HardpointEquipment, SYNTHETIC_EFFECT_KEY)
        .expect("the fixture effect id is valid")
}

/// The fixture gun's shot sound id.
#[must_use]
pub fn synthetic_sound() -> ContentId {
    ContentId::from_source(ContentKind::Sound, SYNTHETIC_SOUND_KEY)
        .expect("the fixture sound id is valid")
}

/// The fixture gun: a synthetic nose cannon with one known damage profile,
/// a declared muzzle velocity, lifetime and spread.
///
/// Every number here is newly authored fixture content. None of it is an
/// original Crimson Skies value and none of it is presented as one.
#[must_use]
pub fn synthetic_gun_definition() -> GunDefinition {
    GunDefinition::try_new(
        synthetic_mount(),
        GunMountKind::Nose,
        SYNTHETIC_CALIBER,
        synthetic_ammunition(),
        GunRate::try_new(SYNTHETIC_TICKS_BETWEEN_SHOTS).expect("the fixture rate is valid"),
        SYNTHETIC_MUZZLE_VELOCITY_MPS,
        SYNTHETIC_LIFETIME_TICKS,
        SpreadCone::try_new(SYNTHETIC_SPREAD_HALF_ANGLE_RAD).expect("the fixture spread is valid"),
        WeaponDamage::try_new(SYNTHETIC_ARMOR_DAMAGE, SYNTHETIC_INTERNAL_DAMAGE)
            .expect("the fixture damage is valid"),
        InheritanceRule::Full,
        synthetic_effect(),
        synthetic_sound(),
    )
    .expect("the fixture gun definition is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ammunition identity discipline: an id must be in the `ammo`
    /// namespace, and there is deliberately no closed enum to fall back on.
    #[test]
    fn accept_f27_a_ammunition_id_requires_the_ammo_namespace() {
        let good = ContentId::from_source(ContentKind::Ammo, SYNTHETIC_AMMO_KEY)
            .expect("a valid content id");
        assert!(AmmunitionId::try_new(good).is_ok());

        let wrong = ContentId::from_source(ContentKind::Gun, SYNTHETIC_AMMO_KEY)
            .expect("a valid content id");
        assert_eq!(
            AmmunitionId::try_new(wrong.clone()),
            Err(AmmunitionIdError::KindMismatch { id: wrong }),
            "a gun id is not an ammunition type"
        );
    }
}
