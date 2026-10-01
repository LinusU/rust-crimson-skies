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

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Meters, Radians};

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
    Resolved::Known(cs_types::content::Known::new(
        value,
        provenance.clone(),
    ))
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
            if !(0.0..=MAX_ENGAGEMENT_RANGE_M).contains(&m) {
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
            validate_knob_value(
                SkillKnob::FireDisciplineTicks,
                SkillKnobValue::Ticks(ticks),
            )?;
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
            (SkillKnob::ProtectedActorWeight, &self.protected_actor_weight),
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
    pub fn knob_value(&self, knob: SkillKnob) -> Result<Resolved<SkillKnobValue>, CombatSchemaError> {
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

// ------------------------------------------------ difficulty ----

/// A declared difficulty tier.
///
/// A designed four-step ordering. The original game's difficulty option
/// names, count and effects are unmeasured (F32-D), so these labels are
/// project design and nothing here claims to be the original's wording.
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
    pub const ALL: &'static [DifficultyTier] = &[
        Self::Relaxed,
        Self::Standard,
        Self::Hard,
        Self::Elite,
    ];

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
                write!(f, "combat subject {subject} is neither a mission nor a launchable")
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
            } => write!(
                f,
                "knob {knob} is expressed in {expected}, not in {found}"
            ),
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
        (
            SkillKnob::ProtectedActorWeight,
            SkillKnobValue::Weight(4.0),
        ),
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
        (
            SkillKnob::ProtectedActorWeight,
            SkillKnobValue::Weight(4.0),
        ),
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
