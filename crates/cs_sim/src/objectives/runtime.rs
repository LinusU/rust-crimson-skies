//! The continuous objective runtime: swept triggers, counted conditions, timer
//! actions and the declared terminal latch, on one ordered event stream.
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39 owns trigger semantics), stage `### F39-B`, acceptance case AC02
//! (*"Destroy a protected actor on the same tick as completing an objective;
//! use declared terminal precedence"*). Shared contract:
//! `docs/contracts/SCRIPT-MISSION.md`, "Objective event ordering" and "IR
//! requirements".
//!
//! # What F39-A left and what this stage adds
//!
//! F39-A shipped the vocabulary — [`ObjectiveCell`](super::state::ObjectiveCell),
//! [`SweptTrigger`](super::trigger::SweptTrigger),
//! [`ActorCounters`](super::counters::ActorCounters) and
//! [`EmissionLedger`](super::spawn::EmissionLedger) — each usable on its own.
//! What a mission needs on top of that is the *continuous* part, and this
//! module is it:
//!
//! * **One tick, one ordered stream.** [`ObjectiveRuntime::step`] takes the
//!   facts one tick produced and answers with a [`Vec<ObjectiveEvent>`] sorted
//!   by [`EventKey`]. The key is `(session, tick, source, sequence)` — the
//!   contract's "session/tick/source/program sequence, not hash map or entity
//!   iteration order" — so two runs that produced the same facts produce the
//!   same stream in the same order.
//! * **No reentrancy.** An action never runs a callback. A mission signal is
//!   eligible to arm a [`TimerStart::OnSignal`] timer on the **next** tick, so
//!   two timers cannot chase each other inside one tick.
//! * **A timer performs its one declared action once.** The action belongs to
//!   the expiry, not to the [`TimerState::Expired`](super::timer::TimerState)
//!   the table then sits in: a deadline that ran out on tick 4 does not re-grant
//!   its reward, re-raise its signal or re-request its wave on ticks 5, 6 and 7
//!   after that. Only a declared start or a fresh [`TimerRequest::Arm`] runs it
//!   again. The [`EmissionLedger`](super::spawn::EmissionLedger) would hide the
//!   repeat for a spawn or a cue, and for nothing else.
//! * **A named reference is never dropped.** A request that names a timer or an
//!   objective this runtime does not have applies nothing and is *reported*, so
//!   "the program asked for something" and "the world did nothing" are
//!   distinguishable from the stream alone.
//! * **Terminal precedence is declared, not incidental.**
//!   [`TerminalPrecedence::SyntheticConservative`](super::terminal::TerminalPrecedence::SyntheticConservative)
//!   resolves the whole set of requests a tick made, and the
//!   [`TerminalLatch`](super::terminal::TerminalLatch) then holds exactly one
//!   answer forever. A request after the latch is refused by name instead of
//!   changing a result the player has already been shown.
//! * **A protected actor is a declared roster, not a heuristic.** A
//!   [`CountCondition`] names the actors and the single
//!   [`CountKind`](super::counters::CountKind) that satisfies it, and its
//!   [`CountReaction`] names what satisfying it means. There is no operation
//!   anywhere in this module that turns "no enemies alive" into a mission
//!   outcome.
//!
//! # The declared phase order
//!
//! `step` applies effects in a fixed order:
//!
//! | phase | what it applies | why there |
//! | --- | --- | --- |
//! | 1 counters | [`LifecycleKind`](crate::damage::LifecycleKind) transitions through the [`MissionTransitions`](super::bailout::MissionTransitions) ledger into [`ActorCounters`](super::counters::ActorCounters) | a counter is a fact about this tick before anything may depend on it, and the ledger is what keeps a destruction arriving after a bailout from becoming one |
//! | 2 conditions | a satisfied [`CountCondition`] latches once and applies its declared [`CountReaction`] | a condition is a decision, so it follows its facts |
//! | 3 triggers | swept crossings from this tick's real movement segments | a crossing is a fact; a program sees it, and it never applies an effect itself |
//! | 4 signals | this tick's declared signals are collected | a signal raised now is eligible next tick |
//! | 5 objectives | the tick's declared objective state changes | state, before the actions that read it |
//! | 6 timers | arms and cancellations, then whole committed ticks, then each expiry's one declared action | the last thing that may start work this tick |
//! | 7 completion effects | the [`CompletionEffect`]s every objective that completed this tick declares, drained from a queue | an effect acts on *another* objective, so it is eligible after this tick's own state changes and never inside them |
//! | 8 outcome | the tick's terminal requests are resolved **together** | one decision per tick, so no producer's request order decides the outcome |
//!
//! The returned stream is sorted by [`EventKey`], which is the *observation*
//! order a consumer should use. Effects were applied in the phase order above;
//! the sort does not undo that.
//!
//! # Spawned instance ids sit in their own band
//!
//! An admitted spawn group's instance ids come from
//! [`SPAWNED_ACTOR_BASE`] upward, in admission order — never from the low range
//! the mission's placed actors (the player, convoy ships, importer-minted
//! world actors) live in. That is the runtime half of the declared-vs-spawned
//! actor-id contract (F39-E-ACTORID-SPACE): because the allocator can never
//! reach the placed range, a declared actor's id can never name a wave
//! instance, and lifecycle facts, trigger watches and counter attribution stay
//! unambiguous in the one [`ActorId`] space they share. The declared half —
//! the [`cs_content`] schema that keeps placed references out of the band and
//! spawned references inside the program's computed wave capacity — lives in
//! `cs_content::objectives`; the decision record is
//! `docs/findings/2026-10-13-f39-e-actorid-space.md`.
//!
//! # Completion effects: one completion moves other objectives
//!
//! [`ObjectiveSpec::completion_effects`] is what completing one objective does to
//! *others*: the declared effects [`CompletionEffectKind`] carries, each
//! naming one objective of the same program. The effect is applied by draining a
//! queue in phase 7, **not** by calling back into the state machine from inside
//! the completion that raised it, which is the contract's *"actions do not
//! directly recurse into callbacks"* rule made structural.
//!
//! The semantics are the engine image's, measured under M01-B-OBJ-1
//! (`docs/findings/2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`)
//! and carried here under the boundaries recorded in
//! `docs/findings/2026-10-12-m01-b-obj-1-fu1-runtime-effect-alignment.md`:
//!
//! * **Fixed order.** The engine applies `WAKE`, then `NAP`, then `KILL`,
//!   then `SLEEP`, whatever order the record spells; the runtime queues a
//!   completion's effects in that order ([`CompletionEffectKind::engine_order`]),
//!   so `WAKE` then `NAP` on one target leaves it napping, as the engine does.
//! * **A wake is a guarded move.** It skips a target that is not
//!   [`present`](ObjectiveRuntime::is_present) or has completed (a final
//!   state), and a target that is already [`Active`] stops the rest of that
//!   completion's wake list ([`ObjectiveEventKind::WakeStopped`]).
//! * **A nap is a timed pause.** It moves the target to
//!   [`ObjectiveState::Napping`] and stores the declared nap time
//!   ([`NapTime`]) on it, exactly as the engine writes its re-wake slot. The
//!   unit of that time is unmeasured, so nothing here converts it into ticks
//!   and nothing ends a nap by itself: a wake or a declared move does.
//! * **A kill is a removal.** It clears the target's presence flag and leaves
//!   its state alone, reported as [`ObjectiveEventKind::ObjectiveRemoved`] —
//!   it is not an outcome, and it never sets [`ObjectiveState::Failed`].
//! * **A repeat is refused, not deduplicated.** One declaration naming the
//!   same `(kind, target)` pair twice is refused at registration
//!   ([`RuntimeError::RepeatedCompletionEffect`]): the readable corpus spells
//!   a repeated completion-effect key nowhere, so what a second site does —
//!   replace the first, be ignored, or apply beside it — is unmeasured
//!   (`UNMEASURED_REPEATED_EFFECT_KEY`, `cs_content::objectives`) and this
//!   runtime does not collapse it into the one effect the declaration
//!   authorized. One kind naming two *different* targets, and one pair named
//!   by *two different* declarations, stay legal.
//!
//! Three properties keep the drain bounded and order-independent:
//!
//! * no effect kind moves its target to [`ObjectiveState::Succeeded`], so an
//!   applied effect can never queue another one — a cascade cannot be written
//!   even before the queue's own rule (a drain takes the due set, and anything
//!   queued while draining waits for the next tick) applies;
//! * a target named with **different** effect kinds by two different completing
//!   objectives is refused at registration
//!   ([`RuntimeError::AmbiguousCompletionEffect`]) rather than ordered: the
//!   engine completes one objective per pass, so that collision cannot arise
//!   there, and which of two same-tick completions should apply last depends on
//!   the pass-origin rule, which stays unmeasured. One declaration carrying two
//!   *different* effects on one target — the shape the corpus spells — is no
//!   longer refused: the engine's fixed order decides it. The same pair twice
//!   is a different shape and stays refused
//!   ([`RuntimeError::RepeatedCompletionEffect`]).
//! * the due set is applied in the order the completions happened — the phase
//!   order, then each objective's effects in engine order — and nothing hashes
//!   or walks an entity table to produce it. Two objectives completing in one
//!   tick may therefore be *applied* in either order, and the stream's final
//!   sort by [`EventKey`] makes them *observed* in symbol order regardless; and
//!   since a contested target cannot be declared, no such order ever decides an
//!   outcome.
//!
//! # What is unknown
//!
//! Which deadline a mission declares, in which domain, armed by what and
//! performing which action; which terminal outcome wins a same-tick collision;
//! and which actors a mission protects are other families' unmeasured rules and
//! stay that way. The completion effects' spellings and closure over the record
//! were measured (F39-D/F39-E2), and their behaviour above was measured in the
//! engine image (M01-B-OBJ-1); what remains unmeasured and unmodelled here is
//! recorded in the alignment finding: the dormant/awake timer families, the
//! nap-time unit (so no nap expiry), the pass-origin rule, the wake's dormancy
//! bookkeeping, and whether the image's behaviour matches its code when run.
//! The spelling `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` has **no reader** in the
//! image; [`CompletionEffectKind::Wakeup`] is kept apart from `Wake` because
//! the declared schema still carries it, and its move stays this project's
//! designed reading, not a measured one.
//!
//! No authored mission is bound to this runtime yet: the content binding is
//! `cs_content::objectives` plus F39-C's wiring, so `content` here is a declared
//! input rather than a table this module invented. What *is* checked here is the
//! engine: ordering, refusals, bounds and stale-session handling.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_script::ir::{ActorId, DirectiveOperation, SymbolId};
use cs_script::runtime::{DirectiveEmission, EventKey, ExecutionKey, SessionGeneration};
use cs_types::Tick;
use cs_types::content::ContentId;

use crate::damage::LifecycleKind;

use super::address::AddressRefusal;
use super::bailout::{
    BailoutConfirmation, BailoutRefusal, MissionTransition, MissionTransitions, TransitionOutcome,
};
use super::counters::{ActorCounters, CountKind};
use super::directives;
use super::spawn::{Admission, Emission, EmissionLedger, IdempotencyKey};
use super::state::{IllegalTransition, ObjectiveCell, ObjectiveState};
use super::terminal::{Resolution, TerminalLatch, TerminalOutcome, TerminalPrecedence};
use super::timer::{MissionTimer, TimerAction, TimerError, TimerRequest, TimerStart, TimerState};
use super::trigger::{Movement, SweptTrigger, TriggerError, TriggerEvent};

/// The `source` of an event that belongs to no declaration: a counted actor's
/// transition.
///
/// Actor identity is not a program symbol, so counted events share one reserved
/// source and are ordered among themselves by their `sequence`. No declaration
/// may use this symbol — [`ObjectiveRuntime::add_objective`],
/// [`add_condition`](Self::add_condition), [`add_timer`](Self::add_timer) and
/// [`add_trigger`](Self::add_trigger) all refuse it — so a counted event can
/// never be mistaken for a declaration's event.
///
/// The reserved value is `u32::MAX` because `0` is a real symbol: a record's
/// blocks carry `SymbolId(0..objectives)` and
/// [`resolve_objective_address`](super::address::resolve_objective_address)
/// mints the same space (`address − 1`), so `SymbolId(0)` is the record's first
/// block, not a spare marker (M02-B-FU5). The count is a `u32`, so no record
/// index and no resolved address can reach `u32::MAX`, and a counted event
/// therefore sorts after every declaration's in the tick's key order.
pub const ACTOR_EVENT_SOURCE: SymbolId = SymbolId(u32::MAX);

/// The first [`ActorId`] a spawned wave instance ever takes: the low end of the
/// band this runtime allocates from.
///
/// The declared-vs-spawned actor-id contract (F39-E-ACTORID-SPACE, recorded in
/// `docs/findings/2026-10-13-f39-e-actorid-space.md`) splits one `ActorId`
/// space in two: **placed** actors — the player, convoy ships and every actor
/// an importer mints from a record — carry ids *below* this base, and **spawned**
/// instances a timer action admits take ids at or above it, in admission order
/// (the k-th admitted instance is `ActorId(SPAWNED_ACTOR_BASE + k - 1)`).
///
/// The split is enforced by allocation, not by a check that could be skipped:
/// [`ObjectiveRuntime::admit_spawn`] never hands out an id below this value, so
/// a declared actor such as `ActorId(2)` can never *be* a wave instance — its
/// trigger watches, roster membership, lifecycle attribution and objective
/// counts stay its own no matter what the waves admit. The declared schema
/// side of the contract lives in `cs_content::objectives`, which carries the
/// same constant under the same name; `cs_app` pins the two equal at compile
/// time.
///
/// The boundary itself is **designed**, not measured: no original spawn-id
/// space is known, so `0x4000_0000` is chosen only to leave both sides enormous
/// headroom — roughly 1.07 billion placed ids below and 3.2 billion spawned ids
/// at and above.
pub const SPAWNED_ACTOR_BASE: u32 = 0x4000_0000;

/// A declared count condition: *which* actors, in *which* category, how many.
///
/// This type is the structural answer to F39 non-negotiable behavior 2
/// ("counters distinguish destroyed, disabled, captured, escaped and despawned
/// actors. Never approximate every objective by `enemy_alive == 0`"):
///
/// * `roster` is an explicit set of actors, so an actor outside it can never
///   satisfy the condition;
/// * `kind` is a single [`CountKind`], never a union, so a captured or despawned
///   raider does not satisfy a *destroyed* condition;
/// * `required` says how many of the roster must be in that category, so the
///   condition is about a roster and not about the world's total.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountCondition {
    /// The condition's symbol; also the `source` of its events.
    pub key: SymbolId,
    /// The one category that satisfies this condition.
    pub kind: CountKind,
    /// The actors that can satisfy it. Never empty: a condition no actor can
    /// reach is a defect, not a condition.
    pub roster: BTreeSet<ActorId>,
    /// How many roster actors in `kind` satisfy it. Never zero: a condition
    /// satisfied by construction observes nothing.
    pub required: usize,
}

impl CountCondition {
    /// Builds a condition from a roster.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::EmptyRoster`] and [`RuntimeError::ZeroRequired`], and
    /// [`RuntimeError::ReservedSymbol`] for the reserved
    /// [`ACTOR_EVENT_SOURCE`].
    pub fn new(
        key: SymbolId,
        kind: CountKind,
        roster: impl IntoIterator<Item = ActorId>,
        required: usize,
    ) -> Result<Self, RuntimeError> {
        if key == ACTOR_EVENT_SOURCE {
            return Err(RuntimeError::ReservedSymbol { symbol: key });
        }
        let roster: BTreeSet<ActorId> = roster.into_iter().collect();
        if roster.is_empty() {
            return Err(RuntimeError::EmptyRoster { condition: key });
        }
        if required == 0 {
            return Err(RuntimeError::ZeroRequired { condition: key });
        }
        Ok(Self {
            key,
            kind,
            roster,
            required,
        })
    }

    /// How many roster actors are currently counted in `kind`.
    #[must_use]
    pub fn observed(&self, counters: &ActorCounters) -> usize {
        self.roster
            .iter()
            .filter(|actor| counters.contains(self.kind, **actor))
            .count()
    }

    /// Whether the condition holds right now.
    #[must_use]
    pub fn satisfied(&self, counters: &ActorCounters) -> bool {
        self.observed(counters) >= self.required
    }
}

/// What a satisfied [`CountCondition`] does.
///
/// The reaction is declared beside the condition, so a count can never *become*
/// a mission ending by accident: an objective whose condition is satisfied
/// reports itself unless the declaration also says what that means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountReaction {
    /// Report the condition and nothing else.
    ReportOnly,
    /// Move a declared objective to a declared state — the unlock, reset,
    /// succeed or fail of F39 non-negotiable behavior 3.
    SetObjectiveState {
        objective: SymbolId,
        state: ObjectiveState,
    },
    /// Request a terminal outcome. This is how a *protected* actor's declared
    /// category fails a mission: the declaration names both the roster and the
    /// outcome, so "the mission failed" and "a raider despawned" cannot be
    /// confused.
    Finish(TerminalOutcome),
}

/// When a hidden objective may be shown (F39 non-negotiable behavior 5: "show
/// objectives only when the original reveal rules allow").
///
/// **The original reveal rules are unmeasured.** These are the declared hooks a
/// program drives, and [`ObjectiveRuntime::is_visible`] is the only way to ask,
/// so no consumer can show an objective by reading its state instead of its
/// rule. A reveal rule firing *is* the reveal: a `Hidden` objective moves to
/// [`ObjectiveState::Pending`] at that moment, and until then no declared action
/// may move it out of `Hidden` at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevealRule {
    /// Shown from the first tick. A declaration that pairs this with a `Hidden`
    /// initial state is refused: it would say both "hidden" and "shown".
    Immediate,
    /// Shown when a declared count condition first latches.
    OnCondition { condition: SymbolId },
    /// Shown when a declared timer runs out.
    OnTimer { timer: SymbolId },
    /// Shown when a named mission signal is raised.
    OnSignal { signal: SymbolId },
    /// Shown when a declared objective reaches a declared state.
    OnObjectiveState {
        objective: SymbolId,
        state: ObjectiveState,
    },
}

/// What completing a declared objective means for the mission.
///
/// A state change is a fact; the mission's ending is a declaration. Keeping the
/// two apart is what makes AC02's collision *expressible*: the same tick can both
/// complete an objective (which requests [`TerminalOutcome::Success`] here) and
/// lose a protected actor (which requests failure through a
/// [`CountReaction::Finish`]), and the declared precedence decides between two
/// **requests** rather than between a fact and a guess.
///
/// A mission *failure* is not declared here: it belongs to the declared condition
/// that watches for it, as a [`CountReaction::Finish`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ObjectiveCompletion {
    /// The objective latches; the mission carries on.
    #[default]
    Continue,
    /// The objective's completion requests this terminal outcome.
    Requests(TerminalOutcome),
}

/// The declared **nap time** a `NAP` completion effect carries.
///
/// F39-E2 measured the shape — every `NAP_OBJECTIVE_WHEN_I_COMPLETE` site in
/// the original's objective records carries one number after its objective list
/// (42 distinct values between 0.5 and 170 across 417 sites) and no other
/// completion effect site carries one — and M01-B-OBJ-1 measured what it is:
/// the loader's own diagnostic is "NAP_OBJECTIVE_WHEN_I_COMPLETE has no
/// nap_time specified", a missing value stores `0.3`, and the engine writes
/// the number into the target's re-wake timer slot, comparing it against
/// accumulated per-pass steps.
///
/// What stays unmeasured is the **unit**: the per-pass step is the engine's
/// global frame step, and reading it as seconds is an inference. This runtime
/// ticks in integers, so converting the nap time into ticks — and with it nap
/// expiry, the rule that ends a nap by itself — would be an invention. The
/// type therefore exists so the time can travel as data without a caller
/// treating it as a duration *of this engine*: nothing here interprets it, and
/// a napped objective leaves [`ObjectiveState::Napping`] only through a wake or
/// a declared move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NapTime(f64);

impl NapTime {
    /// Wraps a finite number, or refuses a non-finite one: a declared value that
    /// is not a number cannot be carried as data either.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    /// The nap time exactly as declared, in the engine's own unit and with no
    /// conversion attached to it.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

/// Which of the completion effects one declaration carries.
///
/// The **spellings** are measured: the engine image reads
/// `WAKE_OBJECTIVE_WHEN_I_COMPLETE` (and its bare alias `WAKE_OBJECTIVE`),
/// `NAP_OBJECTIVE_WHEN_I_COMPLETE`, `KILL_OBJECTIVE_WHEN_I_COMPLETE` and
/// `SLEEP_OBJECTIVE_WHEN_I_COMPLETE` out of an objective block (M01-B-OBJ-1),
/// and the corpus also spells `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` — which the
/// image has **no reader** for, an inertness that stays an inference until an
/// original run settles it. `WAKE` and `WAKEUP` are therefore kept apart here
/// too. What each spelling does is the engine image's measured behaviour
/// (M01-B-OBJ-1); the boundaries it is carried under — no nap expiry, a kill's
/// state kept — are recorded in
/// `docs/findings/2026-10-12-m01-b-obj-1-fu1-runtime-effect-alignment.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CompletionEffectKind {
    /// The named objective is woken: it becomes [`ObjectiveState::Active`],
    /// unless it is not present, has completed, or is already awake — in
    /// which case the rest of this completion's wake list is skipped.
    Wake,
    /// The named objective is put aside for its declared nap time:
    /// [`ObjectiveState::Napping`], carrying a [`NapTime`] this runtime never
    /// converts into ticks.
    Nap,
    /// The named objective is **removed**: its presence flag is cleared and
    /// its state is left alone. It is not an outcome and never sets
    /// [`ObjectiveState::Failed`].
    Kill,
    /// The spelling the corpus writes but the engine image never reads. Kept
    /// apart from [`Wake`](Self::Wake) so a later measurement can settle what,
    /// if anything, it does; until then its move is the designed one.
    Wakeup,
    /// The named objective is put to sleep: [`ObjectiveState::Asleep`]. The
    /// engine reads `SLEEP_OBJECTIVE_WHEN_I_COMPLETE`; no retail record spells
    /// it, so no lowered program carries this kind today.
    Sleep,
}

impl CompletionEffectKind {
    /// The state this effect moves its target to, or [`None`] for a kind that
    /// moves no state at all.
    ///
    /// `Wake` and `Wakeup` both make the target [`ObjectiveState::Active`] —
    /// the engine's wake sets state 1 whatever the target was doing — and are
    /// kept apart only because the corpus spells both while the image reads
    /// one. `Nap` makes the target [`ObjectiveState::Napping`] (engine state
    /// 2) and `Sleep` [`ObjectiveState::Asleep`] (engine state 3). `Kill`
    /// moves nothing: it clears the target's presence flag, which is this
    /// runtime's carried form of the engine's present/active clear.
    ///
    /// **No kind returns [`ObjectiveState::Succeeded`]**, which is what makes
    /// the effect queue non-cascading: an applied effect can never complete an
    /// objective, so it can never queue another effect.
    #[must_use]
    pub const fn moves_to(self) -> Option<ObjectiveState> {
        match self {
            Self::Wake | Self::Wakeup => Some(ObjectiveState::Active),
            Self::Nap => Some(ObjectiveState::Napping),
            Self::Kill => None,
            Self::Sleep => Some(ObjectiveState::Asleep),
        }
    }

    /// The engine's fixed application rank: one completion applies its `WAKE`s
    /// first, then its `NAP`, then its `KILL`s, then its `SLEEP`s, whatever
    /// order the record spells them in (M01-B-OBJ-1, measured at 0x46aa35 →
    /// 0x46ad73 → 0x46addd → 0x46ae11). `Wakeup` takes the wake rank: the
    /// image never reads that spelling, so nothing measured orders it, and
    /// beside `Wake` is this project's designed placement.
    #[must_use]
    pub const fn engine_order(self) -> u8 {
        match self {
            Self::Wake | Self::Wakeup => 0,
            Self::Nap => 1,
            Self::Kill => 2,
            Self::Sleep => 3,
        }
    }

    /// Whether a declaration of this kind carries the measured number beside its
    /// objective list. Only a nap does.
    #[must_use]
    pub const fn carries_argument(self) -> bool {
        matches!(self, Self::Nap)
    }
}

/// What completing one declared objective does to one other objective of the
/// same program.
///
/// Most effects are a **declared** move, applied through
/// [`ObjectiveRuntime::change_objective`](ObjectiveRuntime) like every other
/// declared state change, so they obey the same rules: the target's reveal rule
/// still governs its visibility, the transition table still decides legality,
/// and a refused move is *reported* rather than dropped. A
/// [`CompletionEffectKind::Kill`] is the exception the measurement made: it
/// moves no state, clears the target's presence flag and reports
/// [`ObjectiveEventKind::ObjectiveRemoved`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompletionEffect {
    /// Which effect this is.
    pub kind: CompletionEffectKind,
    /// The objective it acts on.
    pub target: SymbolId,
    /// The number only [`CompletionEffectKind::Nap`] carries: the declared nap
    /// time, in the engine's own unit, never converted into ticks.
    pub argument: Option<NapTime>,
}

impl CompletionEffect {
    /// Builds one effect, refusing a declaration that contradicts the measured
    /// shape: the number belongs to a nap and to nothing else.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::EffectArgument`] when a nap declares no number or another
    /// effect declares one.
    pub fn new(
        kind: CompletionEffectKind,
        target: SymbolId,
        argument: Option<NapTime>,
    ) -> Result<Self, RuntimeError> {
        let effect = Self {
            kind,
            target,
            argument,
        };
        effect.validate()?;
        Ok(effect)
    }

    /// The state this effect moves its target to, or [`None`] for a kill,
    /// which moves no state. See [`CompletionEffectKind::moves_to`].
    #[must_use]
    pub const fn moves_target_to(&self) -> Option<ObjectiveState> {
        self.kind.moves_to()
    }

    /// The check [`new`](Self::new) performs, also applied to a struct literal:
    /// every field is public, so registration is where the shape is enforced.
    fn validate(&self) -> Result<(), RuntimeError> {
        if self.kind.carries_argument() == self.argument.is_none() {
            return Err(RuntimeError::EffectArgument { kind: self.kind });
        }
        Ok(())
    }
}

/// One declared objective's registration.
///
/// `Eq` is not derived: `completion_effects` carries the nap's declared number,
/// and a `f64` is not `Eq`. Wrapping that number in a hand-written `Eq` would
/// buy the derive at the cost of a subtler equality — `0.0 == -0.0` but not
/// their bit patterns — so the bound is dropped instead, as F39-E2 dropped it for
/// the measured effect arguments on the content side.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectiveSpec {
    /// The objective's program symbol.
    pub id: SymbolId,
    /// The stable content id this objective was authored as.
    pub content: ContentId,
    /// Its state before anything happens.
    pub initial: ObjectiveState,
    /// When it may be shown.
    pub reveal: RevealRule,
    /// What completing it means for the mission.
    pub on_complete: ObjectiveCompletion,
    /// What completing it does to *other* objectives, in authored order.
    /// Empty for an objective whose completion moves nothing else, which is
    /// the whole of the behaviour before completion effects existed. The
    /// runtime queues and applies them in the engine's fixed order
    /// ([`CompletionEffectKind::engine_order`]), not in this order.
    pub completion_effects: Vec<CompletionEffect>,
}

/// What one observed event is.
///
/// Every variant is a fact a consumer may act on. None of them *is* the act: an
/// objective unlocks because a declared [`CountReaction`] or [`TimerAction`]
/// said so, never because a crossing occurred.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectiveEventKind {
    /// An actor's declared lifecycle transition was counted. A pilot bailout
    /// and a mission removal are never counted, because they are not one of the
    /// five categories: a bailout is reported as
    /// [`ObjectiveEventKind::PilotBailedOut`] instead, and a mission removal is
    /// reported nowhere.
    Counted { actor: ActorId, kind: CountKind },
    /// A confirmed pilot bailout: the **distinct** mission transition F29 AC04
    /// requires, and never a [`ObjectiveEventKind::Counted`] event however the
    /// policy reads.
    ///
    /// The transition is reported whether or not it changes anything, so a
    /// consumer sees the bailout rather than inferring it from the absence of a
    /// kill. `confirmation` is what asked for it — a [`BailoutConfirmation`]
    /// built from the declared eject edge or from a mission program — and it is
    /// `Some` always, because an unconfirmed bailout produces this event never:
    /// it is refused instead.
    PilotBailedOut {
        /// The actor whose pilot left the airframe.
        actor: ActorId,
        /// What confirmed the departure.
        confirmation: BailoutConfirmation,
    },
    /// A lifecycle transition produced no mission transition and the reason
    /// names why. Refused, not dropped: a caller can tell "the world reported a
    /// destruction after a bailout" from "nothing happened".
    TransitionRefused {
        /// The actor the transition was reported for.
        actor: ActorId,
        /// The transition the caller asked for.
        requested: MissionTransition,
        /// Why nothing was recorded.
        reason: BailoutRefusal,
    },
    /// A count condition latched: it is satisfied and stays satisfied.
    ConditionMet {
        condition: SymbolId,
        kind: CountKind,
        observed: usize,
    },
    /// A swept trigger crossing. The crossing's own `CrossingKind` distinguishes
    /// entry from exit, and a body that stayed inside emits nothing.
    TriggerCrossed(TriggerEvent),
    /// A mission signal was raised. It becomes eligible to arm a
    /// [`TimerStart::OnSignal`] timer on the **next** tick.
    SignalRaised { signal: SymbolId },
    /// An objective's state changed, through a declared action.
    ObjectiveChanged {
        objective: SymbolId,
        from: ObjectiveState,
        to: ObjectiveState,
    },
    /// A declared state change was refused; the objective kept its state.
    ObjectiveChangeRefused {
        objective: SymbolId,
        from: ObjectiveState,
        to: ObjectiveState,
    },
    /// A `KILL` completion effect removed the objective from play: its
    /// presence flag is cleared and `from` is the state it kept. This is not
    /// an outcome — the objective's state is unchanged, no terminal result is
    /// requested, and every later wake skips it.
    ObjectiveRemoved {
        objective: SymbolId,
        from: ObjectiveState,
    },
    /// A completion's wake list stopped early: `objective`, which the list
    /// reached while already [`ObjectiveState::Active`], ended the whole list
    /// (the engine's measured early-out), so the wake targets after it in the
    /// same completion were not woken.
    WakeStopped {
        /// The completing objective whose wake list stopped.
        source: SymbolId,
        /// The already-awake target that stopped it.
        objective: SymbolId,
    },
    /// A hidden objective became visible. `state` is the state it now holds:
    /// [`ObjectiveState::Pending`] when it was `Hidden`, unchanged otherwise.
    ObjectiveRevealed {
        objective: SymbolId,
        state: ObjectiveState,
    },
    /// A timer was armed, by its declared start or by a program action.
    TimerArmed { timer: SymbolId, via: TimerStart },
    /// A timer ran out. Its one declared action follows, later in this tick.
    TimerExpired { timer: SymbolId },
    /// A timer request or start was refused; the timer kept its state.
    TimerRefused { timer: SymbolId, reason: TimerError },
    /// A declared request or action named a declaration this runtime does not
    /// have. Nothing was applied and the reference is named, so a request is
    /// never dropped in silence.
    RequestRefused {
        request: SymbolId,
        reason: RuntimeError,
    },
    /// A spawn group was admitted. `group` is the program's spawn-group symbol
    /// and `instances` are the stable per-session instance ids it took, so the
    /// host instantiates exactly the ids this event names.
    SpawnAdmitted {
        key: IdempotencyKey,
        group: SymbolId,
        instances: Vec<ActorId>,
    },
    /// A repeated spawn key was refused, carrying the ids the first admission
    /// allocated, so a wave cannot be spawned twice under one key.
    SpawnRefused {
        key: IdempotencyKey,
        group: SymbolId,
        instances: Vec<ActorId>,
    },
    /// A dialogue cue was played once.
    CueEmitted {
        key: IdempotencyKey,
        dialogue: ContentId,
    },
    /// A repeated cue key was refused; the dialogue is not repeated.
    CueRefused {
        key: IdempotencyKey,
        dialogue: ContentId,
    },
    /// An optional reward intent. Never terminal.
    OptionalReward { reward: ContentId },
    /// The mission's outcome settled this tick. `superseded` names the
    /// same-tick requests that lost, so the precedence decision is auditable.
    OutcomeSettled {
        outcome: TerminalOutcome,
        superseded: Vec<TerminalOutcome>,
    },
}

/// One ordered event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveEvent {
    /// The stable observation key: session, tick, source symbol, sequence.
    pub key: EventKey,
    pub kind: ObjectiveEventKind,
}

impl ObjectiveEvent {
    /// The source symbol this event attributes to.
    #[must_use]
    pub const fn source(&self) -> SymbolId {
        self.key.source
    }
}

/// Everything one tick's observation carries into the runtime.
///
/// A tick's input is *facts and declared requests only*. No field applies an
/// effect, so a caller cannot unlock, reset, succeed or fail an objective except
/// through the declared request types here or through a [`CountReaction`] /
/// [`TimerAction`] the declaration already contains. A request naming a
/// declaration the runtime does not have is refused and reported
/// ([`ObjectiveEventKind::RequestRefused`]) rather than dropped in silence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickInput<'a> {
    /// The tick these facts belong to. Must be strictly after the last stepped
    /// tick.
    pub tick: Tick,
    /// Whole committed ticks the session's gameplay clock committed since the
    /// last step. A paused frame commits zero, so no timer advances, and there
    /// is no wall-time field, so a frame delta cannot become a variable dt.
    pub committed_ticks: u64,
    /// Lifecycle transitions recorded this tick, in observation order. An actor
    /// recorded twice in one category counts once.
    pub lifecycles: &'a [(ActorId, LifecycleKind)],
    /// The movement each watched actor made this tick, as the **real** segment
    /// from its previous position. An actor absent from this list is not
    /// observed, so no crossing is invented for it. A watched actor listed twice
    /// is refused: a trigger observes one movement per tick.
    pub movements: &'a [(ActorId, Movement)],
    /// Mission signals raised this tick.
    pub signals: &'a [SymbolId],
    /// The tick's declared timer requests.
    pub timer_requests: &'a [TimerRequest],
    /// The tick's declared objective state changes, as `(objective, state)`.
    pub objective_requests: &'a [(SymbolId, ObjectiveState)],
    /// The tick's declared terminal outcome requests, as
    /// `(requesting source, outcome)`, so a refused request names who asked.
    pub terminal_requests: &'a [(SymbolId, TerminalOutcome)],
}

impl TickInput<'_> {
    /// An input for `tick` carrying nothing but the tick.
    #[must_use]
    pub const fn at(tick: Tick) -> Self {
        Self {
            tick,
            committed_ticks: 0,
            lifecycles: &[],
            movements: &[],
            signals: &[],
            timer_requests: &[],
            objective_requests: &[],
            terminal_requests: &[],
        }
    }
}

/// Why a tick stopped early. Never a mission failure: the tick applied nothing
/// and the runtime keeps every bit of its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The tick's declared facts could produce more events than
    /// [`RuntimeLimits::max_events_per_tick`] allows. Checked **before** any
    /// effect is applied, so a bounded tick changes nothing at all.
    EventBudget {
        at_tick: Tick,
        limit: usize,
        declared: usize,
    },
    /// The mission's outcome was already settled, so this tick did no work.
    OutcomeSettled { settled_at: Tick },
}

/// Result of one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectiveTick {
    /// The tick these events belong to.
    pub tick: Tick,
    /// Sorted by [`ObjectiveEvent::key`].
    pub events: Vec<ObjectiveEvent>,
    /// The session's outcome after this tick, when it has settled.
    pub outcome: Option<TerminalOutcome>,
    /// Set when a bound stopped the tick before any effect was applied.
    pub stop: Option<StopReason>,
}

impl ObjectiveTick {
    /// Whether the tick applied nothing.
    #[must_use]
    pub const fn is_stopped(&self) -> bool {
        self.stop.is_some()
    }

    /// The events matching `predicate`, in observation order.
    #[must_use]
    pub fn filter(&self, predicate: impl Fn(&ObjectiveEventKind) -> bool) -> Vec<&ObjectiveEvent> {
        self.events
            .iter()
            .filter(|event| predicate(&event.kind))
            .collect()
    }

    /// The first event matching `predicate`, in observation order.
    #[must_use]
    pub fn first(
        &self,
        predicate: impl Fn(&ObjectiveEventKind) -> bool,
    ) -> Option<&ObjectiveEvent> {
        self.events.iter().find(|event| predicate(&event.kind))
    }
}

/// Why a runtime operation was refused. Nothing here is a mission failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    /// The tick is not after the last stepped one: a replay or a retry cannot
    /// re-evaluate a tick.
    NotAdvancing { last: Tick, given: Tick },
    /// Two objectives claim the same symbol.
    DuplicateObjective { objective: SymbolId },
    /// Two timers claim the same symbol.
    DuplicateTimer { timer: SymbolId },
    /// Two count conditions claim the same symbol.
    DuplicateCondition { condition: SymbolId },
    /// The same `(symbol, actor)` pair is registered twice.
    DuplicateTrigger { trigger: SymbolId, actor: ActorId },
    /// The timer table is full.
    TimerTableFull { limit: usize },
    /// A count condition declares no actor, so nothing can ever satisfy it.
    EmptyRoster { condition: SymbolId },
    /// A count condition requires zero actors, so it is satisfied by
    /// construction and observes nothing.
    ZeroRequired { condition: SymbolId },
    /// A declaration used the reserved [`ACTOR_EVENT_SOURCE`].
    ReservedSymbol { symbol: SymbolId },
    /// A timer request or start named a timer this runtime does not have.
    UnknownTimer { timer: SymbolId },
    /// An objective request or action named an objective this runtime does not
    /// have.
    UnknownObjective { objective: SymbolId },
    /// A declaration pairs [`RevealRule::Immediate`] with a `Hidden` initial
    /// state, which says both "hidden" and "shown from the first tick".
    HiddenButImmediate { objective: SymbolId },
    /// **Different** completing objectives declare **different** effects on
    /// the same target.
    ///
    /// Within *one* declaration, two **different** effects on one target are
    /// no longer refused: M01-B-OBJ-1
    /// measured the engine's fixed order (`WAKE` → `NAP` → `KILL` → `SLEEP`),
    /// which resolves the corpus's one such block (`zbd/c3/m05` `OBJECTIVE8`:
    /// the target is woken first, then napped, so it ends napping). Across
    /// *two* declarations it stays refused rather than ordered: the engine
    /// completes one objective per pass, so the collision cannot arise there,
    /// and this runtime may complete both in one tick — which of them should
    /// apply last depends on the pass-origin rule, which is unmeasured
    /// (M01-B-OBJ-1's open question 2). Applying one in the other's place would
    /// be inventing that rule. Registration order decides nothing either,
    /// because the scan covers every registered objective and not only the one
    /// before this one. The *same* pair twice within one declaration is the
    /// other refusal, [`Self::RepeatedCompletionEffect`].
    AmbiguousCompletionEffect { source: SymbolId, target: SymbolId },
    /// One objective declares the same completion effect on the same target
    /// **twice inside one [`ObjectiveSpec`]**.
    ///
    /// This is the runtime half of F39-E6's refusal
    /// (`docs/findings/2026-10-04-f39-e6-repeated-completion-effect-key.md`).
    /// The declared schema refuses a block spelling one completion-effect key
    /// twice by name (`ObjectivesSchemaError::RepeatedCompletionEffect`,
    /// `cs_content`), and that schema gate is the only way a lowered program
    /// gets in — but an `ObjectiveSpec` built by hand bypasses it, and a caller
    /// seeing the repeat accepted here would read it as a supported shape.
    ///
    /// The refusal is tied to the named unknown `UNMEASURED_REPEATED_EFFECT_KEY`
    /// (`cs_content::objectives`): the shape occurs nowhere in the readable
    /// corpus, so what a second site does — replace the first, be ignored, or
    /// apply beside it — is unmeasured, and no reimplementation may choose one
    /// of those by silently keeping a single effect.
    ///
    /// Deliberately per `(kind, target)` pair of **one** spec, mirroring the
    /// schema check: one kind naming two *different* targets is one
    /// multi-target site's residue and stays legal, and the same pair named by
    /// *two different* objectives agrees on what happens and also stays legal.
    /// Only [`Self::AmbiguousCompletionEffect`] looks across declarations, and
    /// it is about *different* kinds: the two name different shapes — a repeat
    /// is never a conflict and a conflict is never a repeat — though one spec
    /// can carry both, and then the cross-declaration scan that runs first is
    /// what gets reported.
    RepeatedCompletionEffect {
        /// The objective whose spec carries the repeated effect.
        source: SymbolId,
        /// The objective both sites name.
        target: SymbolId,
        /// The effect kind declared twice.
        kind: CompletionEffectKind,
    },
    /// An objective declares a completion effect on itself.
    ///
    /// The effect fires *because* the objective completed, so by the time it is
    /// applied the objective holds `Succeeded`; no effect kind moves a target out
    /// of a final state, so the declaration could never apply.
    SelfCompletionEffect { objective: SymbolId },
    /// A completion effect's declared number contradicts the measured shape: a
    /// nap without its number, or a number on an effect that carries none.
    EffectArgument { kind: CompletionEffectKind },
    /// A trigger movement was refused; see [`TriggerError`].
    Trigger(TriggerError),
    /// A cross-objective address named an objective past the record's block
    /// count; see [`AddressRefusal`] and the named rule
    /// [`crate::objectives::address::OUT_OF_RANGE_OBJECTIVE_ADDRESS`].
    ///
    /// Refused, not clamped: the address the record spelled is carried whole
    /// so the report says which address was past which record, and no
    /// neighbouring objective is moved in its place.
    Address(AddressRefusal),
    /// A family directive's arguments do not have the shape the parse can
    /// produce (a list that is not a list of integers, a nap without a number).
    MalformedDirective { operation: DirectiveOperation },
    /// A family directive's list spells more addresses than the engine's
    /// 15-entry window can store (`0x469af0`'s `cmpl $0xf` cap, carried by
    /// `cs_content::objectives_engine` as `ENGINE_LIST_CAPACITY`). The
    /// window keeps its last slot for the `−1` terminator, so a spelled
    /// prefix of 15 or more leaves none and the whole list is refused —
    /// never truncated, the same refusal `TargetList::from_record` gives
    /// the shape.
    DirectiveListTooLong {
        /// The operation whose list overran the window.
        operation: DirectiveOperation,
        /// How many addresses the spelled prefix holds.
        len: usize,
    },
    /// A family directive was decoded and its addresses checked, but the
    /// lifecycle has no consumer for it yet, so nothing was applied.
    UnexecutedDirective { operation: DirectiveOperation },
    /// A directive emission belongs to another session generation.
    StaleDirective {
        expected: SessionGeneration,
        given: SessionGeneration,
    },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAdvancing { last, given } => write!(
                f,
                "the last stepped tick is {} and this tick is {}",
                last.0, given.0
            ),
            Self::DuplicateObjective { objective } => {
                write!(f, "objective {objective:?} is already registered")
            }
            Self::DuplicateTimer { timer } => write!(f, "timer {timer:?} is already registered"),
            Self::DuplicateCondition { condition } => {
                write!(f, "count condition {condition:?} is already registered")
            }
            Self::DuplicateTrigger { trigger, actor } => {
                write!(
                    f,
                    "trigger {trigger:?} for actor {actor:?} is already registered"
                )
            }
            Self::TimerTableFull { limit } => {
                write!(f, "the timer table already holds its {limit} timers")
            }
            Self::EmptyRoster { condition } => write!(
                f,
                "count condition {condition:?} declares no actor and can never be met"
            ),
            Self::ZeroRequired { condition } => write!(
                f,
                "count condition {condition:?} requires zero actors and is met by construction"
            ),
            Self::ReservedSymbol { symbol } => {
                write!(f, "{symbol:?} is reserved for actor-keyed events")
            }
            Self::UnknownTimer { timer } => {
                write!(f, "no timer {timer:?} is declared in this runtime")
            }
            Self::UnknownObjective { objective } => {
                write!(f, "no objective {objective:?} is declared in this runtime")
            }
            Self::HiddenButImmediate { objective } => write!(
                f,
                "objective {objective:?} declares itself hidden and shown from the first tick"
            ),
            Self::AmbiguousCompletionEffect { source, target } => write!(
                f,
                "objective {source:?} declares a completion effect on {target:?} that a different objective already declares differently, and the engine completes one objective per pass, so which of the two applies when both complete in one tick is unmeasured"
            ),
            Self::RepeatedCompletionEffect {
                source,
                target,
                kind,
            } => write!(
                f,
                "objective {source:?} declares a {kind:?} completion effect on {target:?} twice, and no measured rule says what the second site does (UNMEASURED_REPEATED_EFFECT_KEY)"
            ),
            Self::SelfCompletionEffect { objective } => write!(
                f,
                "objective {objective:?} declares a completion effect on itself, which can only apply once it has completed"
            ),
            Self::EffectArgument { kind } => write!(
                f,
                "a {kind:?} completion effect declares {:?} the number only a nap carries",
                if kind.carries_argument() { "no" } else { "a" }
            ),
            Self::Trigger(error) => write!(f, "trigger movement refused: {error}"),
            Self::Address(refusal) => write!(f, "{refusal}"),
            Self::MalformedDirective { operation } => {
                write!(
                    f,
                    "{operation:?} directive has an argument shape the parse cannot produce"
                )
            }
            Self::DirectiveListTooLong { operation, len } => write!(
                f,
                "{operation:?} directive's list spells {len} addresses before its terminator; \
                 the engine's 15-entry list window stores at most 14 plus the terminator"
            ),
            Self::UnexecutedDirective { operation } => write!(
                f,
                "{operation:?} directive was decoded but the objective lifecycle has no consumer for it; nothing was applied"
            ),
            Self::StaleDirective { expected, given } => write!(
                f,
                "directive emission from session {given:?} given to session {expected:?}"
            ),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<TriggerError> for RuntimeError {
    fn from(error: TriggerError) -> Self {
        Self::Trigger(error)
    }
}

impl From<AddressRefusal> for RuntimeError {
    fn from(refusal: AddressRefusal) -> Self {
        Self::Address(refusal)
    }
}

/// The bounds one session's objective runtime runs under.
///
/// The contract requires bounded control flow with a diagnostic instead of
/// silently skipped work, so [`ObjectiveRuntime::step`] refuses a tick whose
/// *declared* facts already exceed `max_events_per_tick` **before** it applies
/// anything, and the [`StopReason::EventBudget`] diagnostic names the tick, the
/// bound and how much work was declared. `max_timers` bounds the declaration
/// table the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeLimits {
    /// Most events one tick may produce.
    pub max_events_per_tick: usize,
    /// Most timers one session may declare.
    pub max_timers: usize,
}

impl Default for RuntimeLimits {
    /// Bounds far above a normal mission's tick. **A designed bound, not a
    /// measured original limit.**
    fn default() -> Self {
        Self {
            max_events_per_tick: 4096,
            max_timers: 1024,
        }
    }
}

/// The continuous objective runtime of one mission session.
///
/// One session owns one runtime. A retry is a new
/// [`SessionGeneration`](cs_script::runtime::SessionGeneration) and a new
/// runtime: no objective state, counter, trigger, timer, ledger entry or latch
/// survives, because none of them is reachable from the old one — which is
/// F39-C's retry acceptance case's precondition, and this stage's reason for
/// owning every piece of mutable state itself.
#[derive(Clone, Debug)]
pub struct ObjectiveRuntime {
    session: SessionGeneration,
    precedence: TerminalPrecedence,
    limits: RuntimeLimits,
    objectives: BTreeMap<SymbolId, TrackedObjective>,
    conditions: BTreeMap<SymbolId, CountBinding>,
    latched: BTreeSet<SymbolId>,
    triggers: BTreeMap<(SymbolId, ActorId), SweptTrigger>,
    counters: ActorCounters,
    /// The mission-transition ledger: the first mission transition each actor
    /// reached, plus the bailout confirmations recorded ahead of them (F29-C.4).
    ///
    /// It is what keeps a destruction report arriving after a bailout from
    /// becoming a kill, so phase 1 consults it for every lifecycle transition
    /// this module owns a transition for and the counters follow it.
    transitions: MissionTransitions,
    timers: BTreeMap<SymbolId, MissionTimer>,
    /// Signals raised on an earlier tick, eligible to arm timers now. Consumed
    /// by the tick that observes them, so a signal arms a waiting deadline once.
    eligible_signals: BTreeSet<SymbolId>,
    /// Signals raised during the tick being evaluated. They become eligible when
    /// the tick ends, which is how "an action does not recurse into callbacks"
    /// is made structural rather than a convention.
    raised_signals: BTreeSet<SymbolId>,
    ledger: EmissionLedger,
    latch: TerminalLatch,
    /// The completion effects of every objective that completed this tick, waiting
    /// for phase 7. A queue and not a call: applying an effect inside the state
    /// change that raised it would be the contract's forbidden recursion into a
    /// callback. Anything queued while the queue is being drained waits for the
    /// next tick — and no effect kind can queue anything, because none of them
    /// moves its target to `Succeeded`.
    pending_effects: Vec<QueuedEffect>,
    /// Directive emissions already queued, so one is applied exactly once.
    consumed_directives: BTreeSet<ExecutionKey>,
    /// The next instance id an admitted spawn group takes. It starts at
    /// [`SPAWNED_ACTOR_BASE`] and only ever increases, so no spawned instance
    /// ever wears a placed actor's id.
    next_instance: u32,
    /// The tick's terminal requests raised by reactions and timer actions.
    requests: Vec<(SymbolId, TerminalOutcome)>,
    sequence: u32,
    last_tick: Option<Tick>,
}

impl ObjectiveRuntime {
    /// A runtime for one session.
    ///
    /// `precedence` travels with the runtime so no consumer can resolve a
    /// collision with a different rule than the session was built with.
    #[must_use]
    pub fn new(
        session: SessionGeneration,
        precedence: TerminalPrecedence,
        limits: RuntimeLimits,
    ) -> Self {
        Self {
            session,
            precedence,
            limits,
            objectives: BTreeMap::new(),
            conditions: BTreeMap::new(),
            latched: BTreeSet::new(),
            triggers: BTreeMap::new(),
            counters: ActorCounters::default(),
            transitions: MissionTransitions::default(),
            timers: BTreeMap::new(),
            eligible_signals: BTreeSet::new(),
            raised_signals: BTreeSet::new(),
            ledger: EmissionLedger::new(session),
            latch: TerminalLatch::new(),
            pending_effects: Vec::new(),
            consumed_directives: BTreeSet::new(),
            next_instance: SPAWNED_ACTOR_BASE,
            requests: Vec::new(),
            sequence: 0,
            last_tick: None,
        }
    }

    /// The session generation this runtime owns.
    #[must_use]
    pub const fn session(&self) -> SessionGeneration {
        self.session
    }

    /// The declared terminal precedence.
    #[must_use]
    pub const fn precedence(&self) -> TerminalPrecedence {
        self.precedence
    }

    /// The bounds this runtime runs under.
    #[must_use]
    pub const fn limits(&self) -> RuntimeLimits {
        self.limits
    }

    /// The last stepped tick.
    #[must_use]
    pub const fn last_tick(&self) -> Option<Tick> {
        self.last_tick
    }

    /// The mission's outcome once it has settled.
    #[must_use]
    pub const fn outcome(&self) -> Option<TerminalOutcome> {
        self.latch.outcome()
    }

    /// Whether the mission's outcome is settled.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        self.latch.is_settled()
    }

    // -----------------------------------------------------------------------
    // Declaration
    // -----------------------------------------------------------------------

    /// Registers one objective.
    ///
    /// Its [`completion_effects`](ObjectiveSpec::completion_effects) are checked
    /// here rather than in [`CompletionEffect::new`], because every field of a
    /// [`CompletionEffect`] is public and a struct literal would otherwise let a
    /// declaration slip past: the measured number's shape, an effect on the
    /// objective itself, a `(kind, target)` pair this same spec already named
    /// (F39-E6's residue, refused rather than deduplicated), and the one rule
    /// that cannot be expressed in the effect alone — a target named with
    /// **different** effects by two *different* objectives, which is refused
    /// rather than ordered because the engine completes one objective per pass
    /// and the pass-origin rule is unmeasured (M01-B-OBJ-1). One declaration
    /// carrying two *different* effects on one target is no longer refused:
    /// the engine's fixed order decides it; the same pair spelled twice still
    /// is.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::DuplicateObjective`], [`RuntimeError::ReservedSymbol`],
    /// [`RuntimeError::HiddenButImmediate`],
    /// [`RuntimeError::AmbiguousCompletionEffect`],
    /// [`RuntimeError::RepeatedCompletionEffect`],
    /// [`RuntimeError::SelfCompletionEffect`] and
    /// [`RuntimeError::EffectArgument`].
    pub fn add_objective(&mut self, spec: ObjectiveSpec) -> Result<(), RuntimeError> {
        if spec.id == ACTOR_EVENT_SOURCE {
            return Err(RuntimeError::ReservedSymbol { symbol: spec.id });
        }
        if self.objectives.contains_key(&spec.id) {
            return Err(RuntimeError::DuplicateObjective { objective: spec.id });
        }
        if spec.initial == ObjectiveState::Hidden && spec.reveal == RevealRule::Immediate {
            return Err(RuntimeError::HiddenButImmediate { objective: spec.id });
        }
        for effect in &spec.completion_effects {
            effect.validate()?;
            if effect.target == spec.id {
                return Err(RuntimeError::SelfCompletionEffect { objective: spec.id });
            }
        }
        self.check_uncontested_targets(spec.id, &spec.completion_effects)?;
        Self::check_no_repeated_effects(spec.id, &spec.completion_effects)?;
        let revealed = spec.reveal == RevealRule::Immediate;
        self.objectives.insert(
            spec.id,
            TrackedObjective {
                content: spec.content,
                cell: ObjectiveCell::new(spec.initial),
                reveal: spec.reveal,
                revealed,
                on_complete: spec.on_complete,
                completion_effects: spec.completion_effects,
                present: true,
                nap_time: None,
            },
        );
        Ok(())
    }

    /// Refuses a target that a **different** registered objective already
    /// names with a **different** effect kind.
    ///
    /// M01-B-OBJ-1 measured the engine's fixed order (`WAKE` → `NAP` →
    /// `KILL` → `SLEEP`), which decides one declaration that names one target
    /// twice with two different kinds — the corpus's `zbd/c3/m05`
    /// `OBJECTIVE8` shape — so conflicts *within* one declaration are applied
    /// in that order and never refused here. (The *same* pair twice within one
    /// declaration is the other shape and is refused by
    /// [`Self::check_no_repeated_effects`].) Across declarations the question
    /// stays open: the engine completes one objective per pass, so two
    /// completions never collide there, and which of two same-tick completions
    /// should apply last depends on the pass-origin rule, which is unmeasured.
    /// Registration order decides nothing either, which is why the scan covers
    /// every registered objective — and every kind each of them names, not
    /// only the first kind its authored order spells — so a declaration that
    /// already carries two effects on one target still collides here with a
    /// *different* completion naming that target with either of them.
    fn check_uncontested_targets(
        &self,
        source: SymbolId,
        effects: &[CompletionEffect],
    ) -> Result<(), RuntimeError> {
        let mut declared: BTreeMap<SymbolId, Vec<(SymbolId, CompletionEffectKind)>> =
            BTreeMap::new();
        for (id, tracked) in &self.objectives {
            for effect in &tracked.completion_effects {
                declared
                    .entry(effect.target)
                    .or_default()
                    .push((*id, effect.kind));
            }
        }
        for effect in effects {
            let named = declared
                .get(&effect.target)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if named
                .iter()
                .any(|(other, kind)| *other != source && *kind != effect.kind)
            {
                return Err(RuntimeError::AmbiguousCompletionEffect {
                    source,
                    target: effect.target,
                });
            }
            declared
                .entry(effect.target)
                .or_default()
                .push((source, effect.kind));
        }
        Ok(())
    }

    /// Refuses a `(kind, target)` pair the **one** [`ObjectiveSpec`] being
    /// registered already names — the residue of a block spelling one
    /// completion-effect key twice (F39-E6).
    ///
    /// Mirrors the **key** of `ObjectivesSchemaError::RepeatedCompletionEffect`
    /// (`cs_content::objectives`) — one declaring objective, one
    /// `(kind, target)` pair — so the schema gate and this registration gate
    /// refuse exactly the same repeat shapes. One kind naming two
    /// *different* targets stays legal — that is one multi-target site's
    /// residue as much as it is a repeat — and the same pair named by two
    /// *different* specs stays legal, because two declarations that agree on
    /// what happens have no second site to resolve.
    ///
    /// The schema gate is the only way a lowered program reaches this
    /// runtime, so no authored record arrives here carrying the shape; a
    /// hand-built [`ObjectiveSpec`] bypasses it, which is why the refusal
    /// exists on both sides of the boundary instead of only in the schema.
    /// Nothing about the second site's meaning is modelled — replace, ignore
    /// and stack are all unmeasured
    /// (`UNMEASURED_REPEATED_EFFECT_KEY`, `cs_content::objectives`) — so the
    /// pair is refused by name rather than applied as one effect.
    ///
    /// Called after [`Self::check_uncontested_targets`] so this refusal is
    /// strictly additive: a spec that also names a target another
    /// *registered* objective names differently is refused as
    /// [`Self::AmbiguousCompletionEffect`] exactly as it was before this gate
    /// existed, and only a spec with no such conflict reaches the repeat. The
    /// schema interleaves its two checks per effect, so on a spec whose
    /// conflicting effect comes *after* its repeat it names the repeat where
    /// this gate names the conflict; what the two gates agree on, and all a
    /// lowered program can reach, is which shapes are refused at all — the
    /// schema gate runs ahead of this one and carries neither through.
    fn check_no_repeated_effects(
        source: SymbolId,
        effects: &[CompletionEffect],
    ) -> Result<(), RuntimeError> {
        let mut declared: BTreeSet<(CompletionEffectKind, SymbolId)> = BTreeSet::new();
        for effect in effects {
            if !declared.insert((effect.kind, effect.target)) {
                return Err(RuntimeError::RepeatedCompletionEffect {
                    source,
                    target: effect.target,
                    kind: effect.kind,
                });
            }
        }
        Ok(())
    }

    /// Registers one count condition and what satisfying it does.
    ///
    /// The reserved [`ACTOR_EVENT_SOURCE`] is refused here and not only by
    /// [`CountCondition::new`], because every field of a `CountCondition` is
    /// public and a struct literal would otherwise let a condition claim the
    /// source a counted event reports under.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::DuplicateCondition`], [`RuntimeError::EmptyRoster`],
    /// [`RuntimeError::ZeroRequired`] and [`RuntimeError::ReservedSymbol`].
    pub fn add_condition(
        &mut self,
        condition: CountCondition,
        reaction: CountReaction,
    ) -> Result<(), RuntimeError> {
        if condition.key == ACTOR_EVENT_SOURCE {
            return Err(RuntimeError::ReservedSymbol {
                symbol: condition.key,
            });
        }
        if self.conditions.contains_key(&condition.key) {
            return Err(RuntimeError::DuplicateCondition {
                condition: condition.key,
            });
        }
        if condition.roster.is_empty() {
            return Err(RuntimeError::EmptyRoster {
                condition: condition.key,
            });
        }
        if condition.required == 0 {
            return Err(RuntimeError::ZeroRequired {
                condition: condition.key,
            });
        }
        self.conditions.insert(
            condition.key,
            CountBinding {
                condition,
                reaction,
            },
        );
        Ok(())
    }

    /// Registers one swept trigger.
    ///
    /// The `(symbol, actor)` pair is the identity, so one authored volume may
    /// legitimately be watched by several actors.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::DuplicateTrigger`] and [`RuntimeError::ReservedSymbol`].
    pub fn add_trigger(&mut self, trigger: SweptTrigger) -> Result<(), RuntimeError> {
        let (id, actor) = (trigger.id(), trigger.actor());
        if id == ACTOR_EVENT_SOURCE {
            return Err(RuntimeError::ReservedSymbol { symbol: id });
        }
        if self.triggers.contains_key(&(id, actor)) {
            return Err(RuntimeError::DuplicateTrigger { trigger: id, actor });
        }
        self.triggers.insert((id, actor), trigger);
        Ok(())
    }

    /// Registers one declared timer.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::DuplicateTimer`], [`RuntimeError::TimerTableFull`] and
    /// [`RuntimeError::ReservedSymbol`].
    pub fn add_timer(&mut self, timer: MissionTimer) -> Result<(), RuntimeError> {
        if timer.id() == ACTOR_EVENT_SOURCE {
            return Err(RuntimeError::ReservedSymbol { symbol: timer.id() });
        }
        if self.timers.contains_key(&timer.id()) {
            return Err(RuntimeError::DuplicateTimer { timer: timer.id() });
        }
        if self.timers.len() >= self.limits.max_timers {
            return Err(RuntimeError::TimerTableFull {
                limit: self.limits.max_timers,
            });
        }
        self.timers.insert(timer.id(), timer);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Inspection
    // -----------------------------------------------------------------------

    /// An objective's current state.
    #[must_use]
    pub fn objective_state(&self, objective: SymbolId) -> Option<ObjectiveState> {
        self.objectives.get(&objective).map(TrackedObjective::state)
    }

    /// Whether an objective is still *present*: not removed by a
    /// [`CompletionEffectKind::Kill`]. A killed objective keeps its state and
    /// is skipped by every later wake, so this is the one query that tells
    /// "removed from play" apart from "shown in a state".
    #[must_use]
    pub fn is_present(&self, objective: SymbolId) -> Option<bool> {
        self.objectives
            .get(&objective)
            .map(|tracked| tracked.present)
    }

    /// The nap time the last applied `NAP` effect declared for this
    /// objective, in the engine's own unit. Carried as data; this runtime
    /// never converts it into ticks and never ends a nap by it.
    #[must_use]
    pub fn objective_nap_time(&self, objective: SymbolId) -> Option<NapTime> {
        self.objectives
            .get(&objective)
            .and_then(|tracked| tracked.nap_time)
    }

    /// The content id an objective was authored as.
    #[must_use]
    pub fn objective_content(&self, objective: SymbolId) -> Option<&ContentId> {
        self.objectives
            .get(&objective)
            .map(|tracked| &tracked.content)
    }

    /// Whether the player may be shown this objective right now.
    ///
    /// The single place visibility is answered. An objective's own
    /// [`ObjectiveState::is_visible`] is not enough: a `Pending` objective whose
    /// reveal rule has not fired is not shown.
    #[must_use]
    pub fn is_visible(&self, objective: SymbolId) -> bool {
        self.objectives
            .get(&objective)
            .is_some_and(|tracked| tracked.revealed && tracked.state().is_visible())
    }

    /// How many actors are counted in one category. Never a total across
    /// categories.
    #[must_use]
    pub fn counted(&self, kind: CountKind) -> usize {
        self.counters.count(kind)
    }

    /// Whether one actor is counted in one category.
    #[must_use]
    pub fn is_counted(&self, kind: CountKind, actor: ActorId) -> bool {
        self.counters.contains(kind, actor)
    }

    /// Whether one count condition has latched.
    #[must_use]
    pub fn condition_met(&self, condition: SymbolId) -> bool {
        self.latched.contains(&condition)
    }

    /// The mission transition `actor` reached, or `None` when it reached none.
    ///
    /// The **only** way to ask whether an actor died or merely lost its pilot: a
    /// [`MissionTransition::PilotBailedOut`] is not a kill and never reads as
    /// one, whatever the counters hold.
    #[must_use]
    pub fn mission_transition(&self, actor: ActorId) -> Option<MissionTransition> {
        self.transitions.transition(actor)
    }

    /// The declared mission-result policy for a bailout (F29-C.4).
    #[must_use]
    pub const fn bailout_policy(&self) -> super::bailout::BailoutResultPolicy {
        self.transitions.policy()
    }

    /// Records what confirmed a pilot's departure, before the tick that reports
    /// the [`LifecycleKind::PilotBailout`] transition reaches this runtime.
    ///
    /// The declaration gate of the F29-C.4 wiring, and the analogue of
    /// [`add_condition`](Self::add_condition): a confirmation is a *fact about
    /// the input and the program* rather than a field of the tick's facts, so it
    /// is registered here instead of carried in [`TickInput`]. One edge and one
    /// request per actor; a repeated confirmation is refused by name.
    ///
    /// # Errors
    ///
    /// [`BailoutRefusal`] — the input context gate, a repeated confirmation, or
    /// an actor whose mission transition is already latched. Nothing is written
    /// when a confirmation is refused.
    pub fn confirm_bailout(
        &mut self,
        actor: ActorId,
        confirmation: BailoutConfirmation,
    ) -> Result<(), BailoutRefusal> {
        self.transitions.confirm(actor, confirmation)
    }

    /// A declared timer's state.
    #[must_use]
    pub fn timer_state(&self, timer: SymbolId) -> Option<TimerState> {
        self.timers.get(&timer).map(MissionTimer::state)
    }

    /// The instance ids a spawn key was admitted with, whenever it was admitted.
    #[must_use]
    pub fn spawned_instances(&self, key: &IdempotencyKey) -> Option<&[ActorId]> {
        match self.ledger.admitted(key) {
            Some(Emission::Spawn(instances)) => Some(instances),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // The continuous tick
    // -----------------------------------------------------------------------

    /// Applies one tick's facts and returns the ordered events.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::NotAdvancing`] for a tick at or before the last stepped
    /// one, and [`RuntimeError::Trigger`] for a refused movement — a non-finite
    /// segment, a repeated watched actor, or an observation that would not
    /// advance. Every one of them is decided in [`validate_movements`] before any
    /// effect is applied, so a refused tick changes nothing at all: counters keep
    /// their counts, conditions do not latch, no deadline runs and no spawn is
    /// admitted. A bound is not an error: it is reported in [`ObjectiveTick::stop`]
    /// with the tick it belongs to.
    pub fn step(&mut self, input: &TickInput<'_>) -> Result<ObjectiveTick, RuntimeError> {
        if let Some(last) = self.last_tick
            && input.tick <= last
        {
            return Err(RuntimeError::NotAdvancing {
                last,
                given: input.tick,
            });
        }
        let declared = self.declared_event_count(input);
        if declared > self.limits.max_events_per_tick {
            return Ok(ObjectiveTick {
                tick: input.tick,
                events: Vec::new(),
                outcome: self.latch.outcome(),
                stop: Some(StopReason::EventBudget {
                    at_tick: input.tick,
                    limit: self.limits.max_events_per_tick,
                    declared,
                }),
            });
        }
        self.validate_movements(input)?;
        // A settled outcome ends the mission's objective work. This is where the
        // contract's "a protected actor destroyed after a success latch" case is
        // decided rather than left to a producer's ordering: on this engine the
        // outcome cannot change, and a later tick says so instead of acting.
        if self.latch.is_settled() {
            self.last_tick = Some(input.tick);
            return Ok(ObjectiveTick {
                tick: input.tick,
                events: Vec::new(),
                outcome: self.latch.outcome(),
                stop: Some(StopReason::OutcomeSettled {
                    settled_at: self.latch.settled().map_or(input.tick, |(at, _)| at),
                }),
            });
        }

        self.requests.clear();
        let mut out = Emitter::new(self.session, input.tick, self.sequence);

        self.apply_counters(input, &mut out);
        self.apply_conditions(&mut out);
        self.apply_triggers(input, &mut out)?;
        self.collect_signals(input, &mut out);
        self.apply_objective_requests(input, &mut out);
        self.apply_timers(input, &mut out);
        self.apply_completion_effects(&mut out);
        self.resolve_outcome(input, &mut out);

        self.sequence = out.sequence;
        self.last_tick = Some(input.tick);
        // A signal raised by an action this tick becomes eligible next tick, so
        // an action never arms a timer through a callback in the same tick.
        self.eligible_signals
            .extend(std::mem::take(&mut self.raised_signals));
        let mut events = out.events;
        events.sort_by_key(|event| event.key);
        Ok(ObjectiveTick {
            tick: input.tick,
            events,
            outcome: self.latch.outcome(),
            stop: None,
        })
    }

    /// How many events this tick's declared facts could produce.
    ///
    /// An estimate computed *before* anything is applied, so the bound check in
    /// [`step`](Self::step) can never leave the runtime half-updated: the point
    /// is to refuse an obviously unbounded tick, not to meter it exactly. Each
    /// term is the most that source could contribute:
    ///
    /// * one event per lifecycle transition: the counted record, the bailout
    ///   transition, or the refusal — never more than one, whatever the
    ///   transition turned out to be;
    /// * two events per count condition that has not latched (`ConditionMet`
    ///   plus its reaction);
    /// * two crossings per watched trigger per movement (entry then exit);
    /// * one per signal, timer request and objective request;
    /// * two events per timer (one arm, one expiry) plus one per expiry action;
    /// * one event per objective for a reveal;
    /// * one per completion effect any objective declares (the move it declares,
    ///   refused or not);
    /// * one per terminal request.
    ///
    /// A reaction's own cascade — the state change, the reveal it triggers and
    /// the deadlines that state arms — is covered by the objective and timer
    /// terms, which is why a tick may legitimately produce fewer events than
    /// this reports. The completion-effect term is an upper bound for the same
    /// reason and is bounded too: no effect kind completes an objective, so a
    /// drain can never queue an effect of its own.
    fn declared_event_count(&self, input: &TickInput<'_>) -> usize {
        let lifecycles = Self::lifecycle_events(input);
        let conditions = 2 * (self.conditions.len() - self.latched.len());
        let crossings = 2 * self.triggers.len() * input.movements.len();
        let timers = 2 * self.timers.len();
        lifecycles
            + conditions
            + crossings
            + input.signals.len()
            + input.timer_requests.len()
            + input.objective_requests.len()
            + input.terminal_requests.len()
            + timers
            + self.objectives.len()
            + self.declared_completion_effects()
            + self.pending_effects.len()
    }

    /// How many events one tick's lifecycle facts could produce at most.
    ///
    /// One per reported transition, whatever it turns out to be: a counted
    /// record, a bailout transition, or a refusal. F29-C.4 raised this term from
    /// "one per *countable* transition" to "one per transition", because a
    /// bailout is now reported and a refused transition is now named — both were
    /// silent before, and neither may sit outside the bound the tick is checked
    /// against.
    fn lifecycle_events(input: &TickInput<'_>) -> usize {
        input.lifecycles.len()
    }

    /// How many completion effects one tick could apply at most: every effect
    /// every objective declares, because any objective may complete in any tick
    /// and each of its effects is then one event.
    ///
    /// The queue itself cannot be counted here. [`step`](Self::step) reaches this
    /// before phase 7 of the tick it is starting, and the previous tick's drain
    /// emptied the queue before that tick ended, so `pending_effects` is empty at
    /// every call and would count the effects of a tick that has not queued
    /// anything yet — leaving the effects this tick is about to queue outside the
    /// bound they can most easily exceed.
    fn declared_completion_effects(&self) -> usize {
        self.objectives
            .values()
            .map(|objective| objective.completion_effects.len())
            .sum()
    }

    /// Refuses the whole tick's movements before observing any of them.
    ///
    /// A [`SweptTrigger`] keeps its own state, so observing one and then failing
    /// on another would leave the first advanced and the second not: a crossing
    /// lost for one actor and kept for another. Validating first means a refused
    /// tick moves nothing.
    ///
    /// This covers both refusals [`SweptTrigger::observe`] can make. The repeated
    /// actor is the one a caller can produce by listing a watched actor twice:
    /// the first observation would advance the trigger to this tick and the
    /// second would then be refused *mid-tick*, after the counter and condition
    /// phases had already applied — so it is caught here, where refusing costs
    /// nothing.
    fn validate_movements(&self, input: &TickInput<'_>) -> Result<(), RuntimeError> {
        let mut observed: BTreeSet<ActorId> = BTreeSet::new();
        for (actor, movement) in input.movements {
            if !movement.is_finite() {
                return Err(RuntimeError::Trigger(TriggerError::NonFinite));
            }
            let watched: Vec<&SweptTrigger> = self
                .triggers
                .values()
                .filter(|trigger| trigger.actor() == *actor)
                .collect();
            if watched.is_empty() {
                continue;
            }
            if !observed.insert(*actor) {
                return Err(RuntimeError::Trigger(TriggerError::RepeatedActor {
                    actor: *actor,
                    tick: input.tick,
                }));
            }
            for trigger in watched {
                if let Some(last) = trigger.last_tick()
                    && input.tick <= last
                {
                    return Err(RuntimeError::Trigger(TriggerError::NotAdvancing {
                        last,
                        given: input.tick,
                    }));
                }
            }
        }
        Ok(())
    }

    // -- phases -------------------------------------------------------------

    /// Phase 1: fold this tick's lifecycle transitions into the counters, through
    /// the mission-transition ledger.
    ///
    /// The ledger owns the two transitions F29 keeps apart, so the counters
    /// follow it instead of reading the raw facts: a *bailout* is reported as
    /// [`ObjectiveEventKind::PilotBailedOut`] and counted toward nothing, and a
    /// *destruction* is counted only when destruction is the transition this
    /// actor actually reached. That is what stops a destruction report arriving
    /// after a bailout from becoming a kill — refused by name, never dropped.
    /// Capture and despawn keep their own measured counted category, and a
    /// mission removal keeps counting toward nothing.
    fn apply_counters(&mut self, input: &TickInput<'_>, out: &mut Emitter) {
        for (actor, kind) in input.lifecycles {
            let Some(transition) = MissionTransition::from_lifecycle(*kind) else {
                // The three kinds this ledger does not own, unchanged.
                let Some(counted) = CountKind::from_lifecycle(*kind) else {
                    continue;
                };
                if self.counters.record(counted, *actor) {
                    out.push(
                        ACTOR_EVENT_SOURCE,
                        ObjectiveEventKind::Counted {
                            actor: *actor,
                            kind: counted,
                        },
                    );
                }
                continue;
            };
            match self.transitions.observe(*actor, transition) {
                TransitionOutcome::Applied(applied) => match applied.confirmation {
                    // A bailout: reported, never counted, and the declared policy
                    // — which today measures nothing — is the only thing that
                    // could settle the mission over it.
                    Some(confirmation) => {
                        if let Some(outcome) = self.transitions.policy().terminal_outcome() {
                            self.requests.push((ACTOR_EVENT_SOURCE, outcome));
                        }
                        out.push(
                            ACTOR_EVENT_SOURCE,
                            ObjectiveEventKind::PilotBailedOut {
                                actor: applied.actor,
                                confirmation,
                            },
                        );
                    }
                    // A destruction: the kill path, counted once.
                    None => {
                        if self.counters.record(CountKind::Destroyed, applied.actor) {
                            out.push(
                                ACTOR_EVENT_SOURCE,
                                ObjectiveEventKind::Counted {
                                    actor: applied.actor,
                                    kind: CountKind::Destroyed,
                                },
                            );
                        }
                    }
                },
                // The actor already reached this exact transition: the counters
                // are idempotent, so a repeated report stays silent as before.
                TransitionOutcome::Repeated(_) => {}
                TransitionOutcome::Refused(reason) => {
                    out.push(
                        ACTOR_EVENT_SOURCE,
                        ObjectiveEventKind::TransitionRefused {
                            actor: *actor,
                            requested: transition,
                            reason,
                        },
                    );
                }
            }
        }
    }

    /// Phase 2: latch the conditions this tick satisfied and apply their
    /// declared reactions.
    fn apply_conditions(&mut self, out: &mut Emitter) {
        let newly_met: Vec<SymbolId> = self
            .conditions
            .iter()
            .filter(|(key, _)| !self.latched.contains(key))
            .filter(|(_, binding)| binding.condition.satisfied(&self.counters))
            .map(|(key, _)| *key)
            .collect();
        for key in newly_met {
            let Some(binding) = self.conditions.get(&key) else {
                continue;
            };
            let observed = binding.condition.observed(&self.counters);
            let kind = binding.condition.kind;
            let reaction = binding.reaction;
            self.latched.insert(key);
            out.push(
                key,
                ObjectiveEventKind::ConditionMet {
                    condition: key,
                    kind,
                    observed,
                },
            );
            self.settle_reveals(RevealTrigger::Condition(key), out);
            match reaction {
                CountReaction::ReportOnly => {}
                CountReaction::SetObjectiveState { objective, state } => {
                    self.change_objective(objective, state, key, out);
                }
                CountReaction::Finish(outcome) => self.requests.push((key, outcome)),
            }
        }
    }

    /// Phase 3: sweep this tick's real movement segments. A crossing reports; it
    /// never applies an effect.
    fn apply_triggers(
        &mut self,
        input: &TickInput<'_>,
        out: &mut Emitter,
    ) -> Result<(), RuntimeError> {
        for (actor, movement) in input.movements {
            let watched: Vec<SymbolId> = self
                .triggers
                .iter()
                .filter(|((_, watched), _)| *watched == *actor)
                .map(|(key, _)| key.0)
                .collect();
            for key in watched {
                let Some(trigger) = self.triggers.get_mut(&(key, *actor)) else {
                    continue;
                };
                for event in trigger.observe(input.tick, *movement)? {
                    out.push(key, ObjectiveEventKind::TriggerCrossed(event));
                }
            }
        }
        Ok(())
    }

    /// Phase 4: collect this tick's declared signals. They become eligible to
    /// arm a timer on the *next* tick, so a signal never arms a timer through a
    /// callback in the tick that raised it. A signal still reveals an objective
    /// this tick: a reveal is a report about the player, not a work item.
    fn collect_signals(&mut self, input: &TickInput<'_>, out: &mut Emitter) {
        for signal in input.signals {
            out.push(
                *signal,
                ObjectiveEventKind::SignalRaised { signal: *signal },
            );
            self.raised_signals.insert(*signal);
            self.settle_reveals(RevealTrigger::Signal(*signal), out);
        }
    }

    /// Phase 5: this tick's declared objective state changes.
    fn apply_objective_requests(&mut self, input: &TickInput<'_>, out: &mut Emitter) {
        for (objective, state) in input.objective_requests {
            self.change_objective(*objective, *state, *objective, out);
        }
    }

    /// Phase 6: this tick's timer requests, then the whole committed ticks, then
    /// each expiry's one declared action.
    fn apply_timers(&mut self, input: &TickInput<'_>, out: &mut Emitter) {
        // Automatic arms first, so a deadline whose declared start tick is this
        // one runs from this tick rather than the next. A declared start is
        // *consumed* by the tick that took it: without the state filter the
        // runtime would retry the arm on every later tick and report the refusal
        // forever, so a one-shot declaration would produce an unbounded event
        // stream instead of running once.
        let automatic: Vec<SymbolId> = self
            .timers
            .iter()
            .filter(|(_, timer)| timer.state() == TimerState::NotArmed)
            .filter(|(_, timer)| timer.auto_arms_on(out.tick))
            .map(|(id, _)| *id)
            .collect();
        for timer in automatic {
            self.arm_timer(timer, TimerStart::AtTick(out.tick), out);
        }

        // A signal raised on an earlier tick is eligible now, and is consumed by
        // the tick that observes it: a signal arms a waiting deadline once.
        let eligible: Vec<SymbolId> = self.eligible_signals.iter().copied().collect();
        self.eligible_signals.clear();
        for signal in eligible {
            let waiting: Vec<SymbolId> = self
                .timers
                .iter()
                .filter(|(_, timer)| timer.start() == TimerStart::OnSignal(signal))
                .map(|(id, _)| *id)
                .collect();
            for timer in waiting {
                self.arm_timer(timer, TimerStart::OnSignal(signal), out);
            }
        }

        for request in input.timer_requests {
            match request {
                TimerRequest::Arm(timer) => {
                    self.arm_timer(*timer, TimerStart::OnArm, out);
                }
                TimerRequest::Cancel(timer) => self.cancel_timer(*timer, out),
            }
        }

        let mut expired: Vec<SymbolId> = Vec::new();
        if input.committed_ticks > 0 {
            // Collected in `SymbolId` order, which is declared order and never
            // hash order: two runs with the same table expire the same timers in
            // the same sequence.
            let ids: Vec<SymbolId> = self.timers.keys().copied().collect();
            for id in ids {
                if let Some(timer) = self.timers.get_mut(&id)
                    && timer.advance(input.committed_ticks)
                {
                    expired.push(id);
                }
            }
            for timer in expired.iter().copied() {
                self.report_expiry(timer, out);
            }
        }

        // Only the timers that ran out on **this** tick perform their action,
        // and the whole set is collected before any of them is applied, so one
        // timer's action can never observe another's half-applied state.
        //
        // Deriving the set from the timer table instead would replay every
        // expired action on every later tick that commits a tick, because
        // [`TimerState::Expired`] is where a timer stays: a reward intent, a
        // raised signal or a second wave request would repeat forever. Only the
        // [`EmissionLedger`](super::spawn::EmissionLedger) would hide the repeat,
        // and only for spawns and cues.
        let actions: Vec<(SymbolId, TimerAction)> = expired
            .iter()
            .filter_map(|id| {
                self.timers
                    .get(id)
                    .map(|declaration| (*id, declaration.action().clone()))
            })
            .collect();
        for (timer, action) in actions {
            self.apply_timer_action(timer, action, out);
        }
    }

    /// Phase 7: apply the completion effects every objective that completed this
    /// tick declared.
    ///
    /// The due set is *taken* before the first effect is applied, so an effect
    /// that somehow queued another one would wait for the next tick instead of
    /// recursing here. That is belt and braces: no [`CompletionEffectKind`] moves
    /// a target to `Succeeded`, so no applied effect can complete an objective
    /// and the queue cannot grow while it drains.
    ///
    /// Order is the order the completions happened — phase 5's objective
    /// requests as the caller listed them, then phase 6's expiries in timer
    /// order — and within one completion, its effects in the engine's fixed
    /// order (queued so by
    /// [`queue_completion_effects`](Self::queue_completion_effects)). No hash
    /// map or entity iteration is involved. Two objectives completing in one
    /// tick can therefore be applied in either order, which is why this is
    /// only a *reported* order question: [`step`](Self::step) sorts the stream
    /// by [`EventKey`], so their moves are observed in `(source, sequence)`
    /// order however they were applied, and a target named with different
    /// effects by two *different* objectives cannot be registered at all, so no
    /// such order decides a contested outcome.
    ///
    /// Each kind carries the engine's measured behaviour (M01-B-OBJ-1):
    ///
    /// * `WAKE` — per target in list order: a target that is not present or has
    ///   completed (a final state) is skipped and the skip is reported; a
    ///   target already `Active` stops the rest of this completion's wake list
    ///   ([`ObjectiveEventKind::WakeStopped`]); anything else moves through
    ///   [`change_objective`](Self::change_objective), whose reveal rule and
    ///   transition table still gate it. The stop is tracked per *source*, so
    ///   two completions in one tick never stop each other's lists.
    /// * `NAP` — moves the target to [`ObjectiveState::Napping`] and stores
    ///   the declared [`NapTime`] on it; a refused move stores nothing.
    /// * `KILL` — clears the target's presence flag and reports
    ///   [`ObjectiveEventKind::ObjectiveRemoved`] with the state it kept; the
    ///   state itself is never touched, and no outcome is requested.
    /// * `SLEEP` — moves the target to [`ObjectiveState::Asleep`].
    fn apply_completion_effects(&mut self, out: &mut Emitter) {
        let due: Vec<QueuedEffect> = std::mem::take(&mut self.pending_effects);
        let mut wake_stopped: BTreeSet<SymbolId> = BTreeSet::new();
        for QueuedEffect { source, effect } in due {
            let effect = match effect {
                Ok(effect) => effect,
                Err(reason) => {
                    out.push(
                        source,
                        ObjectiveEventKind::RequestRefused {
                            request: source,
                            reason,
                        },
                    );
                    continue;
                }
            };
            match effect.kind {
                CompletionEffectKind::Wake | CompletionEffectKind::Wakeup => {
                    self.apply_wake(source, effect.target, &mut wake_stopped, out);
                }
                CompletionEffectKind::Nap => {
                    if self.change_objective(effect.target, ObjectiveState::Napping, source, out)
                        && let Some(tracked) = self.objectives.get_mut(&effect.target)
                    {
                        tracked.nap_time = effect.argument;
                    }
                }
                CompletionEffectKind::Kill => self.apply_kill(source, effect.target, out),
                CompletionEffectKind::Sleep => {
                    self.change_objective(effect.target, ObjectiveState::Asleep, source, out);
                }
            }
        }
    }

    /// One wake of a completion's wake list, with the engine's measured guards
    /// (0x469af0): skip a target that is not present or has completed; stop the
    /// whole list at a target that is already awake; otherwise wake.
    fn apply_wake(
        &mut self,
        source: SymbolId,
        target: SymbolId,
        wake_stopped: &mut BTreeSet<SymbolId>,
        out: &mut Emitter,
    ) {
        if wake_stopped.contains(&source) {
            // The list already stopped at an earlier target: the engine never
            // reaches the rest of it, and `WakeStopped` already named where
            // and why.
            return;
        }
        let Some(tracked) = self.objectives.get(&target) else {
            out.push(
                source,
                ObjectiveEventKind::RequestRefused {
                    request: target,
                    reason: RuntimeError::UnknownObjective { objective: target },
                },
            );
            return;
        };
        let from = tracked.state();
        if !tracked.present || from.is_final() {
            out.push(
                source,
                ObjectiveEventKind::ObjectiveChangeRefused {
                    objective: target,
                    from,
                    to: ObjectiveState::Active,
                },
            );
            return;
        }
        if from == ObjectiveState::Active {
            // The measured early-out: an already-awake target ends the whole
            // list. The engine also resets that target's elapsed time; this
            // runtime has no per-objective elapsed timer to reset.
            wake_stopped.insert(source);
            out.push(
                source,
                ObjectiveEventKind::WakeStopped {
                    source,
                    objective: target,
                },
            );
            return;
        }
        self.change_objective(target, ObjectiveState::Active, source, out);
    }

    /// One kill: presence cleared, state kept, reported (0x46b130). An unknown
    /// target is refused by name rather than dropped.
    fn apply_kill(&mut self, source: SymbolId, target: SymbolId, out: &mut Emitter) {
        let Some(tracked) = self.objectives.get_mut(&target) else {
            out.push(
                source,
                ObjectiveEventKind::RequestRefused {
                    request: target,
                    reason: RuntimeError::UnknownObjective { objective: target },
                },
            );
            return;
        };
        let from = tracked.state();
        tracked.present = false;
        out.push(
            source,
            ObjectiveEventKind::ObjectiveRemoved {
                objective: target,
                from,
            },
        );
    }

    /// Phase 8: resolve every terminal request this tick made, together.
    fn resolve_outcome(&mut self, input: &TickInput<'_>, out: &mut Emitter) {
        let mut requested: BTreeMap<TerminalOutcome, SymbolId> = BTreeMap::new();
        for (source, outcome) in input.terminal_requests {
            requested.entry(*outcome).or_insert(*source);
        }
        for (source, outcome) in &self.requests {
            requested.entry(*outcome).or_insert(*source);
        }
        if requested.is_empty() {
            return;
        }
        let outcomes: BTreeSet<TerminalOutcome> = requested.keys().copied().collect();
        match self.latch.resolve(out.tick, &outcomes, self.precedence) {
            Resolution::NoneRequested => {}
            Resolution::Settled {
                outcome,
                superseded,
            } => {
                let source = requested
                    .get(&outcome)
                    .copied()
                    .unwrap_or(ACTOR_EVENT_SOURCE);
                out.push(
                    source,
                    ObjectiveEventKind::OutcomeSettled {
                        outcome,
                        superseded: superseded.into_iter().collect(),
                    },
                );
            }
            // `step` returns before this runs when the latch already holds an
            // outcome, so there is no reachable tick in which a request set here
            // meets a settled latch: the tick that found one applied nothing and
            // said so through `StopReason::OutcomeSettled` instead.
            Resolution::AlreadySettled { .. } => {}
        }
    }

    // -- helpers ------------------------------------------------------------

    /// Applies one declared objective state change and its consequences.
    ///
    /// Returns whether the move applied; every refusal is reported on the
    /// stream first, so a `false` is always paired with the refusal event
    /// that named it.
    fn change_objective(
        &mut self,
        objective: SymbolId,
        to: ObjectiveState,
        source: SymbolId,
        out: &mut Emitter,
    ) -> bool {
        let Some(tracked) = self.objectives.get_mut(&objective) else {
            out.push(
                source,
                ObjectiveEventKind::RequestRefused {
                    request: objective,
                    reason: RuntimeError::UnknownObjective { objective },
                },
            );
            return false;
        };
        let from = tracked.state();
        // A hidden objective leaves `Hidden` only through its own reveal rule
        // (F39 non-negotiable behavior 5). A declared action that tries is
        // refused and reported, so "show the objective" can never be a side
        // effect of unlocking or resetting something else.
        let still_hidden = from == ObjectiveState::Hidden && !tracked.revealed;
        let on_complete = tracked.on_complete;
        if still_hidden {
            out.push(
                source,
                ObjectiveEventKind::ObjectiveChangeRefused {
                    objective,
                    from,
                    to,
                },
            );
            return false;
        }
        if let Err(IllegalTransition { from, to }) = tracked.cell.transition(to) {
            out.push(
                source,
                ObjectiveEventKind::ObjectiveChangeRefused {
                    objective,
                    from,
                    to,
                },
            );
            return false;
        }
        out.push(
            source,
            ObjectiveEventKind::ObjectiveChanged {
                objective,
                from,
                to,
            },
        );
        if to == ObjectiveState::Succeeded {
            if let ObjectiveCompletion::Requests(outcome) = on_complete {
                self.requests.push((source, outcome));
            }
            // The objective completed, so whatever it declares happens to *other*
            // objectives — queued, never called: the effects are applied by
            // phase 7, after every state change this tick has already made.
            self.queue_completion_effects(objective);
        }
        self.settle_reveals(
            RevealTrigger::ObjectiveState {
                objective,
                state: to,
            },
            out,
        );
        // A [`TimerStart::OnObjectiveState`] deadline starts from the state it
        // declares. This is a declared start condition resolving, not a callback:
        // the timer still cannot expire before phase 6's committed ticks.
        let waiting: Vec<SymbolId> = self
            .timers
            .iter()
            .filter(|(_, timer)| {
                timer.start()
                    == TimerStart::OnObjectiveState {
                        objective,
                        state: to,
                    }
            })
            .map(|(id, _)| *id)
            .collect();
        for timer in waiting {
            self.arm_timer(
                timer,
                TimerStart::OnObjectiveState {
                    objective,
                    state: to,
                },
                out,
            );
        }
        true
    }

    /// Queues the wake/sleep/kill/nap family's emissions for the next tick's
    /// phase 7, the executor path of `M02-B-FU4` (see [`directives`]).
    ///
    /// `objectives` is the record's block count. Each address is resolved
    /// through [`resolve_objective_address`](super::address::resolve_objective_address);
    /// a refusal is queued beside the effects and reported by the tick as
    /// [`ObjectiveEventKind::RequestRefused`], so nothing is clamped or dropped
    /// in silence. Emissions of one source apply in the engine's fixed order
    /// (`WAKE`, `NAP`, `KILL`, `SLEEP`), list order inside each, and an
    /// emission whose execution key was queued before is not queued again.
    /// Emissions outside the family are left to their own consumers.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::StaleDirective`] when an emission carries another
    /// session; nothing from the call is queued then. Returns how many
    /// emissions were queued.
    pub fn submit_directives(
        &mut self,
        objectives: u32,
        emissions: &[DirectiveEmission],
    ) -> Result<usize, RuntimeError> {
        let family: Vec<&DirectiveEmission> = emissions
            .iter()
            .filter(|emission| directives::is_family(emission.operation))
            .collect();
        if let Some(stale) = family.iter().find(|e| e.key.session != self.session) {
            return Err(RuntimeError::StaleDirective {
                expected: self.session,
                given: stale.key.session,
            });
        }
        let mut sources: Vec<(SymbolId, Vec<(u8, directives::Planned)>)> = Vec::new();
        let mut queued = 0;
        for emission in family {
            if !self
                .consumed_directives
                .insert(emission.key.execution_key())
            {
                continue;
            }
            queued += 1;
            let source = emission.key.source;
            let rank = directives::rank(emission.operation);
            let planned: Vec<(u8, directives::Planned)> = if self.objectives.contains_key(&source) {
                directives::plan(emission.operation, &emission.args, objectives)
                    .into_iter()
                    .map(|item| (rank, item))
                    .collect()
            } else {
                vec![(
                    rank,
                    Err(RuntimeError::UnknownObjective { objective: source }),
                )]
            };
            match sources.iter_mut().find(|(s, _)| *s == source) {
                Some((_, items)) => items.extend(planned),
                None => sources.push((source, planned)),
            }
        }
        for (source, mut items) in sources {
            items.sort_by_key(|(rank, _)| *rank);
            self.pending_effects.extend(
                items
                    .into_iter()
                    .map(|(_, effect)| QueuedEffect { source, effect }),
            );
        }
        Ok(queued)
    }

    /// Queues the completion effects an objective declares, remembering which
    /// objective declared each one: that objective is the `source` its effects'
    /// events report under, because the effect belongs to *its* declaration and
    /// not to whatever request completed it. Within one completion the effects
    /// are queued in the engine's fixed order
    /// ([`CompletionEffectKind::engine_order`]), a stable sort, so two targets
    /// of one kind keep their authored list order while `WAKE` always precedes
    /// `NAP`, `NAP` precedes `KILL` and `KILL` precedes `SLEEP` — measured,
    /// not authored (M01-B-OBJ-1).
    ///
    /// An objective that has already completed latches: no row leaves
    /// `Succeeded`, so a completion happens once per session and so do the
    /// effects it queues.
    fn queue_completion_effects(&mut self, objective: SymbolId) {
        let Some(tracked) = self.objectives.get(&objective) else {
            return;
        };
        let source = objective;
        let mut effects: Vec<CompletionEffect> = tracked.completion_effects.clone();
        effects.sort_by_key(|effect| effect.kind.engine_order());
        self.pending_effects
            .extend(effects.into_iter().map(|effect| QueuedEffect {
                source,
                effect: Ok(effect),
            }));
    }

    /// Reveals every hidden objective whose declared rule this event satisfies.
    ///
    /// The reveal is itself the state change out of `Hidden`: the objective moves
    /// to [`ObjectiveState::Pending`] and is reported once, so
    /// [`is_visible`](Self::is_visible) has a single source of truth.
    fn settle_reveals(&mut self, trigger: RevealTrigger, out: &mut Emitter) {
        let ready: Vec<SymbolId> = self
            .objectives
            .iter()
            .filter(|(_, tracked)| !tracked.revealed)
            .filter(|(_, tracked)| tracked.reveal.satisfied_by(trigger))
            .map(|(id, _)| *id)
            .collect();
        for id in ready {
            let Some(tracked) = self.objectives.get_mut(&id) else {
                continue;
            };
            tracked.revealed = true;
            let state = match tracked.state() {
                ObjectiveState::Hidden => {
                    // The only legal move out of `Hidden` that is also the
                    // reveal itself; a refusal here is impossible by
                    // construction, so the state is written directly.
                    let _ = tracked.cell.transition(ObjectiveState::Pending);
                    tracked.state()
                }
                other => other,
            };
            out.push(
                id,
                ObjectiveEventKind::ObjectiveRevealed {
                    objective: id,
                    state,
                },
            );
        }
    }

    fn arm_timer(&mut self, timer: SymbolId, via: TimerStart, out: &mut Emitter) {
        let tick = out.tick;
        let Some(declaration) = self.timers.get_mut(&timer) else {
            out.push(
                timer,
                ObjectiveEventKind::RequestRefused {
                    request: timer,
                    reason: RuntimeError::UnknownTimer { timer },
                },
            );
            return;
        };
        let result = match via {
            TimerStart::OnArm => declaration.arm(tick),
            _ => declaration.auto_arm(tick),
        };
        match result {
            Ok(()) => out.push(timer, ObjectiveEventKind::TimerArmed { timer, via }),
            Err(reason) => out.push(timer, ObjectiveEventKind::TimerRefused { timer, reason }),
        }
    }

    /// Cancels a declared timer, or names the request that named no timer.
    fn cancel_timer(&mut self, timer: SymbolId, out: &mut Emitter) {
        let reason = match self.timers.get_mut(&timer) {
            None => {
                out.push(
                    timer,
                    ObjectiveEventKind::RequestRefused {
                        request: timer,
                        reason: RuntimeError::UnknownTimer { timer },
                    },
                );
                return;
            }
            Some(declaration) => declaration.cancel().err(),
        };
        if let Some(reason) = reason {
            out.push(timer, ObjectiveEventKind::TimerRefused { timer, reason });
        }
    }

    fn report_expiry(&mut self, timer: SymbolId, out: &mut Emitter) {
        let tick = out.tick;
        let Some(declaration) = self.timers.get_mut(&timer) else {
            return;
        };
        declaration.stamp_expiry(tick);
        out.push(timer, ObjectiveEventKind::TimerExpired { timer });
        self.settle_reveals(RevealTrigger::Timer(timer), out);
    }

    fn apply_timer_action(&mut self, timer: SymbolId, action: TimerAction, out: &mut Emitter) {
        match action {
            TimerAction::SetObjectiveState { objective, state } => {
                self.change_objective(objective, state, timer, out);
            }
            TimerAction::Signal(signal) => {
                out.push(timer, ObjectiveEventKind::SignalRaised { signal });
                self.raised_signals.insert(signal);
                self.settle_reveals(RevealTrigger::Signal(signal), out);
            }
            TimerAction::SpawnGroup { key, group, count } => {
                let instances = self.admit_spawn(&key, count);
                match instances {
                    Some(instances) => out.push(
                        timer,
                        ObjectiveEventKind::SpawnAdmitted {
                            key,
                            group,
                            instances,
                        },
                    ),
                    None => {
                        let prior = self
                            .spawned_instances(&key)
                            .map_or_else(Vec::new, <[ActorId]>::to_vec);
                        out.push(
                            timer,
                            ObjectiveEventKind::SpawnRefused {
                                key,
                                group,
                                instances: prior,
                            },
                        );
                    }
                }
            }
            TimerAction::Cue { key, dialogue } => {
                if self.admit_cue(&key) {
                    out.push(timer, ObjectiveEventKind::CueEmitted { key, dialogue });
                } else {
                    out.push(timer, ObjectiveEventKind::CueRefused { key, dialogue });
                }
            }
            TimerAction::GrantOptionalReward { reward } => {
                out.push(timer, ObjectiveEventKind::OptionalReward { reward });
            }
            TimerAction::Finish(outcome) => self.requests.push((timer, outcome)),
        }
    }

    /// Admits a spawn key once, allocating this session's instance ids on the
    /// first admission. A repeated key is refused and returns `None`, so the
    /// same wave is never spawned twice and the ids stay the ones the first
    /// admission handed out.
    fn admit_spawn(&mut self, key: &IdempotencyKey, count: u32) -> Option<Vec<ActorId>> {
        if self.ledger.admitted(key).is_some() {
            return None;
        }
        let mut instances = Vec::with_capacity(count as usize);
        for _ in 0..count {
            instances.push(ActorId(self.next_instance));
            self.next_instance += 1;
        }
        match self.ledger.admit(
            self.session,
            key.clone(),
            Emission::Spawn(instances.clone()),
        ) {
            Ok(Admission::Admitted) => Some(instances),
            Ok(Admission::Repeated(_)) | Err(_) => None,
        }
    }

    fn admit_cue(&mut self, key: &IdempotencyKey) -> bool {
        matches!(
            self.ledger.admit(self.session, key.clone(), Emission::Cue),
            Ok(Admission::Admitted)
        )
    }
}

/// A count condition together with what satisfying it does.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CountBinding {
    condition: CountCondition,
    reaction: CountReaction,
}

/// One queued completion effect: the effect, and the objective whose declaration
/// carries it (the `source` its events report under).
#[derive(Clone, Debug, PartialEq)]
struct QueuedEffect {
    source: SymbolId,
    /// A refusal is queued in the same place as an effect, so the stream
    /// reports them in the order the directive spelled them.
    effect: Result<CompletionEffect, RuntimeError>,
}

/// One objective's live state.
#[derive(Clone, Debug, PartialEq)]
struct TrackedObjective {
    content: ContentId,
    cell: ObjectiveCell,
    reveal: RevealRule,
    revealed: bool,
    on_complete: ObjectiveCompletion,
    completion_effects: Vec<CompletionEffect>,
    /// The engine's *present* flag (M01-B-OBJ-1): cleared by a
    /// [`CompletionEffectKind::Kill`], which removes the objective from play
    /// without touching its state. A wake skips a target that is not present.
    present: bool,
    /// The nap time the last applied `NAP` effect declared, exactly as the
    /// engine stores it in the target's re-wake slot. Carried, never
    /// converted: the unit is unmeasured and nothing here ends a nap by
    /// elapsed time.
    nap_time: Option<NapTime>,
}

impl TrackedObjective {
    fn state(&self) -> ObjectiveState {
        self.cell.state()
    }
}

/// The event that may satisfy a [`RevealRule`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RevealTrigger {
    Condition(SymbolId),
    Timer(SymbolId),
    Signal(SymbolId),
    ObjectiveState {
        objective: SymbolId,
        state: ObjectiveState,
    },
}

impl RevealRule {
    fn satisfied_by(self, trigger: RevealTrigger) -> bool {
        match trigger {
            RevealTrigger::Condition(met) => {
                matches!(self, Self::OnCondition { condition } if condition == met)
            }
            RevealTrigger::Timer(expired) => {
                matches!(self, Self::OnTimer { timer } if timer == expired)
            }
            RevealTrigger::Signal(raised) => {
                matches!(self, Self::OnSignal { signal } if signal == raised)
            }
            RevealTrigger::ObjectiveState {
                objective: other,
                state: reached,
            } => matches!(
                self,
                Self::OnObjectiveState { objective, state }
                    if objective == other && state == reached
            ),
        }
    }
}

/// The tick's event buffer: assigns each event its stable [`EventKey`].
struct Emitter {
    session: SessionGeneration,
    tick: Tick,
    sequence: u32,
    events: Vec<ObjectiveEvent>,
}

impl Emitter {
    fn new(session: SessionGeneration, tick: Tick, sequence: u32) -> Self {
        Self {
            session,
            tick,
            sequence,
            events: Vec::new(),
        }
    }

    fn push(&mut self, source: SymbolId, kind: ObjectiveEventKind) {
        let key = EventKey {
            session: self.session,
            tick: self.tick,
            source,
            sequence: self.sequence,
        };
        self.sequence += 1;
        self.events.push(ObjectiveEvent { key, kind });
    }
}

#[cfg(test)]
mod tests {
    use cs_types::content::ContentKind;

    use super::*;

    const GEN: SessionGeneration = SessionGeneration(11);

    fn content(key: &str) -> ContentId {
        ContentId::from_source(ContentKind::Objective, key).expect("an objective content id")
    }

    fn spec(id: u32, initial: ObjectiveState, effects: Vec<CompletionEffect>) -> ObjectiveSpec {
        ObjectiveSpec {
            id: SymbolId(id),
            content: content(&format!("synthetic.m01bfu1.{id}")),
            initial,
            reveal: RevealRule::Immediate,
            on_complete: ObjectiveCompletion::Continue,
            completion_effects: effects,
        }
    }

    fn effect(kind: CompletionEffectKind, target: u32, argument: Option<f64>) -> CompletionEffect {
        CompletionEffect::new(kind, SymbolId(target), argument.and_then(NapTime::new))
            .expect("a well-shaped completion effect")
    }

    fn runtime_with(specs: Vec<ObjectiveSpec>) -> ObjectiveRuntime {
        let mut runtime = ObjectiveRuntime::new(
            GEN,
            TerminalPrecedence::SyntheticConservative,
            RuntimeLimits::default(),
        );
        for spec in specs {
            runtime.add_objective(spec).expect("a registering spec");
        }
        runtime
    }

    /// Completes `completing` through the ordinary declared path on `tick`.
    fn complete(runtime: &mut ObjectiveRuntime, completing: u32, tick: u64) -> ObjectiveTick {
        let requests = [(SymbolId(completing), ObjectiveState::Succeeded)];
        let mut input = TickInput::at(Tick(tick));
        input.objective_requests = &requests;
        runtime.step(&input).expect("a legal tick")
    }

    /// The applied state changes of a tick, in observation order, without the
    /// completing objective's own move (the fixture's completion, not one of
    /// its effects).
    fn changes(
        tick: &ObjectiveTick,
        completing: u32,
    ) -> Vec<(SymbolId, ObjectiveState, ObjectiveState)> {
        tick.events
            .iter()
            .filter_map(|event| match event.kind {
                ObjectiveEventKind::ObjectiveChanged {
                    objective,
                    from,
                    to,
                } if objective != SymbolId(completing) => Some((objective, from, to)),
                _ => None,
            })
            .collect()
    }

    /// The removals of a tick, in observation order.
    fn removals(tick: &ObjectiveTick) -> Vec<(SymbolId, ObjectiveState)> {
        tick.events
            .iter()
            .filter_map(|event| match event.kind {
                ObjectiveEventKind::ObjectiveRemoved { objective, from } => Some((objective, from)),
                _ => None,
            })
            .collect()
    }

    /// The refused state changes of a tick, in observation order.
    fn refusals(tick: &ObjectiveTick) -> Vec<(SymbolId, ObjectiveState, ObjectiveState)> {
        tick.events
            .iter()
            .filter_map(|event| match event.kind {
                ObjectiveEventKind::ObjectiveChangeRefused {
                    objective,
                    from,
                    to,
                } => Some((objective, from, to)),
                _ => None,
            })
            .collect()
    }

    /// M01-B-OBJ-1: the engine applies a completion's effects in its own fixed
    /// order — `WAKE`, then `NAP`, then `KILL`, then `SLEEP` — whatever order
    /// the record spells them in, so a declaration whose authored order puts
    /// the nap first still wakes before it naps and removes last.
    #[test]
    fn accept_m01_b_obj_1_fu1_completion_effects_apply_in_the_engine_order() {
        let mut runtime = runtime_with(vec![
            // The completing objective spells its effects deliberately
            // backwards: NAP, then WAKE, then KILL.
            spec(
                1,
                ObjectiveState::Active,
                vec![
                    effect(CompletionEffectKind::Nap, 2, Some(2.0)),
                    effect(CompletionEffectKind::Wake, 3, None),
                    effect(CompletionEffectKind::Kill, 4, None),
                ],
            ),
            spec(2, ObjectiveState::Active, Vec::new()),
            spec(3, ObjectiveState::Pending, Vec::new()),
            spec(4, ObjectiveState::Active, Vec::new()),
        ]);

        let tick = complete(&mut runtime, 1, 1);
        assert_eq!(
            changes(&tick, 1),
            vec![
                (SymbolId(3), ObjectiveState::Pending, ObjectiveState::Active),
                (SymbolId(2), ObjectiveState::Active, ObjectiveState::Napping),
            ],
            "the wake applies before the nap, not in the authored order"
        );
        assert_eq!(
            removals(&tick),
            vec![(SymbolId(4), ObjectiveState::Active)],
            "the kill applies last and removes without a state change"
        );
        assert_eq!(tick.outcome, None, "no completion effect ends the mission");
        assert_eq!(
            runtime.objective_state(SymbolId(2)),
            Some(ObjectiveState::Napping),
            "a napped objective holds the engine's state 2, not `Optional`"
        );
        assert_eq!(
            runtime.objective_nap_time(SymbolId(2)).map(NapTime::value),
            Some(2.0),
            "the declared nap time is stored on the target, as the engine writes its slot"
        );
        assert_eq!(runtime.is_present(SymbolId(4)), Some(false));
    }

    /// M01-B-OBJ-1: the measured wake early-out — a target the list reaches
    /// while already awake stops the whole list, and the stop is per
    /// completion, so it never leaks into another completion's list.
    #[test]
    fn accept_m01_b_obj_1_fu1_an_awake_target_stops_the_rest_of_the_wake_list() {
        let mut runtime = runtime_with(vec![
            spec(
                6,
                ObjectiveState::Active,
                vec![
                    // The list reaches the already-awake target 2 first and
                    // stops there; 3 and 5 are never reached. A kill is
                    // spelled alongside to pin that it still applies after
                    // the stopped list (engine order puts it last).
                    effect(CompletionEffectKind::Wake, 2, None),
                    effect(CompletionEffectKind::Wake, 3, None),
                    effect(CompletionEffectKind::Wake, 5, None),
                    effect(CompletionEffectKind::Kill, 4, None),
                ],
            ),
            spec(2, ObjectiveState::Active, Vec::new()),
            spec(3, ObjectiveState::Pending, Vec::new()),
            spec(4, ObjectiveState::Active, Vec::new()),
            spec(5, ObjectiveState::Pending, Vec::new()),
        ]);

        let tick = complete(&mut runtime, 6, 1);
        assert!(
            tick.events.iter().any(|event| matches!(
                event.kind,
                ObjectiveEventKind::WakeStopped { source, objective }
                    if source == SymbolId(6) && objective == SymbolId(2)
            )),
            "the already-awake target stops the list and says so"
        );
        assert_eq!(
            changes(&tick, 6),
            vec![],
            "no wake moved anything: the list stopped at the first target"
        );
        for behind in [3, 5] {
            assert_eq!(
                runtime.objective_state(SymbolId(behind)),
                Some(ObjectiveState::Pending),
                "objective {behind} is behind the stop and was not woken"
            );
        }
        assert_eq!(
            runtime.is_present(SymbolId(4)),
            Some(false),
            "the kill after the stopped wake list still applies"
        );
    }

    /// M01-B-OBJ-1: a wake skips a completed target and reports the skip — and
    /// a skip does **not** stop the list; only the already-awake early-out
    /// does, so the target after a skipped one is still woken. (The other
    /// skip — a target that is not present — carries the same report; it is
    /// unreachable through registration today because a target one objective
    /// kills cannot be woken by another without an ambiguity refusal, so this
    /// test pins the reachable half of the guard.)
    #[test]
    fn accept_m01_b_obj_1_fu1_a_skipped_wake_does_not_stop_the_list() {
        let mut runtime = runtime_with(vec![
            spec(
                1,
                ObjectiveState::Active,
                vec![
                    effect(CompletionEffectKind::Wake, 7, None),
                    effect(CompletionEffectKind::Wake, 3, None),
                ],
            ),
            // Born completed: the engine's wake skips a completed target.
            spec(7, ObjectiveState::Succeeded, Vec::new()),
            spec(3, ObjectiveState::Pending, Vec::new()),
        ]);

        let tick = complete(&mut runtime, 1, 1);
        assert_eq!(
            refusals(&tick),
            vec![(
                SymbolId(7),
                ObjectiveState::Succeeded,
                ObjectiveState::Active
            )],
            "the completed target is skipped and the skip is reported"
        );
        assert_eq!(
            changes(&tick, 1),
            vec![(SymbolId(3), ObjectiveState::Pending, ObjectiveState::Active)],
            "the wake after the skipped target still applies"
        );
    }

    /// M01-B-OBJ-1: a kill is a removal, not an outcome. It clears presence,
    /// keeps the state, requests nothing terminal — and in this runtime it
    /// gates only later wakes, so a declared move may still name the killed
    /// objective (the engine has no such path, so no measured rule forbids it).
    #[test]
    fn accept_m01_b_obj_1_fu1_a_kill_is_a_removal_not_an_outcome() {
        let mut runtime = runtime_with(vec![
            spec(
                1,
                ObjectiveState::Active,
                vec![effect(CompletionEffectKind::Kill, 2, None)],
            ),
            spec(2, ObjectiveState::Active, Vec::new()),
        ]);

        let tick = complete(&mut runtime, 1, 1);
        assert_eq!(tick.outcome, None, "a kill requests no terminal outcome");
        assert!(
            changes(&tick, 1).is_empty(),
            "a kill moves no state at all, so there is no ObjectiveChanged to observe"
        );
        assert_eq!(
            runtime.objective_state(SymbolId(2)),
            Some(ObjectiveState::Active),
            "the killed objective keeps the state it held"
        );
        assert_eq!(runtime.is_present(SymbolId(2)), Some(false));

        // A declared completion still lands on a killed objective: presence is
        // the wake guard the engine measured, not a general lock-out.
        let requests = [(SymbolId(2), ObjectiveState::Succeeded)];
        let mut input = TickInput::at(Tick(2));
        input.objective_requests = &requests;
        let tick = runtime.step(&input).expect("a legal tick");
        assert_eq!(
            runtime.objective_state(SymbolId(2)),
            Some(ObjectiveState::Succeeded),
            "a declared move is not gated by presence"
        );
        assert_eq!(
            tick.outcome, None,
            "the fixture declares no terminal completion"
        );
    }

    /// M01-B-OBJ-1's `zbd/c3/m05` `OBJECTIVE8` shape: one declaration naming
    /// one target in both a wake and a nap is no longer refused — the engine's
    /// fixed order decides it, and the target ends napping.
    #[test]
    fn accept_m01_b_obj_1_fu1_wake_then_nap_on_one_target_ends_napping() {
        let mut runtime = runtime_with(vec![
            spec(
                1,
                ObjectiveState::Active,
                vec![
                    effect(CompletionEffectKind::Nap, 2, Some(2.0)),
                    effect(CompletionEffectKind::Wake, 2, None),
                ],
            ),
            spec(2, ObjectiveState::Pending, Vec::new()),
        ]);

        let tick = complete(&mut runtime, 1, 1);
        assert_eq!(
            changes(&tick, 1),
            vec![
                (SymbolId(2), ObjectiveState::Pending, ObjectiveState::Active),
                (SymbolId(2), ObjectiveState::Active, ObjectiveState::Napping),
            ],
            "the target is woken first and napped after, so it ends napping"
        );
        assert_eq!(
            runtime.objective_state(SymbolId(2)),
            Some(ObjectiveState::Napping)
        );
    }

    /// What stays refused: two *different* completions naming one target with
    /// different effects. The engine completes one objective per pass, so
    /// which of the two should apply last when both complete in one tick
    /// depends on the unmeasured pass-origin rule.
    #[test]
    fn accept_m01_b_obj_1_fu1_cross_completion_conflicts_still_refuse_by_name() {
        for order in [0, 1] {
            let wake = spec(
                1,
                ObjectiveState::Active,
                vec![effect(CompletionEffectKind::Wake, 3, None)],
            );
            let kill = spec(
                2,
                ObjectiveState::Active,
                vec![effect(CompletionEffectKind::Kill, 3, None)],
            );
            let (first, second) = if order == 0 {
                (wake, kill)
            } else {
                (kill, wake)
            };
            let mut runtime = runtime_with(vec![first]);
            let refusal = runtime.add_objective(second);
            assert_eq!(
                refusal,
                Err(RuntimeError::AmbiguousCompletionEffect {
                    source: SymbolId(if order == 0 { 2 } else { 1 }),
                    target: SymbolId(3),
                }),
                "registration order decides nothing"
            );
            let message = RuntimeError::AmbiguousCompletionEffect {
                source: SymbolId(1),
                target: SymbolId(3),
            }
            .to_string();
            assert!(
                message.contains("completes one objective per pass"),
                "the refusal states the measured rule it leans on: {message}"
            );
        }
    }

    /// What the narrowed refusal still covers in full: the cross-completion
    /// check compares against **every** kind a registered declaration names,
    /// not only the first kind its authored order spells. A declaration that
    /// already carries two effects on one target (the corpus's within-block
    /// shape, legal since the engine's fixed order decides it) still collides
    /// with a *different* completion naming that target with either of them,
    /// whichever order the two register in.
    #[test]
    fn accept_m01_b_obj_1_fu1_multi_effect_declaration_refuses_cross_completion_collision() {
        // Objective 1 carries the corpus's within-declaration shape: a nap and
        // a wake on the same target, spelled nap first — the wake is the kind
        // a check that only read each declaration's first kind would miss.
        let both = spec(
            1,
            ObjectiveState::Active,
            vec![
                effect(CompletionEffectKind::Nap, 3, Some(2.0)),
                effect(CompletionEffectKind::Wake, 3, None),
            ],
        );
        // Objective 2 names the same target with only one of the two effects.
        let single = spec(
            2,
            ObjectiveState::Active,
            vec![effect(CompletionEffectKind::Nap, 3, Some(1.0))],
        );
        for order in [0, 1] {
            let first = if order == 0 {
                both.clone()
            } else {
                single.clone()
            };
            let second = if order == 0 {
                single.clone()
            } else {
                both.clone()
            };
            let mut runtime = runtime_with(vec![first]);
            assert_eq!(
                runtime.add_objective(second),
                Err(RuntimeError::AmbiguousCompletionEffect {
                    source: SymbolId(if order == 0 { 2 } else { 1 }),
                    target: SymbolId(3),
                }),
                "registration order decides nothing: the nap/wake collision across two completions is refused however they arrive"
            );
        }
    }

    /// F39-E6's residue, refused on this side of the boundary too: one
    /// [`ObjectiveSpec`] naming the same `(kind, target)` pair twice is the
    /// declared form of a block spelling one completion-effect key twice, a
    /// shape the readable corpus spells nowhere — so what its second site
    /// does is unmeasured and the spec is refused by name, not registered
    /// with a repeat the declaration never authorized.
    #[test]
    fn accept_f39_e6_runtime_repeat_a_repeated_pair_inside_one_spec_is_refused_by_name() {
        let mut runtime = ObjectiveRuntime::new(
            GEN,
            TerminalPrecedence::SyntheticConservative,
            RuntimeLimits::default(),
        );
        let refusal = runtime.add_objective(spec(
            1,
            ObjectiveState::Active,
            vec![
                effect(CompletionEffectKind::Wake, 2, None),
                effect(CompletionEffectKind::Wake, 2, None),
            ],
        ));
        assert_eq!(
            refusal,
            Err(RuntimeError::RepeatedCompletionEffect {
                source: SymbolId(1),
                target: SymbolId(2),
                kind: CompletionEffectKind::Wake,
            }),
            "the repeat is refused by name, per declaring spec and per (kind, target) pair"
        );
        assert_eq!(
            runtime.objective_state(SymbolId(1)),
            None,
            "the refused spec registers nothing: the repeat never reaches the runtime"
        );
        let message = RuntimeError::RepeatedCompletionEffect {
            source: SymbolId(1),
            target: SymbolId(2),
            kind: CompletionEffectKind::Wake,
        }
        .to_string();
        assert!(
            message.contains("UNMEASURED_REPEATED_EFFECT_KEY"),
            "the refusal is tied to the named unknown behind the schema's own refusal: {message}"
        );
        assert!(
            message.contains("twice"),
            "the refusal states the shape it refuses: {message}"
        );
    }

    /// The pair is `(kind, target)` and nothing else, and one kind naming two
    /// different targets is the legal neighbour: a nap repeated on one target
    /// is refused whether or not the two sites carry different numbers (they
    /// are data, not part of the key), while the same kind on two targets is
    /// one multi-target site's residue and registers as it always did.
    #[test]
    fn accept_f39_e6_runtime_repeat_the_pair_is_kind_and_target_only() {
        let mut runtime = ObjectiveRuntime::new(
            GEN,
            TerminalPrecedence::SyntheticConservative,
            RuntimeLimits::default(),
        );
        for arguments in [[0.5, 170.0], [0.5, 0.5]] {
            let refusal = runtime.add_objective(spec(
                1,
                ObjectiveState::Active,
                vec![
                    effect(CompletionEffectKind::Nap, 2, Some(arguments[0])),
                    effect(CompletionEffectKind::Nap, 2, Some(arguments[1])),
                ],
            ));
            assert_eq!(
                refusal,
                Err(RuntimeError::RepeatedCompletionEffect {
                    source: SymbolId(1),
                    target: SymbolId(2),
                    kind: CompletionEffectKind::Nap,
                }),
                "the numbers {arguments:?} change nothing: the pair is the key"
            );
        }

        // One kind, two different targets: not a repeat, so it registers and
        // both sites are still queued for the engine's fixed order.
        let mut runtime = runtime_with(vec![
            spec(
                1,
                ObjectiveState::Active,
                vec![
                    effect(CompletionEffectKind::Wake, 2, None),
                    effect(CompletionEffectKind::Wake, 3, None),
                ],
            ),
            spec(2, ObjectiveState::Pending, Vec::new()),
            spec(3, ObjectiveState::Pending, Vec::new()),
        ]);
        assert_eq!(
            runtime.objective_state(SymbolId(1)),
            Some(ObjectiveState::Active),
            "one kind naming two targets is a multi-target site's residue, not a repeat"
        );
        let tick = complete(&mut runtime, 1, 1);
        assert_eq!(
            changes(&tick, 1),
            vec![
                (SymbolId(2), ObjectiveState::Pending, ObjectiveState::Active),
                (SymbolId(3), ObjectiveState::Pending, ObjectiveState::Active),
            ],
            "both sites still apply"
        );
    }

    /// The two neighbours the refusal deliberately keeps legal, and the one
    /// refusal it does not replace: the same pair in two *different* specs
    /// agrees on what happens (either registration order), while two
    /// different kinds naming one target across two completions is still
    /// `AmbiguousCompletionEffect` and not the repeat's name.
    #[test]
    fn accept_f39_e6_runtime_repeat_the_legal_neighbours_and_the_other_refusal_stay() {
        for order in [0, 1] {
            let one = spec(
                1,
                ObjectiveState::Active,
                vec![effect(CompletionEffectKind::Wake, 3, None)],
            );
            let two = spec(
                2,
                ObjectiveState::Active,
                vec![effect(CompletionEffectKind::Wake, 3, None)],
            );
            let (first, second) = if order == 0 { (one, two) } else { (two, one) };
            let mut runtime = runtime_with(vec![first]);
            assert_eq!(
                runtime.add_objective(second),
                Ok(()),
                "the same pair in a different spec agrees on what happens, whichever registers first"
            );
        }

        let mut runtime = runtime_with(vec![spec(
            1,
            ObjectiveState::Active,
            vec![effect(CompletionEffectKind::Wake, 3, None)],
        )]);
        assert_eq!(
            runtime.add_objective(spec(
                2,
                ObjectiveState::Active,
                vec![effect(CompletionEffectKind::Kill, 3, None)],
            )),
            Err(RuntimeError::AmbiguousCompletionEffect {
                source: SymbolId(2),
                target: SymbolId(3),
            }),
            "the different-effects-one-target refusal keeps its own name"
        );
    }

    /// The refusal is strictly additive: a spec carrying the repeat *and* a
    /// cross-declaration conflict is still refused under the name the cross
    /// scan gave it before this gate existed, because that scan runs first —
    /// no spec that was already refused changes name here. (The schema walks
    /// its two checks per effect, so on this shape it names the repeat; the
    /// two gates still refuse exactly the same shapes.)
    #[test]
    fn accept_f39_e6_runtime_repeat_a_both_shapes_spec_keeps_the_cross_declaration_name() {
        let mut runtime = runtime_with(vec![spec(
            1,
            ObjectiveState::Active,
            vec![effect(CompletionEffectKind::Wake, 4, None)],
        )]);
        let refusal = runtime.add_objective(spec(
            2,
            ObjectiveState::Active,
            vec![
                effect(CompletionEffectKind::Wake, 3, None),
                effect(CompletionEffectKind::Wake, 3, None),
                effect(CompletionEffectKind::Kill, 4, None),
            ],
        ));
        assert_eq!(
            refusal,
            Err(RuntimeError::AmbiguousCompletionEffect {
                source: SymbolId(2),
                target: SymbolId(4),
            }),
            "the cross-declaration scan runs before the repeat check, so a spec refused \
             before this gate existed keeps that refusal's name"
        );
        assert_eq!(
            runtime.objective_state(SymbolId(2)),
            None,
            "refused either way: nothing of the both-shapes spec registers"
        );
    }

    /// The nap's number is the measured nap time, carried as data and never
    /// converted into this runtime's ticks: two programs differing only in it
    /// produce identical streams, and no committed tick ends a nap by itself.
    #[test]
    fn accept_m01_b_obj_1_fu1_a_nap_time_is_carried_and_never_converted() {
        let napped = |time: f64| {
            vec![
                spec(
                    1,
                    ObjectiveState::Active,
                    vec![effect(CompletionEffectKind::Nap, 2, Some(time))],
                ),
                spec(2, ObjectiveState::Active, Vec::new()),
            ]
        };
        let mut short = runtime_with(napped(0.5));
        let mut long = runtime_with(napped(170.0));

        let short_tick = complete(&mut short, 1, 1);
        let long_tick = complete(&mut long, 1, 1);
        assert_eq!(
            short_tick.events, long_tick.events,
            "the declared number may not change what the nap does"
        );
        assert_eq!(
            short.objective_nap_time(SymbolId(2)).map(NapTime::value),
            Some(0.5),
            "and it arrives verbatim on the target"
        );

        // Nothing expires a nap: the engine's re-wake compares the time
        // against a per-pass step whose unit is unmeasured, so this runtime
        // never ends one by elapsed time.
        let idle = TickInput {
            tick: Tick(2),
            committed_ticks: 100_000,
            lifecycles: &[],
            movements: &[],
            signals: &[],
            timer_requests: &[],
            objective_requests: &[],
            terminal_requests: &[],
        };
        short.step(&idle).expect("a legal tick");
        assert_eq!(
            short.objective_state(SymbolId(2)),
            Some(ObjectiveState::Napping),
            "a committed tick never ends a nap by itself"
        );
    }

    /// The vocabulary itself: the measured moves, the engine's fixed ranks,
    /// and the non-cascading guarantee none of them completes a target.
    #[test]
    fn accept_m01_b_obj_1_fu1_the_effect_vocabulary_carries_the_measured_moves() {
        assert_eq!(
            CompletionEffectKind::Wake.moves_to(),
            Some(ObjectiveState::Active)
        );
        assert_eq!(
            CompletionEffectKind::Wakeup.moves_to(),
            Some(ObjectiveState::Active)
        );
        assert_eq!(
            CompletionEffectKind::Nap.moves_to(),
            Some(ObjectiveState::Napping)
        );
        assert_eq!(
            CompletionEffectKind::Sleep.moves_to(),
            Some(ObjectiveState::Asleep)
        );
        assert_eq!(CompletionEffectKind::Kill.moves_to(), None);
        for kind in [
            CompletionEffectKind::Wake,
            CompletionEffectKind::Wakeup,
            CompletionEffectKind::Nap,
            CompletionEffectKind::Kill,
            CompletionEffectKind::Sleep,
        ] {
            assert_ne!(
                kind.moves_to(),
                Some(ObjectiveState::Succeeded),
                "{kind:?} must not complete its target"
            );
        }
        // The engine's fixed order, as ranks.
        assert_eq!(CompletionEffectKind::Wake.engine_order(), 0);
        assert_eq!(CompletionEffectKind::Wakeup.engine_order(), 0);
        assert_eq!(CompletionEffectKind::Nap.engine_order(), 1);
        assert_eq!(CompletionEffectKind::Kill.engine_order(), 2);
        assert_eq!(CompletionEffectKind::Sleep.engine_order(), 3);
        // Only a nap carries the measured number.
        assert!(CompletionEffectKind::Nap.carries_argument());
        for kind in [
            CompletionEffectKind::Wake,
            CompletionEffectKind::Wakeup,
            CompletionEffectKind::Kill,
            CompletionEffectKind::Sleep,
        ] {
            assert!(!kind.carries_argument());
        }
        // A sleep effect lands through the same table as everything else.
        let mut runtime = runtime_with(vec![
            spec(
                1,
                ObjectiveState::Active,
                vec![effect(CompletionEffectKind::Sleep, 2, None)],
            ),
            spec(2, ObjectiveState::Active, Vec::new()),
        ]);
        let tick = complete(&mut runtime, 1, 1);
        assert_eq!(
            changes(&tick, 1),
            vec![(SymbolId(2), ObjectiveState::Active, ObjectiveState::Asleep)]
        );
    }
}
