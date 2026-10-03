//! The declared objective-program schema: provenance-carrying mission
//! objectives, count conditions, timers, trigger volumes, spawn groups and
//! dialogue cues (F39-C).
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
//! stage `### F39-C`. Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
//!
//! This module is the **content half** of the objective-program contract — the
//! normalized record a mission importer produces and the session wiring
//! consumes. Its runtime counterpart is `cs_sim::objectives` (the continuous
//! `ObjectiveRuntime` F39-B built); the lowering boundary is
//! `cs_app::objectives`. This crate cannot depend on `cs_sim` or `cs_script`,
//! so the declared record keeps its own typed vocabulary — program symbols,
//! program actors, objective states, reveal rules, count kinds, timer starts
//! and actions, terminal outcomes and precedence — and the boundary maps it
//! field-wise into `SymbolId`/`ActorId`/`ObjectiveSpec`/`MissionTimer`/
//! `SweptTrigger`/`CountCondition`.
//!
//! # Records
//!
//! A [`DeclaredObjectiveProgram`] names its `subject` — the `mission` catalog
//! id whose program this is — carries an [`Origin`], a [`Provenance`], the
//! declared terminal [`DeclaredPrecedence`] (a [`Resolved`], because the
//! original precedence is unmeasured and a missing rule stays an explicit
//! unknown rather than a silent default), and five authored lists:
//!
//! * [`DeclaredObjective`]s — the player-visible objectives, each with its
//!   initial state, the [`DeclaredRevealRule`] gating when it may be shown
//!   (F39 non-negotiable behavior 5) and what completing it requests;
//! * [`DeclaredCondition`]s — the count conditions, each an explicit roster
//!   plus *one* [`DeclaredCountKind`] plus a required count plus the
//!   [`DeclaredCountReaction`] satisfying it performs — never an
//!   `enemy_alive == 0` approximation (F39 non-negotiable behavior 2);
//! * [`DeclaredTimer`]s — the deadlines, each with a declared
//!   [`DeclaredTimerStart`], a declared [`DeclaredTimeDomain`], a whole-tick
//!   period and exactly one [`DeclaredTimerAction`] on expiry (F39
//!   non-negotiable behavior 3);
//! * [`DeclaredTrigger`]s — the swept volumes, each bound to one program
//!   actor;
//! * [`DeclaredSpawnGroup`]s — the bindings that give a spawn-group symbol
//!   its subject content, so an admitted wave names what to instantiate.
//!
//! # What the schema validates
//!
//! [`DeclaredObjectiveProgram::try_new`] refuses what a mission program cannot
//! mean: a subject that is not a mission, the reserved actor-event symbol, a
//! duplicate declaration inside one kind, a hidden objective declared
//! `Immediate`, an objective content id that is not an `objective`, an empty
//! roster or a zero (or unreachable) required count, a zero period, an empty
//! emission key, a zero-count wave, a non-finite or inverted volume — and
//! every *dangling declaration reference*: a reveal rule, timer start, timer
//! action or count reaction naming an objective, condition, timer or spawn
//! group the same program does not declare. The runtime catches a dangling
//! name when it is *used*; the schema catches it when it is *declared*, so a
//! mission whose deadline or wave never existed cannot reach a session
//! looking like one that simply never came due. Signal symbols are the one
//! open set: a program may raise a signal this record does not declare, so
//! `OnSignal`/`Signal` references are never validated.
//!
//! # Designed vocabulary, not original data
//!
//! No original mission program has been decoded into this form (F38 owns the
//! original mission language; F39-D calibrates the rules with `retail`).
//! Every kind, rule name and fixture value here is newly authored project
//! design carrying `Origin::SyntheticFixture`/designed provenance.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

/// The symbol a mission program declares for an objective, condition, timer,
/// trigger, spawn group or signal.
///
/// This is the declared-form identity; `cs_app::objectives::lower_program`
/// maps it one-to-one onto `cs_script::ir::SymbolId`. Symbol `0` is reserved
/// by the runtime for actor-keyed events and refused by
/// [`DeclaredObjectiveProgram::try_new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramSymbol(pub u32);

impl fmt::Display for ProgramSymbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "symbol({})", self.0)
    }
}

/// The program-level identity of a mission actor a count roster or a watched
/// trigger names. Mapped one-to-one onto `cs_script::ir::ActorId` at the
/// lowering boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProgramActor(pub u32);

impl fmt::Display for ProgramActor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "actor({})", self.0)
    }
}

/// The seven objective states, as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclaredObjectiveState {
    /// Not shown; a reveal rule has not fired.
    Hidden,
    /// Shown, not yet pursued.
    Pending,
    /// Being pursued.
    Active,
    /// Completed (terminal).
    Succeeded,
    /// Failed (terminal).
    Failed,
    /// Optional; never gates mission success.
    Optional,
    /// Replaced by another objective (terminal).
    Superseded,
}

/// When a hidden objective may be shown.
///
/// Mirrors `cs_sim::objectives::runtime::RevealRule` with declared-side
/// references. `OnSignal` names a signal the program raises; every other
/// variant names a declaration of this program and is validated by
/// [`DeclaredObjectiveProgram::try_new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredRevealRule {
    /// Shown from the first tick.
    Immediate,
    /// Shown when a declared count condition first latches.
    OnCondition(ProgramSymbol),
    /// Shown when a declared timer runs out.
    OnTimer(ProgramSymbol),
    /// Shown when a named mission signal is raised.
    OnSignal(ProgramSymbol),
    /// Shown when a declared objective reaches a declared state.
    OnObjectiveState {
        /// The objective watched.
        objective: ProgramSymbol,
        /// The state that reveals.
        state: DeclaredObjectiveState,
    },
}

/// How the mission ends, as declared. Distinct endings, never a flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclaredTerminalOutcome {
    /// The declared success.
    Success,
    /// Ended by leaving the theatre with what the mission was for.
    Extraction,
    /// The declared failure.
    Failure,
}

/// What completing a declared objective means for the mission.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeclaredCompletion {
    /// The objective latches; the mission carries on.
    #[default]
    Continue,
    /// Completion requests this terminal outcome.
    Requests(DeclaredTerminalOutcome),
}

/// The declared precedence resolving two terminal requests on one tick.
///
/// `SyntheticConservative` is the designed policy F39-B names: `Failure`
/// beats `Extraction` beats `Success`. The original game's rule is
/// unmeasured; a measured rule becomes a new variant, and a program whose
/// precedence is unrecovered carries [`Resolved::Unknown`] so the lowering
/// refuses it by name instead of picking one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredPrecedence {
    /// `Failure` > `Extraction` > `Success`. Designed; synthetic only.
    SyntheticConservative,
}

/// Why an actor stopped counting as present, as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeclaredCountKind {
    /// Destroyed.
    Destroyed,
    /// Disabled but not gone.
    Disabled,
    /// Captured by another faction.
    Captured,
    /// Escaped the theatre.
    Escaped,
    /// Removed from the world without being killed.
    Despawned,
}

/// What a satisfied count condition does, as declared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredCountReaction {
    /// Report the condition and nothing else.
    ReportOnly,
    /// Move a declared objective to a declared state.
    SetObjectiveState {
        /// The objective to move.
        objective: ProgramSymbol,
        /// The state to move it to.
        state: DeclaredObjectiveState,
    },
    /// Request a terminal outcome.
    Finish(DeclaredTerminalOutcome),
}

/// When a declared timer starts counting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredTimerStart {
    /// Armed only by an explicit program arm request.
    OnArm,
    /// Armed automatically at the first evaluated tick at or after this tick.
    AtTick(u64),
    /// Armed the first tick a named signal is raised on an earlier tick made
    /// eligible.
    OnSignal(ProgramSymbol),
    /// Armed the first tick a declared objective reaches a declared state.
    OnObjectiveState {
        /// The objective watched.
        objective: ProgramSymbol,
        /// The state that arms.
        state: DeclaredObjectiveState,
    },
    /// Declared never to run.
    Never,
}

/// The clock domain a declared timer runs on.
///
/// Mirrors `cs_sim::time::TimeDomain`. A mission deadline belongs to a
/// gameplay domain; the lowering refuses a `UiWall`/`MediaUnscaled` deadline
/// by name rather than letting a menu frame or a cutscene advance it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredTimeDomain {
    /// Authoritative simulation ticks.
    Simulation,
    /// UI wall time; keeps running while paused.
    UiWall,
    /// Unscaled media time.
    MediaUnscaled,
    /// The gameplay time a session is judged by.
    AuthoritativeGameplay,
}

/// What one timer does when it expires, as declared. A closed set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredTimerAction {
    /// Move a declared objective to a declared state: the only way a deadline
    /// unlocks, resets, succeeds or fails an objective.
    SetObjectiveState {
        /// The objective to move.
        objective: ProgramSymbol,
        /// The state to move it to.
        state: DeclaredObjectiveState,
    },
    /// Raise a named mission signal, eligible to arm other timers from the
    /// next tick.
    Signal(ProgramSymbol),
    /// Ask for a declared spawn group's wave, once per idempotency `key`.
    SpawnGroup {
        /// The authored idempotency key; a repeat is refused, so a wave is
        /// never spawned twice under one key.
        key: String,
        /// The declared spawn group the wave belongs to.
        group: ProgramSymbol,
        /// How many instances the wave asks for.
        count: u32,
    },
    /// Play one dialogue cue once per idempotency `key`.
    Cue {
        /// The authored idempotency key; a repeat is refused, so a radio
        /// line is never replayed under one key.
        key: String,
        /// The dialogue content to play (a [`ContentKind::Dialogue`]).
        dialogue: ContentId,
    },
    /// Grant an optional reward intent. Never terminal.
    GrantOptionalReward {
        /// The reward content.
        reward: ContentId,
    },
    /// Request the mission's terminal outcome.
    Finish(DeclaredTerminalOutcome),
}

/// A declared trigger volume, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclaredVolume {
    /// A sphere.
    Sphere {
        /// Center, meters.
        center_m: [f64; 3],
        /// Radius, meters.
        radius_m: f64,
    },
    /// An axis-aligned box.
    Aabb {
        /// Minimum corner, meters.
        min_m: [f64; 3],
        /// Maximum corner, meters.
        max_m: [f64; 3],
    },
}

impl DeclaredVolume {
    /// Whether every extent is finite and ordered.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        match *self {
            Self::Sphere { center_m, radius_m } => {
                center_m.iter().all(|v| v.is_finite()) && radius_m.is_finite() && radius_m >= 0.0
            }
            Self::Aabb { min_m, max_m } => {
                (0..3).all(|i| min_m[i].is_finite() && max_m[i].is_finite() && min_m[i] <= max_m[i])
            }
        }
    }
}

/// One declared objective.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredObjective {
    /// The objective's program symbol; also the source of its events.
    pub symbol: ProgramSymbol,
    /// The stable content id the objective was authored as.
    pub content: ContentId,
    /// Its state before anything happens.
    pub initial: DeclaredObjectiveState,
    /// When it may be shown.
    pub reveal: DeclaredRevealRule,
    /// What completing it requests.
    pub on_complete: DeclaredCompletion,
}

/// One declared count condition: a roster, one category, a required count
/// and the reaction satisfying it performs.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredCondition {
    /// The condition's program symbol; also the source of its events.
    pub symbol: ProgramSymbol,
    /// The one category that satisfies it.
    pub kind: DeclaredCountKind,
    /// The program actors that can satisfy it.
    pub roster: Vec<ProgramActor>,
    /// How many roster actors in `kind` satisfy it.
    pub required: usize,
    /// What satisfying it does.
    pub reaction: DeclaredCountReaction,
}

/// One declared mission timer.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredTimer {
    /// The timer's program symbol; also the source of its events.
    pub symbol: ProgramSymbol,
    /// The clock domain it counts on.
    pub domain: DeclaredTimeDomain,
    /// Its declared start condition.
    pub start: DeclaredTimerStart,
    /// Whole ticks from arm to expiry.
    pub period_ticks: u64,
    /// The single action it performs on expiry.
    pub action: DeclaredTimerAction,
}

/// One declared swept trigger: a volume bound to one program actor.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredTrigger {
    /// The trigger's program symbol; also the source of its events.
    pub symbol: ProgramSymbol,
    /// The program actor whose movement it sweeps.
    pub actor: ProgramActor,
    /// The volume.
    pub volume: DeclaredVolume,
}

/// One declared spawn group: the subject content a wave of this group
/// instantiates. The count is on the [`DeclaredTimerAction::SpawnGroup`] that
/// asks for the wave.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredSpawnGroup {
    /// The group's program symbol.
    pub symbol: ProgramSymbol,
    /// The content one instance of the wave is built from.
    pub subject: ContentId,
}

/// Why a [`DeclaredObjectiveProgram`] was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ObjectivesSchemaError {
    /// The subject was not a mission.
    SubjectKind {
        /// The offending id.
        id: ContentId,
    },
    /// A declaration used the reserved actor-event symbol `0`.
    ReservedSymbol {
        /// The symbol.
        symbol: ProgramSymbol,
    },
    /// Two objectives share one symbol.
    DuplicateObjective {
        /// The duplicated symbol.
        symbol: ProgramSymbol,
    },
    /// Two conditions share one symbol.
    DuplicateCondition {
        /// The duplicated symbol.
        symbol: ProgramSymbol,
    },
    /// Two timers share one symbol.
    DuplicateTimer {
        /// The duplicated symbol.
        symbol: ProgramSymbol,
    },
    /// The same `(trigger, actor)` pair is declared twice.
    DuplicateTrigger {
        /// The trigger symbol.
        trigger: ProgramSymbol,
        /// The watched actor.
        actor: ProgramActor,
    },
    /// Two spawn groups share one symbol.
    DuplicateSpawnGroup {
        /// The duplicated symbol.
        symbol: ProgramSymbol,
    },
    /// An objective declares itself hidden and shown from the first tick.
    HiddenButImmediate {
        /// The objective.
        objective: ProgramSymbol,
    },
    /// An objective's content id is not an `objective` kind.
    NotAnObjective {
        /// The objective.
        objective: ProgramSymbol,
        /// The offending id.
        id: ContentId,
    },
    /// A condition declares no actor, so nothing can satisfy it.
    EmptyRoster {
        /// The condition.
        condition: ProgramSymbol,
    },
    /// A condition requires zero actors, so it is met by construction.
    ZeroRequired {
        /// The condition.
        condition: ProgramSymbol,
    },
    /// A condition requires more roster actors than it declares, so it can
    /// never be met — a dead declaration, refused rather than kept.
    RequiredExceedsRoster {
        /// The condition.
        condition: ProgramSymbol,
        /// The declared required count.
        required: usize,
        /// The roster size it can never exceed.
        roster: usize,
    },
    /// A timer declares a zero-tick period.
    ZeroPeriod {
        /// The timer.
        timer: ProgramSymbol,
    },
    /// A spawn or cue action declares an empty idempotency key: the emission
    /// could never be told apart from another.
    EmptyEmissionKey {
        /// The timer carrying the action.
        timer: ProgramSymbol,
    },
    /// A spawn action asks for zero instances: a wave of nothing.
    ZeroSpawnCount {
        /// The timer carrying the action.
        timer: ProgramSymbol,
        /// The group it names.
        group: ProgramSymbol,
    },
    /// A cue action names content that is not dialogue.
    NotDialogue {
        /// The timer carrying the action.
        timer: ProgramSymbol,
        /// The offending id.
        id: ContentId,
    },
    /// A trigger volume is non-finite or inverted.
    InvalidVolume {
        /// The trigger.
        trigger: ProgramSymbol,
    },
    /// A declaration references an objective this program does not declare.
    UnknownObjective {
        /// The declaration carrying the reference.
        by: ProgramSymbol,
        /// The dangling symbol.
        objective: ProgramSymbol,
    },
    /// A reveal rule references a condition this program does not declare.
    UnknownCondition {
        /// The objective carrying the rule.
        by: ProgramSymbol,
        /// The dangling symbol.
        condition: ProgramSymbol,
    },
    /// A reveal rule references a timer this program does not declare.
    UnknownTimer {
        /// The objective carrying the rule.
        by: ProgramSymbol,
        /// The dangling symbol.
        timer: ProgramSymbol,
    },
    /// A spawn action references a spawn group this program does not
    /// declare, so its wave would have ids and no content.
    UnknownSpawnGroup {
        /// The timer carrying the action.
        by: ProgramSymbol,
        /// The dangling symbol.
        group: ProgramSymbol,
    },
}

impl fmt::Display for ObjectivesSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectKind { id } => {
                write!(f, "objective program subject {id} is not a mission")
            }
            Self::ReservedSymbol { symbol } => {
                write!(f, "{symbol} is reserved for actor-keyed events")
            }
            Self::DuplicateObjective { symbol } => {
                write!(f, "objective {symbol} is declared twice")
            }
            Self::DuplicateCondition { symbol } => {
                write!(f, "count condition {symbol} is declared twice")
            }
            Self::DuplicateTimer { symbol } => {
                write!(f, "timer {symbol} is declared twice")
            }
            Self::DuplicateTrigger { trigger, actor } => {
                write!(f, "trigger {trigger} watching {actor} is declared twice")
            }
            Self::DuplicateSpawnGroup { symbol } => {
                write!(f, "spawn group {symbol} is declared twice")
            }
            Self::HiddenButImmediate { objective } => write!(
                f,
                "objective {objective} declares itself hidden and shown from the first tick"
            ),
            Self::NotAnObjective { objective, id } => {
                write!(
                    f,
                    "objective {objective} names {id}, which is not an objective"
                )
            }
            Self::EmptyRoster { condition } => write!(
                f,
                "count condition {condition} declares no actor and can never be met"
            ),
            Self::ZeroRequired { condition } => write!(
                f,
                "count condition {condition} requires zero actors and is met by construction"
            ),
            Self::RequiredExceedsRoster {
                condition,
                required,
                roster,
            } => write!(
                f,
                "count condition {condition} requires {required} of a {roster}-actor roster and can never be met"
            ),
            Self::ZeroPeriod { timer } => {
                write!(f, "timer {timer} declares a zero-tick period")
            }
            Self::EmptyEmissionKey { timer } => {
                write!(f, "timer {timer} declares an empty idempotency key")
            }
            Self::ZeroSpawnCount { timer, group } => {
                write!(f, "timer {timer} asks for a zero-actor wave of {group}")
            }
            Self::NotDialogue { timer, id } => {
                write!(f, "timer {timer} cues {id}, which is not dialogue")
            }
            Self::InvalidVolume { trigger } => {
                write!(
                    f,
                    "trigger {trigger} declares a non-finite or inverted volume"
                )
            }
            Self::UnknownObjective { by, objective } => {
                write!(
                    f,
                    "{by} references objective {objective}, which is not declared"
                )
            }
            Self::UnknownCondition { by, condition } => {
                write!(
                    f,
                    "{by} references condition {condition}, which is not declared"
                )
            }
            Self::UnknownTimer { by, timer } => {
                write!(f, "{by} references timer {timer}, which is not declared")
            }
            Self::UnknownSpawnGroup { by, group } => write!(
                f,
                "{by} references spawn group {group}, which is not declared"
            ),
        }
    }
}

impl std::error::Error for ObjectivesSchemaError {}

/// One mission's declared objective program: the authored record the
/// importer produces and `cs_app::objectives::lower_program` lowers into the
/// `cs_sim::objectives` runtime declarations.
///
/// The record is **closed**: a reveal rule, timer start, timer action or
/// count reaction may only name a declaration of the same program (signals
/// excepted — a program may raise a signal nothing declared). What the
/// original game declared is unmeasured; the record carries `origin` and
/// `provenance` so an `installation` row and a `synthetic_fixture` row are
/// never interchangeable.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredObjectiveProgram {
    subject: ContentId,
    origin: Origin,
    provenance: Provenance,
    precedence: Resolved<DeclaredPrecedence>,
    objectives: Vec<DeclaredObjective>,
    conditions: Vec<DeclaredCondition>,
    timers: Vec<DeclaredTimer>,
    triggers: Vec<DeclaredTrigger>,
    spawn_groups: Vec<DeclaredSpawnGroup>,
}

impl DeclaredObjectiveProgram {
    /// Assembles and validates a declared objective program.
    ///
    /// # Errors
    ///
    /// [`ObjectivesSchemaError`] naming the first invalid declaration.
    #[expect(clippy::too_many_arguments, reason = "one field per declared list")]
    pub fn try_new(
        subject: ContentId,
        origin: Origin,
        provenance: Provenance,
        precedence: Resolved<DeclaredPrecedence>,
        objectives: Vec<DeclaredObjective>,
        conditions: Vec<DeclaredCondition>,
        timers: Vec<DeclaredTimer>,
        triggers: Vec<DeclaredTrigger>,
        spawn_groups: Vec<DeclaredSpawnGroup>,
    ) -> Result<Self, ObjectivesSchemaError> {
        validate(
            &subject,
            &objectives,
            &conditions,
            &timers,
            &triggers,
            &spawn_groups,
        )?;
        Ok(Self {
            subject,
            origin,
            provenance,
            precedence,
            objectives,
            conditions,
            timers,
            triggers,
            spawn_groups,
        })
    }

    /// The mission catalog id this program belongs to.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// Where the record came from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The record's provenance.
    #[must_use]
    pub const fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// The declared terminal precedence, or an explicit unknown.
    #[must_use]
    pub const fn precedence(&self) -> &Resolved<DeclaredPrecedence> {
        &self.precedence
    }

    /// The declared objectives, in authored order.
    #[must_use]
    pub fn objectives(&self) -> &[DeclaredObjective] {
        &self.objectives
    }

    /// The declared count conditions, in authored order.
    #[must_use]
    pub fn conditions(&self) -> &[DeclaredCondition] {
        &self.conditions
    }

    /// The declared timers, in authored order.
    #[must_use]
    pub fn timers(&self) -> &[DeclaredTimer] {
        &self.timers
    }

    /// The declared swept triggers, in authored order.
    #[must_use]
    pub fn triggers(&self) -> &[DeclaredTrigger] {
        &self.triggers
    }

    /// The declared spawn groups, in authored order.
    #[must_use]
    pub fn spawn_groups(&self) -> &[DeclaredSpawnGroup] {
        &self.spawn_groups
    }
}

/// The structural and closed-world validation
/// [`DeclaredObjectiveProgram::try_new`] applies.
fn validate(
    subject: &ContentId,
    objectives: &[DeclaredObjective],
    conditions: &[DeclaredCondition],
    timers: &[DeclaredTimer],
    triggers: &[DeclaredTrigger],
    spawn_groups: &[DeclaredSpawnGroup],
) -> Result<(), ObjectivesSchemaError> {
    if subject.kind() != ContentKind::Mission {
        return Err(ObjectivesSchemaError::SubjectKind {
            id: subject.clone(),
        });
    }

    let mut objective_ids = BTreeSet::new();
    for objective in objectives {
        check_reserved(objective.symbol)?;
        if !objective_ids.insert(objective.symbol) {
            return Err(ObjectivesSchemaError::DuplicateObjective {
                symbol: objective.symbol,
            });
        }
        if objective.initial == DeclaredObjectiveState::Hidden
            && objective.reveal == DeclaredRevealRule::Immediate
        {
            return Err(ObjectivesSchemaError::HiddenButImmediate {
                objective: objective.symbol,
            });
        }
        if objective.content.kind() != ContentKind::Objective {
            return Err(ObjectivesSchemaError::NotAnObjective {
                objective: objective.symbol,
                id: objective.content.clone(),
            });
        }
    }

    let mut condition_ids = BTreeSet::new();
    for condition in conditions {
        check_reserved(condition.symbol)?;
        if !condition_ids.insert(condition.symbol) {
            return Err(ObjectivesSchemaError::DuplicateCondition {
                symbol: condition.symbol,
            });
        }
        if condition.roster.is_empty() {
            return Err(ObjectivesSchemaError::EmptyRoster {
                condition: condition.symbol,
            });
        }
        if condition.required == 0 {
            return Err(ObjectivesSchemaError::ZeroRequired {
                condition: condition.symbol,
            });
        }
        let effective = condition.roster.iter().collect::<BTreeSet<_>>().len();
        if condition.required > effective {
            return Err(ObjectivesSchemaError::RequiredExceedsRoster {
                condition: condition.symbol,
                required: condition.required,
                roster: effective,
            });
        }
    }

    let mut timer_ids = BTreeSet::new();
    for timer in timers {
        check_reserved(timer.symbol)?;
        if !timer_ids.insert(timer.symbol) {
            return Err(ObjectivesSchemaError::DuplicateTimer {
                symbol: timer.symbol,
            });
        }
        if timer.period_ticks == 0 {
            return Err(ObjectivesSchemaError::ZeroPeriod {
                timer: timer.symbol,
            });
        }
    }

    let mut trigger_ids = BTreeSet::new();
    for trigger in triggers {
        check_reserved(trigger.symbol)?;
        if !trigger_ids.insert((trigger.symbol, trigger.actor)) {
            return Err(ObjectivesSchemaError::DuplicateTrigger {
                trigger: trigger.symbol,
                actor: trigger.actor,
            });
        }
        if !trigger.volume.is_finite() {
            return Err(ObjectivesSchemaError::InvalidVolume {
                trigger: trigger.symbol,
            });
        }
    }

    let mut group_ids = BTreeSet::new();
    for group in spawn_groups {
        check_reserved(group.symbol)?;
        if !group_ids.insert(group.symbol) {
            return Err(ObjectivesSchemaError::DuplicateSpawnGroup {
                symbol: group.symbol,
            });
        }
    }

    // Closed-world references: every name a declaration carries must name a
    // declaration of this program, so nothing waits on or acts on a row that
    // was never declared. Signals are the open set and are never checked.
    for objective in objectives {
        check_reveal(objective, &condition_ids, &timer_ids, &objective_ids)?;
    }
    for condition in conditions {
        if let DeclaredCountReaction::SetObjectiveState { objective, .. } = condition.reaction
            && !objective_ids.contains(&objective)
        {
            return Err(ObjectivesSchemaError::UnknownObjective {
                by: condition.symbol,
                objective,
            });
        }
    }
    for timer in timers {
        check_timer(timer, &objective_ids, &group_ids)?;
    }
    Ok(())
}

fn check_reserved(symbol: ProgramSymbol) -> Result<(), ObjectivesSchemaError> {
    if symbol.0 == 0 {
        return Err(ObjectivesSchemaError::ReservedSymbol { symbol });
    }
    Ok(())
}

fn check_reveal(
    objective: &DeclaredObjective,
    conditions: &BTreeSet<ProgramSymbol>,
    timers: &BTreeSet<ProgramSymbol>,
    objectives: &BTreeSet<ProgramSymbol>,
) -> Result<(), ObjectivesSchemaError> {
    match objective.reveal {
        DeclaredRevealRule::OnCondition(condition) if !conditions.contains(&condition) => {
            Err(ObjectivesSchemaError::UnknownCondition {
                by: objective.symbol,
                condition,
            })
        }
        DeclaredRevealRule::OnTimer(timer) if !timers.contains(&timer) => {
            Err(ObjectivesSchemaError::UnknownTimer {
                by: objective.symbol,
                timer,
            })
        }
        DeclaredRevealRule::OnObjectiveState {
            objective: watched, ..
        } if !objectives.contains(&watched) => Err(ObjectivesSchemaError::UnknownObjective {
            by: objective.symbol,
            objective: watched,
        }),
        _ => Ok(()),
    }
}

fn check_timer(
    timer: &DeclaredTimer,
    objectives: &BTreeSet<ProgramSymbol>,
    groups: &BTreeSet<ProgramSymbol>,
) -> Result<(), ObjectivesSchemaError> {
    if let DeclaredTimerStart::OnObjectiveState { objective, .. } = timer.start
        && !objectives.contains(&objective)
    {
        return Err(ObjectivesSchemaError::UnknownObjective {
            by: timer.symbol,
            objective,
        });
    }
    match &timer.action {
        DeclaredTimerAction::SetObjectiveState { objective, .. }
            if !objectives.contains(objective) =>
        {
            Err(ObjectivesSchemaError::UnknownObjective {
                by: timer.symbol,
                objective: *objective,
            })
        }
        DeclaredTimerAction::SpawnGroup { key, group, count } => {
            if key.trim().is_empty() {
                return Err(ObjectivesSchemaError::EmptyEmissionKey {
                    timer: timer.symbol,
                });
            }
            if *count == 0 {
                return Err(ObjectivesSchemaError::ZeroSpawnCount {
                    timer: timer.symbol,
                    group: *group,
                });
            }
            if !groups.contains(group) {
                return Err(ObjectivesSchemaError::UnknownSpawnGroup {
                    by: timer.symbol,
                    group: *group,
                });
            }
            Ok(())
        }
        DeclaredTimerAction::Cue { key, dialogue } => {
            if key.trim().is_empty() {
                return Err(ObjectivesSchemaError::EmptyEmissionKey {
                    timer: timer.symbol,
                });
            }
            if dialogue.kind() != ContentKind::Dialogue {
                return Err(ObjectivesSchemaError::NotDialogue {
                    timer: timer.symbol,
                    id: dialogue.clone(),
                });
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The synthetic fixture mission's program symbol for its primary objective.
pub const SYNTHETIC_PRIMARY: ProgramSymbol = ProgramSymbol(1);
/// The fixture's hidden secondary objective, revealed by the `reached-wreck`
/// signal.
pub const SYNTHETIC_SECONDARY: ProgramSymbol = ProgramSymbol(2);
/// The fixture's protected-actor condition.
pub const SYNTHETIC_PROTECTED_LOST: ProgramSymbol = ProgramSymbol(10);
/// The three wave timers: each admits one wave of [`SYNTHETIC_RAIDERS`] on an
/// explicit program arm.
pub const SYNTHETIC_WAVE_TIMERS: [ProgramSymbol; 3] =
    [ProgramSymbol(20), ProgramSymbol(21), ProgramSymbol(22)];
/// The fixture's radio-cue timer.
pub const SYNTHETIC_RADIO: ProgramSymbol = ProgramSymbol(25);
/// The fixture's completion deadline: an explicit arm completes the primary
/// objective one tick later.
pub const SYNTHETIC_DEADLINE: ProgramSymbol = ProgramSymbol(30);
/// The fixture's spawn group, bound to the `synthetic.raider` airframe.
pub const SYNTHETIC_RAIDERS: ProgramSymbol = ProgramSymbol(40);
/// The fixture's approach trigger, watching [`SYNTHETIC_PLAYER`].
pub const SYNTHETIC_APPROACH: ProgramSymbol = ProgramSymbol(50);
/// The player actor the fixture's trigger and rosters name.
pub const SYNTHETIC_PLAYER: ProgramActor = ProgramActor(7);
/// The protected convoy actor the fixture's failure condition names.
pub const SYNTHETIC_PROTECTED: ProgramActor = ProgramActor(41);
/// The signal the fixture's secondary objective reveals on.
pub const SYNTHETIC_REACHED_WRECK: ProgramSymbol = ProgramSymbol(60);

fn designed<T>(value: T, provenance: &Provenance) -> Resolved<T> {
    Resolved::Known(Known::new(value, provenance.clone()))
}

/// The minimal synthetic mission program in declared form: one active
/// primary objective whose completion requests `Success`, one hidden
/// secondary revealed by a named signal, a protected-actor `Destroyed`
/// condition requesting `Failure`, three spawn-wave timers with distinct
/// idempotency keys, one radio-cue timer, one completion deadline and one
/// approach trigger on the player.
///
/// The subject is `mission/synthetic.f39c.rescue`, every identity lives
/// under `synthetic.f39c` keys and the record carries
/// [`Origin::SyntheticFixture`] with designed provenance — it can never be
/// mistaken for retail content and cannot stand in for it.
#[must_use]
pub fn declared_synthetic_objectives() -> DeclaredObjectiveProgram {
    let provenance =
        Provenance::designed(ClaimId::new("f39c.synthetic-rescue").expect("valid claim id"));
    let objective_id = |key: &str| {
        ContentId::from_source(ContentKind::Objective, key).expect("valid objective id")
    };
    let mission = ContentId::from_source(ContentKind::Mission, "synthetic.f39c.rescue")
        .expect("valid mission id");
    let raider = ContentId::from_source(ContentKind::Airframe, "synthetic.raider")
        .expect("valid airframe id");
    let dialogue = ContentId::from_source(ContentKind::Dialogue, "synthetic.f39c.wave-inbound")
        .expect("valid dialogue id");

    DeclaredObjectiveProgram::try_new(
        mission,
        Origin::SyntheticFixture,
        provenance.clone(),
        designed(DeclaredPrecedence::SyntheticConservative, &provenance),
        vec![
            DeclaredObjective {
                symbol: SYNTHETIC_PRIMARY,
                content: objective_id("synthetic.f39c.primary"),
                initial: DeclaredObjectiveState::Active,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: DeclaredCompletion::Requests(DeclaredTerminalOutcome::Success),
            },
            DeclaredObjective {
                symbol: SYNTHETIC_SECONDARY,
                content: objective_id("synthetic.f39c.secondary"),
                initial: DeclaredObjectiveState::Hidden,
                reveal: DeclaredRevealRule::OnSignal(SYNTHETIC_REACHED_WRECK),
                on_complete: DeclaredCompletion::Continue,
            },
        ],
        vec![DeclaredCondition {
            symbol: SYNTHETIC_PROTECTED_LOST,
            kind: DeclaredCountKind::Destroyed,
            roster: vec![SYNTHETIC_PROTECTED],
            required: 1,
            reaction: DeclaredCountReaction::Finish(DeclaredTerminalOutcome::Failure),
        }],
        vec![
            DeclaredTimer {
                symbol: SYNTHETIC_WAVE_TIMERS[0],
                domain: DeclaredTimeDomain::AuthoritativeGameplay,
                start: DeclaredTimerStart::OnArm,
                period_ticks: 1,
                action: DeclaredTimerAction::SpawnGroup {
                    key: "synthetic.f39c.wave-1".to_owned(),
                    group: SYNTHETIC_RAIDERS,
                    count: 2,
                },
            },
            DeclaredTimer {
                symbol: SYNTHETIC_WAVE_TIMERS[1],
                domain: DeclaredTimeDomain::AuthoritativeGameplay,
                start: DeclaredTimerStart::OnArm,
                period_ticks: 1,
                action: DeclaredTimerAction::SpawnGroup {
                    key: "synthetic.f39c.wave-2".to_owned(),
                    group: SYNTHETIC_RAIDERS,
                    count: 2,
                },
            },
            DeclaredTimer {
                symbol: SYNTHETIC_WAVE_TIMERS[2],
                domain: DeclaredTimeDomain::AuthoritativeGameplay,
                start: DeclaredTimerStart::OnArm,
                period_ticks: 1,
                action: DeclaredTimerAction::SpawnGroup {
                    key: "synthetic.f39c.wave-3".to_owned(),
                    group: SYNTHETIC_RAIDERS,
                    count: 2,
                },
            },
            DeclaredTimer {
                symbol: SYNTHETIC_RADIO,
                domain: DeclaredTimeDomain::AuthoritativeGameplay,
                start: DeclaredTimerStart::OnArm,
                period_ticks: 1,
                action: DeclaredTimerAction::Cue {
                    key: "synthetic.f39c.radio-wave-inbound".to_owned(),
                    dialogue,
                },
            },
            DeclaredTimer {
                symbol: SYNTHETIC_DEADLINE,
                domain: DeclaredTimeDomain::AuthoritativeGameplay,
                start: DeclaredTimerStart::OnArm,
                period_ticks: 1,
                action: DeclaredTimerAction::SetObjectiveState {
                    objective: SYNTHETIC_PRIMARY,
                    state: DeclaredObjectiveState::Succeeded,
                },
            },
        ],
        vec![DeclaredTrigger {
            symbol: SYNTHETIC_APPROACH,
            actor: SYNTHETIC_PLAYER,
            volume: DeclaredVolume::Aabb {
                min_m: [-1.0, -1.0, -1.0],
                max_m: [1.0, 1.0, 1.0],
            },
        }],
        vec![DeclaredSpawnGroup {
            symbol: SYNTHETIC_RAIDERS,
            subject: raider,
        }],
    )
    .expect("the synthetic objective program is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> DeclaredObjectiveProgram {
        declared_synthetic_objectives()
    }

    #[test]
    fn accept_f39_c_the_declared_program_is_self_contained_and_typed() {
        let program = program();
        assert_eq!(program.subject().kind(), ContentKind::Mission);
        assert_eq!(program.origin(), &Origin::SyntheticFixture);
        assert!(program.precedence().is_known());
        assert_eq!(program.objectives().len(), 2);
        assert_eq!(program.conditions().len(), 1);
        assert_eq!(program.timers().len(), 5);
        assert_eq!(program.triggers().len(), 1);
        assert_eq!(program.spawn_groups().len(), 1);
    }

    #[test]
    #[allow(
        clippy::type_complexity,
        reason = "the rebuild edit names one &mut per declared list"
    )]
    fn accept_f39_c_the_schema_refuses_dangling_names_and_dead_declarations() {
        let base = program();
        let rebuild = |edit: &dyn Fn(
            &mut Vec<DeclaredObjective>,
            &mut Vec<DeclaredCondition>,
            &mut Vec<DeclaredTimer>,
            &mut Vec<DeclaredTrigger>,
            &mut Vec<DeclaredSpawnGroup>,
        )| {
            let (mut o, mut c, mut t, mut g, mut s) = (
                base.objectives.clone(),
                base.conditions.clone(),
                base.timers.clone(),
                base.triggers.clone(),
                base.spawn_groups.clone(),
            );
            edit(&mut o, &mut c, &mut t, &mut g, &mut s);
            DeclaredObjectiveProgram::try_new(
                base.subject.clone(),
                base.origin.clone(),
                base.provenance.clone(),
                base.precedence.clone(),
                o,
                c,
                t,
                g,
                s,
            )
        };

        // A reveal rule naming a timer the program does not declare.
        assert!(matches!(
            rebuild(&|o, _, _, _, _| {
                o[1].reveal = DeclaredRevealRule::OnTimer(ProgramSymbol(99));
            })
            .unwrap_err(),
            ObjectivesSchemaError::UnknownTimer { .. }
        ));
        // A reaction moving an objective that does not exist.
        assert!(matches!(
            rebuild(&|_, c, _, _, _| {
                c[0].reaction = DeclaredCountReaction::SetObjectiveState {
                    objective: ProgramSymbol(99),
                    state: DeclaredObjectiveState::Failed,
                };
            })
            .unwrap_err(),
            ObjectivesSchemaError::UnknownObjective { .. }
        ));
        // A wave of a group nobody declared: ids with no content.
        assert!(matches!(
            rebuild(&|_, _, _, _, s| {
                s.clear();
            })
            .unwrap_err(),
            ObjectivesSchemaError::UnknownSpawnGroup { .. }
        ));
        // A condition no roster actor can satisfy.
        assert!(matches!(
            rebuild(&|_, c, _, _, _| {
                c[0].required = 2;
            })
            .unwrap_err(),
            ObjectivesSchemaError::RequiredExceedsRoster { .. }
        ));
        // A repeated wave key under one key is fine; an empty key is not.
        assert!(matches!(
            rebuild(&|_, _, t, _, _| {
                t[0].action = DeclaredTimerAction::SpawnGroup {
                    key: String::new(),
                    group: SYNTHETIC_RAIDERS,
                    count: 2,
                };
            })
            .unwrap_err(),
            ObjectivesSchemaError::EmptyEmissionKey { .. }
        ));
        // A cue that is not dialogue.
        let gun = ContentId::from_source(ContentKind::Gun, "x").unwrap();
        assert!(matches!(
            rebuild(&|_, _, t, _, _| {
                t[3].action = DeclaredTimerAction::Cue {
                    key: "k".to_owned(),
                    dialogue: gun.clone(),
                };
            })
            .unwrap_err(),
            ObjectivesSchemaError::NotDialogue { .. }
        ));
        // The reserved actor-event symbol.
        assert_eq!(
            rebuild(&|o, _, _, _, _| {
                o[0].symbol = ProgramSymbol(0);
            })
            .unwrap_err(),
            ObjectivesSchemaError::ReservedSymbol {
                symbol: ProgramSymbol(0)
            }
        );
    }
}
