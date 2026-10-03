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
//! looking like one that simply never came due. Signal names are the one
//! open set — a program may raise a signal nothing declares — but never an
//! *ambiguous* name: a signal reference may not carry the reserved
//! actor-event symbol or a symbol this program declares, because a raised
//! signal reports under its own name and would alias that declaration's
//! events. A declared move or watch may likewise target only a state a
//! declared event can produce (`Active`, `Succeeded`, `Failed`,
//! `Superseded`): the legal transitions into `Hidden`, `Pending` and
//! `Optional` all start from `Hidden`, which only the reveal rule may
//! leave, so a declaration aiming at them could never apply or fire.
//!
//! # The actor id space
//!
//! Declared [`ProgramActor`]s and the wave instances the runtime allocates
//! share one `cs_script::ir::ActorId` space. An admitted wave takes ids from
//! 1 upward in admission order — the earlier tick first, and within a tick
//! the expiring timers' symbol order — and a roster or trigger naming one
//! of those ids names the spawned instance, which is the *only* way a
//! condition counts a spawned wave. A pre-placed actor must therefore carry
//! an id outside the range its program's waves allocate; the fixture keeps
//! the player at `actor(7)` and the convoy at `actor(41)`, above the six
//! ids its three two-raider waves take. Which actor ids original missions
//! declared and how their spawn identities worked is unmeasured (F39-D);
//! nothing here is an original-fidelity claim.
//!
//! # Designed vocabulary, not original data
//!
//! No original mission program has been decoded into this form (F38 owns the
//! original mission language). Every kind, rule name and fixture value here is
//! newly authored project design carrying `Origin::SyntheticFixture`/designed
//! provenance.
//!
//! # F39-D: support, and branches that cannot fire
//!
//! Two things this module gained in the calibration stage, both answers to
//! questions F39-D had to put to the original installation
//! (`docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`):
//!
//! * **Support.** Every record carries a [`DeclaredSupport`], derived from its
//!   `origin`: a newly authored record is [`DeclaredSupport::Authored`] and
//!   playable, while an `installation` record is
//!   [`DeclaredSupport::Original`] with **no** recovery and is therefore
//!   unplayable until an importer attaches one through
//!   [`DeclaredObjectiveProgram::with_recovery`]. F39-D's retail census read
//!   every mission directory's `objectives.zrd` on the owner's installation
//!   and decoded **none** of them, because the mission-language instruction
//!   table is still unmeasured (F13-B/C) — so `docs/contracts/SCRIPT-MISSION.md`'s
//!   rule ("if the actual program … cannot be decoded, the mission remains
//!   Unsupported") currently holds for *every* original mission, and the
//!   record says so instead of shipping a designed progression under an
//!   original mission's id.
//! * **Dead branches.** F39 AC04 asks that supported objectives be completable
//!   out of the common order without deadlocking the program, and F39-D found
//!   two declarations that do exactly that: a reveal rule waiting for the
//!   objective it reveals ([`ObjectivesSchemaError::DeadSelfReveal`]) and a
//!   watch on a state the watched objective already holds
//!   ([`ObjectivesSchemaError::DeadWatch`]). Neither can fire in any order, so
//!   both are refused at declaration, by name, with the rule stated. The
//!   order-independent *completion* rule lives with the state table in
//!   `cs_sim::objectives::state`.

use std::collections::{BTreeMap, BTreeSet};
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
/// references. `OnSignal` names a signal the program raises — an open name,
/// checked only against ambiguity: never the reserved actor-event source or
/// a symbol this program declares. Every other variant names a declaration
/// of this program and is validated by [`DeclaredObjectiveProgram::try_new`].
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
    /// eligible. An open name, checked only against ambiguity: never the
    /// reserved actor-event source or a symbol this program declares.
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
    /// next tick. An open name, checked only against ambiguity: never the
    /// reserved actor-event source or a symbol this program declares.
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

/// What makes a mission's objective program playable, and what that claim is
/// worth.
///
/// This is F39-D's answer to the word **supported** in F39's acceptance case
/// AC04 ("complete *supported* objectives …") and to
/// `docs/contracts/SCRIPT-MISSION.md`'s rule: *"If the actual program is
/// unavailable or cannot be decoded, the mission remains Unsupported."*
///
/// The two variants are the two claims a record can make, and only one of them
/// is playable today:
///
/// * [`DeclaredSupport::Authored`] — the record was written by this project (a
///   synthetic fixture or a designed engine rule). It may be lowered and played,
///   and it is **never** a claim about the original game.
/// * [`DeclaredSupport::Original`] — the record's bytes came from the owner's
///   installation. It may carry a [`MeasuredObjectiveRecord`] — what F39-D
///   measured of the mission's `objectives.zrd` member — but it is **not**
///   playable, because a census of declaration sites is not a set of rules: the
///   original declares 1338 objective blocks across 53 mission readers, and
///   F39-D could read *which* declarations a block carries and not *what any of
///   them does*. Playing a designed progression in its place would substitute
///   invented semantics for 1338 measured declarations.
///
/// A record whose rules are later recovered by a measured probe — which needs
/// the mission-language instruction table F13-C/F38 own — becomes playable then,
/// under its own support variant. Nothing constructs that variant today and this
/// stage does not invent it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclaredSupport {
    /// Newly authored: a synthetic fixture or designed engine rule. Runnable,
    /// and never an original-fidelity claim.
    Authored,
    /// Original installation data, with or without a measurement.
    Original {
        /// What F39-D measured of the mission's objective record. `None` when
        /// nothing was measured.
        record: Option<Box<MeasuredObjectiveRecord>>,
        /// Why this program's semantics may not be run. Always present, so a
        /// refusal names itself instead of only reporting that something is
        /// missing.
        reason: String,
    },
}

impl DeclaredSupport {
    /// Whether a session may lower and run a program carrying this support.
    ///
    /// The single place the answer is given, so no consumer can decide support
    /// from the record's contents instead of from its provenance.
    #[must_use]
    pub fn is_playable(&self) -> bool {
        matches!(self, Self::Authored)
    }

    /// Why a program carrying this support may not be played, by name.
    #[must_use]
    pub fn refusal(&self) -> Option<&str> {
        match self {
            Self::Authored => None,
            Self::Original { reason, .. } => Some(reason),
        }
    }
}

/// What F39-D measured of one mission's original objective record.
///
/// A **census of declaration sites**, deliberately not a rule: each number says
/// how many times a measured key occurs inside that mission's numbered
/// `OBJECTIVE<N>` blocks, never what one occurrence does. Nothing here may be
/// read as a decoded behaviour, and the counts are exactly why a designed
/// progression must not stand in for the original (see
/// [`DeclaredSupport::Original`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredObjectiveRecord {
    /// The reader archive the record is a member of, as the installation spells
    /// it (`ZBD/<GROUP>/<MISSION>/zrdr.zbd`).
    ///
    /// This is production discovery's `RelativePath::as_str()`, the exact path
    /// the bytes were read from and therefore the spelling to reproduce.
    /// [`RelativePath::logical_key`](cs_types::install::RelativePath::logical_key)
    /// of it is the lowercase canonical key (`zbd/<group>/<mission>/zrdr.zbd`)
    /// the rest of the project indexes by.
    pub container: String,
    /// The member's name inside that archive (`objectives.zrd`).
    pub member: String,
    /// The member's SHA-256, hex.
    pub sha256: String,
    /// The member's length in bytes.
    pub byte_len: u64,
    /// How many numbered `OBJECTIVE<N>` blocks the mission declares.
    pub blocks: u32,
    /// Occurrences of [`BRANCH_KEY_VOCABULARY`] keys across those blocks: the
    /// measured *sites* where one objective's completion is declared to change
    /// another, or where an order is declared.
    pub branching_sites: u32,
    /// Occurrences of the optionality keys across those blocks.
    pub optional_sites: u32,
    /// Occurrences of [`FAILURE_KEY_VOCABULARY`] keys across those blocks: the
    /// measured sites where an outcome is declared.
    pub failure_sites: u32,
}

/// The measured key names an objective block uses to say what happens to
/// *another* objective when this one completes, plus the one explicit ordering
/// dependency.
///
/// Measured over the owner's installation (F39-D): every mission-scoped reader
/// archive was opened, its `objectives.zrd` member decoded with the production
/// `.zrd` reader, and the numbered `OBJECTIVE<N>` blocks read. Across 53 mission
/// readers and 1338 blocks these keys occur 1091 times in all
/// (`WAKE_OBJECTIVE_WHEN_I_COMPLETE` 412, `NAP_OBJECTIVE_WHEN_I_COMPLETE` 417,
/// `KILL_OBJECTIVE_WHEN_I_COMPLETE` 225, `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` 2,
/// `TICK_DEPENDS_ON_OBJ` 35).
///
/// The spellings are **measured**; what they do is an **inference and stays
/// one**. No original executable has been run, and the compiled program behind
/// these records is not decoded — F13-C/F38 own the instruction table.
pub const BRANCH_KEY_VOCABULARY: [&str; 5] = [
    "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    "KILL_OBJECTIVE_WHEN_I_COMPLETE",
    "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE",
    "TICK_DEPENDS_ON_OBJ",
];

/// The measured prefix of an *inactive stage* key inside an objective block.
///
/// Measured: `INACTIVE1` through `INACTIVE18`, one per numbered stage. That a
/// dormant stage becomes active once its count is met is an **inference** from
/// [`OBJECTIVE_INACTIVE_COUNT_KEY`] sitting beside the stages, and stays one.
pub const OBJECTIVE_INACTIVE_STAGE_PREFIX: &str = "INACTIVE";

/// The measured count key that sits beside an objective block's inactive
/// stages. Measured in 130 of the 1338 blocks.
pub const OBJECTIVE_INACTIVE_COUNT_KEY: &str = "INACTIVE_COMPLETION_COUNT";

/// The measured key an objective block carries to begin dormant. Measured in
/// 1096 of the 1338 blocks.
pub const OBJECTIVE_DORMANT_KEY: &str = "BEGIN_DORMANT";

/// The measured key names an objective block uses to declare an outcome.
///
/// Measured over the same corpus: `INSTANTWIN` occurs 15 times and
/// `INSTANTLOSS` 9 times across 53 mission readers. Which outcome each one
/// means, and what ends the mission, is **not** measured.
pub const FAILURE_KEY_VOCABULARY: [&str; 2] = ["INSTANTWIN", "INSTANTLOSS"];

/// Whether a measured key is one of the optionality declarations.
///
/// [`OBJECTIVE_INACTIVE_COUNT_KEY`] is matched exactly; a stage key is
/// `INACTIVE` followed by the stage number. Exact matching keeps
/// `INACTIVE_COMPLETION_COUNT` from reading as a stage and keeps a key this
/// stage never saw — `INACTIVATED`, `INACTIVE_A` — out of the count.
///
/// The numeric suffix is deliberately *not* capped at the measured
/// `INACTIVE1`…`INACTIVE18`: a stage number beyond 18 is the same measured
/// family, and a cap would silently drop a key the census can see. So the rule
/// can over-match a stage number nobody has seen; what it cannot do is invent
/// one. `RetailObjectiveCensus::vocabulary` publishes the whole measured key
/// list beside every classification, and
/// `accept_f39_d_retail_objective_records_declare_branching_outcomes_and_optionality`
/// asserts that nothing outside the measured range matches, so an over-match
/// fails loudly instead of quietly inflating a count.
#[must_use]
pub fn is_optional_objective_key(key: &str) -> bool {
    if key == OBJECTIVE_INACTIVE_COUNT_KEY {
        return true;
    }
    let Some(rest) = key.strip_prefix(OBJECTIVE_INACTIVE_STAGE_PREFIX) else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

/// Why an original record's rules are unmeasured, stated once so the refusal,
/// the finding and the census all say the same thing.
///
/// It names the measured fact behind it: the objective record's declaration
/// vocabulary is readable and its rules are not.
pub const UNMEASURED_OBJECTIVE_SEMANTICS: &str = "the mission's objective record was measured and its branching, optionality and outcome \
     declarations were counted, but no rule was recovered: what any declaration means is unmeasured, \
     so the mission's branching, optional and failure conditions are unknown and a designed \
     progression must not stand in for them";

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
    /// A declared signal reference names the reserved actor-event source or
    /// a symbol the same program declares. A raised signal is attributed to
    /// its name, so under this name it could not be told apart from that
    /// declaration's own events.
    CollidingSignal {
        /// The declaration carrying the reference.
        by: ProgramSymbol,
        /// The colliding signal name.
        signal: ProgramSymbol,
    },
    /// A declared move or watch targets a state nothing produces. No legal
    /// transition a declared action can perform reaches `Hidden`, `Pending`
    /// or `Optional` — the rows into them start from `Hidden`, which only
    /// the reveal rule may leave — so the declaration could never apply or
    /// fire: dead, refused at declaration.
    DeadState {
        /// The declaration carrying the target.
        by: ProgramSymbol,
        /// The unreachable target state.
        state: DeclaredObjectiveState,
    },
    /// A reveal rule watches the objective it reveals.
    ///
    /// An objective leaves `Hidden` only through its own reveal rule, and a
    /// state change against a still-hidden objective is refused, so a rule
    /// waiting for that objective's own state change has no order in which it
    /// can fire first: the branch is dead, and a mission with a dead reveal
    /// never shows the objective at all.
    DeadSelfReveal {
        /// The objective carrying the rule.
        objective: ProgramSymbol,
    },
    /// A reveal rule or a timer start watches a state the watched objective
    /// can never reach.
    ///
    /// The runtime fires a watch on a *state change into* the watched state, so
    /// a watch is dead in exactly two cases, both refused here: the objective
    /// already holds the state (no state is reachable from itself), and the
    /// objective was born terminal (no row leaves a final state at all). A
    /// reveal that never reveals, or a deadline that never arms, is a program
    /// waiting on an event no declaration can produce.
    DeadWatch {
        /// The declaration carrying the watch.
        by: ProgramSymbol,
        /// The objective it watches.
        objective: ProgramSymbol,
        /// The state it waits for.
        state: DeclaredObjectiveState,
    },
    /// A measurement names no archive or member, so it measures nothing.
    EmptyMeasurement,
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
            Self::CollidingSignal { by, signal } => write!(
                f,
                "{by} references signal {signal}, which is a declared name and would report under it"
            ),
            Self::DeadState { by, state } => write!(
                f,
                "{by} targets objective state {state:?}, which no declared action or event can produce"
            ),
            Self::DeadSelfReveal { objective } => write!(
                f,
                "objective {objective} is revealed by its own state change, which it cannot make while hidden"
            ),
            Self::DeadWatch {
                by,
                objective,
                state,
            } => write!(
                f,
                "{by} watches objective {objective} reaching {state:?}, a state it already holds and can never reach again"
            ),
            Self::EmptyMeasurement => write!(
                f,
                "a measured objective record must name the archive and member it was read from"
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
/// excepted — a program may raise a signal nothing declares, so long as the
/// name aliases no declaration and reserves no actor-event source). What
/// the original game declared is unmeasured; the record carries `origin`
/// and `provenance` so an `installation` row and a `synthetic_fixture` row
/// are never interchangeable.
///
/// It also carries a [`DeclaredSupport`], derived from `origin` at
/// construction: a newly authored record is [`DeclaredSupport::Authored`] and
/// playable, while an `installation` record starts
/// [`DeclaredSupport::Original`] with **no** recovery and is therefore
/// unplayable until an importer attaches one through
/// [`with_recovery`](Self::with_recovery). That default is the safe direction
/// on purpose: an importer that forgets its recovery gets a program nobody can
/// run, not a generic progression wearing an original mission's name.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredObjectiveProgram {
    subject: ContentId,
    origin: Origin,
    provenance: Provenance,
    support: DeclaredSupport,
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
            support: support_for(&origin),
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

    /// Attaches what F39-D measured of this mission's original objective record.
    ///
    /// The measurement is a census of **declaration sites** — which keys a
    /// mission's objective blocks carry, and how often — and it never makes the
    /// program playable: [`DeclaredSupport::Original`] stays unplayable because
    /// a declaration site is not a rule. Attaching it is what lets a refusal
    /// name the numbers instead of only naming the absence, and it is the only
    /// way a record's support stops saying "nothing was measured".
    ///
    /// # Errors
    ///
    /// [`ObjectivesSchemaError::EmptyMeasurement`] when the measurement names
    /// no archive or no member, which would be a measurement of nothing.
    pub fn with_measured_record(
        mut self,
        record: MeasuredObjectiveRecord,
    ) -> Result<Self, ObjectivesSchemaError> {
        if record.container.trim().is_empty() || record.member.trim().is_empty() {
            return Err(ObjectivesSchemaError::EmptyMeasurement);
        }
        if let DeclaredSupport::Original { reason, .. } = &self.support {
            let reason = reason.clone();
            self.support = DeclaredSupport::Original {
                record: Some(Box::new(record)),
                reason,
            };
        }
        // A newly authored record needs no measurement and is not turned into
        // an original claim by attaching one.
        Ok(self)
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

    /// What this record's objective semantics are worth, and whether they may
    /// be run.
    #[must_use]
    pub const fn support(&self) -> &DeclaredSupport {
        &self.support
    }

    /// Whether a session may lower and run this program.
    #[must_use]
    pub fn is_playable(&self) -> bool {
        self.support.is_playable()
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

/// The support a record's origin implies before anything is measured from it.
///
/// The safe direction is the whole point: `installation` data starts
/// **unplayable**, so an importer that never attaches its census cannot publish
/// a runnable objective progression under an original mission's id.
fn support_for(origin: &Origin) -> DeclaredSupport {
    match origin {
        Origin::SyntheticFixture | Origin::Designed => DeclaredSupport::Authored,
        Origin::Installation { .. } => DeclaredSupport::Original {
            record: None,
            reason: UNMEASURED_OBJECTIVE_SEMANTICS.to_owned(),
        },
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
    let mut objective_by_symbol: BTreeMap<ProgramSymbol, &DeclaredObjective> = BTreeMap::new();
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
        objective_by_symbol.insert(objective.symbol, objective);
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

    // The names this program declares: a signal reference may name the open
    // set, but never one of these — a raised signal reports under its own
    // name and would alias the declaration's events.
    let declared: BTreeSet<ProgramSymbol> = objective_ids
        .iter()
        .chain(&condition_ids)
        .chain(&timer_ids)
        .chain(trigger_ids.iter().map(|(symbol, _)| symbol))
        .chain(&group_ids)
        .copied()
        .collect();

    // Closed-world references: every name a declaration carries must name a
    // declaration of this program, so nothing waits on or acts on a row that
    // was never declared — and every target state it names must be one a
    // declared event can produce, so nothing dead is authored.
    for objective in objectives {
        check_reveal(
            objective,
            &condition_ids,
            &timer_ids,
            &objective_by_symbol,
            &declared,
        )?;
    }
    for condition in conditions {
        if let DeclaredCountReaction::SetObjectiveState { objective, state } = condition.reaction {
            if !objective_ids.contains(&objective) {
                return Err(ObjectivesSchemaError::UnknownObjective {
                    by: condition.symbol,
                    objective,
                });
            }
            check_state_target(condition.symbol, state)?;
        }
    }
    for timer in timers {
        check_timer(timer, &objective_by_symbol, &group_ids, &declared)?;
    }
    Ok(())
}

fn check_reserved(symbol: ProgramSymbol) -> Result<(), ObjectivesSchemaError> {
    if symbol.0 == 0 {
        return Err(ObjectivesSchemaError::ReservedSymbol { symbol });
    }
    Ok(())
}

/// A declared signal reference may name the open set — signals nothing
/// declares — but never the reserved actor-event source or a symbol this
/// program declares: a raised signal is attributed to its name, and under a
/// declared name it could not be told apart from that declaration's own
/// events.
fn check_signal(
    by: ProgramSymbol,
    signal: ProgramSymbol,
    declared: &BTreeSet<ProgramSymbol>,
) -> Result<(), ObjectivesSchemaError> {
    check_reserved(signal)?;
    if declared.contains(&signal) {
        return Err(ObjectivesSchemaError::CollidingSignal { by, signal });
    }
    Ok(())
}

/// A declared state change a live declared action can perform never leaves
/// `Hidden`, so the rows into `Hidden`, `Pending` and `Optional` are reachable
/// only from `Hidden`, which only the reveal rule leaves. A declaration aiming
/// at one of them could never apply: dead, refused at declaration.
fn check_state_target(
    by: ProgramSymbol,
    state: DeclaredObjectiveState,
) -> Result<(), ObjectivesSchemaError> {
    match state {
        DeclaredObjectiveState::Hidden
        | DeclaredObjectiveState::Pending
        | DeclaredObjectiveState::Optional => Err(ObjectivesSchemaError::DeadState { by, state }),
        _ => Ok(()),
    }
}

/// A watch that can never fire, because nothing can ever move the objective it
/// watches into the state it waits for.
///
/// A watch is satisfied by a *state change* into the watched state, and a state
/// is only reachable from a state a row leaves. That rules a watch out in
/// exactly two cases, and both are refused here:
///
/// * the watched objective **already holds** the state — no state is reachable
///   from itself (`Active` only moves to the three final states, every move out
///   of `Pending`/`Optional` lands on a state no row returns from, and the
///   reveal out of `Hidden` reports `ObjectiveRevealed` rather than a state
///   change);
/// * the watched objective was **born terminal** — no row leaves `Succeeded`,
///   `Failed` or `Superseded`, so no declaration can move it anywhere at all.
///
/// Those two cases exhaust the possibility, because every remaining birth state
/// reaches every remaining watchable state: `Hidden` reaches `Active`,
/// `Optional`, `Succeeded`, `Failed` and `Superseded` (the first by a declared
/// move, the rest from `Active`/`Optional`), and `Pending`, `Active` and
/// `Optional` each reach the three final states. So a program that passes here
/// has a watch that can fire, not merely one that is not obviously dead.
fn check_watch(
    by: ProgramSymbol,
    objective: &DeclaredObjective,
    state: DeclaredObjectiveState,
) -> Result<(), ObjectivesSchemaError> {
    let already_holds = objective.initial == state;
    let born_terminal = matches!(
        objective.initial,
        DeclaredObjectiveState::Succeeded
            | DeclaredObjectiveState::Failed
            | DeclaredObjectiveState::Superseded
    );
    if already_holds || born_terminal {
        return Err(ObjectivesSchemaError::DeadWatch {
            by,
            objective: objective.symbol,
            state,
        });
    }
    Ok(())
}

fn check_reveal(
    objective: &DeclaredObjective,
    conditions: &BTreeSet<ProgramSymbol>,
    timers: &BTreeSet<ProgramSymbol>,
    objectives: &BTreeMap<ProgramSymbol, &DeclaredObjective>,
    declared: &BTreeSet<ProgramSymbol>,
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
            objective: watched,
            state,
        } => {
            let Some(target) = objectives.get(&watched) else {
                return Err(ObjectivesSchemaError::UnknownObjective {
                    by: objective.symbol,
                    objective: watched,
                });
            };
            check_state_target(objective.symbol, state)?;
            // A reveal rule that waits for the objective it reveals has no
            // order in which it fires: the objective leaves `Hidden` through
            // this very rule, and a state change against a still-hidden
            // objective is refused.
            if watched == objective.symbol {
                return Err(ObjectivesSchemaError::DeadSelfReveal {
                    objective: objective.symbol,
                });
            }
            check_watch(objective.symbol, target, state)
        }
        DeclaredRevealRule::OnSignal(signal) => check_signal(objective.symbol, signal, declared),
        _ => Ok(()),
    }
}

fn check_timer(
    timer: &DeclaredTimer,
    objectives: &BTreeMap<ProgramSymbol, &DeclaredObjective>,
    groups: &BTreeSet<ProgramSymbol>,
    declared: &BTreeSet<ProgramSymbol>,
) -> Result<(), ObjectivesSchemaError> {
    match timer.start {
        DeclaredTimerStart::OnObjectiveState { objective, state } => {
            let Some(target) = objectives.get(&objective) else {
                return Err(ObjectivesSchemaError::UnknownObjective {
                    by: timer.symbol,
                    objective,
                });
            };
            check_state_target(timer.symbol, state)?;
            check_watch(timer.symbol, target, state)?;
        }
        DeclaredTimerStart::OnSignal(signal) => check_signal(timer.symbol, signal, declared)?,
        _ => {}
    }
    match &timer.action {
        DeclaredTimerAction::SetObjectiveState { objective, state } => {
            let Some(_) = objectives.get(objective) else {
                return Err(ObjectivesSchemaError::UnknownObjective {
                    by: timer.symbol,
                    objective: *objective,
                });
            };
            check_state_target(timer.symbol, *state)
        }
        DeclaredTimerAction::Signal(signal) => check_signal(timer.symbol, *signal, declared),
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
        // A signal reference may name the open set, but never the reserved
        // actor source or a symbol the program declares: under that name the
        // raised signal would alias the declaration's own events.
        assert!(matches!(
            rebuild(&|o, _, _, _, _| {
                o[1].reveal = DeclaredRevealRule::OnSignal(ProgramSymbol(0));
            })
            .unwrap_err(),
            ObjectivesSchemaError::ReservedSymbol { .. }
        ));
        assert_eq!(
            rebuild(&|o, _, _, _, _| {
                o[1].reveal = DeclaredRevealRule::OnSignal(SYNTHETIC_PRIMARY);
            })
            .unwrap_err(),
            ObjectivesSchemaError::CollidingSignal {
                by: SYNTHETIC_SECONDARY,
                signal: SYNTHETIC_PRIMARY
            }
        );
        assert_eq!(
            rebuild(&|_, _, t, _, _| {
                t[0].action = DeclaredTimerAction::Signal(SYNTHETIC_RAIDERS);
            })
            .unwrap_err(),
            ObjectivesSchemaError::CollidingSignal {
                by: SYNTHETIC_WAVE_TIMERS[0],
                signal: SYNTHETIC_RAIDERS
            }
        );
        assert_eq!(
            rebuild(&|_, _, t, _, _| {
                t[0].start = DeclaredTimerStart::OnSignal(SYNTHETIC_WAVE_TIMERS[1]);
            })
            .unwrap_err(),
            ObjectivesSchemaError::CollidingSignal {
                by: SYNTHETIC_WAVE_TIMERS[0],
                signal: SYNTHETIC_WAVE_TIMERS[1]
            }
        );
        // A move or a watch to a state nothing produces is dead the same way
        // an oversized required count is: `Hidden`, `Pending` and `Optional`
        // are reachable only from `Hidden`, which no declared action can
        // leave.
        assert_eq!(
            rebuild(&|_, c, _, _, _| {
                c[0].reaction = DeclaredCountReaction::SetObjectiveState {
                    objective: SYNTHETIC_PRIMARY,
                    state: DeclaredObjectiveState::Hidden,
                };
            })
            .unwrap_err(),
            ObjectivesSchemaError::DeadState {
                by: SYNTHETIC_PROTECTED_LOST,
                state: DeclaredObjectiveState::Hidden
            }
        );
        assert_eq!(
            rebuild(&|_, _, t, _, _| {
                t[4].action = DeclaredTimerAction::SetObjectiveState {
                    objective: SYNTHETIC_PRIMARY,
                    state: DeclaredObjectiveState::Pending,
                };
            })
            .unwrap_err(),
            ObjectivesSchemaError::DeadState {
                by: SYNTHETIC_DEADLINE,
                state: DeclaredObjectiveState::Pending
            }
        );
        assert_eq!(
            rebuild(&|o, _, _, _, _| {
                o[1].reveal = DeclaredRevealRule::OnObjectiveState {
                    objective: SYNTHETIC_PRIMARY,
                    state: DeclaredObjectiveState::Optional,
                };
            })
            .unwrap_err(),
            ObjectivesSchemaError::DeadState {
                by: SYNTHETIC_SECONDARY,
                state: DeclaredObjectiveState::Optional
            }
        );
    }
}
