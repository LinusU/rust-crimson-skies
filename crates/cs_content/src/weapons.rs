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
//! The original gun set, calibers, per-type ammunition damage, convergence rule
//! and interaction rules are unmeasured (F27 "Research boundary"; the public
//! manual establishes no ammunition or ballistic table). Every kind, rule name
//! and fixture value here is **newly authored project design** carrying
//! `Origin::SyntheticFixture` and designed provenance, recorded in
//! `docs/findings/2026-10-01-f27-a-weapon-ammo-schemas-and-fire-events.md`.
//!
//! What F27-D **measured** is the *shape* of the original's loadout surface —
//! how many ammunition types, guns, gun slots and rocket slots it declares and
//! which gun groups it names ([`OriginalGunLoadout`], [`ORIGINAL_GUN_GROUPS`]).
//! [`AmmunitionAudit`] audits a declared catalogue against that surface and
//! reports every gap by name; it never fills one. What stays unmeasured is
//! recorded in
//! `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::Radians;

use crate::damage::DamageNodeKey;
use crate::scene::SceneNodeId;

/// Where a weapon sits on the airframe — the declared counterpart of
/// `cs_sim::weapons::GunMountKind`.
///
/// The list is **designed**, not measured: it is the smallest set that states
/// AC02 ("a disabled *wing* gun") and the per-mount discipline of non-negotiable
/// 2. F27-D mapped the original's measured hardpoint vocabulary onto it and
/// reports the part that does not fit — see
/// [`DeclaredGunMountKind::original_groups`] and
/// [`uncovered_original_gun_groups`]. The boundary lowers the kind field-wise.
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

    /// The gun-group ids this kind covers, in [`ORIGINAL_GUN_GROUPS`] order.
    ///
    /// **A partial mapping, and that is the point.** These are exactly the
    /// groups whose kind the original's own header macro determines: a label
    /// that says *left* is a left wing gun, *right* a right wing gun, *nose* a
    /// nose gun, *rear* a tail gun and *fuselage* a fuselage gun. The remaining
    /// eleven groups — the inner/outer/middle wing, upper/lower wing and centre
    /// groups and their `2` variants — name a wing station and often omit the
    /// side, which needs the per-airframe tables inside the executable. They
    /// stay uncovered and [`AmmunitionAudit`] reports them; assigning them a
    /// side would be a guess presented as a mount rule.
    #[must_use]
    pub fn original_groups(self) -> Vec<DeclaredGunGroup> {
        ORIGINAL_GUN_GROUPS
            .iter()
            .copied()
            .filter(|group| self.covers_group(group.id))
            .collect()
    }

    /// Whether this kind covers the gun group with string id `id`.
    #[must_use]
    pub const fn covers_group(self, id: u32) -> bool {
        matches!(
            (self, id),
            (Self::Nose, 3063 | 3064 | 3071 | 3072 | 3080)
                | (Self::Tail, 3073)
                | (Self::Gondola, 3066)
                | (Self::WingLeft, 3068)
                | (Self::WingRight, 3067)
        )
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
///
/// # Declared is not the same as applied
///
/// `self_hit` and `friendly_fire` are **applied**: the lowering boundary
/// carries them into `cs_sim::weapons::WeaponRules`, whose
/// `admit_candidates` decides every sweep's candidate list from them.
///
/// `penetration`, `ricochet` and `ammo_switching` are declared, typed and
/// carried, but **no production code reads them**. They are not placeholders
/// for a feature this stage should have built: a penetration or ricochet
/// *model* is exactly the "simulator features unsupported by game content"
/// F27 non-negotiable 4 forbids inventing, and in-flight ammunition switching
/// needs a multi-type per-mount inventory whose selection rule is unmeasured.
/// The original behavior is unmeasured as well (F27 "Research boundary"), so
/// implementing any of them now would be guesswork.
///
/// The deferral is therefore **part of the schema**, not only prose in a
/// findings file: [`InteractionOption`] names each option, and
/// [`InteractionRules::applied_by`] / [`InteractionRules::deferred`] report
/// which production path applies it and which stage must resolve it, so an
/// audit can ask the content contract itself. `cs_sim::weapons::WeaponRules`
/// carries the same two booleans and the same note.
#[derive(Clone, Debug, PartialEq)]
pub struct InteractionRules {
    /// Whether a round may hit the airframe that fired it.
    pub self_hit: Resolved<DeclaredSelfHitRule>,
    /// Which declared relations a round may damage.
    pub friendly_fire: Resolved<DeclaredFriendlyFireRule>,
    /// Whether this ammunition type is declared to penetrate what it hits.
    ///
    /// **Declared, not applied**: no production code reads it. See
    /// [`InteractionRules`] and [`InteractionOption::Penetration`].
    pub penetration: Resolved<bool>,
    /// Whether this ammunition type is declared to ricochet.
    ///
    /// **Declared, not applied**: no production code reads it. See
    /// [`InteractionRules`] and [`InteractionOption::Ricochet`].
    pub ricochet: Resolved<bool>,
    /// Whether a pilot may change ammunition type in flight.
    ///
    /// **Declared, not applied**: no production code reads it. See
    /// [`InteractionRules`] and [`InteractionOption::AmmoSwitching`].
    pub ammo_switching: Resolved<bool>,
}

/// One declared interaction option of [`InteractionRules`].
///
/// Naming the options makes "declared but not applied" a queryable fact about
/// the schema rather than a claim in prose: [`InteractionRules::deferred`]
/// reports the ones no production path reads, and
/// [`InteractionRules::applied_by`] names the path that reads the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum InteractionOption {
    /// Whether a round may hit the airframe that fired it.
    SelfHit,
    /// Which declared relations a round may damage.
    FriendlyFire,
    /// Whether a round is declared to penetrate what it hits.
    Penetration,
    /// Whether a round is declared to ricochet.
    Ricochet,
    /// Whether a pilot may change ammunition type in flight.
    AmmoSwitching,
}

impl InteractionOption {
    /// Every option, in a stable order.
    pub const ALL: &'static [InteractionOption] = &[
        Self::SelfHit,
        Self::FriendlyFire,
        Self::Penetration,
        Self::Ricochet,
        Self::AmmoSwitching,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SelfHit => "self_hit",
            Self::FriendlyFire => "friendly_fire",
            Self::Penetration => "penetration",
            Self::Ricochet => "ricochet",
            Self::AmmoSwitching => "ammo_switching",
        }
    }

    /// The name of the production path that **applies** this option today, or
    /// [`None`] when no production code reads it.
    ///
    /// The name is a pointer, not a claim that the path decides the option
    /// alone: [`APPLIED_BY_SELF_HIT_FILTER`] and
    /// [`APPLIED_BY_FRIENDLY_FIRE_FILTER`] are the same
    /// `cs_sim::weapons::WeaponRules::admit_candidates` predicate, which
    /// consults both declared rules together.
    #[must_use]
    pub const fn applied_by(self) -> Option<&'static str> {
        match self {
            Self::SelfHit | Self::FriendlyFire => Some(APPLIED_BY_CANDIDATE_FILTER),
            Self::Penetration | Self::Ricochet | Self::AmmoSwitching => None,
        }
    }

    /// The stage that must resolve this option when no production path
    /// applies it, with the reason it is not applied yet.
    ///
    /// `None` for an applied option: there is nothing outstanding to resolve
    /// *as a deferral* (the option's *value* may still be unknown, which is
    /// what `Resolved` is for).
    #[must_use]
    pub const fn deferred_to(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::SelfHit | Self::FriendlyFire => None,
            Self::Penetration => Some((
                DEFERRAL_STAGE,
                "a penetration model would have to invent what a round does after \
                 it passes through a part; F27 non-negotiable 4 forbids a simulator \
                 feature game content does not support, and the original ammunition's \
                 behavior is unmeasured",
            )),
            Self::Ricochet => Some((
                DEFERRAL_STAGE,
                "a ricochet model would have to invent the direction a round leaves \
                 a part in; the original's behavior is unmeasured, so a modeled \
                 reflection would be a guess presented as a rule",
            )),
            Self::AmmoSwitching => Some((
                DEFERRAL_STAGE,
                "switching ammunition type in flight needs a per-mount inventory of \
                 several types and a selection rule over them; neither exists, and \
                 selecting a gun bank (cs_sim::weapons::WeaponState::select) is a \
                 different rule that must not be mistaken for it",
            )),
        }
    }
}

impl fmt::Display for InteractionOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The production path that applies the declared self-hit and friendly-fire
/// rules: `cs_sim::weapons::WeaponRules::admit_candidates`, the F27-C
/// candidate query every swept hit passes through.
pub const APPLIED_BY_CANDIDATE_FILTER: &str = "cs_sim::weapons::WeaponRules::admit_candidates";

/// The stage a declared-but-unapplied interaction option is deferred to.
///
/// F27-D is the stage whose scenario is the original ammo/loadout audit
/// ("every type maps to its behavior and damage consumer"), so it is where a
/// measured rule can replace a deferral.
pub const DEFERRAL_STAGE: &str = "F27-D";

impl InteractionRules {
    /// The production path that applies `option` today, or [`None`] when no
    /// production code reads it.
    ///
    /// This reads the **schema**, not this record's values: a
    /// `Resolved::Unknown` self-hit rule is still an option a production path
    /// consults — it is the lowering boundary that refuses it, which is a
    /// different question from whether anything applies the rule.
    #[must_use]
    pub const fn applied_by(&self, option: InteractionOption) -> Option<&'static str> {
        option.applied_by()
    }

    /// The options no production path applies yet, in
    /// [`InteractionOption::ALL`] order, each with the stage that must resolve
    /// it.
    ///
    /// An empty result means every declared option has an applying path; a
    /// non-empty one names the gap explicitly, so the deferral survives in the
    /// machine-readable contract and not only in a findings file.
    #[must_use]
    pub fn deferred(&self) -> Vec<(InteractionOption, &'static str, &'static str)> {
        InteractionOption::ALL
            .iter()
            .filter_map(|option| {
                let (stage, reason) = (*option).deferred_to()?;
                Some((*option, stage, reason))
            })
            .collect()
    }

    /// Whether `option`'s value is known.
    ///
    /// An unknown option refuses to lower, whatever
    /// [`InteractionRules::applied_by`] says: the option is *consulted* by a
    /// production path, and this record cannot answer what it is. The two
    /// questions are separate and are reported separately, so "we apply the
    /// self-hit rule" and "we know what the self-hit rule is" never get
    /// confused.
    #[must_use]
    pub fn is_known(&self, option: InteractionOption) -> bool {
        match option {
            InteractionOption::SelfHit => self.self_hit.is_known(),
            InteractionOption::FriendlyFire => self.friendly_fire.is_known(),
            InteractionOption::Penetration => self.penetration.is_known(),
            InteractionOption::Ricochet => self.ricochet.is_known(),
            InteractionOption::AmmoSwitching => self.ammo_switching.is_known(),
        }
    }
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
    /// The record ties guns and ammunition types together but cannot check
    /// *which* type a given gun is loaded with: a gun entry is a bare
    /// `weapon` id, and the declared gun's own [`Resolved<AmmunitionId>`]
    /// field is what names its ammunition. F27-D's audit therefore walks
    /// [`DeclaredLoadout::pairings`] — every gun with every declared type —
    /// rather than a per-gun default this record does not state.
    ///
    /// # Errors
    ///
    /// [`LoadoutSchemaError`] when the subject is not a `loadout` id, when
    /// no gun is declared, when a gun or ammunition id is duplicated, or
    /// when a gun entry is not a `weapon` id.
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

// -------------------------------------------- the original loadout surface ----

/// The container member the original declares its gun and ammunition identity
/// vocabulary in.
///
/// **Measured, not authored.** The retail asset container
/// `GOSDATA/ASSETS/crimson.rof` ships this member and it is the engine's own
/// resource header, a C include the original build generated. It is named here
/// because a claim about the original's loadout has to say where it was read;
/// the retail acceptance tests re-read this member and re-measure every value
/// in [`OriginalGunLoadout`] against it.
pub const ORIGINAL_RESOURCE_HEADER: &str = "ASSETS/SCRIPTS/RESOURCE.H";

/// The first string id of the original's **gun-group** (hardpoint) names.
///
/// `3060` introduces the group table and names no group of its own, so the
/// groups run from `3061`.
pub const ORIGINAL_GUN_GROUP_NAMES_BASE_ID: u32 = 3061;

/// The last string id of the original's gun-group names.
pub const ORIGINAL_GUN_GROUP_NAMES_LAST_ID: u32 = 3080;

/// One gun group (hardpoint) the original declares — the declared counterpart
/// of `cs_sim::weapons::GunGroupName`.
///
/// `id` is the string identifier the engine resolves the group's name from and
/// `label` is the header macro that declares it. Both are original text.
///
/// The label says **which group the original names**, not where the group sits:
/// assigning an inner- or outer-wing group to a side needs the per-airframe gun
/// tables inside the executable, which no agent can read. So
/// [`DeclaredGunMountKind::covers_group`] covers only the groups the label
/// itself determines and [`ORIGINAL_GUN_GROUPS`] keeps every group addressable
/// whether it is covered or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeclaredGunGroup {
    id: u32,
    label: &'static str,
}

impl DeclaredGunGroup {
    /// A group: the string id it resolves from and the macro that declares it.
    #[must_use]
    pub const fn new(id: u32, label: &'static str) -> Self {
        Self { id, label }
    }

    /// The string id the engine resolves this group's name from.
    #[must_use]
    pub const fn id(self) -> u32 {
        self.id
    }

    /// The original's own name for the group, as its resource header spells it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        self.label
    }

    /// Looks a group up by its string id.
    #[must_use]
    pub fn by_id(id: u32) -> Option<Self> {
        ORIGINAL_GUN_GROUPS
            .iter()
            .copied()
            .find(|group| group.id == id)
    }

    /// Whether a [`DeclaredGunMountKind`] covers this group.
    #[must_use]
    pub fn is_covered(self) -> bool {
        DeclaredGunMountKind::ALL
            .iter()
            .any(|kind| kind.covers_group(self.id))
    }
}

impl fmt::Display for DeclaredGunGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.label, self.id)
    }
}

/// Every gun group the original declares, in ascending id order.
///
/// **Measured**: twenty contiguous ids in [`ORIGICAL_RESOURCE_HEADER`]. The
/// names the engine *shows* for these ids live in the runtime string catalog
/// and are readable from no file, so `label` is the header macro.
pub const ORIGINAL_GUN_GROUPS: [DeclaredGunGroup; 20] = [
    DeclaredGunGroup::new(3061, "INNERWINGGUNS"),
    DeclaredGunGroup::new(3062, "OUTERWINGGUNS"),
    DeclaredGunGroup::new(3063, "LOWERNOSEGUNS"),
    DeclaredGunGroup::new(3064, "UPPERNOSEGUNS"),
    DeclaredGunGroup::new(3065, "CENTERGUNS"),
    DeclaredGunGroup::new(3066, "RIGHTFUSELAGEGUNS"),
    DeclaredGunGroup::new(3067, "RIGHTWINGGUNS"),
    DeclaredGunGroup::new(3068, "LEFTWINGGUNS"),
    DeclaredGunGroup::new(3069, "OUTERWINGGUNS2"),
    DeclaredGunGroup::new(3070, "INNERWINGGUNS2"),
    DeclaredGunGroup::new(3071, "NOSEGUNS"),
    DeclaredGunGroup::new(3072, "NOSEGUNS2"),
    DeclaredGunGroup::new(3073, "REARTURRET"),
    DeclaredGunGroup::new(3074, "LOWINNERWINGGUNS"),
    DeclaredGunGroup::new(3075, "LOWOUTERWINGGUNS"),
    DeclaredGunGroup::new(3076, "UPPERINNERWINGGUNS"),
    DeclaredGunGroup::new(3077, "UPPEROUTERWINGGUNS"),
    DeclaredGunGroup::new(3078, "CENTERGUNS2"),
    DeclaredGunGroup::new(3079, "MIDDLEWINGGUNS"),
    DeclaredGunGroup::new(3080, "NOSETURRET"),
];

/// The string-id blocks the original allocates to its ammunition identity
/// vocabulary.
///
/// **Measured**: four blocks in [`ORIGINAL_RESOURCE_HEADER`], declared at
/// `3350`, `3360`, `3365` and `3370`. They are **not** equally wide: the next
/// block the header declares after them is `IDS_ROCKETLONGNAME 3380`, so the
/// gaps between the four bases are `10`, `5` and `5` ids. A block width read off
/// the bases would therefore give 10 or 5, not [`ORIGINAL_AMMUNITION_TYPES`] —
/// which is why the type count is measured from the ammunition screens instead
/// (see that constant).
pub const ORIGINAL_AMMO_NAME_BLOCKS: [(u32, &str); 4] = [
    (3350, "ammo_long_name"),
    (3360, "ammo_short_name"),
    (3365, "ammo_abbreviation"),
    (3370, "ammo_description"),
];

/// The number of gun ammunition types the original declares.
///
/// **Measured** from the ammunition screens, three independent ways, in
/// agreement: the multiplayer ammunition screen builds one dropdown row per
/// hardpoint ammunition entry behind a leading header row (five rows, four
/// types), it iterates `selection` over `1..=4` and indexes the description
/// block as `3370 + selection - 1`, so the descriptions occupy `3370..=3373`, and
/// the outlaw ammunition screen holds four selectable entries.
///
/// It is deliberately **not** read from the gaps between
/// [`ORIGINAL_AMMO_NAME_BLOCKS`], which are `10`, `5` and `5`: those measure the
/// header's block allocation, not the number of types.
pub const ORIGINAL_AMMUNITION_TYPES: u32 = 4;

/// The number of distinct guns the original lets one loadout choose from.
///
/// **Measured** from two independent places, both of which ask the engine for
/// gun *names* rather than infer a count: the multiplayer ammunition screen
/// declares `string UHA[5]` and fills it from the engine's gun-name callback,
/// and the outlaw gun screen declares `object ZAA[5]` with
/// `for(int R=0; R < 5; R++)`. The layout file's `GUNS` group is also five
/// entries wide, but that file does not say what the group holds, so it
/// corroborates rather than decides.
pub const ORIGINAL_SELECTABLE_GUNS: u32 = 5;

/// The number of gun slots one airframe's loadout offers.
///
/// **Measured**: the single-player ordinance layout builds four gun-name and
/// four ammunition dropdowns, and the multiplayer ammunition screen iterates
/// four gun slots.
pub const ORIGINAL_GUN_SLOTS: u32 = 4;

/// The number of rocket/ordnance slots one airframe's loadout offers.
///
/// **Measured**: the single-player ordinance layout builds eight rocket
/// dropdowns beside the same four gun slots.
pub const ORIGINAL_ROCKET_SLOTS: u32 = 8;

/// The number of hardpoint *points* the original's plane construction offers.
///
/// **Measured**: the hardpoint page builds two selectable points against the
/// four gun slots.
pub const ORIGINAL_HARDPOINT_POINTS: u32 = 2;

/// The gun groups no [`DeclaredGunMountKind`] covers, in
/// [`ORIGINAL_GUN_GROUPS`] order.
///
/// This is the measured size of the gap between the designed mount vocabulary
/// and the original's. A caller can state it without consulting any table, and
/// [`AmmunitionAudit`] reports every entry by name.
#[must_use]
pub fn uncovered_original_gun_groups() -> Vec<DeclaredGunGroup> {
    ORIGINAL_GUN_GROUPS
        .iter()
        .copied()
        .filter(|group| !group.is_covered())
        .collect()
}

/// The five counts a measured loadout surface carries.
///
/// Grouped so the surface's constructor stays small and so a zero count is
/// refused *here*, by name, before a surface that describes no loadout can be
/// built at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginalLoadoutCounts {
    /// How many gun ammunition types the installation declares.
    pub ammunition_types: u32,
    /// How many distinct guns one loadout may choose from.
    pub selectable_guns: u32,
    /// How many gun slots one airframe's loadout offers.
    pub gun_slots: u32,
    /// How many rocket/ordnance slots one airframe's loadout offers.
    pub rocket_slots: u32,
    /// How many hardpoint points plane construction offers.
    pub hardpoint_points: u32,
}

impl OriginalLoadoutCounts {
    /// The counts a surface was measured to hold, refusing a zero in any of
    /// them.
    ///
    /// # Errors
    ///
    /// [`OriginalLoadoutError::ZeroCount`] naming the field that was zero.
    pub fn try_new(
        ammunition_types: u32,
        selectable_guns: u32,
        gun_slots: u32,
        rocket_slots: u32,
        hardpoint_points: u32,
    ) -> Result<Self, OriginalLoadoutError> {
        for (field, value) in [
            ("ammunition_types", ammunition_types),
            ("selectable_guns", selectable_guns),
            ("gun_slots", gun_slots),
            ("rocket_slots", rocket_slots),
            ("hardpoint_points", hardpoint_points),
        ] {
            if value == 0 {
                return Err(OriginalLoadoutError::ZeroCount { field });
            }
        }
        Ok(Self {
            ammunition_types,
            selectable_guns,
            gun_slots,
            rocket_slots,
            hardpoint_points,
        })
    }
}

/// What an installation's gun/ammunition surface was **measured** to be.
///
/// This is the closure target of AC04's audit: the declared catalogue is only
/// complete if it enumerates at least as many ammunition types as the
/// installation declares and its mount kinds cover the groups the installation
/// names. Without it, "every type" is unfalsifiable — a catalogue holding one
/// type would satisfy any check there was.
///
/// # What is deliberately absent
///
/// The *names* of the ammunition types, their calibers, their damage amounts
/// and the convergence rule are **not** here. They live in the executable's own
/// tables, which no agent can read: the ammunition screens ask the engine for
/// the name array (`callback($$E$$,5054,…)`) rather than naming it. A record
/// that carried a name would be a fabrication, so the surface carries counts
/// and identifiers only, and the audit reports the rest as unknown.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalGunLoadout {
    origin: Origin,
    counts: OriginalLoadoutCounts,
    gun_groups: Vec<DeclaredGunGroup>,
    provenance: Provenance,
}

impl OriginalGunLoadout {
    /// Records a measured loadout surface.
    ///
    /// # Errors
    ///
    /// [`OriginalLoadoutError::NoGunGroups`] when the surface names no group,
    /// or [`OriginalLoadoutError::GunGroupOutOfRange`] /
    /// [`OriginalLoadoutError::DuplicateGunGroup`] when a group id is outside
    /// `[ORIGINAL_GUN_GROUP_NAMES_BASE_ID, ORIGINAL_GUN_GROUP_NAMES_LAST_ID]`
    /// or is listed twice. A surface that contradicts the original's own
    /// identifier range is a misread, and is refused rather than audited.
    pub fn try_new(
        origin: Origin,
        counts: OriginalLoadoutCounts,
        gun_groups: Vec<DeclaredGunGroup>,
        provenance: Provenance,
    ) -> Result<Self, OriginalLoadoutError> {
        if gun_groups.is_empty() {
            return Err(OriginalLoadoutError::NoGunGroups);
        }
        let mut seen = BTreeMap::new();
        for group in &gun_groups {
            if group.id < ORIGINAL_GUN_GROUP_NAMES_BASE_ID
                || group.id > ORIGINAL_GUN_GROUP_NAMES_LAST_ID
            {
                return Err(OriginalLoadoutError::GunGroupOutOfRange { group: *group });
            }
            if seen.insert(group.id, ()).is_some() {
                return Err(OriginalLoadoutError::DuplicateGunGroup { group: *group });
            }
        }
        Ok(Self {
            origin,
            counts,
            gun_groups,
            provenance,
        })
    }

    /// Where the surface was read from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Every measured count.
    #[must_use]
    pub const fn counts(&self) -> &OriginalLoadoutCounts {
        &self.counts
    }

    /// The measured number of gun ammunition types.
    #[must_use]
    pub const fn ammunition_types(&self) -> u32 {
        self.counts.ammunition_types
    }

    /// The measured number of selectable guns.
    #[must_use]
    pub const fn selectable_guns(&self) -> u32 {
        self.counts.selectable_guns
    }

    /// The measured number of gun slots per airframe.
    #[must_use]
    pub const fn gun_slots(&self) -> u32 {
        self.counts.gun_slots
    }

    /// The measured number of rocket slots per airframe.
    #[must_use]
    pub const fn rocket_slots(&self) -> u32 {
        self.counts.rocket_slots
    }

    /// The measured number of hardpoint points.
    #[must_use]
    pub const fn hardpoint_points(&self) -> u32 {
        self.counts.hardpoint_points
    }

    /// The measured gun groups, in declared order.
    #[must_use]
    pub fn gun_groups(&self) -> &[DeclaredGunGroup] {
        &self.gun_groups
    }

    /// The gun groups no declared mount kind covers.
    #[must_use]
    pub fn uncovered_gun_groups(&self) -> Vec<DeclaredGunGroup> {
        self.gun_groups
            .iter()
            .copied()
            .filter(|group| !group.is_covered())
            .collect()
    }

    /// Where this surface was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Why a measured loadout surface was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriginalLoadoutError {
    /// A required count was zero, so the surface describes no loadout at all.
    ZeroCount {
        /// The name of the count field.
        field: &'static str,
    },
    /// The surface names no gun group.
    NoGunGroups,
    /// A gun group id is outside the original's declared range.
    GunGroupOutOfRange {
        /// The offending group.
        group: DeclaredGunGroup,
    },
    /// The same gun group was listed twice.
    DuplicateGunGroup {
        /// The duplicated group.
        group: DeclaredGunGroup,
    },
}

impl fmt::Display for OriginalLoadoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCount { field } => {
                write!(f, "the measured loadout declares no {field}")
            }
            Self::NoGunGroups => write!(f, "the measured loadout names no gun group"),
            Self::GunGroupOutOfRange { group } => write!(
                f,
                "gun group {group} is outside the original's declared id range \
                 {ORIGINAL_GUN_GROUP_NAMES_BASE_ID}..={ORIGINAL_GUN_GROUP_NAMES_LAST_ID}"
            ),
            Self::DuplicateGunGroup { group } => write!(f, "gun group {group} is listed twice"),
        }
    }
}

impl std::error::Error for OriginalLoadoutError {}

// ------------------------------------------------------------- the audit ----

/// How one ammunition type behaves, and what a production path does with it.
///
/// The split is the point: [`applied`](Self::applied) names the options a
/// production path reads today, and [`deferred`](Self::deferred) names the ones
/// that are declared but read by nothing. A deferral is **not** a finding —
/// it is a standing property of the schema, and [`AmmunitionAuditReport::is_
/// complete`] would never be reachable if it were. What is a finding is an
/// option whose *value* is unknown (see
/// [`AmmoAuditFinding::UnmeasuredRule`]).
#[derive(Clone, Debug, PartialEq)]
pub struct AmmoBehavior {
    rules: InteractionRules,
    applied: Vec<InteractionOption>,
    deferred: Vec<(InteractionOption, &'static str, &'static str)>,
    unmeasured: Vec<InteractionOption>,
}

impl AmmoBehavior {
    /// The behavior of one declared ammunition type.
    #[must_use]
    pub fn of(rules: &InteractionRules) -> Self {
        Self {
            applied: InteractionOption::ALL
                .iter()
                .copied()
                .filter(|option| option.applied_by().is_some())
                .collect(),
            deferred: rules.deferred(),
            unmeasured: InteractionOption::ALL
                .iter()
                .copied()
                .filter(|option| !rules.is_known(*option))
                .collect(),
            rules: rules.clone(),
        }
    }

    /// The declared rules themselves.
    #[must_use]
    pub const fn rules(&self) -> &InteractionRules {
        &self.rules
    }

    /// The options a production path applies, in [`InteractionOption::ALL`] order.
    #[must_use]
    pub fn applied(&self) -> &[InteractionOption] {
        &self.applied
    }

    /// The options no production path applies, with the stage that owes them.
    #[must_use]
    pub fn deferred(&self) -> &[(InteractionOption, &'static str, &'static str)] {
        &self.deferred
    }

    /// The options whose value this record does not know.
    #[must_use]
    pub fn unmeasured(&self) -> &[InteractionOption] {
        &self.unmeasured
    }

    /// Whether every declared option has a known value, so the type could
    /// lower.
    #[must_use]
    pub fn is_measurable(&self) -> bool {
        self.unmeasured.is_empty()
    }
}

/// Who consumes one ammunition type's declared damage.
///
/// "Consumer" is declared here and resolved to a concrete production path by
/// [`DAMAGE_CONSUMED_BY_ROUTER`]: a type with **no** known amount on any channel
/// has no consumer, and this says so instead of naming a path that could never
/// receive anything.
#[derive(Clone, Debug, PartialEq)]
pub struct AmmoDamageConsumer {
    known: Vec<(DeclaredDamageChannel, f64)>,
    unmeasured: Vec<DeclaredDamageChannel>,
}

impl AmmoDamageConsumer {
    /// The consumer implied by one declared damage profile.
    #[must_use]
    pub fn of(damage: &DeclaredWeaponDamage) -> Self {
        let mut known = Vec::new();
        let mut unmeasured = Vec::new();
        for channel in DeclaredDamageChannel::ALL {
            match damage.channel(*channel) {
                Some(Resolved::Known(known_value)) => {
                    known.push((*channel, known_value.value));
                }
                _ => unmeasured.push(*channel),
            }
        }
        Self { known, unmeasured }
    }

    /// The channels with a known amount and the amounts themselves, in
    /// [`DeclaredDamageChannel::ALL`] order.
    #[must_use]
    pub fn known(&self) -> &[(DeclaredDamageChannel, f64)] {
        &self.known
    }

    /// The channels whose amount this record does not know.
    #[must_use]
    pub fn unmeasured(&self) -> &[DeclaredDamageChannel] {
        &self.unmeasured
    }

    /// The known amount on `channel`, if any.
    #[must_use]
    pub fn amount(&self, channel: DeclaredDamageChannel) -> Option<f64> {
        self.known
            .iter()
            .find(|(candidate, _)| *candidate == channel)
            .map(|(_, amount)| *amount)
    }

    /// Whether any declared amount reaches a consumer.
    ///
    /// A known amount of zero counts: the original's data can declare a channel
    /// it does not damage, and that is a *measurement*, unlike an unknown
    /// amount. Whether it delivers anything is the runtime registry's question.
    #[must_use]
    pub fn is_consumed(&self) -> bool {
        !self.known.is_empty()
    }

    /// The production path that applies the declared amounts, when one is
    /// reached.
    #[must_use]
    pub fn consumer(&self) -> Option<&'static str> {
        self.is_consumed().then_some(DAMAGE_CONSUMED_BY_ROUTER)
    }
}

/// The production path that consumes a declared ammunition type's damage:
/// `cs_sim::weapons::GunHitRouter::route`, which turns a swept contact into
/// `HitEvent`s on the declared channels for the damage resolver to apply.
pub const DAMAGE_CONSUMED_BY_ROUTER: &str = "cs_sim::weapons::GunHitRouter::route";

/// One ammunition type's audited row: what it is, how it behaves, who consumes
/// its damage, and which loadouts pair it with which guns.
#[derive(Clone, Debug, PartialEq)]
pub struct AmmoAuditRow {
    ammunition: AmmunitionId,
    caliber: Resolved<DeclaredCaliber>,
    behavior: AmmoBehavior,
    consumer: AmmoDamageConsumer,
    guns: Vec<ContentId>,
    loadouts: Vec<ContentId>,
    origin: Origin,
    provenance: Provenance,
}

impl AmmoAuditRow {
    /// The ammunition type this row audits.
    #[must_use]
    pub const fn ammunition(&self) -> &AmmunitionId {
        &self.ammunition
    }

    /// The declared caliber, known or explicitly unknown.
    #[must_use]
    pub const fn caliber(&self) -> &Resolved<DeclaredCaliber> {
        &self.caliber
    }

    /// The known caliber text, if it is resolved.
    #[must_use]
    pub fn known_caliber(&self) -> Option<&str> {
        match &self.caliber {
            Resolved::Known(known) => Some(known.value.as_str()),
            Resolved::Unknown { .. } => None,
        }
    }

    /// How this type behaves.
    #[must_use]
    pub const fn behavior(&self) -> &AmmoBehavior {
        &self.behavior
    }

    /// Who consumes this type's damage.
    #[must_use]
    pub const fn consumer(&self) -> &AmmoDamageConsumer {
        &self.consumer
    }

    /// The declared guns paired with this type, in ascending id order.
    #[must_use]
    pub fn guns(&self) -> &[ContentId] {
        &self.guns
    }

    /// The declared loadouts that carry this type, in ascending id order.
    #[must_use]
    pub fn loadouts(&self) -> &[ContentId] {
        &self.loadouts
    }

    /// Where the type's record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Where the type's record was measured.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// One gap the audit found, named.
#[derive(Clone, Debug, PartialEq)]
pub enum AmmoAuditFinding {
    /// The installation declares more ammunition types than the declared
    /// catalogue enumerates, so at least one type has no row at all.
    UndeclaredAmmunitionType {
        /// How many types the installation declares.
        observed: u32,
        /// How many the catalogue enumerates.
        declared: u32,
    },
    /// A gun group the installation names that no declared mount kind covers.
    UncoveredGunGroup {
        /// The group nothing covers.
        group: DeclaredGunGroup,
    },
    /// A declared mount kind a declared gun uses that corresponds to no gun
    /// group the installation names.
    UnobservedMountKind {
        /// The kind with no counterpart.
        kind: DeclaredGunMountKind,
    },
    /// The type declares no known damage amount on any channel, so no consumer
    /// receives anything from it.
    NoDamageConsumer {
        /// The type nothing consumes.
        ammunition: AmmunitionId,
    },
    /// The type declares an interaction option whose value is unknown, so no
    /// session could lower it.
    UnmeasuredRule {
        /// The type with the unknown rule.
        ammunition: AmmunitionId,
        /// The option that is unknown.
        option: InteractionOption,
    },
    /// The type's caliber is unknown.
    UnmeasuredCaliber {
        /// The type with the unknown caliber.
        ammunition: AmmunitionId,
    },
    /// The type is declared but paired with no gun in any loadout.
    Unpaired {
        /// The type no gun fires.
        ammunition: AmmunitionId,
    },
    /// A loadout names a gun that no declared gun record describes.
    UndescribedGun {
        /// The loadout that names it.
        loadout: ContentId,
        /// The gun nothing describes.
        gun: ContentId,
    },
    /// A loadout names an ammunition type that no declared record describes.
    UndescribedAmmunition {
        /// The loadout that names it.
        loadout: ContentId,
        /// The type nothing describes.
        ammunition: AmmunitionId,
    },
}

impl AmmoAuditFinding {
    /// The stable machine-readable label of this finding.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::UndeclaredAmmunitionType { .. } => "undeclared_ammunition_type",
            Self::UncoveredGunGroup { .. } => "uncovered_gun_group",
            Self::UnobservedMountKind { .. } => "unobserved_mount_kind",
            Self::NoDamageConsumer { .. } => "no_damage_consumer",
            Self::UnmeasuredRule { .. } => "unmeasured_rule",
            Self::UnmeasuredCaliber { .. } => "unmeasured_caliber",
            Self::Unpaired { .. } => "unpaired",
            Self::UndescribedGun { .. } => "undescribed_gun",
            Self::UndescribedAmmunition { .. } => "undescribed_ammunition",
        }
    }
}

impl fmt::Display for AmmoAuditFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredAmmunitionType { observed, declared } => write!(
                f,
                "the installation declares {observed} ammunition types but the catalogue \
                 enumerates only {declared}"
            ),
            Self::UncoveredGunGroup { group } => {
                write!(f, "gun group {group} is covered by no declared mount kind")
            }
            Self::UnobservedMountKind { kind } => {
                write!(
                    f,
                    "mount kind {kind} corresponds to no gun group the installation names"
                )
            }
            Self::NoDamageConsumer { ammunition } => {
                write!(
                    f,
                    "{ammunition} declares no damage amount, so nothing consumes it"
                )
            }
            Self::UnmeasuredRule { ammunition, option } => {
                write!(f, "{ammunition} leaves its {option} rule unmeasured")
            }
            Self::UnmeasuredCaliber { ammunition } => {
                write!(f, "{ammunition} leaves its caliber unmeasured")
            }
            Self::Unpaired { ammunition } => {
                write!(
                    f,
                    "{ammunition} is declared but paired with no gun in any loadout"
                )
            }
            Self::UndescribedGun { loadout, gun } => {
                write!(
                    f,
                    "loadout {loadout} names gun {gun}, which no declared record describes"
                )
            }
            Self::UndescribedAmmunition {
                loadout,
                ammunition,
            } => write!(
                f,
                "loadout {loadout} names ammunition {ammunition}, which no declared record describes"
            ),
        }
    }
}

/// The result of an ammunition/loadout audit.
///
/// `complete` is the only verdict, and it is deliberately hard to reach: it
/// holds when the declared catalogue covers every ammunition type the
/// installation declares, its mount kinds cover every gun group the
/// installation names, and every type has a measured caliber, a measurable
/// interaction rule and a damage consumer. Anything else is a named finding, so
/// a partial audit reports itself as partial.
#[derive(Clone, Debug, PartialEq)]
pub struct AmmunitionAuditReport {
    rows: Vec<AmmoAuditRow>,
    findings: Vec<AmmoAuditFinding>,
}

impl AmmunitionAuditReport {
    /// One row per declared ammunition type, in ascending id order.
    #[must_use]
    pub fn rows(&self) -> &[AmmoAuditRow] {
        &self.rows
    }

    /// The row for one type, if it is declared.
    #[must_use]
    pub fn row(&self, ammunition: &AmmunitionId) -> Option<&AmmoAuditRow> {
        self.rows.iter().find(|row| row.ammunition == *ammunition)
    }

    /// Every gap found, in report order.
    #[must_use]
    pub fn findings(&self) -> &[AmmoAuditFinding] {
        &self.findings
    }

    /// The findings of one label, so a caller can name one gap at a time.
    #[must_use]
    pub fn findings_of(&self, label: &str) -> Vec<&AmmoAuditFinding> {
        self.findings
            .iter()
            .filter(|finding| finding.label() == label)
            .collect()
    }

    /// How many ammunition types the catalogue enumerates.
    #[must_use]
    pub fn declared_types(&self) -> usize {
        self.rows.len()
    }

    /// How many types reached a damage consumer.
    #[must_use]
    pub fn consumed_types(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| row.consumer.is_consumed())
            .count()
    }

    /// Whether the audit found no gap at all.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.findings.is_empty()
    }
}

/// The declared ammunition/loadout audit (AC04).
///
/// The audit walks the declared ammunition records, the declared guns and the
/// declared loadouts of one installation and answers, per ammunition type: what
/// it is, how it behaves, which guns fire it, which loadouts carry it and who
/// consumes its damage. It compares the result against a measured
/// [`OriginalGunLoadout`], because "every type" is only falsifiable against a
/// number the installation itself declares.
///
/// # Why it reports rather than repairs
///
/// Nothing here fills a gap. An unnamed ammunition type, an unmeasured damage
/// amount and an uncovered gun group all stay gaps and are named, because the
/// alternative — inventing a fourth ammunition type or assigning an inner-wing
/// group to a side — is precisely the guess F27 non-negotiable 1 and 4 forbid.
/// An audit that always passed would be worse than none: it would let a
/// one-type catalogue stand in for the original's four.
#[derive(Clone, Debug, Default)]
pub struct AmmunitionAudit {
    ammunition: Vec<DeclaredAmmunition>,
    guns: Vec<DeclaredGunDefinition>,
    loadouts: Vec<DeclaredLoadout>,
}

impl AmmunitionAudit {
    /// An empty audit: no declared records, so nothing to cover.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one declared ammunition record.
    pub fn add_ammunition(&mut self, record: DeclaredAmmunition) -> &mut Self {
        self.ammunition.push(record);
        self
    }

    /// Adds one declared gun record.
    pub fn add_gun(&mut self, record: DeclaredGunDefinition) -> &mut Self {
        self.guns.push(record);
        self
    }

    /// Adds one declared loadout record.
    pub fn add_loadout(&mut self, record: DeclaredLoadout) -> &mut Self {
        self.loadouts.push(record);
        self
    }

    /// How many declared ammunition records the audit holds.
    #[must_use]
    pub fn ammunition_count(&self) -> usize {
        self.ammunition.len()
    }

    /// How many declared gun records the audit holds.
    #[must_use]
    pub fn gun_count(&self) -> usize {
        self.guns.len()
    }

    /// How many declared loadout records the audit holds.
    #[must_use]
    pub fn loadout_count(&self) -> usize {
        self.loadouts.len()
    }

    /// Runs the audit against a measured installation surface.
    #[must_use]
    pub fn run(&self, original: &OriginalGunLoadout) -> AmmunitionAuditReport {
        let mut rows = Vec::new();
        let mut findings = Vec::new();

        // One row per distinct type, in ascending id order, so two records for
        // the same type cannot inflate the type count past the closure check.
        let mut types: BTreeMap<String, &DeclaredAmmunition> = BTreeMap::new();
        for record in &self.ammunition {
            types
                .entry(record.ammunition().as_str().to_owned())
                .or_insert(record);
        }

        // The pairing walk: which loadout carries which (gun, type) row.
        let mut guns_by_type: BTreeMap<String, BTreeSet<ContentId>> = BTreeMap::new();
        let mut loadouts_by_type: BTreeMap<String, BTreeSet<ContentId>> = BTreeMap::new();
        let mut described_guns: BTreeMap<ContentId, &DeclaredGunDefinition> = BTreeMap::new();
        for gun in &self.guns {
            described_guns.insert(gun.gun().clone(), gun);
        }
        for loadout in &self.loadouts {
            // Each dangling reference is reported once, not once per pairing:
            // `DeclaredLoadout::pairings` crosses every gun with every type, so
            // walking it for these checks would report one undeclared type
            // twice — or `guns * ammunition` times — and a caller counting
            // findings could not tell a real second gap from an echo.
            for gun in loadout.guns() {
                if !described_guns.contains_key(gun) {
                    findings.push(AmmoAuditFinding::UndescribedGun {
                        loadout: loadout.subject().clone(),
                        gun: gun.clone(),
                    });
                }
            }
            for ammunition in loadout.ammunition() {
                if !types.contains_key(ammunition.as_str()) {
                    findings.push(AmmoAuditFinding::UndescribedAmmunition {
                        loadout: loadout.subject().clone(),
                        ammunition: ammunition.clone(),
                    });
                }
            }
            for (gun, ammunition) in loadout.pairings() {
                if !types.contains_key(ammunition.as_str()) {
                    continue;
                }
                guns_by_type
                    .entry(ammunition.as_str().to_owned())
                    .or_default()
                    .insert(gun.clone());
                loadouts_by_type
                    .entry(ammunition.as_str().to_owned())
                    .or_default()
                    .insert(loadout.subject().clone());
            }
        }

        for (key, record) in &types {
            let ammunition = record.ammunition();
            let guns = guns_by_type.get(key).cloned().unwrap_or_default();
            let loadouts = loadouts_by_type.get(key).cloned().unwrap_or_default();
            let behavior = AmmoBehavior::of(record.rules());
            let consumer = AmmoDamageConsumer::of(record.damage());

            if record.known_caliber().is_none() {
                findings.push(AmmoAuditFinding::UnmeasuredCaliber {
                    ammunition: ammunition.clone(),
                });
            }
            for option in behavior.unmeasured() {
                findings.push(AmmoAuditFinding::UnmeasuredRule {
                    ammunition: ammunition.clone(),
                    option: *option,
                });
            }
            if !consumer.is_consumed() {
                findings.push(AmmoAuditFinding::NoDamageConsumer {
                    ammunition: ammunition.clone(),
                });
            }
            if guns.is_empty() {
                findings.push(AmmoAuditFinding::Unpaired {
                    ammunition: ammunition.clone(),
                });
            }

            rows.push(AmmoAuditRow {
                ammunition: ammunition.clone(),
                caliber: record.caliber().clone(),
                behavior,
                consumer,
                guns: guns.into_iter().collect(),
                loadouts: loadouts.into_iter().collect(),
                origin: record.origin().clone(),
                provenance: record.provenance().clone(),
            });
        }

        // The closure check against the installation: how many types it
        // declares, and how many the catalogue enumerates.
        if types.len() < original.ammunition_types() as usize {
            findings.push(AmmoAuditFinding::UndeclaredAmmunitionType {
                observed: original.ammunition_types(),
                declared: types.len() as u32,
            });
        }

        // The mount side, both directions: every group the installation names
        // that no declared kind covers, and every declared kind *in use* that
        // covers none of the groups this installation names. The second test is
        // about the surface, not about the kind in the abstract: `WingLeft`
        // covers a real measured group, so on a surface that names only a nose
        // group it is the wing kind that is unobserved here.
        for group in original.uncovered_gun_groups() {
            findings.push(AmmoAuditFinding::UncoveredGunGroup { group });
        }
        let surface_groups: BTreeSet<u32> = original
            .gun_groups()
            .iter()
            .map(|group| group.id())
            .collect();
        let mut kinds_in_use: BTreeSet<DeclaredGunMountKind> = BTreeSet::new();
        for gun in &self.guns {
            kinds_in_use.insert(gun.mount_kind());
        }
        for kind in DeclaredGunMountKind::ALL {
            if !kinds_in_use.contains(kind) {
                continue;
            }
            let covered = kind
                .original_groups()
                .iter()
                .any(|group| surface_groups.contains(&group.id()));
            if !covered {
                findings.push(AmmoAuditFinding::UnobservedMountKind { kind: *kind });
            }
        }

        AmmunitionAuditReport { rows, findings }
    }
}

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
