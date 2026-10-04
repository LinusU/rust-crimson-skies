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
//!
//! # F39-E1: what the dormant/reveal declarations declare, and what they do not
//!
//! F39-D left one question open (its unknown #4): 1096 of the installation's
//! 1338 objective blocks carry [`OBJECTIVE_DORMANT_KEY`], 1335 carry an
//! `INACTIVE<n>` stage and 130 an [`OBJECTIVE_INACTIVE_COUNT_KEY`], and
//! nothing was known about what any of them *does*. The measurement half of
//! that question is here: [`measure_dormant_declarations`] reads the three
//! declarations plus the display [`OBJECTIVE_IDENTITY_KEY`] out of a decoded
//! objective record and keeps every value **as measured** — the `-1`
//! sentinel ([`DormantReading::Sentinel`]), a positive elapsed-time quantity in
//! an *unmeasured* unit ([`DormantReading::ElapsedTime`]), the conditions as
//! their measured subject/part/attribute spellings ([`InactiveCondition`]), and
//! the display identity as role/ordinal/message ([`MeasuredIdentity`]).
//!
//! It produces **no** [`DeclaredRevealRule`]. Recovering the rule needs an
//! observation of the original running, which no agent has; the inference, the
//! contrary hypotheses and the verification that would settle it are recorded
//! in
//! `docs/findings/2026-10-03-f39-e1-objective-dormant-reveal-lifecycle.md`, and
//! `DeclaredSupport::Original` stays unplayable for it. The reader also refuses
//! every declaration shape this stage did not measure
//! ([`DormantReadError`]), so an installation with a shape nobody has seen
//! fails loudly instead of reading as a block that declares nothing.
//!
//! # F39-E2: precedence between completion effects in one block
//!
//! The original declares, inside one `OBJECTIVE<N>` block, what happens to
//! *other* objectives when this one completes: [`BranchEffectKind`] is the
//! measured vocabulary of those keys, and [`MeasuredBranchPrecedence`] is what a
//! census measures about them — how many blocks declare more than one effect,
//! whether their target sets overlap, and the isolated conditions where two
//! effects name the same objective. The measured answer is that the corpus asks
//! the ordering question exactly once in 1338 blocks and answers it nowhere, so
//! [`UNMEASURED_BLOCK_PRECEDENCE`] is the verdict and
//! [`MeasuredObjectiveRecord`] carries the reading. The authored field order is
//! recorded as the only ordering the bytes carry and is measured *not* to be a
//! format invariant, so nothing may rank effects by it.
//!
//! # F39-E5: completion effects, and the one shape that is refused
//!
//! F39-E2 measured the *vocabulary* and the isolated conflict. What the engine
//! could not do with them is this stage's subject: the declared form now carries
//! them as a typed [`DeclaredCompletionEffect`] list on the declaring
//! [`DeclaredObjective`], over the measured [`BranchEffectKind`] vocabulary, and
//! `cs_sim::objectives::runtime` applies each one as a declared move on the
//! objective it names. Every such target names an objective of the **same
//! record** (all 1706 measured targets do, and none names its own block), so a
//! program symbol is the right identity and there is no cross-record name to
//! resolve. The nap's second number is kept as an [`UnmeasuredQuantity`] — a
//! number with **no unit**, because nothing measured what it measures.
//!
//! What each spelling *does* stays an inference from the spelling; the state the
//! engine moves the target to is designed vocabulary living in
//! `cs_sim::objectives::runtime::CompletionEffectKind`. Nothing here is an
//! original-fidelity claim, and [`DeclaredSupport::Original`] stays unplayable:
//! giving the engine a way to *say* what a record declares is not recovering what
//! it means.
//!
//! One shape is refused rather than resolved, by name at declaration: **two
//! different effects naming the same objective** in one program
//! ([`ObjectivesSchemaError::AmbiguousCompletionEffect`]). F39-E2 measured that
//! shape in exactly one of 1338 blocks (`zbd/c3/m05` `OBJECTIVE8` naming
//! objective 68) and could not say which of the two wins there. The declaration's
//! own field order cannot stand in for the answer either: the corpus writes
//! every conflicting pair **both** ways round (`WAKE`/`NAP` 99 vs 81,
//! `WAKE`/`KILL` 93 vs 20, `NAP`/`KILL` 70 vs 34), so the order is authored per
//! block and carries no engine intent — the same conclusion
//! [`UNMEASURED_BLOCK_PRECEDENCE`] records on the measurement side. Dead
//! declarations are refused with it: an effect on the objective that fires it
//! ([`ObjectivesSchemaError::SelfCompletionEffect`]), one naming a target born
//! finished ([`ObjectivesSchemaError::DeadCompletionEffect`]), one declared by an
//! objective that never completes
//! ([`ObjectivesSchemaError::UnfiredCompletionEffects`]) and one whose declared
//! number contradicts the measured shape
//! ([`ObjectivesSchemaError::EffectArgumentShape`]) — a nap without its number,
//! or a number on an effect that carries none.
//!
//! # F39-E4: which counter categories the original declares
//!
//! F39 non-negotiable behavior 2 asks counters to distinguish destroyed,
//! disabled, captured, escaped and despawned actors. F39-A/B/C/D all recorded
//! that two of the five — `Disabled` and `Escaped` — had no producer at all:
//! nothing in the engine reported them, and F39-D's category test had to use a
//! capture to show that a captured convoy does not satisfy a `Destroyed`
//! condition. F39-E4 put the question to the owner's installation and the answer
//! is **no declaration for either**:
//!
//! * the original's target records name two of the five as **localized
//!   objective kinds** — `MSG_OBJ_DESTROY` on 107 records and
//!   `MSG_OBJ_DISABLE`/`MSG_OBJ_DISABLEENG` on 13 (5 and 8) — and name no
//!   captured, escaped or despawned kind at all;
//! * the counted conditions the original *does* write (1335 `INACTIVE<n>` sites
//!   in 129 thresholded blocks, beside 130 completion-count thresholds) name
//!   **actor, part and part-state names**, 226 distinct spellings of which
//!   `healthy` (983) and `panels` (194) are the largest, and none of the 226
//!   names a category;
//! * the compiled mission program that would carry a counter is undecoded
//!   (F13-B/C, F38), so no opcode can be read either.
//!
//! A label is not a counted transition, so this measurement produces no
//! producer; what it produces is the gate. [`CountCategorySupport`] states per
//! category whether the project's own lifecycle vocabulary reports it,
//! [`DeclaredCountKind::declared_by_original`] states what the corpus spells,
//! [`UNMEASURED_COUNT_CATEGORY`] is the named verdict, and
//! [`ObjectivesSchemaError::UnmeasuredCountCategory`] refuses an **original**
//! record that counts a category the original never declares. The five-category
//! vocabulary stays design, is labelled design here and in
//! `cs_sim::objectives::counters`, and the finding records the measurement, so
//! the claim is gated rather than quietly over-declared.
//!
//! # F39-E6: a repeated completion-effect key is its own unmeasured shape
//!
//! F39-E2 measured precedence between two *different* effects in one block and
//! had to set one reading aside: a block that spells the **same**
//! completion-effect key twice. F39-E6 measured it over the whole readable
//! corpus — every mission-scoped `objectives.zrd` block, the shared reader
//! `zbd/zrdr.zbd` and the eight world-group readers that census excludes, and
//! every `targets.zrd` record — and found it **nowhere**, in 612 excluded
//! members and 332 target records alike. So the corpus holds no instance to
//! infer from, and its silence is not a rule: "the corpus never writes it" is
//! not evidence that the original does not support it. The shape therefore gets
//! its own named unknown, [`UNMEASURED_REPEATED_EFFECT_KEY`], carried by
//! [`MeasuredBranchPrecedence::repeated_effects`] beside — never folded into —
//! the conflict reading: a repeat asks what the *second site of one key* does
//! (replace the first, be ignored, apply beside it), a different question from
//! which of two different effects applies.
//!
//! The declared vocabulary refuses the residue by name with it: one objective
//! declaring the same effect on the same objective twice
//! ([`ObjectivesSchemaError::RepeatedCompletionEffect`]) is the declared form
//! of a repeated site and is refused rather than deduplicated into a count the
//! record never authorized. Two entries naming the same objective with the
//! same kind from *different* objectives stay legal — they agree on what
//! happens — and one kind naming two different objectives stays legal too:
//! that is one multi-target site, not a repetition.

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

impl DeclaredObjectiveState {
    /// Whether no further transition is legal, mirroring
    /// `cs_sim::objectives::state::ObjectiveState::is_final`.
    ///
    /// The schema uses it to refuse the declarations that could never fire
    /// because their objective is already finished: a watch waiting for a state
    /// no row leaves, and a completion effect naming such an objective.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Superseded)
    }
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

/// A number a nap declaration carried beside its objective list, with **no
/// measured unit**.
///
/// F39-E2 measured 417 such numbers over the installation — 42 distinct values
/// between 0.5 and 170 — and could not say what any of them measures; the verdict
/// is [`UNMEASURED_NAP_ARGUMENT`]. Nothing in this project has run the original,
/// and the program behind the record is not decoded, so a duration, a weight and
/// a threshold are all consistent with the bytes. The wrapper exists so that no
/// caller can read the value as a time: a non-finite value cannot be built at all
/// ([`UnmeasuredQuantity::new`] returns `None`), the schema refuses a declaration
/// whose number is missing where the measurement says one is there
/// ([`ObjectivesSchemaError::EffectArgumentShape`]), and the runtime carries the
/// number without ever interpreting it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnmeasuredQuantity(f64);

impl UnmeasuredQuantity {
    /// Wraps a finite number, or refuses a non-finite one.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    /// The number exactly as declared. It has no unit, and this accessor is the
    /// only way to read it back.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

/// What completing one declared objective does to one other objective of the same
/// program.
///
/// The measured record writes one *site* per key with a list of objective
/// numbers (one to twelve for `WAKE`/`KILL`, exactly one for a `NAP`), while
/// the engine acts on one objective at a time. So a site is modelled as one entry
/// per named objective — a six-target wake is six entries — and `objective` is
/// the declared identity of the block it names. F39-E2 measured that every target
/// names a block the *same* record declares, so no cross-record naming space
/// exists to express.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeclaredCompletionEffect {
    /// Which effect this is.
    pub kind: BranchEffectKind,
    /// The objective it acts on: a symbol of the same program.
    pub objective: ProgramSymbol,
    /// The number only [`BranchEffectKind::Nap`] carries.
    pub argument: Option<UnmeasuredQuantity>,
}

impl DeclaredCompletionEffect {
    /// Builds one effect. The number's *shape* is checked by
    /// [`DeclaredObjectiveProgram::try_new`], so this constructor only wraps the
    /// value.
    ///
    /// # Errors
    ///
    /// `None` when `value` is not finite; there is no number to declare.
    pub fn new(
        kind: BranchEffectKind,
        objective: ProgramSymbol,
        argument: Option<f64>,
    ) -> Option<Self> {
        Some(Self {
            kind,
            objective,
            argument: match argument {
                Some(value) => Some(UnmeasuredQuantity::new(value)?),
                None => None,
            },
        })
    }
}

/// The declared precedence resolving two terminal requests on one tick.
///
/// `SyntheticConservative` is the designed policy F39-B names: `Failure`
/// beats `Extraction` beats `Success`. The original game's rule is
/// unmeasured; a measured rule becomes a new variant, and a program whose
/// precedence is unrecovered carries [`Resolved::Unknown`] so the lowering
/// refuses it by name instead of picking one.
///
/// # The two precedence questions are not one (F39-E2)
///
/// This precedence resolves two **terminal outcome requests** — success,
/// extraction, failure — colliding on one tick. It does **not** resolve two
/// completion effects on one objective, which is a different question about a
/// different record: F39-E2 measured that corpus and found the original's answer
/// for it nowhere in the data (one instance in 1338 blocks, unresolved; see
/// [`MeasuredBranchPrecedence`] and [`UNMEASURED_BLOCK_PRECEDENCE`]). Nothing in
/// this module ranks `WAKE` against `NAP` against `KILL`, and adding a ranking
/// here would be the invented rule F39-E2 was asked to measure instead of
/// assume. The original has no measured precedence for either question, so this
/// variant stays designed and stays labelled synthetic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclaredPrecedence {
    /// `Failure` > `Extraction` > `Success`. Designed; synthetic only.
    SyntheticConservative,
}

/// Why an actor stopped counting as present, as declared.
///
/// The five categories are F39's required vocabulary (non-negotiable behavior 2)
/// and are **newly authored design**: F39-E4 measured the owner's installation
/// for a producer of each and found none for two of them, which is what
/// [`DeclaredCountKind::declared_by_original`] and
/// [`CountCategorySupport`] state and what
/// [`ObjectivesSchemaError::UnmeasuredCountCategory`] refuses.
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

impl DeclaredCountKind {
    /// Every category, in declaration order.
    ///
    /// The enumeration a consumer walks instead of matching on the type, so the
    /// five-category claim and every measurement over it stay exhaustive.
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [
            Self::Destroyed,
            Self::Disabled,
            Self::Captured,
            Self::Escaped,
            Self::Despawned,
        ]
    }

    /// The stable label used in refusals, reports and evidence.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Destroyed => "destroyed",
            Self::Disabled => "disabled",
            Self::Captured => "captured",
            Self::Escaped => "escaped",
            Self::Despawned => "despawned",
        }
    }

    /// Whether this project's own damage lifecycle vocabulary produces the
    /// category, measured over the declared half rather than assumed.
    ///
    /// `Destroyed`, `Captured` and `Despawned` mirror a
    /// [`cs_sim::damage::LifecycleKind`](https://docs.rs/cs_sim) transition;
    /// `Disabled` and `Escaped` do not, which is why they need
    /// [`Self::declared_by_original`] to be checked before a record may claim
    /// them.
    #[must_use]
    pub const fn support(self) -> CountCategorySupport {
        match self {
            Self::Destroyed | Self::Captured | Self::Despawned => CountCategorySupport::Lifecycle,
            Self::Disabled | Self::Escaped => CountCategorySupport::Unmeasured,
        }
    }

    /// Whether the original's own objective records **declare** this category.
    ///
    /// F39-E4's measured answer, per category (see
    /// `docs/findings/2026-10-04-f39-e4-count-category-producers.md`):
    ///
    /// | category | declaring sites | what the corpus writes |
    /// | --- | --- | --- |
    /// | `Destroyed` | 107 | `targets.zrd` objective kind `MSG_OBJ_DESTROY` |
    /// | `Disabled` | 13 | `MSG_OBJ_DISABLE` (5) and `MSG_OBJ_DISABLEENG` (8) |
    /// | `Captured` | 0 | — |
    /// | `Escaped` | 0 | — |
    /// | `Despawned` | 0 | — |
    ///
    /// Every one of those sites is a **localized label** on a target, and the
    /// original's counted conditions — 1335 `INACTIVE<n>` sites in 129
    /// thresholded blocks, beside 130 `INACTIVE_COMPLETION_COUNT` thresholds —
    /// name actor, part and part-state names instead (226 distinct spellings,
    /// `healthy` 983 and `panels` 194 the largest), none of which names a
    /// category. So `true` here means **the records spell this category**, never
    /// that the engine's counter has a producer: the
    /// compiled mission program that would carry a counter is undecoded
    /// (F13-B/C, F38). The retail test re-measures every row of the table above
    /// and fails if this answer drifts from the corpus.
    #[must_use]
    pub const fn declared_by_original(self) -> bool {
        matches!(self, Self::Destroyed | Self::Disabled)
    }

    /// The declared **name stem** a measured spelling is recognised by.
    ///
    /// The one rule by which a name the original's records carry is classified
    /// into a counter category: the name, compared case-insensitively, must
    /// *begin with* this stem. The stems are stems and not whole words because
    /// the corpus spells two of the categories as prefixes of a longer label —
    /// `MSG_OBJ_DISABLE` and `MSG_OBJ_DISABLEENG` — and a whole-word rule would
    /// miss both.
    ///
    /// **This is a vocabulary match, not a decoded behaviour.** It bounds which
    /// spellings exist; it never says what a declaration does, and a match on
    /// its own is not a producer (see [`UNMEASURED_COUNT_CATEGORY`]).
    #[must_use]
    pub const fn name_stem(self) -> &'static str {
        match self {
            Self::Destroyed => "DESTROY",
            Self::Disabled => "DISABL",
            Self::Captured => "CAPTUR",
            Self::Escaped => "ESCAP",
            Self::Despawned => "DESPAWN",
        }
    }

    /// Whether a measured spelling names this category under
    /// [`Self::name_stem`].
    ///
    /// The single place the classification is applied, so a census and a report
    /// cannot classify the same name two ways.
    ///
    /// The rule is **segment-based and case-insensitive**: the name is split on
    /// `_` and a category claims it when one of its segments *begins with* the
    /// category's stem. Measured labels are prefixed (`MSG_OBJ_DISABLE`, a
    /// segment of its own), so a whole-string prefix rule would miss every one
    /// of them, and the measured part-state spellings (`healthy`, `panels`,
    /// `healthy_part`, …) contain no segment that starts with any of the five
    /// stems.
    #[must_use]
    pub fn names_spelling(spelling: &str) -> Option<Self> {
        spelling
            .split('_')
            .filter_map(|segment| {
                Self::all()
                    .into_iter()
                    .find(|kind| segment.to_ascii_uppercase().starts_with(kind.name_stem()))
            })
            .next()
    }
}

impl fmt::Display for DeclaredCountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Where a declared count category's producer comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CountCategorySupport {
    /// The project's own damage lifecycle vocabulary reports this category
    /// (`cs_sim::objectives::counters::CountKind::from_lifecycle`), so an
    /// authored session can be handed the count.
    Lifecycle,
    /// Nothing measured reports this category: the original declares no such
    /// actor-count declaration and no subsystem produces one, so only a caller
    /// that invents a report could count it.
    Unmeasured,
}

impl CountCategorySupport {
    /// Whether a session can be handed this category by a measured producer.
    #[must_use]
    pub const fn is_measured(self) -> bool {
        matches!(self, Self::Lifecycle)
    }
}

/// Why a counter category stays unproduced, stated once so the schema, the
/// lowering, the census and the finding say the same thing.
///
/// It names the measured fact behind it: over the owner's installation the
/// original's objective records declare no actor-count category for a disabled
/// or escaped actor, and the compiled program that would carry the counter is
/// not decoded, so the engine has no source for either count.
pub const UNMEASURED_COUNT_CATEGORY: &str = "the mission's objective records were measured for the actor-count categories \
     they declare: they name a destroy kind and a disable kind as localized target labels, and none names a captured, \
     escaped or despawned category, while the counted conditions themselves name part states rather than actor \
     end-states; the compiled program that would carry a counter is not decoded, so a disabled or escaped count has no \
     measured producer and an original record must not claim one";

/// Why a counter category stays **undeclared** by the original, stated once so
/// the schema, the census and the finding say the same thing.
///
/// It names the measured fact behind it, **with its bound**: no name either of
/// the original's measured objective surfaces writes — its counted conditions'
/// part states or its targets' localized labels — spells a captured, escaped or
/// despawned category. The measurement covers the mission-scoped reader
/// archives; the shared and world-group readers are outside that census
/// (F39-D unknown #5), which is why the refusal says "no … measured declaring
/// it" and not "the original has no such category".
pub const UNDECLARED_COUNT_CATEGORY: &str = "no counted condition or target label in the mission-scoped objective \
     records measured over the owner's installation names this actor-count category — the whole measured vocabulary is \
     published by the census, and the only spellings in it that name a category are a destroy kind and two disable kinds \
     — so no original mission record has been measured declaring it and an original record must not claim one; the \
     shared and world-group readers are outside that census (F39-D unknown #5)";

/// Why an **original** record may not carry a count condition in `kind`, or
/// `None` when it may.
///
/// The single place F39-E4's gate is decided, from two separately measured
/// facts and their two named verdicts:
///
/// 1. the original's own records must **spell** the category
///    ([`DeclaredCountKind::declared_by_original`], else
///    [`UNDECLARED_COUNT_CATEGORY`]); and
/// 2. something must be able to **report** it
///    ([`CountCategorySupport::is_measured`], else
///    [`UNMEASURED_COUNT_CATEGORY`]).
///
/// On the owner's installation exactly one category clears both — `Destroyed` —
/// so an original record may count destroyed actors and nothing else. A
/// **newly authored** record is never refused here: all five categories are
/// design, which is what its origin says it is.
#[must_use]
pub const fn original_count_category_refusal(kind: DeclaredCountKind) -> Option<&'static str> {
    if !kind.declared_by_original() {
        Some(UNDECLARED_COUNT_CATEGORY)
    } else if !kind.support().is_measured() {
        Some(UNMEASURED_COUNT_CATEGORY)
    } else {
        None
    }
}

/// What a measured corpus declares about one counter category.
///
/// The **sites** a census counted that name `kind` under
/// [`DeclaredCountKind::name_stem`], with the measured spellings that matched.
/// An empty [`Self::names`] is the measured statement *"nothing in this surface
/// spells this category"* — a bound on the vocabulary, never a claim about what
/// the engine does with an actor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasuredCategoryEvidence {
    /// How many measured sites name the category, across both surfaces.
    pub sites: u32,
    /// The measured spellings that named it, sorted, with their site counts.
    pub names: Vec<(String, u32)>,
}

impl MeasuredCategoryEvidence {
    /// Whether the measured corpus names this category at all.
    ///
    /// The reading [`DeclaredCountKind::declared_by_original`] must agree with:
    /// the retail test measures every category over the owner's installation and
    /// compares, so the declared answer cannot drift from the corpus unnoticed.
    #[must_use]
    pub fn is_declared(&self) -> bool {
        !self.names.is_empty()
    }
}

/// F39-E4's reading of one record's **counted conditions**: the `INACTIVE<n>`
/// sites an `OBJECTIVE<N>` block declares beside its
/// [`OBJECTIVE_INACTIVE_COUNT_KEY`] threshold.
///
/// A census of **names and shapes**, deliberately not a rule: every number says
/// how many measured sites a block declares and what they are spelled, never
/// what reaching a named state means — the counted condition's *semantics* are
/// the part F39-E1 could not recover, and the compiled program behind the record
/// is undecoded (F13-B/C).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasuredCountConditions {
    /// How many `INACTIVE<n>` sites the record declares.
    pub stage_sites: u32,
    /// How many [`OBJECTIVE_INACTIVE_COUNT_KEY`] thresholds the record declares.
    pub threshold_sites: u32,
    /// How many blocks carry both a threshold and at least one stage.
    ///
    /// Blocks, not sites: the threshold is the count and the stages are what it
    /// counts, so a consumer asking "how many counted conditions are there?"
    /// has to be answered in blocks.
    pub thresholded_blocks: u32,
    /// How many stage sites carry exactly `N` names, keyed by `N`.
    ///
    /// Measured `1 → 35`, `2 → 356`, `3 → 944`: the corpus writes one, two and
    /// three names per site and nothing else, so a fourth shape is a discovery
    /// rather than a variant this reading has already seen.
    pub shapes: BTreeMap<u32, u32>,
    /// The measured name spellings of every stage site, sorted, with the number
    /// of sites carrying each. **Every** name a site carries is counted, not
    /// only its last one, so "no site names a category" is the strongest
    /// statement this reading supports.
    pub names: BTreeMap<String, u32>,
}

impl MeasuredCountConditions {
    /// What this record's counted conditions declare about one category: the
    /// measured sites whose names match [`DeclaredCountKind::name_stem`].
    #[must_use]
    pub fn evidence(&self, kind: DeclaredCountKind) -> MeasuredCategoryEvidence {
        measured_category_evidence(&self.names, kind)
    }
}

/// F39-E4's reading of one mission's **objective targets**: the localized kind
/// and category each `targets.zrd` record carries.
///
/// The surface that names *what must happen to an actor* — `MSG_OBJ_DESTROY`
/// reads as "destroy this", `MSG_OBJ_DISABLE`/`MSG_OBJ_DISABLEENG` as "disable
/// this" — and therefore the only place in the objective records where a
/// counter category could be spelled. It is a census of **localized label
/// names**: resolving one to its displayed text is F12/F51's string catalog, and
/// what a label means at runtime is unmeasured.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasuredTargetKinds {
    /// How many target records the member declares.
    pub records: u32,
    /// How many of them carry a `help_label` naming an objective kind.
    pub labelled: u32,
    /// Every measured label spelling — the union of the records' `help_label`
    /// and `category_label` values — sorted, with the number of records carrying
    /// each.
    pub names: BTreeMap<String, u32>,
}

impl MeasuredTargetKinds {
    /// What this mission's target records declare about one category: the
    /// measured labels whose spelling matches [`DeclaredCountKind::name_stem`].
    #[must_use]
    pub fn evidence(&self, kind: DeclaredCountKind) -> MeasuredCategoryEvidence {
        measured_category_evidence(&self.names, kind)
    }
}

/// Classifies one measured name vocabulary into a counter category.
///
/// The one place a vocabulary is classified, so the census, the gate and the
/// tests cannot classify a spelling two different ways. Public because a census
/// spanning **both** measured surfaces (a mission's counted conditions *and* its
/// target labels) has to merge them and classify the union.
#[must_use]
pub fn measured_category_evidence(
    names: &BTreeMap<String, u32>,
    kind: DeclaredCountKind,
) -> MeasuredCategoryEvidence {
    let names: Vec<(String, u32)> = names
        .iter()
        .filter(|(name, _)| DeclaredCountKind::names_spelling(name) == Some(kind))
        .map(|(name, count)| ((*name).clone(), *count))
        .collect();
    MeasuredCategoryEvidence {
        sites: names.iter().map(|(_, count)| *count).sum(),
        names,
    }
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
    /// What completing it does to *other* objectives, in authored order. Empty
    /// for an objective whose completion moves nothing else, which is the whole
    /// of the behaviour before completion effects existed.
    ///
    /// `on_complete` and this list answer two different questions, and both stay
    /// apart: `on_complete` is what the completion means for the **mission**
    /// (a terminal outcome request), while these are what it does to other
    /// **objectives** (a declared move on each one it names).
    pub completion_effects: Vec<DeclaredCompletionEffect>,
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
///
/// `Eq` is not derived: it carries a [`MeasuredObjectiveRecord`], which F39-E2's
/// per-block reading makes a float-carrying value.
#[derive(Clone, Debug, PartialEq)]
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
///
/// `Eq` is deliberately **not** derived: F39-E2's per-block reading carries the
/// measured numbers beside the completion-effect targets, and a float is not
/// `Eq`.
#[derive(Clone, Debug, PartialEq)]
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
    /// What F39-E2 measured of the **order** between two completion effects in
    /// one block: which blocks declare more than one effect, whether their
    /// targets overlap, and the isolated conditions where they do.
    ///
    /// Not optional: a record that carries no per-block reading has not measured
    /// the one thing F39-E2 was asked to measure, and a default of "no
    /// conflict" would assert that the original's corpus is unambiguous.
    pub branch_precedence: MeasuredBranchPrecedence,
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

/// The measured **completion-effect** keys: the part of
/// [`BRANCH_KEY_VOCABULARY`] that declares what happens to *another* objective
/// when this one completes. F39-E2 separated this family from
/// [`BRANCH_ORDER_KEY`] because they answer different questions: an effect names
/// a target, while an order key names an objective this one is sequenced behind.
///
/// Measured over the owner's installation (F39-E2), reusing F39-D's census of the
/// same 53 mission readers and 1338 blocks: 412 `WAKE`, 417 `NAP`, 225 `KILL` and
/// 2 `WAKEUP` sites, 1056 in all. The four spellings are **measured**; what any
/// of them does is an **inference from the spelling** and stays one.
pub const BRANCH_EFFECT_KEY_VOCABULARY: [&str; 4] = [
    "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
    "NAP_OBJECTIVE_WHEN_I_COMPLETE",
    "KILL_OBJECTIVE_WHEN_I_COMPLETE",
    "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE",
];

/// The measured **order-dependency** key, the rest of
/// [`BRANCH_KEY_VOCABULARY`]: measured 35 times, always with a single integer
/// naming another objective of the same mission. It is not a completion effect
/// and never counts as one.
pub const BRANCH_ORDER_KEY: &str = "TICK_DEPENDS_ON_OBJ";

/// The measured completion-effect key that names **several** objectives in one
/// site, beside the NAP key that carries a second number.
///
/// Measured (F39-E2): `WAKE` and `KILL` sites hold a list of one to twelve
/// integers and nothing else; every one of the 417 `NAP` sites holds exactly one
/// integer followed by exactly one float. **What the float is** — a duration, a
/// weight, a threshold — is **unmeasured**: no original executable has been run
/// and the program behind the record is not decoded. It is therefore carried as
/// an unnamed number, never as a unit.
pub const BRANCH_EFFECT_MULTI_TARGET_KEYS: [&str; 2] = [
    "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
    "KILL_OBJECTIVE_WHEN_I_COMPLETE",
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

/// One measured **completion effect**: a key an objective block carries to
/// declare what happens to *another* objective when this one completes.
///
/// The four spellings are measured (see [`BRANCH_EFFECT_KEY_VOCABULARY`]) and
/// this enum is exactly that set — a closed vocabulary, never an open one. It
/// deliberately keeps `WAKEUP` apart from `WAKE`: the original writes both
/// spellings in the same corpus (`c1b/m03` `OBJECTIVE13` carries one of each), so
/// normalising one into the other would assert a difference or an identity the
/// data does not show. What each effect *does* is an inference from its spelling
/// and is labelled as one everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BranchEffectKind {
    /// `WAKE_OBJECTIVE_WHEN_I_COMPLETE`: 412 measured sites.
    Wake,
    /// `NAP_OBJECTIVE_WHEN_I_COMPLETE`: 417 measured sites, each carrying a
    /// second number whose meaning is unmeasured.
    Nap,
    /// `KILL_OBJECTIVE_WHEN_I_COMPLETE`: 225 measured sites.
    Kill,
    /// `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`: 2 measured sites.
    Wakeup,
}

impl BranchEffectKind {
    /// The measured key this effect is spelled as.
    ///
    /// The inverse of [`Self::from_measured_key`] for every variant, so the
    /// vocabulary and this enum cannot drift apart.
    #[must_use]
    pub const fn measured_key(self) -> &'static str {
        match self {
            Self::Wake => "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
            Self::Nap => "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            Self::Kill => "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            Self::Wakeup => "WAKEUP_OBJECTIVE_WHEN_I_COMPLETE",
        }
    }

    /// The short stable label used in diagnostics, evidence and conflict keys
    /// (`"WAKE"`, `"NAP"`, `"KILL"`, `"WAKEUP"`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wake => "WAKE",
            Self::Nap => "NAP",
            Self::Kill => "KILL",
            Self::Wakeup => "WAKEUP",
        }
    }

    /// Whether `key` is one of the four measured completion-effect spellings.
    ///
    /// Only those four. `TICK_DEPENDS_ON_OBJ` is a measured order dependency and
    /// returns `None` here, so an order key can never be counted as an effect or
    /// make two "effects" that never collide.
    #[must_use]
    pub fn from_measured_key(key: &str) -> Option<Self> {
        BRANCH_EFFECT_KEY_VOCABULARY
            .iter()
            .find(|measured| **measured == key)
            .and_then(|measured| {
                [Self::Wake, Self::Nap, Self::Kill, Self::Wakeup]
                    .into_iter()
                    .find(|kind| kind.measured_key() == *measured)
            })
    }

    /// Every measured effect, in [`Self`] order.
    ///
    /// The enumeration a consumer walks instead of the key list, so a new
    /// variant cannot be added without every consumer seeing it.
    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Wake, Self::Nap, Self::Kill, Self::Wakeup]
    }

    /// Whether a declaration of this effect carries the measured number beside
    /// its objective list.
    ///
    /// F39-E2 measured that only a nap does: all 417 measured `NAP` sites hold
    /// exactly one integer followed by exactly one number, and no `WAKE`, `KILL`
    /// or `WAKEUP` site carries one. The declared form uses this to refuse a
    /// declaration whose number is missing where the measurement says one is
    /// there — and **what** the number measures stays unmeasured
    /// ([`UNMEASURED_NAP_ARGUMENT`]), so it is carried, never interpreted.
    #[must_use]
    pub const fn carries_argument(self) -> bool {
        matches!(self, Self::Nap)
    }
}

impl fmt::Display for BranchEffectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One measured completion-effect site: a measured key inside one
/// `OBJECTIVE<N>` block, the objective numbers it names and the numbers that are
/// **not** objective numbers.
///
/// A **site**, not a rule. `targets` are the measured integers of the key's
/// value list and `arguments` the measured floats beside them; whether a target
/// is an objective *number* is F39-E2's inference from the corpus (all 1706
/// measured targets name a block the same mission declares, and none names its
/// own block), and the argument list is deliberately unnamed because the
/// original does not say what it measures.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredBranchSite {
    /// Which measured completion effect this site declares.
    pub kind: BranchEffectKind,
    /// The measured integers of the value, in the order the record spells them.
    pub targets: Vec<u32>,
    /// The measured non-integer numbers of the value, in order.
    pub arguments: Vec<f32>,
}

impl MeasuredBranchSite {
    /// Whether this site names `target`.
    #[must_use]
    pub fn names(&self, target: u32) -> bool {
        self.targets.contains(&target)
    }
}

/// One block in which two measured completion effects name the **same**
/// objective: the only shape in which "in which order do they take effect" has a
/// answer, and the shape the corpus contains exactly once (F39-E2).
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredBranchConflict {
    /// The `OBJECTIVE<N>` block the conflict sits in, as the record spells it.
    pub block: String,
    /// The objective number the conflicting sites both name.
    pub target: u32,
    /// The conflicting sites, in the block's **authored field order** — the only
    /// ordering the bytes carry, and measured to be authored per block rather
    /// than fixed by the format (see [`MeasuredBranchPrecedence::declared_order`]),
    /// so it is recorded and never read as a precedence rule.
    pub sites: Vec<MeasuredBranchSite>,
}

impl MeasuredBranchConflict {
    /// The conflicting effects' short labels, in authored order.
    #[must_use]
    pub fn effect_labels(&self) -> Vec<&'static str> {
        self.sites.iter().map(|site| site.kind.label()).collect()
    }

    /// The conflicting effects' measured keys, sorted, joined with `+`.
    ///
    /// A canonical combination key, so `NAP` before `WAKE` and `WAKE` before
    /// `NAP` are counted as the same combination. The `BTreeSet` does both jobs —
    /// it drops a kind a block spells twice and yields `BranchEffectKind` order,
    /// which is the enum's own declaration order.
    #[must_use]
    pub fn combination(&self) -> String {
        self.sites
            .iter()
            .map(|site| site.kind)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|kind| kind.label())
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// One measured **repeated effect key**: an `OBJECTIVE<N>` block that spells
/// the same completion-effect key two or more times (F39-E6).
///
/// A different unmeasured shape from [`MeasuredBranchConflict`], and kept
/// deliberately separate: a conflict asks which of two *different* effects the
/// original applies to one target, while a repeat asks what the **second site
/// of one key** does — replace the first site, be ignored, or apply beside it.
/// The measured corpus holds the conflict once and the repeat nowhere, so the
/// repeat is its own named unknown ([`UNMEASURED_REPEATED_EFFECT_KEY`]) and is
/// never folded into the conflict reading or silently deduplicated into "two
/// sites, one effect".
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredRepeatedEffect {
    /// The `OBJECTIVE<N>` block the repeat was measured in, as the record
    /// spells it.
    pub block: String,
    /// The one kind the block spells more than once.
    pub kind: BranchEffectKind,
    /// Every site of `kind` in the block, in authored field order — each with
    /// its own target list and argument list, so a repeat whose sites spell
    /// different targets or different numbers keeps both spellings.
    pub sites: Vec<MeasuredBranchSite>,
}

impl MeasuredRepeatedEffect {
    /// `"OBJECTIVE_8: WAKE x2"` — display label for diagnostics.
    #[must_use]
    pub fn label(&self) -> String {
        format!(
            "{}: {} x{}",
            self.block,
            self.kind.label(),
            self.sites.len()
        )
    }
}

/// What F39-E2 measured of one record's per-block completion-effect precedence.
///
/// A census of **declaration sites and their targets**, deliberately not a rule.
/// The question `docs/contracts/SCRIPT-MISSION.md` demands be measured rather
/// than assumed — *in which order do two completion effects on the same
/// objective take effect* — is measured here as far as the data allows, and the
/// answer is that the corpus gives it **one** instance and resolves **none** of
/// it (see [`UNMEASURED_BLOCK_PRECEDENCE`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeasuredBranchPrecedence {
    /// How many numbered `OBJECTIVE<N>` blocks the record declares.
    pub blocks: u32,
    /// How many blocks declare at least one completion effect.
    pub effect_blocks: u32,
    /// How many completion-effect sites the record declares in all.
    pub effect_sites: u32,
    /// How many sites carry a number that is not an objective number.
    pub argument_sites: u32,
    /// How many blocks declare two or more **different** effects.
    pub multi_effect_blocks: u32,
    /// How many of those blocks name disjoint target sets, so no two of their
    /// effects can act on the same objective and no ordering question arises.
    pub disjoint_multi_effect_blocks: u32,
    /// Objective numbers the completion-effect sites name in all, sites included.
    pub targets: u32,
    /// Sites whose target list names the site's **own** block.
    pub self_referencing_sites: u32,
    /// Sites naming an objective number the record does **not** declare.
    ///
    /// Measured zero over the owner's installation: a target is another objective
    /// of the same mission, so the corpus needs no cross-record namespace the
    /// engine would have to invent.
    pub dangling_sites: u32,
    /// The largest target list any site carries (measured `12`).
    pub widest_site: u32,
    /// How many multi-effect blocks spell one effect **before** another, as the
    /// ordered pairs of [`BranchEffectKind`], aggregated over the record.
    ///
    /// This is the corpus's *declared* order, and it is the measurement that
    /// keeps the reading honest: measured over the installation, **both**
    /// directions of every pair the corpus writes more than once occur, so the
    /// order a block spells its effects in is authored per block and is not a
    /// property of the format. Nothing may rank two effects by it — a format
    /// order would carry no engine intent either, and this order varies.
    pub authored_orders: BTreeMap<(BranchEffectKind, BranchEffectKind), u32>,
    /// The blocks where two effects *do* name a common objective, with the
    /// shared objective and the authored order of the sites.
    pub conflicts: Vec<MeasuredBranchConflict>,
    /// The blocks where one completion-effect key is spelled **two or more
    /// times**, with the kind and every one of its sites in authored order.
    ///
    /// Measured nowhere in the corpus (F39-E6): a different unmeasured shape
    /// from [`Self::conflicts`], so it is carried as its own named verdict
    /// ([`UNMEASURED_REPEATED_EFFECT_KEY`]) and never counted as a conflict —
    /// a repeat is not "two effects for one objective", it is the same effect
    /// declared twice.
    pub repeated_effects: Vec<MeasuredRepeatedEffect>,
}

impl MeasuredBranchPrecedence {
    /// Whether the record needs an ordering rule between completion effects that
    /// is not measured.
    ///
    /// The single place that answer is given, so no consumer decides it from the
    /// counts instead of from the isolated conditions.
    #[must_use]
    pub fn needs_unmeasured_order(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// How many **blocks** of this record carry at least one conflict.
    ///
    /// Blocks, not [`Self::conflicts`]: one block can name two objectives in
    /// common, so a count of conflicts is not a count of blocks, and a consumer
    /// asking "how many blocks must I handle?" has to be answered in blocks.
    #[must_use]
    pub fn conflicting_blocks(&self) -> u32 {
        self.conflicts
            .iter()
            .map(|conflict| conflict.block.as_str())
            .collect::<BTreeSet<_>>()
            .len() as u32
    }

    /// Whether every measured completion-effect target names another block of the
    /// same record.
    ///
    /// The measured closure fact, and the reason the engine needs no external
    /// naming space for a branch target: an objective number indexes the record's
    /// own numbered blocks. A `false` is a real reading, not a defect to be
    /// papered over — it would mean a target the record does not declare.
    #[must_use]
    pub fn is_closed_over_its_record(&self) -> bool {
        self.self_referencing_sites == 0 && self.dangling_sites == 0
    }

    /// Whether the record declares more than one effect for one event at all.
    ///
    /// Distinct from [`Self::needs_unmeasured_order`]: most multi-effect blocks
    /// name disjoint targets and raise no ordering question whatever rule the
    /// original uses.
    #[must_use]
    pub fn declares_multi_effect_blocks(&self) -> bool {
        self.multi_effect_blocks > 0
    }

    /// How many blocks spell `first` before `second`, and how many spell it the
    /// other way round.
    ///
    /// The declared-order measurement in queryable form. `first > 0 && second > 0`
    /// means the corpus does **not** impose an order on the pair, which is what
    /// rules reading the field order as the original's precedence.
    #[must_use]
    pub fn declared_order(&self, first: BranchEffectKind, second: BranchEffectKind) -> (u32, u32) {
        (
            self.authored_orders
                .get(&(first, second))
                .copied()
                .unwrap_or(0),
            self.authored_orders
                .get(&(second, first))
                .copied()
                .unwrap_or(0),
        )
    }

    /// The measured completion-effect combination of every conflict, sorted, with
    /// how many conflicts carry it (`"KILL+NAP"`, `"WAKE+NAP"`, …).
    #[must_use]
    pub fn conflict_combinations(&self) -> Vec<(String, u32)> {
        let mut counts: BTreeMap<String, u32> = BTreeMap::new();
        for conflict in &self.conflicts {
            *counts.entry(conflict.combination()).or_insert(0) += 1;
        }
        counts.into_iter().collect()
    }

    /// Why this record needs an ordering rule it does not have, by name, or
    /// `None` when it raises no ordering question at all.
    ///
    /// The one place the verdict is stated, so a refusal or a report carries the
    /// same words as the schema and the finding.
    #[must_use]
    pub fn unmeasured_order_reason(&self) -> Option<&'static str> {
        self.needs_unmeasured_order()
            .then_some(UNMEASURED_BLOCK_PRECEDENCE)
    }

    /// Whether any block of the record spells one completion-effect key two or
    /// more times — a different unmeasured shape from
    /// [`Self::needs_unmeasured_order`].
    ///
    /// The single place that answer is given, so no consumer reads a repeat as
    /// "one effect, two sites" and deduplicates what the record never
    /// authorized deduplicating.
    #[must_use]
    pub fn needs_unmeasured_repeated_effect(&self) -> bool {
        !self.repeated_effects.is_empty()
    }

    /// How many **blocks** of this record carry at least one repeated effect
    /// key.
    ///
    /// Blocks, not [`Self::repeated_effects`]: one block can spell two
    /// different keys twice each, so a count of repeats is not a count of
    /// blocks, and a consumer asking "how many blocks must I handle?" has to be
    /// answered in blocks.
    #[must_use]
    pub fn repeated_effect_blocks(&self) -> u32 {
        self.repeated_effects
            .iter()
            .map(|repeated| repeated.block.as_str())
            .collect::<BTreeSet<_>>()
            .len() as u32
    }

    /// Why a record spelling one completion-effect key twice is unresolved, by
    /// name, or `None` when no block does.
    ///
    /// A separate verdict from [`Self::unmeasured_order_reason`]: the question
    /// is what the second site of *one* key does, not which of two different
    /// effects applies, and a record can carry both shapes in different blocks.
    #[must_use]
    pub fn unmeasured_repeated_effect_reason(&self) -> Option<&'static str> {
        self.needs_unmeasured_repeated_effect()
            .then_some(UNMEASURED_REPEATED_EFFECT_KEY)
    }
}

/// Why the original's per-block completion-effect order stays unmeasured, stated
/// once so the census, the record and the finding say the same thing.
///
/// It names the measured fact behind it: the corpus declares two or more effects
/// on one objective in a single block exactly once across 1338 blocks, and the
/// bytes cannot say which of the two the original applies.
pub const UNMEASURED_BLOCK_PRECEDENCE: &str = "the record's completion-effect sites were measured per block, with their targets and \
     the order the block spells them in, and the corpus declares two effects for one objective in exactly one of its \
     blocks; which of them the original applies, and in which order, is unmeasured because the compiled program behind \
     the record is not decoded, so a session must not apply an authored field order as if it were a rule";

/// Why a block spelling one completion-effect key twice stays unmeasured,
/// stated once so the census, the record and the finding say the same thing
/// (F39-E6).
///
/// It names the measured fact behind it: the shape was counted over the whole
/// readable corpus — 1338 mission `OBJECTIVE<N>` blocks, 612 members of the
/// shared and world-group readers, 332 `targets.zrd` records — and occurs
/// **nowhere**, so there is no instance to infer from and the corpus's silence
/// is not evidence the original refuses it. A second site of one key may
/// replace the first, be ignored, or apply beside it; which is unmeasured, so
/// an importer must not collapse the repeat into a deduplicated count.
pub const UNMEASURED_REPEATED_EFFECT_KEY: &str = "a block spelling the same completion-effect key twice was measured over \
     the whole readable corpus and occurs nowhere: 1338 mission blocks, the shared and world-group reader archives \
     and every targets.zrd record spell it zero times; what a second site of one key does — replace the first, be \
     ignored, or apply beside it — is unmeasured because the compiled program behind the record is not decoded and \
     the corpus's silence is not a rule, so a repeated key must not be deduplicated into one effect";

/// Why the number a nap site carries stays unmeasured, stated once so the
/// declared record, the runtime and a report say the same thing.
///
/// It names the measured fact behind it: every one of the 417 measured nap sites
/// holds exactly one number after its objective list and no other effect site
/// holds one, and 42 distinct values occur — but the bytes do not say what it
/// measures, so the number is carried as an [`UnmeasuredQuantity`] with no unit
/// and is never interpreted.
pub const UNMEASURED_NAP_ARGUMENT: &str = "the record's nap sites were measured and each carries one number beside its \
     objective list, while no other completion-effect site carries one; what that number measures, in what unit and \
     over what domain is unmeasured because the compiled program behind the record is not decoded, so it is carried \
     as a plain number and nothing may schedule, compare or weigh anything on it";

/// The measured key names an objective block uses to declare an outcome.
///
/// Measured over the same corpus: `INSTANTWIN` occurs 15 times and
/// `INSTANTLOSS` 9 times across 53 mission readers. Which outcome each one
/// means, and what ends the mission, is **not** measured.
pub const FAILURE_KEY_VOCABULARY: [&str; 2] = ["INSTANTWIN", "INSTANTLOSS"];

/// Whether a measured key is one of the optionality declarations.
///
/// [`OBJECTIVE_INACTIVE_COUNT_KEY`] is matched exactly; a stage key is
/// `INACTIVE` followed by the stage number (see
/// [`is_objective_inactive_stage`]). Exact matching keeps
/// `INACTIVE_COMPLETION_COUNT` from reading as a stage and keeps a key this
/// stage never saw — `INACTIVATED`, `INACTIVE_A` — out of the count.
#[must_use]
pub fn is_optional_objective_key(key: &str) -> bool {
    key == OBJECTIVE_INACTIVE_COUNT_KEY || is_objective_inactive_stage(key)
}

/// Whether a measured key is one **stage** of an objective block's counted
/// condition: `INACTIVE` followed by the stage number.
///
/// Split out of [`is_optional_objective_key`] for F39-E4, which counts the
/// stages and the threshold separately: they are different roles in one
/// declaration, and a walk that could not tell them apart would report a count
/// it cannot read.
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
pub fn is_objective_inactive_stage(key: &str) -> bool {
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
    /// A declared move or watch targets a state nothing of *its* kind produces.
    ///
    /// No legal transition a **deadline action or count reaction** can perform
    /// reaches `Hidden`, `Pending` or `Optional` — the rows into `Hidden` and
    /// `Pending` start from `Hidden`, which only the reveal rule may leave, and
    /// the rows into `Optional` are the ones a *completion effect* uses
    /// (`Pending -> Optional`, `Active -> Optional`, F39-E5). So the declaration
    /// could never apply or fire: dead, refused at declaration. Naming the real
    /// reason matters, because the transition table itself does reach
    /// `Optional`: what cannot reach it is a deadline or a count, which has no
    /// "when I complete" to hang a set-aside on.
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
    /// Two **different** completion effects name the same objective in one
    /// program.
    ///
    /// F39-E2 measured this shape in exactly one of 1338 original blocks
    /// (`zbd/c3/m05` `OBJECTIVE8`, a `WAKE` and a `NAP` both naming objective
    /// 68) and could not say which of them wins there: the record's own field
    /// order is refuted as a rule by the same corpus, which writes every
    /// conflicting pair both ways round. So the declaration is refused by name
    /// rather than resolved by an invented order.
    AmbiguousCompletionEffect {
        /// The objective whose declaration carries the second effect.
        by: ProgramSymbol,
        /// The objective both effects name.
        objective: ProgramSymbol,
        /// The effect the schema met first, walking the program's objectives in
        /// declaration order and each objective's effects in authored order.
        ///
        /// Reported as **data about the declaration**, not as a rule: the
        /// refusal is about the shape, so which of the two the message calls
        /// `first` never decides anything — declaring them the other way round
        /// is refused by the same rule, with the two swapped.
        first: BranchEffectKind,
        /// The effect the schema met second, on the same terms as `first`.
        second: BranchEffectKind,
    },
    /// One objective declares the same completion effect on the same
    /// objective **twice**.
    ///
    /// This is the declared form of the repeated-key shape F39-E6 measured:
    /// an `OBJECTIVE<N>` block spelling one completion-effect key twice. The
    /// shape occurs nowhere in the readable corpus, so what the second site
    /// does — replace the first, be ignored, or apply beside it — is
    /// unmeasured and the declaration is refused by name
    /// ([`UNMEASURED_REPEATED_EFFECT_KEY`]) rather than deduplicated into a
    /// count the record never authorized.
    ///
    /// Deliberately narrower than "one objective spelling one kind twice":
    /// `WAKE` naming objectives 2 and 3 in two declared entries is the residue
    /// of **one** multi-target site as much as of a repeated key, so it stays
    /// legal. The refusal is per (kind, objective) pair of one declaring
    /// objective — the shape that can only have come from a repeated site.
    /// Two *different* objectives declaring the same `(kind, objective)` pair
    /// also stay legal: they agree on what happens, so there is no repeat to
    /// resolve.
    RepeatedCompletionEffect {
        /// The objective whose declaration carries the repeated effect.
        by: ProgramSymbol,
        /// The objective both sites name.
        objective: ProgramSymbol,
        /// The effect kind declared twice.
        kind: BranchEffectKind,
    },
    /// An objective declares a completion effect on itself.
    ///
    /// The effect fires *because* the objective completed, so by the time it
    /// applies the objective holds `Succeeded`, and no completion effect moves a
    /// target out of a final state: the declaration could never apply.
    SelfCompletionEffect {
        /// The objective declaring the effect.
        objective: ProgramSymbol,
    },
    /// A completion effect names an objective born in a state no completion
    /// effect can move it out of. No row leaves `Succeeded`, `Failed` or
    /// `Superseded`, so the effect could never apply.
    DeadCompletionEffect {
        /// The objective whose declaration carries the effect.
        by: ProgramSymbol,
        /// The objective the effect names.
        objective: ProgramSymbol,
        /// The state the target is born in.
        state: DeclaredObjectiveState,
    },
    /// An objective born in a terminal state declares completion effects. It
    /// never completes, so they can never apply.
    ///
    /// Only about *declared* effects: an objective born in a terminal state that
    /// declares none is an ordinary declaration. Nothing measured how a program
    /// may introduce an objective that is already finished, so this rule refuses
    /// the effects that could never fire and nothing else — a schema that
    /// refused such an objective outright would be enforcing an unmeasured rule
    /// under a message blaming effects it never had.
    UnfiredCompletionEffects {
        /// The objective declaring the effects.
        by: ProgramSymbol,
        /// The state it is born in.
        state: DeclaredObjectiveState,
    },
    /// A completion effect's declared number contradicts the measured shape: a
    /// nap without the number every measured nap carries, or a number on an
    /// effect that carries none.
    EffectArgumentShape {
        /// The objective whose declaration carries the effect.
        by: ProgramSymbol,
        /// The objective the effect names.
        objective: ProgramSymbol,
        /// The effect kind whose shape is wrong.
        kind: BranchEffectKind,
    },
    /// A measurement names no archive or member, so it measures nothing.
    EmptyMeasurement,
    /// An original record declares a count condition in a category the original
    /// never declares, or in one nothing measured reports.
    ///
    /// F39-E4 measured the owner's installation and found the two facts
    /// separate: the objective records name a destroy and a disable category as
    /// localized target labels and name no captured, escaped or despawned
    /// category ([`UNDECLARED_COUNT_CATEGORY`]), while no measured transition
    /// reports a disabled or escaped count at all
    /// ([`UNMEASURED_COUNT_CATEGORY`]). A record that claims the original's
    /// provenance and then counts either would be running invented semantics
    /// under an original mission's id, so the declaration is refused here, by
    /// name, with the category and the measured verdict. A **newly authored**
    /// record may still use any of the five: it is design, and it says so
    /// through its origin.
    UnmeasuredCountCategory {
        /// The condition carrying the category.
        condition: ProgramSymbol,
        /// The category the original may not be counted in.
        kind: DeclaredCountKind,
        /// Which measured fact refuses it, by name:
        /// [`UNDECLARED_COUNT_CATEGORY`] or [`UNMEASURED_COUNT_CATEGORY`].
        reason: &'static str,
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
            Self::CollidingSignal { by, signal } => write!(
                f,
                "{by} references signal {signal}, which is a declared name and would report under it"
            ),
            Self::DeadState { by, state } => write!(
                f,
                "{by} targets objective state {state:?}, which no declared deadline or count reaction can produce{}",
                if *state == DeclaredObjectiveState::Optional {
                    ": only the completion of a named other objective can set one aside"
                } else {
                    ""
                }
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
            Self::AmbiguousCompletionEffect {
                by,
                objective,
                first,
                second,
            } => write!(
                f,
                "{by} declares a {second:?} completion effect on {objective}, which a {first:?} effect already names, and no measured rule says which applies"
            ),
            Self::RepeatedCompletionEffect {
                by,
                objective,
                kind,
            } => write!(
                f,
                "{by} declares a {kind:?} completion effect on {objective} twice, and no measured rule says what the second site does"
            ),
            Self::SelfCompletionEffect { objective } => write!(
                f,
                "{objective} declares a completion effect on itself, which can only apply once it has completed"
            ),
            Self::DeadCompletionEffect {
                by,
                objective,
                state,
            } => write!(
                f,
                "{by} declares a completion effect on {objective}, which is born {state:?} and cannot be moved out of a finished state"
            ),
            Self::UnfiredCompletionEffects { by, state } => write!(
                f,
                "{by} is born {state:?} and never completes, so the completion effects it declares can never apply"
            ),
            Self::EffectArgumentShape {
                by,
                objective,
                kind,
            } => write!(
                f,
                "{by} declares a {kind:?} completion effect on {objective} whose declared {} contradicts the measured shape: the number belongs to a nap",
                if kind.carries_argument() {
                    "absence of a number"
                } else {
                    "number"
                }
            ),
            Self::EmptyMeasurement => write!(
                f,
                "a measured objective record must name the archive and member it was read from"
            ),
            Self::UnmeasuredCountCategory {
                condition,
                kind,
                reason,
            } => write!(
                f,
                "count condition {condition} counts {kind} actors, which an original \
                 record may not claim: {reason}"
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
    /// [`ObjectivesSchemaError`] naming the first invalid declaration, and
    /// [`ObjectivesSchemaError::UnmeasuredCountCategory`] when an **original**
    /// record counts a category the original's own objective records never
    /// declare (F39-E4). A newly authored record may use all five: it is
    /// design and carries an authored origin.
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
        let support = support_for(&origin);
        // F39-E4's gate, decided in one place from the two measured facts: an
        // original record may not count a category the original's own records do
        // not spell, nor one no measured transition reports. Checked *after* the
        // structural validation so the first invalid declaration is still named
        // first, and against the support this origin actually earns, so a
        // designed record is never refused for using designed categories.
        if let DeclaredSupport::Original { .. } = &support {
            for condition in &conditions {
                if let Some(reason) = original_count_category_refusal(condition.kind) {
                    return Err(ObjectivesSchemaError::UnmeasuredCountCategory {
                        condition: condition.symbol,
                        kind: condition.kind,
                        reason,
                    });
                }
            }
        }
        Ok(Self {
            support,
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
    check_completion_effects(objectives, &objective_ids, &objective_by_symbol)?;
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

/// The closure and dead-declaration rules for the completion effects a program
/// declares.
///
/// Every rule here is about a shape, never about an order of application:
///
/// * an effect names an objective **of the same program** (F39-E2 measured that
///   all 1706 retail targets do), so a dangling target is refused here instead of
///   waiting for an event no declaration can produce;
/// * the number's shape is the measured one — a nap carries one, nothing else
///   does — so an importer cannot attach a number to an effect that never had
///   one and lose it silently;
/// * three dead declarations are refused: an effect on the objective that fires
///   it, an effect on a target born finished, and effects declared by an
///   objective born finished (it never completes, so they never apply);
/// * **two different effects naming the same objective is refused, not ordered**:
///   that is the shape F39-E2 measured once in 1338 blocks and could not
///   resolve, and the declaration's own field order is refuted as a rule by the
///   same corpus. Two objectives naming the *same* effect kind for one objective
///   are accepted — they agree on what happens, so there is no order to decide.
/// * **one objective declaring the same effect on the same objective twice is
///   refused, not deduplicated** (F39-E6): that is the declared form of a block
///   spelling one completion-effect key twice, a shape the whole readable
///   corpus spells nowhere — so what the second site does is unmeasured
///   ([`UNMEASURED_REPEATED_EFFECT_KEY`]) and the repeat is refused by name
///   instead of collapsing into a count the record never authorized. The check
///   is per (kind, objective) pair of one declaring objective, not per kind:
///   one kind naming two *different* objectives is one multi-target site's
///   residue and stays legal.
fn check_completion_effects(
    objectives: &[DeclaredObjective],
    objective_ids: &BTreeSet<ProgramSymbol>,
    objective_by_symbol: &BTreeMap<ProgramSymbol, &DeclaredObjective>,
) -> Result<(), ObjectivesSchemaError> {
    // What kind of effect each objective of the program is already named for.
    // Keyed by the *target*, so this is the one place the contended question is
    // even representable.
    let mut named: BTreeMap<ProgramSymbol, BranchEffectKind> = BTreeMap::new();
    for source in objectives {
        if source.initial.is_terminal() && !source.completion_effects.is_empty() {
            return Err(ObjectivesSchemaError::UnfiredCompletionEffects {
                by: source.symbol,
                state: source.initial,
            });
        }
        // Which (kind, objective) pairs *this* objective has already declared.
        // A pair the same source declares twice can only come from a block
        // spelling the key twice — the measured corpus's never-written shape —
        // so it is refused by name rather than silently kept once.
        let mut declared: BTreeSet<(BranchEffectKind, ProgramSymbol)> = BTreeSet::new();
        for effect in &source.completion_effects {
            if effect.kind.carries_argument() != effect.argument.is_some() {
                return Err(ObjectivesSchemaError::EffectArgumentShape {
                    by: source.symbol,
                    objective: effect.objective,
                    kind: effect.kind,
                });
            }
            if !objective_ids.contains(&effect.objective) {
                return Err(ObjectivesSchemaError::UnknownObjective {
                    by: source.symbol,
                    objective: effect.objective,
                });
            }
            if effect.objective == source.symbol {
                return Err(ObjectivesSchemaError::SelfCompletionEffect {
                    objective: source.symbol,
                });
            }
            let target = objective_by_symbol
                .get(&effect.objective)
                .expect("the target was just found in the program's objectives");
            if target.initial.is_terminal() {
                return Err(ObjectivesSchemaError::DeadCompletionEffect {
                    by: source.symbol,
                    objective: effect.objective,
                    state: target.initial,
                });
            }
            match named.get(&effect.objective) {
                Some(kind) if *kind != effect.kind => {
                    return Err(ObjectivesSchemaError::AmbiguousCompletionEffect {
                        by: source.symbol,
                        objective: effect.objective,
                        first: *kind,
                        second: effect.kind,
                    });
                }
                Some(_) => {}
                None => {
                    named.insert(effect.objective, effect.kind);
                }
            }
            if !declared.insert((effect.kind, effect.objective)) {
                return Err(ObjectivesSchemaError::RepeatedCompletionEffect {
                    by: source.symbol,
                    objective: effect.objective,
                    kind: effect.kind,
                });
            }
        }
    }
    Ok(())
}

/// A declared state change a live declared action can perform never leaves
/// `Hidden`, so the rows into `Hidden` and `Pending` are reachable only from
/// `Hidden`, which only the reveal rule leaves. A declaration aiming at one of
/// them could never apply: dead, refused at declaration.
///
/// `Optional` is refused here for a different reason, and it is the one state
/// this stage gave a second source: a *completion effect* may put an objective
/// aside, but only the completion of a **named other objective** can do it. A
/// count reaction or a deadline is not a completion — it has no "when I
/// complete" — so a declaration of either kind aiming at `Optional` would be
/// reaching for the completion vocabulary from outside it, and is refused with
/// the rule stated rather than allowed to drift in.
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
                completion_effects: Vec::new(),
            },
            DeclaredObjective {
                symbol: SYNTHETIC_SECONDARY,
                content: objective_id("synthetic.f39c.secondary"),
                initial: DeclaredObjectiveState::Hidden,
                reveal: DeclaredRevealRule::OnSignal(SYNTHETIC_REACHED_WRECK),
                on_complete: DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
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

/// The synthetic fixture mission's program symbol for the objective whose
/// completion moves the others (F39-E5).
pub const SYNTHETIC_E5_COMPLETING: ProgramSymbol = ProgramSymbol(1);
/// The fixture's wake target, born shown but not yet pursued.
pub const SYNTHETIC_E5_WOKEN: ProgramSymbol = ProgramSymbol(2);
/// The fixture's nap target, born pursued.
pub const SYNTHETIC_E5_NAPPED: ProgramSymbol = ProgramSymbol(3);
/// The fixture's wakeup target, born set aside.
pub const SYNTHETIC_E5_RESUMED: ProgramSymbol = ProgramSymbol(4);
/// The fixture's kill target, born pursued.
pub const SYNTHETIC_E5_KILLED: ProgramSymbol = ProgramSymbol(5);
/// The fixture's completion deadline: an explicit arm completes
/// [`SYNTHETIC_E5_COMPLETING`] one tick later, which is what makes every effect
/// fire from one ordinary deadline expiry rather than from a test-only request.
pub const SYNTHETIC_E5_DEADLINE: ProgramSymbol = ProgramSymbol(30);

/// The minimal synthetic mission program whose one completion moves four other
/// objectives, in declared form.
///
/// Five objectives and one deadline:
///
/// * [`SYNTHETIC_E5_COMPLETING`] is born `Active`, completes without ending the
///   mission, and declares **one** effect per target — a `Wake` of
///   [`SYNTHETIC_E5_WOKEN`], a `Nap` of [`SYNTHETIC_E5_NAPPED`] carrying the
///   number `2.0`, a `Wakeup` of [`SYNTHETIC_E5_RESUMED`] and a `Kill` of
///   [`SYNTHETIC_E5_KILLED`]. No two of them names the same objective, which is
///   the shape the schema accepts (the refused one is in
///   `accept_f39_e5_two_effects_naming_one_objective_are_refused_by_name`);
/// * the four targets are born in the states their effect moves them out of:
///   shown-not-pursued, pursued, set aside and pursued;
/// * [`SYNTHETIC_E5_DEADLINE`] completes the first objective on an explicit arm,
///   so the effects are produced by an ordinary declared expiry.
///
/// The subject is `mission/synthetic.f39e.completion-effects`, every identity
/// lives under `synthetic.f39e` keys and the record carries
/// [`Origin::SyntheticFixture`] with designed provenance — it can never be
/// mistaken for retail content and cannot stand in for it.
#[must_use]
pub fn declared_synthetic_completion_effects() -> DeclaredObjectiveProgram {
    let provenance = Provenance::designed(
        ClaimId::new("f39e.synthetic-completion-effects").expect("valid claim id"),
    );
    let objective_id = |key: &str| {
        ContentId::from_source(ContentKind::Objective, key).expect("valid objective id")
    };
    let mission = ContentId::from_source(ContentKind::Mission, "synthetic.f39e.completion-effects")
        .expect("valid mission id");
    let effect = |kind, objective| {
        DeclaredCompletionEffect::new(kind, objective, None).expect("a finite declared effect")
    };
    let nap = || {
        DeclaredCompletionEffect::new(BranchEffectKind::Nap, SYNTHETIC_E5_NAPPED, Some(2.0))
            .expect("a nap with its measured number")
    };

    DeclaredObjectiveProgram::try_new(
        mission,
        Origin::SyntheticFixture,
        provenance.clone(),
        designed(DeclaredPrecedence::SyntheticConservative, &provenance),
        vec![
            DeclaredObjective {
                symbol: SYNTHETIC_E5_COMPLETING,
                content: objective_id("synthetic.f39e.completing"),
                initial: DeclaredObjectiveState::Active,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: DeclaredCompletion::Continue,
                completion_effects: vec![
                    effect(BranchEffectKind::Wake, SYNTHETIC_E5_WOKEN),
                    nap(),
                    effect(BranchEffectKind::Wakeup, SYNTHETIC_E5_RESUMED),
                    effect(BranchEffectKind::Kill, SYNTHETIC_E5_KILLED),
                ],
            },
            DeclaredObjective {
                symbol: SYNTHETIC_E5_WOKEN,
                content: objective_id("synthetic.f39e.woken"),
                initial: DeclaredObjectiveState::Pending,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
            DeclaredObjective {
                symbol: SYNTHETIC_E5_NAPPED,
                content: objective_id("synthetic.f39e.napped"),
                initial: DeclaredObjectiveState::Active,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
            DeclaredObjective {
                symbol: SYNTHETIC_E5_RESUMED,
                content: objective_id("synthetic.f39e.resumed"),
                initial: DeclaredObjectiveState::Optional,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
            DeclaredObjective {
                symbol: SYNTHETIC_E5_KILLED,
                content: objective_id("synthetic.f39e.killed"),
                initial: DeclaredObjectiveState::Active,
                reveal: DeclaredRevealRule::Immediate,
                on_complete: DeclaredCompletion::Continue,
                completion_effects: Vec::new(),
            },
        ],
        vec![],
        vec![DeclaredTimer {
            symbol: SYNTHETIC_E5_DEADLINE,
            domain: DeclaredTimeDomain::AuthoritativeGameplay,
            start: DeclaredTimerStart::OnArm,
            period_ticks: 1,
            action: DeclaredTimerAction::SetObjectiveState {
                objective: SYNTHETIC_E5_COMPLETING,
                state: DeclaredObjectiveState::Succeeded,
            },
        }],
        vec![],
        vec![],
    )
    .expect("the synthetic completion-effect program is valid")
}

// ---------------------------------------------------------------------------
// F39-E1: the measured dormant/reveal declarations of one objective block
// ---------------------------------------------------------------------------
//
// What follows is a **reader of what the original's objective records declare**
// about when a block's objective may become active, and nothing more. It is the
// measurement half of F39-D unknown #4
// (`docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`),
// which left the dormant/reveal lifecycle unrecovered because no rule had been
// isolated for any of the three declarations: `BEGIN_DORMANT` (1096 of the
// installation's 1338 blocks), the `INACTIVE<n>` stage keys (1335) and
// `INACTIVE_COMPLETION_COUNT` (130).
//
// The reader keeps every measured value **as measured** and classifies nothing
// it did not measure:
//
// * [`DormantReading::Sentinel`] is the measured `-1` argument and
//   [`DormantReading::ElapsedTime`] is a measured positive argument. The
//   *unit* of the positive argument is **unmeasured** — F39-E1's controlled
//   conditions order it against the original's own radio-cue numbering and
//   nothing more — so the reader names it an elapsed-time quantity and never a
//   number of seconds, ticks or objective indices.
// * [`InactiveCondition`] keeps its measured arity and its subject/part/
//   attribute spellings verbatim. `healthy`, `healthy_part`, `healthy_balloon`
//   and `panels` are **measured spellings of a condition about an actor**, not
//   a decoded damage rule: nothing here says what satisfying one means, or
//   even whether a satisfied condition is monotone.
// * [`MeasuredIdentity`] is the block's declared display identity (role,
//   ordinal and an optional message id) — *what* the objective is labelled,
//   never *when* the player is shown it.
//
// What this reader deliberately does **not** produce: a
// [`DeclaredRevealRule`]. Recovering the rule needs an observation of the
// original running, which no agent has; `docs/contracts/SCRIPT-MISSION.md`
// requires the inference, the contrary hypotheses and the verification to be
// recorded for such a case, and
// `docs/findings/2026-10-03-f39-e1-objective-dormant-reveal-lifecycle.md`
// records them. A reader that turned these declarations into a reveal rule
// would be the guess the contract forbids.

use crate::stunts::{OBJECTIVE_BLOCK_PREFIX, ZrdValue, objective_record, zrd_flat_fields};

/// The key a block declares its objective identity with: the display role, the
/// ordinal inside that role and, for most blocks, the message id the original
/// shows for it. Measured in 112 blocks of the 1338 (F39-E1).
pub const OBJECTIVE_IDENTITY_KEY: &str = "IDENTITY";

/// The key a block uses to play a sound group when the objective completes.
///
/// Measured in 585 of the 1338 blocks, always as exactly one name.
pub const OBJECTIVE_COMPLETED_SOUND_GROUP_KEY: &str = "COMPLETED_SOUND_GROUP";

/// The key a block uses to play a sound group when the objective *activates*.
///
/// Measured in 123 blocks and, in 37 of them, beside a positive
/// `BEGIN_DORMANT` argument — which is what makes it the ordering probe F39-E1
/// used for its first controlled condition.
pub const OBJECTIVE_WAKEUP_SOUND_GROUP_KEY: &str = "WAKEUP_SOUND_GROUP";

/// The measured `BEGIN_DORMANT` argument that declares no elapsed time.
///
/// Measured: 992 of the 1096 `BEGIN_DORMANT` arguments are exactly this value,
/// and no other negative argument occurs.
pub const DORMANT_NO_ELAPSED_TIME: f32 = -1.0;

/// What a block's single `BEGIN_DORMANT` argument reads as, measured.
///
/// The variant names only what the argument's *value* is. What the original
/// *does* when the value elapses, and whether the elapsed time is measured in
/// seconds, is unmeasured and stays that way; see [`InactiveCondition`] and the
/// module section above.
///
/// The split into the two arms is **measured** (992 arguments are exactly `-1`
/// and 104 are positive). That the positive arm is a *time* rather than some
/// other quantity that merely increases through the mission is an **inference**
/// from the key's spelling and from F39-E1's controlled condition over the
/// activation cues, and it is labelled as one in the finding: a mission-relative
/// event ordinal is not excluded by any shipped file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DormantReading {
    /// The measured `-1`: the block declares that it begins dormant and no
    /// elapsed time of its own.
    Sentinel,
    /// A measured positive argument: an elapsed-time quantity in an
    /// **unmeasured** unit, and in an **unmeasured kind** (see the note above).
    ElapsedTime(f32),
}

impl DormantReading {
    /// The argument as measured.
    #[must_use]
    pub const fn argument(self) -> f32 {
        match self {
            Self::Sentinel => DORMANT_NO_ELAPSED_TIME,
            Self::ElapsedTime(value) => value,
        }
    }

    /// Whether this argument declares no elapsed time of its own.
    #[must_use]
    pub const fn is_sentinel(self) -> bool {
        matches!(self, Self::Sentinel)
    }
}

/// One measured `INACTIVE<n>` condition: the actor it names and, when the
/// original wrote them, a part and an attribute of that actor.
///
/// Measured over the installation's 1335 stage declarations: 35 name one
/// element, 356 two and 944 three; the three-element form's third element is
/// `healthy` (750) or `panels` (194), and the second element's 88 spellings
/// include engine and gasbag node names (`reng11`, `gasbag3`) next to the same
/// words used without a part (`healthy`, `healthy_part`, `healthy_balloon`).
/// What satisfying a condition *means* — and whether the original counts the
/// loss of the named state or its presence — is **unmeasured**; F39-E1 records
/// the inference and its contrary hypotheses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InactiveCondition {
    /// The `n` of the key this condition was declared under.
    pub stage: u32,
    /// The actor the condition is about.
    pub subject: String,
    /// A part of that actor, when the original wrote one.
    pub part: Option<String>,
    /// An attribute of the actor (or of the part), when the original wrote one.
    pub attribute: Option<String>,
    /// How many elements the declaration held, exactly as measured.
    pub arity: usize,
}

/// One measured `IDENTITY` declaration: the display role, the ordinal inside
/// it and the message id the original names.
///
/// Measured in 112 declarations across 111 blocks: 81 `PRIMARY`, 29 `SECONDARY`,
/// 2 `TERTIARY`, and 79 of them carry a message id. The id is a *name*
/// (`MSG_BRF_HWM4_OBJ2`); F39-E1 measured that the installation's two shipped
/// generated headers define no `MSG_*` id at all, so the text behind one is
/// **not** resolvable from the shipped files and stays unknown.
///
/// One block of the installation declares **two** identities, a `PRIMARY` with
/// a message and a `SECONDARY` without. Which one the original honours is
/// unmeasured, so a block keeps every declaration it makes
/// ([`MeasuredDormantBlock::identities`]) rather than one of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeasuredIdentity {
    /// `PRIMARY`, `SECONDARY` or `TERTIARY`, verbatim.
    pub role: String,
    /// The ordinal the block declares inside its role.
    pub ordinal: u32,
    /// The message id the block names, when it names one.
    pub message: Option<String>,
}

/// What one numbered `OBJECTIVE<N>` block declares about its own lifecycle.
///
/// Every field is a measurement. None of them is a rule.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredDormantBlock {
    /// The block's own key, e.g. `OBJECTIVE42`.
    pub block: String,
    /// The block's [`OBJECTIVE_DORMANT_KEY`] argument, when it declares one.
    pub dormant: Option<DormantReading>,
    /// The block's [`OBJECTIVE_INACTIVE_COUNT_KEY`], when it declares one.
    pub completion_count: Option<u32>,
    /// The block's `INACTIVE<n>` conditions, in stage order.
    pub conditions: Vec<InactiveCondition>,
    /// Every [`OBJECTIVE_IDENTITY_KEY`] declaration the block makes, in
    /// declaration order. Empty when it declares none; more than one when the
    /// original wrote more than one (measured: one block of 1338).
    pub identities: Vec<MeasuredIdentity>,
    /// The sound group the block plays when the objective activates, as the
    /// name the original wrote. Whether the block is *shown* anything when it
    /// activates is unmeasured; this is a declared cue, not a reveal rule.
    pub wakeup_sound_group: Option<String>,
    /// The sound group the block plays when the objective completes, as the
    /// name the original wrote.
    pub completed_sound_group: Option<String>,
}

impl MeasuredDormantBlock {
    /// Whether the block declares that it begins dormant.
    #[must_use]
    pub const fn begins_dormant(&self) -> bool {
        self.dormant.is_some()
    }

    /// How many `INACTIVE<n>` conditions the block declares.
    #[must_use]
    pub fn condition_count(&self) -> usize {
        self.conditions.len()
    }

    /// Whether the block declares a completion count that its own conditions
    /// cannot satisfy.
    ///
    /// Measured over the installation: exactly one block of the 130 that carry a
    /// count declares one larger than its number of conditions, and it is the
    /// **same** block that declares a count with no condition at all
    /// (`zbd/c4/m03 OBJECTIVE52`, count 2 over zero conditions). Of the rest, 16
    /// equal their condition count and 113 are below it. Both facts are what a
    /// lowering needs in order to refuse such a declaration instead of running
    /// it, so the check lives in production and is queried rather than re-derived
    /// by each consumer.
    #[must_use]
    pub fn count_exceeds_conditions(&self) -> bool {
        self.completion_count
            .is_some_and(|count| count as usize > self.conditions.len())
    }

    /// Whether the block carries a completion count and **no** condition, which
    /// is the one measured shape where a count names nothing to count.
    #[must_use]
    pub fn count_without_conditions(&self) -> bool {
        self.completion_count.is_some() && self.conditions.is_empty()
    }

    /// The block's conditions as the tuples F39-E1's controlled condition
    /// compares: subject, part and attribute, in stage order.
    ///
    /// Two blocks with equal tuples declare the *same* conditions, which is
    /// what lets a census find a ladder of blocks watching one condition set at
    /// different thresholds without knowing what a condition means.
    #[must_use]
    pub fn condition_signature(&self) -> Vec<(String, Option<String>, Option<String>)> {
        self.conditions
            .iter()
            .map(|condition| {
                (
                    condition.subject.clone(),
                    condition.part.clone(),
                    condition.attribute.clone(),
                )
            })
            .collect()
    }
}

/// Why a block's dormant/reveal declarations could not be measured.
///
/// Every variant names the *field* that refused, so a reader that cannot
/// measure a block says which declaration is not what F39-E1 measured, rather
/// than dropping it and letting the block look like one that declares nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum DormantReadError {
    /// `BEGIN_DORMANT` did not hold exactly one number.
    DormantArgument {
        /// The block that refused.
        block: String,
        /// How many elements the declaration held.
        arity: usize,
    },
    /// `BEGIN_DORMANT` held a value that is not finite.
    NonFiniteDormant {
        /// The block that refused.
        block: String,
        /// The value as read.
        value: f32,
    },
    /// `BEGIN_DORMANT` held a positive value below the measured sentinel in
    /// magnitude, which F39-E1 never measured — kept as a refusal so an
    /// unmeasured argument is never read as a duration.
    UnmeasuredDormantArgument {
        /// The block that refused.
        block: String,
        /// The value as read.
        value: f32,
    },
    /// `INACTIVE_COMPLETION_COUNT` did not hold exactly one integer.
    CompletionCount {
        /// The block that refused.
        block: String,
        /// How many elements the declaration held.
        arity: usize,
    },
    /// A stage declaration held no subject, or more elements than the
    /// installation's 1335 stages ever held.
    ConditionShape {
        /// The block that refused.
        block: String,
        /// The stage number whose declaration refused.
        stage: u32,
        /// How many elements it held.
        arity: usize,
    },
    /// A stage element that was not text, so no subject/part/attribute can be
    /// named for it.
    NonTextConditionElement {
        /// The block that refused.
        block: String,
        /// The stage number whose declaration refused.
        stage: u32,
        /// The zero-based position of the element that refused.
        index: usize,
    },
    /// The stage numbers are not `1..=N`, which every measured block is.
    StageNumbering {
        /// The block that refused.
        block: String,
        /// The stage numbers as declared, in declaration order.
        declared: Vec<u32>,
    },
    /// `IDENTITY` did not hold two or three elements: a role and an ordinal, with
    /// an optional message id.
    IdentityShape {
        /// The block that refused.
        block: String,
        /// How many elements the declaration held.
        arity: usize,
    },
    /// An `IDENTITY` element was not the kind of value its position is measured
    /// to hold, so no role, ordinal or message can be named for it.
    IdentityElement {
        /// The block that refused.
        block: String,
        /// The zero-based position of the element that refused.
        index: usize,
        /// What that position is measured to hold.
        wanted: &'static str,
    },
    /// A sound-group declaration did not hold exactly one non-empty name.
    SoundGroupShape {
        /// The block that refused.
        block: String,
        /// The key that refused.
        key: &'static str,
        /// How many elements the declaration held.
        arity: usize,
    },
}

impl DormantReadError {
    /// The objective block this refusal is about.
    ///
    /// Every variant carries the block it names, so a caller that has to place
    /// the refusal (the retail census names the block in its own error) reads
    /// it from here instead of parsing the rendered message.
    #[must_use]
    pub fn block(&self) -> &str {
        match self {
            Self::DormantArgument { block, .. }
            | Self::NonFiniteDormant { block, .. }
            | Self::UnmeasuredDormantArgument { block, .. }
            | Self::CompletionCount { block, .. }
            | Self::ConditionShape { block, .. }
            | Self::NonTextConditionElement { block, .. }
            | Self::StageNumbering { block, .. }
            | Self::IdentityShape { block, .. }
            | Self::IdentityElement { block, .. }
            | Self::SoundGroupShape { block, .. } => block,
        }
    }
}

impl fmt::Display for DormantReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DormantArgument { block, arity } => write!(
                f,
                "{block}: BEGIN_DORMANT held {arity} elements, the measured shape is one"
            ),
            Self::NonFiniteDormant { block, value } => {
                write!(
                    f,
                    "{block}: BEGIN_DORMANT held the non-finite value {value}"
                )
            }
            Self::UnmeasuredDormantArgument { block, value } => write!(
                f,
                "{block}: BEGIN_DORMANT held {value}, a value F39-E1 never measured"
            ),
            Self::CompletionCount { block, arity } => write!(
                f,
                "{block}: INACTIVE_COMPLETION_COUNT held {arity} elements, the measured shape is one"
            ),
            Self::ConditionShape {
                block,
                stage,
                arity,
            } => write!(
                f,
                "{block}: INACTIVE{stage} held {arity} elements, the measured shapes hold one, two or three"
            ),
            Self::NonTextConditionElement {
                block,
                stage,
                index,
            } => write!(
                f,
                "{block}: INACTIVE{stage} element {index} is not text and names nothing"
            ),
            Self::StageNumbering { block, declared } => write!(
                f,
                "{block}: stage numbers {declared:?} are not 1..=N as every measured block is"
            ),
            Self::IdentityShape { block, arity } => write!(
                f,
                "{block}: IDENTITY held {arity} elements, the measured shapes hold two or three"
            ),
            Self::IdentityElement {
                block,
                index,
                wanted,
            } => write!(
                f,
                "{block}: IDENTITY element {index} is not {wanted} and names nothing"
            ),
            Self::SoundGroupShape { block, key, arity } => write!(
                f,
                "{block}: {key} held {arity} elements, the measured shape is one name"
            ),
        }
    }
}

impl std::error::Error for DormantReadError {}

/// The largest stage number F39-E1 measured in the installation.
///
/// A census pins the measured range rather than trusting this constant: a stage
/// number outside it would be a declaration shape this stage never saw.
pub const MEASURED_MAX_INACTIVE_STAGE: u32 = 18;

/// The largest number of elements a measured `INACTIVE<n>` declaration holds.
pub const MEASURED_MAX_CONDITION_ARITY: usize = 3;

/// Measures the dormant/reveal declarations of one objective block.
///
/// `fields` is the block's flat `key, value` list, as
/// [`crate::stunts::zrd_flat_fields`] reads it. Keys this stage does not
/// measure are ignored, so the block may declare anything else; the keys it
/// *does* measure are read strictly, and a shape F39-E1 never measured is a
/// named [`DormantReadError`] rather than a silently dropped declaration.
///
/// # Errors
///
/// [`DormantReadError`] when a measured declaration is not a shape this stage
/// measured. See each variant for the measured shape it compares against.
pub fn measure_dormant_block(
    block: &str,
    fields: &[(&str, &ZrdValue)],
) -> Result<MeasuredDormantBlock, DormantReadError> {
    let mut measured = MeasuredDormantBlock {
        block: block.to_owned(),
        dormant: None,
        completion_count: None,
        conditions: Vec::new(),
        identities: Vec::new(),
        wakeup_sound_group: None,
        completed_sound_group: None,
    };
    let mut stages: Vec<(u32, InactiveCondition)> = Vec::new();
    let mut declared: Vec<u32> = Vec::new();

    for (key, value) in fields {
        match *key {
            OBJECTIVE_DORMANT_KEY => {
                measured.dormant = Some(read_dormant(block, value)?);
            }
            OBJECTIVE_INACTIVE_COUNT_KEY => {
                measured.completion_count = Some(read_completion_count(block, value)?);
            }
            OBJECTIVE_IDENTITY_KEY => {
                measured.identities.push(read_identity(block, value)?);
            }
            OBJECTIVE_WAKEUP_SOUND_GROUP_KEY => {
                measured.wakeup_sound_group = Some(read_sound_group(
                    block,
                    OBJECTIVE_WAKEUP_SOUND_GROUP_KEY,
                    value,
                )?);
            }
            OBJECTIVE_COMPLETED_SOUND_GROUP_KEY => {
                measured.completed_sound_group = Some(read_sound_group(
                    block,
                    OBJECTIVE_COMPLETED_SOUND_GROUP_KEY,
                    value,
                )?);
            }
            key => {
                let Some(stage) = inactive_stage_number(key) else {
                    continue;
                };
                declared.push(stage);
                stages.push((stage, read_condition(block, stage, value)?));
            }
        }
    }

    // Every measured block numbers its stages `1..=N`, so a gap or a repeat is
    // a shape this stage has not seen and is refused rather than sorted over.
    declared.sort_unstable();
    let expected: Vec<u32> = (1..=declared.len() as u32).collect();
    if declared != expected {
        return Err(DormantReadError::StageNumbering {
            block: block.to_owned(),
            declared,
        });
    }
    stages.sort_unstable_by_key(|(stage, _)| *stage);
    measured.conditions = stages.into_iter().map(|(_, condition)| condition).collect();

    Ok(measured)
}

/// Measures every numbered `OBJECTIVE<N>` block of one decoded objective
/// record.
///
/// The blocks are returned in the order the record declares them, which F39-E1
/// measured over the installation to be `OBJECTIVE1`, `OBJECTIVE2`, … with no
/// gap and no offset in any of the 53 mission readers; the retail test asserts
/// that per row, so this is a measurement and not an assumption.
///
/// # Errors
///
/// [`DormantReadError`] for the first block whose measured declarations are
/// not a measured shape. Failing rather than skipping is deliberate: a block
/// that silently vanished from the result would read as a block that declares
/// nothing dormant at all.
pub fn measure_dormant_declarations(
    document: &ZrdValue,
) -> Result<Vec<MeasuredDormantBlock>, DormantReadError> {
    let mut measured = Vec::new();
    for (key, value) in zrd_flat_fields(objective_record(document)) {
        if objective_block_number(key).is_none() {
            continue;
        }
        measured.push(measure_dormant_block(key, &zrd_flat_fields(value))?);
    }
    Ok(measured)
}

/// The block number of a numbered `OBJECTIVE<N>` key, or `None` for any other
/// spelling.
///
/// Only `OBJECTIVE` followed by a non-empty run of ASCII digits is a block, which
/// is the same rule `cs_content::stunts`' objective state machine uses. The
/// installation's one other key with that prefix — `OBJECTIVE_DELAY`, measured in
/// two blocks of the 1338 — is a mission-level declaration, not a block, and is
/// read as neither.
#[must_use]
pub fn objective_block_number(key: &str) -> Option<u32> {
    let digits = key.strip_prefix(OBJECTIVE_BLOCK_PREFIX)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The stage number of an `INACTIVE<n>` key, or `None` for any other spelling.
///
/// Only `INACTIVE` followed by a non-empty run of ASCII digits is a stage, so
/// `INACTIVE_COMPLETION_COUNT` (handled by its own key) and a spelling like
/// `INACTIVATED` or `INACTIVE_A` are never read as one. `INACTIVE0` **is**
/// stage-shaped and is returned as `Some(0)`, which the block reader then
/// refuses; see [`measure_dormant_block`].
#[must_use]
pub fn inactive_stage_number(key: &str) -> Option<u32> {
    let rest = key.strip_prefix(OBJECTIVE_INACTIVE_STAGE_PREFIX)?;
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    // `INACTIVE0` is a stage-shaped key with an unusable number: it is returned
    // here and refused by `measure_dormant_block`'s `1..=N` rule, never
    // dropped, because dropping it would read a two-stage block as a
    // one-condition ladder.
    rest.parse::<u32>().ok()
}

/// Reads one declared sound group: a single non-empty text element, which is the
/// shape every measured declaration holds.
///
/// A declaration of any other shape is a refusal rather than a silently absent
/// cue: a cue this reader cannot read would leave the block looking like one
/// that declares none, and the controlled condition over the dated blocks draws
/// its population from exactly these names.
fn read_sound_group(
    block: &str,
    key: &'static str,
    value: &ZrdValue,
) -> Result<String, DormantReadError> {
    let elements = value.as_list().unwrap_or_default();
    match elements {
        [ZrdValue::Text(text)] if !text.is_empty() => Ok(text.clone()),
        _ => Err(DormantReadError::SoundGroupShape {
            block: block.to_owned(),
            key,
            arity: elements.len(),
        }),
    }
}

fn read_dormant(block: &str, value: &ZrdValue) -> Result<DormantReading, DormantReadError> {
    let Some([element]) = value.as_list() else {
        return Err(DormantReadError::DormantArgument {
            block: block.to_owned(),
            arity: value.as_list().map_or(0, <[ZrdValue]>::len),
        });
    };
    let argument = match element {
        ZrdValue::Int(value) => *value as f32,
        ZrdValue::Float(value) => *value,
        _ => {
            return Err(DormantReadError::DormantArgument {
                block: block.to_owned(),
                arity: 1,
            });
        }
    };
    if !argument.is_finite() {
        return Err(DormantReadError::NonFiniteDormant {
            block: block.to_owned(),
            value: argument,
        });
    }
    if argument == DORMANT_NO_ELAPSED_TIME {
        return Ok(DormantReading::Sentinel);
    }
    if argument > 0.0 {
        return Ok(DormantReading::ElapsedTime(argument));
    }
    Err(DormantReadError::UnmeasuredDormantArgument {
        block: block.to_owned(),
        value: argument,
    })
}

fn read_completion_count(block: &str, value: &ZrdValue) -> Result<u32, DormantReadError> {
    let shape = || DormantReadError::CompletionCount {
        block: block.to_owned(),
        arity: value.as_list().map_or(0, <[ZrdValue]>::len),
    };
    match value.as_list() {
        Some([ZrdValue::Int(count)]) => Ok(*count),
        _ => Err(shape()),
    }
}

fn read_condition(
    block: &str,
    stage: u32,
    value: &ZrdValue,
) -> Result<InactiveCondition, DormantReadError> {
    let elements = value.as_list().unwrap_or_default();
    if elements.is_empty() || elements.len() > MEASURED_MAX_CONDITION_ARITY {
        return Err(DormantReadError::ConditionShape {
            block: block.to_owned(),
            stage,
            arity: elements.len(),
        });
    }
    let mut texts: Vec<String> = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        let ZrdValue::Text(text) = element else {
            return Err(DormantReadError::NonTextConditionElement {
                block: block.to_owned(),
                stage,
                index,
            });
        };
        if text.is_empty() {
            return Err(DormantReadError::ConditionShape {
                block: block.to_owned(),
                stage,
                arity: elements.len(),
            });
        }
        texts.push(text.clone());
    }
    let mut parts = texts.into_iter();
    let subject = parts.next().expect("the empty shape was refused above");
    Ok(InactiveCondition {
        stage,
        subject,
        part: parts.next(),
        attribute: parts.next(),
        arity: elements.len(),
    })
}

fn read_identity(block: &str, value: &ZrdValue) -> Result<MeasuredIdentity, DormantReadError> {
    let shape = || DormantReadError::IdentityShape {
        block: block.to_owned(),
        arity: value.as_list().map_or(0, <[ZrdValue]>::len),
    };
    let element = |index, wanted: &'static str| DormantReadError::IdentityElement {
        block: block.to_owned(),
        index,
        wanted,
    };
    let elements = value.as_list().unwrap_or_default();
    if !(2..=3).contains(&elements.len()) {
        return Err(shape());
    }
    let ZrdValue::Text(role) = &elements[0] else {
        return Err(element(0, "a role"));
    };
    if role.is_empty() {
        return Err(element(0, "a non-empty role"));
    }
    let Some(ZrdValue::Int(ordinal)) = elements.get(1) else {
        return Err(element(1, "an ordinal"));
    };
    let message = match elements.get(2) {
        Some(ZrdValue::Text(text)) if !text.is_empty() => Some(text.clone()),
        Some(ZrdValue::Text(_)) => return Err(element(2, "a non-empty message id")),
        Some(_) => return Err(element(2, "a message id")),
        None => None,
    };
    Ok(MeasuredIdentity {
        role: role.clone(),
        ordinal: *ordinal,
        message,
    })
}

// ---------------------------------------------------------------------------
// F39-E7: is a detached actor a counted category?
// ---------------------------------------------------------------------------
//
// `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering") requires
// conditions to tell **six** things apart — "disabled, dead, captured, escaped,
// detached and despawned" — while the F39 sheet's own non-negotiable 2 and the
// engine's counter vocabulary name **five** (`DeclaredCountKind`). The missing
// one is `detached`, and nothing in this project had asked the original what it
// means. F39-E4 measured which of the five the original's records declare; it
// deliberately invented no sixth. This section is the question for the sixth:
//
// **does any objective declaration in the installation spell a detached
// category at all?**
//
// The answer is measured over three surfaces that are deliberately kept apart,
// because a name found on one of them is not evidence about the others:
//
// 1. [`DetachedVocabularySurface::CountedCondition`] — the `INACTIVE<n>`
//    conditions of a block that **also** declares an
//    `INACTIVE_COMPLETION_COUNT`. This is the only counter the original's
//    records actually write, so a category declared here is a category a
//    condition can be satisfied by.
// 2. [`DetachedVocabularySurface::ObjectiveKind`] — the `help_label`,
//    `category_label` and `description` of a `targets.zrd` record: a localized
//    label saying what must happen to a named actor. A label is *not* a counted
//    transition, which is exactly why F39-E4 refused to make one a producer.
// 3. [`DetachedVocabularySurface::BlockDeclaration`] — every text element of
//    every `OBJECTIVE<n>` block: the vocabulary the objective *triggers* are
//    written in (`WAKE_ANIM`, `COMPLETED_SOUND_GROUP`, ...). This is where a
//    scripted detach shows up, if anywhere.
//
// A name matches a family when one of its `_`-separated **segments** begins
// with that family's stem, case-insensitively — the same segment rule F39-E4
// used for its five categories, kept here as its own declared rule rather than
// borrowed, because the family list below is this stage's. Segment-prefix
// rather than whole-string, because the measured labels carry a `MSG_OBJ_`
// prefix; segment rather than substring, because a stem that matched anywhere
// would claim `healthy` for a `THREAT`-shaped spelling.
//
// **What a negative here does and does not mean.** A zero over these three
// surfaces is a measured absence *of the spelling*, over a published stem list,
// in the declarations this reader can see. It is not a proof that the original
// has no notion of a detached actor: the compiled program behind each record is
// undecoded (F13-B/C, F38 own the instruction table), and no original
// executable has been run. What the surfaces do show is *where* the original
// writes a detach, which is the question F39-E7 had to answer.

use crate::stunts::{
    SCENARIO_TARGETS_MEMBER, TARGET_CATEGORY_KEY, TARGET_DESCRIPTION_KEY, TARGET_HELP_KEY,
    zrd_field,
};

/// The contract's own word for the sixth distinction, as a spelling stem.
///
/// Measured: the token `detach` appears in **no** file of the owner's
/// installation except two third-party Windows/DirectX export names
/// (`DLL_PROCESS_DETACH` in `dsetup32.dll`, `CImmDevice::detach_effects` in
/// `ifc21.dll`). It is still the contract's word, so it leads the family.
pub const DETACH_STEM: &str = "DETACH";

/// Every spelling family a detached objective category could be declared under.
///
/// A published list, not a proof: a name spelled outside these twelve stems
/// would not be found, which is why the finding records the rule and the list
/// rather than only the count. The list is the contract's own word plus the
/// release/drop/ejection vocabulary the installation is measured to use for
/// scripted detaches (`release_hook`, `drop_paratroopers`, `dropit`,
/// `drop_smokescreen_canister`), read at segment level so `MSG_OBJ_RELEASE`
/// matches `RELEASE`.
pub const DETACHED_SPELLING_STEMS: &[&str] = &[
    DETACH_STEM,
    "RELEASE",
    "DROP",
    "EJECT",
    "JETTISON",
    "LAUNCH",
    "LOOSE",
    "FREE",
    "UNDOCK",
    "UNCOUPLE",
    "DISCONNECT",
    "CASTOFF",
];

/// Which surface of an objective record a name was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DetachedVocabularySurface {
    /// The `INACTIVE<n>` conditions of a block that also declares an
    /// `INACTIVE_COMPLETION_COUNT`.
    CountedCondition,
    /// A `targets.zrd` record's `help_label`, `category_label` or `description`.
    ObjectiveKind,
    /// Any text element of any `OBJECTIVE<n>` block.
    BlockDeclaration,
}

impl DetachedVocabularySurface {
    /// The stable lowercase label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CountedCondition => "counted_condition",
            Self::ObjectiveKind => "objective_kind",
            Self::BlockDeclaration => "block_declaration",
        }
    }

    /// Every surface, in the order the reader reads them.
    pub const ALL: &'static [Self] = &[
        Self::CountedCondition,
        Self::ObjectiveKind,
        Self::BlockDeclaration,
    ];
}

/// The family a name belongs to, or `None` when no segment of it begins with
/// one of [`DETACHED_SPELLING_STEMS`].
///
/// The rule is **one**: split on `_`, and a family claims the name when one
/// segment starts with that family's stem, compared case-insensitively. The
/// first family in [`DETACHED_SPELLING_STEMS`] wins, so the answer does not
/// depend on iteration order.
#[must_use]
pub fn detached_spelling_family(name: &str) -> Option<&'static str> {
    for segment in name.split('_').filter(|segment| !segment.is_empty()) {
        let upper = segment.to_ascii_uppercase();
        if let Some(stem) = DETACHED_SPELLING_STEMS
            .iter()
            .copied()
            .find(|stem| upper.starts_with(stem))
        {
            return Some(stem);
        }
    }
    None
}

/// One measured name, with the surface and block it was read from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DetachedSpellingSite {
    /// The surface the name was read from.
    pub surface: DetachedVocabularySurface,
    /// The `OBJECTIVE<n>` block, or `None` for a `targets.zrd` label.
    pub block: Option<String>,
    /// The name exactly as the record spells it.
    pub name: String,
    /// The family the name matched, or `None` when it matched none.
    pub family: Option<&'static str>,
}

/// What one objective record's declarations spell, per surface.
///
/// Every number is a count of **names read**, kept as read: the reader never
/// drops a name and never merges the surfaces, so a caller can compare them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MeasuredDetachedVocabulary {
    /// Every name read, in surface order then record order.
    pub names: Vec<DetachedSpellingSite>,
}

impl MeasuredDetachedVocabulary {
    /// How many names one surface contributed.
    #[must_use]
    pub fn surface_sites(&self, surface: DetachedVocabularySurface) -> usize {
        self.names
            .iter()
            .filter(|site| site.surface == surface)
            .count()
    }

    /// How many **distinct** names one surface spelled.
    #[must_use]
    pub fn surface_names(&self, surface: DetachedVocabularySurface) -> usize {
        self.distinct(surface).len()
    }

    /// How many sites on one surface matched one family.
    #[must_use]
    pub fn family_sites(&self, surface: DetachedVocabularySurface, family: &str) -> usize {
        self.names
            .iter()
            .filter(|site| site.surface == surface && site.family == Some(family))
            .count()
    }

    /// How many sites on **any** surface matched one family.
    #[must_use]
    pub fn sites_in_family(&self, family: &str) -> usize {
        self.names
            .iter()
            .filter(|site| site.family == Some(family))
            .count()
    }

    /// Every site that matched one family, in surface order.
    #[must_use]
    pub fn sites_of_family(&self, family: &str) -> Vec<&DetachedSpellingSite> {
        self.names
            .iter()
            .filter(|site| site.family == Some(family))
            .collect()
    }

    /// How many sites on one surface matched no family at all.
    #[must_use]
    pub fn unclaimed_sites(&self, surface: DetachedVocabularySurface) -> usize {
        self.names
            .iter()
            .filter(|site| site.surface == surface && site.family.is_none())
            .count()
    }

    /// Every distinct name one surface spelled, sorted.
    #[must_use]
    pub fn distinct(&self, surface: DetachedVocabularySurface) -> Vec<&str> {
        let names: BTreeSet<&str> = self
            .names
            .iter()
            .filter(|site| site.surface == surface)
            .map(|site| site.name.as_str())
            .collect();
        names.into_iter().collect()
    }

    /// How many distinct names each surface spelled, per surface, in
    /// [`DetachedVocabularySurface::ALL`] order.
    #[must_use]
    pub fn distinct_names(&self) -> Vec<(DetachedVocabularySurface, usize)> {
        DetachedVocabularySurface::ALL
            .iter()
            .map(|surface| (*surface, self.surface_names(*surface)))
            .collect()
    }

    /// How many sites each family matched on each surface, in
    /// [`DETACHED_SPELLING_STEMS`] order.
    #[must_use]
    pub fn family_counts(&self) -> Vec<(&'static str, BTreeMap<DetachedVocabularySurface, usize>)> {
        DETACHED_SPELLING_STEMS
            .iter()
            .map(|stem| {
                let per_surface = DetachedVocabularySurface::ALL
                    .iter()
                    .map(|surface| (*surface, self.family_sites(*surface, stem)))
                    .collect();
                (*stem, per_surface)
            })
            .collect()
    }

    fn push(&mut self, surface: DetachedVocabularySurface, block: Option<&str>, name: &str) {
        self.names.push(DetachedSpellingSite {
            surface,
            block: block.map(str::to_owned),
            name: name.to_owned(),
            family: detached_spelling_family(name),
        });
    }
}

/// Measures what one objective record spells about a detached category.
///
/// `objectives` is a decoded `objectives.zrd` member and `targets` an optional
/// decoded `targets.zrd` member from the **same** reader archive; `None` is a
/// measured absence of the member (F39-E4 measured exactly one mission reader
/// with no `targets.zrd`) and contributes no objective-kind name, so a caller
/// can tell "this reader declares no objective kind" from "this reader was
/// never read".
///
/// Never fails and never refuses: a name is kept exactly as spelled, on the
/// surface it was read from. A record that declares nothing yields an empty
/// measurement, which is a measurement.
#[must_use]
pub fn measure_detached_vocabulary(
    objectives: &ZrdValue,
    targets: Option<&ZrdValue>,
) -> MeasuredDetachedVocabulary {
    let mut measured = MeasuredDetachedVocabulary::default();
    for (key, value) in zrd_flat_fields(objective_record(objectives)) {
        let Some(block) = key.strip_prefix(OBJECTIVE_BLOCK_PREFIX) else {
            continue;
        };
        if block.is_empty() || !block.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let block = format!("{OBJECTIVE_BLOCK_PREFIX}{block}");
        let fields = zrd_flat_fields(value);
        // Surface 1: the only counter the records write is a threshold over a
        // block's own stages, so a stage beside no threshold is not a counted
        // condition and is read as a plain declaration instead.
        let counted = fields
            .iter()
            .any(|(key, _)| *key == OBJECTIVE_INACTIVE_COUNT_KEY);
        for (key, value) in &fields {
            if inactive_stage_number(key).is_some() {
                for element in value.as_list().unwrap_or_default() {
                    if let ZrdValue::Text(text) = element {
                        measured.push(
                            if counted {
                                DetachedVocabularySurface::CountedCondition
                            } else {
                                DetachedVocabularySurface::BlockDeclaration
                            },
                            Some(&block),
                            text,
                        );
                    }
                }
            }
        }
        // Surface 3: every other text element of the block, in declaration
        // order. Keys are skipped: a key is the original's own vocabulary
        // (`BEGIN_DORMANT`, `WAKE_ANIM`, ...), not a name it declares about an
        // actor, and `INACTIVE` stages were already read above.
        for (key, value) in &fields {
            if inactive_stage_number(key).is_some() || *key == OBJECTIVE_INACTIVE_COUNT_KEY {
                continue;
            }
            read_declaration_text(value, &mut |text| {
                measured.push(
                    DetachedVocabularySurface::BlockDeclaration,
                    Some(&block),
                    text,
                );
            });
        }
    }

    // Surface 2: the localized objective kinds.
    if let Some(targets) = targets {
        for record in targets.as_list().unwrap_or_default() {
            for key in [TARGET_HELP_KEY, TARGET_CATEGORY_KEY, TARGET_DESCRIPTION_KEY] {
                if let Some(text) = zrd_field(record, key).and_then(ZrdValue::as_text) {
                    measured.push(DetachedVocabularySurface::ObjectiveKind, None, text);
                }
            }
        }
    }
    measured
}

/// Every text element inside `value`, depth first, in declaration order.
fn read_declaration_text(value: &ZrdValue, each: &mut impl FnMut(&str)) {
    match value {
        ZrdValue::Text(text) => each(text),
        ZrdValue::List(children) => {
            for child in children {
                read_declaration_text(child, each);
            }
        }
        ZrdValue::Int(_) | ZrdValue::Float(_) => {}
    }
}

/// The `targets.zrd` member this section reads, named so a caller does not
/// repeat the spelling.
pub const DETACHED_TARGETS_MEMBER: &str = SCENARIO_TARGETS_MEMBER;

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
