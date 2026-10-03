//! Combat AI: roles, skill profiles, target priority, maneuvers, firing
//! solutions and the per-session runtime that composes them (F32-A, F32-B and
//! F32-C).
//!
//! Spec: `specs/F32-ai-combat-formations-aces-and-difficulty.md`, stages
//! `### F32-A`, `### F32-B` and `### F32-C`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage **F32-A** defines the typed contract and the minimal synthetic
//! fixture; stage **F32-B** lowers that contract into the maneuver the role
//! flies and the per-mount firing solution an ace fires; stage **F32-C**
//! wires the pieces into the session runtime: the ace variants and difficulty
//! profiles that select an actor's profile, and the formation coordinator
//! that applies the declared recovery when a leader is lost. None of the three
//! is the whole combat runtime. The consumer half of the combat-AI contract
//! lives here; the provenance-carrying producer record an importer will emit
//! is `cs_content::ai`. The conversion boundary between them is
//! `cs_app::ai::combat`, which is **not** in this task's owner paths and is
//! filed separately as #551 `F32-LOWERING`.
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
//! * [`CombatManeuver`] (F32-B): the intent the role flies this tick,
//!   selected from the role and whether a target was chosen. It names no
//!   waypoint, heading or control law, so it cannot become a second pose
//!   owner (`docs/contracts/FLIGHT-PHYSICS.md`).
//! * [`FiringSolution`] (F32-B): the per-mount verdict for the selected
//!   target. A disabled mount (F29) and an empty rack (F27/F28) are two
//!   distinct refusals and neither is a question of skill: an ace fires off
//!   the same [`ArsenalSnapshot`] the player does (AC02).
//! * [`CombatRuntime`] (F32-C): the per-session authority that owns the
//!   planner, the [`AceVariant`] registry, the [`DifficultyRoster`] and the
//!   [`FormationCoordinator`], and turns one mission tick into
//!   [`CombatStep`]s. It is the wiring: the coordinator *produces* the
//!   [`FormationFacts`] the planner *consumes*, so a leader promoted by a
//!   recovery is the leader the next tick's decision reasons about.
//!
//! # Recovery is applied, not only reported
//!
//! F32-A's [`RecoveryOutcome`] *reports* the declared recovery path; on its own
//! that is a trace entry and nothing would ever happen. F32-C's
//! [`FormationCoordinator::apply`] is the half that acts: it promotes a new
//! leader, recomputes the regroup point from the survivors, releases stations,
//! and dissolves a formation whose members are all gone. Every station it
//! hands out is a finite world point anchored on a **living** member or on the
//! survivors' centroid — never on a destroyed actor, and never derived from a
//! normalized direction that a coincident position could turn into a NaN. That
//! is what AC03's "no NaNs or permanent orbit" means in code.
//!
//! # Difficulty selects, it never composes
//!
//! [`DifficultyRoster`] maps a tier to the already-lowered [`SkillProfile`]s
//! for that tier and [`AceVariant`] names the tier its profile was lowered
//! for. The runtime *selects* between them and refuses an unknown tier, an
//! unknown ace, an ace of another role and an ace lowered for another tier.
//! It never composes a tier onto a profile, because that composition belongs
//! to the lowering boundary (#551). Since a [`SkillProfile`] has no field a
//! rate, a time scale or a damage multiplier could be expressed in, F32
//! non-negotiable 1 ("never increase simulation speed to fake difficulty")
//! holds by construction rather than by review.
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
//! `docs/findings/2026-10-01-f32-a-combat-roles-skill-knobs-and-decision-traces.md`
//! and, for the maneuvers and firing solutions, in
//! `docs/findings/2026-10-03-f32-b-maneuvers-priority-and-firing-solutions.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
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
///
/// `Ord` follows [`RecoveryTrigger::ALL`], so the F32-C coordinator can hold
/// the set of already-answered triggers in a `BTreeSet` and report them in a
/// stable order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

// --------------------------------------------------------- maneuver ----

/// The maneuver a role intends to fly this tick.
///
/// The F32 sheet separates *role selection* (what a script assigned) from
/// *maneuver selection* (what that role does about the situation it is in).
/// This is the typed intent the navigation layer consumes: it names no
/// waypoint, no heading and no control law, so choosing it cannot make the
/// planner a second pose owner (`docs/contracts/FLIGHT-PHYSICS.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CombatManeuver {
    /// Close with and track the selected target.
    Engage,
    /// Fly an ordnance delivery run at the selected target.
    AttackRun,
    /// Hold station with the protected actor and answer what threatens it.
    Screen,
    /// Break away from the selected threat without leaving the mission.
    BreakAway,
    /// Withdraw from the engagement.
    Withdraw,
    /// Nothing to act on: no target was selected and the role is not
    /// ordered to break off, so the current path is held.
    Hold,
}

impl CombatManeuver {
    /// Every maneuver, in a stable order.
    pub const ALL: &'static [CombatManeuver] = &[
        Self::Engage,
        Self::AttackRun,
        Self::Screen,
        Self::BreakAway,
        Self::Withdraw,
        Self::Hold,
    ];

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Engage => "engage",
            Self::AttackRun => "attack_run",
            Self::Screen => "screen",
            Self::BreakAway => "break_away",
            Self::Withdraw => "withdraw",
            Self::Hold => "hold",
        }
    }

    /// The maneuver one role flies with or without a selected target.
    ///
    /// The map is total and deterministic: the same `(role, has_target)`
    /// always yields the same maneuver, so an output cannot depend on the
    /// order a caller happened to consider the roles in.
    #[must_use]
    pub const fn select(role: CombatRole, has_target: bool) -> Self {
        match role {
            CombatRole::FighterAttack | CombatRole::Intercept => {
                if has_target {
                    Self::Engage
                } else {
                    Self::Hold
                }
            }
            CombatRole::BomberRun | CombatRole::TorpedoRun => {
                if has_target {
                    Self::AttackRun
                } else {
                    Self::Hold
                }
            }
            // An escort's default posture is to stay with its charge; it
            // only closes when it has something to answer.
            CombatRole::Escort => {
                if has_target {
                    Self::Engage
                } else {
                    Self::Screen
                }
            }
            CombatRole::Evade => Self::BreakAway,
            CombatRole::Retreat => Self::Withdraw,
        }
    }
}

impl fmt::Display for CombatManeuver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

// --------------------------------------------------- firing solution ----

/// One mount's verdict in a firing solution.
///
/// The two availability reasons are deliberately distinct: a disabled gun
/// and an empty rocket rack are different failures, and neither is a
/// question of skill. A high-skill shooter cannot fire either one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MountFireState {
    /// The mount fires this tick.
    Firing,
    /// The mount's damage node was destroyed (F29), so the shot is refused
    /// whatever the shooter's skill.
    Disabled,
    /// The mount is intact but has no rounds left (F27/F28), so the shot
    /// is refused whatever the shooter's skill.
    Empty,
    /// The mount is loaded but still on its cadence, so it is not fired
    /// this tick. This is a *schedule* refusal, not an availability one:
    /// the mount is intact and armed.
    CoolingDown {
        /// Ticks remaining before the mount may fire.
        remaining_ticks: u64,
    },
    /// The role's [`RoleArsenal`] does not declare this mount's kind, so a
    /// guns-only role never fires ordnance and a torpedo role never fires
    /// guns.
    WrongKind,
}

impl MountFireState {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Firing => "firing",
            Self::Disabled => "disabled",
            Self::Empty => "empty",
            Self::CoolingDown { .. } => "cooling_down",
            Self::WrongKind => "wrong_kind",
        }
    }
}

impl fmt::Display for MountFireState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One mount's contribution to a firing solution.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MountFire {
    /// The mount, in the same [`DamageNodeKey`] namespace the F29 damage
    /// graph disables.
    pub mount: DamageNodeKey,
    /// Whether the mount carries a gun or ordnance.
    pub kind: MountKind,
    /// Whether the mount fires, and if not, why.
    pub state: MountFireState,
}

/// Why a firing solution fires nothing this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FireHoldReason {
    /// The role's [`RoleArsenal`] declares no weapon kind at all — the
    /// evasion and retreat roles.
    RoleUnarmed,
    /// No mount of a kind the role may use can fire this tick, with the
    /// count of each distinct refusal. A checker that only looked at the
    /// total would hide whether the cause was destruction, exhaustion or
    /// cadence.
    NoUsableMount {
        /// Mounts whose damage node was destroyed.
        disabled: u32,
        /// Mounts that are intact but out of rounds.
        empty: u32,
        /// Mounts that are loaded but still cooling down.
        cooling: u32,
        /// Mounts whose kind the role does not declare.
        wrong_kind: u32,
    },
}

/// Which of one actor's mounts may fire this tick, and the aim error the
/// shot carries.
///
/// F32-B's AC02: an ace fires off the same [`ArsenalSnapshot`] the player
/// does, and a disabled mount or an empty rack is refused by name whatever
/// the shooter's skill. The solution never invents a mount that the
/// snapshot did not report, and it never fires two shots from one mount.
///
/// Line-of-fire avoidance is deliberately *not* folded in here (F32
/// non-negotiable 3): the friendly-in-line-of-fire veto is a property of
/// the selected candidate and stays on [`CandidateTrace::fire_veto`], so
/// "my mount may fire" and "the line to my target is clear" remain two
/// separate answers.
#[derive(Clone, Debug, PartialEq)]
pub struct FiringSolution {
    /// The selected target the solution is for.
    pub target: ActorId,
    /// The tick the solution was made on.
    pub tick: Tick,
    /// The half-angle of aim error the profile adds to the shot.
    pub aim_error_rad: f64,
    /// The AI's own minimum cadence between fire decisions, from the
    /// profile's `fire_discipline_ticks`.
    ///
    /// The per-mount `cooldown_ticks` a [`MountAvailability`] carries is the
    /// weapon's schedule and is enforced here. This knob is the *actor's*
    /// cadence across ticks; keeping that state is the session's job
    /// (F32-C), so the solution reports it rather than silently dropping a
    /// declared behavior knob.
    pub fire_discipline_ticks: u64,
    /// The role's declared arsenal rule the solution applied.
    pub arsenal: RoleArsenal,
    /// Every mount the snapshot reported, in declared order, with its
    /// verdict.
    pub mounts: Vec<MountFire>,
}

impl FiringSolution {
    /// Solves one tick for one shooter against one already-selected target.
    ///
    /// The profile and the snapshot are both validated at construction, so
    /// this is a pure classification: a mount the role may not use is
    /// [`MountFireState::WrongKind`], otherwise a destroyed mount is
    /// [`MountFireState::Disabled`], otherwise an out-of-rounds mount is
    /// [`MountFireState::Empty`], otherwise a mount still on its cadence is
    /// [`MountFireState::CoolingDown`], otherwise it
    /// [`MountFireState::Firing`]s.
    #[must_use]
    pub fn solve(
        profile: &SkillProfile,
        target: ActorId,
        tick: Tick,
        arsenal: &ArsenalSnapshot,
    ) -> Self {
        let rule = profile.arsenal();
        let mut mounts = Vec::with_capacity(arsenal.mounts().len());
        for mount in arsenal.mounts() {
            let allowed = match mount.kind {
                MountKind::Gun => rule.gun,
                MountKind::Ordnance => rule.ordnance,
            };
            let state = if !allowed {
                MountFireState::WrongKind
            } else if mount.disabled {
                MountFireState::Disabled
            } else if mount.rounds == 0 {
                MountFireState::Empty
            } else if mount.cooldown_ticks > 0 {
                MountFireState::CoolingDown {
                    remaining_ticks: mount.cooldown_ticks,
                }
            } else {
                MountFireState::Firing
            };
            mounts.push(MountFire {
                mount: mount.mount.clone(),
                kind: mount.kind,
                state,
            });
        }
        Self {
            target,
            tick,
            aim_error_rad: profile.knobs().aim_error_rad,
            fire_discipline_ticks: profile.knobs().fire_discipline_ticks,
            arsenal: rule,
            mounts,
        }
    }

    /// The mounts that fire this tick, in declared order.
    pub fn firing(&self) -> impl Iterator<Item = &MountFire> {
        self.mounts
            .iter()
            .filter(|fire| fire.state == MountFireState::Firing)
    }

    /// Whether at least one mount fires this tick.
    #[must_use]
    pub fn is_firing(&self) -> bool {
        self.mounts
            .iter()
            .any(|fire| fire.state == MountFireState::Firing)
    }

    /// The verdict for one mount, by key.
    #[must_use]
    pub fn state(&self, mount: &DamageNodeKey) -> Option<MountFireState> {
        self.mounts
            .iter()
            .find(|fire| &fire.mount == mount)
            .map(|fire| fire.state)
    }

    /// Why nothing fires this tick, or `None` when a shot is available.
    ///
    /// An unarmed role reports [`FireHoldReason::RoleUnarmed`] whatever its
    /// snapshot carries; any other role reports the count of each refusal
    /// so the cause is visible rather than only the absence of a shot.
    #[must_use]
    pub fn hold(&self) -> Option<FireHoldReason> {
        if self.is_firing() {
            return None;
        }
        if self.arsenal == RoleArsenal::none() {
            return Some(FireHoldReason::RoleUnarmed);
        }
        let mut disabled = 0;
        let mut empty = 0;
        let mut cooling = 0;
        let mut wrong_kind = 0;
        for fire in &self.mounts {
            match fire.state {
                MountFireState::Firing => {}
                MountFireState::Disabled => disabled += 1,
                MountFireState::Empty => empty += 1,
                MountFireState::CoolingDown { .. } => cooling += 1,
                MountFireState::WrongKind => wrong_kind += 1,
            }
        }
        Some(FireHoldReason::NoUsableMount {
            disabled,
            empty,
            cooling,
            wrong_kind,
        })
    }
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
    /// The maneuver the role flies this tick (F32-B).
    pub maneuver: CombatManeuver,
    /// The firing solution for the selected target, when there is one and
    /// the caller supplied an arsenal snapshot (F32-B).
    pub firing: Option<FiringSolution>,
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

    /// Builds the firing solution for one selected target.
    ///
    /// The per-session authority is preserved here as it is everywhere
    /// else: a target from another session generation is refused by name,
    /// so a caller cannot ask this session to solve fire against a stale
    /// actor even though the classification itself only reads the profile
    /// and the snapshot.
    ///
    /// # Errors
    ///
    /// [`CombatError::ForeignSession`] when the target belongs to another
    /// session generation.
    pub fn firing_solution(
        &self,
        profile: &SkillProfile,
        target: ActorId,
        tick: Tick,
        arsenal: &ArsenalSnapshot,
    ) -> Result<FiringSolution, CombatError> {
        if target.session.get() != self.session {
            return Err(CombatError::ForeignSession {
                actor: target,
                session: self.session,
            });
        }
        Ok(FiringSolution::solve(profile, target, tick, arsenal))
    }

    /// Decides one target for one tick, then selects the maneuver the role
    /// flies and, when the caller supplied an arsenal and a target was
    /// chosen, the firing solution for that target (F32-B).
    ///
    /// Target selection comes first and is unchanged: the trace's candidate
    /// scoring is the record the later stages consume. An `Evade` or
    /// `Retreat` assignment is still given the target its policy ranks;
    /// [`CombatManeuver::select`] then chooses the break-away or withdrawal
    /// the role actually flies, and [`FiringSolution::solve`] reads the
    /// same availability the player's shot would.
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
        if request.observer.session.get() != self.session {
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
            && actor.session.get() != self.session
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
            for actor in [Some(facts.leader), facts.assigned_target]
                .into_iter()
                .flatten()
            {
                if actor.session.get() != self.session {
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

        // The maneuver is a function of the role and whether a target was
        // chosen; the firing solution is a function of the effective
        // profile and the snapshot the caller supplied. Neither consults
        // the world, so the decision stays a pure function of its request.
        let maneuver = CombatManeuver::select(profile.role(), chosen.is_some());
        let firing = match (chosen, request.arsenal) {
            (Some(target), Some(arsenal)) => {
                Some(self.firing_solution(&profile, target, request.now, arsenal)?)
            }
            _ => None,
        };

        let trace = DecisionTrace {
            observer: request.observer,
            tick: request.now,
            role: profile.role(),
            protected_actor: protected,
            arsenal: arsenal_report,
            maneuver,
            firing,
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
        if candidate.actor.session.get() != self.session {
            return Err(CombatError::ForeignSession {
                actor: candidate.actor,
                session: self.session,
            });
        }
        if let Some(threat) = &candidate.threat {
            if threat.hit.session.get() != self.session {
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

// ------------------------------------------------- difficulty ----

/// The runtime mirror of `cs_content::ai::DifficultyTier`.
///
/// The tier is the *key* a mission's selected difficulty resolves against; it
/// carries no number of its own. Every value a tier moves arrives already
/// lowered into a [`SkillProfile`], so this crate never re-derives a tier's
/// effect and cannot invent a simulation-rate difference instead
/// (non-negotiable 1).
///
/// Whether the original game offers exactly these four steps, in this order,
/// under these names is **unmeasured** (F32-D). The ordering is project design.
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
}

impl fmt::Display for DifficultyTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The lowered effect of each declared difficulty tier: one [`SkillProfile`]
/// per role for that tier.
///
/// A tier's entry is what the lowering boundary already produced, so this is
/// the *only* place a difficulty choice becomes a behavior profile and it
/// holds nothing else — no tick rate, no time scale, no damage multiplier, no
/// weapon grant. Non-negotiable 1 therefore holds by construction: a
/// [`SkillProfile`] has no field any of those could be expressed in, and
/// [`RoleArsenal`] is the role's structural requirement rather than a knob,
/// so no tier can hand a guns-only role a rocket rack.
///
/// An *undeclared* tier is refused by name rather than silently resolved to the
/// baseline: choosing difficulty is an explicit mission decision, and defaulting
/// it would hide a content bug behind a plausible profile.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DifficultyRoster {
    tiers: BTreeMap<DifficultyTier, BTreeMap<CombatRole, SkillProfile>>,
}

impl DifficultyRoster {
    /// An empty roster: no tier resolves yet.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tiers: BTreeMap::new(),
        }
    }

    /// Declares one tier's lowered role profiles.
    ///
    /// # Errors
    ///
    /// [`CombatError::DuplicateDifficultyTier`] when the tier is declared
    /// twice, [`CombatError::DuplicateRoleProfile`] when two profiles carry
    /// the same role, and the validation errors of
    /// [`SkillProfile::try_new`] — a caller cannot smuggle an unbounded or
    /// non-finite number in through a difficulty profile, because the profile
    /// it would have to build is validated first.
    pub fn with_tier(
        mut self,
        tier: DifficultyTier,
        profiles: &[SkillProfile],
    ) -> Result<Self, CombatError> {
        if self.tiers.contains_key(&tier) {
            return Err(CombatError::DuplicateDifficultyTier { tier });
        }
        let mut declared = BTreeMap::new();
        for profile in profiles {
            if declared.insert(profile.role(), *profile).is_some() {
                return Err(CombatError::DuplicateRoleProfile {
                    role: profile.role(),
                });
            }
        }
        self.tiers.insert(tier, declared);
        Ok(self)
    }

    /// Whether a tier is declared.
    #[must_use]
    pub fn has_tier(&self, tier: DifficultyTier) -> bool {
        self.tiers.contains_key(&tier)
    }

    /// The declared tiers, from most forgiving to most demanding.
    pub fn tiers(&self) -> impl Iterator<Item = DifficultyTier> + '_ {
        self.tiers.keys().copied()
    }

    /// The roles one tier declares, in ascending order.
    pub fn roles(&self, tier: DifficultyTier) -> Vec<CombatRole> {
        self.tiers
            .get(&tier)
            .map(|roles| roles.keys().copied().collect())
            .unwrap_or_default()
    }

    /// The profile one role runs at one tier.
    ///
    /// # Errors
    ///
    /// [`CombatError::UnknownDifficultyTier`] when the tier is not declared
    /// and [`CombatError::DifficultyTierMissingRole`] when the tier is
    /// declared but says nothing about that role.
    pub fn profile(
        &self,
        tier: DifficultyTier,
        role: CombatRole,
    ) -> Result<SkillProfile, CombatError> {
        let declared = self
            .tiers
            .get(&tier)
            .ok_or(CombatError::UnknownDifficultyTier { tier })?;
        declared
            .get(&role)
            .copied()
            .ok_or(CombatError::DifficultyTierMissingRole { tier, role })
    }
}

/// The runtime identity of one ace variant: a `pilot` catalog id.
///
/// A separate type from [`ActorId`] for the same reason
/// `crates::allies::PilotId` is: the *content* identity of a behavior variant
/// is not an *actor*, so a pilot id can never be mistaken for one
/// (`docs/contracts/IDENTITY-CONTENT.md`, the `pilot` namespace).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AceId(ContentId);

impl AceId {
    /// Wraps a content id, refusing one outside the `pilot` namespace.
    ///
    /// # Errors
    ///
    /// [`CombatError::AceKindMismatch`] for any other namespace: an ace is a
    /// pilot's behavior, not an airframe's.
    pub fn try_new(id: ContentId) -> Result<Self, CombatError> {
        if id.kind() != ContentKind::Pilot {
            return Err(CombatError::AceKindMismatch { id });
        }
        Ok(Self(id))
    }

    /// The wrapped catalog id.
    #[must_use]
    pub const fn as_content(&self) -> &ContentId {
        &self.0
    }
}

impl fmt::Display for AceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One lowered ace variant: the behavior profile a pilot flies.
///
/// The variant is *data*: a pilot id, the role it modifies, the tier its
/// profile was lowered for, and the profile itself. It has no damage, armor or
/// health field and no tick rate, so an ace is a behavior/skill variant and
/// not inflated health (F32 "Deliverable and interfaces", non-negotiable 1).
///
/// The declared counterpart is `cs_content::ai::DeclaredAceProfile`; the
/// boundary (#551) turns its [`SkillKnob`](cs_content_ai::SkillKnob)
/// overrides into this profile. This crate never re-applies an override list,
/// so an ace can never be lowered twice.
#[derive(Clone, Debug, PartialEq)]
pub struct AceVariant {
    id: AceId,
    base_role: CombatRole,
    tier: DifficultyTier,
    profile: SkillProfile,
}

impl AceVariant {
    /// Assembles one lowered variant.
    ///
    /// # Errors
    ///
    /// [`CombatError::AceRoleMismatch`] when the profile belongs to another
    /// role than the variant modifies: an ace of the fighter role cannot fly
    /// an escort, and re-interpreting it would silently give an actor a
    /// behavior its mission never assigned.
    pub fn try_new(
        id: AceId,
        base_role: CombatRole,
        tier: DifficultyTier,
        profile: SkillProfile,
    ) -> Result<Self, CombatError> {
        if profile.role() != base_role {
            return Err(CombatError::AceRoleMismatch {
                id,
                base: base_role,
                profile: profile.role(),
            });
        }
        Ok(Self {
            id,
            base_role,
            tier,
            profile,
        })
    }

    /// The variant's identity.
    #[must_use]
    pub const fn id(&self) -> &AceId {
        &self.id
    }

    /// The role this variant modifies.
    #[must_use]
    pub const fn base_role(&self) -> CombatRole {
        self.base_role
    }

    /// The tier this variant's profile was lowered for.
    #[must_use]
    pub const fn tier(&self) -> DifficultyTier {
        self.tier
    }

    /// The behavior profile the ace flies.
    #[must_use]
    pub const fn profile(&self) -> &SkillProfile {
        &self.profile
    }
}

// ----------------------------------------------------- formations ----

/// The designed trailing spacing between formation slots, in meters.
///
/// A station offset is an *authored constant* per slot, never the result of
/// normalizing a direction: `normalize(zero_vector)` is the classic way to put
/// a NaN into a guidance input, and AC03 requires that no follower ever
/// receives one (F32 "Research boundary": the original's formation spacing is
/// unmeasured, so this is project design).
pub const FORMATION_TRAIL_SPACING_M: f64 = 120.0;

/// The designed radius a follower aims for when it takes its station.
pub const FORMATION_STATION_RADIUS_M: f64 = 60.0;

/// The designed offset one slot keeps astern of its formation's anchor, in
/// canonical world meters.
///
/// Slot 0 leads and keeps the anchor itself; every other slot trails it by a
/// whole [`FORMATION_TRAIL_SPACING_M`] per index. The result is a pure
/// function of the slot index, so it is finite for every index and never
/// depends on the geometry of the actors in it.
#[must_use]
pub fn station_offset_m(slot: u32) -> [f64; 3] {
    [-(slot as f64) * FORMATION_TRAIL_SPACING_M, 0.0, 0.0]
}

/// One declared member of a formation, as the runtime registers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FormationRosterMember {
    /// The slot the member fills.
    pub slot: u32,
    /// The actor that fills it.
    pub actor: ActorId,
}

/// The declared membership of one formation: which slot leads and who fills
/// each slot.
///
/// The declared counterpart is `cs_content::ai::DeclaredFormation`; the
/// boundary lowers its `leader_slot` and `members` onto this record.
#[derive(Clone, Debug, PartialEq)]
pub struct FormationRoster {
    formation: FormationId,
    leader_slot: u32,
    members: Vec<FormationRosterMember>,
}

impl FormationRoster {
    /// Assembles and validates a formation's membership.
    ///
    /// # Errors
    ///
    /// [`CombatError::EmptyFormation`] for a formation with no member,
    /// [`CombatError::FormationLeaderNotAMember`] when the leading slot is not
    /// one of the members — a formation that leads nobody cannot elect a
    /// successor — and [`CombatError::FormationSlotOccupiedTwice`] when two
    /// members share one slot.
    pub fn try_new(
        formation: FormationId,
        leader_slot: u32,
        members: Vec<FormationRosterMember>,
    ) -> Result<Self, CombatError> {
        if members.is_empty() {
            return Err(CombatError::EmptyFormation { formation });
        }
        if !members.iter().any(|member| member.slot == leader_slot) {
            return Err(CombatError::FormationLeaderNotAMember {
                formation,
                leader_slot,
            });
        }
        let mut slots = BTreeSet::new();
        for member in &members {
            if !slots.insert(member.slot) {
                return Err(CombatError::FormationSlotOccupiedTwice {
                    formation,
                    slot: member.slot,
                });
            }
        }
        Ok(Self {
            formation,
            leader_slot,
            members,
        })
    }

    /// The formation this roster describes.
    #[must_use]
    pub const fn formation(&self) -> FormationId {
        self.formation
    }

    /// The slot that leads the formation.
    #[must_use]
    pub const fn leader_slot(&self) -> u32 {
        self.leader_slot
    }

    /// The declared members, in the order they were given.
    #[must_use]
    pub fn members(&self) -> &[FormationRosterMember] {
        &self.members
    }
}

/// One member's report for one formation tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormationMemberReport {
    /// The slot the report is about.
    pub slot: u32,
    /// The actor the runtime has in that slot.
    pub actor: ActorId,
    /// The member's canonical world position, as the motion producer reports
    /// it.
    pub position: WorldPosition,
    /// Whether the member is still alive. A member reported destroyed is
    /// retired: it leaves the formation's shape for good.
    pub alive: bool,
}

/// One tick of formation facts, as the producers report them.
///
/// `members` must list **every** registered member exactly once. A partial
/// report is refused rather than reconciled, because a centroid computed over
/// a partial membership is a different point than the same centroid over the
/// whole one, and a recovery that silently used the wrong point is exactly the
/// kind of plausible-but-wrong output this engine refuses to produce.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormationTick<'a> {
    /// The formation this report is about.
    pub formation: FormationId,
    /// The tick the report is for.
    pub now: Tick,
    /// Every registered member, exactly once.
    pub members: &'a [FormationMemberReport],
    /// The formation's assigned target, when it has one.
    pub assigned_target: Option<ActorId>,
    /// Whether that assigned target is still alive.
    pub assigned_target_alive: bool,
    /// Whether the formation's declared route is available.
    pub route_available: bool,
}

/// What one follower's station is anchored on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StationAnchor {
    /// The formation's surviving leader. It is a live actor by construction:
    /// [`StationAnchor::Leader`] is never built for a destroyed member, which
    /// is the orbit AC03 forbids.
    Leader(ActorId),
    /// The regroup point, computed from the formation's surviving members.
    RegroupPoint,
}

/// One follower's station: the world point it is to fly to.
///
/// A station is a *position*, never a pose: no heading, no attitude and no
/// control law, so following it cannot make this module a second pose owner
/// (`docs/contracts/FLIGHT-PHYSICS.md`). The navigation layer consumes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StationAssignment {
    /// The member the station belongs to.
    pub actor: ActorId,
    /// The slot the member holds.
    pub slot: u32,
    /// What the station is anchored on.
    pub anchor: StationAnchor,
    /// The station itself, in canonical world meters.
    pub position: WorldPosition,
    /// How close the member should come to it, in meters.
    pub radius_m: f64,
}

/// What one formation's recovery did on one tick.
///
/// This is the runtime trace F32-D's probe and the consumer read: which
/// trigger fired, which declared action answered it, who leads afterwards,
/// where every living follower was sent, who was released and whether the
/// formation dissolved.
#[derive(Clone, Debug, PartialEq)]
pub struct FormationUpdate {
    /// The formation the update belongs to.
    pub formation: FormationId,
    /// The tick it was applied on.
    pub now: Tick,
    /// The leader before this tick's recovery, when one was living.
    pub previous_leader: Option<ActorId>,
    /// The leader after this tick's recovery, when one is living.
    pub leader: Option<ActorId>,
    /// The trigger this update answered, when it answered one.
    pub trigger: Option<RecoveryTrigger>,
    /// The declared action it applied.
    pub action: Option<RecoveryAction>,
    /// The station every living follower is now assigned, ascending by slot.
    /// The leader never receives one: it leads.
    pub stations: Vec<StationAssignment>,
    /// The living members whose station was released, ascending by actor.
    pub released: Vec<ActorId>,
    /// A trigger that is still pending but has already been answered, so it is
    /// not answered again until its fact recovers. A recovery that re-fired
    /// every tick would be a permanent state, not a recovery.
    pub latched: Option<RecoveryTrigger>,
    /// Whether the formation dissolved on this tick because nothing survived.
    pub dissolved: bool,
}

impl FormationUpdate {
    /// The anchor every station in this update shares, when there is one.
    #[must_use]
    pub fn anchor(&self) -> Option<StationAnchor> {
        self.stations.first().map(|station| station.anchor)
    }

    /// The station of one member, by identity.
    #[must_use]
    pub fn station(&self, actor: ActorId) -> Option<StationAssignment> {
        self.stations
            .iter()
            .copied()
            .find(|station| station.actor == actor)
    }
}

/// One member's runtime state inside a formation.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MemberRuntime {
    actor: ActorId,
    position: WorldPosition,
    alive: bool,
}

/// What a formation's stations are currently anchored on.
#[derive(Clone, Copy, Debug, PartialEq)]
enum AnchorMode {
    /// The formation's surviving leader.
    Leader,
    /// The regroup point last computed from the survivors.
    RegroupPoint(WorldPosition),
}

/// One formation's mutable runtime state.
#[derive(Clone, Debug, PartialEq)]
struct FormationRuntime {
    formation: FormationId,
    members: BTreeMap<u32, MemberRuntime>,
    leader_slot: u32,
    anchor: AnchorMode,
    last_tick: Option<Tick>,
    assigned_target: Option<ActorId>,
    assigned_target_alive: bool,
    route_available: bool,
    latched: BTreeSet<RecoveryTrigger>,
}

impl FormationRuntime {
    /// The living leader, when the leading slot still holds a living member.
    fn leader(&self) -> Option<ActorId> {
        self.members
            .get(&self.leader_slot)
            .filter(|member| member.alive)
            .map(|member| member.actor)
    }

    /// The actor that occupies the leading slot, alive or not.
    fn registered_leader(&self) -> Option<ActorId> {
        self.members
            .get(&self.leader_slot)
            .map(|member| member.actor)
    }

    /// The living members, ascending by slot.
    fn survivors(&self) -> impl Iterator<Item = (u32, &MemberRuntime)> {
        self.members
            .iter()
            .filter(|(_, member)| member.alive)
            .map(|(slot, member)| (*slot, member))
    }

    fn survivor_count(&self) -> usize {
        self.survivors().count()
    }

    /// The trigger that is pending right now, in the same fixed precedence
    /// [`FormationFacts::pending_trigger`] uses.
    ///
    /// With no survivor there is nothing to recover, so no trigger is raised:
    /// the tick ends in dissolution instead of electing a leader out of the
    /// dead.
    fn pending_trigger(&self) -> Option<RecoveryTrigger> {
        if self.leader().is_none() && self.survivor_count() > 0 {
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

    /// The centroid of the living members, in canonical world meters.
    ///
    /// Only the survivors are averaged, so a leader at an extreme coordinate
    /// cannot contaminate the point. The mean is re-validated as a
    /// [`WorldPosition`] before it leaves here: summing two coordinates large
    /// enough to overflow `f64` would otherwise produce an infinite station,
    /// and a non-finite station must be refused by name rather than handed to
    /// the navigation layer.
    fn centroid(&self, formation: FormationId) -> Result<WorldPosition, CombatError> {
        let mut sum = [0.0_f64; 3];
        let mut count = 0.0_f64;
        for member in self.members.values().filter(|member| member.alive) {
            sum[0] += member.position.x();
            sum[1] += member.position.y();
            sum[2] += member.position.z();
            count += 1.0;
        }
        if count == 0.0 {
            return Err(CombatError::NoSurvivingMember { formation });
        }
        finite_position(formation, [sum[0] / count, sum[1] / count, sum[2] / count])
    }

    /// The point the stations are anchored on right now, or `None` when the
    /// formation has nothing to anchor on (no leader and no computed regroup
    /// point yet).
    fn resolve_anchor(&self) -> Option<(StationAnchor, WorldPosition)> {
        match self.anchor {
            AnchorMode::RegroupPoint(point) => Some((StationAnchor::RegroupPoint, point)),
            AnchorMode::Leader => {
                let leader = self.leader()?;
                let point = self.members.get(&self.leader_slot)?.position;
                Some((StationAnchor::Leader(leader), point))
            }
        }
    }

    /// Every living follower's station, ascending by slot.
    fn stations(&self, formation: FormationId) -> Result<Vec<StationAssignment>, CombatError> {
        let Some((anchor, anchor_point)) = self.resolve_anchor() else {
            return Ok(Vec::new());
        };
        let mut stations = Vec::with_capacity(self.survivor_count());
        let leader_leads = matches!(anchor, StationAnchor::Leader(_));
        for (slot, member) in self.survivors() {
            // The leader leads: it holds no station of its own.
            if slot == self.leader_slot && leader_leads {
                continue;
            }
            let offset = station_offset_m(slot);
            let position = finite_position(
                formation,
                [
                    anchor_point.x() + offset[0],
                    anchor_point.y() + offset[1],
                    anchor_point.z() + offset[2],
                ],
            )?;
            stations.push(StationAssignment {
                actor: member.actor,
                slot,
                anchor,
                position,
                radius_m: FORMATION_STATION_RADIUS_M,
            });
        }
        Ok(stations)
    }

    /// Promotes the lowest living slot to lead.
    ///
    /// A designed succession rule, not a measured one: the original's
    /// promotion order is unmeasured (F32-D). "Lowest slot" is used because it
    /// is total and deterministic, so two followers in the same situation can
    /// never both believe they were promoted. A survivor that already holds the
    /// leading slot keeps it.
    fn promote_leader(&mut self) {
        let leading = self.leader_slot;
        let promotion = self
            .survivors()
            .map(|(slot, _)| slot)
            .find(|slot| *slot != leading);
        if let Some(slot) = promotion {
            self.leader_slot = slot;
        }
        self.anchor = AnchorMode::Leader;
    }

    /// Folds one tick's report into this formation's state.
    fn reconcile(&mut self, tick: &FormationTick<'_>) -> Result<(), CombatError> {
        let mut seen = BTreeSet::new();
        for report in tick.members {
            if !seen.insert(report.slot) {
                return Err(CombatError::FormationMemberReportedTwice {
                    formation: self.formation,
                    slot: report.slot,
                });
            }
            let member = self.members.get(&report.slot).copied().ok_or(
                CombatError::UnknownFormationSlot {
                    formation: self.formation,
                    slot: report.slot,
                },
            )?;
            if member.actor != report.actor {
                return Err(CombatError::FormationMemberMismatch {
                    formation: self.formation,
                    slot: report.slot,
                    registered: member.actor,
                    reported: report.actor,
                });
            }
            if !member.alive && report.alive {
                return Err(CombatError::RetiredFormationMember {
                    formation: self.formation,
                    slot: report.slot,
                    actor: member.actor,
                });
            }
            self.members.insert(
                report.slot,
                MemberRuntime {
                    actor: report.actor,
                    position: report.position,
                    alive: report.alive,
                },
            );
        }
        for slot in self.members.keys() {
            if !seen.contains(slot) {
                return Err(CombatError::FormationMemberNotReported {
                    formation: self.formation,
                    slot: *slot,
                });
            }
        }
        Ok(())
    }

    /// Applies the declared action for one trigger.
    fn apply_action(
        &mut self,
        trigger: RecoveryTrigger,
        action: RecoveryAction,
    ) -> Result<(), CombatError> {
        let formation = self.formation;
        match action {
            RecoveryAction::ReassignLead => self.promote_leader(),
            RecoveryAction::Regroup => {
                self.anchor = AnchorMode::RegroupPoint(self.centroid(formation)?);
            }
            // "Hold the shape" means keep the shape, not keep aiming at a
            // wreck: with a living leader nothing moves, and without one the
            // survivors close onto the point they already share.
            RecoveryAction::HoldFormation => {
                if self.leader().is_none() {
                    self.anchor = AnchorMode::RegroupPoint(self.centroid(formation)?);
                }
            }
            // "Leave the engagement" has no geometry: the caller releases the
            // stations and tears the formation down in the same breath.
            RecoveryAction::Withdraw => {}
            // "Return to the route" restores the shape the formation had
            // before the interruption: leader-led when a leader lives, closed
            // up otherwise. Route *following* belongs to navigation.
            RecoveryAction::ResumeRoute => {
                if self.leader().is_some() {
                    self.anchor = AnchorMode::Leader;
                } else {
                    self.anchor = AnchorMode::RegroupPoint(self.centroid(formation)?);
                }
            }
        }
        self.latched.insert(trigger);
        Ok(())
    }

    /// Forgets a latched trigger once the fact it came from recovered.
    fn release_latches(&mut self) {
        if self.leader().is_some() {
            self.latched.remove(&RecoveryTrigger::LeaderLost);
        }
        if self.assigned_target.is_none() || self.assigned_target_alive {
            self.latched
                .remove(&RecoveryTrigger::AssignedTargetDestroyed);
        }
        if self.route_available {
            self.latched.remove(&RecoveryTrigger::RouteInterrupted);
        }
    }
}

/// Validates a computed station point, refusing a non-finite one by name.
fn finite_position(formation: FormationId, value: [f64; 3]) -> Result<WorldPosition, CombatError> {
    WorldPosition::try_new(value).map_err(|_| CombatError::StationNotFinite { formation })
}

/// The per-session formation runtime: membership, leadership and the declared
/// recovery actions, applied tick by tick.
///
/// This is the half of F32 that actually *acts*. [`CombatPlanner::decide`]
/// reports which recovery path is pending; the coordinator applies it, keeps
/// the resulting leadership and hands every living follower a finite station.
///
/// # Ordering, and why a refused tick changes nothing
///
/// [`FormationCoordinator::apply`] validates the whole report and computes the
/// whole next state before it commits any of it, so a refused tick leaves the
/// coordinator byte-identical and the caller may re-send a corrected report for
/// the same tick. A tick that is not strictly newer than the last applied one
/// is refused by name ([`CombatError::StaleFormationTick`]): replaying an old
/// tick would let a stale leader come back from the dead and would elect a
/// second leader, so the coordinator refuses rather than guessing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormationCoordinator {
    session: u64,
    formations: BTreeMap<FormationId, FormationRuntime>,
}

impl FormationCoordinator {
    /// An empty coordinator for session `session`.
    #[must_use]
    pub const fn new(session: u64) -> Self {
        Self {
            session,
            formations: BTreeMap::new(),
        }
    }

    /// The session generation this coordinator owns.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// The formations currently held, in ascending order.
    pub fn formations(&self) -> impl Iterator<Item = FormationId> + '_ {
        self.formations.keys().copied()
    }

    /// Whether the coordinator still holds a formation.
    #[must_use]
    pub fn is_registered(&self, formation: FormationId) -> bool {
        self.formations.contains_key(&formation)
    }

    /// Registers one formation's declared membership.
    ///
    /// # Errors
    ///
    /// [`CombatError::DuplicateFormation`] when the formation is registered
    /// twice, [`CombatError::ForeignSession`] when a member belongs to another
    /// session generation, and the validation errors of
    /// [`FormationRoster::try_new`].
    pub fn register(&mut self, roster: &FormationRoster) -> Result<(), CombatError> {
        if self.formations.contains_key(&roster.formation) {
            return Err(CombatError::DuplicateFormation {
                formation: roster.formation,
            });
        }
        let mut members = BTreeMap::new();
        for declared in &roster.members {
            if declared.actor.session.get() != self.session {
                return Err(CombatError::ForeignSession {
                    actor: declared.actor,
                    session: self.session,
                });
            }
            members.insert(
                declared.slot,
                MemberRuntime {
                    actor: declared.actor,
                    position: WorldPosition::try_new([0.0, 0.0, 0.0])
                        .expect("the origin is finite"),
                    alive: true,
                },
            );
        }
        self.formations.insert(
            roster.formation,
            FormationRuntime {
                formation: roster.formation,
                members,
                leader_slot: roster.leader_slot,
                anchor: AnchorMode::Leader,
                last_tick: None,
                assigned_target: None,
                assigned_target_alive: true,
                route_available: true,
                latched: BTreeSet::new(),
            },
        );
        Ok(())
    }

    /// Applies one tick of formation facts.
    ///
    /// `policies` is the formation's declared recovery set, owned by the
    /// planner; the coordinator keeps no second copy of it, so the declared
    /// path and the applied path cannot drift apart.
    ///
    /// # Errors
    ///
    /// [`CombatError::UnknownFormation`] for a formation the coordinator does
    /// not hold (including one it has already dissolved),
    /// [`CombatError::StaleFormationTick`] for a tick that is not newer than
    /// the last applied one, [`CombatError::ForeignSession`] for a member from
    /// another generation, the report-shape errors
    /// ([`CombatError::FormationMemberReportedTwice`],
    /// [`CombatError::UnknownFormationSlot`],
    /// [`CombatError::FormationMemberMismatch`],
    /// [`CombatError::RetiredFormationMember`],
    /// [`CombatError::FormationMemberNotReported`]),
    /// [`CombatError::StationNotFinite`] when a station cannot be computed as a
    /// finite point, and [`CombatError::NoSurvivingMember`] when a declared
    /// recovery asks for a point that no living member can supply. Every one
    /// leaves the coordinator unchanged.
    pub fn apply(
        &mut self,
        policies: RecoveryPolicySet,
        tick: &FormationTick<'_>,
    ) -> Result<FormationUpdate, CombatError> {
        let formation = tick.formation;
        let current = self
            .formations
            .get(&formation)
            .ok_or(CombatError::UnknownFormation { formation })?;
        if let Some(last) = current.last_tick
            && tick.now <= last
        {
            return Err(CombatError::StaleFormationTick {
                formation,
                now: tick.now,
                last,
            });
        }
        for report in tick.members {
            if report.actor.session.get() != self.session {
                return Err(CombatError::ForeignSession {
                    actor: report.actor,
                    session: self.session,
                });
            }
        }
        if let Some(target) = tick.assigned_target
            && target.session.get() != self.session
        {
            return Err(CombatError::ForeignSession {
                actor: target,
                session: self.session,
            });
        }

        // Work on a copy: nothing below commits until the whole tick is good.
        let previous_leader = current.leader();
        let mut next = current.clone();
        next.assigned_target = tick.assigned_target;
        next.assigned_target_alive = tick.assigned_target_alive;
        next.route_available = tick.route_available;
        next.reconcile(tick)?;
        next.release_latches();

        let pending = next.pending_trigger();
        // Nothing survived: there is no shape left to recover, so the tick
        // ends in dissolution rather than electing a leader out of the dead.
        let nothing_survives = next.survivor_count() == 0;
        let mut trigger = None;
        let mut action = None;
        if !nothing_survives
            && let Some(pending) = pending
            && !next.latched.contains(&pending)
        {
            let declared = policies.action(pending);
            next.apply_action(pending, declared)?;
            trigger = Some(pending);
            action = Some(declared);
        }

        // "Leave the engagement" has no geometry either: the stations are
        // released and the formation is torn down with them.
        let dissolved = nothing_survives || action == Some(RecoveryAction::Withdraw);
        let released = if dissolved {
            next.members
                .values()
                .filter(|member| member.alive)
                .map(|member| member.actor)
                .collect()
        } else {
            Vec::new()
        };
        let stations = if dissolved {
            Vec::new()
        } else {
            next.stations(formation)?
        };
        let leader = next.leader();
        let latched = pending.filter(|pending| trigger != Some(*pending));

        if dissolved {
            self.formations.remove(&formation);
        } else {
            next.last_tick = Some(tick.now);
            self.formations.insert(formation, next);
        }
        Ok(FormationUpdate {
            formation,
            now: tick.now,
            previous_leader,
            leader,
            trigger,
            action,
            stations,
            released,
            latched,
            dissolved,
        })
    }

    /// Tears a formation down, dropping its membership and leadership.
    ///
    /// This is the teardown half of the recovery contract: a dissolved
    /// formation is not resurrected by a later report, and
    /// [`CombatError::UnknownFormation`] is what a caller sees instead of
    /// state from a mission that has moved on.
    ///
    /// # Errors
    ///
    /// [`CombatError::UnknownFormation`] when the coordinator does not hold
    /// the formation.
    pub fn dissolve(&mut self, formation: FormationId) -> Result<(), CombatError> {
        self.formations
            .remove(&formation)
            .map(|_| ())
            .ok_or(CombatError::UnknownFormation { formation })
    }

    /// The facts one actor's decision is made from, as this coordinator's state
    /// currently sees them.
    ///
    /// `None` when the coordinator holds no such formation, or when the
    /// observer is not one of its living members: a retired member has no
    /// formation facts, so a stale request cannot resurrect its slot.
    #[must_use]
    pub fn facts(&self, formation: FormationId, observer: ActorId) -> Option<FormationFacts> {
        let state = self.formations.get(&formation)?;
        let living = state
            .members
            .values()
            .any(|member| member.alive && member.actor == observer);
        if !living {
            return None;
        }
        let leader = state.leader();
        Some(FormationFacts {
            formation,
            leader: state.registered_leader()?,
            leader_alive: leader.is_some(),
            observer_is_leader: leader == Some(observer),
            assigned_target: state.assigned_target,
            assigned_target_alive: state.assigned_target_alive,
            route_available: state.route_available,
        })
    }

    /// The tick the coordinator last applied for a formation.
    #[must_use]
    pub fn last_tick(&self, formation: FormationId) -> Option<Tick> {
        self.formations
            .get(&formation)
            .and_then(|state| state.last_tick)
    }
}

// -------------------------------------------------------- runtime ----

/// Where the effective profile one decision ran under came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileSource {
    /// The role's profile at the selected tier, with no ace variant.
    Role {
        /// The tier the profile was selected from.
        tier: DifficultyTier,
    },
    /// An ace variant's lowered profile, at the tier it was lowered for.
    Ace {
        /// The ace's identity.
        id: AceId,
        /// The tier the profile was lowered for.
        tier: DifficultyTier,
    },
}

/// One actor's combat tick, as the mission runtime reports it.
///
/// The formation is named, not supplied: [`CombatRuntime`] builds the
/// [`FormationFacts`] from its own coordinator, so a decision can never be
/// made against facts the coordinator does not believe.
#[derive(Debug)]
pub struct CombatantRequest<'a> {
    /// The deciding actor.
    pub observer: ActorId,
    /// The tick the decision is made on.
    pub now: Tick,
    /// The observer's canonical world position.
    pub observer_position: WorldPosition,
    /// The observer's script-assigned role.
    pub assignment: &'a RoleAssignment,
    /// The formation the observer belongs to, when it belongs to one.
    pub formation: Option<FormationId>,
    /// Whether the observer's protected actor is still alive. `None` means the
    /// caller did not report it, which is not the statement "destroyed".
    pub protected_alive: Option<bool>,
    /// The candidates the approved perception model reported.
    pub candidates: &'a [CandidateView],
    /// The observer's weapons and launchers, when the caller has a snapshot.
    pub arsenal: Option<&'a ArsenalSnapshot>,
    /// The ace variant the mission assigned this actor, when it authored one.
    pub ace: Option<&'a AceId>,
    /// The difficulty tier the mission selected.
    pub tier: DifficultyTier,
}

/// One actor's decision plus the record of what it ran under.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatStep {
    /// The decision the planner made.
    pub decision: CombatDecision,
    /// Where the effective profile came from.
    pub source: ProfileSource,
    /// The formation facts the decision was made against.
    pub formation: Option<FormationFacts>,
}

impl CombatStep {
    /// The selected target, if any.
    #[must_use]
    pub fn target(&self) -> Option<ActorId> {
        self.decision.target
    }

    /// The trace that explains the decision.
    #[must_use]
    pub fn trace(&self) -> &DecisionTrace {
        &self.decision.trace
    }
}

/// The per-session combat-AI runtime: the planner, the ace variants, the
/// difficulty tiers and the formation coordinator, wired together.
///
/// One instance per session generation, like
/// [`crate::damage::DamageResolver`],
/// [`crate::targeting::TargetStore`] and [`crate::allies::AlliesRoster`]. Every
/// actor identity it is asked about must belong to its own session.
///
/// # The order a tick runs in
///
/// 1. [`CombatRuntime::update_formation`] once per formation, with the facts
///    the producers report. This *applies* any declared recovery.
/// 2. [`CombatRuntime::step`] once per AI actor, in any order; it is a pure
///    function of the request and the runtime's immutable policy, so one
///    actor's decision cannot depend on another's.
///
/// Stepping before updating a formation is not an error — it reports the
/// recovery that is pending, which is what F32-A's trace is for — it simply
/// decides against the pre-recovery leadership.
#[derive(Clone, Debug, PartialEq)]
pub struct CombatRuntime {
    session: u64,
    planner: CombatPlanner,
    aces: BTreeMap<AceId, AceVariant>,
    difficulty: DifficultyRoster,
    formations: FormationCoordinator,
}

impl CombatRuntime {
    /// Builds a runtime for one session generation.
    ///
    /// # Errors
    ///
    /// [`CombatError::DuplicateRoleProfile`] and
    /// [`CombatError::DuplicateAce`] for a repeated role or ace id, and the
    /// validation errors of [`CombatPlanner::new`].
    pub fn new(
        session: u64,
        profiles: &[SkillProfile],
        aces: Vec<AceVariant>,
        difficulty: DifficultyRoster,
    ) -> Result<Self, CombatError> {
        let mut declared = BTreeMap::new();
        for variant in aces {
            let id = variant.id().clone();
            if declared.insert(id.clone(), variant).is_some() {
                return Err(CombatError::DuplicateAce { id });
            }
        }
        Ok(Self {
            session,
            planner: CombatPlanner::new(session, profiles)?,
            aces: declared,
            difficulty,
            formations: FormationCoordinator::new(session),
        })
    }

    /// The session generation this runtime owns.
    #[must_use]
    pub const fn session(&self) -> u64 {
        self.session
    }

    /// Registers one formation's declared recovery paths *and* its membership.
    ///
    /// One call registers both halves, so a formation can never exist as
    /// declared policy without runtime state to apply it to.
    ///
    /// # Errors
    ///
    /// [`CombatError::DuplicateFormation`] when the formation is registered
    /// twice, [`CombatError::ForeignSession`] for a member of another
    /// generation, and the validation errors of [`FormationRoster::try_new`].
    pub fn with_formation(
        mut self,
        roster: FormationRoster,
        policies: RecoveryPolicySet,
    ) -> Result<Self, CombatError> {
        self.formations.register(&roster)?;
        self.planner = self.planner.with_formation(roster.formation(), policies)?;
        Ok(self)
    }

    /// Applies one tick of formation facts and reports what the recovery did.
    ///
    /// # Errors
    ///
    /// [`CombatError::NoRecoveryPolicy`] for a formation that was never
    /// registered with [`CombatRuntime::with_formation`], plus every error
    /// [`FormationCoordinator::apply`] reports. The runtime's state is
    /// unchanged in each case.
    pub fn update_formation(
        &mut self,
        tick: &FormationTick<'_>,
    ) -> Result<FormationUpdate, CombatError> {
        let policies = self.planner.recovery_policies(tick.formation).ok_or(
            CombatError::NoRecoveryPolicy {
                formation: tick.formation,
            },
        )?;
        self.formations.apply(policies, tick)
    }

    /// Tears one formation down.
    ///
    /// # Errors
    ///
    /// [`CombatError::UnknownFormation`] when the runtime holds no such
    /// formation.
    pub fn dissolve_formation(&mut self, formation: FormationId) -> Result<(), CombatError> {
        self.formations.dissolve(formation)
    }

    /// The formation facts one actor's decision would be made from, as this
    /// runtime's coordinator currently sees them.
    #[must_use]
    pub fn facts(&self, formation: FormationId, observer: ActorId) -> Option<FormationFacts> {
        self.formations.facts(formation, observer)
    }

    /// The formations the runtime still holds, in ascending order.
    pub fn formations(&self) -> impl Iterator<Item = FormationId> + '_ {
        self.formations.formations()
    }

    /// The difficulty roster.
    #[must_use]
    pub const fn difficulty(&self) -> &DifficultyRoster {
        &self.difficulty
    }

    /// The ace variants, in ascending id order.
    pub fn aces(&self) -> impl Iterator<Item = (&AceId, &AceVariant)> {
        self.aces.iter()
    }

    /// The immutable planner this runtime decides through.
    #[must_use]
    pub const fn planner(&self) -> &CombatPlanner {
        &self.planner
    }

    /// Resolves the effective profile one actor runs under, and records where
    /// it came from.
    ///
    /// The selection is total and checkable: the tier must be declared in the
    /// roster, an ace must exist, must modify the actor's assigned role and
    /// must have been lowered for the selected tier. Anything else is refused
    /// by name. Composing a tier onto a profile is *not* done here — that
    /// belongs to the declared→runtime lowering boundary (#551), and doing it
    /// in two places is how a difficulty tier ends up meaning two things.
    ///
    /// # Errors
    ///
    /// [`CombatError::UnknownDifficultyTier`],
    /// [`CombatError::DifficultyTierMissingRole`], [`CombatError::UnknownAce`],
    /// [`CombatError::AceAssignmentMismatch`] and
    /// [`CombatError::AceTierMismatch`].
    pub fn resolve_profile(
        &self,
        assignment: &RoleAssignment,
        ace: Option<&AceId>,
        tier: DifficultyTier,
    ) -> Result<(SkillProfile, ProfileSource), CombatError> {
        // The tier must be declared even when an ace overrides the role's
        // profile: the mission's difficulty choice is explicit, and a tier the
        // content never described is a content bug, not a reason to run at the
        // baseline.
        let role_profile = self.difficulty.profile(tier, assignment.role())?;
        match ace {
            None => Ok((role_profile, ProfileSource::Role { tier })),
            Some(id) => {
                let variant = self
                    .aces
                    .get(id)
                    .ok_or_else(|| CombatError::UnknownAce { id: id.clone() })?;
                if variant.base_role() != assignment.role() {
                    return Err(CombatError::AceAssignmentMismatch {
                        id: id.clone(),
                        base: variant.base_role(),
                        assigned: assignment.role(),
                    });
                }
                if variant.tier() != tier {
                    return Err(CombatError::AceTierMismatch {
                        id: id.clone(),
                        declared: variant.tier(),
                        selected: tier,
                    });
                }
                Ok((
                    *variant.profile(),
                    ProfileSource::Ace {
                        id: id.clone(),
                        tier,
                    },
                ))
            }
        }
    }

    /// Decides one actor for one tick under the selected tier and ace variant.
    ///
    /// # Errors
    ///
    /// Every error [`CombatRuntime::resolve_profile`] reports, plus
    /// [`CombatError::UnknownFormationMember`] when the request names a
    /// formation the observer is not a living member of, and every error
    /// [`CombatPlanner::decide`] reports. Nothing is mutated, so a refused
    /// step can be retried unchanged.
    pub fn step(&self, request: &CombatantRequest<'_>) -> Result<CombatStep, CombatError> {
        let facts = match request.formation {
            None => None,
            Some(formation) => Some(self.formations.facts(formation, request.observer).ok_or(
                CombatError::UnknownFormationMember {
                    formation,
                    observer: request.observer,
                },
            )?),
        };
        let (profile, source) =
            self.resolve_profile(request.assignment, request.ace, request.tier)?;
        let decision = self.planner.decide(&CombatRequest {
            observer: request.observer,
            now: request.now,
            observer_position: request.observer_position,
            assignment: request.assignment,
            formation: facts.as_ref(),
            protected_alive: request.protected_alive,
            candidates: request.candidates,
            arsenal: request.arsenal,
            profile: Some(&profile),
        })?;
        Ok(CombatStep {
            decision,
            source,
            formation: facts,
        })
    }
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
    /// An ace variant's id is not in the `pilot` namespace.
    AceKindMismatch {
        /// The offending catalog id.
        id: ContentId,
    },
    /// An ace variant is registered more than once.
    DuplicateAce {
        /// The repeated variant.
        id: AceId,
    },
    /// The mission named an ace variant the runtime does not carry. An
    /// unknown pilot's behavior is refused rather than replaced by the role's
    /// plain profile, which would make a missing content record look like a
    /// working one.
    UnknownAce {
        /// The ace the request named.
        id: AceId,
    },
    /// An ace variant modifies a role the actor is not assigned: an ace of
    /// the fighter role cannot defend a charge as an escort.
    AceRoleMismatch {
        /// The ace the request named.
        id: AceId,
        /// The role the ace variant modifies.
        base: CombatRole,
        /// The role the profile it carries belongs to.
        profile: CombatRole,
    },
    /// The mission named an ace variant whose role does not match the
    /// observer's assignment.
    AceAssignmentMismatch {
        /// The ace the request named.
        id: AceId,
        /// The role the ace variant modifies.
        base: CombatRole,
        /// The role the actor is assigned.
        assigned: CombatRole,
    },
    /// An ace variant was lowered for one difficulty tier and the mission
    /// selected another. Mixing the two would make the tier mean one thing for
    /// an ace and another for the rest of the squadron.
    AceTierMismatch {
        /// The ace the request named.
        id: AceId,
        /// The tier the variant's profile was lowered for.
        declared: DifficultyTier,
        /// The tier the mission selected.
        selected: DifficultyTier,
    },
    /// A difficulty tier is declared more than once.
    DuplicateDifficultyTier {
        /// The repeated tier.
        tier: DifficultyTier,
    },
    /// The mission selected a difficulty tier the content never declared.
    /// There is no baseline fallback: choosing difficulty is an explicit
    /// mission decision, and defaulting it would hide a content bug.
    UnknownDifficultyTier {
        /// The undeclared tier.
        tier: DifficultyTier,
    },
    /// A declared difficulty tier says nothing about a role the mission
    /// assigned, so there is no behavior profile to run it under.
    DifficultyTierMissingRole {
        /// The tier.
        tier: DifficultyTier,
        /// The role it does not describe.
        role: CombatRole,
    },
    /// A formation with no member was declared. An empty formation has no
    /// leader to promote and no survivors to regroup on.
    EmptyFormation {
        /// The formation.
        formation: FormationId,
    },
    /// A formation's leading slot is not one of its members, so it cannot
    /// elect a successor when its leader is lost.
    FormationLeaderNotAMember {
        /// The formation.
        formation: FormationId,
        /// The slot that leads it.
        leader_slot: u32,
    },
    /// Two declared members share one slot.
    FormationSlotOccupiedTwice {
        /// The formation.
        formation: FormationId,
        /// The repeated slot.
        slot: u32,
    },
    /// A report names a slot the formation does not have.
    UnknownFormationSlot {
        /// The formation.
        formation: FormationId,
        /// The reported slot.
        slot: u32,
    },
    /// A report names a different actor than the one registered in that slot.
    FormationMemberMismatch {
        /// The formation.
        formation: FormationId,
        /// The slot.
        slot: u32,
        /// The actor the runtime has in the slot.
        registered: ActorId,
        /// The actor the report named.
        reported: ActorId,
    },
    /// A report named the same slot twice, so its membership would be
    /// ambiguous.
    FormationMemberReportedTwice {
        /// The formation.
        formation: FormationId,
        /// The repeated slot.
        slot: u32,
    },
    /// A report omitted a registered member. A centroid over a partial
    /// membership is a different point, so a partial report is refused rather
    /// than reconciled.
    FormationMemberNotReported {
        /// The formation.
        formation: FormationId,
        /// The slot that went unreported.
        slot: u32,
    },
    /// A report brought a retired member back to life. A destroyed member
    /// leaves the formation for good; a stale report cannot resurrect it.
    RetiredFormationMember {
        /// The formation.
        formation: FormationId,
        /// The slot.
        slot: u32,
        /// The retired actor.
        actor: ActorId,
    },
    /// A formation tick is not newer than the last one applied. Replaying an
    /// old tick would let a destroyed leader come back and would let a second
    /// tick elect a second leader.
    StaleFormationTick {
        /// The formation.
        formation: FormationId,
        /// The tick the report claims.
        now: Tick,
        /// The last tick the coordinator applied.
        last: Tick,
    },
    /// The formation is not held: it was never registered, or it has already
    /// dissolved. A dissolved formation is not resurrected by a later report.
    UnknownFormation {
        /// The formation.
        formation: FormationId,
    },
    /// A decision named a formation the observer is not a living member of,
    /// so the runtime cannot produce facts for it.
    UnknownFormationMember {
        /// The formation.
        formation: FormationId,
        /// The observer.
        observer: ActorId,
    },
    /// A declared recovery asked for a regroup point that no living member can
    /// supply, or a tick left nothing alive to recover.
    NoSurvivingMember {
        /// The formation.
        formation: FormationId,
    },
    /// A station could not be computed as a finite world point. Coordinates
    /// large enough to overflow `f64` are a producer bug, and an infinite
    /// station is refused by name rather than handed to navigation.
    StationNotFinite {
        /// The formation.
        formation: FormationId,
    },
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
            Self::AceKindMismatch { id } => {
                write!(f, "ace variant id {id} is not in the pilot namespace")
            }
            Self::DuplicateAce { id } => write!(f, "ace variant {id} is declared more than once"),
            Self::UnknownAce { id } => write!(f, "no ace variant {id} is registered"),
            Self::AceRoleMismatch { id, base, profile } => write!(
                f,
                "ace variant {id} modifies role {base} but carries a {profile} profile"
            ),
            Self::AceAssignmentMismatch { id, base, assigned } => write!(
                f,
                "ace variant {id} modifies role {base}, but the actor is assigned {assigned}"
            ),
            Self::AceTierMismatch {
                id,
                declared,
                selected,
            } => write!(
                f,
                "ace variant {id} was lowered for the {declared} tier, not {selected}"
            ),
            Self::DuplicateDifficultyTier { tier } => {
                write!(f, "difficulty tier {tier} is declared more than once")
            }
            Self::UnknownDifficultyTier { tier } => {
                write!(f, "no difficulty tier {tier} is declared")
            }
            Self::DifficultyTierMissingRole { tier, role } => write!(
                f,
                "the {tier} difficulty tier declares no behavior profile for role {role}"
            ),
            Self::EmptyFormation { formation } => {
                write!(f, "{formation} is declared with no member")
            }
            Self::FormationLeaderNotAMember {
                formation,
                leader_slot,
            } => write!(
                f,
                "{formation} is led by slot {leader_slot}, which is not one of its members"
            ),
            Self::FormationSlotOccupiedTwice { formation, slot } => {
                write!(f, "{formation} has more than one member in slot {slot}")
            }
            Self::UnknownFormationSlot { formation, slot } => {
                write!(f, "{formation} has no slot {slot}")
            }
            Self::FormationMemberMismatch {
                formation,
                slot,
                registered,
                reported,
            } => write!(
                f,
                "{formation} slot {slot} holds {registered}, but the report named {reported}"
            ),
            Self::FormationMemberReportedTwice { formation, slot } => {
                write!(
                    f,
                    "the report for {formation} names slot {slot} more than once"
                )
            }
            Self::FormationMemberNotReported { formation, slot } => write!(
                f,
                "the report for {formation} leaves slot {slot} unreported"
            ),
            Self::RetiredFormationMember {
                formation,
                slot,
                actor,
            } => write!(
                f,
                "the report for {formation} brings retired member {actor} in slot {slot} back to life"
            ),
            Self::StaleFormationTick {
                formation,
                now,
                last,
            } => write!(
                f,
                "tick {} is not newer than the tick {} already applied to {formation}",
                now.0, last.0
            ),
            Self::UnknownFormation { formation } => {
                write!(f, "{formation} is not held by this runtime")
            }
            Self::UnknownFormationMember {
                formation,
                observer,
            } => write!(f, "{observer} is not a living member of {formation}"),
            Self::NoSurvivingMember { formation } => {
                write!(f, "{formation} has no living member left")
            }
            Self::StationNotFinite { formation } => write!(
                f,
                "a station for {formation} is not a finite world position"
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

/// [`SYNTHETIC_SESSION`] as the shared nonzero session type.
pub const SYNTHETIC_SESSION_ID: SessionId = match SessionId::new(SYNTHETIC_SESSION) {
    Some(id) => id,
    None => unreachable!(),
};

/// An actor of the synthetic session.
#[must_use]
pub const fn synthetic_actor(serial: u64) -> ActorId {
    ActorId {
        session: SYNTHETIC_SESSION_ID,
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

/// The synthetic bomber profile: guns and ordnance, a 20-tick reaction, a
/// 0.05 rad aim error, an 800 m engagement range and a policy that prefers
/// the script-assigned objective (3.0) over proximity (1.0).
///
/// The ordnance-carrying role AC02's firing solution is checked against: a
/// bomber is the simplest role that may fire both a gun and a rocket rack,
/// so a disabled gun and an empty rack are both its business.
#[must_use]
pub fn synthetic_bomber_profile() -> SkillProfile {
    SkillProfile::try_new(
        CombatRole::BomberRun,
        RoleArsenal::guns_and_ordnance(),
        SkillKnobs {
            reaction_ticks: 20,
            aim_error_rad: 0.05,
            engagement_range_m: 800.0,
            fire_discipline_ticks: 24,
        },
        PriorityPolicy {
            protected_actor_weight: 0.0,
            objective_weight: 3.0,
            self_defense_weight: 1.0,
            proximity_weight: 1.0,
            threat_window_ticks: 90,
        },
    )
    .expect("the synthetic bomber profile is valid")
}

/// The synthetic ace variant of the bomber profile: a 6-tick reaction, a
/// 0.015 rad aim error and a doubled objective weight, the same behavior
/// overrides the escort ace carries.
///
/// It still declares guns and ordnance, so it is the shooter AC02 names: an
/// ace that cannot fire a disabled gun or an empty rocket rack. It has no
/// damage, armor or health field — the ace is *skill*, not durability.
#[must_use]
pub fn synthetic_ace_bomber_profile() -> SkillProfile {
    synthetic_bomber_profile()
        .with_knobs(SkillKnobs {
            reaction_ticks: 6,
            aim_error_rad: 0.015,
            ..synthetic_bomber_profile().knobs()
        })
        .and_then(|profile| {
            let priority = profile.priority();
            profile.with_priority(PriorityPolicy {
                objective_weight: 6.0,
                ..priority
            })
        })
        .expect("the synthetic ace bomber profile is valid")
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
            session: SYNTHETIC_SESSION_ID,
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

/// The synthetic ace identity: the same `pilot` id the declared fixture's ace
/// variant carries (`cs_content::ai::declared_synthetic_ace_profile`), so the
/// producer record and the runtime record name one pilot.
#[must_use]
pub fn synthetic_ace_id() -> AceId {
    AceId::try_new(
        ContentId::from_source(ContentKind::Pilot, "synthetic.ace-wing-leader")
            .expect("the fixture ace id is valid"),
    )
    .expect("the fixture ace id is in the pilot namespace")
}

/// The synthetic ace variant: the escort role's ace behavior, lowered for the
/// [`DifficultyTier::Standard`] tier — a 6-tick reaction and a 0.015 rad aim
/// error.
///
/// Same three behavior overrides as
/// `cs_content::ai::declared_synthetic_ace_profile`, presented as the profile
/// the boundary produced. It has no damage, armor, health or rate field, so it
/// cannot be an inflated-health ace or a faster simulation.
#[must_use]
pub fn synthetic_ace_variant() -> AceVariant {
    AceVariant::try_new(
        synthetic_ace_id(),
        CombatRole::Escort,
        DifficultyTier::Standard,
        synthetic_ace_profile(),
    )
    .expect("the synthetic ace variant is valid")
}

/// The escort profile at one tier: the declared behavior knobs scaled across
/// the four designed tiers.
///
/// A designed fixture, not original data: the original's difficulty option is
/// unmeasured (F32-D). Only the four [`SkillKnobs`] and the protected-actor
/// weight move with the tier; the engagement range, the threat window, the
/// cadence and the role's [`RoleArsenal`] are identical at every tier, which
/// is what keeps non-negotiable 1 and 2 checkable.
fn escort_profile_at(tier: DifficultyTier) -> SkillProfile {
    let (reaction, aim_error, protected_weight) = match tier {
        DifficultyTier::Relaxed => (48, 0.12, 1.0),
        DifficultyTier::Standard => (24, 0.06, 2.0),
        DifficultyTier::Hard => (12, 0.03, 3.0),
        DifficultyTier::Elite => (6, 0.015, 4.0),
    };
    let base = synthetic_escort_profile();
    base.with_knobs(SkillKnobs {
        reaction_ticks: reaction,
        aim_error_rad: aim_error,
        ..base.knobs()
    })
    .and_then(|profile| {
        let priority = profile.priority();
        profile.with_priority(PriorityPolicy {
            protected_actor_weight: protected_weight,
            ..priority
        })
    })
    .expect("the tier escort profile is valid")
}

/// The synthetic difficulty roster: all four designed tiers, each declaring
/// the escort and fighter roles.
///
/// Every tier is declared explicitly, including the baseline, so a tier is
/// never resolved by a silent fallback.
#[must_use]
pub fn synthetic_difficulty_roster() -> DifficultyRoster {
    let fighter_reaction = |tier: DifficultyTier| -> u64 {
        match tier {
            DifficultyTier::Relaxed => 36,
            DifficultyTier::Standard => 18,
            DifficultyTier::Hard => 12,
            DifficultyTier::Elite => 6,
        }
    };
    let mut roster = DifficultyRoster::new();
    for tier in DifficultyTier::ALL {
        let fighter = SkillProfile::try_new(
            CombatRole::FighterAttack,
            synthetic_fighter_profile().arsenal(),
            SkillKnobs {
                reaction_ticks: fighter_reaction(*tier),
                ..synthetic_fighter_profile().knobs()
            },
            synthetic_fighter_profile().priority(),
        )
        .expect("the tier fighter profile is valid");
        roster = roster
            .with_tier(*tier, &[escort_profile_at(*tier), fighter])
            .expect("the synthetic difficulty roster is valid");
    }
    roster
}

/// The synthetic formation's leader.
pub const SYNTHETIC_FORMATION_LEADER: u64 = 10;

/// The synthetic formation's two followers.
pub const SYNTHETIC_FORMATION_WINGMEN: [u64; 2] = [11, 12];

/// The synthetic formation's roster: slot 0 leads, slots 1 and 2 follow.
///
/// The same membership `cs_content::ai::declared_synthetic_formation` declares
/// (a leader plus two followers), with the runtime's session-qualified actor
/// identities.
#[must_use]
pub fn synthetic_formation_roster() -> FormationRoster {
    FormationRoster::try_new(
        FormationId(1),
        0,
        vec![
            FormationRosterMember {
                slot: 0,
                actor: synthetic_actor(SYNTHETIC_FORMATION_LEADER),
            },
            FormationRosterMember {
                slot: 1,
                actor: synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[0]),
            },
            FormationRosterMember {
                slot: 2,
                actor: synthetic_actor(SYNTHETIC_FORMATION_WINGMEN[1]),
            },
        ],
    )
    .expect("the synthetic formation roster is valid")
}

/// One member report for the synthetic formation.
#[must_use]
pub fn synthetic_member_report(
    slot: u32,
    serial: u64,
    position_m: [f64; 3],
    alive: bool,
) -> FormationMemberReport {
    FormationMemberReport {
        slot,
        actor: synthetic_actor(serial),
        position: WorldPosition::try_new(position_m).expect("the fixture position is finite"),
        alive,
    }
}

/// The synthetic combat runtime: the synthetic session's escort and fighter
/// roles, one ace variant, four difficulty tiers and formation 1 with its
/// declared recovery paths.
#[must_use]
pub fn synthetic_combat_runtime() -> CombatRuntime {
    CombatRuntime::new(
        SYNTHETIC_SESSION,
        &[synthetic_escort_profile(), synthetic_fighter_profile()],
        vec![synthetic_ace_variant()],
        synthetic_difficulty_roster(),
    )
    .expect("the synthetic combat runtime is valid")
    .with_formation(synthetic_formation_roster(), synthetic_recovery_policies())
    .expect("the synthetic formation registers")
}
