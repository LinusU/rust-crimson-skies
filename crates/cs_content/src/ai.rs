//! The declared combat-AI schema: roles, skill knobs, priority policy,
//! ace variants, formations and difficulty profiles (F32-A).
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! This module is the **content half** of the combat-AI contract — the
//! normalized record a mission/rules importer produces. Its runtime
//! counterpart is `cs_sim::ai::combat` (the [`CombatPlanner`] a session
//! runs); the conversion boundary between them is `cs_app::ai::combat`.
//! The split mirrors `target_rules` ↔ `cs_sim::targeting` and
//! `damage` ↔ `cs_sim::damage`: this crate cannot depend on `cs_sim`, so
//! the declared record keeps its own [`DeclaredCombatRole`] and
//! [`RecoveryPolicy`] vocabulary and the boundary maps them field-wise.
//!
//! # Records
//!
//! [`DeclaredCombatRules`] names the `subject` the rules belong to — the
//! catalog id of the mission or launchable scenario — and carries the
//! declared role definitions, the declared ace variants, the declared
//! formations and the declared difficulty profiles.
//!
//! * [`SkillKnobs`] and [`PriorityPolicy`] are the tuning values. Every one
//!   of them is a [`Resolved`]: measured, authored or explicitly unknown
//!   with its claim id and reason. A role whose weights are unknown refuses
//!   to lower rather than running combat AI on a silent default
//!   (F32 non-negotiable 1, AGENTS "unknown means unknown").
//! * [`SkillKnob`] is the **closed** behavior vocabulary a variant may
//!   move: reaction delay, aim error, engagement range, fire discipline and
//!   the four priority weights. There is deliberately no damage, armor,
//!   health or simulation-rate variant, so an ace is a list of *behavior*
//!   overrides and a difficulty tier cannot fake difficulty by speeding the
//!   simulation up (F32 "Deliverable and interfaces", non-negotiable 1).
//! * [`PriorityPolicy`] keeps the four priority terms and the threat window
//!   separate: which candidate is *hostile* is a relation question answered
//!   by `cs_content::target_rules`, and this policy only ranks the hostiles
//!   the mission already declared. Friendly-fire avoidance and line-of-fire
//!   checks are a third, separate predicate and are not expressed as a
//!   priority term (non-negotiable 3).
//! * [`DeclaredAceProfile`] is a data-driven behavior/skill **variant** of a
//!   role: an id, a base role and [`SkillKnobOverride`]s.
//! * [`DeclaredFormation`] names its leader, its members and one
//!   [`RecoveryPolicy`] per recovery trigger, so leader loss, assigned-target
//!   destruction and route interruption have declared recovery paths
//!   (non-negotiable 4) instead of a per-tick invention.
//! * [`DifficultyProfile`] is a named [`DifficultyTier`] plus overrides; it
//!   can only move [`SkillKnob`]s, by construction.
//!
//! # Designed vocabulary, not original data
//!
//! The original game's AI role set, its target-priority order, reaction
//! times, aim error, engagement ranges, formation membership, recovery
//! behavior and the effect of its difficulty option are **unmeasured** (F32
//! "Research boundary"; F32-D's retail stage). Every role, weight, knob,
//! bound and fixture value in this module is newly authored project design
//! carrying `Origin::SyntheticFixture` or `Origin::Designed` and designed
//! provenance, recorded in
//! `docs/findings/2026-10-01-f32-a-combat-roles-skill-knobs-and-decision-traces.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::sync::OnceLock;

use cs_assets::install;
use cs_assets::rof::mount_rof_into;
use cs_assets::vfs::{INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_formats::ParseContext;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::text::resource_header::read_resource_header;
use cs_types::asset_id::{
    AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext, SourceSpan,
};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};
use cs_types::install::{InstallManifest, RelativePath};
use cs_types::space::{Meters, Radians};

use crate::config::StringCatalog;
use crate::stunts::{
    SCENARIO_MEMBER, ZrdValue, decode_zrd, scenario_mission_type, scenario_non_player_aircraft,
    zrd_field,
};

/// The largest reaction delay a declared role may carry, in ticks.
///
/// A designed bound, not a measured original reaction time: it only keeps a
/// corrupt or mis-scaled value out of the record. One minute at a 60 Hz
/// simulation tick.
pub const MAX_REACTION_TICKS: u64 = 3_600;

/// The largest fire discipline delay a declared role may carry, in ticks.
/// A designed bound, as [`MAX_REACTION_TICKS`].
pub const MAX_FIRE_DISCIPLINE_TICKS: u64 = 3_600;

/// The largest window an authoritative attack stays a live threat, in
/// ticks. A designed bound, as [`MAX_REACTION_TICKS`].
pub const MAX_THREAT_WINDOW_TICKS: u64 = 3_600;

/// The largest engagement range a declared role may carry, in meters. A
/// designed bound, not a measured weapon or sensor range.
pub const MAX_ENGAGEMENT_RANGE_M: f64 = 20_000.0;

/// The smallest engagement range a declared role may carry, in meters.
///
/// A zero-meter range is not a tight engagement, it is a role that can
/// never acquire anything: the proximity term is normalized over it, so the
/// range has to be positive. This mirrors the guard the lowered
/// `cs_sim::ai::combat::SkillProfile` applies, so a declared record cannot
/// carry a value the lowering boundary would refuse.
pub const MIN_ENGAGEMENT_RANGE_M: f64 = 1e-9;

/// The largest aim error half-angle a declared role may carry, in radians.
/// A designed bound: a quarter turn, so an "aim error" can never exceed a
/// deliberate deflection.
pub const MAX_AIM_ERROR_RAD: f64 = std::f64::consts::FRAC_PI_2;

/// The largest absolute priority weight a declared policy may carry.
/// A designed bound that keeps a mis-scaled weight from silently outvoting
/// every other term.
pub const MAX_PRIORITY_WEIGHT: f64 = 1_000.0;

// --------------------------------------------------------- roles ----

/// A declared combat role: what one AI actor is trying to do.
///
/// These are the seven behaviors the F32 sheet names — fighter attack,
/// bomber run, torpedo run, escort, interception, evasion and retreat.
/// Whether the original game classifies its AI with exactly this set is
/// unmeasured (F32-D); this is designed engine vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredCombatRole {
    /// Engage the nearest eligible hostile with guns.
    FighterAttack,
    /// Run an ordnance attack on a declared target.
    BomberRun,
    /// Run an aerial-torpedo attack on a declared target.
    TorpedoRun,
    /// Stay with a protected actor and answer what threatens it.
    Escort,
    /// Intercept a declared hostile.
    Intercept,
    /// Break away from a declared threat without disengaging the mission.
    Evade,
    /// Withdraw from the engagement.
    Retreat,
}

impl DeclaredCombatRole {
    /// Every role, in a stable order.
    pub const ALL: &'static [DeclaredCombatRole] = &[
        Self::FighterAttack,
        Self::BomberRun,
        Self::TorpedoRun,
        Self::Escort,
        Self::Intercept,
        Self::Evade,
        Self::Retreat,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FighterAttack => "fighter_attack",
            Self::BomberRun => "bomber_run",
            Self::TorpedoRun => "torpedo_run",
            Self::Escort => "escort",
            Self::Intercept => "intercept",
            Self::Evade => "evade",
            Self::Retreat => "retreat",
        }
    }

    /// Whether the role is declared to need ordnance. A structural
    /// requirement, not a tuning value: a torpedo run has nothing to do
    /// without a launcher.
    #[must_use]
    pub const fn needs_ordnance(self) -> bool {
        matches!(self, Self::BomberRun | Self::TorpedoRun)
    }
}

impl fmt::Display for DeclaredCombatRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a role is allowed to shoot with.
///
/// Structural, designed vocabulary. It is deliberately not a
/// [`Resolved`]: a role's *arsenal requirement* is part of this engine's
/// own role definition, while every *number* below is a measured or
/// authored tuning value. An actor without the mounts a role requires
/// cannot hold that role — that check is the runtime's
/// (`cs_sim::ai::combat` AC02 stage), not this record's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct RoleArsenal {
    /// Whether the role is declared to use gun mounts.
    pub gun: bool,
    /// Whether the role is declared to use ordnance hardpoints.
    pub ordnance: bool,
}

impl RoleArsenal {
    /// No weapon at all: the evasion and retreat roles, which maneuver
    /// rather than shoot.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            gun: false,
            ordnance: false,
        }
    }

    /// Guns only.
    #[must_use]
    pub const fn guns() -> Self {
        Self {
            gun: true,
            ordnance: false,
        }
    }

    /// Guns and ordnance.
    #[must_use]
    pub const fn guns_and_ordnance() -> Self {
        Self {
            gun: true,
            ordnance: true,
        }
    }
}

// ---------------------------------------------------------- knobs ----

/// The unit a [`SkillKnob`] is expressed in.
///
/// Every knob has exactly one unit, and an override whose value carries
/// another unit is refused rather than reinterpreted — a distance in a
/// weight slot is a content bug, not a rescaled weight.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SkillKnobUnit {
    /// A count of simulation ticks.
    Ticks,
    /// An angle in radians.
    Angle,
    /// A distance in meters.
    Distance,
    /// A dimensionless priority weight.
    Weight,
}

impl SkillKnobUnit {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ticks => "ticks",
            Self::Angle => "radians",
            Self::Distance => "meters",
            Self::Weight => "weight",
        }
    }
}

impl fmt::Display for SkillKnobUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The closed vocabulary of behavior knobs a variant or a difficulty tier
/// may move.
///
/// The list is the F32-A contract that an ace is a *behavior* variant and
/// that difficulty is not a simulation rate (non-negotiable 1): there is no
/// damage, armor, health, tick-rate or time-scale variant here, so neither
/// record can express one. F32-B/F32-C add no variant to this enum without
/// the corresponding evidence; the retail stage measures what the original
/// actually varies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SkillKnob {
    /// Ticks between perceiving a threat and acting on it.
    ReactionTicks,
    /// The half-angle of the aim error added to a firing solution.
    AimErrorRad,
    /// The range beyond which a candidate is not an engagement.
    EngagementRangeM,
    /// Ticks the AI waits between two fire decisions.
    FireDisciplineTicks,
    /// How many ticks after an attack it still counts as a live threat.
    ThreatWindowTicks,
    /// The weight of "this candidate is attacking my protected actor".
    ProtectedActorWeight,
    /// The weight of "this candidate is the script-assigned objective".
    ObjectiveWeight,
    /// The weight of "this candidate is attacking me".
    SelfDefenseWeight,
    /// The weight of proximity, normalized over the engagement range.
    ProximityWeight,
}

impl SkillKnob {
    /// Every knob, in a stable order.
    pub const ALL: &'static [SkillKnob] = &[
        Self::ReactionTicks,
        Self::AimErrorRad,
        Self::EngagementRangeM,
        Self::FireDisciplineTicks,
        Self::ThreatWindowTicks,
        Self::ProtectedActorWeight,
        Self::ObjectiveWeight,
        Self::SelfDefenseWeight,
        Self::ProximityWeight,
    ];

    /// The stable label used in reports and in content files.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ReactionTicks => "reaction_ticks",
            Self::AimErrorRad => "aim_error_rad",
            Self::EngagementRangeM => "engagement_range_m",
            Self::FireDisciplineTicks => "fire_discipline_ticks",
            Self::ThreatWindowTicks => "threat_window_ticks",
            Self::ProtectedActorWeight => "protected_actor_weight",
            Self::ObjectiveWeight => "objective_weight",
            Self::SelfDefenseWeight => "self_defense_weight",
            Self::ProximityWeight => "proximity_weight",
        }
    }

    /// The unit this knob is expressed in.
    #[must_use]
    pub const fn unit(self) -> SkillKnobUnit {
        match self {
            Self::ReactionTicks | Self::FireDisciplineTicks | Self::ThreatWindowTicks => {
                SkillKnobUnit::Ticks
            }
            Self::AimErrorRad => SkillKnobUnit::Angle,
            Self::EngagementRangeM => SkillKnobUnit::Distance,
            Self::ProtectedActorWeight
            | Self::ObjectiveWeight
            | Self::SelfDefenseWeight
            | Self::ProximityWeight => SkillKnobUnit::Weight,
        }
    }

    /// The knob with the given [`SkillKnob::label`], if there is one.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|knob| knob.label() == label)
    }
}

impl fmt::Display for SkillKnob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A typed value for one [`SkillKnob`].
///
/// The variant carries the knob's unit, so an override cannot put a
/// distance into a weight slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SkillKnobValue {
    /// A tick count.
    Ticks(u64),
    /// An angle in radians.
    Angle(f64),
    /// A distance in meters.
    Distance(f64),
    /// A dimensionless weight.
    Weight(f64),
}

impl SkillKnobValue {
    /// The unit this value is expressed in.
    #[must_use]
    pub const fn unit(self) -> SkillKnobUnit {
        match self {
            Self::Ticks(_) => SkillKnobUnit::Ticks,
            Self::Angle(_) => SkillKnobUnit::Angle,
            Self::Distance(_) => SkillKnobUnit::Distance,
            Self::Weight(_) => SkillKnobUnit::Weight,
        }
    }
}

impl fmt::Display for SkillKnobValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ticks(ticks) => write!(f, "{ticks} ticks"),
            Self::Angle(rad) => write!(f, "{rad} rad"),
            Self::Distance(m) => write!(f, "{m} m"),
            Self::Weight(weight) => write!(f, "{weight}"),
        }
    }
}

/// One declared override of a single [`SkillKnob`].
///
/// Every override carries its own [`Provenance`]: an ace that changes a
/// knob is an evidence-classed claim, not a silent tuning change.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillKnobOverride {
    /// Which knob is overridden.
    pub knob: SkillKnob,
    /// The new value, in the knob's own unit.
    pub value: SkillKnobValue,
    /// Where the override came from.
    pub provenance: Provenance,
}

impl SkillKnobOverride {
    /// Validates that the value matches the knob's unit and lies inside the
    /// declared bounds.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::KnobUnitMismatch`] when the value's unit is not
    /// the knob's, [`CombatSchemaError::NonFiniteKnob`] for a NaN or
    /// infinite number and [`CombatSchemaError::KnobOutOfRange`] for a
    /// number outside the knob's approved range.
    pub fn try_new(
        knob: SkillKnob,
        value: SkillKnobValue,
        provenance: Provenance,
    ) -> Result<Self, CombatSchemaError> {
        validate_knob_value(knob, value)?;
        Ok(Self {
            knob,
            value,
            provenance,
        })
    }
}

/// The value of a [`Resolved`], when it is known.
///
/// A local helper so the validators read as "check the known values" and
/// an unknown stays unknown instead of collapsing to a number.
fn resolved_value<T: Copy>(resolved: &Resolved<T>) -> Option<T> {
    match resolved {
        Resolved::Known(known) => Some(known.value),
        Resolved::Unknown { .. } => None,
    }
}

/// A known [`Resolved`] carrying the given provenance: what an override
/// leaves behind after it moves a knob.
fn resolved_with<T>(value: T, provenance: &Provenance) -> Resolved<T> {
    Resolved::Known(cs_types::content::Known::new(value, provenance.clone()))
}

/// Checks a knob value against its unit and its approved range.
fn validate_knob_value(knob: SkillKnob, value: SkillKnobValue) -> Result<(), CombatSchemaError> {
    if value.unit() != knob.unit() {
        return Err(CombatSchemaError::KnobUnitMismatch {
            knob,
            expected: knob.unit(),
            found: value.unit(),
        });
    }
    match value {
        SkillKnobValue::Ticks(ticks) => {
            let max = match knob {
                SkillKnob::ReactionTicks => MAX_REACTION_TICKS,
                SkillKnob::FireDisciplineTicks => MAX_FIRE_DISCIPLINE_TICKS,
                SkillKnob::ThreatWindowTicks => MAX_THREAT_WINDOW_TICKS,
                _ => {
                    return Err(CombatSchemaError::KnobOutOfRange {
                        knob,
                        value: value.to_string(),
                    });
                }
            };
            if ticks > max {
                return Err(CombatSchemaError::KnobOutOfRange {
                    knob,
                    value: value.to_string(),
                });
            }
        }
        SkillKnobValue::Angle(rad) => {
            if !rad.is_finite() {
                return Err(CombatSchemaError::NonFiniteKnob { knob });
            }
            if !(0.0..=MAX_AIM_ERROR_RAD).contains(&rad) {
                return Err(CombatSchemaError::KnobOutOfRange {
                    knob,
                    value: value.to_string(),
                });
            }
        }
        SkillKnobValue::Distance(m) => {
            if !m.is_finite() {
                return Err(CombatSchemaError::NonFiniteKnob { knob });
            }
            if !(MIN_ENGAGEMENT_RANGE_M..=MAX_ENGAGEMENT_RANGE_M).contains(&m) {
                return Err(CombatSchemaError::KnobOutOfRange {
                    knob,
                    value: value.to_string(),
                });
            }
        }
        SkillKnobValue::Weight(weight) => {
            if !weight.is_finite() {
                return Err(CombatSchemaError::NonFiniteKnob { knob });
            }
            if !(0.0..=MAX_PRIORITY_WEIGHT).contains(&weight) {
                return Err(CombatSchemaError::KnobOutOfRange {
                    knob,
                    value: value.to_string(),
                });
            }
        }
    }
    Ok(())
}

/// The reaction, aim, range and fire-cadence knobs of one role.
///
/// Each field is a [`Resolved`]: a value the importer could evidence is
/// `Known` with its provenance, and a value that matters but is unmeasured
/// is `Unknown` with its claim id and reason, which refuses to lower.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillKnobs {
    /// Ticks between perceiving a threat and acting on it.
    pub reaction_ticks: Resolved<u64>,
    /// The half-angle of the aim error added to a firing solution.
    pub aim_error_rad: Resolved<Radians>,
    /// The range beyond which a candidate is not an engagement.
    pub engagement_range_m: Resolved<Meters>,
    /// Ticks the AI waits between two fire decisions.
    pub fire_discipline_ticks: Resolved<u64>,
}

impl SkillKnobs {
    /// Validates the known values against the declared bounds.
    ///
    /// Unknown values pass: an unknown is reported as unknown at the
    /// lowering boundary, never clamped into a number here.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::NonFiniteKnob`] or
    /// [`CombatSchemaError::KnobOutOfRange`] naming the offending knob.
    pub fn validate(&self) -> Result<(), CombatSchemaError> {
        if let Some(ticks) = resolved_value(&self.reaction_ticks) {
            validate_knob_value(SkillKnob::ReactionTicks, SkillKnobValue::Ticks(ticks))?;
        }
        if let Some(rad) = resolved_value(&self.aim_error_rad) {
            validate_knob_value(SkillKnob::AimErrorRad, SkillKnobValue::Angle(rad.0))?;
        }
        if let Some(m) = resolved_value(&self.engagement_range_m) {
            validate_knob_value(SkillKnob::EngagementRangeM, SkillKnobValue::Distance(m.0))?;
        }
        if let Some(ticks) = resolved_value(&self.fire_discipline_ticks) {
            validate_knob_value(SkillKnob::FireDisciplineTicks, SkillKnobValue::Ticks(ticks))?;
        }
        Ok(())
    }

    /// Applies one validated override, refusing a knob this record does
    /// not carry.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::NotASkillKnob`] for a priority weight, which
    /// belongs to [`PriorityPolicy`], and the validation errors of
    /// [`SkillKnobOverride::try_new`] for a malformed value.
    pub fn apply(&mut self, change: &SkillKnobOverride) -> Result<(), CombatSchemaError> {
        validate_knob_value(change.knob, change.value)?;
        match (change.knob, change.value) {
            (SkillKnob::ReactionTicks, SkillKnobValue::Ticks(ticks)) => {
                self.reaction_ticks = resolved_with(ticks, &change.provenance);
            }
            (SkillKnob::AimErrorRad, SkillKnobValue::Angle(rad)) => {
                self.aim_error_rad = resolved_with(Radians(rad), &change.provenance);
            }
            (SkillKnob::EngagementRangeM, SkillKnobValue::Distance(m)) => {
                self.engagement_range_m = resolved_with(Meters(m), &change.provenance);
            }
            (SkillKnob::FireDisciplineTicks, SkillKnobValue::Ticks(ticks)) => {
                self.fire_discipline_ticks = resolved_with(ticks, &change.provenance);
            }
            _ => return Err(CombatSchemaError::NotASkillKnob { knob: change.knob }),
        }
        Ok(())
    }
}

// -------------------------------------------------------- policy ----

/// One scored term of a declared [`PriorityPolicy`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PriorityTerm {
    /// The candidate is attacking the observer's protected actor.
    ProtectedActorThreat,
    /// The candidate is the script-assigned objective.
    ScriptObjective,
    /// The candidate is attacking the observer.
    SelfDefense,
    /// The candidate is close, normalized over the engagement range.
    Proximity,
}

impl PriorityTerm {
    /// Every term, in a stable order.
    pub const ALL: &'static [PriorityTerm] = &[
        Self::ProtectedActorThreat,
        Self::ScriptObjective,
        Self::SelfDefense,
        Self::Proximity,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ProtectedActorThreat => "protected_actor_threat",
            Self::ScriptObjective => "script_objective",
            Self::SelfDefense => "self_defense",
            Self::Proximity => "proximity",
        }
    }

    /// The knob that carries this term's weight.
    #[must_use]
    pub const fn knob(self) -> SkillKnob {
        match self {
            Self::ProtectedActorThreat => SkillKnob::ProtectedActorWeight,
            Self::ScriptObjective => SkillKnob::ObjectiveWeight,
            Self::SelfDefense => SkillKnob::SelfDefenseWeight,
            Self::Proximity => SkillKnob::ProximityWeight,
        }
    }
}

impl fmt::Display for PriorityTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared target-priority policy of one role.
///
/// The four weights rank the hostiles a mission already declared hostile:
/// hostility itself is a *relation* (`cs_content::target_rules`) and is a
/// gate, not a term, so an objective that is not declared hostile stays
/// unselectable however high its weight is. Friendly-fire avoidance and
/// line-of-fire checks are a third predicate and are not terms here
/// (non-negotiable 3).
///
/// `threat_window_ticks` is how long an authoritative attack on the
/// protected actor keeps counting: threats come from recorded attack
/// events, never from proximity (non-negotiable 4).
#[derive(Clone, Debug, PartialEq)]
pub struct PriorityPolicy {
    /// Weight of "this candidate is attacking my protected actor".
    pub protected_actor_weight: Resolved<f64>,
    /// Weight of "this candidate is the script-assigned objective".
    pub objective_weight: Resolved<f64>,
    /// Weight of "this candidate is attacking me".
    pub self_defense_weight: Resolved<f64>,
    /// Weight of proximity, normalized over the engagement range.
    pub proximity_weight: Resolved<f64>,
    /// How many ticks after an attack it still counts as a threat.
    pub threat_window_ticks: Resolved<u64>,
}

impl PriorityPolicy {
    /// Validates the known values against the declared bounds and refuses
    /// a policy that scores nothing.
    ///
    /// Unknown weights do not fail here — they are reported at the lowering
    /// boundary — but a policy whose four weights are *all* known and zero
    /// is refused by name: it would tie every candidate and make the
    /// decision an artifact of the tie-break.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::NonFiniteKnob`] or
    /// [`CombatSchemaError::KnobOutOfRange`] naming the offending knob, and
    /// [`CombatSchemaError::NoScoredPriorityTerm`] when no weight is
    /// positive.
    pub fn validate(&self) -> Result<(), CombatSchemaError> {
        let weights = [
            (
                SkillKnob::ProtectedActorWeight,
                &self.protected_actor_weight,
            ),
            (SkillKnob::ObjectiveWeight, &self.objective_weight),
            (SkillKnob::SelfDefenseWeight, &self.self_defense_weight),
            (SkillKnob::ProximityWeight, &self.proximity_weight),
        ];
        for (knob, weight) in weights {
            if let Some(value) = resolved_value(weight) {
                validate_knob_value(knob, SkillKnobValue::Weight(value))?;
            }
        }
        if let Some(ticks) = resolved_value(&self.threat_window_ticks) {
            validate_knob_value(SkillKnob::ThreatWindowTicks, SkillKnobValue::Ticks(ticks))?;
        }
        if weights
            .iter()
            .all(|(_, weight)| resolved_value(weight).is_some_and(|value| value == 0.0))
        {
            return Err(CombatSchemaError::NoScoredPriorityTerm);
        }
        Ok(())
    }

    /// The known weight of one term, if it is known.
    #[must_use]
    pub fn known_weight(&self, term: PriorityTerm) -> Option<f64> {
        let weight = match term {
            PriorityTerm::ProtectedActorThreat => &self.protected_actor_weight,
            PriorityTerm::ScriptObjective => &self.objective_weight,
            PriorityTerm::SelfDefense => &self.self_defense_weight,
            PriorityTerm::Proximity => &self.proximity_weight,
        };
        resolved_value(weight)
    }

    /// Applies one validated policy override: a priority weight or the
    /// threat window.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::NotAPriorityWeight`] for a skill knob and the
    /// validation errors of [`SkillKnobOverride::try_new`] for a malformed
    /// value.
    pub fn apply(&mut self, change: &SkillKnobOverride) -> Result<(), CombatSchemaError> {
        validate_knob_value(change.knob, change.value)?;
        match (change.knob, change.value) {
            (SkillKnob::ProtectedActorWeight, SkillKnobValue::Weight(weight)) => {
                self.protected_actor_weight = resolved_with(weight, &change.provenance);
            }
            (SkillKnob::ObjectiveWeight, SkillKnobValue::Weight(weight)) => {
                self.objective_weight = resolved_with(weight, &change.provenance);
            }
            (SkillKnob::SelfDefenseWeight, SkillKnobValue::Weight(weight)) => {
                self.self_defense_weight = resolved_with(weight, &change.provenance);
            }
            (SkillKnob::ProximityWeight, SkillKnobValue::Weight(weight)) => {
                self.proximity_weight = resolved_with(weight, &change.provenance);
            }
            (SkillKnob::ThreatWindowTicks, SkillKnobValue::Ticks(ticks)) => {
                self.threat_window_ticks = resolved_with(ticks, &change.provenance);
            }
            _ => return Err(CombatSchemaError::NotAPriorityWeight { knob: change.knob }),
        }
        Ok(())
    }
}

// ---------------------------------------------------------- aces ----

/// A declared ace variant: a data-driven behavior/skill variant of a role.
///
/// The record has **no** damage, armor or health field. An ace is faster to
/// react, more accurate, more willing to leave formation or more careful
/// with its weapons — the F32 sheet's "not just inflated health" requirement
/// is a property of this type, not a convention a producer has to honor
/// (F32 "Deliverable and interfaces").
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredAceProfile {
    /// The variant's identity, in the `pilot` namespace.
    ///
    /// The identity carrier only. Which pilot, aircraft and faction
    /// relations a variant belongs to is F33-A's separation contract, not
    /// this stage's.
    id: ContentId,
    /// The role this variant modifies.
    base_role: DeclaredCombatRole,
    /// The behavior overrides it applies to that role.
    overrides: Vec<SkillKnobOverride>,
    origin: Origin,
    provenance: Provenance,
}

impl DeclaredAceProfile {
    /// Assembles and validates an ace variant.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::AceKindMismatch`] when the id is not in the
    /// `pilot` namespace and [`CombatSchemaError::DuplicateKnob`] when two
    /// overrides name the same knob.
    pub fn try_new(
        id: ContentId,
        base_role: DeclaredCombatRole,
        overrides: Vec<SkillKnobOverride>,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, CombatSchemaError> {
        if id.kind() != ContentKind::Pilot {
            return Err(CombatSchemaError::AceKindMismatch { id });
        }
        check_unique_knobs(&overrides)?;
        Ok(Self {
            id,
            base_role,
            overrides,
            origin,
            provenance,
        })
    }

    /// The variant's identity.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }

    /// The role this variant modifies.
    #[must_use]
    pub const fn base_role(&self) -> DeclaredCombatRole {
        self.base_role
    }

    /// The behavior overrides, in authored order.
    #[must_use]
    pub fn overrides(&self) -> &[SkillKnobOverride] {
        &self.overrides
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

    /// The declared value of one knob after this variant's overrides.
    ///
    /// This is the variant's whole effect on a role: a [`Resolved`] that
    /// carries the override's own provenance, so a lowered profile can say
    /// *which* claim moved the knob.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::NotASkillKnob`] for a priority weight, which
    /// lives on the policy rather than the knobs.
    pub fn knob_value(
        &self,
        knob: SkillKnob,
    ) -> Result<Resolved<SkillKnobValue>, CombatSchemaError> {
        let change = self
            .overrides
            .iter()
            .find(|change| change.knob == knob)
            .ok_or(CombatSchemaError::UnknownKnob { knob })?;
        Ok(resolved_with(change.value, &change.provenance))
    }
}

// ------------------------------------------------- formations ----

/// The identity of one declared formation.
///
/// A formation is mission-authored, not a catalog asset, so it takes the
/// same `u32` newtype [`RouteNodeId`](super::routes::RouteNodeId) does for
/// route nodes: stable within a subject, comparable, and never a filename
/// guess.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FormationId(pub u32);

impl FormationId {
    /// The formation's index.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0
    }
}

impl fmt::Display for FormationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "formation {}", self.0)
    }
}

/// Whether a formation member leads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FormationRole {
    /// The member other members follow; losing it raises the leader-loss
    /// recovery trigger.
    Leader,
    /// A member that keeps station on the leader.
    Follower,
}

/// One declared formation member.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FormationMember {
    /// The member's slot index within the formation.
    pub slot: u32,
    /// Whether it leads.
    pub formation_role: FormationRole,
    /// The combat role it holds.
    pub role: DeclaredCombatRole,
}

/// What a formation does when one of its recovery triggers fires.
///
/// Declared per trigger, never invented per tick (non-negotiable 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecoveryPolicy {
    /// Fall back onto the surviving members and hold a defensive shape.
    Regroup,
    /// Promote another member to lead and continue.
    ReassignLead,
    /// Keep the current shape and do not re-task.
    HoldFormation,
    /// Leave the engagement entirely.
    Withdraw,
    /// Return to the declared route.
    ResumeRoute,
}

impl RecoveryPolicy {
    /// Every policy, in a stable order.
    pub const ALL: &'static [RecoveryPolicy] = &[
        Self::Regroup,
        Self::ReassignLead,
        Self::HoldFormation,
        Self::Withdraw,
        Self::ResumeRoute,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Regroup => "regroup",
            Self::ReassignLead => "reassign_lead",
            Self::HoldFormation => "hold_formation",
            Self::Withdraw => "withdraw",
            Self::ResumeRoute => "resume_route",
        }
    }
}

impl fmt::Display for RecoveryPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The declared recovery paths of one formation: one policy per trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FormationRecovery {
    /// What followers do when the leader is destroyed.
    pub leader_loss: RecoveryPolicy,
    /// What the formation does when its assigned target is destroyed.
    pub assigned_target_destroyed: RecoveryPolicy,
    /// What the formation does when its route is interrupted.
    pub route_interrupted: RecoveryPolicy,
    /// What an escort does when the actor it protects is destroyed.
    pub protected_actor_lost: RecoveryPolicy,
}

/// A declared formation: its leader, its members and its recovery paths.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredFormation {
    id: FormationId,
    leader_slot: u32,
    members: Vec<FormationMember>,
    recovery: FormationRecovery,
    provenance: Provenance,
}

impl DeclaredFormation {
    /// Assembles and validates a declared formation.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::FormationLeaderNotAMember`] when the leader slot
    /// is not one of `members`, and
    /// [`CombatSchemaError::DuplicateFormationMember`] when two members
    /// share a slot.
    pub fn try_new(
        id: FormationId,
        leader_slot: u32,
        members: Vec<FormationMember>,
        recovery: FormationRecovery,
        provenance: Provenance,
    ) -> Result<Self, CombatSchemaError> {
        if !members.iter().any(|member| member.slot == leader_slot) {
            return Err(CombatSchemaError::FormationLeaderNotAMember {
                formation: id,
                leader_slot,
            });
        }
        let mut slots = BTreeSet::new();
        for member in &members {
            if !slots.insert(member.slot) {
                return Err(CombatSchemaError::DuplicateFormationMember {
                    formation: id,
                    slot: member.slot,
                });
            }
        }
        Ok(Self {
            id,
            leader_slot,
            members,
            recovery,
            provenance,
        })
    }

    /// The formation's identity.
    #[must_use]
    pub const fn id(&self) -> FormationId {
        self.id
    }

    /// The slot that leads the formation.
    #[must_use]
    pub const fn leader_slot(&self) -> u32 {
        self.leader_slot
    }

    /// The declared members, in authored order.
    #[must_use]
    pub fn members(&self) -> &[FormationMember] {
        &self.members
    }

    /// The declared recovery paths.
    #[must_use]
    pub const fn recovery(&self) -> &FormationRecovery {
        &self.recovery
    }

    /// Where the record came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// ------------------------------------------- original surface ----

/// The retail member that holds the engine's own resource header, and
/// therefore every string-id block the shipped UI is built from.
///
/// Re-used from [`crate::weapons`] rather than re-spelled, so the F32-D
/// measurement and the F27/F28 ones name one member.
pub use crate::weapons::ORIGINAL_RESOURCE_HEADER;

/// The macro that opens the original's **campaign difficulty option** block.
///
/// **Measured** in [`ORIGINAL_RESOURCE_HEADER`], together with
/// [`ORIGINAL_DIFFICULTY_OPTION_BOUND_MACRO`].
pub const ORIGINAL_DIFFICULTY_OPTION_MACRO: &str = "IDS_DIFFICULTY";

/// The id [`ORIGINAL_DIFFICULTY_OPTION_MACRO`] declares.
pub const ORIGINAL_DIFFICULTY_OPTION_FIRST_ID: u32 = 109;

/// The first macro the resource header declares **after** the difficulty
/// option, with its id.
///
/// **Measured**: this is what bounds the difficulty block. The rule it applies
/// — *a block of option labels runs from its macro's id up to the next
/// declared macro's id, exclusive* — is not invented here: it is the same rule
/// [`crate::ordnance::ORIGINAL_NEXT_ROCKET_BLOCK`] measured for the rocket
/// name blocks (a 15-id run) and `IDS_AIRFRAMEGUNGROUPNAMES` measured for the
/// gun-group block (a 20-id run).
pub const ORIGINAL_DIFFICULTY_OPTION_BOUND_MACRO: &str = "IDS_VIEWCOCKPIT";

/// The id [`ORIGINAL_DIFFICULTY_OPTION_BOUND_MACRO`] declares.
pub const ORIGINAL_DIFFICULTY_OPTION_BOUND_ID: u32 = 112;

/// How many steps the original's campaign difficulty option offers.
///
/// **Derived**, never hand-typed: it is the gap between the two measured ids,
/// so a corrected measurement changes this constant instead of leaving a
/// stale count beside a fresh span.
///
/// **Measured**: **three**. The block is ids `109..=111` and the shipped
/// string image populates **all three** — see
/// [`ORIGINAL_DIFFICULTY_OPTION_IDS`].
pub const ORIGINAL_DIFFICULTY_STEPS: u32 =
    ORIGINAL_DIFFICULTY_OPTION_BOUND_ID - ORIGINAL_DIFFICULTY_OPTION_FIRST_ID;

/// The ids the original's campaign difficulty option occupies, in option order.
///
/// **Measured**: `109..=111`, one per step. Each id carries exactly one
/// non-empty string in the shipped English string image
/// (`GOSDATA/ASSETS/BINARIES/langui.dll`, language 1033), so the block is
/// three *usable* steps and not three reserved slots.
///
/// The text behind those ids is the original's localizable display text and is
/// **not** reproduced here, here or in any committed finding (AGENTS rule 3):
/// the ids, their count and their occupancy are the measurement.
pub const ORIGINAL_DIFFICULTY_OPTION_IDS: [u32; ORIGINAL_DIFFICULTY_STEPS as usize] =
    [109, 110, 111];

/// The macro and id of the difficulty row's **title** on the game-options
/// screen, and of its **description**.
///
/// **Measured**: both are single ids — the next declared macro follows each
/// immediately (`IDS_GO_VIEW_TITLE` 1085, `IDS_GO_VIEW_DESC` 1088) — so the
/// difficulty option is **one row of one screen**, a selector with a label and
/// a help line, not a list of per-objective difficulty records.
pub const ORIGINAL_GAME_OPTION_DIFFICULTY_TITLE: (&str, u32) = ("IDS_GO_DIFFICULTY_TITLE", 1084);

/// The macro and id of the difficulty row's description on the game-options
/// screen.
pub const ORIGINAL_GAME_OPTION_DIFFICULTY_DESC: (&str, u32) = ("IDS_GO_DIFFICULTY_DESC", 1087);

/// The macro and id the instant-action story screens use for a difficulty
/// **label**.
///
/// **Measured**: a single id (`IDS_IA_PLANES` 3700 follows it), and it is
/// absent from every measured instant-action scenario descriptor — see
/// [`ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO`].
pub const ORIGINAL_IA_DIFFICULTY_LABEL: (&str, u32) = ("IDS_IA_DIFFICULTY", 3695);

/// Whether any measured per-scenario record carries the difficulty.
///
/// **Measured**: **no**. The complete key vocabulary of all eight
/// instant-action scenario descriptors — every key at **any** depth, not only
/// the root ones, so a nested group record is covered too — contains no
/// difficulty key, and the option itself is a screen row with a title and a
/// description. So the original's difficulty is a *selection*, not a value any
/// mission record stores: which step is in force is the player's choice at the
/// options screen, and nothing in the measured data binds a step to a mission.
///
/// This is the load-bearing negative of F32-D: it is why no
/// [`DifficultyProfile`] may claim to be the original's per-mission
/// difficulty, and why [`DeclaredDifficultyOrigin`] distinguishes a *selected*
/// option from a *recorded* one.
pub const ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO: bool = false;

/// One AI skill tier, as the original's scenario descriptors spell it.
///
/// **Measured** (F32-D) over the eight instant-action scenario descriptors of
/// the owner's installation: **forty** skill labels across **thirty-two**
/// enemy groups and **eight** named aces, and the label vocabulary is exactly
/// these three. A label outside them is refused rather than carried, because a
/// fourth tier would be a designed alternative with no measured spelling.
///
/// This is the **per-aircraft** tier (`enemy_skill`, `ace_skill`) and is a
/// different thing from [`ORIGINAL_DIFFICULTY_STEPS`], which is the count of
/// steps in the *player's* difficulty option. They happen to be three each;
/// nothing in the measured data says the game maps one onto the other, so
/// nothing here does either.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DeclaredSkillTier {
    /// The lowest measured tier.
    Novice,
    /// The middle measured tier.
    Veteran,
    /// The highest measured tier, the one the scenario's own named ace holds.
    Ace,
}

impl DeclaredSkillTier {
    /// Every measured tier, from lowest to highest.
    pub const ALL: &'static [DeclaredSkillTier] = &[Self::Novice, Self::Veteran, Self::Ace];

    /// The stable label, which is also the original's own spelling.
    ///
    /// **Measured**: these three are the label vocabulary of `enemy_skill`
    /// and `ace_skill` in the eight measured scenario descriptors.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Novice => "novice",
            Self::Veteran => "veteran",
            Self::Ace => "ace",
        }
    }

    /// The measured tier a label names, or `None` for a label the installation
    /// never spells.
    ///
    /// The lookup is exact and case-sensitive because the measured corpus is:
    /// every one of the forty measured labels is one of [`Self::ALL`]'s
    /// spellings, byte for byte. A label this returns `None` for is a content
    /// change or a new installation, and must be measured — not normalized
    /// into the nearest tier.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|tier| tier.label() == label)
    }
}

impl fmt::Display for DeclaredSkillTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// How many skill labels the measured scenario descriptors declare.
///
/// **Measured**: thirty-two enemy groups plus eight named aces.
pub const ORIGINAL_SKILL_LABEL_COUNT: u32 = 40;

/// How many enemy groups the measured scenario descriptors declare.
///
/// **Measured**: four groups in each of eight descriptors.
pub const ORIGINAL_ENEMY_GROUP_COUNT: u32 = 32;

/// How many instant-action scenario descriptors the measured installation
/// carries.
///
/// **Measured**: one per world group, each an `IA1` reader archive's `ia.zrd`.
pub const ORIGINAL_SCENARIO_DESCRIPTOR_COUNT: u32 = 8;

/// How many integer slots the original's declared ace stat vector has.
///
/// **Measured**: `ace_stats` is a nine-element integer list in all eight
/// descriptors, and every slot is [`ORIGINAL_ACE_STAT_MAX`] in all eight.
///
/// This is a *count and an extent*, deliberately not a meaning: no measured
/// file names what the nine slots are, what order they are in, or what a slot
/// below nine does. A reimplementation may therefore use a nine-slot vector as
/// the shape of its ace record and must not claim what any slot controls.
pub const ORIGINAL_ACE_STAT_SLOTS: usize = 9;

/// The largest value any measured `ace_stats` slot carries.
///
/// **Measured**: 9, in every slot of every one of the eight descriptors. The
/// original ships no descriptor with a slot below this, so the vector's
/// *interior* — whether any tier below an ace exists in the data at all — is
/// unmeasured and stays unmeasured.
pub const ORIGINAL_ACE_STAT_MAX: i64 = 9;

/// Where a declared [`DifficultyProfile`]'s tier came from, given what the
/// original's option actually is.
///
/// **Measured** (F32-D): the original's difficulty option is a screen row with
/// three steps and **no** measured per-mission difficulty record
/// ([`ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO`]). A reimplementation
/// therefore has to say which of those two worlds a declared profile belongs
/// to, and the two are not interchangeable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredDifficultyOrigin {
    /// The tier stands for one of the original option's measured steps, chosen
    /// by the player. Its knobs are **designed**: the original's data names
    /// the step but never says what any step changes.
    SelectedOptionStep {
        /// The measured step, `0..ORIGINAL_DIFFICULTY_STEPS`.
        step: u32,
    },
    /// The tier is a project extension with no measured counterpart: this
    /// engine offers a step the original's option does not, which is allowed
    /// by non-negotiable 1 as an explicitly designed alternative and may not be
    /// presented as the original's.
    DesignedExtension,
}

// ------------------------------------------------ difficulty ----

/// A declared difficulty tier.
///
/// A designed four-step ordering. **Measured** (F32-D) against the owner's
/// installation, the original's campaign difficulty option offers
/// [`ORIGINAL_DIFFICULTY_STEPS`] steps, so this vocabulary has one more step
/// than the original's option; [`Self::measured_step`] says which steps
/// correspond and [`Self::is_designed_extension`] names the one that does not.
/// The original's own step *names* live in its shipped localizable string
/// image and are not reproduced here (AGENTS rule 3), so these labels remain
/// project design and claim nothing about the original's wording.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DifficultyTier {
    /// The most forgiving declared tier.
    Relaxed,
    /// The declared baseline tier.
    Standard,
    /// A harder declared tier.
    Hard,
    /// The most demanding declared tier.
    Elite,
}

impl DifficultyTier {
    /// Every tier, from most forgiving to most demanding.
    pub const ALL: &'static [DifficultyTier] =
        &[Self::Relaxed, Self::Standard, Self::Hard, Self::Elite];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Relaxed => "relaxed",
            Self::Standard => "standard",
            Self::Hard => "hard",
            Self::Elite => "elite",
        }
    }

    /// This tier's position in [`Self::ALL`], from `0`.
    #[must_use]
    pub const fn index(self) -> u32 {
        match self {
            Self::Relaxed => 0,
            Self::Standard => 1,
            Self::Hard => 2,
            Self::Elite => 3,
        }
    }

    /// The measured original option step this designed tier stands for, when
    /// it stands for one.
    ///
    /// **Measured** ([`ORIGINAL_DIFFICULTY_STEPS`] = 3) and **positional**:
    /// [`Self::ALL`] runs most forgiving to most demanding and the measured
    /// option runs the same way, so the first `ORIGINAL_DIFFICULTY_STEPS`
    /// tiers correspond to the measured steps in order and any tier past them
    /// has no measured counterpart.
    ///
    /// The mapping says nothing about the original's *names* and nothing about
    /// what any step changes: no measured file records that, so a caller that
    /// needs it must say so with a [`DeclaredDifficultyOrigin`].
    #[must_use]
    pub const fn measured_step(self) -> Option<u32> {
        let step = self.index();
        if step < ORIGINAL_DIFFICULTY_STEPS {
            Some(step)
        } else {
            None
        }
    }

    /// Whether this tier is a project extension past the original option's
    /// measured steps.
    #[must_use]
    pub const fn is_designed_extension(self) -> bool {
        self.measured_step().is_none()
    }

    /// How many declared tiers correspond to a measured original step.
    ///
    /// Computed, not written down, so the count follows
    /// [`ORIGINAL_DIFFICULTY_STEPS`] when a measurement is corrected.
    #[must_use]
    pub const fn measured_tier_count() -> usize {
        let mut count = 0;
        let mut index = 0;
        while index < Self::ALL.len() {
            if Self::ALL[index].measured_step().is_some() {
                count += 1;
            }
            index += 1;
        }
        count
    }
}

impl fmt::Display for DifficultyTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A declared difficulty profile: a tier plus the behavior knobs it moves.
///
/// The type cannot express a simulation rate, a tick rate, a time scale or
/// a damage multiplier, so F32 non-negotiable 1 — "never increase
/// simulation speed to fake difficulty" — holds by construction rather than
/// by review. Each override's [`Provenance`] names whether the change is an
/// evidence-backed parameter or an explicitly designed alternative.
#[derive(Clone, Debug, PartialEq)]
pub struct DifficultyProfile {
    tier: DifficultyTier,
    overrides: Vec<SkillKnobOverride>,
    provenance: Provenance,
}

impl DifficultyProfile {
    /// Assembles and validates a difficulty profile.
    ///
    /// An empty override list is valid and means "this tier runs the
    /// declared role profiles unchanged" — the baseline tier, stated
    /// explicitly rather than left missing.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::DuplicateKnob`] when two overrides name the
    /// same knob.
    pub fn try_new(
        tier: DifficultyTier,
        overrides: Vec<SkillKnobOverride>,
        provenance: Provenance,
    ) -> Result<Self, CombatSchemaError> {
        check_unique_knobs(&overrides)?;
        Ok(Self {
            tier,
            overrides,
            provenance,
        })
    }

    /// The declared tier.
    #[must_use]
    pub const fn tier(&self) -> DifficultyTier {
        self.tier
    }

    /// The declared overrides, in authored order.
    #[must_use]
    pub fn overrides(&self) -> &[SkillKnobOverride] {
        &self.overrides
    }

    /// Where the record came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// Refuses two overrides of the same knob.
fn check_unique_knobs(overrides: &[SkillKnobOverride]) -> Result<(), CombatSchemaError> {
    let mut seen = BTreeSet::new();
    for change in overrides {
        if !seen.insert(change.knob) {
            return Err(CombatSchemaError::DuplicateKnob { knob: change.knob });
        }
    }
    Ok(())
}

// -------------------------------------------------------- errors ----

/// Why a declared combat record was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CombatSchemaError {
    /// The rules' subject is neither a mission nor a launchable scenario.
    SubjectKind {
        /// The offending subject id.
        subject: ContentId,
    },
    /// A role is declared more than once.
    DuplicateRole {
        /// The repeated role.
        role: DeclaredCombatRole,
    },
    /// A formation member or ace variant names a role the subject never
    /// declared.
    UndeclaredRole {
        /// The undeclared role.
        role: DeclaredCombatRole,
    },
    /// A role was declared with an arsenal that cannot carry it: a bomber
    /// or torpedo run with no ordnance requirement would declare a role its
    /// own actor can never execute.
    RoleArsenalMissing {
        /// The role.
        role: DeclaredCombatRole,
    },
    /// An ace variant's id is not in the `pilot` namespace.
    AceKindMismatch {
        /// The offending id.
        id: ContentId,
    },
    /// An ace variant is declared more than once.
    DuplicateAce {
        /// The repeated id.
        id: ContentId,
    },
    /// A formation is declared more than once.
    DuplicateFormation {
        /// The repeated formation.
        formation: FormationId,
    },
    /// A formation's leader slot is not one of its members.
    FormationLeaderNotAMember {
        /// The formation.
        formation: FormationId,
        /// The leader slot that is not a member.
        leader_slot: u32,
    },
    /// Two formation members share one slot.
    DuplicateFormationMember {
        /// The formation.
        formation: FormationId,
        /// The repeated slot.
        slot: u32,
    },
    /// A difficulty tier is declared more than once.
    DuplicateDifficultyTier {
        /// The repeated tier.
        tier: DifficultyTier,
    },
    /// Two overrides of one record name the same knob.
    DuplicateKnob {
        /// The repeated knob.
        knob: SkillKnob,
    },
    /// An override's value carries another unit than its knob.
    KnobUnitMismatch {
        /// The knob.
        knob: SkillKnob,
        /// The unit the knob is expressed in.
        expected: SkillKnobUnit,
        /// The unit the value carried.
        found: SkillKnobUnit,
    },
    /// A knob value was NaN or infinite.
    NonFiniteKnob {
        /// The knob.
        knob: SkillKnob,
    },
    /// A knob value fell outside its approved range.
    KnobOutOfRange {
        /// The knob.
        knob: SkillKnob,
        /// The rejected value.
        value: String,
    },
    /// A skill-knob override named a priority weight.
    NotASkillKnob {
        /// The offending knob.
        knob: SkillKnob,
    },
    /// A priority-policy override named a non-weight knob.
    NotAPriorityWeight {
        /// The offending knob.
        knob: SkillKnob,
    },
    /// A record asked for a knob it does not carry.
    UnknownKnob {
        /// The requested knob.
        knob: SkillKnob,
    },
    /// Every priority weight is known and zero, so the policy scores
    /// nothing and the decision would be an artifact of the tie-break.
    NoScoredPriorityTerm,
}

impl fmt::Display for CombatSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectKind { subject } => {
                write!(
                    f,
                    "combat subject {subject} is neither a mission nor a launchable"
                )
            }
            Self::DuplicateRole { role } => write!(f, "role {role} is declared more than once"),
            Self::UndeclaredRole { role } => {
                write!(f, "role {role} is used but never declared")
            }
            Self::RoleArsenalMissing { role } => {
                write!(f, "role {role} is declared without the ordnance it needs")
            }
            Self::AceKindMismatch { id } => {
                write!(f, "ace variant id {id} is not in the pilot namespace")
            }
            Self::DuplicateAce { id } => write!(f, "ace variant {id} is declared more than once"),
            Self::DuplicateFormation { formation } => {
                write!(f, "{formation} is declared more than once")
            }
            Self::FormationLeaderNotAMember {
                formation,
                leader_slot,
            } => write!(
                f,
                "{formation} names slot {leader_slot} as its leader, which is not a member"
            ),
            Self::DuplicateFormationMember { formation, slot } => {
                write!(f, "{formation} has more than one member in slot {slot}")
            }
            Self::DuplicateDifficultyTier { tier } => {
                write!(f, "difficulty tier {tier} is declared more than once")
            }
            Self::DuplicateKnob { knob } => {
                write!(f, "knob {knob} is overridden more than once in one record")
            }
            Self::KnobUnitMismatch {
                knob,
                expected,
                found,
            } => write!(f, "knob {knob} is expressed in {expected}, not in {found}"),
            Self::NonFiniteKnob { knob } => write!(f, "knob {knob} must be a finite number"),
            Self::KnobOutOfRange { knob, value } => {
                write!(f, "knob {knob} value {value} is outside its approved range")
            }
            Self::NotASkillKnob { knob } => {
                write!(f, "knob {knob} is a priority weight, not a skill knob")
            }
            Self::NotAPriorityWeight { knob } => {
                write!(f, "knob {knob} is a skill knob, not a priority weight")
            }
            Self::UnknownKnob { knob } => write!(f, "knob {knob} is not carried by this record"),
            Self::NoScoredPriorityTerm => write!(
                f,
                "every priority weight is zero, so the policy would score no candidate"
            ),
        }
    }
}

impl std::error::Error for CombatSchemaError {}

// ------------------------------------------------------ records ----

/// One declared role: its arsenal requirement, its knobs and its priority
/// policy.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredRoleProfile {
    role: DeclaredCombatRole,
    arsenal: RoleArsenal,
    knobs: SkillKnobs,
    priority: PriorityPolicy,
    origin: Origin,
    provenance: Provenance,
}

impl DeclaredRoleProfile {
    /// Assembles and validates a declared role profile.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::NonFiniteKnob`], [`CombatSchemaError::KnobOutOfRange`]
    /// or [`CombatSchemaError::NoScoredPriorityTerm`] from the knobs and the
    /// policy.
    pub fn try_new(
        role: DeclaredCombatRole,
        arsenal: RoleArsenal,
        knobs: SkillKnobs,
        priority: PriorityPolicy,
        origin: Origin,
        provenance: Provenance,
    ) -> Result<Self, CombatSchemaError> {
        knobs.validate()?;
        priority.validate()?;
        if role.needs_ordnance() && !arsenal.ordnance {
            return Err(CombatSchemaError::RoleArsenalMissing { role });
        }
        Ok(Self {
            role,
            arsenal,
            knobs,
            priority,
            origin,
            provenance,
        })
    }

    /// The role.
    #[must_use]
    pub const fn role(&self) -> DeclaredCombatRole {
        self.role
    }

    /// What the role is allowed to shoot with.
    #[must_use]
    pub const fn arsenal(&self) -> RoleArsenal {
        self.arsenal
    }

    /// The declared knobs.
    #[must_use]
    pub const fn knobs(&self) -> &SkillKnobs {
        &self.knobs
    }

    /// The declared priority policy.
    #[must_use]
    pub const fn priority(&self) -> &PriorityPolicy {
        &self.priority
    }

    /// Where the role came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Applies one override, routing it to the knobs or to the policy by
    /// its knob.
    ///
    /// # Errors
    ///
    /// The validation errors of [`SkillKnobs::apply`] and
    /// [`PriorityPolicy::apply`].
    pub fn apply(&mut self, change: &SkillKnobOverride) -> Result<(), CombatSchemaError> {
        match change.knob.unit() {
            SkillKnobUnit::Weight => self.priority.apply(change),
            _ => self.knobs.apply(change),
        }
    }
}

/// The declared combat-AI rules of one catalog subject.
///
/// `subject` is the catalog id the rules belong to — a `mission` id for
/// per-mission rules, an `ia_scenario` id for instant-action rules — so
/// the record shares the catalog's identity discipline.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCombatRules {
    subject: ContentId,
    origin: Origin,
    roles: Vec<DeclaredRoleProfile>,
    aces: Vec<DeclaredAceProfile>,
    formations: Vec<DeclaredFormation>,
    difficulties: Vec<DifficultyProfile>,
    provenance: Provenance,
}

impl DeclaredCombatRules {
    /// Assembles and validates a declared combat rules record.
    ///
    /// # Errors
    ///
    /// [`CombatSchemaError::SubjectKind`] for a subject that is neither a
    /// mission nor a launchable scenario,
    /// [`CombatSchemaError::DuplicateRole`] for a role declared twice,
    /// [`CombatSchemaError::UndeclaredRole`] for a formation member or ace
    /// variant naming an undeclared role, [`CombatSchemaError::DuplicateAce`],
    /// [`CombatSchemaError::DuplicateFormation`] and
    /// [`CombatSchemaError::DuplicateDifficultyTier`] for a repeated id, tier
    /// or formation, plus the validation errors of the nested records.
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        roles: Vec<DeclaredRoleProfile>,
        aces: Vec<DeclaredAceProfile>,
        formations: Vec<DeclaredFormation>,
        difficulties: Vec<DifficultyProfile>,
        provenance: Provenance,
    ) -> Result<Self, CombatSchemaError> {
        if subject.kind() != ContentKind::Mission && subject.kind() != ContentKind::IaScenario {
            return Err(CombatSchemaError::SubjectKind { subject });
        }

        let mut declared_roles = BTreeSet::new();
        for profile in &roles {
            if !declared_roles.insert(profile.role()) {
                return Err(CombatSchemaError::DuplicateRole {
                    role: profile.role(),
                });
            }
        }

        let mut ace_ids = BTreeSet::new();
        for ace in &aces {
            if !declared_roles.contains(&ace.base_role()) {
                return Err(CombatSchemaError::UndeclaredRole {
                    role: ace.base_role(),
                });
            }
            if !ace_ids.insert(ace.id().clone()) {
                return Err(CombatSchemaError::DuplicateAce {
                    id: ace.id().clone(),
                });
            }
        }

        let mut formation_ids = BTreeSet::new();
        for formation in &formations {
            for member in formation.members() {
                if !declared_roles.contains(&member.role) {
                    return Err(CombatSchemaError::UndeclaredRole { role: member.role });
                }
            }
            if !formation_ids.insert(formation.id()) {
                return Err(CombatSchemaError::DuplicateFormation {
                    formation: formation.id(),
                });
            }
        }

        let mut tiers = BTreeSet::new();
        for profile in &difficulties {
            if !tiers.insert(profile.tier()) {
                return Err(CombatSchemaError::DuplicateDifficultyTier {
                    tier: profile.tier(),
                });
            }
        }

        Ok(Self {
            subject,
            origin,
            roles,
            aces,
            formations,
            difficulties,
            provenance,
        })
    }

    /// The catalog id the rules belong to.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The declared roles, in authored order.
    #[must_use]
    pub fn roles(&self) -> &[DeclaredRoleProfile] {
        &self.roles
    }

    /// The declared role with the given name, if the subject declares it.
    #[must_use]
    pub fn role(&self, role: DeclaredCombatRole) -> Option<&DeclaredRoleProfile> {
        self.roles.iter().find(|profile| profile.role() == role)
    }

    /// The declared ace variants, in authored order.
    #[must_use]
    pub fn aces(&self) -> &[DeclaredAceProfile] {
        &self.aces
    }

    /// The declared formations, in authored order.
    #[must_use]
    pub fn formations(&self) -> &[DeclaredFormation] {
        &self.formations
    }

    /// The declared difficulty profiles, in authored order.
    #[must_use]
    pub fn difficulties(&self) -> &[DifficultyProfile] {
        &self.difficulties
    }

    /// The difficulty profile of one tier, if the subject declares it.
    #[must_use]
    pub fn difficulty(&self, tier: DifficultyTier) -> Option<&DifficultyProfile> {
        self.difficulties
            .iter()
            .find(|profile| profile.tier() == tier)
    }

    /// Where the record itself came from.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

// -------------------------------------------------------- fixture ----

/// The claim id the synthetic fixture's values carry.
pub const SYNTHETIC_COMBAT_CLAIM: &str = "f32a.synthetic-escort-sortie";

/// The synthetic fixture's claim id.
#[must_use]
pub fn synthetic_combat_claim() -> ClaimId {
    ClaimId::new(SYNTHETIC_COMBAT_CLAIM).expect("the synthetic claim id is valid")
}

fn known<T>(value: T) -> Resolved<T> {
    resolved_with(value, &Provenance::designed(synthetic_combat_claim()))
}

/// The synthetic escort role's declared knobs: a 24-tick reaction, a
/// 3.4-degree aim error, a 1.5 km engagement range and a 30-tick fire
/// discipline. Designed fixture values, not original data.
#[must_use]
pub fn declared_synthetic_escort_knobs() -> SkillKnobs {
    SkillKnobs {
        reaction_ticks: known(24),
        aim_error_rad: known(Radians(0.06)),
        engagement_range_m: known(Meters(1_500.0)),
        fire_discipline_ticks: known(30),
    }
}

/// The synthetic escort role's declared priority policy: the protected
/// actor's threat outweighs proximity, so an escort answers what endangers
/// its charge rather than whatever is closest.
///
/// Designed fixture values, not original data.
#[must_use]
pub fn declared_synthetic_escort_policy() -> PriorityPolicy {
    PriorityPolicy {
        protected_actor_weight: known(2.0),
        objective_weight: known(1.0),
        self_defense_weight: known(1.5),
        proximity_weight: known(0.5),
        threat_window_ticks: known(120),
    }
}

/// The synthetic fighter-attack role: guns, a 400 m range and a policy that
/// prefers the script-assigned objective over the nearest hostile.
#[must_use]
pub fn declared_synthetic_fighter_policy() -> PriorityPolicy {
    PriorityPolicy {
        protected_actor_weight: known(0.0),
        objective_weight: known(4.0),
        self_defense_weight: known(1.0),
        proximity_weight: known(1.0),
        threat_window_ticks: known(90),
    }
}

/// The synthetic fighter-attack role's declared knobs.
#[must_use]
pub fn declared_synthetic_fighter_knobs() -> SkillKnobs {
    SkillKnobs {
        reaction_ticks: known(18),
        aim_error_rad: known(Radians(0.04)),
        engagement_range_m: known(Meters(400.0)),
        fire_discipline_ticks: known(24),
    }
}

/// The declared synthetic ace variant: the escort role with a 6-tick
/// reaction, a 0.86-degree aim error and a doubled protected-actor weight.
///
/// Every override is a *behavior*; the variant has no damage, armor or
/// health field at all, so it cannot be an inflated-health ace.
#[must_use]
pub fn declared_synthetic_ace_profile() -> DeclaredAceProfile {
    let provenance = Provenance::designed(synthetic_combat_claim());
    let overrides = [
        (SkillKnob::ReactionTicks, SkillKnobValue::Ticks(6)),
        (SkillKnob::AimErrorRad, SkillKnobValue::Angle(0.015)),
        (SkillKnob::ProtectedActorWeight, SkillKnobValue::Weight(4.0)),
    ]
    .into_iter()
    .map(|(knob, value)| {
        SkillKnobOverride::try_new(knob, value, provenance.clone())
            .expect("the declared synthetic ace override is valid")
    })
    .collect();
    DeclaredAceProfile::try_new(
        ContentId::from_source(ContentKind::Pilot, "synthetic.ace-wing-leader")
            .expect("the fixture ace id is valid"),
        DeclaredCombatRole::Escort,
        overrides,
        Origin::SyntheticFixture,
        provenance,
    )
    .expect("the declared synthetic ace variant is valid")
}

/// The declared synthetic formation: a leader plus two followers, with one
/// recovery policy per trigger.
#[must_use]
pub fn declared_synthetic_formation() -> DeclaredFormation {
    DeclaredFormation::try_new(
        FormationId(1),
        0,
        vec![
            FormationMember {
                slot: 0,
                formation_role: FormationRole::Leader,
                role: DeclaredCombatRole::FighterAttack,
            },
            FormationMember {
                slot: 1,
                formation_role: FormationRole::Follower,
                role: DeclaredCombatRole::Escort,
            },
            FormationMember {
                slot: 2,
                formation_role: FormationRole::Follower,
                role: DeclaredCombatRole::Escort,
            },
        ],
        FormationRecovery {
            leader_loss: RecoveryPolicy::ReassignLead,
            assigned_target_destroyed: RecoveryPolicy::Regroup,
            route_interrupted: RecoveryPolicy::ResumeRoute,
            protected_actor_lost: RecoveryPolicy::Regroup,
        },
        Provenance::designed(synthetic_combat_claim()),
    )
    .expect("the declared synthetic formation is valid")
}

/// The declared synthetic difficulty profiles: the baseline tier states
/// that it changes nothing, and the top tier moves only the reaction, aim
/// error and protected-actor weight.
#[must_use]
pub fn declared_synthetic_difficulty_profiles() -> Vec<DifficultyProfile> {
    let provenance = Provenance::designed(synthetic_combat_claim());
    let elite = [
        (SkillKnob::ReactionTicks, SkillKnobValue::Ticks(6)),
        (SkillKnob::AimErrorRad, SkillKnobValue::Angle(0.015)),
        (SkillKnob::ProtectedActorWeight, SkillKnobValue::Weight(4.0)),
    ]
    .into_iter()
    .map(|(knob, value)| {
        SkillKnobOverride::try_new(knob, value, provenance.clone())
            .expect("the declared synthetic difficulty override is valid")
    })
    .collect();
    vec![
        DifficultyProfile::try_new(DifficultyTier::Standard, Vec::new(), provenance.clone())
            .expect("the baseline difficulty profile is valid"),
        DifficultyProfile::try_new(DifficultyTier::Elite, elite, provenance)
            .expect("the elite difficulty profile is valid"),
    ]
}

/// The minimal synthetic combat rules fixture: an escort-sortie subject
/// with two roles, one ace variant, one formation and two difficulty tiers.
///
/// Newly authored project design with `Origin::SyntheticFixture` and
/// designed provenance. It can never stand in for retail content: the
/// original's role set, priority order and difficulty mapping are
/// unmeasured (F32-D).
#[must_use]
pub fn declared_synthetic_combat_rules() -> DeclaredCombatRules {
    let provenance = Provenance::designed(synthetic_combat_claim());
    let roles = vec![
        DeclaredRoleProfile::try_new(
            DeclaredCombatRole::Escort,
            RoleArsenal::guns(),
            declared_synthetic_escort_knobs(),
            declared_synthetic_escort_policy(),
            Origin::SyntheticFixture,
            provenance.clone(),
        )
        .expect("the synthetic escort role is valid"),
        DeclaredRoleProfile::try_new(
            DeclaredCombatRole::FighterAttack,
            RoleArsenal::guns(),
            declared_synthetic_fighter_knobs(),
            declared_synthetic_fighter_policy(),
            Origin::SyntheticFixture,
            provenance.clone(),
        )
        .expect("the synthetic fighter role is valid"),
        DeclaredRoleProfile::try_new(
            DeclaredCombatRole::BomberRun,
            RoleArsenal::guns_and_ordnance(),
            declared_synthetic_fighter_knobs(),
            declared_synthetic_fighter_policy(),
            Origin::SyntheticFixture,
            provenance.clone(),
        )
        .expect("the synthetic bomber role is valid"),
    ];
    DeclaredCombatRules::try_new(
        ContentId::from_source(ContentKind::Mission, "synthetic.escort-sortie")
            .expect("the fixture subject id is valid"),
        Origin::SyntheticFixture,
        roles,
        vec![declared_synthetic_ace_profile()],
        vec![declared_synthetic_formation()],
        declared_synthetic_difficulty_profiles(),
        provenance,
    )
    .expect("the declared synthetic combat rules fixture is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::content::Known;

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("valid claim id")
    }

    fn designed() -> Provenance {
        Provenance::designed(claim("f32a.unit"))
    }

    fn pilot(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Pilot, key).expect("valid pilot id")
    }

    fn override_of(knob: SkillKnob, value: SkillKnobValue) -> SkillKnobOverride {
        SkillKnobOverride::try_new(knob, value, designed()).expect("the unit override is valid")
    }

    /// The declared synthetic rules are a synthetic, designed record: an
    /// escort-sortie mission with the three roles the fixture needs, one
    /// behavior-only ace variant, one formation with four declared recovery
    /// paths and two difficulty tiers.
    #[test]
    fn accept_f32_a_declared_synthetic_rules_are_synthetic_and_complete() {
        let rules = declared_synthetic_combat_rules();
        assert_eq!(rules.subject().as_str(), "mission/synthetic.escort-sortie");
        assert_eq!(rules.origin(), &Origin::SyntheticFixture);
        assert!(!rules.origin().is_original());
        assert_eq!(rules.roles().len(), 3);
        assert!(rules.role(DeclaredCombatRole::Escort).is_some());
        assert!(rules.role(DeclaredCombatRole::BomberRun).is_some());
        assert!(rules.role(DeclaredCombatRole::Intercept).is_none());

        let escort = rules
            .role(DeclaredCombatRole::Escort)
            .expect("the fixture declares an escort role");
        assert_eq!(escort.arsenal(), RoleArsenal::guns());
        assert_eq!(
            escort
                .priority()
                .known_weight(PriorityTerm::ProtectedActorThreat),
            Some(2.0)
        );
        assert!(
            escort
                .priority()
                .known_weight(PriorityTerm::ProtectedActorThreat)
                > escort.priority().known_weight(PriorityTerm::Proximity),
            "the declared escort policy weighs the charge's threat above proximity"
        );
        assert_eq!(escort.knobs().reaction_ticks.clone().known(), Some(24));
        assert_eq!(
            escort.knobs().engagement_range_m.clone().known(),
            Some(Meters(1_500.0))
        );
        assert_eq!(
            escort.priority().threat_window_ticks.clone().known(),
            Some(120)
        );

        // Every declared value carries provenance, so a lowered profile can
        // say which claim produced it.
        assert_eq!(
            escort
                .knobs()
                .reaction_ticks
                .provenance()
                .map(|provenance| provenance.claim_id.clone()),
            Some(synthetic_combat_claim())
        );

        assert_eq!(rules.aces().len(), 1);
        assert_eq!(rules.formations().len(), 1);
        assert_eq!(rules.difficulties().len(), 2);
        assert!(rules.difficulty(DifficultyTier::Standard).is_some());
        assert!(rules.difficulty(DifficultyTier::Elite).is_some());
        assert!(rules.difficulty(DifficultyTier::Hard).is_none());

        // The baseline tier states that it changes nothing rather than
        // being left missing.
        assert!(
            rules
                .difficulty(DifficultyTier::Standard)
                .expect("the baseline tier is declared")
                .overrides()
                .is_empty()
        );
    }

    /// A difficulty profile can only move behavior knobs. The knob
    /// vocabulary is the enforcement of F32 non-negotiable 1: there is no
    /// simulation-rate, time-scale, tick-rate, damage, armor or health
    /// variant, so difficulty cannot be faked by speeding the simulation
    /// up and an ace cannot be inflated health.
    #[test]
    fn accept_f32_a_difficulty_profile_moves_only_evidence_backed_knobs() {
        let labels: Vec<&str> = SkillKnob::ALL.iter().map(|knob| knob.label()).collect();
        assert_eq!(labels.len(), 9, "the knob vocabulary is closed");
        for forbidden in [
            "simulation_speed",
            "tick_rate",
            "time_scale",
            "physics_rate",
            "ai_tick_divisor",
            "damage_scale",
            "damage_taken_scale",
            "player_health",
            "enemy_health",
            "armor_scale",
        ] {
            assert_eq!(
                SkillKnob::from_label(forbidden),
                None,
                "{forbidden} must not be expressible as a skill knob"
            );
            assert!(
                !labels.contains(&forbidden),
                "{forbidden} is not part of the knob vocabulary"
            );
        }
        for knob in SkillKnob::ALL {
            assert_eq!(
                SkillKnob::from_label(knob.label()),
                Some(*knob),
                "label and lookup cannot disagree about one knob"
            );
        }

        // A knob carries a unit, and an override whose value is in another
        // unit is refused rather than reinterpreted.
        assert_eq!(SkillKnob::AimErrorRad.unit(), SkillKnobUnit::Angle);
        assert_eq!(SkillKnob::EngagementRangeM.unit(), SkillKnobUnit::Distance);
        assert_eq!(SkillKnob::ProximityWeight.unit(), SkillKnobUnit::Weight);
        assert_eq!(SkillKnob::ReactionTicks.unit(), SkillKnobUnit::Ticks);
        assert_eq!(
            SkillKnobOverride::try_new(
                SkillKnob::ProximityWeight,
                SkillKnobValue::Distance(500.0),
                designed(),
            )
            .expect_err("a distance in a weight slot is a content bug"),
            CombatSchemaError::KnobUnitMismatch {
                knob: SkillKnob::ProximityWeight,
                expected: SkillKnobUnit::Weight,
                found: SkillKnobUnit::Distance,
            }
        );

        // Out-of-range and non-finite values are refused by name.
        assert_eq!(
            SkillKnobOverride::try_new(
                SkillKnob::AimErrorRad,
                SkillKnobValue::Angle(f64::NAN),
                designed()
            )
            .expect_err("NaN is refused"),
            CombatSchemaError::NonFiniteKnob {
                knob: SkillKnob::AimErrorRad
            }
        );
        assert!(matches!(
            SkillKnobOverride::try_new(
                SkillKnob::ReactionTicks,
                SkillKnobValue::Ticks(MAX_REACTION_TICKS + 1),
                designed()
            )
            .expect_err("a reaction delay beyond the bound is refused"),
            CombatSchemaError::KnobOutOfRange {
                knob: SkillKnob::ReactionTicks,
                ..
            }
        ));
        // A zero-meter engagement range is refused by the declared record
        // for the same reason the lowered runtime profile refuses it: the
        // proximity term is normalized over it, so it must be positive, and
        // a record the lowering boundary would reject must not validate
        // here.
        assert!(matches!(
            SkillKnobOverride::try_new(
                SkillKnob::EngagementRangeM,
                SkillKnobValue::Distance(0.0),
                designed()
            )
            .expect_err("a zero engagement range is refused"),
            CombatSchemaError::KnobOutOfRange {
                knob: SkillKnob::EngagementRangeM,
                ..
            }
        ));
        let mut zero_range = declared_synthetic_escort_knobs();
        zero_range.engagement_range_m = known(Meters(0.0));
        assert!(
            matches!(
                zero_range.validate(),
                Err(CombatSchemaError::KnobOutOfRange {
                    knob: SkillKnob::EngagementRangeM,
                    ..
                })
            ),
            "a role profile carrying a zero range is refused by name, not \
             clamped into a number: {zero_range:?}"
        );
        assert!(
            SkillKnobs::validate(&declared_synthetic_escort_knobs()).is_ok(),
            "the declared fixture range is inside the bounds"
        );

        // The elite tier moves three behavior knobs, each carrying its own
        // provenance.
        let elite = DifficultyProfile::try_new(
            DifficultyTier::Elite,
            vec![
                override_of(SkillKnob::ReactionTicks, SkillKnobValue::Ticks(6)),
                override_of(SkillKnob::AimErrorRad, SkillKnobValue::Angle(0.015)),
                override_of(SkillKnob::ProtectedActorWeight, SkillKnobValue::Weight(4.0)),
            ],
            designed(),
        )
        .expect("the elite tier is valid");
        assert_eq!(elite.tier(), DifficultyTier::Elite);
        assert_eq!(elite.overrides().len(), 3);
        for change in elite.overrides() {
            assert_eq!(&change.provenance.claim_id, &claim("f32a.unit"));
        }
        // A tier that moves one knob twice is refused: the second value
        // would silently win.
        assert_eq!(
            DifficultyProfile::try_new(
                DifficultyTier::Elite,
                vec![
                    override_of(SkillKnob::ReactionTicks, SkillKnobValue::Ticks(6)),
                    override_of(SkillKnob::ReactionTicks, SkillKnobValue::Ticks(12)),
                ],
                designed()
            )
            .expect_err("a knob cannot be overridden twice"),
            CombatSchemaError::DuplicateKnob {
                knob: SkillKnob::ReactionTicks
            }
        );
    }

    /// The declared ace variant is a list of behavior overrides, applied to
    /// the base role's profile field-wise, and it carries no damage, armor
    /// or health field to inflate.
    #[test]
    fn accept_f32_a_ace_variant_is_a_behavior_override_of_its_base_role() {
        let ace = declared_synthetic_ace_profile();
        assert_eq!(ace.id().as_str(), "pilot/synthetic.ace-wing-leader");
        assert_eq!(ace.base_role(), DeclaredCombatRole::Escort);
        assert_eq!(ace.origin(), &Origin::SyntheticFixture);
        assert_eq!(ace.overrides().len(), 3);
        assert_eq!(
            ace.knob_value(SkillKnob::ReactionTicks)
                .expect("the ace overrides the reaction")
                .known(),
            Some(SkillKnobValue::Ticks(6))
        );
        assert_eq!(
            ace.knob_value(SkillKnob::AimErrorRad)
                .expect("the ace overrides the aim error")
                .known(),
            Some(SkillKnobValue::Angle(0.015))
        );
        assert!(matches!(
            ace.knob_value(SkillKnob::ProximityWeight),
            Err(CombatSchemaError::UnknownKnob { .. })
        ));

        // Applying the variant to the declared escort role moves exactly the
        // three behavior numbers and leaves everything else alone.
        let mut profile = DeclaredRoleProfile::try_new(
            DeclaredCombatRole::Escort,
            RoleArsenal::guns(),
            declared_synthetic_escort_knobs(),
            declared_synthetic_escort_policy(),
            Origin::SyntheticFixture,
            designed(),
        )
        .expect("the escort role is valid");
        for change in ace.overrides() {
            profile.apply(change).expect("the ace override applies");
        }
        assert_eq!(profile.knobs().reaction_ticks.clone().known(), Some(6));
        assert_eq!(
            profile.knobs().aim_error_rad.clone().known(),
            Some(Radians(0.015))
        );
        assert_eq!(
            profile
                .priority()
                .known_weight(PriorityTerm::ProtectedActorThreat),
            Some(4.0)
        );
        // Untouched knobs keep their declared values.
        assert_eq!(
            profile.knobs().engagement_range_m.clone().known(),
            Some(Meters(1_500.0))
        );
        assert_eq!(
            profile.knobs().fire_discipline_ticks.clone().known(),
            Some(30)
        );
        assert_eq!(
            profile.priority().threat_window_ticks.clone().known(),
            Some(120)
        );
        // A moved knob carries the override's own claim id, so a lowered
        // profile can say which record moved it.
        assert_eq!(
            profile
                .knobs()
                .reaction_ticks
                .provenance()
                .map(|provenance| provenance.claim_id.clone()),
            Some(synthetic_combat_claim())
        );

        // An ace id outside the pilot namespace is refused.
        assert_eq!(
            DeclaredAceProfile::try_new(
                ContentId::from_source(ContentKind::Airframe, "synthetic.ace-plane")
                    .expect("valid airframe id"),
                DeclaredCombatRole::Escort,
                Vec::new(),
                Origin::SyntheticFixture,
                designed(),
            )
            .expect_err("an ace variant is identified by a pilot id"),
            CombatSchemaError::AceKindMismatch {
                id: ContentId::from_source(ContentKind::Airframe, "synthetic.ace-plane")
                    .expect("valid airframe id")
            }
        );
    }

    /// The declared formation names a leader that is one of its members and
    /// one policy per recovery trigger, and refuses a leader slot that is
    /// not a member (F32 non-negotiable 4).
    #[test]
    fn accept_f32_a_formation_declares_a_leader_and_a_policy_per_trigger() {
        let formation = declared_synthetic_formation();
        assert_eq!(formation.id(), FormationId(1));
        assert_eq!(formation.leader_slot(), 0);
        assert_eq!(formation.members().len(), 3);
        assert_eq!(formation.members()[0].formation_role, FormationRole::Leader);
        assert_eq!(
            formation.members()[1].formation_role,
            FormationRole::Follower
        );
        assert_eq!(
            formation.recovery().leader_loss,
            RecoveryPolicy::ReassignLead
        );
        assert_eq!(
            formation.recovery().assigned_target_destroyed,
            RecoveryPolicy::Regroup
        );
        assert_eq!(
            formation.recovery().route_interrupted,
            RecoveryPolicy::ResumeRoute
        );
        assert_eq!(
            formation.recovery().protected_actor_lost,
            RecoveryPolicy::Regroup
        );
        assert_eq!(RecoveryPolicy::ALL.len(), 5);
        for policy in RecoveryPolicy::ALL {
            assert!(!policy.label().is_empty());
        }

        let recovery = *formation.recovery();
        // A leader slot that is not a member is refused by name.
        assert_eq!(
            DeclaredFormation::try_new(
                FormationId(1),
                7,
                vec![FormationMember {
                    slot: 0,
                    formation_role: FormationRole::Leader,
                    role: DeclaredCombatRole::Escort,
                }],
                recovery,
                designed()
            )
            .expect_err("a leader that is not a member is refused"),
            CombatSchemaError::FormationLeaderNotAMember {
                formation: FormationId(1),
                leader_slot: 7,
            }
        );
        // Two members in one slot are refused.
        assert_eq!(
            DeclaredFormation::try_new(
                FormationId(1),
                0,
                vec![
                    FormationMember {
                        slot: 0,
                        formation_role: FormationRole::Leader,
                        role: DeclaredCombatRole::Escort,
                    },
                    FormationMember {
                        slot: 0,
                        formation_role: FormationRole::Follower,
                        role: DeclaredCombatRole::Escort,
                    },
                ],
                recovery,
                designed()
            )
            .expect_err("a slot holds one member"),
            CombatSchemaError::DuplicateFormationMember {
                formation: FormationId(1),
                slot: 0,
            }
        );
    }

    /// The rules record refuses a wrong subject, a duplicated role, a role
    /// used but never declared, and a duplicated ace, formation or tier — and
    /// an unknown knob value stays an explicit unknown rather than a number.
    #[test]
    fn accept_f32_a_declared_rules_refuse_inconsistent_records() {
        let fixture = declared_synthetic_combat_rules();

        // An unknown weight is not a zero: it stays unknown and is reported
        // at the lowering boundary.
        let mut escort_policy = declared_synthetic_escort_policy();
        escort_policy.protected_actor_weight = Resolved::Unknown {
            claim_id: claim("f32a.escort-protected-weight"),
            reason: "the original escort's protected-actor weight is unmeasured".to_owned(),
        };
        let role = DeclaredRoleProfile::try_new(
            DeclaredCombatRole::Escort,
            RoleArsenal::guns(),
            declared_synthetic_escort_knobs(),
            escort_policy.clone(),
            Origin::SyntheticFixture,
            designed(),
        )
        .expect("an unknown weight is recorded, not refused at construction");
        assert!(!role.priority().protected_actor_weight.is_known());
        assert_eq!(
            role.priority()
                .known_weight(PriorityTerm::ProtectedActorThreat),
            None,
            "an unknown weight has no number to lower"
        );
        // A policy whose four weights are all known and zero is refused: it
        // would score nothing.
        let mut unscored = declared_synthetic_escort_policy();
        unscored.protected_actor_weight = known(0.0);
        unscored.objective_weight = known(0.0);
        unscored.self_defense_weight = known(0.0);
        unscored.proximity_weight = known(0.0);
        assert_eq!(
            unscored
                .validate()
                .expect_err("an all-zero policy scores nothing"),
            CombatSchemaError::NoScoredPriorityTerm
        );

        // A subject that is neither a mission nor a launchable scenario.
        assert_eq!(
            DeclaredCombatRules::try_new(
                ContentId::from_source(ContentKind::Airframe, "synthetic.not-a-subject")
                    .expect("valid airframe id"),
                Origin::SyntheticFixture,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                designed()
            )
            .expect_err("only a mission or a launchable owns combat rules"),
            CombatSchemaError::SubjectKind {
                subject: ContentId::from_source(ContentKind::Airframe, "synthetic.not-a-subject")
                    .expect("valid airframe id")
            }
        );

        // A duplicated role.
        let escort_role = || {
            DeclaredRoleProfile::try_new(
                DeclaredCombatRole::Escort,
                RoleArsenal::guns(),
                declared_synthetic_escort_knobs(),
                declared_synthetic_escort_policy(),
                Origin::SyntheticFixture,
                designed(),
            )
            .expect("the escort role is valid")
        };
        assert_eq!(
            DeclaredCombatRules::try_new(
                fixture.subject().clone(),
                Origin::SyntheticFixture,
                vec![escort_role(), escort_role()],
                Vec::new(),
                Vec::new(),
                Vec::new(),
                designed()
            )
            .expect_err("one role is declared once"),
            CombatSchemaError::DuplicateRole {
                role: DeclaredCombatRole::Escort
            }
        );

        // A formation member naming a role the subject never declared.
        let undeclared_member = DeclaredFormation::try_new(
            FormationId(2),
            0,
            vec![FormationMember {
                slot: 0,
                formation_role: FormationRole::Leader,
                role: DeclaredCombatRole::Retreat,
            }],
            *declared_synthetic_formation().recovery(),
            designed(),
        )
        .expect("the formation itself is well-formed");
        assert_eq!(
            DeclaredCombatRules::try_new(
                fixture.subject().clone(),
                Origin::SyntheticFixture,
                vec![escort_role()],
                Vec::new(),
                vec![undeclared_member],
                Vec::new(),
                designed()
            )
            .expect_err("a member cannot use an undeclared role"),
            CombatSchemaError::UndeclaredRole {
                role: DeclaredCombatRole::Retreat
            }
        );

        // A duplicated ace, formation and tier are each refused by name.
        // The synthetic formation's leader holds the fighter role, so these
        // cases declare both roles.
        let fighter_role = || {
            DeclaredRoleProfile::try_new(
                DeclaredCombatRole::FighterAttack,
                RoleArsenal::guns(),
                declared_synthetic_fighter_knobs(),
                declared_synthetic_fighter_policy(),
                Origin::SyntheticFixture,
                designed(),
            )
            .expect("the fighter role is valid")
        };
        let both_roles = vec![escort_role(), fighter_role()];
        let ace = declared_synthetic_ace_profile();
        assert_eq!(
            DeclaredCombatRules::try_new(
                fixture.subject().clone(),
                Origin::SyntheticFixture,
                both_roles.clone(),
                vec![ace.clone(), ace.clone()],
                Vec::new(),
                Vec::new(),
                designed()
            )
            .expect_err("one ace variant per id"),
            CombatSchemaError::DuplicateAce {
                id: ace.id().clone()
            }
        );
        assert_eq!(
            DeclaredCombatRules::try_new(
                fixture.subject().clone(),
                Origin::SyntheticFixture,
                both_roles.clone(),
                vec![ace.clone()],
                vec![
                    declared_synthetic_formation(),
                    declared_synthetic_formation()
                ],
                Vec::new(),
                designed()
            )
            .expect_err("one formation per id"),
            CombatSchemaError::DuplicateFormation {
                formation: FormationId(1)
            }
        );
        assert_eq!(
            DeclaredCombatRules::try_new(
                fixture.subject().clone(),
                Origin::SyntheticFixture,
                both_roles,
                Vec::new(),
                Vec::new(),
                vec![
                    DifficultyProfile::try_new(DifficultyTier::Elite, Vec::new(), designed())
                        .expect("the tier is valid"),
                    DifficultyProfile::try_new(DifficultyTier::Elite, Vec::new(), designed())
                        .expect("the tier is valid"),
                ],
                designed()
            )
            .expect_err("one profile per tier"),
            CombatSchemaError::DuplicateDifficultyTier {
                tier: DifficultyTier::Elite
            }
        );

        // An ace naming a base role the subject never declared.
        assert_eq!(
            DeclaredCombatRules::try_new(
                fixture.subject().clone(),
                Origin::SyntheticFixture,
                vec![
                    DeclaredRoleProfile::try_new(
                        DeclaredCombatRole::FighterAttack,
                        RoleArsenal::guns(),
                        declared_synthetic_fighter_knobs(),
                        declared_synthetic_fighter_policy(),
                        Origin::SyntheticFixture,
                        designed(),
                    )
                    .expect("the fighter role is valid")
                ],
                vec![
                    DeclaredAceProfile::try_new(
                        pilot("synthetic.ace-of-another-role"),
                        DeclaredCombatRole::Escort,
                        Vec::new(),
                        Origin::SyntheticFixture,
                        designed(),
                    )
                    .expect("the ace itself is well-formed")
                ],
                Vec::new(),
                Vec::new(),
                designed()
            )
            .expect_err("an ace must name a declared base role"),
            CombatSchemaError::UndeclaredRole {
                role: DeclaredCombatRole::Escort
            }
        );
    }

    /// A role whose declared arsenal cannot carry it is refused: a torpedo
    /// run with no ordnance would declare a role no actor can execute.
    #[test]
    fn accept_f32_a_role_arsenal_must_carry_the_role() {
        assert_eq!(
            DeclaredRoleProfile::try_new(
                DeclaredCombatRole::TorpedoRun,
                RoleArsenal::guns(),
                declared_synthetic_fighter_knobs(),
                declared_synthetic_fighter_policy(),
                Origin::SyntheticFixture,
                designed(),
            )
            .expect_err("a torpedo run needs a launcher"),
            CombatSchemaError::RoleArsenalMissing {
                role: DeclaredCombatRole::TorpedoRun
            }
        );
        assert!(
            DeclaredRoleProfile::try_new(
                DeclaredCombatRole::TorpedoRun,
                RoleArsenal::guns_and_ordnance(),
                declared_synthetic_fighter_knobs(),
                declared_synthetic_fighter_policy(),
                Origin::SyntheticFixture,
                designed(),
            )
            .is_ok()
        );
        // The role vocabulary matches the seven behaviors the F32 sheet
        // names, and the ordnance roles are the two that need a launcher.
        assert_eq!(DeclaredCombatRole::ALL.len(), 7);
        assert!(DeclaredCombatRole::BomberRun.needs_ordnance());
        assert!(DeclaredCombatRole::TorpedoRun.needs_ordnance());
        assert!(!DeclaredCombatRole::Escort.needs_ordnance());
        for role in DeclaredCombatRole::ALL {
            assert!(!role.label().is_empty());
        }
        assert_eq!(PriorityTerm::ALL.len(), 4);
        for term in PriorityTerm::ALL {
            assert_eq!(term.knob().unit(), SkillKnobUnit::Weight);
        }
    }

    /// The declared `Known` values all carry designed provenance, so no
    /// record in the fixture can be read as original data.
    #[test]
    fn accept_f32_a_declared_fixture_claims_no_original_data() {
        let rules = declared_synthetic_combat_rules();
        for profile in rules.roles() {
            assert_eq!(profile.origin(), &Origin::SyntheticFixture);
            assert_eq!(
                profile.provenance().claim_id.clone(),
                synthetic_combat_claim()
            );
            assert!(matches!(
                profile.knobs().aim_error_rad,
                Resolved::Known(Known { .. })
            ));
        }
        for ace in rules.aces() {
            assert_eq!(ace.origin(), &Origin::SyntheticFixture);
            assert!(!ace.origin().is_original());
        }
        assert_eq!(DifficultyTier::ALL.len(), 4);
        for tier in DifficultyTier::ALL {
            assert!(!tier.label().is_empty());
        }
    }
}

// ------------------------------------------- measured surface ----

/// The retail container the engine's resource header and the UI string image
/// are read beside.
const BASE_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The shipped English UI string image, named as the installation spells it.
///
/// **Measured**: it carries the option labels `IDS_DIFFICULTY` and its two
/// successors, in language 1033.
const STRING_IMAGE: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

/// What one measured AI skill label occupies in the installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillLabelRow {
    /// The label the data spells, byte for byte.
    pub label: String,
    /// How many declarations carry it.
    pub count: u32,
}

/// One measured instant-action scenario descriptor's AI facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioSurface {
    /// The reader archive the descriptor came from, as the installation
    /// spells it in lowercase logical form.
    pub container: String,
    /// The declared mission type, when the descriptor names one.
    pub mission_type: Option<String>,
    /// The descriptor's complete key vocabulary, at **every** depth, sorted and
    /// deduplicated.
    ///
    /// Not only the root keys: a nested record could in principle carry a
    /// difficulty of its own, so a negative measured over the root keys alone
    /// would not be the negative a caller reads. This is the whole vocabulary
    /// a `.zrd` value list spells, walked by [`zrd_key_census`].
    pub keys: Vec<String>,
    /// How many enemy groups it declares.
    pub enemy_groups: u32,
    /// Every skill label it declares, over its groups and its named ace.
    pub skills: Vec<String>,
    /// Its `ace_stats` vector, when every element is an integer.
    pub ace_stats: Option<Vec<i64>>,
}

/// The original's AI surface, measured over one installation.
///
/// Everything here is re-derived from the installation on each call through
/// `cs_assets`' ROF mount, `cs_formats`' resource-header and PE-resource
/// readers, [`crate::config::StringCatalog`] and [`crate::stunts`]' `.zrd`
/// decoder — so a committed constant in this module can be compared against a
/// fresh measurement instead of being trusted.
///
/// **No display text leaves this type.** The string-image side reports
/// *occupancy* — how many ids carry a non-empty string, in how many languages,
/// over how many code units — and never the string itself, so nothing here
/// reproduces original display content (AGENTS rule 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalAiSurface {
    /// The installation digest every number below is bound to.
    pub install_sha256: String,
    /// The canonical-content digest of the same installation.
    pub content_sha256: String,
    /// The id `IDS_DIFFICULTY` declares.
    pub difficulty_macro_id: u32,
    /// The id of the first macro the header declares after it.
    pub difficulty_bound_id: u32,
    /// The bound's width: how many ids the option's name list can hold.
    pub difficulty_steps: u32,
    /// How many of those ids the shipped string image populates.
    pub populated_difficulty_ids: u32,
    /// The difficulty row's game-options title, as `(macro, id)`.
    pub game_option_title: (String, u32),
    /// The difficulty row's game-options description, as `(macro, id)`.
    pub game_option_desc: (String, u32),
    /// The instant-action difficulty label, as `(macro, id)`.
    pub ia_difficulty_label: (String, u32),
    /// Whether any measured scenario descriptor records a difficulty.
    pub difficulty_recorded_per_scenario: bool,
    /// Every reader archive the installation carries.
    pub reader_archives: u32,
    /// Every scenario descriptor, in installation order.
    pub scenarios: Vec<ScenarioSurface>,
    /// The skill label census over every descriptor, sorted by label.
    pub skill_labels: Vec<SkillLabelRow>,
    /// How many skill labels the descriptors declare in total.
    pub skill_label_count: u32,
    /// How many enemy groups the descriptors declare.
    pub enemy_group_count: u32,
    /// How many ids the engine's resource header declares.
    pub declared_header_ids: u32,
}

impl OriginalAiSurface {
    /// The distinct measured skill tiers, as the labels the data spells,
    /// sorted.
    #[must_use]
    pub fn skill_tiers(&self) -> Vec<&str> {
        self.skill_labels
            .iter()
            .map(|row| row.label.as_str())
            .collect()
    }

    /// How many instances of `label` the descriptors declare.
    #[must_use]
    pub fn skill_label_count_of(&self, label: &str) -> u32 {
        self.skill_labels
            .iter()
            .find(|row| row.label == label)
            .map_or(0, |row| row.count)
    }

    /// The mission types the descriptors declare, sorted and deduplicated.
    #[must_use]
    pub fn mission_types(&self) -> Vec<&str> {
        let mut types: Vec<&str> = self
            .scenarios
            .iter()
            .filter_map(|scenario| scenario.mission_type.as_deref())
            .collect();
        types.sort_unstable();
        types.dedup();
        types
    }

    /// The complete descriptor key vocabulary, at every depth, sorted and
    /// deduplicated.
    ///
    /// This is what makes
    /// [`ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO`] a measurement of the whole
    /// descriptor rather than of its root keys: a nested record is included, so
    /// "no key anywhere names a difficulty" is a statement about everything the
    /// eight descriptors spell.
    #[must_use]
    pub fn scenario_key_vocabulary(&self) -> Vec<&str> {
        let mut keys: Vec<&str> = self
            .scenarios
            .iter()
            .flat_map(|scenario| scenario.keys.iter().map(String::as_str))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    /// Whether the measured step count matches the committed
    /// [`ORIGINAL_DIFFICULTY_STEPS`].
    #[must_use]
    pub fn agrees_with_committed_steps(&self) -> bool {
        self.difficulty_steps == ORIGINAL_DIFFICULTY_STEPS
            && self.populated_difficulty_ids == ORIGINAL_DIFFICULTY_STEPS
    }

    /// The ids the option occupies, ascending.
    #[must_use]
    pub fn difficulty_ids(&self) -> Vec<u32> {
        (self.difficulty_macro_id..self.difficulty_bound_id).collect()
    }

    /// Whether the measured skill vocabulary matches [`DeclaredSkillTier`].
    #[must_use]
    pub fn agrees_with_declared_skill_tiers(&self) -> bool {
        let mut measured = self.skill_tiers();
        measured.sort_unstable();
        let mut declared: Vec<&str> = DeclaredSkillTier::ALL.iter().map(|t| t.label()).collect();
        declared.sort_unstable();
        measured == declared
    }
}

/// Why an AI surface could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SurfaceError {
    /// The installation could not be discovered.
    Discovery(String),
    /// A file could not be read.
    Read {
        /// The installation-relative spelling that failed.
        spelling: String,
        /// What failed.
        reason: String,
    },
    /// The engine's resource header could not be decoded.
    ResourceHeader(String),
    /// A scenario descriptor could not be decoded.
    Scenario {
        /// The reader archive it came from.
        container: String,
        /// What failed.
        reason: String,
    },
    /// A measured fact the measurement depends on was absent.
    MissingFact {
        /// Which fact.
        what: &'static str,
    },
}

impl fmt::Display for SurfaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery(reason) => write!(f, "the installation is not discoverable: {reason}"),
            Self::Read { spelling, reason } => write!(f, "{spelling} is unreadable: {reason}"),
            Self::ResourceHeader(reason) => {
                write!(f, "the engine's resource header is unreadable: {reason}")
            }
            Self::Scenario { container, reason } => {
                write!(
                    f,
                    "{container}'s scenario descriptor is undecodable: {reason}"
                )
            }
            Self::MissingFact { what } => write!(f, "the installation declares no {what}"),
        }
    }
}

impl std::error::Error for SurfaceError {}

/// The installation manifest, discovered once per process.
///
/// Production discovery hashes every file of the installation, so a per-call
/// discovery would hash a multi-gigabyte installation once per measurement
/// field; the digest is cached and every span is bound to *it*, which is what
/// stops a report from one installation being reused for another's numbers.
///
/// The cache is keyed on nothing, so **the first root a process measures is
/// the only one it can measure**: a caller that passes a second installation
/// root in the same process is answered from the first one's manifest rather
/// than from its own. Measuring two installations is two processes — which is
/// what every `accept_f32_d_*` test does, since each binds its numbers to one
/// fingerprint pair.
fn installation(root: &Path) -> Result<&'static InstallManifest, SurfaceError> {
    static CACHE: OnceLock<Result<InstallManifest, String>> = OnceLock::new();
    let cached = CACHE.get_or_init(|| {
        install::discover(root)
            .map(|found| found.manifest)
            .map_err(|error| format!("{error:?}"))
    });
    match cached {
        Ok(manifest) => Ok(manifest),
        Err(reason) => Err(SurfaceError::Discovery(reason.clone())),
    }
}

/// The installation digest every span of a measurement is bound to.
fn install_sha256(root: &Path) -> Result<ContentHash, SurfaceError> {
    Ok(install::fingerprint(installation(root)?))
}

/// Opens one retail member's bytes through the production ROF mount.
///
/// The mount answers *resolution* and the [`RofSource`] answers *bytes*: a ROF
/// member lives inside its container and may be compressed, so the session's
/// directory read reports `no backing` for it **by design** and the production
/// member reader is the byte path. The session is still used, to prove the
/// member resolves through the mount before a byte is read.
fn read_member(root: &Path, spelling: &str) -> Result<Vec<u8>, SurfaceError> {
    let install = install_sha256(root)?;
    let id: String = format!("rof-{}", BASE_CONTAINER.to_ascii_lowercase())
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
            MountId::new(&id).map_err(|error| SurfaceError::Read {
                spelling: BASE_CONTAINER.to_owned(),
                reason: error.to_string(),
            })?,
            MountNamespace::new(INSTALL_NAMESPACE).map_err(|error| SurfaceError::Read {
                spelling: BASE_CONTAINER.to_owned(),
                reason: error.to_string(),
            })?,
            PrecedenceClass::Shared,
            BASE_CONTAINER,
        )
        .retail(),
        &root.join(BASE_CONTAINER),
    )
    .map_err(|error| SurfaceError::Read {
        spelling: BASE_CONTAINER.to_owned(),
        reason: format!("{error:?}"),
    })?;
    let session = builder.open();
    let key = AssetKey::from_spelling(INSTALL_NAMESPACE, spelling, "default").map_err(|error| {
        SurfaceError::Read {
            spelling: spelling.to_owned(),
            reason: error.to_string(),
        }
    })?;
    session.resolve(&key).map_err(|error| SurfaceError::Read {
        spelling: spelling.to_owned(),
        reason: format!("{error:?}"),
    })?;
    source
        .read(&key)
        .map(|read| read.data)
        .map_err(|error| SurfaceError::Read {
            spelling: spelling.to_owned(),
            reason: format!("{error:?}"),
        })
}

/// The engine's resource header, read out of the base retail archive.
fn resource_header(root: &Path) -> Result<Vec<u32>, SurfaceError> {
    let bytes = read_member(root, ORIGINAL_RESOURCE_HEADER)?;
    let header = read_resource_header(
        &mut ParseContext::with_defaults(ORIGINAL_RESOURCE_HEADER),
        &bytes,
    )
    .map_err(|error| SurfaceError::ResourceHeader(error.to_string()))?;
    let mut ids: Vec<u32> = header
        .defines()
        .filter_map(|define| define.resource_id())
        .collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

/// The id one macro declares in the engine's resource header.
fn macro_id(root: &Path, name: &'static str) -> Result<u32, SurfaceError> {
    let bytes = read_member(root, ORIGINAL_RESOURCE_HEADER)?;
    let header = read_resource_header(
        &mut ParseContext::with_defaults(ORIGINAL_RESOURCE_HEADER),
        &bytes,
    )
    .map_err(|error| SurfaceError::ResourceHeader(error.to_string()))?;
    header
        .resource_id(name.as_bytes())
        .ok_or(SurfaceError::MissingFact { what: name })
}

/// How many of the option's ids the shipped string image populates.
///
/// **Counts only**: a non-empty string is counted, never returned.
fn populated_difficulty_ids(root: &Path, first: u32, bound: u32) -> Result<u32, SurfaceError> {
    let bytes = std::fs::read(root.join(STRING_IMAGE)).map_err(|error| SurfaceError::Read {
        spelling: STRING_IMAGE.to_owned(),
        reason: error.to_string(),
    })?;
    let source = SourceSpan::new(
        install_sha256(root)?,
        STRING_IMAGE,
        None,
        0,
        bytes.len() as u64,
        None,
    )
    .map_err(|error| SurfaceError::Read {
        spelling: STRING_IMAGE.to_owned(),
        reason: error.to_string(),
    })?;
    let catalog = StringCatalog::read(
        &mut ParseContext::with_defaults(STRING_IMAGE),
        source,
        &bytes,
    )
    .map_err(|error| SurfaceError::Read {
        spelling: STRING_IMAGE.to_owned(),
        reason: error.to_string(),
    })?;
    let mut populated = 0;
    for id in first..bound {
        let rows: Vec<_> = catalog.rows().iter().filter(|row| row.id == id).collect();
        if rows
            .iter()
            .any(|row| row.text.as_ref().is_some_and(|text| !text.is_empty()))
        {
            populated += 1;
        }
    }
    Ok(populated)
}

/// Every instant-action scenario descriptor the installation carries, read
/// through the production reader-archive discovery and the production `.zrd`
/// decoder.
fn scenario_descriptors(root: &Path) -> Result<Vec<ScenarioSurface>, SurfaceError> {
    let manifest = installation(root)?;
    let mut rows = Vec::new();
    for record in &manifest.files {
        let key = record.relative_spelling.logical_key();
        if !key.ends_with("zrdr.zbd") || !key.contains("/ia1/") {
            continue;
        }
        let spelling = record.relative_spelling.as_str();
        let bytes = std::fs::read(manifest.host_root.join(spelling)).map_err(|error| {
            SurfaceError::Read {
                spelling: spelling.to_owned(),
                reason: error.to_string(),
            }
        })?;
        let path = RelativePath::new(spelling).map_err(|error| SurfaceError::Read {
            spelling: spelling.to_owned(),
            reason: error.to_string(),
        })?;
        let discovery = discover_container(&key, &path, &bytes);
        for program in discovery.programs() {
            if program.locator().member() != Some(SCENARIO_MEMBER) {
                continue;
            }
            let node = decode_zrd(program.bytes()).map_err(|error| SurfaceError::Scenario {
                container: key.clone(),
                reason: format!("{error:?}"),
            })?;
            rows.push(ScenarioSurface::of(&key, &node));
        }
    }
    Ok(rows)
}

impl ScenarioSurface {
    /// Reads one descriptor's measured AI facts through the production
    /// [`crate::stunts`] parsers.
    fn of(container: &str, node: &ZrdValue) -> Self {
        let roster = scenario_non_player_aircraft(node);
        let mut skills: Vec<String> = roster
            .enemy_groups()
            .iter()
            .filter_map(|group| group.skill().map(str::to_owned))
            .collect();
        if let Some(skill) = roster.ace().skill() {
            skills.push(skill.to_owned());
        }
        let ace_stats = zrd_field(node, "ace_stats").and_then(|value| {
            let list = value.as_list()?;
            let mut slots = Vec::with_capacity(list.len());
            for entry in list {
                match entry {
                    ZrdValue::Int(value) => slots.push(i64::from(*value)),
                    _ => return None,
                }
            }
            Some(slots)
        });
        Self {
            container: container.to_owned(),
            mission_type: scenario_mission_type(node).map(str::to_owned),
            keys: zrd_key_census(node),
            enemy_groups: roster.enemy_groups().len() as u32,
            skills,
            ace_stats,
        }
    }
}

/// Every key a `.zrd` value spells, at **any** depth, sorted and deduplicated.
///
/// [`crate::stunts::zrd_flat_fields`] reads one level of the alternating
/// key/value list grammar, which is enough for the fields a caller looks up by
/// name. A *negative* needs more: "no key in this descriptor names a difficulty"
/// is only a statement about the descriptor if the walk descends, because a
/// nested record (`group1 = [ ... ]`) could carry one of its own. So this walks
/// every list recursively and collects every text that occupies a key position,
/// sorted so the census is comparable between installations.
fn zrd_key_census(node: &ZrdValue) -> Vec<String> {
    fn walk(node: &ZrdValue, keys: &mut BTreeSet<String>) {
        let Some(children) = node.as_list() else {
            return;
        };
        let mut index = 0;
        while index + 1 < children.len() {
            if let ZrdValue::Text(name) = &children[index] {
                keys.insert(name.clone());
                walk(&children[index + 1], keys);
                index += 2;
            } else {
                index += 1;
            }
        }
    }
    let mut keys = BTreeSet::new();
    walk(node, &mut keys);
    keys.into_iter().collect()
}

/// Measures the original's AI surface over one installation.
///
/// The measurement is what the constants at the top of this module encode: the
/// campaign difficulty option's **bound** (from the engine's resource header)
/// and its **occupancy** (from the shipped string image), the per-aircraft AI
/// skill vocabulary (from the scenario descriptors), and the shape of the
/// declared ace stat vector.
///
/// # Errors
///
/// [`SurfaceError`] when the installation cannot be discovered, mounted or
/// read, or when a measured fact the measurement depends on is absent. It
/// never falls back to a synthetic fixture: a measurement that could not be
/// taken is a failure, not a default.
pub fn original_ai_surface(root: &Path) -> Result<OriginalAiSurface, SurfaceError> {
    let manifest = installation(root)?;
    let ids = resource_header(root)?;
    let difficulty_macro_id = macro_id(root, ORIGINAL_DIFFICULTY_OPTION_MACRO)?;
    let difficulty_bound_id = macro_id(root, ORIGINAL_DIFFICULTY_OPTION_BOUND_MACRO)?;
    if difficulty_bound_id <= difficulty_macro_id {
        return Err(SurfaceError::MissingFact {
            what: ORIGINAL_DIFFICULTY_OPTION_MACRO,
        });
    }
    let populated = populated_difficulty_ids(root, difficulty_macro_id, difficulty_bound_id)?;
    let scenarios = scenario_descriptors(root)?;

    let mut census: BTreeMap<String, u32> = BTreeMap::new();
    for scenario in &scenarios {
        for label in &scenario.skills {
            *census.entry(label.clone()).or_default() += 1;
        }
    }
    let skill_labels: Vec<SkillLabelRow> = census
        .into_iter()
        .map(|(label, count)| SkillLabelRow { label, count })
        .collect();
    let skill_label_count = skill_labels.iter().map(|row| row.count).sum();
    let enemy_group_count = scenarios.iter().map(|scenario| scenario.enemy_groups).sum();
    let mut reader_archives = 0u32;
    for record in &manifest.files {
        if record.relative_spelling.logical_key().ends_with("zrdr.zbd") {
            reader_archives += 1;
        }
    }
    let difficulty_recorded_per_scenario = scenarios.iter().any(|scenario| {
        scenario
            .keys
            .iter()
            .any(|key| key.to_ascii_lowercase().contains("difficul"))
    });

    Ok(OriginalAiSurface {
        install_sha256: install::fingerprint(manifest).to_hex(),
        content_sha256: install::content_fingerprint(manifest).to_hex(),
        difficulty_macro_id,
        difficulty_bound_id,
        difficulty_steps: difficulty_bound_id - difficulty_macro_id,
        populated_difficulty_ids: populated,
        game_option_title: (
            ORIGINAL_GAME_OPTION_DIFFICULTY_TITLE.0.to_owned(),
            macro_id(root, ORIGINAL_GAME_OPTION_DIFFICULTY_TITLE.0)?,
        ),
        game_option_desc: (
            ORIGINAL_GAME_OPTION_DIFFICULTY_DESC.0.to_owned(),
            macro_id(root, ORIGINAL_GAME_OPTION_DIFFICULTY_DESC.0)?,
        ),
        ia_difficulty_label: (
            ORIGINAL_IA_DIFFICULTY_LABEL.0.to_owned(),
            macro_id(root, ORIGINAL_IA_DIFFICULTY_LABEL.0)?,
        ),
        difficulty_recorded_per_scenario,
        reader_archives,
        scenarios,
        skill_labels,
        skill_label_count,
        enemy_group_count,
        declared_header_ids: ids.len() as u32,
    })
}

// ------------------------------------------------ f32-d evidence ----

/// The F32-D acceptance tests.
///
/// They live in this file because `crates/cs_content/tests/` is not F32-D's
/// owner path and a `cs_sim` test cannot reach `cs_content`. Every assertion is
/// about [`original_ai_surface`] — production code — so a stale committed
/// constant fails instead of passing.
#[cfg(test)]
mod f32_d {
    use std::path::PathBuf;

    use cs_types::evidence::ClaimStatus;

    use super::*;
    use crate::ordnance::ORIGINAL_ROCKET_NAME_BLOCKS;
    use crate::stunts::{
        ENEMY_SKILL_ACE, ENEMY_SKILL_KEY, ENEMY_SKILL_NOVICE, ENEMY_SKILL_VETERAN,
        SCENARIO_ACE_SKILL_KEY,
    };

    /// The installation fingerprint the F32-D constants were measured over.
    const RETAIL_INSTALL_SHA256: &str =
        "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";

    /// The canonical-content digest of the same installation.
    const RETAIL_CONTENT_SHA256: &str =
        "a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d";

    /// The read-only installation root, or a loud failure: a retail test must
    /// fail, not pass, when `CS_GAME_DIR` is absent.
    fn game_dir() -> PathBuf {
        let dir = std::env::var_os("CS_GAME_DIR").expect(
            "CS_GAME_DIR is not set: this measurement needs the original installation \
             (capability `retail`)",
        );
        let dir = PathBuf::from(dir);
        assert!(
            dir.is_dir(),
            "CS_GAME_DIR {} is not a directory",
            dir.display()
        );
        dir
    }

    /// The measured surface, bound to the fingerprint the constants record.
    fn measured() -> OriginalAiSurface {
        let surface = original_ai_surface(&game_dir())
            .unwrap_or_else(|error| panic!("the AI surface must measure: {error}"));
        assert_eq!(
            surface.install_sha256, RETAIL_INSTALL_SHA256,
            "the installation fingerprint the F32-D constants were measured over"
        );
        assert_eq!(surface.content_sha256, RETAIL_CONTENT_SHA256);
        surface
    }

    /// The measurement binds every number it reports to one installation: the
    /// digest pair travels with the surface, so a report cannot describe one
    /// installation's numbers under another one's fingerprint.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_the_measurement_is_bound_to_one_installation() {
        let surface = measured();
        assert_eq!(surface.install_sha256, RETAIL_INSTALL_SHA256);
        assert_eq!(surface.content_sha256, RETAIL_CONTENT_SHA256);
        assert!(
            surface.declared_header_ids > 100,
            "the resource header declares the id vocabulary the option blocks are cut from"
        );
        assert_eq!(surface.reader_archives, 62, "every reader archive");
    }

    /// **AC04, retail half.** The original's campaign difficulty option is a
    /// **three**-step selector, and the project's four-step
    /// [`DifficultyTier`] vocabulary is one step longer than the original's —
    /// which [`DifficultyTier::measured_step`] and
    /// [`DifficultyTier::is_designed_extension`] report instead of hiding.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_the_original_campaign_difficulty_option_is_a_three_step_selector() {
        let surface = measured();

        assert_eq!(
            surface.difficulty_macro_id, ORIGINAL_DIFFICULTY_OPTION_FIRST_ID,
            "IDS_DIFFICULTY is where the committed block starts"
        );
        assert_eq!(
            surface.difficulty_bound_id, ORIGINAL_DIFFICULTY_OPTION_BOUND_ID,
            "IDS_VIEWCOCKPIT is what bounds the committed block"
        );
        assert_eq!(ORIGINAL_DIFFICULTY_STEPS, 3, "three measured steps");
        assert_eq!(
            surface.difficulty_steps, ORIGINAL_DIFFICULTY_STEPS,
            "the header's bound is the committed step count"
        );
        assert_eq!(
            surface.populated_difficulty_ids, ORIGINAL_DIFFICULTY_STEPS,
            "and the shipped string image populates all of them, so the bound is met"
        );
        assert!(
            surface.agrees_with_committed_steps(),
            "the measured surface agrees with the committed constant"
        );
        assert_eq!(
            surface.difficulty_ids(),
            ORIGINAL_DIFFICULTY_OPTION_IDS.to_vec(),
            "the block is exactly the committed id run"
        );

        // The designed vocabulary is one step longer, and says so.
        assert_eq!(DifficultyTier::ALL.len(), 4);
        assert_eq!(
            DifficultyTier::measured_tier_count(),
            ORIGINAL_DIFFICULTY_STEPS as usize,
            "exactly as many declared tiers have a measured step as the option has steps"
        );
        let extensions: Vec<&str> = DifficultyTier::ALL
            .iter()
            .filter(|tier| tier.is_designed_extension())
            .map(|tier| tier.label())
            .collect();
        assert_eq!(
            extensions,
            vec!["elite"],
            "the fourth tier is the extension"
        );
        for tier in DifficultyTier::ALL {
            match tier.measured_step() {
                Some(step) => assert!(step < ORIGINAL_DIFFICULTY_STEPS),
                None => assert!(tier.is_designed_extension()),
            }
        }
    }

    /// The difficulty count is the **header's bound met by the string
    /// image**, not a block width read as a count.
    ///
    /// The header's next declared id after `IDS_DIFFICULTY` bounds the name
    /// list at **three** ids — an *upper* bound, because the header is allowed
    /// to declare an id *inside* a list. The default-view list is the case
    /// that shows it is: `IDS_VIEWCOCKPIT` 112 and `IDS_VIEWCHASE` 113 are
    /// both declared and both are view entries, so the gap from 112 to 113 is
    /// one while the list holds two. Reading a gap as a count is the mistake
    /// F27-D repaired in the ammunition blocks; here it would have been right
    /// by luck, so the count is pinned from both sides.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_the_difficulty_count_is_a_bound_the_string_image_meets() {
        let root = game_dir();
        let surface = measured();
        let ids = resource_header(&root).expect("the header reads");

        // The bound, from the header alone.
        assert_eq!(
            surface.difficulty_steps,
            surface.difficulty_bound_id - surface.difficulty_macro_id,
            "the header's upper bound"
        );
        // No id *inside* the bound is declared by the header, so all three are
        // free for the option to use.
        for id in surface.difficulty_macro_id + 1..surface.difficulty_bound_id {
            assert!(
                !ids.contains(&id),
                "id {id} lies inside the measured bound and is declared by the header"
            );
        }
        // The lower bound: the shipped image gives every id of the bound a
        // non-empty label, so the option offers exactly that many steps.
        assert_eq!(
            surface.populated_difficulty_ids, surface.difficulty_steps,
            "every id of the bound is populated"
        );
        // The next id at or above the bound is the next list's first entry, so
        // the difficulty names cannot run past it.
        let next_at_or_above = ids
            .iter()
            .copied()
            .find(|id| *id >= surface.difficulty_bound_id);
        assert_eq!(next_at_or_above, Some(surface.difficulty_bound_id));

        // The occupancy reader is a *measurement*, not a restatement of the
        // bound: a block the header declares but the shipped image leaves empty
        // must read zero, which is the only way the two sides can be told
        // apart at all. `IDS_KEYUNMAPPED` is such a block.
        let empty_first = macro_id(&root, "IDS_KEYUNMAPPED").expect("the block exists");
        let empty_bound = ids
            .iter()
            .copied()
            .find(|id| *id > empty_first)
            .expect("a macro above it is declared");
        assert_eq!(
            populated_difficulty_ids(&root, empty_first, empty_bound)
                .expect("the empty block's occupancy reads"),
            0,
            "a declared block the shipped image leaves empty occupies nothing"
        );

        // The counterexample: a gap is not a count.
        let view_first = macro_id(&root, "IDS_VIEWCOCKPIT").expect("the view block exists");
        let view_second =
            macro_id(&root, "IDS_VIEWCHASE").expect("the view block's second entry exists");
        assert_eq!(
            view_second - view_first,
            1,
            "the gap inside the view list is one"
        );
        let lighting = macro_id(&root, "IDS_LIGHTINGLEVELS")
            .expect("the lighting block exists after the view block");
        assert_eq!(
            populated_difficulty_ids(&root, view_first, lighting).expect("the view block occupies"),
            2,
            "and the view list really holds two entries"
        );

        // The same bound, re-measured on the blocks F27-D/F28-D already
        // measured with it.
        let rocket_first = macro_id(&root, "IDS_ROCKETLONGNAME").expect("the rocket block exists");
        let rocket_second =
            macro_id(&root, "IDS_ROCKETSHORTNAME").expect("the rocket block's next entry");
        assert_eq!(
            rocket_second - rocket_first,
            ORIGINAL_ROCKET_NAME_BLOCKS[1].0 - ORIGINAL_ROCKET_NAME_BLOCKS[0].0,
            "the rocket name block is fifteen ids wide"
        );
        let airframe_first =
            macro_id(&root, "IDS_AIRFRAMELONGNAME").expect("the airframe block exists");
        let airframe_second =
            macro_id(&root, "IDS_AIRFRAMESHORTNAME").expect("the airframe block's next entry");
        assert_eq!(
            airframe_second - airframe_first,
            20,
            "the airframe name block is twenty ids wide"
        );
    }

    /// The option is **one row of one screen**, not a per-mission record: the
    /// title and the description are single ids, and no measured scenario
    /// descriptor carries a difficulty key.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_no_scenario_records_a_difficulty_so_only_a_selection_is_measurable() {
        let root = game_dir();
        let surface = measured();
        let ids = resource_header(&root).expect("the header reads");
        let after = |id: u32| ids.iter().copied().find(|other| *other > id);

        assert_eq!(
            surface.game_option_title,
            (ORIGINAL_GAME_OPTION_DIFFICULTY_TITLE.0.to_owned(), 1084)
        );
        assert_eq!(
            surface.game_option_desc,
            (ORIGINAL_GAME_OPTION_DIFFICULTY_DESC.0.to_owned(), 1087)
        );
        assert_eq!(after(1084), Some(1085), "the title is a single id");
        assert_eq!(after(1087), Some(1088), "the description is a single id");
        assert_eq!(surface.ia_difficulty_label.1, 3695);
        assert_eq!(
            after(3695),
            Some(3700),
            "the instant-action label is a single id"
        );
        assert!(
            surface.difficulty_recorded_per_scenario == ORIGINAL_DIFFICULTY_RECORDED_PER_SCENARIO,
            "the measured negative and the committed constant agree"
        );
        assert!(
            !surface.difficulty_recorded_per_scenario,
            "no measured scenario descriptor records a difficulty"
        );
        let vocabulary = surface.scenario_key_vocabulary();
        assert!(
            !vocabulary
                .iter()
                .any(|key| key.to_ascii_lowercase().contains("difficul")),
            "no measured scenario key names a difficulty: {vocabulary:?}"
        );
        // The negative is only as good as the census behind it, so the census is
        // pinned too: a root key (`ace_stats`) and a key that only exists
        // *inside* a nested record (`enemy_skill`, declared by each enemy
        // group) are both present. A walk that stopped at the root would still
        // have produced the negative above, so this is what makes it a
        // measurement of the whole descriptor.
        assert!(
            vocabulary.contains(&"ace_stats"),
            "a root key is in the census: {vocabulary:?}"
        );
        assert!(
            vocabulary.contains(&ENEMY_SKILL_KEY),
            "a key nested inside an enemy-group record is in the census: {vocabulary:?}"
        );
        assert!(
            vocabulary.contains(&SCENARIO_ACE_SKILL_KEY),
            "and one nested inside the ace record: {vocabulary:?}"
        );
        for scenario in &surface.scenarios {
            assert!(
                scenario.keys.iter().any(|key| key == ENEMY_SKILL_KEY),
                "{}: this descriptor's own census descends into its groups",
                scenario.container
            );
        }
        // The two origins stay distinct: a selected step is bounded by the
        // measurement, a designed extension is not.
        assert_ne!(
            DeclaredDifficultyOrigin::SelectedOptionStep { step: 0 },
            DeclaredDifficultyOrigin::DesignedExtension
        );
        for step in 0..ORIGINAL_DIFFICULTY_STEPS {
            assert!(
                DifficultyTier::ALL
                    .iter()
                    .any(|tier| tier.measured_step() == Some(step)),
                "measured step {step} has a declared tier"
            );
        }
    }

    /// The measured per-aircraft skill vocabulary is exactly
    /// [`DeclaredSkillTier`]'s three labels, and a label outside them is
    /// refused rather than carried.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_the_original_declares_exactly_three_ai_skill_tiers() {
        let surface = measured();

        assert_eq!(
            surface.scenarios.len() as u32,
            ORIGINAL_SCENARIO_DESCRIPTOR_COUNT,
            "one scenario descriptor per world group"
        );
        assert_eq!(surface.enemy_group_count, ORIGINAL_ENEMY_GROUP_COUNT);
        assert_eq!(
            surface.skill_label_count, ORIGINAL_SKILL_LABEL_COUNT,
            "group labels plus the named aces"
        );
        assert!(
            surface.agrees_with_declared_skill_tiers(),
            "the measured vocabulary is exactly the declared one: {:?}",
            surface.skill_labels
        );
        assert_eq!(surface.skill_tiers().len(), 3);
        assert!(surface.skill_label_count_of(ENEMY_SKILL_NOVICE) > 0);
        assert!(surface.skill_label_count_of(ENEMY_SKILL_VETERAN) > 0);
        assert!(surface.skill_label_count_of(ENEMY_SKILL_ACE) > 0);

        // Every label resolves; nothing is normalized into the nearest tier.
        for row in &surface.skill_labels {
            assert!(
                DeclaredSkillTier::from_label(&row.label).is_some(),
                "{} is a measured tier",
                row.label
            );
        }
        for unknown in ["ACE", "Ace", "ace ", "elite", "recruit", "", "hard"] {
            assert_eq!(
                DeclaredSkillTier::from_label(unknown),
                None,
                "{unknown:?} is not a measured spelling and must be refused"
            );
        }
    }

    /// The declared skill vocabulary is the one the `.zrd` reader already
    /// spells, so the two cannot drift: the same three constants in
    /// [`crate::stunts`] are what `enemy_skill` is read with.
    #[test]
    fn accept_f32_d_the_declared_skill_vocabulary_is_the_one_the_zrd_reader_spells() {
        assert_eq!(
            DeclaredSkillTier::ALL
                .iter()
                .map(|tier| tier.label())
                .collect::<Vec<_>>(),
            [ENEMY_SKILL_NOVICE, ENEMY_SKILL_VETERAN, ENEMY_SKILL_ACE]
        );
        assert_eq!(DeclaredSkillTier::ALL.len(), 3);
        assert_eq!(ENEMY_SKILL_KEY, "enemy_skill");
        assert_eq!(SCENARIO_ACE_SKILL_KEY, "ace_skill");
    }

    /// The ace is a **nine**-slot integer vector in every measured scenario,
    /// saturated at nine in every slot — and this records the extent *without*
    /// claiming what a slot means.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_every_scenario_declares_a_nine_slot_ace_stat_vector() {
        let surface = measured();
        assert_eq!(
            surface.scenarios.len() as u32,
            ORIGINAL_SCENARIO_DESCRIPTOR_COUNT
        );
        for scenario in &surface.scenarios {
            let stats = scenario
                .ace_stats
                .as_ref()
                .unwrap_or_else(|| panic!("{} declares no ace_stats", scenario.container));
            assert_eq!(
                stats.len(),
                ORIGINAL_ACE_STAT_SLOTS,
                "{}: the ace vector has nine slots",
                scenario.container
            );
            for (slot, value) in stats.iter().enumerate() {
                assert_eq!(
                    *value, ORIGINAL_ACE_STAT_MAX,
                    "{}: slot {slot} is saturated at the measured maximum",
                    scenario.container
                );
            }
            assert!(
                !scenario
                    .keys
                    .iter()
                    .any(|key| key == "ace_damage" || key == "ace_health"),
                "{}: the measured ace record names no damage or health slot",
                scenario.container
            );
        }
    }

    /// The mission types the installation measures: three of them. This is the
    /// vocabulary a difficulty probe's scenario may be built from — no invented
    /// fourth mission type.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_the_scenarios_declare_exactly_three_measured_mission_types() {
        let surface = measured();
        assert_eq!(
            surface.mission_types(),
            vec!["dogfight_squadron", "stunt_flying", "zeppelin_run"]
        );
    }

    /// The anti-drift check between the two crates' copies of the measured
    /// step count.
    ///
    /// `cs_sim::ai::combat` holds its own [`ORIGINAL_DIFFICULTY_STEPS`] because
    /// it may not depend on `cs_content` (AGENTS rule 7), so nothing else
    /// compares the two. This test is where they are compared, and it runs on
    /// the side that owns the measurement: it re-derives the count from the
    /// installation through [`original_ai_surface`] and then reads the
    /// runtime's mirror and its tier mapping.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f32_d_retail_the_runtime_tier_mirror_agrees_with_the_measured_option() {
        use cs_sim::ai::combat::DifficultyTier as RuntimeTier;

        let surface = measured();
        assert_eq!(
            surface.difficulty_steps, ORIGINAL_DIFFICULTY_STEPS,
            "the header's bound"
        );
        assert_eq!(
            surface.populated_difficulty_ids, ORIGINAL_DIFFICULTY_STEPS,
            "and the shipped string image meets it"
        );
        assert_eq!(
            cs_sim::ai::combat::ORIGINAL_DIFFICULTY_STEPS,
            ORIGINAL_DIFFICULTY_STEPS,
            "the runtime mirrors the measured option's step count"
        );
        assert_eq!(
            RuntimeTier::measured_tier_count() as u32,
            surface.difficulty_steps,
            "so every measured step has a declared tier and no more"
        );
        let mut steps: Vec<u32> = RuntimeTier::ALL
            .iter()
            .filter_map(|tier| tier.measured_step())
            .collect();
        assert_eq!(
            steps,
            (0..surface.difficulty_steps).collect::<Vec<u32>>(),
            "the measured steps are covered contiguously, in option order"
        );
        steps.dedup();
        assert_eq!(steps.len(), surface.difficulty_steps as usize);
        assert!(
            !surface.difficulty_recorded_per_scenario,
            "no measured record binds a step to a scenario, so a tier is a selection"
        );

        // The per-aircraft skill vocabulary is measured too, and it is a
        // *different* thing from the difficulty option: nothing measured maps
        // one onto the other, so this asserts only that both are three and
        // never that they are the same three.
        assert_eq!(
            surface.skill_tiers().len(),
            3,
            "three measured AI skill tiers"
        );
        assert_eq!(
            surface.skill_labels,
            vec![
                SkillLabelRow {
                    label: ENEMY_SKILL_ACE.to_owned(),
                    count: surface.skill_label_count_of(ENEMY_SKILL_ACE),
                },
                SkillLabelRow {
                    label: ENEMY_SKILL_NOVICE.to_owned(),
                    count: surface.skill_label_count_of(ENEMY_SKILL_NOVICE),
                },
                SkillLabelRow {
                    label: ENEMY_SKILL_VETERAN.to_owned(),
                    count: surface.skill_label_count_of(ENEMY_SKILL_VETERAN),
                },
            ]
            .into_iter()
            .filter(|row| row.count > 0)
            .collect::<Vec<_>>(),
            "the measured label vocabulary, sorted by label"
        );
        assert_eq!(surface.skill_label_count, ORIGINAL_SKILL_LABEL_COUNT);
        assert_eq!(
            surface.enemy_group_count as usize + surface.scenarios.len(),
            surface.skill_label_count as usize,
            "one label per enemy group plus one per named ace"
        );
    }

    /// The negative the measurement must not be able to fake: the *declared*
    /// fixture carries no measured step, so a synthetic profile can never be
    /// reported as one of the original's three.
    #[test]
    fn accept_f32_d_the_declared_fixture_stands_for_no_measured_step() {
        let rules = declared_synthetic_combat_rules();
        assert_eq!(rules.origin(), &Origin::SyntheticFixture);
        for profile in rules.difficulties() {
            assert_eq!(
                profile.provenance().class,
                ClaimStatus::Designed,
                "a declared difficulty profile is designed, never measured"
            );
            assert!(
                !profile
                    .overrides()
                    .iter()
                    .any(|change| change.provenance.class == ClaimStatus::VerifiedOriginal),
                "no fixture override claims original measurement"
            );
        }
        assert_eq!(
            ORIGINAL_DIFFICULTY_OPTION_IDS.len() as u32,
            ORIGINAL_DIFFICULTY_STEPS
        );
        assert!(
            ORIGINAL_DIFFICULTY_OPTION_IDS
                .iter()
                .all(|id| *id >= ORIGINAL_DIFFICULTY_OPTION_FIRST_ID
                    && *id < ORIGINAL_DIFFICULTY_OPTION_BOUND_ID),
            "every id the option occupies lies inside the measured bound"
        );
    }
}

// ------------------------------------------------ f32-d evidence ----

/// The F32-D evidence-report harness (`docs/contracts/CLI-EVIDENCE.md`,
/// `schemas/evidence.schema.json`).
///
/// Not named `accept_f32_d_*`: it is not part of the acceptance suite and it
/// fails loudly when its inputs are missing rather than reporting a pass it
/// never earned.
///
/// 1. `cargo test --workspace --locked -- accept_f32_d_ --include-ignored 2>&1 |
///    tee private/evidence/F32-D/cargo-test.log` (note the exit status)
/// 2. ```sh
///    CS_EVIDENCE_DIR=private/evidence/F32-D \
///    CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
///    CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f32_d_ --include-ignored" \
///    CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
///      cargo test --locked -p cs_content --lib --lib ai::f32_d::evidence_report_f32_d \
///        -- --ignored --exact
///    ```
/// 3. `python3 tools/validate_evidence.py private/evidence/F32-D/acceptance.json
///    --artifact-root private/evidence/F32-D --require-pass`
/// 4. Commit a copy as `docs/findings/evidence/F32-D.json`.
///
/// Two artifacts are second, independent **production observations** of the
/// same run, not restatements of the suite: `ai-surface.json` is a fresh
/// [`original_ai_surface`] measurement of the installation, and
/// `difficulty-probe.json` is a fresh
/// `cs_sim::ai::combat::CombatRuntime::probe_difficulties` replay. Both carry
/// identifiers, counts, digests and aggregates — never original display text.
#[cfg(test)]
mod evidence_report_f32_d {
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use cs_assets::install::sha256;

    use super::original_ai_surface;

    fn env_var(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!("{name} is required: an evidence report that guesses its inputs is not evidence")
        })
    }

    /// A caller-supplied path, resolved against the workspace root so a
    /// relative spelling means the same thing it does in the documented
    /// command line.
    fn workspace_path(as_described: &str) -> PathBuf {
        let path = Path::new(as_described);
        if path.is_absolute() {
            return path.to_path_buf();
        }
        workspace_root().join(path)
    }

    fn git(args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).expect("git prints utf-8")
    }

    fn rustc_version() -> String {
        let output = Command::new("rustc")
            .arg("--version")
            .output()
            .expect("rustc runs");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// The version a package is pinned at in the **committed** lock file, read
    /// from Git rather than from the working tree so a report describes the
    /// tree it names and not whatever is checked out beside it.
    fn locked_version(package: &str) -> String {
        let text = git(&["show", "HEAD:Cargo.lock"]);
        let mut wanted = false;
        for line in text.lines() {
            if line.trim_start().starts_with("[[package]]") {
                wanted = false;
            } else if let Some(name) = line.trim_start().strip_prefix("name = \"") {
                wanted = name.trim_end_matches('"') == package;
            } else if let Some(version) = line.trim_start().strip_prefix("version = \"")
                && wanted
            {
                return version.trim_end_matches('"').to_owned();
            }
        }
        panic!("package {package:?} must be in the committed Cargo.lock");
    }

    #[derive(Debug, Default)]
    struct Suite {
        passed: u64,
        failed: u64,
        ignored: u64,
        assertions: Vec<(String, &'static str)>,
    }

    fn parse_suite(log: &str) -> Suite {
        let mut suite = Suite::default();
        let mut pending: VecDeque<String> = VecDeque::new();
        for line in log.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("test result:") {
                for (count, kind) in summary_fields(trimmed) {
                    match kind {
                        "passed" => suite.passed += count,
                        "failed" => suite.failed += count,
                        "ignored" => suite.ignored += count,
                        _ => {}
                    }
                }
                continue;
            }
            if pending.front().is_some() {
                if trimmed == "ok" {
                    let name = pending.pop_front().expect("pending test");
                    record(&mut suite, name, "pass");
                    continue;
                }
                if trimmed == "FAILED" {
                    let name = pending.pop_front().expect("pending test");
                    record(&mut suite, name, "fail");
                    continue;
                }
            }
            let mut cursor = trimmed;
            while let Some(position) = cursor.find("test ") {
                let after = &cursor[position + 5..];
                let Some(separator) = after.find(" ... ") else {
                    break;
                };
                let name = after[..separator].to_owned();
                let tail = &after[separator + 5..];
                cursor = tail;
                if !name.contains("accept_f32_d_") {
                    continue;
                }
                match tail.split_whitespace().next() {
                    Some("ok") => record(&mut suite, name, "pass"),
                    Some("FAILED") => record(&mut suite, name, "fail"),
                    _ => pending.push_back(name),
                }
            }
        }
        suite.assertions.dedup_by(|left, right| left.0 == right.0);
        suite
    }

    fn summary_fields(line: &str) -> Vec<(u64, &str)> {
        let mut fields = Vec::new();
        for segment in line["test result:".len()..].split(';') {
            let words: Vec<&str> = segment.split_whitespace().collect();
            for pair in words.windows(2) {
                if let Ok(count) = pair[0].parse::<u64>()
                    && matches!(pair[1], "passed" | "failed" | "ignored")
                {
                    fields.push((count, pair[1]));
                    break;
                }
            }
        }
        fields
    }

    fn record(suite: &mut Suite, name: String, status: &'static str) {
        if suite.assertions.iter().any(|(seen, _)| *seen == name) {
            return;
        }
        suite.assertions.push((name, status));
    }

    fn artifact(source: &Path, kind: &str) -> (String, String, String) {
        let name = source
            .file_name()
            .expect("an artifact has a file name")
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(source).expect("an artifact is readable");
        let digest = sha256(&bytes).to_hex();
        (name, digest, kind.to_owned())
    }

    fn jstr(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for character in value.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    fn str_array(items: &[String]) -> String {
        format!(
            "[{}]",
            items
                .iter()
                .map(|item| jstr(item))
                .collect::<Vec<_>>()
                .join(",")
        )
    }

    fn assertion_array(assertions: &[(String, &'static str)]) -> String {
        let rows: Vec<String> = assertions
            .iter()
            .map(|(id, status)| {
                format!(
                    "{{\"id\":{},\"status\":\"{status}\",\"evidence\":[\"cargo-test.log\",\"ai-surface.json\",\"difficulty-probe.json\"]}}",
                    jstr(id)
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    fn artifact_array(artifacts: &[(String, String, String)]) -> String {
        let rows: Vec<String> = artifacts
            .iter()
            .map(|(path, digest, kind)| {
                format!(
                    "{{\"path\":{},\"sha256\":\"{digest}\",\"kind\":{}}}",
                    jstr(path),
                    jstr(kind)
                )
            })
            .collect();
        format!("[{}]", rows.join(","))
    }

    fn iso_utc_now() -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_secs();
        let (year, month, day, hour, minute, second) = civil_from_unix(seconds as i64);
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    /// The proleptic-Gregorian civil date of a Unix second count.
    fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
        let days = seconds.div_euclid(86_400);
        let rest = seconds.rem_euclid(86_400);
        let (hour, minute, second) = (
            (rest / 3_600) as u32,
            ((rest % 3_600) / 60) as u32,
            (rest % 60) as u32,
        );
        // Howard Hinnant's `civil_from_days`, which is exact for the whole
        // i64 range this report needs.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
        let year = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (
            if month <= 2 { year + 1 } else { year },
            month,
            day,
            hour,
            minute,
            second,
        )
    }

    /// What the report's `review.method` says.
    ///
    /// Long by design: the machine-readable record is where a limitation
    /// survives the branch, so every unmeasured original behaviour this stage
    /// *did not* resolve is named here with its resolving task, rather than
    /// only in the committed finding.
    const REVIEW_METHOD: &str = concat!(
        "Acceptance suite run locally with the retail capability (CS_GAME_DIR set, ",
        "CS_CAPABILITIES includes retail); this report is derived from the recorded log plus ",
        "two second, independent production observations of the same tree, recorded in ",
        "ai-surface.json (a fresh cs_content::ai::original_ai_surface measurement) and ",
        "difficulty-probe.json (a fresh cs_sim::ai::combat::CombatRuntime::probe_difficulties ",
        "replay). MEASURED (identifiers, counts, digests and aggregates only, no original ",
        "display text): the installation fingerprint b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978 ",
        "and canonical-content fingerprint a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d; ",
        "the engine's own resource header ASSETS/SCRIPTS/RESOURCE.H inside GOSDATA/ASSETS/crimson.rof, ",
        "where IDS_DIFFICULTY is id 109 and the first macro declared after it, IDS_VIEWCOCKPIT, is id ",
        "112, bounding the campaign difficulty option's name list at THREE ids (109..=111); the ",
        "shipped string image GOSDATA/ASSETS/BINARIES/langui.dll (language 1033) populates all three ",
        "of those ids with one non-empty string each, so the bound is met and the option offers three ",
        "steps; the difficulty row is ONE row of the game-options screen - IDS_GO_DIFFICULTY_TITLE at ",
        "1084 with IDS_GO_VIEW_TITLE at 1085 immediately after, and IDS_GO_DIFFICULTY_DESC at 1087 with ",
        "IDS_GO_VIEW_DESC at 1088 immediately after - and the instant-action screens' single ",
        "IDS_IA_DIFFICULTY at 3695 with IDS_IA_PLANES at 3700 after it; the complete key vocabulary ",
        "of all EIGHT instant-action scenario descriptors, at every depth rather than at the root ",
        "only, contains no difficulty key and no measured record anywhere binds a step to a ",
        "scenario, so a difficulty is a SELECTION and not a per-mission value; the per-aircraft AI ",
        "skill vocabulary is exactly THREE labels over FORTY ",
        "declarations - thirty-two enemy groups (four per descriptor) plus eight named aces - with the ",
        "counts the census produced; the mission types the descriptors declare are exactly three; and ",
        "ace_stats is a NINE-slot integer vector saturated at 9 in every slot of every one of the eight ",
        "descriptors, with no measured key naming a damage or health slot. AC04: the production ",
        "cs_sim::ai::combat::CombatRuntime::probe_difficulties replays one mission-combat scenario 24 ",
        "times at each of the four declared tiers, 600 ticks per run, from one recorded root seed under ",
        "the contract's domain recipe (stream seed = SplitMix64 of root_seed ^ run << 32 ^ the probe's ",
        "own domain constant); run n ",
        "replays byte-identical geometry and threat stamps at every tier (a per-index 64-bit FNV-1a ",
        "digest over every position and threat stamp, tier-invariant), the weapons snapshot is one ",
        "snapshot for every tier (tier-invariant digest), and every tier replayed the same number of ",
        "ticks, so the only thing that differs is the profile resolve_profile selected - which is F32 ",
        "non-negotiable 1 and non-negotiable 2 checked mechanically rather than by review. The measured ",
        "outcome over the synthetic fixture: the ticks that answered an authoritative attack against ",
        "the protected actor rose at every step (relaxed 190.04 per run, standard 303.00, hard 351.00, ",
        "elite 375.00) while the deferred-threat count fell (184.00, 97.00, 49.00, 25.00 per run), with ",
        "the engagements, range refusals and applied formation recoveries identical at every tier as ",
        "controls; the relaxed tier's answer count has a non-zero population variance (380.87 of the ",
        "mean square) while the elite tier's is exactly zero, so the comparison is a distribution and ",
        "not four deterministic traces. Each run applies exactly ONE recovery, the declared ",
        "assigned-target-destruction path at the tick the attacker disappears: the probe also loses a ",
        "formation FOLLOWER mid-run, and a follower is not a leader, so it is a membership change and ",
        "raises no recovery - DifficultyProbeRun::recovery_triggers measures the attribution rather than ",
        "asserting it. A degenerate roster that resolves ONE profile for all four tiers ",
        "measures one distinct profile and no difference at all, which is the negative the probe exists ",
        "to catch. FIDELITY LIMITATIONS (unmeasured original behavior, listed in this report's ",
        "unknowns and in review.method, recorded in ai-surface.json, ",
        "in the committed finding docs/findings/2026-10-03-f32-d-original-ai-roles-and-difficulty.md and ",
        "in the filed follow-up tasks; none of them is claimed by this report): claim ",
        "f32.d.limit.difficulty_effects - the option's THREE steps are measured but nothing in any ",
        "shipped file says what any step changes, so cs_content::ai::SkillKnob remains a designed ",
        "vocabulary and the runtime's tier profiles remain designed alternatives under F32 ",
        "non-negotiable 1, and nothing yet REQUIRES a tier's author to say which of its knobs are ",
        "evidence-backed and which are alternatives (resolving task #566 F32-DIFFICULTY-EFFECT); claim ",
        "f32.d.limit.skill_tier_effects - the per-aircraft skill tiers are measured as three labels but ",
        "nothing measures what a novice, a veteran or an ace does, so cs_sim's SkillKnobs for those tiers ",
        "remain designed, DeclaredSkillTier is a closed vocabulary with a refusal and NO production ",
        "consumer yet, and an importer can still carry an unrecognised enemy_skill label past it ",
        "(resolving task #567 F32-SKILL-EFFECT); claim f32.d.limit.ace_stats - the ",
        "nine ace stat slots and their maximum are measured but no file names a slot, their order, or ",
        "what a value below the maximum does, so a later named nine-field ace struct would be guessing the original nine meanings while looking measured, and the runtime must not claim what any slot controls ",
        "(resolving task #568 F32-ACE-STATS); claim f32.d.limit.ai_roles - the original's AI ROLE set is not ",
        "in any measured file; cs_content::ai::DeclaredCombatRole's seven roles and cs_sim's mirror ",
        "remain designed vocabulary with no code check distinguishing them, and only the IA scenario's ",
        "skill labels and squadron shape are measured (resolving task #569 F32-ROLES); claim ",
        "f32.d.limit.difficulty_naming - the original's three ",
        "step names live in its shipped localizable string image and are deliberately NOT reproduced in ",
        "Git or in any report (AGENTS rule 3), so the declared labels relaxed/standard/hard/elite claim ",
        "nothing about the original's wording and the tier-to-step mapping is positional only ",
        "(resolving task F32-LOWERING, #551); claim f32.d.limit.per_mission_difficulty - measured: NO ",
        "measured record binds a difficulty step to a scenario or mission, so this engine has no way to ",
        "read one from content and cs_content::ai::DeclaredDifficultyOrigin::SelectedOptionStep is the ",
        "only honest origin a declared profile may claim (resolving task F32-LOWERING, #551); claim ",
        "f32.d.limit.probe_geometry - every number in the probe's scenario (positions, speeds, the ",
        "120-tick attack period, the jitter bound) is newly authored project design, because no measured ",
        "file describes an original AI encounter; the probe therefore measures THIS engine's per-tier ",
        "decision behavior, is not evidence about the original's numbers, and carries no machine-readable ",
        "marker saying so (resolving task #570 F32-PROBE-GEOMETRY); claim f32.d.limit.fire_discipline - ",
        "the runtime REPORTS a profile's ",
        "fire_discipline_ticks and aim_error_rad and neither is enforced across ticks, so the probe's ",
        "firing-tick counts are a control that cannot discriminate a tier, two tiers differing ONLY in ",
        "those two knobs measure as identical, and the per-actor fire-discipline state F32-B left ",
        "reported is still owned by no session (resolving task #571 F32-FIRE-DISCIPLINE). The claim is ",
        "implemented: a code and test pass awards nothing above that. ",
        "retail here is read access to original files, NOT evidence that the original executable ran, ",
        "and no agent review replaces the owner's human approval. Validated with ",
        "tools/validate_evidence.py --require-pass."
    );

    /// What the report's `unknowns` says: one line per unresolved original
    /// behaviour, each naming the claim id, what is unknown, the content it
    /// affects and the task that resolves it.
    ///
    /// The same eight claims are spelled out in [`REVIEW_METHOD`]. They are in
    /// *both* places on purpose: `unknowns` is the machine-readable field a
    /// validator and a reader look at first, and a limitation that lives only in
    /// prose is a limitation a tool can drop. Nothing here is removed to make a
    /// check pass.
    const UNKNOWNS: &[&str] = &[
        "f32.d.limit.difficulty_effects: the original's difficulty option has three measured steps and no measured file says what any step changes; cs_content::ai::SkillKnob and the runtime's tier profiles are designed alternatives under F32 non-negotiable 1, and nothing forces a tier's author to say which knobs are evidence-backed (resolving task #566 F32-DIFFICULTY-EFFECT)",
        "f32.d.limit.skill_tier_effects: the measured per-aircraft labels novice/veteran/ace carry no measured effect, so cs_sim's skill knobs for them are designed and cs_content::ai::DeclaredSkillTier is a closed vocabulary with no production consumer yet (resolving task #567 F32-SKILL-EFFECT)",
        "f32.d.limit.ace_stats: nine ace stat slots and the maximum of nine are measured, but no file names a slot, its order, or what a value below the maximum does, and no measured key names a damage or health slot (resolving task #568 F32-ACE-STATS)",
        "f32.d.limit.ai_roles: the original's AI role set is in no measured file; cs_content::ai::DeclaredCombatRole's seven roles and cs_sim's mirror are designed vocabulary with no code check distinguishing them (resolving task #569 F32-ROLES)",
        "f32.d.limit.difficulty_naming: the original's three step names live in its shipped localizable string image and are deliberately not reproduced in Git or in any report, so the declared tier labels claim nothing about the original's wording and the tier-to-step mapping is positional only (resolving task F32-LOWERING, #551)",
        "f32.d.limit.per_mission_difficulty: measured negative - no key at any depth of any of the eight scenario descriptors names a difficulty, so this engine cannot read a per-mission difficulty from content (resolving task F32-LOWERING, #551)",
        "f32.d.limit.probe_geometry: every number in the probe's scenario is newly authored project design because no measured file describes an original AI encounter, so the probe measures this engine's per-tier decision behavior and is not evidence about the original's numbers (resolving task #570 F32-PROBE-GEOMETRY)",
        "f32.d.limit.fire_discipline: the runtime reports fire_discipline_ticks and aim_error_rad without enforcing them, so two tiers differing only in those two knobs measure as identical and the probe's firing-tick control cannot discriminate a tier (resolving task #571 F32-FIRE-DISCIPLINE)",
    ];

    #[test]
    #[ignore = "evidence harness: needs CS_EVIDENCE_DIR, CS_CANDIDATE_TREE, CS_EVIDENCE_ARGV, CS_EVIDENCE_EXIT_CODE, CS_EVIDENCE_REVIEWER, CS_GAME_DIR"]
    fn evidence_report_f32_d_writes_the_acceptance_report() {
        let evidence_dir = workspace_path(&env_var("CS_EVIDENCE_DIR"));
        std::fs::create_dir_all(&evidence_dir).expect("the evidence directory is created");
        let tree = env_var("CS_CANDIDATE_TREE");
        let argv = env_var("CS_EVIDENCE_ARGV");
        let exit_code: i64 = env_var("CS_EVIDENCE_EXIT_CODE")
            .parse()
            .expect("CS_EVIDENCE_EXIT_CODE is an integer");
        let reviewer = env_var("CS_EVIDENCE_REVIEWER");
        let root = workspace_path(&env_var("CS_GAME_DIR"));

        let log_path = evidence_dir.join("cargo-test.log");
        let log = std::fs::read_to_string(&log_path)
            .unwrap_or_else(|error| panic!("{} must exist: {error}", log_path.display()));
        let suite = parse_suite(&log);
        assert!(
            suite.failed == 0 && suite.passed > 0,
            "the recorded log must show a passing, nonempty run: {suite:?}"
        );

        // Artifact 1: a fresh production measurement of the installation.
        let surface = original_ai_surface(&root).expect("the AI surface measures");
        assert!(
            surface.agrees_with_committed_steps(),
            "the installation must agree with the committed step count"
        );
        assert!(
            surface.agrees_with_declared_skill_tiers(),
            "the installation must agree with the declared skill vocabulary"
        );
        let surface_json = surface_json(&surface);
        let surface_path = evidence_dir.join("ai-surface.json");
        std::fs::write(&surface_path, surface_json).expect("the surface artifact is written");

        // Artifact 2: a fresh production replay of the probe.
        let (probe, probe_seed, probe_ticks) = probe_json();
        let probe_path = evidence_dir.join("difficulty-probe.json");
        std::fs::write(&probe_path, probe).expect("the probe artifact is written");

        let artifacts = vec![
            artifact(&log_path, "log"),
            artifact(&surface_path, "json"),
            artifact(&probe_path, "json"),
        ];

        let engine = format!(
            "{{\"rust\":{},\"bevy\":{},\"avian\":{}}}",
            jstr(&rustc_version()),
            jstr(&locked_version("bevy")),
            jstr(&locked_version("avian3d"))
        );
        let command = format!(
            "{{\"argv\":{},\"cwd\":{},\"exit_code\":{exit_code}}}",
            str_array(
                &argv
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            ),
            jstr(&workspace_root().display().to_string())
        );
        let source = format!(
            "{{\"install_sha256\":{},\"content_sha256\":{}}}",
            jstr(&surface.install_sha256),
            jstr(&surface.content_sha256)
        );
        let tests = format!(
            "{{\"discovered\":{},\"executed\":{},\"passed\":{},\"failed\":{},\"ignored\":{}}}",
            suite.passed + suite.failed + suite.ignored,
            suite.passed + suite.failed,
            suite.passed,
            suite.failed,
            suite.ignored
        );
        let review = format!(
            "{{\"identity\":{},\"method\":{}}}",
            jstr(&reviewer),
            jstr(REVIEW_METHOD)
        );
        let unknowns = str_array(
            &UNKNOWNS
                .iter()
                .map(|unknown| (*unknown).to_owned())
                .collect::<Vec<_>>(),
        );
        assert!(
            !unknowns.is_empty() && UNKNOWNS.len() >= 8,
            "a retail report that leaves its unresolved original behaviour out of \
             `unknowns` is not reporting it"
        );

        let report = format!(
            concat!(
                "{{\"schema_version\":1,\"task_id\":\"F32-D\",\"candidate_tree\":{tree},",
                "\"engine\":{engine},\"created_at\":{created},\"command\":{command},",
                "\"source\":{source},\"seed\":{seed},",
                "\"ticks\":{{\"start\":0,\"end\":{ticks}}},\"overrides\":[],",
                "\"capabilities\":[\"retail\",\"synthetic\"],\"tests\":{tests},",
                "\"assertions\":{assertions},\"artifacts\":{artifacts},\"unknowns\":{unknowns},",
                "\"review\":{review},\"claim\":\"implemented\"}}\n"
            ),
            tree = jstr(&tree),
            engine = engine,
            created = jstr(&iso_utc_now()),
            command = command,
            source = source,
            seed = probe_seed,
            ticks = probe_ticks.saturating_sub(1),
            tests = tests,
            assertions = assertion_array(&suite.assertions),
            artifacts = artifact_array(&artifacts),
            unknowns = unknowns,
            review = review,
        );
        std::fs::write(evidence_dir.join("acceptance.json"), report)
            .expect("the acceptance report is written");
    }

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("the workspace root is two levels above crates/cs_content")
            .to_path_buf()
    }

    /// The measured surface as JSON: identifiers, counts and digests only.
    fn surface_json(surface: &super::OriginalAiSurface) -> String {
        let scenarios: Vec<String> = surface
            .scenarios
            .iter()
            .map(|scenario| {
                format!(
                    "{{\"container\":{},\"mission_type\":{},\"enemy_groups\":{},\"skill_labels\":{},\"keys\":{},\"ace_stats\":{}}}",
                    jstr(&scenario.container),
                    scenario
                        .mission_type
                        .as_ref()
                        .map_or_else(|| "null".to_owned(), |value| jstr(value)),
                    scenario.enemy_groups,
                    str_array(&scenario.skills.clone()),
                    str_array(&scenario.keys.clone()),
                    scenario
                        .ace_stats
                        .as_ref()
                        .map_or_else(|| "null".to_owned(), |slots| {
                            format!(
                                "[{}]",
                                slots.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")
                            )
                        }),
                )
            })
            .collect();
        let labels: Vec<String> = surface
            .skill_labels
            .iter()
            .map(|row| format!("{{\"label\":{},\"count\":{}}}", jstr(&row.label), row.count))
            .collect();
        format!(
            "{{\"install_sha256\":{},\"content_sha256\":{},\"difficulty_option\":{{\"macro\":{},\"first_id\":{},\"bound_macro\":{},\"bound_id\":{},\"steps\":{},\"populated_ids\":{},\"ids\":[{}],\"game_options_title\":{{\"macro\":{},\"id\":{}}},\"game_options_desc\":{{\"macro\":{},\"id\":{}}},\"ia_label\":{{\"macro\":{},\"id\":{}}},\"recorded_per_scenario\":{}}},\"reader_archives\":{},\"declared_header_ids\":{},\"enemy_groups\":{},\"skill_label_count\":{},\"skill_labels\":[{}],\"mission_types\":{},\"scenario_count\":{},\"scenarios\":[{}]}}\n",
            jstr(&surface.install_sha256),
            jstr(&surface.content_sha256),
            jstr(super::ORIGINAL_DIFFICULTY_OPTION_MACRO),
            surface.difficulty_macro_id,
            jstr(super::ORIGINAL_DIFFICULTY_OPTION_BOUND_MACRO),
            surface.difficulty_bound_id,
            surface.difficulty_steps,
            surface.populated_difficulty_ids,
            surface
                .difficulty_ids()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
            jstr(&surface.game_option_title.0),
            surface.game_option_title.1,
            jstr(&surface.game_option_desc.0),
            surface.game_option_desc.1,
            jstr(&surface.ia_difficulty_label.0),
            surface.ia_difficulty_label.1,
            surface.difficulty_recorded_per_scenario,
            surface.reader_archives,
            surface.declared_header_ids,
            surface.enemy_group_count,
            surface.skill_label_count,
            labels.join(","),
            str_array(
                &surface
                    .mission_types()
                    .into_iter()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            ),
            surface.scenarios.len(),
            scenarios.join(","),
        )
    }

    /// The probe replay as JSON: aggregates and digests, no geometry.
    fn probe_json() -> (String, i64, u64) {
        use cs_sim::ai::combat::{
            DifficultyTier, synthetic_combat_runtime, synthetic_difficulty_probe_spec,
        };

        let spec = synthetic_difficulty_probe_spec();
        let report = synthetic_combat_runtime()
            .probe_difficulties(&spec)
            .expect("the probe replays");
        let tiers: Vec<String> = report
            .tiers
            .iter()
            .map(|tier| {
                format!(
                    "{{\"tier\":{},\"measured_step\":{},\"runs\":{},\"ticks_per_run\":{},\"engagements\":{},\"mean_engagements\":{},\"variance_engagements\":{},\"protected_answers\":{},\"mean_protected_answers\":{},\"variance_protected_answers\":{},\"deferred_threats\":{},\"noticed_threats\":{},\"range_rejects\":{},\"recoveries\":{},\"firing_ticks\":{},\"arsenal_digest\":{},\"profile_digest\":{},\"geometry_digests\":[{}]}}",
                    jstr(tier.tier.label()),
                    tier.measured_step.map_or_else(|| "null".to_owned(), |step| step.to_string()),
                    tier.runs,
                    tier.ticks_per_run,
                    tier.engagements,
                    tier.mean_engagements,
                    tier.variance_engagements,
                    tier.protected_answers,
                    tier.mean_protected_answers,
                    tier.variance_protected_answers,
                    tier.deferred_threats,
                    tier.noticed_threats,
                    tier.range_rejects,
                    tier.recoveries,
                    tier.firing_ticks,
                    tier.arsenal_fingerprints.first().copied().unwrap_or(0),
                    tier.profile_fingerprints.first().copied().unwrap_or(0),
                    tier.geometry_fingerprints
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                )
            })
            .collect();
        let comparisons: Vec<String> = report
            .comparisons
            .iter()
            .map(|comparison| {
                format!(
                    "{{\"lower\":{},\"higher\":{},\"engagements\":{},\"protected_answers\":{},\"deferred_threats\":{}}}",
                    jstr(comparison.lower.label()),
                    jstr(comparison.higher.label()),
                    jstr(&format!("{:?}", comparison.engagements).to_lowercase()),
                    jstr(&format!("{:?}", comparison.protected_answers).to_lowercase()),
                    jstr(&format!("{:?}", comparison.deferred_threats).to_lowercase()),
                )
            })
            .collect();
        let json = format!(
            "{{\"seed\":{},\"ticks\":{},\"runs_per_tier\":{},\"domain\":{},\"declared_tiers\":{},\"measured_steps\":{},\"covers_every_measured_step\":{},\"geometry_is_tier_invariant\":{},\"arsenal_is_tier_invariant\":{},\"clock_is_tier_invariant\":{},\"distinct_profile_count\":{},\"outcomes_differ\":{},\"protected_answers_never_regress\":{},\"measured_tiers\":{},\"tiers\":[{}],\"comparisons\":[{}]}}\n",
            spec.root_seed,
            spec.ticks,
            spec.runs_per_tier,
            cs_sim::ai::combat::DIFFICULTY_PROBE_DOMAIN,
            DifficultyTier::ALL.len(),
            cs_sim::ai::combat::ORIGINAL_DIFFICULTY_STEPS,
            report.covers_every_measured_step(),
            report.geometry_is_tier_invariant(),
            report.arsenal_is_tier_invariant(),
            report.clock_is_tier_invariant(),
            report.distinct_profile_count(),
            report.outcomes_differ(),
            report.protected_answers_never_regress(),
            str_array(
                &report
                    .measured_tiers()
                    .into_iter()
                    .map(|tier| tier.label().to_owned())
                    .collect::<Vec<_>>()
            ),
            tiers.join(","),
            comparisons.join(","),
        );
        (json, spec.root_seed as i64, spec.ticks)
    }
}
