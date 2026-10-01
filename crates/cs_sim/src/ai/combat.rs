//! Combat AI: roles, skill profiles, target priority and decision traces
//! (F32-A).
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stage
//! `### F32-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage **F32-A** defines the typed contract and the minimal synthetic
//! fixture; it is not the whole combat runtime. The consumer half of the
//! combat-AI contract lives here; the provenance-carrying producer record an
//! importer will emit is `cs_content::ai`. The conversion boundary between
//! them is `cs_app::ai::combat` (F32-B lowers roles and the priority
//! policy, F32-C lowers ace variants, difficulty profiles and formations).
//!
//! [`combat`] declares:
//!
//! * [`CombatRole`], the seven behaviors the F32 sheet names, and
//!   [`RoleAssignment`], the *script-assigned* role of one actor together
//!   with the actor it protects and the formation slot it holds. Role
//!   selection is data a mission assigns, not something the planner infers.
//! * [`SkillProfile`], the lowered [`SkillKnobs`] and [`PriorityPolicy`] one
//!   role runs under, and [`CombatPlanner`], the per-session authority that
//!   holds one profile per role and one [`RecoveryPolicySet`] per
//!   formation. A profile is the *lowered* form of a declared record, so a
//!   value that content could not evidence never reached this point.
//! * [`CandidateView`] and [`ThreatEvidence`]: the only knowledge the
//!   planner has. Candidates are the ones the approved perception model
//!   reported, their allegiance is the one
//!   [`crate::targeting`] resolved, and a threat is an authoritative
//!   [`crate::damage::HitEventId`] — never proximity, never "every hostile
//!   in radius" (non-negotiable 3 and 4).
//! * [`CombatRequest`] in, [`CombatDecision`] out. [`CombatPlanner::decide`]
//!   is a pure function of the planner's immutable policy and one typed
//!   request, and the total `(score desc, distance asc, ActorId asc)`
//!   selection order makes the outcome independent of the order the ECS
//!   presented the candidates in.
//! * [`DecisionTrace`]: the record the decision carries — every candidate's
//!   gate verdict, its reaction state, each scored term with its weight and
//!   contribution, the total, and the *separate* fire veto. F32-D's
//!   consumer trace and any later difficulty comparison read this record;
//!   a decision without it would be unverifiable.
//!
//! # Hostility, friendly fire and line of fire are three predicates
//!
//! Non-negotiable 3 keeps them apart. Hostility is a gate: only a *declared*
//! hostile candidate is selectable. The fire veto is reported
//! independently on the selected target, so "this is my target" and "I may
//! shoot past this friendly" are separate answers and a friendly in the
//! line of fire never silently becomes a non-target.
//!
//! # Designed vocabulary, not original data
//!
//! The original game's AI roles, priority order, reaction times, aim error,
//! engagement ranges, formation recovery and difficulty mapping are
//! **unmeasured** (F32 "Research boundary"; F32-D's retail stage). Every
//! constant, bound and fixture in this module is newly authored project
//! design, recorded in
//! `docs/findings/2026-10-01-f32-a-combat-roles-skill-knobs-and-decision-traces.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

use std::collections::BTreeMap;
use std::fmt;

use cs_types::Tick;
use cs_types::evidence::ClaimId;
use cs_types::space::WorldPosition;

use crate::damage::{ActorId, DamageNodeKey, HitEventId};
use crate::targeting::Allegiance;

// ---------------------------------------------------------- bounds ----

/// The largest reaction delay a profile may carry, in ticks: one minute at
/// a 60 Hz simulation tick. A designed bound, mirroring
/// `cs_content::ai::MAX_REACTION_TICKS`.
pub const MAX_REACTION_TICKS: u64 = 3_600;

/// The largest fire discipline delay a profile may carry, in ticks. A
/// designed bound, as [`MAX_REACTION_TICKS`].
pub const MAX_FIRE_DISCIPLINE_TICKS: u64 = 3_600;

/// The largest threat window a profile may carry, in ticks. A designed
/// bound, as [`MAX_REACTION_TICKS`].
pub const MAX_THREAT_WINDOW_TICKS: u64 = 3_600;

/// The largest engagement range a profile may carry, in meters. A designed
/// bound, not a measured weapon or sensor range.
pub const MAX_ENGAGEMENT_RANGE_M: f64 = 20_000.0;

/// The largest aim-error half-angle a profile may carry, in radians. A
/// designed bound: a quarter turn.
pub const MAX_AIM_ERROR_RAD: f64 = std::f64::consts::FRAC_PI_2;

/// The largest absolute priority weight a profile may carry. A designed
/// bound that keeps a mis-scaled weight from silently outvoting every other
/// term.
pub const MAX_PRIORITY_WEIGHT: f64 = 1_000.0;

/// Positions closer than this are treated as the same place when a
/// proximity term is normalized, so a zero-length vector never produces a
/// NaN. A numerical guard, not a scale.
pub const PROXIMITY_EPSILON_M: f64 = 1e-9;

// ----------------------------------------------------------- role ----

/// The runtime mirror of `cs_content::ai::DeclaredCombatRole`.
///
/// The boundary maps the two field-wise; this crate cannot depend on
/// `cs_content` (`docs/01-ARCHITECTURE.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CombatRole {
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

impl CombatRole {
    /// Every role, in a stable order.
    pub const ALL: &'static [CombatRole] = &[
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
    /// requirement, not a tuning value.
    #[must_use]
    pub const fn needs_ordnance(self) -> bool {
        matches!(self, Self::BomberRun | Self::TorpedoRun)
    }
}

impl fmt::Display for CombatRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What a role is allowed to shoot with: the lowered mirror of
/// `cs_content::ai::RoleArsenal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct RoleArsenal {
    /// Whether the role is declared to use gun mounts.
    pub gun: bool,
    /// Whether the role is declared to use ordnance hardpoints.
    pub ordnance: bool,
}

impl RoleArsenal {
    /// No weapon at all.
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

// -------------------------------------------------------- profile ----

/// The lowered reaction, aim, range and fire-cadence knobs of one role.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillKnobs {
    /// Ticks between perceiving a threat and acting on it. A threat younger
    /// than this is *deferred*, not ignored: the trace records both the
    /// threat's age and the delay it is waiting out.
    pub reaction_ticks: u64,
    /// The half-angle of the aim error a firing solution adds.
    pub aim_error_rad: f64,
    /// The range beyond which a candidate is not an engagement.
    pub engagement_range_m: f64,
    /// Ticks the AI waits between two fire decisions.
    pub fire_discipline_ticks: u64,
}

/// The lowered target-priority policy of one role: four weighted terms plus
/// the threat window.
///
/// The weights rank the *declared hostiles* only: hostility is a gate
/// (non-negotiable 3), so a script-assigned objective that is not declared
/// hostile is never selected here, however high `objective_weight` is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PriorityPolicy {
    /// Weight of "this candidate is attacking my protected actor".
    pub protected_actor_weight: f64,
    /// Weight of "this candidate is the script-assigned objective".
    pub objective_weight: f64,
    /// Weight of "this candidate is attacking me".
    pub self_defense_weight: f64,
    /// Weight of proximity, normalized over the engagement range.
    pub proximity_weight: f64,
    /// How many ticks after an attack it still counts as a threat.
    pub threat_window_ticks: u64,
}

impl PriorityPolicy {
    /// The weight of one term.
    #[must_use]
    pub const fn weight(&self, term: PriorityTerm) -> f64 {
        match term {
            PriorityTerm::ProtectedActorThreat => self.protected_actor_weight,
            PriorityTerm::ScriptObjective => self.objective_weight,
            PriorityTerm::SelfDefense => self.self_defense_weight,
            PriorityTerm::Proximity => self.proximity_weight,
        }
    }
}

/// The lowered profile one role runs under: its arsenal requirement, its
/// knobs and its priority policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkillProfile {
    role: CombatRole,
    arsenal: RoleArsenal,
    knobs: SkillKnobs,
    priority: PriorityPolicy,
}

impl SkillProfile {
    /// Assembles and validates one role's profile.
    ///
    /// # Errors
    ///
    /// [`CombatError::NonFiniteKnob`] for a NaN or infinite number,
    /// [`CombatError::KnobOutOfRange`] for a number outside its approved
    /// range, [`CombatError::NoScoredPriorityTerm`] when every weight is
    /// zero — which would make the decision an artifact of the tie-break —
    /// and [`CombatError::RoleArsenalMissing`] for a bomber or torpedo run
    /// that does not declare ordnance.
    pub fn try_new(
        role: CombatRole,
        arsenal: RoleArsenal,
        knobs: SkillKnobs,
        priority: PriorityPolicy,
    ) -> Result<Self, CombatError> {
        check_bound(
            "reaction_ticks",
            knobs.reaction_ticks as f64,
            0.0,
            MAX_REACTION_TICKS as f64,
        )?;
        check_bound(
            "fire_discipline_ticks",
            knobs.fire_discipline_ticks as f64,
            0.0,
            MAX_FIRE_DISCIPLINE_TICKS as f64,
        )?;
        check_bound(
            "threat_window_ticks",
            priority.threat_window_ticks as f64,
            0.0,
            MAX_THREAT_WINDOW_TICKS as f64,
        )?;
        check_bound("aim_error_rad", knobs.aim_error_rad, 0.0, MAX_AIM_ERROR_RAD)?;
        check_bound(
            "engagement_range_m",
            knobs.engagement_range_m,
            PROXIMITY_EPSILON_M,
            MAX_ENGAGEMENT_RANGE_M,
        )?;
        for (name, weight) in [
            ("protected_actor_weight", priority.protected_actor_weight),
            ("objective_weight", priority.objective_weight),
            ("self_defense_weight", priority.self_defense_weight),
            ("proximity_weight", priority.proximity_weight),
        ] {
            check_bound(name, weight, 0.0, MAX_PRIORITY_WEIGHT)?;
        }
        if [
            priority.protected_actor_weight,
            priority.objective_weight,
            priority.self_defense_weight,
            priority.proximity_weight,
        ]
        .iter()
        .all(|weight| *weight == 0.0)
        {
            return Err(CombatError::NoScoredPriorityTerm);
        }
        if role.needs_ordnance() && !arsenal.ordnance {
            return Err(CombatError::RoleArsenalMissing { role });
        }
        Ok(Self {
            role,
            arsenal,
            knobs,
            priority,
        })
    }

    /// The role this profile belongs to.
    #[must_use]
    pub const fn role(&self) -> CombatRole {
        self.role
    }

    /// What the role is allowed to shoot with.
    #[must_use]
    pub const fn arsenal(&self) -> RoleArsenal {
        self.arsenal
    }

    /// The role's knobs.
    #[must_use]
    pub const fn knobs(&self) -> SkillKnobs {
        self.knobs
    }

    /// The role's priority policy.
    #[must_use]
    pub const fn priority(&self) -> PriorityPolicy {
        self.priority
    }

    /// A copy of this profile with different knobs — the shape an ace
    /// variant or a difficulty tier takes once lowered.
    ///
    /// # Errors
    ///
    /// The validation errors of [`SkillProfile::try_new`].
    pub fn with_knobs(&self, knobs: SkillKnobs) -> Result<Self, CombatError> {
        Self::try_new(self.role, self.arsenal, knobs, self.priority)
    }

    /// A copy of this profile with a different priority policy.
    ///
    /// # Errors
    ///
    /// The validation errors of [`SkillProfile::try_new`].
    pub fn with_priority(&self, priority: PriorityPolicy) -> Result<Self, CombatError> {
        Self::try_new(self.role, self.arsenal, self.knobs, priority)
    }
}

/// Checks one knob number against its bounds.
fn check_bound(name: &'static str, value: f64, min: f64, max: f64) -> Result<(), CombatError> {
    if !value.is_finite() {
        return Err(CombatError::NonFiniteKnob { knob: name });
    }
    if value < min || value > max {
        return Err(CombatError::KnobOutOfRange {
            knob: name,
            value: value.to_string(),
        });
    }
    Ok(())
}

// ----------------------------------------------------- assignment ----

/// The role one actor holds, and the commitments that come with it.
///
/// The assignment is *data a mission sets*, not something the planner
/// infers (non-negotiable 3: script-assigned objectives are honored, and
/// the planner never invents a role of its own).
#[derive(Clone, Debug, PartialEq)]
pub struct RoleAssignment {
    actor: ActorId,
    role: CombatRole,
    protected: Option<ActorId>,
    formation: Option<FormationSlot>,
}

impl RoleAssignment {
    /// Assigns a role with no protected actor and no formation slot.
    #[must_use]
    pub const fn new(actor: ActorId, role: CombatRole) -> Self {
        Self {
            actor,
            role,
            protected: None,
            formation: None,
        }
    }

    /// Assigns a role that protects another actor — the escort case.
    ///
    /// # Errors
    ///
    /// [`CombatError::ProtectedIsSelf`] when the protected actor is the
    /// assigned actor itself: an escort that must defend itself is a
    /// content bug, not a self-defense role.
    pub fn protecting(
        actor: ActorId,
        role: CombatRole,
        protected: ActorId,
    ) -> Result<Self, CombatError> {
        if actor == protected {
            return Err(CombatError::ProtectedIsSelf { actor });
        }
        Ok(Self {
            actor,
            role,
            protected: Some(protected),
            formation: None,
        })
    }

    /// Sets the formation slot the actor holds.
    #[must_use]
    pub const fn in_formation(mut self, slot: FormationSlot) -> Self {
        self.formation = Some(slot);
        self
    }

    /// The assigned actor.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The assigned role.
    #[must_use]
    pub const fn role(&self) -> CombatRole {
        self.role
    }

    /// The actor this assignment protects, when it protects one.
    #[must_use]
    pub const fn protected(&self) -> Option<ActorId> {
        self.protected
    }

    /// The formation slot the actor holds, when it holds one.
    #[must_use]
    pub const fn formation(&self) -> Option<FormationSlot> {
        self.formation
    }
}

/// The identity of one formation: a `u32` index, the same discipline
/// `RouteNodeId` uses for route nodes — stable within a subject, never a
/// catalog asset or a filename guess.
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

/// One slot of one formation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FormationSlot {
    /// The formation the slot belongs to.
    pub formation: FormationId,
    /// The slot index within the formation.
    pub slot: u32,
}

/// What a formation does when one of its recovery triggers fires.
///
/// The lowered mirror of `cs_content::ai::RecoveryPolicy`: declared per
/// trigger by the mission, never invented per tick (non-negotiable 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecoveryAction {
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

impl RecoveryAction {
    /// Every action, in a stable order.
    pub const ALL: &'static [RecoveryAction] = &[
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

impl fmt::Display for RecoveryAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What can raise a formation's recovery path (non-negotiable 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecoveryTrigger {
    /// The formation's leader was destroyed.
    LeaderLost,
    /// The formation's assigned target was destroyed.
    AssignedTargetDestroyed,
    /// The formation's route was interrupted.
    RouteInterrupted,
    /// The actor this assignment protects was destroyed.
    ProtectedActorLost,
}

impl RecoveryTrigger {
    /// Every trigger, in a stable order.
    pub const ALL: &'static [RecoveryTrigger] = &[
        Self::LeaderLost,
        Self::AssignedTargetDestroyed,
        Self::RouteInterrupted,
        Self::ProtectedActorLost,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LeaderLost => "leader_lost",
            Self::AssignedTargetDestroyed => "assigned_target_destroyed",
            Self::RouteInterrupted => "route_interrupted",
            Self::ProtectedActorLost => "protected_actor_lost",
        }
    }
}

impl fmt::Display for RecoveryTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One declared recovery path per trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecoveryPolicySet {
    /// What followers do when the leader is destroyed.
    pub leader_loss: RecoveryAction,
    /// What the formation does when its assigned target is destroyed.
    pub assigned_target_destroyed: RecoveryAction,
    /// What the formation does when its route is interrupted.
    pub route_interrupted: RecoveryAction,
    /// What an escort does when the actor it protects is destroyed.
    pub protected_actor_lost: RecoveryAction,
}

impl RecoveryPolicySet {
    /// The action one trigger raises.
    #[must_use]
    pub const fn action(&self, trigger: RecoveryTrigger) -> RecoveryAction {
        match trigger {
            RecoveryTrigger::LeaderLost => self.leader_loss,
            RecoveryTrigger::AssignedTargetDestroyed => self.assigned_target_destroyed,
            RecoveryTrigger::RouteInterrupted => self.route_interrupted,
            RecoveryTrigger::ProtectedActorLost => self.protected_actor_lost,
        }
    }
}

/// The state a formation's producers report about one actor.
///
/// These are *facts about the world*, not decisions: the damage resolver
/// and the navigation set own them, and the planner only reads them to name
/// the pending recovery trigger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormationFacts {
    /// The formation the actor belongs to.
    pub formation: FormationId,
    /// The formation's current leader.
    pub leader: ActorId,
    /// Whether that leader is still alive.
    pub leader_alive: bool,
    /// Whether the observer *is* that leader — a destroyed leader is not its
    /// own recovery case.
    pub observer_is_leader: bool,
    /// The formation's assigned target, when it has one.
    pub assigned_target: Option<ActorId>,
    /// Whether that assigned target is still alive.
    pub assigned_target_alive: bool,
    /// Whether the formation's route is available.
    pub route_available: bool,
}

impl FormationFacts {
    /// The single recovery trigger that is currently pending, in the fixed
    /// order [`RecoveryTrigger::LeaderLost`],
    /// [`RecoveryTrigger::AssignedTargetDestroyed`],
    /// [`RecoveryTrigger::RouteInterrupted`] — so the reported trigger is a
    /// function of the facts, never of the order the producers filled them
    /// in.
    #[must_use]
    pub fn pending_trigger(&self) -> Option<RecoveryTrigger> {
        if !self.leader_alive && !self.observer_is_leader {
            return Some(RecoveryTrigger::LeaderLost);
        }
        if self.assigned_target.is_some() && !self.assigned_target_alive {
            return Some(RecoveryTrigger::AssignedTargetDestroyed);
        }
        if !self.route_available {
            return Some(RecoveryTrigger::RouteInterrupted);
        }
        None
    }
}

// ------------------------------------------------------ perception ----

/// An authoritative attack, as the planner is told about it.
///
/// `hit` is the damage system's [`HitEventId`], so a threat always traces
/// to the event that caused it and a stale-generation event can never mint
/// one (non-negotiable 4, mirroring [`crate::targeting::AttackEvent`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThreatEvidence {
    /// The actor that attacked.
    pub attacker: ActorId,
    /// The actor that was attacked.
    pub victim: ActorId,
    /// The tick the attack happened on.
    pub at: Tick,
    /// The authoritative event that evidences the attack.
    pub hit: HitEventId,
}

impl ThreatEvidence {
    /// Builds the evidence record.
    #[must_use]
    pub const fn new(attacker: ActorId, victim: ActorId, at: Tick, hit: HitEventId) -> Self {
        Self {
            attacker,
            victim,
            at,
            hit,
        }
    }

    /// How many ticks ago the attack happened, saturating at zero for an
    /// attack stamped in the future — which [`CombatPlanner::decide`]
    /// refuses rather than scoring.
    #[must_use]
    pub fn age_ticks(&self, now: Tick) -> u64 {
        now.0.saturating_sub(self.at.0)
    }
}

/// One candidate the approved perception model reported.
///
/// The planner has no other knowledge: it does not query the world, and a
/// candidate that perception did not report is not considered at all
/// (non-negotiable 3).
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateView {
    /// The candidate's session-qualified identity.
    pub actor: ActorId,
    /// The candidate's canonical world position.
    pub position: WorldPosition,
    /// The declared relation between the observer's faction and the
    /// candidate's, as [`crate::targeting`] resolved it. `None` is an
    /// *undeclared* pair: it is neither hostile nor friendly and never
    /// matches the hostile gate (F30 non-negotiable 1).
    pub allegiance: Option<Allegiance>,
    /// Whether mission rules flagged the candidate as the objective.
    pub objective: bool,
    /// The authoritative attack this candidate is known to have made, if
    /// the threat ledger holds one inside the window.
    pub threat: Option<ThreatEvidence>,
    /// How many friendlies the line-of-fire producer reported between the
    /// observer and this candidate.
    pub friendlies_in_line_of_fire: u32,
}

impl CandidateView {
    /// A candidate the perception model reported, with no threat evidence
    /// and a clear line of fire.
    ///
    /// # Errors
    ///
    /// [`cs_types::space::SpaceError`] when the position is not finite.
    pub fn new(
        actor: ActorId,
        position: WorldPosition,
        allegiance: Option<Allegiance>,
        objective: bool,
    ) -> Self {
        Self {
            actor,
            position,
            allegiance,
            objective,
            threat: None,
            friendlies_in_line_of_fire: 0,
        }
    }

    /// Attaches the candidate's authoritative attack evidence.
    #[must_use]
    pub const fn with_threat(mut self, threat: ThreatEvidence) -> Self {
        self.threat = Some(threat);
        self
    }

    /// Attaches the line-of-fire report.
    #[must_use]
    pub const fn with_friendlies_in_line_of_fire(mut self, friendlies: u32) -> Self {
        self.friendlies_in_line_of_fire = friendlies;
        self
    }
}

// --------------------------------------------------------- arsenal ----

/// What kind of weapon mount an [`ArsenalSnapshot`] entry describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MountKind {
    /// A gun mount, silenced by a destroyed weapon-mount damage node.
    Gun,
    /// An ordnance launcher, silenced by the same per-mount rule.
    Ordnance,
}

/// One mount's availability, as the weapon and ordnance systems report it.
///
/// F32-B's firing solution (AC02) reads this: a disabled mount and an empty
/// rack are two different reasons not to fire, and neither is visible to
/// the planner as anything else.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MountAvailability {
    /// The mount, in the same [`DamageNodeKey`] namespace the F29 damage
    /// graph disables.
    pub mount: DamageNodeKey,
    /// Whether the mount carries a gun or ordnance.
    pub kind: MountKind,
    /// Rounds or items remaining.
    pub rounds: u64,
    /// Whether the mount's damage node was destroyed.
    pub disabled: bool,
    /// Ticks the mount must still wait before it may fire.
    pub cooldown_ticks: u64,
}

impl MountAvailability {
    /// A mount that is neither disabled nor empty.
    #[must_use]
    pub const fn usable(mount: DamageNodeKey, kind: MountKind, rounds: u64) -> Self {
        Self {
            mount,
            kind,
            rounds,
            disabled: false,
            cooldown_ticks: 0,
        }
    }

    /// Whether the mount could produce a shot right now: not disabled and
    /// not empty. Cooldown is a *cadence* question the firing solution
    /// answers, not an availability one.
    #[must_use]
    pub const fn is_usable(&self) -> bool {
        !self.disabled && self.rounds > 0
    }
}

/// The read-only snapshot of one actor's weapons and launchers.
#[derive(Clone, Debug, PartialEq)]
pub struct ArsenalSnapshot {
    mounts: Vec<MountAvailability>,
}

impl ArsenalSnapshot {
    /// Builds a snapshot, refusing two mounts that share one key.
    ///
    /// # Errors
    ///
    /// [`CombatError::DuplicateMount`] naming the repeated mount.
    pub fn try_new(mounts: Vec<MountAvailability>) -> Result<Self, CombatError> {
        let mut seen: Vec<&DamageNodeKey> = Vec::with_capacity(mounts.len());
        for mount in &mounts {
            if seen.contains(&&mount.mount) {
                return Err(CombatError::DuplicateMount {
                    mount: mount.mount.clone(),
                });
            }
            seen.push(&mount.mount);
        }
        Ok(Self { mounts })
    }

    /// An empty snapshot: an actor carrying nothing usable.
    #[must_use]
    pub const fn empty() -> Self {
        Self { mounts: Vec::new() }
    }

    /// The mounts, in the order they were declared.
    #[must_use]
    pub fn mounts(&self) -> &[MountAvailability] {
        &self.mounts
    }

    /// The report the decision trace carries.
    #[must_use]
    pub fn report(&self) -> ArsenalReport {
        let mut report = ArsenalReport {
            total_mounts: self.mounts.len() as u32,
            ..ArsenalReport::default()
        };
        for mount in &self.mounts {
            if mount.disabled {
                report.disabled_mounts += 1;
                continue;
            }
            if mount.rounds == 0 {
                report.empty_mounts += 1;
                continue;
            }
            match mount.kind {
                MountKind::Gun => report.usable_guns += 1,
                MountKind::Ordnance => report.ready_ordnance += 1,
            }
        }
        report
    }
}

/// The arsenal counts a decision trace reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArsenalReport {
    /// How many mounts the actor carries.
    pub total_mounts: u32,
    /// Gun mounts that are neither disabled nor empty.
    pub usable_guns: u32,
    /// Ordnance mounts that are neither disabled nor empty.
    pub ready_ordnance: u32,
    /// Mounts whose damage node was destroyed.
    pub disabled_mounts: u32,
    /// Mounts that are intact but empty.
    pub empty_mounts: u32,
}

// ------------------------------------------------------------ terms ----

/// One scored term of a decision.
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
}

impl fmt::Display for PriorityTerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One term's contribution to a candidate's score.
///
/// `value` is the normalized `[0, 1]` factor the term measured — 1.0 for a
/// satisfied discrete term, the proximity ratio for
/// [`PriorityTerm::Proximity`] — and `contribution` is
/// `weight * value`. Keeping both means a trace shows *why* a candidate
/// scored what it scored, not only that it did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TermScore {
    /// The term.
    pub term: PriorityTerm,
    /// The profile's weight for the term.
    pub weight: f64,
    /// The normalized factor the term measured.
    pub value: f64,
    /// `weight * value`.
    pub contribution: f64,
}

/// Why a candidate was not selectable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RejectReason {
    /// The candidate is the observer.
    ObserverItself,
    /// The candidate's faction relation was never declared, so it is
    /// neither hostile nor friendly — the absence is the unknown, and it
    /// never matches the hostile gate.
    UndeclaredAllegiance,
    /// The candidate is declared friendly or neutral.
    NotHostile {
        /// The declared relation that was found instead.
        allegiance: Allegiance,
    },
    /// The candidate is farther away than the profile's engagement range.
    BeyondEngagementRange,
}

/// A candidate's gate verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CandidateVerdict {
    /// The candidate was scored.
    Eligible,
    /// The candidate was refused, by this reason.
    Rejected(RejectReason),
}

/// Whether the planner has noticed a threat yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReactionState {
    /// The candidate carries no authoritative attack, so there is nothing
    /// to react to.
    NotApplicable,
    /// The attack is older than the profile's reaction delay.
    Noticed {
        /// How many ticks ago the attack happened.
        age_ticks: u64,
    },
    /// The attack is younger than the profile's reaction delay: the planner
    /// has not noticed it yet and scores the term as zero.
    Deferred {
        /// How many ticks ago the attack happened.
        age_ticks: u64,
        /// The delay it is waiting out.
        required_ticks: u64,
    },
}

/// A reason the selected target may not be shot at *yet*.
///
/// Deliberately separate from [`RejectReason`]: non-negotiable 3 keeps
/// friendly-fire avoidance and line-of-fire checks apart from target
/// hostility, so a hostile with a friendly in the line of fire is still the
/// target and the veto is reported on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FireVeto {
    /// The line-of-fire producer reported friendlies between the observer
    /// and the target.
    FriendlyInLineOfFire {
        /// How many friendlies were reported.
        friendlies: u32,
    },
    /// The observer has no usable gun and no ready launcher.
    ArsenalUnusable {
        /// Usable gun mounts.
        usable_guns: u32,
        /// Ready ordnance mounts.
        ready_ordnance: u32,
    },
}

/// Why no target was selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HoldReason {
    /// No candidate passed the gate.
    NoEligibleCandidate,
}

/// The recovery a decision is reporting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecoveryOutcome {
    /// The trigger that is pending.
    pub trigger: RecoveryTrigger,
    /// The action the formation declared for that trigger.
    pub action: RecoveryAction,
}

/// Everything one candidate contributed to a decision.
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateTrace {
    /// The candidate's identity.
    pub actor: ActorId,
    /// Its distance from the observer, in meters.
    pub distance_m: f64,
    /// The declared relation the gate found.
    pub allegiance: Option<Allegiance>,
    /// Whether mission rules flagged it as the objective.
    pub objective: bool,
    /// Whether the gate selected it.
    pub verdict: CandidateVerdict,
    /// Whether the planner has noticed its attack evidence yet.
    pub reaction: ReactionState,
    /// The scored terms, in [`PriorityTerm::ALL`] order.
    pub terms: Vec<TermScore>,
    /// The total score.
    pub total: f64,
    /// The fire veto, reported independently of the gate.
    pub fire_veto: Option<FireVeto>,
}

impl CandidateTrace {
    /// The contribution of one term, if the term was scored.
    #[must_use]
    pub fn term(&self, term: PriorityTerm) -> Option<TermScore> {
        self.terms.iter().copied().find(|score| score.term == term)
    }
}

/// The record a decision carries: why it chose what it chose.
#[derive(Clone, Debug, PartialEq)]
pub struct DecisionTrace {
    /// The deciding actor.
    pub observer: ActorId,
    /// The tick the decision was made on.
    pub tick: Tick,
    /// The role the observer held.
    pub role: CombatRole,
    /// The actor the observer protected, when it protected one.
    pub protected_actor: Option<ActorId>,
    /// The arsenal the decision was made with, when the caller supplied
    /// one.
    pub arsenal: Option<ArsenalReport>,
    /// The pending recovery, when a trigger is pending.
    pub recovery: Option<RecoveryOutcome>,
    /// Every candidate the request carried: the eligible ones in selection
    /// order first, then the rejected ones in actor-id order. The order is
    /// a function of the candidates, never of the request's order.
    pub candidates: Vec<CandidateTrace>,
    /// The selected target.
    pub chosen: Option<ActorId>,
    /// Why no target was selected.
    pub hold: Option<HoldReason>,
}

impl DecisionTrace {
    /// The trace of one candidate, by identity.
    #[must_use]
    pub fn candidate(&self, actor: ActorId) -> Option<&CandidateTrace> {
        self.candidates.iter().find(|trace| trace.actor == actor)
    }

    /// The eligible candidates in selection order.
    pub fn eligible(&self) -> impl Iterator<Item = &CandidateTrace> {
        self.candidates
            .iter()
            .filter(|trace| trace.verdict == CandidateVerdict::Eligible)
    }
}

// -------------------------------------------------------- decision ----

/// One typed request: everything the planner is allowed to know.
#[derive(Debug)]
pub struct CombatRequest<'a> {
    /// The deciding actor.
    pub observer: ActorId,
    /// The tick the decision is made on.
    pub now: Tick,
    /// The observer's canonical world position.
    pub observer_position: WorldPosition,
    /// The observer's role assignment.
    pub assignment: &'a RoleAssignment,
    /// The formation facts, when the observer belongs to a formation.
    pub formation: Option<&'a FormationFacts>,
    /// Whether the observer's protected actor is still alive. `None` means
    /// the caller did not report it, which is not the same statement as
    /// "destroyed".
    pub protected_alive: Option<bool>,
    /// The candidates the approved perception model reported.
    pub candidates: &'a [CandidateView],
    /// The profile this actor actually runs under, when it runs a declared
    /// variant — an ace or a difficulty-overridden copy of its role.
    ///
    /// `None` means the actor runs the planner's profile for its assigned
    /// role. This is the typed seam F32-C fills from the declared ace and
    /// difficulty records: the variant is *data*, not a different planner.
    pub profile: Option<&'a SkillProfile>,
    /// The observer's weapons and launchers, when the caller has a
    /// snapshot. F32-B's firing solution reads the same snapshot.
    pub arsenal: Option<&'a ArsenalSnapshot>,
}

/// One decision: the target, and the trace that explains it.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatDecision {
    /// The role the observer held.
    pub role: CombatRole,
    /// The tick the decision was made on.
    pub tick: Tick,
    /// The selected target, or `None` when nothing was eligible.
    pub target: Option<ActorId>,
    /// Why the decision came out this way.
    pub trace: DecisionTrace,
}

/// The per-session combat-AI authority.
///
/// One planner per session generation, mirroring
/// [`crate::damage::DamageResolver`] and [`crate::targeting::TargetStore`]:
/// every actor identity it is asked about must belong to its own session,
/// and a foreign generation is refused by name
/// (`docs/contracts/STATE-TRANSACTIONS.md`).
///
/// It owns no per-actor mutable state: [`CombatPlanner::decide`] is a pure
/// function of the immutable policy below and the typed request, so the
/// stateful formation recovery, maneuver selection and firing solution
/// (F32-B/F32-C) sit above it and cannot leak into a decision.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatPlanner {
    session: u64,
    profiles: BTreeMap<CombatRole, SkillProfile>,
    formations: BTreeMap<FormationId, RecoveryPolicySet>,
}

impl CombatPlanner {
    /// Builds a planner with one profile per role.
    ///
    /// # Errors
    ///
    /// [`CombatError::DuplicateRoleProfile`] when two profiles carry the
    /// same role — the ambiguity would otherwise resolve by iteration
    /// order.
    pub fn new(session: u64, profiles: &[SkillProfile]) -> Result<Self, CombatError> {
        let mut map = BTreeMap::new();
        for profile in profiles {
            if map.insert(profile.role(), *profile).is_some() {
                return Err(CombatError::DuplicateRoleProfile {
                    role: profile.role(),
                });
            }
        }
        Ok(Self {
            session,
            profiles: map,
            formations: BTreeMap::new(),
        })
    }

    /// Registers one formation's declared recovery paths.
    ///
    /// # Errors
    ///
    /// [`CombatError::DuplicateFormation`] when the formation is already
    /// registered.
    pub fn with_formation(
        mut self,
        formation: FormationId,
        policies: RecoveryPolicySet,
    ) -> Result<Self, CombatError> {
        if self.formations.insert(formation, policies).is_some() {
            return Err(CombatError::DuplicateFormation { formation });
        }
        Ok(self)
    }

    /// The session generation this planner belongs to.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The profile of one role, if the planner carries one.
    #[must_use]
    pub fn profile(&self, role: CombatRole) -> Option<&SkillProfile> {
        self.profiles.get(&role)
    }

    /// The registered formation ids, in ascending order.
    pub fn formations(&self) -> impl Iterator<Item = FormationId> + '_ {
        self.formations.keys().copied()
    }

    /// The recovery paths of one formation, if it is registered.
    #[must_use]
    pub fn recovery_policies(&self, formation: FormationId) -> Option<RecoveryPolicySet> {
        self.formations.get(&formation).copied()
    }

    /// Decides one target for one tick.
    ///
    /// This is target *selection* and nothing more: the maneuver the role
    /// flies, whether a shot may be taken and what happens after the
    /// formation's recovery action are F32-B's and F32-C's, and the trace
    /// is the record they consume. An `Evade` or `Retreat` assignment is
    /// still given the target its policy ranks; what to do about it is the
    /// maneuver stage's decision, not an omission here.
    ///
    /// # Errors
    ///
    /// [`CombatError::ForeignSession`] when the observer, a candidate, a
    /// threat event, the protected actor or a formation member identity
    /// belongs to another session generation,
    /// [`CombatError::AssignmentObserverMismatch`] when the request's
    /// assignment belongs to another actor,
    /// [`CombatError::ProfileRoleMismatch`] when a supplied variant profile
    /// belongs to another role,
    /// [`CombatError::NoProfileForRole`] when the planner carries no profile
    /// for the assigned role,
    /// [`CombatError::ThreatAttackerMismatch`] when a candidate's evidence
    /// names a different attacker,
    /// [`CombatError::ThreatFromTheFuture`] when a candidate's evidence is
    /// stamped on a later tick,
    /// [`CombatError::FormationAssignmentMismatch`] when the assignment and
    /// the formation facts disagree about the observer's formation, and
    /// [`CombatError::NoRecoveryPolicy`] when a recovery trigger is pending
    /// for a formation the planner does not carry. Each refuses rather than
    /// deciding on inconsistent input.
    pub fn decide(&self, request: &CombatRequest<'_>) -> Result<CombatDecision, CombatError> {
        if request.observer.session != self.session {
            return Err(CombatError::ForeignSession {
                actor: request.observer,
                session: self.session,
            });
        }
        if request.assignment.actor() != request.observer {
            return Err(CombatError::AssignmentObserverMismatch {
                observer: request.observer,
                assignment: request.assignment.actor(),
            });
        }
        let profile = match request.profile {
            Some(variant) => {
                if variant.role() != request.assignment.role() {
                    return Err(CombatError::ProfileRoleMismatch {
                        assigned: request.assignment.role(),
                        profile: variant.role(),
                    });
                }
                *variant
            }
            None => *self.profiles.get(&request.assignment.role()).ok_or(
                CombatError::NoProfileForRole {
                    role: request.assignment.role(),
                },
            )?,
        };
        // Every identity the request carries must belong to this session
        // generation, whether or not the caller reported anything else
        // about it: a foreign protected actor is refused on the identity
        // alone, so a request that omits the lifecycle report cannot slip
        // a stale generation past the check.
        let protected = request.assignment.protected();
        if let Some(actor) = protected
            && actor.session != self.session
        {
            return Err(CombatError::ForeignSession {
                actor,
                session: self.session,
            });
        }
        // The formation the assignment places the observer in and the
        // formation the facts describe are one statement about the
        // observer; a request that makes two is refused instead of
        // resolving a recovery path from the wrong formation.
        let assigned_formation = request.assignment.formation().map(|slot| slot.formation);
        if let Some(facts) = request.formation {
            if assigned_formation != Some(facts.formation) {
                return Err(CombatError::FormationAssignmentMismatch {
                    assigned: assigned_formation,
                    facts: facts.formation,
                });
            }
            for actor in [Some(facts.leader), facts.assigned_target].into_iter().flatten() {
                if actor.session != self.session {
                    return Err(CombatError::ForeignSession {
                        actor,
                        session: self.session,
                    });
                }
            }
        }
        let protected_alive = request.protected_alive;
        let arsenal_report = request.arsenal.map(ArsenalSnapshot::report);

        let mut traces = Vec::with_capacity(request.candidates.len());
        for candidate in request.candidates {
            traces.push(self.score_candidate(
                request,
                &profile,
                protected,
                protected_alive,
                arsenal_report,
                candidate,
            )?);
        }

        traces.sort_by(compare_traces);
        let chosen = traces
            .iter()
            .find(|trace| trace.verdict == CandidateVerdict::Eligible)
            .map(|trace| trace.actor);
        let hold = if chosen.is_none() {
            Some(HoldReason::NoEligibleCandidate)
        } else {
            None
        };

        // The protected-actor trigger comes from the assignment plus the
        // caller's lifecycle report, not from the formation facts: an
        // escort whose charge is destroyed reports the declared recovery
        // even when its formation is intact.
        let protected_lost = match (protected, protected_alive) {
            (Some(actor), Some(false)) => Some(actor),
            _ => None,
        };
        let recovery = match request.formation {
            Some(facts) => {
                let trigger = facts
                    .pending_trigger()
                    .or_else(|| protected_lost.map(|_| RecoveryTrigger::ProtectedActorLost));
                match trigger {
                    Some(trigger) => {
                        let policies = self.formations.get(&facts.formation).ok_or(
                            CombatError::NoRecoveryPolicy {
                                formation: facts.formation,
                            },
                        )?;
                        Some(RecoveryOutcome {
                            trigger,
                            action: policies.action(trigger),
                        })
                    }
                    None => None,
                }
            }
            // Without a formation there is no declared recovery path to
            // report, and inventing one is exactly what non-negotiable 4
            // forbids.
            None => None,
        };

        let trace = DecisionTrace {
            observer: request.observer,
            tick: request.now,
            role: profile.role(),
            protected_actor: protected,
            arsenal: arsenal_report,
            recovery,
            candidates: traces,
            chosen,
            hold,
        };
        Ok(CombatDecision {
            role: profile.role(),
            tick: request.now,
            target: chosen,
            trace,
        })
    }

    /// Scores one candidate: gate, reaction, terms, total and fire veto.
    #[allow(clippy::too_many_arguments)]
    fn score_candidate(
        &self,
        request: &CombatRequest<'_>,
        profile: &SkillProfile,
        protected: Option<ActorId>,
        protected_alive: Option<bool>,
        arsenal: Option<ArsenalReport>,
        candidate: &CandidateView,
    ) -> Result<CandidateTrace, CombatError> {
        if candidate.actor.session != self.session {
            return Err(CombatError::ForeignSession {
                actor: candidate.actor,
                session: self.session,
            });
        }
        if let Some(threat) = &candidate.threat {
            if threat.hit.session != self.session {
                return Err(CombatError::ForeignSession {
                    actor: threat.attacker,
                    session: self.session,
                });
            }
            if threat.attacker != candidate.actor {
                return Err(CombatError::ThreatAttackerMismatch {
                    candidate: candidate.actor,
                    attacker: threat.attacker,
                });
            }
            if threat.at > request.now {
                return Err(CombatError::ThreatFromTheFuture {
                    attacker: threat.attacker,
                    at: threat.at,
                    now: request.now,
                });
            }
        }

        let distance_m = distance(request.observer_position, candidate.position);
        let mut trace = CandidateTrace {
            actor: candidate.actor,
            distance_m,
            allegiance: candidate.allegiance,
            objective: candidate.objective,
            verdict: CandidateVerdict::Eligible,
            reaction: ReactionState::NotApplicable,
            terms: Vec::with_capacity(PriorityTerm::ALL.len()),
            total: 0.0,
            fire_veto: None,
        };

        // The gate: the observer, an undeclared relation, a declared
        // non-hostile relation, or a candidate outside the engagement
        // range. Hostility is a *relation* question, never a weight.
        trace.verdict = if candidate.actor == request.observer {
            CandidateVerdict::Rejected(RejectReason::ObserverItself)
        } else {
            match candidate.allegiance {
                None => CandidateVerdict::Rejected(RejectReason::UndeclaredAllegiance),
                Some(allegiance) if allegiance != Allegiance::Hostile => {
                    CandidateVerdict::Rejected(RejectReason::NotHostile { allegiance })
                }
                Some(_) if distance_m > profile.knobs.engagement_range_m => {
                    CandidateVerdict::Rejected(RejectReason::BeyondEngagementRange)
                }
                Some(_) => CandidateVerdict::Eligible,
            }
        };

        // The reaction gate: a threat younger than the profile's delay has
        // not been noticed, so its terms score zero — visibly, in the
        // trace, rather than by dropping the candidate.
        let fresh = candidate
            .threat
            .filter(|threat| threat.age_ticks(request.now) <= profile.priority.threat_window_ticks);
        if let Some(threat) = &candidate.threat {
            let age = threat.age_ticks(request.now);
            trace.reaction = if age < profile.knobs.reaction_ticks {
                ReactionState::Deferred {
                    age_ticks: age,
                    required_ticks: profile.knobs.reaction_ticks,
                }
            } else {
                ReactionState::Noticed { age_ticks: age }
            };
        }
        let noticed = matches!(trace.reaction, ReactionState::Noticed { .. });

        let protects_live = protected.is_some() && protected_alive.unwrap_or(true);
        for term in PriorityTerm::ALL {
            let value = match term {
                PriorityTerm::ProtectedActorThreat => match (fresh, protects_live, noticed) {
                    (Some(threat), true, true) if Some(threat.victim) == protected => 1.0,
                    _ => 0.0,
                },
                PriorityTerm::SelfDefense => match (fresh, noticed) {
                    (Some(threat), true) if threat.victim == request.observer => 1.0,
                    _ => 0.0,
                },
                PriorityTerm::ScriptObjective => f64::from(u8::from(candidate.objective)),
                PriorityTerm::Proximity => {
                    // A refused candidate has no engagement to be close to,
                    // so its proximity factor is zero rather than a number
                    // that would only matter if the gate had passed. The
                    // other three terms are still reported for a refused
                    // candidate: the trace shows what a producer's mistake
                    // would have been worth.
                    if trace.verdict != CandidateVerdict::Eligible {
                        0.0
                    } else {
                        proximity_value(distance_m, profile.knobs.engagement_range_m)
                    }
                }
            };
            let weight = profile.priority.weight(*term);
            trace.terms.push(TermScore {
                term: *term,
                weight,
                value,
                contribution: weight * value,
            });
        }
        trace.total = trace.terms.iter().map(|score| score.contribution).sum();

        // The fire veto is reported independently of the gate: non-negotiable
        // 3 keeps friendly-fire avoidance and line of fire apart from
        // target hostility.
        trace.fire_veto = if candidate.friendlies_in_line_of_fire > 0 {
            Some(FireVeto::FriendlyInLineOfFire {
                friendlies: candidate.friendlies_in_line_of_fire,
            })
        } else {
            arsenal
                .filter(|report| report.usable_guns == 0 && report.ready_ordnance == 0)
                .map(|report| FireVeto::ArsenalUnusable {
                    usable_guns: report.usable_guns,
                    ready_ordnance: report.ready_ordnance,
                })
        };

        Ok(trace)
    }
}

/// The total order a decision's candidates are emitted in: eligible first
/// by `(score desc, distance asc, ActorId asc)`, rejected after them by
/// `ActorId`. A candidate's input order cannot change the result.
fn compare_traces(left: &CandidateTrace, right: &CandidateTrace) -> std::cmp::Ordering {
    match (
        left.verdict == CandidateVerdict::Eligible,
        right.verdict == CandidateVerdict::Eligible,
    ) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (true, true) => right
            .total
            .total_cmp(&left.total)
            .then_with(|| left.distance_m.total_cmp(&right.distance_m))
            .then_with(|| left.actor.cmp(&right.actor)),
        (false, false) => left.actor.cmp(&right.actor),
    }
}

/// The normalized proximity factor: 1 at the observer, 0 at the engagement
/// range, never negative and never NaN for a coincident position.
fn proximity_value(distance_m: f64, engagement_range_m: f64) -> f64 {
    if distance_m <= PROXIMITY_EPSILON_M {
        return 1.0;
    }
    (1.0 - distance_m / engagement_range_m).clamp(0.0, 1.0)
}

/// The canonical distance between two world positions, in meters.
fn distance(from: WorldPosition, to: WorldPosition) -> f64 {
    let dx = to.x() - from.x();
    let dy = to.y() - from.y();
    let dz = to.z() - from.z();
    (dx * dx + dy * dy + dz * dz).sqrt()
}

// --------------------------------------------------------- errors ----

/// Why a combat-AI request or profile was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum CombatError {
    /// An actor identity belongs to another session generation.
    ForeignSession {
        /// The offending actor.
        actor: ActorId,
        /// The session this planner belongs to.
        session: u64,
    },
    /// The request's role assignment belongs to a different actor than the
    /// request's observer.
    AssignmentObserverMismatch {
        /// The request's observer.
        observer: ActorId,
        /// The assignment's actor.
        assignment: ActorId,
    },
    /// Two profiles carry the same role.
    DuplicateRoleProfile {
        /// The repeated role.
        role: CombatRole,
    },
    /// The planner carries no profile for the assigned role.
    NoProfileForRole {
        /// The assigned role.
        role: CombatRole,
    },
    /// A variant profile was supplied for a role the assignment does not
    /// hold: an ace of the fighter role cannot defend a charge as an
    /// escort, and the mismatch is refused rather than re-interpreted.
    ProfileRoleMismatch {
        /// The role the assignment holds.
        assigned: CombatRole,
        /// The role the supplied profile belongs to.
        profile: CombatRole,
    },
    /// The same formation is registered twice.
    DuplicateFormation {
        /// The repeated formation.
        formation: FormationId,
    },
    /// A recovery trigger is pending for a formation the planner carries no
    /// recovery paths for: the decision refuses rather than silently
    /// dropping a declared recovery case.
    NoRecoveryPolicy {
        /// The formation.
        formation: FormationId,
    },
    /// The role assignment and the formation facts disagree about which
    /// formation the observer is in. The recovery path would otherwise be
    /// resolved from whichever of the two the caller happened to fill in.
    FormationAssignmentMismatch {
        /// The formation the assignment places the observer in, when it
        /// places it in one at all.
        assigned: Option<FormationId>,
        /// The formation the supplied facts describe.
        facts: FormationId,
    },
    /// Two arsenal entries share one mount key.
    DuplicateMount {
        /// The repeated mount.
        mount: DamageNodeKey,
    },
    /// A candidate's threat evidence names a different attacker.
    ThreatAttackerMismatch {
        /// The candidate the evidence was attached to.
        candidate: ActorId,
        /// The attacker the evidence names.
        attacker: ActorId,
    },
    /// A candidate's threat evidence is stamped on a later tick than the
    /// decision.
    ThreatFromTheFuture {
        /// The attacker the evidence names.
        attacker: ActorId,
        /// The evidence's tick.
        at: Tick,
        /// The decision's tick.
        now: Tick,
    },
    /// An assignment declares the assigned actor as its own protected
    /// actor.
    ProtectedIsSelf {
        /// The offending actor.
        actor: ActorId,
    },
    /// A role was declared with an arsenal that cannot carry it.
    RoleArsenalMissing {
        /// The role.
        role: CombatRole,
    },
    /// A knob value was NaN or infinite.
    NonFiniteKnob {
        /// The knob's name.
        knob: &'static str,
    },
    /// A knob value fell outside its approved range.
    KnobOutOfRange {
        /// The knob's name.
        knob: &'static str,
        /// The rejected value.
        value: String,
    },
    /// Every priority weight is zero, so the policy would score no
    /// candidate and the decision would be an artifact of the tie-break.
    NoScoredPriorityTerm,
}

impl fmt::Display for CombatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { actor, session } => {
                write!(f, "{actor} does not belong to session {session}")
            }
            Self::AssignmentObserverMismatch {
                observer,
                assignment,
            } => write!(
                f,
                "the role assignment belongs to {assignment}, not to the observer {observer}"
            ),
            Self::DuplicateRoleProfile { role } => {
                write!(f, "role {role} has more than one profile")
            }
            Self::NoProfileForRole { role } => {
                write!(f, "no skill profile is registered for role {role}")
            }
            Self::ProfileRoleMismatch { assigned, profile } => write!(
                f,
                "the assignment holds role {assigned} but the supplied profile is a {profile} profile"
            ),
            Self::DuplicateFormation { formation } => {
                write!(f, "{formation} is registered more than once")
            }
            Self::NoRecoveryPolicy { formation } => write!(
                f,
                "{formation} has a pending recovery trigger but no declared recovery paths"
            ),
            Self::FormationAssignmentMismatch { assigned, facts } => {
                let assigned = match assigned {
                    Some(formation) => format!("formation {formation}"),
                    None => "no formation at all".to_owned(),
                };
                write!(
                    f,
                    "the formation facts describe {} but the assignment declares {assigned}",
                    facts.0
                )
            }
            Self::DuplicateMount { mount } => {
                write!(f, "the arsenal lists mount {mount} more than once")
            }
            Self::ThreatAttackerMismatch {
                candidate,
                attacker,
            } => write!(
                f,
                "the threat evidence on {candidate} names {attacker} as the attacker"
            ),
            Self::ThreatFromTheFuture { attacker, at, now } => write!(
                f,
                "{attacker}'s attack is stamped on tick {} but the decision is tick {}",
                at.0, now.0
            ),
            Self::ProtectedIsSelf { actor } => {
                write!(f, "{actor} cannot be its own protected actor")
            }
            Self::RoleArsenalMissing { role } => {
                write!(f, "role {role} is declared without the ordnance it needs")
            }
            Self::NonFiniteKnob { knob } => write!(f, "knob {knob} must be a finite number"),
            Self::KnobOutOfRange { knob, value } => {
                write!(f, "knob {knob} value {value} is outside its approved range")
            }
            Self::NoScoredPriorityTerm => write!(
                f,
                "every priority weight is zero, so the policy would score no candidate"
            ),
        }
    }
}

impl std::error::Error for CombatError {}

// -------------------------------------------------------- fixture ----

/// The claim id the synthetic fixture's values carry.
pub const SYNTHETIC_COMBAT_CLAIM: &str = "f32a.synthetic-escort-sortie";

/// The synthetic fixture's claim id.
#[must_use]
pub fn synthetic_combat_claim() -> ClaimId {
    ClaimId::new(SYNTHETIC_COMBAT_CLAIM).expect("the synthetic claim id is valid")
}

/// The fixture's session generation.
pub const SYNTHETIC_SESSION: u64 = 7;

/// An actor of the synthetic session.
#[must_use]
pub const fn synthetic_actor(serial: u64) -> ActorId {
    ActorId {
        session: SYNTHETIC_SESSION,
        serial,
    }
}

/// The synthetic escort profile — the *approved policy* of the AC01
/// scenario: guns, a 24-tick reaction, a 0.06 rad aim error, a 1.5 km
/// engagement range, and a priority policy in which the protected actor's
/// threat (2.0) outweighs proximity (0.5).
///
/// The numbers mirror `cs_content::ai::declared_synthetic_escort_knobs` and
/// `declared_synthetic_escort_policy`; the declared→runtime lowering
/// boundary is `cs_app::ai::combat` (F32-B/C), which is where the two
/// records are joined. Designed fixture values, not original data.
#[must_use]
pub fn synthetic_escort_profile() -> SkillProfile {
    SkillProfile::try_new(
        CombatRole::Escort,
        RoleArsenal::guns(),
        SkillKnobs {
            reaction_ticks: 24,
            aim_error_rad: 0.06,
            engagement_range_m: 1_500.0,
            fire_discipline_ticks: 30,
        },
        PriorityPolicy {
            protected_actor_weight: 2.0,
            objective_weight: 1.0,
            self_defense_weight: 1.5,
            proximity_weight: 0.5,
            threat_window_ticks: 120,
        },
    )
    .expect("the synthetic escort profile is valid")
}

/// The synthetic ace variant of the escort profile: a 6-tick reaction, a
/// 0.015 rad aim error and a doubled protected-actor weight, applied on top
/// of [`synthetic_escort_profile`].
///
/// It mirrors `cs_content::ai::declared_synthetic_ace_profile`'s three
/// behavior overrides. There is no damage, armor or health field anywhere
/// in this module, so an ace cannot be inflated health (F32 "Deliverable and
/// interfaces").
#[must_use]
pub fn synthetic_ace_profile() -> SkillProfile {
    synthetic_escort_profile()
        .with_knobs(SkillKnobs {
            reaction_ticks: 6,
            aim_error_rad: 0.015,
            ..synthetic_escort_profile().knobs()
        })
        .and_then(|profile| {
            let priority = profile.priority();
            profile.with_priority(PriorityPolicy {
                protected_actor_weight: 4.0,
                ..priority
            })
        })
        .expect("the synthetic ace profile is valid")
}

/// The synthetic rookie escort profile: the same role with **no**
/// protected-actor weight, so its only threat answer is self-defense.
///
/// The negative case of the AC01 scenario — it shows the
/// protected-actor term, not the escort role name, is what makes the
/// escort defend its charge.
#[must_use]
pub fn synthetic_rookie_escort_profile() -> SkillProfile {
    let priority = synthetic_escort_profile().priority();
    synthetic_escort_profile()
        .with_priority(PriorityPolicy {
            protected_actor_weight: 0.0,
            ..priority
        })
        .expect("the synthetic rookie escort profile is valid")
}

/// The synthetic fighter-attack profile: guns, an 18-tick reaction, a
/// 400 m range and a policy that prefers the script-assigned objective
/// (4.0) over proximity (1.0).
#[must_use]
pub fn synthetic_fighter_profile() -> SkillProfile {
    SkillProfile::try_new(
        CombatRole::FighterAttack,
        RoleArsenal::guns(),
        SkillKnobs {
            reaction_ticks: 18,
            aim_error_rad: 0.04,
            engagement_range_m: 400.0,
            fire_discipline_ticks: 24,
        },
        PriorityPolicy {
            protected_actor_weight: 0.0,
            objective_weight: 4.0,
            self_defense_weight: 1.0,
            proximity_weight: 1.0,
            threat_window_ticks: 90,
        },
    )
    .expect("the synthetic fighter profile is valid")
}

/// The synthetic formation's declared recovery paths: reassign the lead when
/// the leader is lost, regroup when the assigned target dies, resume the
/// route when it is interrupted and regroup when the protected actor is
/// lost.
#[must_use]
pub const fn synthetic_recovery_policies() -> RecoveryPolicySet {
    RecoveryPolicySet {
        leader_loss: RecoveryAction::ReassignLead,
        assigned_target_destroyed: RecoveryAction::Regroup,
        route_interrupted: RecoveryAction::ResumeRoute,
        protected_actor_lost: RecoveryAction::Regroup,
    }
}

/// The synthetic planner: the synthetic session with the escort, fighter
/// and ace profiles, and formation 1's recovery paths.
#[must_use]
pub fn synthetic_combat_planner() -> CombatPlanner {
    CombatPlanner::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile(), synthetic_fighter_profile()],
    )
    .and_then(|planner| planner.with_formation(FormationId(1), synthetic_recovery_policies()))
    .expect("the synthetic combat planner is valid")
}

/// A candidate at a canonical world position, with a declared relation.
#[must_use]
pub fn synthetic_candidate(
    serial: u64,
    position_m: [f64; 3],
    allegiance: Option<Allegiance>,
    objective: bool,
) -> CandidateView {
    CandidateView::new(
        synthetic_actor(serial),
        WorldPosition::try_new(position_m).expect("the fixture position is finite"),
        allegiance,
        objective,
    )
}

/// The authoritative attack evidence of one attacker against one victim.
#[must_use]
pub const fn synthetic_threat(
    attacker: u64,
    victim: u64,
    at: Tick,
    producer: u32,
    sequence: u32,
) -> ThreatEvidence {
    ThreatEvidence::new(
        synthetic_actor(attacker),
        synthetic_actor(victim),
        at,
        HitEventId {
            session: SYNTHETIC_SESSION,
            tick: at,
            producer,
            sequence,
        },
    )
}

/// The synthetic arsenal: two usable guns and one ready launcher.
#[must_use]
pub fn synthetic_arsenal() -> ArsenalSnapshot {
    ArsenalSnapshot::try_new(vec![
        MountAvailability::usable(synthetic_mount("gun_mount_1"), MountKind::Gun, 250),
        MountAvailability::usable(synthetic_mount("gun_mount_2"), MountKind::Gun, 180),
        MountAvailability::usable(synthetic_mount("ordnance_mount_1"), MountKind::Ordnance, 6),
    ])
    .expect("the synthetic arsenal is valid")
}

/// A damage-node key for a synthetic mount name.
fn synthetic_mount(key: &str) -> DamageNodeKey {
    DamageNodeKey::new(key).expect("the fixture mount key is valid")
}
