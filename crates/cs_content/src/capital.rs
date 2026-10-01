//! The declared capital-ship schema: provenance-carrying ship, subsystem
//! and bay records (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! This module is the **content half** of the capital-ship contract — the
//! normalized record a content importer produces and the catalog consumes.
//! Its runtime counterpart is `cs_sim::capital`; the lowering boundary
//! between them is `cs_app::capital`. The split mirrors `damage` ↔
//! `cs_sim::damage`: this crate cannot depend on `cs_sim`, so the declared
//! record keeps its own typed vocabulary — subsystem kinds and effects,
//! bay exposure windows — and the boundary maps it field-wise.
//!
//! # Records
//!
//! A [`DeclaredCapitalShip`] is one catalog subject ([`ContentId`]) with an
//! [`Origin`], a [`Provenance`], and the parts the F35 deliverable names:
//! an optional authored trajectory, engines, gas/structural sections,
//! turrets, weapon bays, launch bays with their socket transforms, docking
//! anchors, cargo and initial ownership. Identity is the subsystem key,
//! never position. The shared [`DeclaredSubsystem`] list carries each
//! part's [`CapitalSubsystemKind`], its [`CapitalSubsystemEffect`] and
//! whether destroying it is lethal; the typed detail collections carry the
//! per-kind data.
//!
//! Every load-bearing value is a [`Resolved`], so an unmeasured thrust,
//! integrity, socket, capacity or owner stays an explicit unknown with its
//! claim id and reason instead of a silent default (F14 non-negotiable
//! behavior 3). Nothing here is measured original data.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{ContentId, Origin, Provenance, Resolved};

/// Maximum byte length of a [`CapitalSubsystemKey`].
pub const MAX_SUBSYSTEM_KEY_LEN: usize = 128;

/// Why a [`CapitalSubsystemKey`] was rejected. Same grammar as
/// `cs_sim::capital::SubsystemKey`: the two crates apply the one identity
/// discipline independently because `cs_types` does not yet own a shared
/// key type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapitalSubsystemKeyError {
    /// The key was empty.
    Empty,
    /// The key exceeded [`MAX_SUBSYSTEM_KEY_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The key contained a character outside `[a-z0-9._-]` after lowercasing.
    BadCharacter {
        /// The offending character.
        ch: char,
    },
    /// The key had no ASCII alphanumeric character.
    NoAlphanumeric,
}

impl fmt::Display for CapitalSubsystemKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a subsystem key must not be empty"),
            Self::TooLong { len } => write!(
                f,
                "a subsystem key is {len} bytes, max is {MAX_SUBSYSTEM_KEY_LEN}"
            ),
            Self::BadCharacter { ch } => {
                write!(f, "a subsystem key contains disallowed character {ch:?}")
            }
            Self::NoAlphanumeric => write!(f, "a subsystem key must contain an ASCII alphanumeric"),
        }
    }
}

impl std::error::Error for CapitalSubsystemKeyError {}

/// The declared identity of one subsystem.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapitalSubsystemKey(String);

impl CapitalSubsystemKey {
    /// Validates and wraps a subsystem key.
    ///
    /// # Errors
    ///
    /// [`CapitalSubsystemKeyError`] when the key is empty, too long, carries
    /// a character outside `[a-z0-9._-]` or has no alphanumeric.
    pub fn new(key: &str) -> Result<Self, CapitalSubsystemKeyError> {
        let key = key.to_ascii_lowercase();
        if key.is_empty() {
            return Err(CapitalSubsystemKeyError::Empty);
        }
        if key.len() > MAX_SUBSYSTEM_KEY_LEN {
            return Err(CapitalSubsystemKeyError::TooLong { len: key.len() });
        }
        for ch in key.chars() {
            if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && !matches!(ch, '.' | '_' | '-') {
                return Err(CapitalSubsystemKeyError::BadCharacter { ch });
            }
        }
        if !key.bytes().any(|byte| byte.is_ascii_alphanumeric()) {
            return Err(CapitalSubsystemKeyError::NoAlphanumeric);
        }
        Ok(Self(key))
    }

    /// The normalized key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CapitalSubsystemKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What kind of part a declared subsystem models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapitalSubsystemKind {
    /// A propulsion engine.
    Engine,
    /// A broadside or weapon bay.
    WeaponBay,
    /// A hangar / launch bay.
    LaunchBay,
    /// A turret mount.
    Turret,
    /// A docking anchor.
    DockingAnchor,
    /// A lifting-gas cell.
    GasCell,
    /// Internal structure or a load-bearing section.
    StructuralSection,
}

impl CapitalSubsystemKind {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::WeaponBay => "weapon_bay",
            Self::LaunchBay => "launch_bay",
            Self::Turret => "turret",
            Self::DockingAnchor => "docking_anchor",
            Self::GasCell => "gas_cell",
            Self::StructuralSection => "structural_section",
        }
    }

    /// Whether a part of this kind may carry `effect`.
    #[must_use]
    pub const fn allows_effect(self, effect: CapitalSubsystemEffect) -> bool {
        matches!(
            (self, effect),
            (Self::Engine, CapitalSubsystemEffect::Propulsion)
                | (
                    Self::WeaponBay | Self::Turret,
                    CapitalSubsystemEffect::WeaponAccess
                )
                | (Self::LaunchBay, CapitalSubsystemEffect::Launching)
                | (Self::DockingAnchor, CapitalSubsystemEffect::Docking)
                | (Self::GasCell, CapitalSubsystemEffect::Vulnerability)
                | (
                    Self::StructuralSection,
                    CapitalSubsystemEffect::MissionCondition
                )
        )
    }

    /// Whether destroying a part of this kind may destroy the whole actor.
    #[must_use]
    pub const fn can_be_lethal(self) -> bool {
        matches!(self, Self::GasCell | Self::StructuralSection)
    }
}

impl fmt::Display for CapitalSubsystemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A gameplay behaviour a destroyed subsystem changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapitalSubsystemEffect {
    /// The actor's propulsion changes.
    Propulsion,
    /// Weapon access changes.
    WeaponAccess,
    /// Aircraft spawning changes.
    Launching,
    /// Docking eligibility changes.
    Docking,
    /// The actor's vulnerability changes.
    Vulnerability,
    /// A mission condition changes.
    MissionCondition,
}

impl fmt::Display for CapitalSubsystemEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

impl CapitalSubsystemEffect {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Propulsion => "propulsion",
            Self::WeaponAccess => "weapon_access",
            Self::Launching => "launching",
            Self::Docking => "docking",
            Self::Vulnerability => "vulnerability",
            Self::MissionCondition => "mission_condition",
        }
    }
}

/// The shared identity of one declared subsystem.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredSubsystem {
    /// The subsystem's stable key.
    pub key: CapitalSubsystemKey,
    /// The subsystem's kind.
    pub kind: CapitalSubsystemKind,
    /// The behavior its destruction changes, if any.
    pub effect: Option<CapitalSubsystemEffect>,
    /// Whether destroying it destroys the whole actor.
    pub lethal: bool,
}

/// A declared engine.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredEngine {
    /// The subsystem key this engine is.
    pub key: CapitalSubsystemKey,
    /// The rated thrust, or an explicit unknown.
    pub thrust_n: Resolved<f64>,
    /// The unit thrust axis in the ship body frame.
    pub axis: [f64; 3],
}

/// A declared exposure window, in simulation ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredExposure {
    /// Ticks fully closed.
    pub concealed_ticks: u64,
    /// Ticks with the doors moving open.
    pub opening_ticks: u64,
    /// Ticks fully open (hittable weakpoint).
    pub exposed_ticks: u64,
    /// Ticks with the doors moving shut.
    pub closing_ticks: u64,
}

/// Why a declared [`DeclaredExposure`] was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureSchemaError {
    /// A window whose open phase is empty never exposes a weakpoint.
    NoExposedTicks,
    /// The sum of all phases was zero.
    ZeroCycle,
    /// The four phases summed past `u64::MAX`; the window cannot be lowered
    /// to a runtime exposure window without wrapping.
    CycleOverflow,
}

impl DeclaredExposure {
    /// Validates the cycle.
    ///
    /// # Errors
    ///
    /// [`ExposureSchemaError::NoExposedTicks`],
    /// [`ExposureSchemaError::ZeroCycle`] or
    /// [`ExposureSchemaError::CycleOverflow`].
    pub fn validate(&self) -> Result<(), ExposureSchemaError> {
        let mut total = 0_u64;
        for part in [
            self.concealed_ticks,
            self.opening_ticks,
            self.exposed_ticks,
            self.closing_ticks,
        ] {
            total = total
                .checked_add(part)
                .ok_or(ExposureSchemaError::CycleOverflow)?;
        }
        if total == 0 {
            return Err(ExposureSchemaError::ZeroCycle);
        }
        if self.exposed_ticks == 0 {
            return Err(ExposureSchemaError::NoExposedTicks);
        }
        Ok(())
    }
}

/// A declared weapon bay.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredWeaponBay {
    /// The subsystem key this bay is.
    pub key: CapitalSubsystemKey,
    /// The bay's open/close cycle.
    pub exposure: DeclaredExposure,
}

/// A declared launch bay with its socket and capacity.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredLaunchBay {
    /// The subsystem key this bay is.
    pub key: CapitalSubsystemKey,
    /// The bay's open/close cycle.
    pub exposure: DeclaredExposure,
    /// The release socket offset in the ship body frame, or an unknown.
    pub socket_offset_m: Resolved<[f64; 3]>,
    /// The bay's aircraft capacity, or an unknown.
    pub capacity: Resolved<u32>,
}

/// A declared turret mount.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredTurret {
    /// The subsystem key this turret is.
    pub key: CapitalSubsystemKey,
    /// The weapon the turret mounts, or an unknown.
    pub weapon: Resolved<ContentId>,
    /// The traverse arc in degrees, or an unknown.
    pub traverse_deg: Resolved<f64>,
}

/// A declared docking anchor.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredDockingAnchor {
    /// The subsystem key this anchor is.
    pub key: CapitalSubsystemKey,
    /// The anchor offset in the ship body frame, or an unknown.
    pub offset_m: Resolved<[f64; 3]>,
}

/// A declared gas or structural section.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredSection {
    /// The subsystem key this section is.
    pub key: CapitalSubsystemKey,
    /// The section's integrity pool, or an unknown.
    pub integrity: Resolved<f64>,
}

/// A declared trajectory keyframe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeclaredKeyframe {
    /// The integer tick of the key.
    pub tick: u64,
    /// The position at the key.
    pub position_m: [f64; 3],
    /// The orientation as `[x, y, z, w]`.
    pub orientation: [f64; 4],
}

/// A declared authored trajectory.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredTrajectory {
    /// Simulation ticks per second.
    pub ticks_per_second: u32,
    /// The keyframes, strictly ascending in tick.
    pub keyframes: Vec<DeclaredKeyframe>,
}

/// Why a [`DeclaredCapitalShip`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CapitalSchemaError {
    /// A ship with no subsystems models nothing.
    EmptyShip,
    /// Two entries share one subsystem key.
    DuplicateKey {
        /// The duplicated key.
        key: CapitalSubsystemKey,
    },
    /// A detail record names no declared subsystem.
    UnknownSubsystem {
        /// The dangling key.
        key: CapitalSubsystemKey,
    },
    /// A detail record's kind does not match its subsystem's declared kind.
    KindMismatch {
        /// The offending subsystem.
        key: CapitalSubsystemKey,
        /// The kind the detail collection requires.
        expected: CapitalSubsystemKind,
        /// The kind the subsystem declares.
        actual: CapitalSubsystemKind,
    },
    /// A subsystem declares an effect its kind does not allow.
    EffectKindMismatch {
        /// The offending subsystem.
        key: CapitalSubsystemKey,
        /// Its kind.
        kind: CapitalSubsystemKind,
        /// The refused effect.
        effect: CapitalSubsystemEffect,
    },
    /// A subsystem is lethal although its kind can never destroy the actor.
    LethalKind {
        /// The offending subsystem.
        key: CapitalSubsystemKey,
        /// Its kind.
        kind: CapitalSubsystemKind,
    },
    /// A known value was not finite.
    NonFinite {
        /// The subsystem or field the value belongs to.
        key: String,
        /// The field name.
        field: &'static str,
    },
    /// A known value was negative.
    Negative {
        /// The subsystem or field the value belongs to.
        key: String,
        /// The field name.
        field: &'static str,
        /// The refused value.
        value: f64,
    },
    /// A known engine axis was the zero vector.
    ZeroAxis {
        /// The offending subsystem.
        key: CapitalSubsystemKey,
    },
    /// A known capacity was zero.
    ZeroCapacity {
        /// The offending subsystem.
        key: CapitalSubsystemKey,
    },
    /// A declared exposure window is invalid.
    Exposure {
        /// The offending subsystem.
        key: CapitalSubsystemKey,
        /// The refusal.
        source: ExposureSchemaError,
    },
}

impl fmt::Display for CapitalSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyShip => write!(f, "a capital ship must declare at least one subsystem"),
            Self::DuplicateKey { key } => {
                write!(f, "subsystem key {key:?} is used more than once")
            }
            Self::UnknownSubsystem { key } => {
                write!(f, "subsystem {key} has no shared declaration")
            }
            Self::KindMismatch {
                key,
                expected,
                actual,
            } => write!(f, "subsystem {key} is {actual}, expected {expected}"),
            Self::EffectKindMismatch { key, kind, effect } => write!(
                f,
                "subsystem {key} of kind {kind} cannot apply the {effect} effect"
            ),
            Self::LethalKind { key, kind } => {
                write!(f, "subsystem {key} of kind {kind} cannot be lethal")
            }
            Self::NonFinite { key, field } => {
                write!(f, "{key} has a non-finite {field}")
            }
            Self::Negative { key, field, value } => {
                write!(f, "{key} has a negative {field} ({value})")
            }
            Self::ZeroAxis { key } => write!(f, "engine {key} has a zero axis"),
            Self::ZeroCapacity { key } => write!(f, "launch bay {key} has zero capacity"),
            Self::Exposure { key, source } => {
                write!(
                    f,
                    "subsystem {key} has an invalid exposure window: {source:?}"
                )
            }
        }
    }
}

impl std::error::Error for CapitalSchemaError {}

/// The declared capital-ship model of one catalog subject.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCapitalShip {
    subject: ContentId,
    origin: Origin,
    provenance: Provenance,
    trajectory: Option<DeclaredTrajectory>,
    subsystems: Vec<DeclaredSubsystem>,
    engines: Vec<DeclaredEngine>,
    weapon_bays: Vec<DeclaredWeaponBay>,
    launch_bays: Vec<DeclaredLaunchBay>,
    turrets: Vec<DeclaredTurret>,
    docking_anchors: Vec<DeclaredDockingAnchor>,
    sections: Vec<DeclaredSection>,
    cargo: Resolved<f64>,
    ownership: Resolved<ContentId>,
}

/// The parts of a [`DeclaredCapitalShip`] the constructor receives.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCapitalParts {
    /// The shared subsystem identities.
    pub subsystems: Vec<DeclaredSubsystem>,
    /// The engines.
    pub engines: Vec<DeclaredEngine>,
    /// The weapon bays.
    pub weapon_bays: Vec<DeclaredWeaponBay>,
    /// The launch bays.
    pub launch_bays: Vec<DeclaredLaunchBay>,
    /// The turrets.
    pub turrets: Vec<DeclaredTurret>,
    /// The docking anchors.
    pub docking_anchors: Vec<DeclaredDockingAnchor>,
    /// The gas and structural sections.
    pub sections: Vec<DeclaredSection>,
}

impl DeclaredCapitalShip {
    /// Assembles and validates a declared capital ship.
    ///
    /// # Errors
    ///
    /// [`CapitalSchemaError`] on an empty or duplicated subsystem set, a
    /// detail record with no matching subsystem or the wrong kind, a
    /// disallowed effect or lethal kind, a corrupt known value, a zero axis
    /// or capacity, or an invalid exposure window.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        provenance: Provenance,
        trajectory: Option<DeclaredTrajectory>,
        parts: DeclaredCapitalParts,
        cargo: Resolved<f64>,
        ownership: Resolved<ContentId>,
    ) -> Result<Self, CapitalSchemaError> {
        validate(&parts)?;
        validate_optional(&cargo, "capital.cargo", "cargo")?;
        Ok(Self {
            subject,
            origin,
            provenance,
            trajectory,
            subsystems: parts.subsystems,
            engines: parts.engines,
            weapon_bays: parts.weapon_bays,
            launch_bays: parts.launch_bays,
            turrets: parts.turrets,
            docking_anchors: parts.docking_anchors,
            sections: parts.sections,
            cargo,
            ownership,
        })
    }

    /// The catalog id the ship describes.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The authored trajectory, when the ship has one.
    #[must_use]
    pub const fn trajectory(&self) -> Option<&DeclaredTrajectory> {
        self.trajectory.as_ref()
    }

    /// The shared subsystem identities, in authored order.
    #[must_use]
    pub fn subsystems(&self) -> &[DeclaredSubsystem] {
        &self.subsystems
    }

    /// A shared subsystem identity by key.
    #[must_use]
    pub fn subsystem(&self, key: &CapitalSubsystemKey) -> Option<&DeclaredSubsystem> {
        self.subsystems
            .iter()
            .find(|subsystem| &subsystem.key == key)
    }

    /// The engines, in authored order.
    #[must_use]
    pub fn engines(&self) -> &[DeclaredEngine] {
        &self.engines
    }

    /// The weapon bays, in authored order.
    #[must_use]
    pub fn weapon_bays(&self) -> &[DeclaredWeaponBay] {
        &self.weapon_bays
    }

    /// The launch bays, in authored order.
    #[must_use]
    pub fn launch_bays(&self) -> &[DeclaredLaunchBay] {
        &self.launch_bays
    }

    /// The turrets, in authored order.
    #[must_use]
    pub fn turrets(&self) -> &[DeclaredTurret] {
        &self.turrets
    }

    /// The docking anchors, in authored order.
    #[must_use]
    pub fn docking_anchors(&self) -> &[DeclaredDockingAnchor] {
        &self.docking_anchors
    }

    /// The gas and structural sections, in authored order.
    #[must_use]
    pub fn sections(&self) -> &[DeclaredSection] {
        &self.sections
    }

    /// The cargo capacity, or an explicit unknown.
    #[must_use]
    pub const fn cargo(&self) -> &Resolved<f64> {
        &self.cargo
    }

    /// The initial owner, or an explicit unknown.
    #[must_use]
    pub const fn ownership(&self) -> &Resolved<ContentId> {
        &self.ownership
    }
}

fn validate_optional(
    value: &Resolved<f64>,
    key: &str,
    field: &'static str,
) -> Result<(), CapitalSchemaError> {
    if let Resolved::Known(known) = value {
        if !known.value.is_finite() {
            return Err(CapitalSchemaError::NonFinite {
                key: key.to_owned(),
                field,
            });
        }
        if known.value < 0.0 {
            return Err(CapitalSchemaError::Negative {
                key: key.to_owned(),
                field,
                value: known.value,
            });
        }
    }
    Ok(())
}

/// Validates the shared subsystem list plus every typed detail collection.
///
/// # Errors
///
/// [`CapitalSchemaError`] describing the first refusal.
pub fn validate(parts: &DeclaredCapitalParts) -> Result<(), CapitalSchemaError> {
    if parts.subsystems.is_empty() {
        return Err(CapitalSchemaError::EmptyShip);
    }
    let mut by_key: BTreeMap<&CapitalSubsystemKey, &DeclaredSubsystem> = BTreeMap::new();
    for subsystem in &parts.subsystems {
        if by_key.insert(&subsystem.key, subsystem).is_some() {
            return Err(CapitalSchemaError::DuplicateKey {
                key: subsystem.key.clone(),
            });
        }
        if let Some(effect) = subsystem.effect
            && !subsystem.kind.allows_effect(effect)
        {
            return Err(CapitalSchemaError::EffectKindMismatch {
                key: subsystem.key.clone(),
                kind: subsystem.kind,
                effect,
            });
        }
        if subsystem.lethal && !subsystem.kind.can_be_lethal() {
            return Err(CapitalSchemaError::LethalKind {
                key: subsystem.key.clone(),
                kind: subsystem.kind,
            });
        }
    }

    let mut seen: BTreeMap<CapitalSubsystemKey, CapitalSubsystemKind> = BTreeMap::new();
    let mut claim =
        |key: &CapitalSubsystemKey, kind: CapitalSubsystemKind| -> Result<(), CapitalSchemaError> {
            if !by_key.contains_key(key) {
                return Err(CapitalSchemaError::UnknownSubsystem { key: key.clone() });
            }
            if seen.insert(key.clone(), kind).is_some() {
                return Err(CapitalSchemaError::DuplicateKey { key: key.clone() });
            }
            Ok(())
        };
    let kind_of = |key: &CapitalSubsystemKey| by_key.get(key).map(|s| s.kind);

    for engine in &parts.engines {
        claim(&engine.key, CapitalSubsystemKind::Engine)?;
        if kind_of(&engine.key) != Some(CapitalSubsystemKind::Engine) {
            return Err(CapitalSchemaError::KindMismatch {
                key: engine.key.clone(),
                expected: CapitalSubsystemKind::Engine,
                actual: kind_of(&engine.key).expect("key is present"),
            });
        }
        if !engine.axis.iter().all(|value| value.is_finite()) {
            return Err(CapitalSchemaError::NonFinite {
                key: format!("engine {}", engine.key),
                field: "axis",
            });
        }
        let norm = (engine.axis[0] * engine.axis[0]
            + engine.axis[1] * engine.axis[1]
            + engine.axis[2] * engine.axis[2])
            .sqrt();
        if norm <= f64::EPSILON {
            return Err(CapitalSchemaError::ZeroAxis {
                key: engine.key.clone(),
            });
        }
        validate_optional(
            &engine.thrust_n,
            &format!("engine {}", engine.key),
            "thrust_n",
        )?;
    }

    for bay in &parts.weapon_bays {
        claim(&bay.key, CapitalSubsystemKind::WeaponBay)?;
        if kind_of(&bay.key) != Some(CapitalSubsystemKind::WeaponBay) {
            return Err(CapitalSchemaError::KindMismatch {
                key: bay.key.clone(),
                expected: CapitalSubsystemKind::WeaponBay,
                actual: kind_of(&bay.key).expect("key is present"),
            });
        }
        bay.exposure
            .validate()
            .map_err(|source| CapitalSchemaError::Exposure {
                key: bay.key.clone(),
                source,
            })?;
    }

    for bay in &parts.launch_bays {
        claim(&bay.key, CapitalSubsystemKind::LaunchBay)?;
        if kind_of(&bay.key) != Some(CapitalSubsystemKind::LaunchBay) {
            return Err(CapitalSchemaError::KindMismatch {
                key: bay.key.clone(),
                expected: CapitalSubsystemKind::LaunchBay,
                actual: kind_of(&bay.key).expect("key is present"),
            });
        }
        bay.exposure
            .validate()
            .map_err(|source| CapitalSchemaError::Exposure {
                key: bay.key.clone(),
                source,
            })?;
        if let Resolved::Known(known) = &bay.socket_offset_m
            && !known.value.iter().all(|value| value.is_finite())
        {
            return Err(CapitalSchemaError::NonFinite {
                key: format!("launch bay {}", bay.key),
                field: "socket_offset_m",
            });
        }
        if let Resolved::Known(known) = &bay.capacity
            && known.value == 0
        {
            return Err(CapitalSchemaError::ZeroCapacity {
                key: bay.key.clone(),
            });
        }
    }

    for turret in &parts.turrets {
        claim(&turret.key, CapitalSubsystemKind::Turret)?;
        if kind_of(&turret.key) != Some(CapitalSubsystemKind::Turret) {
            return Err(CapitalSchemaError::KindMismatch {
                key: turret.key.clone(),
                expected: CapitalSubsystemKind::Turret,
                actual: kind_of(&turret.key).expect("key is present"),
            });
        }
        validate_optional(
            &turret.traverse_deg,
            &format!("turret {}", turret.key),
            "traverse_deg",
        )?;
    }

    for anchor in &parts.docking_anchors {
        claim(&anchor.key, CapitalSubsystemKind::DockingAnchor)?;
        if kind_of(&anchor.key) != Some(CapitalSubsystemKind::DockingAnchor) {
            return Err(CapitalSchemaError::KindMismatch {
                key: anchor.key.clone(),
                expected: CapitalSubsystemKind::DockingAnchor,
                actual: kind_of(&anchor.key).expect("key is present"),
            });
        }
        if let Resolved::Known(known) = &anchor.offset_m
            && !known.value.iter().all(|value| value.is_finite())
        {
            return Err(CapitalSchemaError::NonFinite {
                key: format!("docking anchor {}", anchor.key),
                field: "offset_m",
            });
        }
    }

    for section in &parts.sections {
        let Some(kind) = kind_of(&section.key) else {
            return Err(CapitalSchemaError::UnknownSubsystem {
                key: section.key.clone(),
            });
        };
        if !matches!(
            kind,
            CapitalSubsystemKind::GasCell | CapitalSubsystemKind::StructuralSection
        ) {
            return Err(CapitalSchemaError::KindMismatch {
                key: section.key.clone(),
                expected: CapitalSubsystemKind::StructuralSection,
                actual: kind,
            });
        }
        claim(&section.key, kind)?;
        validate_optional(
            &section.integrity,
            &format!("section {}", section.key),
            "integrity",
        )?;
    }

    Ok(())
}

// ----------------------------------------------------------- fixture ------

fn key(name: &str) -> CapitalSubsystemKey {
    CapitalSubsystemKey::new(name).expect("fixture subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(cs_types::content::Known::new(
        value,
        Provenance::designed(
            cs_types::evidence::ClaimId::new("f35a.synthetic-leviathan")
                .expect("fixture claim id is valid"),
        ),
    ))
}

fn designed_vec(value: [f64; 3]) -> Resolved<[f64; 3]> {
    Resolved::Known(cs_types::content::Known::new(
        value,
        Provenance::designed(
            cs_types::evidence::ClaimId::new("f35a.synthetic-leviathan")
                .expect("fixture claim id is valid"),
        ),
    ))
}

/// The minimal synthetic fixture in declared form: the same capital ship
/// `cs_sim::capital::synthetic_capital_ship` defines, with declared
/// provenance.
///
/// The subject is `airframe/synthetic.leviathan` and every identity lives
/// under the `synthetic` key; the record carries [`Origin::SyntheticFixture`]
/// and designed provenance — it can never be mistaken for retail content and
/// cannot stand in for it.
#[must_use]
pub fn declared_synthetic_capital_ship() -> DeclaredCapitalShip {
    let parts = DeclaredCapitalParts {
        subsystems: vec![
            DeclaredSubsystem {
                key: key("engine_1"),
                kind: CapitalSubsystemKind::Engine,
                effect: Some(CapitalSubsystemEffect::Propulsion),
                lethal: false,
            },
            DeclaredSubsystem {
                key: key("engine_2"),
                kind: CapitalSubsystemKind::Engine,
                effect: Some(CapitalSubsystemEffect::Propulsion),
                lethal: false,
            },
            DeclaredSubsystem {
                key: key("weapon_bay_1"),
                kind: CapitalSubsystemKind::WeaponBay,
                effect: Some(CapitalSubsystemEffect::WeaponAccess),
                lethal: false,
            },
            DeclaredSubsystem {
                key: key("launch_bay_1"),
                kind: CapitalSubsystemKind::LaunchBay,
                effect: Some(CapitalSubsystemEffect::Launching),
                lethal: false,
            },
            DeclaredSubsystem {
                key: key("docking_anchor_1"),
                kind: CapitalSubsystemKind::DockingAnchor,
                effect: Some(CapitalSubsystemEffect::Docking),
                lethal: false,
            },
            DeclaredSubsystem {
                key: key("turret_1"),
                kind: CapitalSubsystemKind::Turret,
                effect: Some(CapitalSubsystemEffect::WeaponAccess),
                lethal: false,
            },
            DeclaredSubsystem {
                key: key("gas_cell_1"),
                kind: CapitalSubsystemKind::GasCell,
                effect: Some(CapitalSubsystemEffect::Vulnerability),
                lethal: true,
            },
            DeclaredSubsystem {
                key: key("keel"),
                kind: CapitalSubsystemKind::StructuralSection,
                effect: Some(CapitalSubsystemEffect::MissionCondition),
                lethal: true,
            },
        ],
        engines: vec![
            DeclaredEngine {
                key: key("engine_1"),
                thrust_n: designed(400_000.0),
                axis: [1.0, 0.0, 0.0],
            },
            DeclaredEngine {
                key: key("engine_2"),
                thrust_n: designed(400_000.0),
                axis: [1.0, 0.0, 0.0],
            },
        ],
        weapon_bays: vec![DeclaredWeaponBay {
            key: key("weapon_bay_1"),
            exposure: DeclaredExposure {
                concealed_ticks: 40,
                opening_ticks: 10,
                exposed_ticks: 60,
                closing_ticks: 10,
            },
        }],
        launch_bays: vec![DeclaredLaunchBay {
            key: key("launch_bay_1"),
            exposure: DeclaredExposure {
                concealed_ticks: 30,
                opening_ticks: 5,
                exposed_ticks: 45,
                closing_ticks: 5,
            },
            socket_offset_m: Resolved::Known(cs_types::content::Known::new(
                [0.0, -5.0, 0.0],
                Provenance::designed(
                    cs_types::evidence::ClaimId::new("f35a.synthetic-leviathan")
                        .expect("fixture claim id is valid"),
                ),
            )),
            capacity: Resolved::Known(cs_types::content::Known::new(
                4,
                Provenance::designed(
                    cs_types::evidence::ClaimId::new("f35a.synthetic-leviathan")
                        .expect("fixture claim id is valid"),
                ),
            )),
        }],
        turrets: vec![DeclaredTurret {
            key: key("turret_1"),
            weapon: Resolved::Unknown {
                claim_id: cs_types::evidence::ClaimId::new("f35a.turret-weapon-unmeasured")
                    .expect("fixture claim id is valid"),
                reason: "turret weapon binding unmeasured".to_owned(),
            },
            traverse_deg: designed(180.0),
        }],
        docking_anchors: vec![DeclaredDockingAnchor {
            key: key("docking_anchor_1"),
            offset_m: designed_vec([0.0, 0.0, 20.0]),
        }],
        sections: vec![
            DeclaredSection {
                key: key("gas_cell_1"),
                integrity: designed(120.0),
            },
            DeclaredSection {
                key: key("keel"),
                integrity: designed(200.0),
            },
        ],
    };

    DeclaredCapitalShip::try_new(
        ContentId::from_source(
            cs_types::content::ContentKind::Airframe,
            "synthetic.leviathan",
        )
        .expect("fixture subject id is valid"),
        Origin::SyntheticFixture,
        Provenance::designed(
            cs_types::evidence::ClaimId::new("f35a.synthetic-leviathan")
                .expect("fixture claim id is valid"),
        ),
        Some(DeclaredTrajectory {
            ticks_per_second: 10,
            keyframes: vec![
                DeclaredKeyframe {
                    tick: 0,
                    position_m: [0.0, 0.0, 0.0],
                    orientation: [0.0, 0.0, 0.0, 1.0],
                },
                DeclaredKeyframe {
                    tick: 500,
                    position_m: [1000.0, 0.0, 0.0],
                    orientation: [0.0, 0.0, 0.0, 1.0],
                },
            ],
        }),
        parts,
        designed(5_000.0),
        Resolved::Known(cs_types::content::Known::new(
            ContentId::from_source(cs_types::content::ContentKind::Faction, "synthetic.raiders")
                .expect("fixture owner id is valid"),
            Provenance::designed(
                cs_types::evidence::ClaimId::new("f35a.synthetic-leviathan")
                    .expect("fixture claim id is valid"),
            ),
        )),
    )
    .expect("the declared synthetic capital ship is valid")
}
