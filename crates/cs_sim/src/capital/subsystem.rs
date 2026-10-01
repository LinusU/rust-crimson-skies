//! Capital-ship subsystems: stable keys, kinds, effects and the disable graph
//! (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! A capital ship's overall health is not one number: mission interactions
//! target named subsystems and destroying one must change *which* behaviour
//! (movement, weapon access, spawning, vulnerability, a mission condition),
//! per non-negotiable behavior 1. This module is the shared identity and
//! state vocabulary every other capital module keys off:
//!
//! * [`SubsystemKey`] is the graph-local identity, applying the same
//!   `IDENTITY-CONTENT` key grammar as `cs_sim::damage::DamageNodeKey` and
//!   `cs_content::capital::CapitalSubsystemKey`.
//! * [`SubsystemKind`] distinguishes the parts the deliverable names —
//!   engines, weapon bays, launch bays, turrets, docking anchors, gas cells
//!   and structural sections.
//! * [`SubsystemEffect`] is the typed "changes the appropriate behavior"
//!   enumeration, and [`SubsystemKind::allows_effect`] refuses a pairing the
//!   design does not define instead of silently applying it.
//! * [`SubsystemGraph`] holds the parts and their [`SubsystemState`];
//!   [`SubsystemGraph::disable`] is the one place a part transitions and
//!   reports the effect it applied. A lethal part's disablement destroys the
//!   actor, and nothing else does.
//!
//! Everything here is newly authored project design with no measured
//! original counterpart; see
//! `docs/findings/2026-10-01-f35-a-capital-subsystems-and-bays.md`.

use std::collections::BTreeMap;
use std::fmt;

/// Maximum byte length of a [`SubsystemKey`].
pub const MAX_SUBSYSTEM_KEY_LEN: usize = 128;

/// Why a [`SubsystemKey`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubsystemKeyError {
    /// The key was empty.
    Empty,
    /// The key exceeded [`MAX_SUBSYSTEM_KEY_LEN`] bytes.
    TooLong {
        /// Its length in bytes.
        len: usize,
    },
    /// The key contained a character outside `[a-z0-9._-]` (after ASCII
    /// lowercasing).
    BadCharacter {
        /// The offending character.
        ch: char,
    },
    /// The key had no ASCII alphanumeric character.
    NoAlphanumeric,
}

impl fmt::Display for SubsystemKeyError {
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
            Self::NoAlphanumeric => {
                write!(f, "a subsystem key must contain an ASCII alphanumeric")
            }
        }
    }
}

impl std::error::Error for SubsystemKeyError {}

/// The stable identity of one subsystem inside a capital ship.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubsystemKey(String);

impl SubsystemKey {
    /// Validates and wraps a subsystem key.
    ///
    /// # Errors
    ///
    /// [`SubsystemKeyError`] when the key is empty, too long, carries a
    /// character outside `[a-z0-9._-]` or has no alphanumeric.
    pub fn new(key: &str) -> Result<Self, SubsystemKeyError> {
        let key = key.to_ascii_lowercase();
        if key.is_empty() {
            return Err(SubsystemKeyError::Empty);
        }
        if key.len() > MAX_SUBSYSTEM_KEY_LEN {
            return Err(SubsystemKeyError::TooLong { len: key.len() });
        }
        for ch in key.chars() {
            if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && !matches!(ch, '.' | '_' | '-') {
                return Err(SubsystemKeyError::BadCharacter { ch });
            }
        }
        if !key.bytes().any(|byte| byte.is_ascii_alphanumeric()) {
            return Err(SubsystemKeyError::NoAlphanumeric);
        }
        Ok(Self(key))
    }

    /// The normalized key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SubsystemKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What kind of part a subsystem models.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubsystemKind {
    /// A propulsion engine.
    Engine,
    /// A broadside or weapon bay whose opening is a weakpoint window.
    WeaponBay,
    /// A hangar / launch bay that releases aircraft.
    LaunchBay,
    /// A turret mount.
    Turret,
    /// A docking anchor.
    DockingAnchor,
    /// A lifting-gas cell whose rupture is a vulnerability.
    GasCell,
    /// Internal structure or a load-bearing section.
    StructuralSection,
}

impl SubsystemKind {
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

    /// Whether a subsystem of this kind may carry `effect`.
    ///
    /// The pairing is design, not a measured original rule: a turret cannot
    /// claim to spawn aircraft and a gas cell cannot claim to be a weapon
    /// mount. An unpaired effect is refused at graph construction.
    #[must_use]
    pub const fn allows_effect(self, effect: SubsystemEffect) -> bool {
        matches!(
            (self, effect),
            (Self::Engine, SubsystemEffect::Propulsion)
                | (
                    Self::WeaponBay | Self::Turret,
                    SubsystemEffect::WeaponAccess
                )
                | (Self::LaunchBay, SubsystemEffect::Launching)
                | (Self::DockingAnchor, SubsystemEffect::Docking)
                | (Self::GasCell, SubsystemEffect::Vulnerability)
                | (Self::StructuralSection, SubsystemEffect::MissionCondition)
        )
    }

    /// Whether destroying a part of this kind may destroy the whole actor.
    #[must_use]
    pub const fn can_be_lethal(self) -> bool {
        matches!(self, Self::GasCell | Self::StructuralSection)
    }
}

impl fmt::Display for SubsystemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A gameplay behaviour a destroyed subsystem changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SubsystemEffect {
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

impl SubsystemEffect {
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

impl fmt::Display for SubsystemEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The state of one subsystem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubsystemState {
    /// The part is whole. (Damage that has not yet destroyed it is tracked
    /// by the F29 damage graph, not here.)
    Intact,
    /// The part is destroyed: its effect is applied and it is never reused.
    Disabled,
}

/// One declared part of a capital ship.
#[derive(Clone, Debug, PartialEq)]
pub struct Subsystem {
    /// The part's stable key within the ship.
    pub key: SubsystemKey,
    /// The part's kind.
    pub kind: SubsystemKind,
    /// The behavior its destruction changes, if any.
    pub effect: Option<SubsystemEffect>,
    /// Whether destroying it destroys the whole actor.
    pub lethal: bool,
}

impl Subsystem {
    /// A part with no effect and no lethality.
    #[must_use]
    pub fn new(key: SubsystemKey, kind: SubsystemKind) -> Self {
        Self {
            key,
            kind,
            effect: None,
            lethal: false,
        }
    }

    /// Sets the effect its destruction applies.
    #[must_use]
    pub fn with_effect(mut self, effect: SubsystemEffect) -> Self {
        self.effect = Some(effect);
        self
    }

    /// Marks the part as lethal.
    #[must_use]
    pub fn with_lethal(mut self, lethal: bool) -> Self {
        self.lethal = lethal;
        self
    }
}

/// Why a [`SubsystemGraph`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubsystemGraphError {
    /// A ship with no parts models nothing.
    Empty,
    /// Two parts share one key.
    DuplicateKey {
        /// The duplicated key.
        key: SubsystemKey,
    },
    /// A part declares an effect its kind does not allow.
    EffectKindMismatch {
        /// The offending part.
        key: SubsystemKey,
        /// Its kind.
        kind: SubsystemKind,
        /// The refused effect.
        effect: SubsystemEffect,
    },
    /// A part is lethal although its kind can never destroy the actor.
    LethalKind {
        /// The offending part.
        key: SubsystemKey,
        /// Its kind.
        kind: SubsystemKind,
    },
    /// A key names no part of the graph.
    UnknownSubsystem(SubsystemKey),
}

impl fmt::Display for SubsystemGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a subsystem graph must contain at least one part"),
            Self::DuplicateKey { key } => {
                write!(f, "subsystem key {key:?} is used more than once")
            }
            Self::EffectKindMismatch { key, kind, effect } => write!(
                f,
                "subsystem {key} of kind {kind} cannot apply the {effect} effect"
            ),
            Self::LethalKind { key, kind } => {
                write!(f, "subsystem {key} of kind {kind} cannot be lethal")
            }
            Self::UnknownSubsystem(key) => {
                write!(f, "subsystem {key} is not part of this ship")
            }
        }
    }
}

impl std::error::Error for SubsystemGraphError {}

/// What disabling one subsystem changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisableOutcome {
    /// The subsystem's state after the call.
    pub state: SubsystemState,
    /// Whether this call newly disabled the part. An idempotent repeat is
    /// `false` and applies nothing twice.
    pub changed: bool,
    /// Whether the actor is destroyed. Only a lethal part sets this.
    pub actor_destroyed: bool,
    /// The behavior the destruction applied, if the part has one.
    pub effect: Option<SubsystemEffect>,
}

/// The parts of one capital ship and their current state.
///
/// Identity is the key, never the position in the authored list.
#[derive(Clone, Debug, PartialEq)]
pub struct SubsystemGraph {
    subsystems: BTreeMap<SubsystemKey, Subsystem>,
    states: BTreeMap<SubsystemKey, SubsystemState>,
}

impl SubsystemGraph {
    /// Assembles and validates a graph.
    ///
    /// # Errors
    ///
    /// [`SubsystemGraphError`] on an empty part set, a duplicate key, an
    /// effect its kind does not allow or a lethal non-structural/gas part.
    pub fn try_new(subsystems: Vec<Subsystem>) -> Result<Self, SubsystemGraphError> {
        if subsystems.is_empty() {
            return Err(SubsystemGraphError::Empty);
        }
        let mut map = BTreeMap::new();
        let mut states = BTreeMap::new();
        for subsystem in subsystems {
            if let Some(effect) = subsystem.effect
                && !subsystem.kind.allows_effect(effect)
            {
                return Err(SubsystemGraphError::EffectKindMismatch {
                    key: subsystem.key,
                    kind: subsystem.kind,
                    effect,
                });
            }
            if subsystem.lethal && !subsystem.kind.can_be_lethal() {
                return Err(SubsystemGraphError::LethalKind {
                    key: subsystem.key,
                    kind: subsystem.kind,
                });
            }
            let key = subsystem.key.clone();
            if map.insert(key.clone(), subsystem).is_some() {
                return Err(SubsystemGraphError::DuplicateKey { key });
            }
            states.insert(key, SubsystemState::Intact);
        }
        Ok(Self {
            subsystems: map,
            states,
        })
    }

    /// The declared parts, in key order.
    pub fn subsystems(&self) -> impl Iterator<Item = &Subsystem> {
        self.subsystems.values()
    }

    /// The number of parts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.subsystems.len()
    }

    /// Whether the graph has no parts. Never `true` for a constructed graph.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.subsystems.is_empty()
    }

    /// A part by key.
    #[must_use]
    pub fn subsystem(&self, key: &SubsystemKey) -> Option<&Subsystem> {
        self.subsystems.get(key)
    }

    /// A part's current state.
    #[must_use]
    pub fn state(&self, key: &SubsystemKey) -> Option<SubsystemState> {
        self.states.get(key).copied()
    }

    /// Whether any lethal part has been destroyed.
    #[must_use]
    pub fn is_destroyed(&self) -> bool {
        self.subsystems.values().any(|subsystem| {
            subsystem.lethal && self.states.get(&subsystem.key) == Some(&SubsystemState::Disabled)
        })
    }

    /// Disables one part and reports what changed. A part with no effect
    /// still transitions; only its own state changes. This is the only
    /// place a subsystem transitions, so its effect cannot be applied by a
    /// side path.
    ///
    /// # Errors
    ///
    /// [`SubsystemGraphError::UnknownSubsystem`] when the key names no part.
    pub fn disable(&mut self, key: &SubsystemKey) -> Result<DisableOutcome, SubsystemGraphError> {
        let Some(subsystem) = self.subsystems.get(key) else {
            return Err(SubsystemGraphError::UnknownSubsystem(key.clone()));
        };
        let effect = subsystem.effect;
        let lethal = subsystem.lethal;
        if self.states.get(key) == Some(&SubsystemState::Disabled) {
            return Ok(DisableOutcome {
                state: SubsystemState::Disabled,
                changed: false,
                actor_destroyed: false,
                effect: None,
            });
        }
        self.states.insert(key.clone(), SubsystemState::Disabled);
        Ok(DisableOutcome {
            state: SubsystemState::Disabled,
            changed: true,
            actor_destroyed: lethal,
            effect,
        })
    }
}
