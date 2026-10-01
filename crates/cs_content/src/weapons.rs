//! The declared weapon and ammunition schema: provenance-carrying gun,
//! ammunition and interaction records (F27-A).
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! This module is the **content half** of the weapon contract — the
//! normalized record an importer produces and the catalog consumes. Its
//! runtime counterpart is `cs_sim::weapons` (the definition the resolver
//! runs and the state it keeps); the conversion boundary between them is
//! `cs_app::weapons`. The split mirrors `damage` ↔ `cs_sim::damage`: this
//! crate cannot depend on `cs_sim`, so the declared record keeps its own
//! typed vocabulary — mount kinds, damage channels, interaction rules and
//! the inheritance rule — and the boundary maps it field-wise.
//!
//! # Records
//!
//! A [`DeclaredGunDefinition`] names its `gun` — the `weapon` catalog id it
//! describes — carries an [`Origin`], and holds each field F27's deliverable
//! names as its own [`Resolved`] value: `mount`, `caliber`, `ammunition`,
//! `rate`, `muzzle_velocity_mps`, `lifetime_ticks`, `spread`, `damage` (one
//! entry per [`DeclaredDamageChannel`]), `effect`, `sound`,
//! `inheritance` and the [`InteractionRules`].
//!
//! Every load-bearing value is a [`Resolved`]: an unmeasured caliber,
//! cadence, muzzle velocity, spread model, penetration or ricochet behavior
//! is *either* known with [`Provenance`] *or* an explicit unknown with its
//! claim id and reason — never a silent default (F14 non-negotiable
//! behavior 3). The lowering boundary refuses an unknown rather than
//! inventing a gun, because a session must not fire a weapon whose
//! ballistic parameters were guessed.
//!
//! `mount` is a [`crate::damage::DamageNodeKey`] — the same identity
//! discipline the declared damage graph uses for its weapon-mount nodes —
//! and `scene_binding` ties the mount to its visual [`SceneNodeId`] in the
//! live aircraft hierarchy (F27 non-negotiable 2). The scene binding is
//! for the presentation and F27-B transform consumer; weapon decisions
//! never read it.
//!
//! [`AmmunitionId`] is a validated `ammo`-namespace [`ContentId`], not an
//! enum. F27 non-negotiable 1 names slug, armor-piercing, dum-dum and
//! explosive as *discovery leads* and forbids an unverified multiplier
//! table, so the original ammunition set stays unknown until F27-D
//! enumerates it: a [`DeclaredAmmunition`] here is a *named type with a
//! damage profile and an interaction behavior*, never an invented catalogue.
//!
//! # Designed vocabulary, not original data
//!
//! The original gun set, calibers, ammunition types, hardpoint layout,
//! cadence, muzzle velocities, lifetimes, spread model, damage numbers and
//! interaction rules are unmeasured (F27 "Research boundary"; the public
//! manual establishes no ammunition or ballistic table). Every kind, rule
//! name and fixture value here is **newly authored project design** carrying
//! `Origin::SyntheticFixture` and designed provenance, recorded in
//! `docs/findings/2026-10-01-f27-a-weapon-ammo-schemas-and-fire-events.md`.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

use crate::damage::DamageNodeKey;
use crate::scene::SceneNodeId;

/// Where a weapon sits on the airframe — the declared counterpart of
/// `cs_sim::weapons::GunMountKind`.
///
/// The list is **designed**, not measured: the original hardpoint layout is
/// unmeasured and F27-D maps it onto this set. The boundary lowers it
/// field-wise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredGunMountKind {
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

impl DeclaredGunMountKind {
    /// Every mount kind, in a stable order.
    pub const ALL: &'static [DeclaredGunMountKind] = &[
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

    /// Whether this mount is on a wing.
    #[must_use]
    pub const fn is_wing(self) -> bool {
        matches!(self, Self::WingLeft | Self::WingRight)
    }
}

impl fmt::Display for DeclaredGunMountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The damage channel one declared amount routes on.
///
/// Mirrors `cs_sim::damage::DamageChannel`; the boundary lowers it
/// field-wise. It chooses the channel, never a multiplier — the declared
/// amount **is** the damage on that channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredDamageChannel {
    /// Routed through the target's declared armor zone first.
    Armor,
    /// Applied to the named part directly; armor never intercepts it.
    Internal,
}

impl DeclaredDamageChannel {
    /// Every channel, in a stable order.
    pub const ALL: &'static [DeclaredDamageChannel] = &[Self::Armor, Self::Internal];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Armor => "armor",
            Self::Internal => "internal",
        }
    }
}

impl fmt::Display for DeclaredDamageChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared caliber of a gun or ammunition type.
///
/// A validated free string, not an enum: the original caliber vocabulary is
/// unmeasured, so a closed set here would be a fabrication. The text is
/// trimmed and must be non-empty and within [`MAX_CALIBER_LEN`] bytes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclaredCaliber(String);

impl DeclaredCaliber {
    /// Validates and normalizes caliber text.
    ///
    /// # Errors
    ///
    /// [`WeaponSchemaError::EmptyCaliber`] when the text is empty or
    /// whitespace, [`WeaponSchemaError::CaliberTooLong`] when it exceeds
    /// [`MAX_CALIBER_LEN`] bytes.
    pub fn try_new(text: &str) -> Result<Self, WeaponSchemaError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(WeaponSchemaError::EmptyCaliber);
        }
        if trimmed.len() > MAX_CALIBER_LEN {
            return Err(WeaponSchemaError::CaliberTooLong { len: trimmed.len() });
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// The normalized caliber text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeclaredCaliber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Maximum byte length of a [`DeclaredCaliber`].
pub const MAX_CALIBER_LEN: usize = 64;

/// One declared ammunition type's identity.
///
/// Not an enum: F27 non-negotiable 1 forbids an unverified catalogue, so a
/// type is an opaque `ammo`-namespace [`ContentId`] and the original set is
/// enumerated by F27-D from the installation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AmmunitionId(ContentId);

impl AmmunitionId {
    /// Wraps an ammunition content id, validating its namespace.
    ///
    /// # Errors
    ///
    /// [`WeaponSchemaError::AmmoKindMismatch`] when the id is not in the
    /// `ammo` namespace.
    pub fn try_new(id: ContentId) -> Result<Self, WeaponSchemaError> {
        if id.kind() != ContentKind::Ammo {
            return Err(WeaponSchemaError::AmmoKindMismatch { id });
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

/// How fast a declared gun may fire — in ticks between shots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclaredGunRate {
    /// Ticks the gun must wait between two accepted shots.
    pub ticks_between_shots: u32,
}

/// The declared damage profile of a gun: one amount per channel.
///
/// A gun declares a *profile*, never a scalar: how a target's armor
/// intercepts a channel is the damage resolver's declared rule, not the
/// gun's, so no multiplier is invented here. A gun may declare only one
/// channel if that is what its data supports.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredWeaponDamage {
    /// Damage on the armor channel, or an explicit unknown.
    pub armor: Resolved<f64>,
    /// Damage on the internal channel, or an explicit unknown.
    pub internal: Resolved<f64>,
}

impl DeclaredWeaponDamage {
    /// The declared amount on one channel, or `None` when the channel is
    /// not part of this gun's profile at all.
    ///
    /// An absent channel is distinct from a channel whose amount is
    /// `Resolved::Unknown`: the first is "this gun does not use that
    /// channel", the second is "we do not know what it does".
    #[must_use]
    pub fn channel(&self, channel: DeclaredDamageChannel) -> Option<&Resolved<f64>> {
        match channel {
            DeclaredDamageChannel::Armor => Some(&self.armor),
            DeclaredDamageChannel::Internal => Some(&self.internal),
        }
    }

    /// The known amount on one channel, if that channel is resolved.
    #[must_use]
    pub fn known_amount(&self, channel: DeclaredDamageChannel) -> Option<f64> {
        self.channel(channel).and_then(|resolved| match resolved {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        })
    }

    /// The known amount on one channel as a resolved-value accessor, kept for
    /// callers that want the whole [`Resolved`] rather than the bare number.
    #[must_use]
    pub fn amount(&self, channel: DeclaredDamageChannel) -> Option<&Resolved<f64>> {
        self.channel(channel)
    }

    /// Every channel's [`Resolved`] amount, in the stable channel order.
    #[must_use]
    pub fn entries(&self) -> [(&'static str, &Resolved<f64>); 2] {
        [
            (DeclaredDamageChannel::Armor.label(), &self.armor),
            (DeclaredDamageChannel::Internal.label(), &self.internal),
        ]
    }
}

/// The declared spread model of a gun: a cone half-angle.
///
/// The sampling model the original used is unmeasured, so this records the
/// cone only and draws no samples.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredSpreadCone {
    /// The cone half-angle, in radians, or an explicit unknown.
    pub half_angle: Resolved<Radians>,
}

/// Whether a declared gun's rounds may hit the airframe that fired them.
///
/// F27 non-negotiable 4 requires this be defined by evidence or marked
/// unknown; it is a declared [`Resolved`] option, never an implicit
/// exclusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredSelfHitRule {
    /// A round never damages the airframe that fired it.
    Excluded,
    /// A round may damage its own airframe.
    Allowed,
}

impl DeclaredSelfHitRule {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Excluded => "excluded",
            Self::Allowed => "allowed",
        }
    }
}

impl fmt::Display for DeclaredSelfHitRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which declared relations a gun's rounds may damage.
///
/// Mirrors `cs_sim::weapons::FriendlyFireRule`; the boundary lowers it
/// field-wise. An undeclared relation pair is not "friendly" — it is
/// undeclared, and only `Everyone` admits it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredFriendlyFireRule {
    /// Only a declared hostile relation may be damaged.
    HostileOnly,
    /// A declared hostile or neutral relation may be damaged; an ally never.
    NonFriendly,
    /// Every actor except the shooter may be damaged.
    Everyone,
}

impl DeclaredFriendlyFireRule {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::HostileOnly => "hostile_only",
            Self::NonFriendly => "non_friendly",
            Self::Everyone => "everyone",
        }
    }
}

impl fmt::Display for DeclaredFriendlyFireRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared rule for how much of the firing airframe's velocity a round
/// inherits.
///
/// F27 non-negotiable 2 makes inherited velocity an *explicit verified
/// rule*; the original's rule is unmeasured, so this is a declared
/// [`Resolved`] option the boundary refuses to guess.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclaredInheritanceRule {
    /// The round keeps the airframe's whole world velocity.
    Full,
    /// The round keeps a declared fraction of the airframe's velocity.
    Fraction {
        /// The declared share, which must be finite and within `[0, 1]`.
        share: f64,
    },
    /// The round keeps none of the airframe's velocity.
    None,
}

impl DeclaredInheritanceRule {
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

impl fmt::Display for DeclaredInheritanceRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fraction { share } => write!(f, "fraction({share})"),
            other => f.write_str(other.label()),
        }
    }
}

/// The declared interaction rules one gun's rounds run under.
///
/// Each field is a [`Resolved`]: a value the importer could not evidence
/// stays an explicit unknown and refuses to lower, so no session runs a gun
/// under a guessed self-hit, friendly-fire, penetration, ricochet or
/// ammo-switching rule (F27 non-negotiable 4).
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionRules {
    /// Whether a round may hit the airframe that fired it.
    pub self_hit: Resolved<DeclaredSelfHitRule>,
    /// Which declared relations a round may damage.
    pub friendly_fire: Resolved<DeclaredFriendlyFireRule>,
    /// Whether this ammunition type is declared to penetrate what it hits.
    pub penetration: Resolved<bool>,
    /// Whether this ammunition type is declared to ricochet.
    pub ricochet: Resolved<bool>,
    /// Whether a pilot may change ammunition type in flight.
    pub ammo_switching: Resolved<bool>,
}

/// Why a declared weapon record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum WeaponSchemaError {
    /// The gun id is not in the `weapon` namespace.
    GunKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The ammunition id is not in the `ammo` namespace.
    AmmoKindMismatch {
        /// The offending id.
        id: ContentId,
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
    /// The caliber text was empty or whitespace.
    EmptyCaliber,
    /// The caliber text exceeded [`MAX_CALIBER_LEN`] bytes.
    CaliberTooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The rate interval was zero.
    ZeroRateInterval,
    /// A known muzzle velocity was NaN or infinite.
    NonFiniteMuzzleVelocity,
    /// A known muzzle velocity was zero or negative.
    NonPositiveMuzzleVelocity {
        /// The rejected value.
        muzzle_velocity_mps: f64,
    },
    /// A known round lifetime was zero.
    ZeroLifetime,
    /// A known damage amount was NaN or infinite.
    NonFiniteDamage {
        /// The channel whose amount was corrupt.
        channel: DeclaredDamageChannel,
    },
    /// A known damage amount was negative.
    NegativeDamage {
        /// The channel whose amount was rejected.
        channel: DeclaredDamageChannel,
        /// The rejected value.
        amount: f64,
    },
    /// A known spread half-angle was NaN or infinite.
    NonFiniteSpread,
    /// A known spread half-angle fell outside `[0, π/2]`.
    SpreadOutOfRange {
        /// The rejected value.
        half_angle_radians: f64,
    },
    /// A declared inheritance share was NaN, infinite or outside `[0, 1]`.
    InvalidInheritanceShare {
        /// The rejected share.
        share: f64,
    },
}

impl fmt::Display for WeaponSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GunKindMismatch { id } => {
                write!(f, "gun id {id} is not in the weapon namespace")
            }
            Self::AmmoKindMismatch { id } => {
                write!(f, "ammunition id {id} is not in the ammo namespace")
            }
            Self::EffectKindMismatch { id } => {
                write!(
                    f,
                    "effect id {id} is not in the hardpoint_equipment namespace"
                )
            }
            Self::SoundKindMismatch { id } => {
                write!(f, "sound id {id} is not in the sound namespace")
            }
            Self::EmptyCaliber => write!(f, "a gun caliber must not be empty"),
            Self::CaliberTooLong { len } => {
                write!(f, "a gun caliber is {len} bytes, max is {MAX_CALIBER_LEN}")
            }
            Self::ZeroRateInterval => {
                write!(f, "a gun rate must be at least one tick between shots")
            }
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
            Self::NonFiniteDamage { channel } => {
                write!(f, "the {channel} damage amount must be finite")
            }
            Self::NegativeDamage { channel, amount } => {
                write!(f, "the {channel} damage amount is negative: {amount}")
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
        }
    }
}

impl std::error::Error for WeaponSchemaError {}

/// The declared definition of one gun.
///
/// `gun` is the `weapon` catalog id the record describes. Every load-bearing
/// value is a [`Resolved`] so an unmeasured parameter is recorded rather
/// than defaulted.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredGunDefinition {
    gun: ContentId,
    origin: Origin,
    mount: DamageNodeKey,
    mount_kind: DeclaredGunMountKind,
    scene_binding: Option<Resolved<SceneNodeId>>,
    caliber: Resolved<DeclaredCaliber>,
    ammunition: Resolved<AmmunitionId>,
    rate: Resolved<DeclaredGunRate>,
    muzzle_velocity_mps: Resolved<f64>,
    lifetime_ticks: Resolved<u64>,
    spread: DeclaredSpreadCone,
    damage: DeclaredWeaponDamage,
    inheritance: Resolved<DeclaredInheritanceRule>,
    effect: Resolved<ContentId>,
    sound: Resolved<ContentId>,
    rules: InteractionRules,
    provenance: Provenance,
}

impl DeclaredGunDefinition {
    /// Assembles and validates a declared gun record.
    ///
    /// Validation covers identity and known-value sanity only; it never
    /// decides an unknown. The lowering boundary is what refuses an unknown.
    ///
    /// # Errors
    ///
    /// [`WeaponSchemaError`] on a wrong-namespace gun, effect or sound id,
    /// an empty or over-long caliber, a zero rate interval, a corrupt known
    /// muzzle velocity, lifetime, damage amount, spread angle or inheritance
    /// share.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        gun: ContentId,
        origin: Origin,
        mount: DamageNodeKey,
        mount_kind: DeclaredGunMountKind,
        scene_binding: Option<Resolved<SceneNodeId>>,
        caliber: Resolved<DeclaredCaliber>,
        ammunition: Resolved<AmmunitionId>,
        rate: Resolved<DeclaredGunRate>,
        muzzle_velocity_mps: Resolved<f64>,
        lifetime_ticks: Resolved<u64>,
        spread: DeclaredSpreadCone,
        damage: DeclaredWeaponDamage,
        inheritance: Resolved<DeclaredInheritanceRule>,
        effect: Resolved<ContentId>,
        sound: Resolved<ContentId>,
        rules: InteractionRules,
        provenance: Provenance,
    ) -> Result<Self, WeaponSchemaError> {
        if gun.kind() != ContentKind::Weapon {
            return Err(WeaponSchemaError::GunKindMismatch { id: gun });
        }
        if let Resolved::Known(known) = &effect
            && known.value.kind() != ContentKind::HardpointEquipment
        {
            return Err(WeaponSchemaError::EffectKindMismatch {
                id: known.value.clone(),
            });
        }
        if let Resolved::Known(known) = &sound
            && known.value.kind() != ContentKind::Sound
        {
            return Err(WeaponSchemaError::SoundKindMismatch {
                id: known.value.clone(),
            });
        }
        if let Resolved::Known(known) = &rate
            && known.value.ticks_between_shots == 0
        {
            return Err(WeaponSchemaError::ZeroRateInterval);
        }
        if let Resolved::Known(known) = &muzzle_velocity_mps {
            if !known.value.is_finite() {
                return Err(WeaponSchemaError::NonFiniteMuzzleVelocity);
            }
            if known.value <= 0.0 {
                return Err(WeaponSchemaError::NonPositiveMuzzleVelocity {
                    muzzle_velocity_mps: known.value,
                });
            }
        }
        if let Resolved::Known(known) = &lifetime_ticks
            && known.value == 0
        {
            return Err(WeaponSchemaError::ZeroLifetime);
        }
        validate_damage(&damage)?;
        if let Resolved::Known(known) = &spread.half_angle {
            if !known.value.0.is_finite() {
                return Err(WeaponSchemaError::NonFiniteSpread);
            }
            if !(0.0..=std::f64::consts::FRAC_PI_2).contains(&known.value.0) {
                return Err(WeaponSchemaError::SpreadOutOfRange {
                    half_angle_radians: known.value.0,
                });
            }
        }
        if let Resolved::Known(known) = &inheritance
            && let DeclaredInheritanceRule::Fraction { share } = known.value
            && (!share.is_finite() || !(0.0..=1.0).contains(&share))
        {
            return Err(WeaponSchemaError::InvalidInheritanceShare { share });
        }
        Ok(Self {
            gun,
            origin,
            mount,
            mount_kind,
            scene_binding,
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
            rules,
            provenance,
        })
    }

    /// The `weapon` catalog id this record describes.
    #[must_use]
    pub const fn gun(&self) -> &ContentId {
        &self.gun
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The mount this gun occupies, by damage-node key.
    #[must_use]
    pub const fn mount(&self) -> &DamageNodeKey {
        &self.mount
    }

    /// Where on the airframe the mount sits.
    #[must_use]
    pub const fn mount_kind(&self) -> DeclaredGunMountKind {
        self.mount_kind
    }

    /// The visual mount binding into the live aircraft hierarchy, when the
    /// record declares one. Presentation and F27-B transforms read it;
    /// weapon decisions never do (F27 non-negotiable 2).
    #[must_use]
    pub const fn scene_binding(&self) -> Option<&Resolved<SceneNodeId>> {
        self.scene_binding.as_ref()
    }

    /// The declared caliber.
    #[must_use]
    pub const fn caliber(&self) -> &Resolved<DeclaredCaliber> {
        &self.caliber
    }

    /// The declared ammunition type.
    #[must_use]
    pub const fn ammunition(&self) -> &Resolved<AmmunitionId> {
        &self.ammunition
    }

    /// The declared rate.
    #[must_use]
    pub const fn rate(&self) -> &Resolved<DeclaredGunRate> {
        &self.rate
    }

    /// The declared muzzle velocity, in meters per second.
    #[must_use]
    pub const fn muzzle_velocity_mps(&self) -> &Resolved<f64> {
        &self.muzzle_velocity_mps
    }

    /// The declared round lifetime, in ticks.
    #[must_use]
    pub const fn lifetime_ticks(&self) -> &Resolved<u64> {
        &self.lifetime_ticks
    }

    /// The declared spread model.
    #[must_use]
    pub const fn spread(&self) -> &DeclaredSpreadCone {
        &self.spread
    }

    /// The declared damage profile.
    #[must_use]
    pub const fn damage(&self) -> &DeclaredWeaponDamage {
        &self.damage
    }

    /// The known declared caliber of this gun, if it is resolved.
    #[must_use]
    pub fn known_caliber(&self) -> Option<&str> {
        match &self.caliber {
            Resolved::Known(known) => Some(known.value.as_str()),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The known ammunition type this gun fires, if it is resolved.
    #[must_use]
    pub fn known_ammunition(&self) -> Option<&AmmunitionId> {
        match &self.ammunition {
            Resolved::Known(known) => Some(&known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The known inherited-velocity rule, if it is resolved.
    #[must_use]
    pub fn known_inheritance(&self) -> Option<DeclaredInheritanceRule> {
        match &self.inheritance {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The known muzzle effect id, if it is resolved.
    #[must_use]
    pub fn known_effect(&self) -> Option<&ContentId> {
        match &self.effect {
            Resolved::Known(known) => Some(&known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The known shot sound id, if it is resolved.
    #[must_use]
    pub fn known_sound(&self) -> Option<&ContentId> {
        match &self.sound {
            Resolved::Known(known) => Some(&known.value),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The declared inherited-velocity rule.
    #[must_use]
    pub const fn inheritance(&self) -> &Resolved<DeclaredInheritanceRule> {
        &self.inheritance
    }

    /// The declared muzzle/hit effect id.
    #[must_use]
    pub const fn effect(&self) -> &Resolved<ContentId> {
        &self.effect
    }

    /// The declared shot sound id.
    #[must_use]
    pub const fn sound(&self) -> &Resolved<ContentId> {
        &self.sound
    }

    /// The declared interaction rules.
    #[must_use]
    pub const fn rules(&self) -> &InteractionRules {
        &self.rules
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One declared ammunition type: its identity, its damage profile and its
/// interaction behavior.
///
/// A type's damage profile is **declared per type, never derived from a
/// multiplier table**: F27 non-negotiable 1 forbids hardcoding an unverified
/// table, so each channel's amount is its own [`Resolved`] with its own
/// provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredAmmunition {
    ammunition: AmmunitionId,
    origin: Origin,
    caliber: Resolved<DeclaredCaliber>,
    damage: DeclaredWeaponDamage,
    rules: InteractionRules,
    provenance: Provenance,
}

impl DeclaredAmmunition {
    /// Assembles and validates a declared ammunition record.
    ///
    /// # Errors
    ///
    /// [`WeaponSchemaError`] on a corrupt known damage amount.
    pub fn try_new(
        ammunition: AmmunitionId,
        origin: Origin,
        caliber: Resolved<DeclaredCaliber>,
        damage: DeclaredWeaponDamage,
        rules: InteractionRules,
        provenance: Provenance,
    ) -> Result<Self, WeaponSchemaError> {
        validate_damage(&damage)?;
        Ok(Self {
            ammunition,
            origin,
            caliber,
            damage,
            rules,
            provenance,
        })
    }

    /// The ammunition type's identity.
    #[must_use]
    pub const fn ammunition(&self) -> &AmmunitionId {
        &self.ammunition
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared caliber of this ammunition type.
    #[must_use]
    pub const fn caliber(&self) -> &Resolved<DeclaredCaliber> {
        &self.caliber
    }

    /// The declared damage profile of one round of this type.
    #[must_use]
    pub const fn damage(&self) -> &DeclaredWeaponDamage {
        &self.damage
    }

    /// The known amount of this type's damage on one channel.
    #[must_use]
    pub fn known_damage(&self, channel: DeclaredDamageChannel) -> Option<f64> {
        self.damage.known_amount(channel)
    }

    /// The known declared caliber of this type.
    #[must_use]
    pub fn known_caliber(&self) -> Option<&str> {
        match &self.caliber {
            Resolved::Known(known) => Some(known.value.as_str()),
            Resolved::Unknown { .. } => None,
        }
    }

    /// The declared interaction rules of this type.
    #[must_use]
    pub const fn rules(&self) -> &InteractionRules {
        &self.rules
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// The structural validation both declared records share: every known
/// damage amount must be finite and non-negative.
///
/// An `Unknown` amount is *not* validated and *not* repaired — it is a
/// legal declared record, and it is the lowering boundary that refuses it.
fn validate_damage(damage: &DeclaredWeaponDamage) -> Result<(), WeaponSchemaError> {
    for channel in DeclaredDamageChannel::ALL {
        if let Some(Resolved::Known(known)) = damage.channel(*channel) {
            if !known.value.is_finite() {
                return Err(WeaponSchemaError::NonFiniteDamage { channel: *channel });
            }
            if known.value < 0.0 {
                return Err(WeaponSchemaError::NegativeDamage {
                    channel: *channel,
                    amount: known.value,
                });
            }
        }
    }
    Ok(())
}

/// A declared loadout: the guns and ammunition one actor starts with.
///
/// AC04's "ammo/loadout audit maps every type to its behavior and damage
/// consumer" runs against a loadout, so the record that ties a gun to the
/// ammunition it is loaded with is declared here. The audit that walks it
/// is F27-D.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredLoadout {
    subject: ContentId,
    origin: Origin,
    guns: Vec<ContentId>,
    ammunition: Vec<AmmunitionId>,
    provenance: Provenance,
}

impl DeclaredLoadout {
    /// Assembles a declared loadout.
    ///
    /// # Errors
    ///
    /// [`LoadoutSchemaError`] when the subject is not a `loadout` id, when
    /// no gun is declared, when a gun or ammunition id is duplicated, or
    /// when an ammunition id the guns reference is not declared.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        guns: Vec<ContentId>,
        ammunition: Vec<AmmunitionId>,
        provenance: Provenance,
    ) -> Result<Self, LoadoutSchemaError> {
        if subject.kind() != ContentKind::Loadout {
            return Err(LoadoutSchemaError::SubjectKindMismatch { id: subject });
        }
        if guns.is_empty() {
            return Err(LoadoutSchemaError::EmptyLoadout);
        }
        let mut seen_guns = BTreeMap::new();
        for gun in &guns {
            if gun.kind() != ContentKind::Weapon {
                return Err(LoadoutSchemaError::GunKindMismatch { id: gun.clone() });
            }
            if seen_guns.insert(gun.clone(), ()).is_some() {
                return Err(LoadoutSchemaError::DuplicateGun { id: gun.clone() });
            }
        }
        let mut seen_ammo = BTreeMap::new();
        for ammo in &ammunition {
            if seen_ammo.insert(ammo.as_str(), ()).is_some() {
                return Err(LoadoutSchemaError::DuplicateAmmunition {
                    id: ammo.as_str().to_owned(),
                });
            }
        }
        Ok(Self {
            subject,
            origin,
            guns,
            ammunition,
            provenance,
        })
    }

    /// The `loadout` catalog id this record describes.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared guns, in authored order.
    #[must_use]
    pub fn guns(&self) -> &[ContentId] {
        &self.guns
    }

    /// The declared ammunition types, in authored order.
    #[must_use]
    pub fn ammunition(&self) -> &[AmmunitionId] {
        &self.ammunition
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Every `(gun, ammunition)` pair this loadout pairs — the rows an
    /// ammunition audit walks.
    ///
    /// A loadout pairs each gun with each declared type: an airframe that
    /// carries both a machine gun and a cannon and is loaded with two types
    /// can fire any gun with any type, and the audit must cover every
    /// pairing rather than assume a per-gun default.
    pub fn pairings(&self) -> Vec<(&ContentId, &AmmunitionId)> {
        self.guns
            .iter()
            .flat_map(|gun| self.ammunition.iter().map(move |ammo| (gun, ammo)))
            .collect()
    }
}

/// Why a [`DeclaredLoadout`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadoutSchemaError {
    /// The subject id is not in the `loadout` namespace.
    SubjectKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The loadout declares no gun.
    EmptyLoadout,
    /// A gun entry is not in the `weapon` namespace.
    GunKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// The same gun is declared twice.
    DuplicateGun {
        /// The duplicated id.
        id: ContentId,
    },
    /// The same ammunition type is declared twice.
    DuplicateAmmunition {
        /// The duplicated id text.
        id: String,
    },
}

impl fmt::Display for LoadoutSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectKindMismatch { id } => {
                write!(f, "loadout subject {id} is not in the loadout namespace")
            }
            Self::EmptyLoadout => write!(f, "a loadout must declare at least one gun"),
            Self::GunKindMismatch { id } => {
                write!(f, "loadout gun {id} is not in the weapon namespace")
            }
            Self::DuplicateGun { id } => write!(f, "loadout gun {id} is declared twice"),
            Self::DuplicateAmmunition { id } => {
                write!(f, "loadout ammunition {id} is declared twice")
            }
        }
    }
}

impl std::error::Error for LoadoutSchemaError {}

// ---------------------------------------------------------------- fixture ----

/// The synthetic fixture's weapon catalog key.
pub const SYNTHETIC_GUN_KEY: &str = "synthetic.fixture_gun";
/// The synthetic fixture's ammunition catalog key.
pub const SYNTHETIC_AMMO_KEY: &str = "synthetic.fixture_slug";
/// The synthetic fixture's loadout catalog key.
pub const SYNTHETIC_LOADOUT_KEY: &str = "synthetic.fixture_loadout";
/// The synthetic fixture's mount key, matching the synthetic airframe
/// damage graph's weapon-mount node.
pub const SYNTHETIC_GUN_MOUNT: &str = "gun_mount_1";
/// The synthetic fixture's effect catalog key.
pub const SYNTHETIC_EFFECT_KEY: &str = "synthetic.fixture_muzzle";
/// The synthetic fixture's sound catalog key.
pub const SYNTHETIC_SOUND_KEY: &str = "synthetic.fixture_shot";
/// The synthetic fixture's caliber text.
pub const SYNTHETIC_CALIBER: &str = "synthetic fixture caliber";
/// The synthetic fixture's muzzle velocity, in meters per second.
pub const SYNTHETIC_MUZZLE_VELOCITY_MPS: f64 = 640.0;
/// The synthetic fixture's round lifetime, in ticks.
pub const SYNTHETIC_LIFETIME_TICKS: u64 = 90;
/// The synthetic fixture's spread cone half-angle, in radians.
pub const SYNTHETIC_SPREAD_HALF_ANGLE_RAD: f64 = 0.004;
/// The synthetic fixture's armor-channel damage.
pub const SYNTHETIC_ARMOR_DAMAGE: f64 = 6.0;
/// The synthetic fixture's internal-channel damage.
pub const SYNTHETIC_INTERNAL_DAMAGE: f64 = 3.0;
/// The synthetic fixture's rate, in ticks between shots.
pub const SYNTHETIC_TICKS_BETWEEN_SHOTS: u32 = 4;

/// The claim the declared fixtures carry.
#[must_use]
pub fn synthetic_claim() -> ClaimId {
    ClaimId::new("f27a.synthetic-fixture").expect("the fixture claim id is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(cs_types::content::Known::new(
        value,
        Provenance::designed(synthetic_claim()),
    ))
}

/// The declared interaction rules the fixture uses: every rule `Known` with
/// *designed* provenance.
///
/// Every one of these is project design. The original's self-hit,
/// friendly-fire, penetration, ricochet and ammo-switching behavior is
/// unmeasured, which is exactly why the schema carries them as `Resolved`
/// options; the fixture declares a value only so a session has something
/// to run under, and it is never evidence.
#[must_use]
pub fn synthetic_interaction_rules() -> InteractionRules {
    InteractionRules {
        self_hit: known(DeclaredSelfHitRule::Excluded),
        friendly_fire: known(DeclaredFriendlyFireRule::HostileOnly),
        penetration: known(false),
        ricochet: known(false),
        ammo_switching: known(false),
    }
}

/// The declared synthetic ammunition type.
///
/// One designed fixture type, deliberately *not* a claim about the original
/// catalogue: the word "slug" appears only inside a fixture key.
#[must_use]
pub fn declared_synthetic_ammunition() -> DeclaredAmmunition {
    DeclaredAmmunition::try_new(
        synthetic_ammunition_id(),
        Origin::SyntheticFixture,
        known(DeclaredCaliber::try_new(SYNTHETIC_CALIBER).expect("the fixture caliber is valid")),
        DeclaredWeaponDamage {
            armor: known(SYNTHETIC_ARMOR_DAMAGE),
            internal: known(SYNTHETIC_INTERNAL_DAMAGE),
        },
        synthetic_interaction_rules(),
        Provenance::designed(synthetic_claim()),
    )
    .expect("the declared synthetic ammunition is valid")
}

/// The declared synthetic gun: a nose cannon with one known damage profile,
/// a declared rate, muzzle velocity, lifetime, spread and inheritance rule.
///
/// Every number is newly authored fixture content, never original game data.
#[must_use]
pub fn declared_synthetic_gun() -> DeclaredGunDefinition {
    DeclaredGunDefinition::try_new(
        ContentId::from_source(ContentKind::Weapon, SYNTHETIC_GUN_KEY)
            .expect("the fixture gun id is valid"),
        Origin::SyntheticFixture,
        crate::damage::DamageNodeKey::new(SYNTHETIC_GUN_MOUNT)
            .expect("the fixture mount key is valid"),
        DeclaredGunMountKind::Nose,
        None,
        known(DeclaredCaliber::try_new(SYNTHETIC_CALIBER).expect("the fixture caliber is valid")),
        known(synthetic_ammunition_id()),
        known(DeclaredGunRate {
            ticks_between_shots: SYNTHETIC_TICKS_BETWEEN_SHOTS,
        }),
        known(SYNTHETIC_MUZZLE_VELOCITY_MPS),
        known(SYNTHETIC_LIFETIME_TICKS),
        DeclaredSpreadCone {
            half_angle: known(Radians(SYNTHETIC_SPREAD_HALF_ANGLE_RAD)),
        },
        DeclaredWeaponDamage {
            armor: known(SYNTHETIC_ARMOR_DAMAGE),
            internal: known(SYNTHETIC_INTERNAL_DAMAGE),
        },
        known(DeclaredInheritanceRule::Full),
        known(synthetic_effect_id()),
        known(synthetic_sound_id()),
        synthetic_interaction_rules(),
        Provenance::designed(synthetic_claim()),
    )
    .expect("the declared synthetic gun is valid")
}

/// The declared synthetic loadout: the fixture gun plus the fixture
/// ammunition type, which is the one `(gun, ammunition)` pairing F27-D's
/// audit walks.
#[must_use]
pub fn declared_synthetic_loadout() -> DeclaredLoadout {
    DeclaredLoadout::try_new(
        ContentId::from_source(ContentKind::Loadout, SYNTHETIC_LOADOUT_KEY)
            .expect("the fixture loadout id is valid"),
        Origin::SyntheticFixture,
        vec![
            ContentId::from_source(ContentKind::Weapon, SYNTHETIC_GUN_KEY)
                .expect("the fixture gun id is valid"),
        ],
        vec![synthetic_ammunition_id()],
        Provenance::designed(synthetic_claim()),
    )
    .expect("the declared synthetic loadout is valid")
}

/// The fixture's ammunition type id.
#[must_use]
pub fn synthetic_ammunition_id() -> AmmunitionId {
    AmmunitionId::try_new(
        ContentId::from_source(ContentKind::Ammo, SYNTHETIC_AMMO_KEY)
            .expect("the fixture ammunition id is valid"),
    )
    .expect("the fixture ammunition id is in the ammo namespace")
}

/// The fixture gun's muzzle effect id.
#[must_use]
pub fn synthetic_effect_id() -> ContentId {
    ContentId::from_source(ContentKind::HardpointEquipment, SYNTHETIC_EFFECT_KEY)
        .expect("the fixture effect id is valid")
}

/// The fixture gun's shot sound id.
#[must_use]
pub fn synthetic_sound_id() -> ContentId {
    ContentId::from_source(ContentKind::Sound, SYNTHETIC_SOUND_KEY)
        .expect("the fixture sound id is valid")
}
