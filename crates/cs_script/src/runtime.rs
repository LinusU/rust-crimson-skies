//! Mutable mission state, stable event ordering, the bounded evaluator, the
//! pending-work queue and the save record (F37-A, F37-B, F37-C).
//!
//! Program data is [`crate::ir`]; this module holds the *execution* state and
//! the pure per-tick resolution that the simulation host drives. The bounded
//! work budget and the deferred work queue are F37-B; the versioned
//! [`MissionStateSnapshot`] that preserves a pending timer's exact remaining
//! ticks across save/restore is F37-C. Host effect application is the
//! simulation side (`cs_sim::mission`).
//!
//! Phases of one tick (`docs/contracts/SCRIPT-MISSION.md`, "Objective event
//! ordering"; "actions do not directly recurse into callbacks"):
//! 1. **Observe** — every condition is evaluated against the state as it was
//!    at the start of the tick; nothing evaluated in this tick sees a write
//!    made in this tick.
//! 2. **Queue + resolve objectives** — firing objectives are taken in program
//!    order and their actions run. `Schedule`/`Reschedule` never call back
//!    into evaluation: they append to the *pending* queue, which is drained
//!    separately.
//! 3. **Drain pending work** — items whose `due` tick is now or past run in
//!    (due, enqueue) order, one action at a time. A zero-delay item appends to
//!    the end of the queue being drained, so it cannot starve earlier work.
//! 4. **Apply** — `State` writes become visible, `Terminal` requests are
//!    resolved by the [`PrecedencePolicy`], `Host` effects are emitted.
//!
//! Bounds (contract: "each tick has an instruction/action budget and
//! recursion/stack limits"):
//! - every objective firing, pending dequeue and action execution spends one
//!   unit of the per-tick [`WorkLimits::max_work_per_tick`] budget;
//! - the pending queue holds at most
//!   [`WorkLimits::max_pending_items`] scheduled items (memory cap);
//! - `Schedule` nesting is bounded at validation ([`crate::ir::MAX_ACTION_NESTING`]).
//!
//! When the budget runs out the tick does **not** error: the interrupted list
//! is re-queued at its next action, everything already executed stays
//! committed, and the result carries [`StopReason`] with the mission id,
//! program locator and the trace — the contract's budget diagnostic. Nothing
//! is skipped and nothing is repeated.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::random::SplitMix64;

use crate::ir::{
    Action, ActorId, ActorState, CompareOp, Condition, MAX_ACTIONS_PER_OBJECTIVE, Outcome,
    ProgramLocator, SymbolId, ValidatedProgram, Value, ValueType,
};

/// SplitMix64 domain separating the mission evaluator's stream from every
/// other consumer of a run seed (`"MSN_EVAL"`). See
/// `cs_types::random::SplitMix64::for_domain`.
const MISSION_EVALUATOR_DOMAIN: u64 = 0x4D53_4E5F_4556_414C;

/// Default per-tick work budget: units of "one firing, one dequeue or one
/// action". The value is a design bound, not a measured original limit.
pub const MAX_WORK_PER_TICK: u64 = 4096;

/// Default cap on stored scheduled items (contract: "cap memory/time").
/// A design bound, not a measured original limit.
pub const MAX_PENDING_ITEMS: usize = 4096;

/// Version of the [`MissionStateSnapshot`] record this crate writes. It is
/// separate from [`crate::ir::IR_VERSION`] because a save outlives the program
/// it was taken from: an older record must be refused, never reinterpreted.
pub const SNAPSHOT_VERSION: u32 = 1;

/// Most `Draw`s a restore will replay to rewind the mission's RNG stream.
///
/// `SplitMix64` exposes no state getter (`cs_types::random` is outside this
/// stage's owner paths), so [`MissionStateSnapshot`] stores how many draws the
/// session took and a restore re-seeds the same domain-separated stream and
/// replays them. A design bound on restore work — contract: "cap memory/time"
/// — and not a measured original limit.
pub const MAX_RNG_REPLAY_DRAWS: u64 = 1 << 22;

/// The per-tick and queue bounds the evaluator runs under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkLimits {
    /// Most work units one tick may spend.
    pub max_work_per_tick: u64,
    /// Most scheduled items the pending queue may hold.
    pub max_pending_items: usize,
}

impl Default for WorkLimits {
    fn default() -> Self {
        Self {
            max_work_per_tick: MAX_WORK_PER_TICK,
            max_pending_items: MAX_PENDING_ITEMS,
        }
    }
}

/// Generation of one mission session; a restarted mission gets a new one so
/// stale events can never be confused with current ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionGeneration(pub u32);

/// Total order of emitted events: session, tick, source objective, then the
/// program sequence inside that objective (0 = the completion itself, then
/// action index + 1). Never hash or entity order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventKey {
    pub session: SessionGeneration,
    pub tick: Tick,
    pub source: SymbolId,
    pub sequence: u32,
}

impl EventKey {
    /// The key without the tick: identifies *what* was consumed so a retry on
    /// a later tick cannot repeat a reward or a capture.
    pub fn execution_key(&self) -> ExecutionKey {
        ExecutionKey {
            session: self.session,
            source: self.source,
            sequence: self.sequence,
        }
    }
}

/// Exactly-once identity of one event or action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExecutionKey {
    pub session: SessionGeneration,
    pub source: SymbolId,
    pub sequence: u32,
}

/// The mission's terminal state (contract: exactly one of these).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalState {
    Running,
    Succeeded,
    Failed,
    Aborted,
    /// Reached only through launch refusal ([`crate::ir::ValidationError`]),
    /// never from evaluation.
    Unsupported,
}

impl From<Outcome> for TerminalState {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Succeeded => Self::Succeeded,
            Outcome::Failed => Self::Failed,
            Outcome::Aborted => Self::Aborted,
        }
    }
}

/// How simultaneous terminal requests on one tick are resolved.
///
/// The original game's precedence is **unmeasured**: this is a designed
/// conservative policy, valid for synthetic tests only until an original
/// observation replaces it (contract, "Objective event ordering").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecedencePolicy {
    /// `Aborted` beats `Failed` beats `Succeeded`.
    SyntheticConservative,
}

impl PrecedencePolicy {
    fn pick(self, requested: &BTreeSet<Outcome>) -> Option<Outcome> {
        match self {
            // `Outcome`'s order is Succeeded < Failed < Aborted.
            Self::SyntheticConservative => requested.iter().next_back().copied(),
        }
    }
}

/// What the simulation tells the mission about actors this tick. An actor
/// absent from the map matches no [`Condition::ActorIs`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissionFacts {
    pub actors: BTreeMap<ActorId, ActorState>,
}

/// What an emitted event is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventKind {
    ObjectiveCompleted,
    RewardGranted(ContentId),
    TerminalRequested(Outcome),
}

/// One ordered event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionEvent {
    pub key: EventKey,
    pub kind: EventKind,
}

/// Result of one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickResult {
    pub tick: Tick,
    /// Sorted by [`EventKey`].
    pub events: Vec<MissionEvent>,
    pub terminal: TerminalState,
    /// Set when a bound stopped the tick early: the contract's budget
    /// diagnostic (mission id, program locator, trace) plus `events`, the
    /// short event trace of everything that ran before the stop.
    pub stop: Option<StopReason>,
}

/// Why one tick stopped before its work was done. Never a mission failure:
/// the interrupted list is re-queued at its next action, the work spent is
/// committed, and a later tick resumes it. `Running` keeps flowing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The per-tick work budget ran out.
    WorkBudget { at: ProgramLocator, spent: u64 },
    /// A `Schedule`/`Reschedule` could not enqueue because the pending queue
    /// already holds `WorkLimits::max_pending_items` items; the enqueueing
    /// action is retried on the next tick.
    PendingLimit { at: ProgramLocator, queued: usize },
    /// The session's work-item ordinal space ran out (~66 million scheduled
    /// items): the locator names the schedule that could not take a key.
    SequenceExhausted { at: ProgramLocator },
}

/// Why a tick was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickError {
    /// The tick is not after the last evaluated one (a replay or retry).
    NotAdvancing { last: Tick, given: Tick },
}

/// One scheduled work item as the save record sees it.
///
/// The record keeps the item's action list, not a reference into the program:
/// a budget stop defers the *unexecuted suffix* of an action list, so the
/// resume point cannot be re-derived from program data alone. Program data is
/// immutable and shared, so carrying the deferred text in the record costs
/// space and never a second source of truth.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduledWork {
    /// The program symbol the item's events and diagnostics attribute to.
    pub source: SymbolId,
    /// Session-unique ordinal that keeps this item's event keys distinct.
    pub ordinal: u32,
    /// First tick the item is eligible on.
    pub due: Tick,
    /// First action not yet executed.
    pub next: usize,
    pub actions: Vec<Action>,
}

/// One queued item as an observer sees it: when it fires and how much of it is
/// left. The same view is available on live state
/// ([`MissionState::pending_timers`]) and on a save record
/// ([`MissionStateSnapshot::pending_timers`]), so a caller can compare what was
/// pending before and after a restore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingTimer {
    /// The program symbol the item's events attribute to.
    pub source: SymbolId,
    /// The tick the item becomes eligible on.
    pub due: Tick,
    /// Ticks from the session's last evaluated tick to [`PendingTimer::due`];
    /// `0` means "eligible on the tick evaluated last", which is what a
    /// work-budget stop leaves behind.
    pub remaining: u64,
    /// First action not yet executed.
    pub next: usize,
    /// How many actions the item holds in total.
    pub actions_total: usize,
}

impl PendingTimer {
    /// Actions still to run: a resumed item's tail.
    pub fn actions_remaining(&self) -> usize {
        self.actions_total - self.next
    }
}

/// Builds one [`PendingTimer`] view from a queue entry and the session's last
/// evaluated tick.
fn pending_timer(
    source: SymbolId,
    due: Tick,
    next: usize,
    actions_total: usize,
    last_tick: Option<Tick>,
) -> PendingTimer {
    let remaining = last_tick.map_or(due.0, |last| due.0.saturating_sub(last.0));
    PendingTimer {
        source,
        due,
        remaining,
        next,
        actions_total,
    }
}

/// The gameplay-relevant execution state of one mission session, as one
/// versioned record (contract, "IR requirements": "state snapshot/restore
/// must preserve all gameplay-relevant pieces or declare mid-mission save
/// unsupported").
///
/// It carries every piece that can change a later observation: variable
/// values, the latched objectives, the consumed execution keys, the terminal
/// state, the last evaluated tick, the whole pending queue in drain order with
/// its eligibility ticks, the item-ordinal counter and the RNG draw count.
/// Mid-mission save is therefore supported; nothing is declared unsupported.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionStateSnapshot {
    pub version: u32,
    /// The mission this state belongs to; a record may only be restored into
    /// the program that produced it.
    pub mission: ContentId,
    pub session: SessionGeneration,
    /// In `SymbolId` order, which is the live map's own order.
    pub variables: Vec<(SymbolId, Value)>,
    /// Latched objectives in `SymbolId` order.
    pub completed: Vec<SymbolId>,
    /// Every execution key already emitted, in key order.
    pub consumed: Vec<ExecutionKey>,
    pub terminal: TerminalState,
    /// The last evaluated tick; a restored session still refuses to re-evaluate
    /// it ([`TickError::NotAdvancing`]).
    pub last_tick: Option<Tick>,
    pub policy: PrecedencePolicy,
    /// The whole pending queue in `(due, enqueue)` order — the exact order a
    /// drain would rebuild it in.
    pub pending: Vec<ScheduledWork>,
    /// Ordinal the next scheduled item will take.
    pub next_item_ordinal: u32,
    pub limits: WorkLimits,
    /// Draws taken from the session's RNG stream.
    pub rng_draws: u64,
}

impl MissionStateSnapshot {
    /// The pending queue as [`PendingTimer`]s, in the same `(due, enqueue)`
    /// order and relative to the record's own `last_tick`.
    pub fn pending_timers(&self) -> Vec<PendingTimer> {
        self.pending
            .iter()
            .map(|item| {
                pending_timer(
                    item.source,
                    item.due,
                    item.next,
                    item.actions.len(),
                    self.last_tick,
                )
            })
            .collect()
    }
}

/// An internal inconsistency in a save record. The record is data from outside
/// the process, so every field is checked instead of trusted: restoring a
/// record that broke these invariants would silently change which events
/// fire, which is the one thing an execution key exists to prevent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreDefect {
    /// Two queued items share one ordinal, so their event keys can collide and
    /// the exactly-once guard would swallow one item's events.
    DuplicateOrdinal { ordinal: u32 },
    /// An item's ordinal is at or above `next_item_ordinal`, so this session
    /// never allocated it.
    OrdinalNotAllocated { ordinal: u32 },
    /// An item's resume cursor points outside its action list.
    ActionCursor { next: usize, actions: usize },
    /// The queue is not in `(due, enqueue)` order, so draining it would not
    /// reproduce the order the record claims.
    PendingOrder { previous: Tick, given: Tick },
    /// A consumed execution key belongs to another session.
    ForeignExecutionKey { session: SessionGeneration },
}

/// Why a save record could not be restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreError {
    /// The record was written by another snapshot version.
    SnapshotVersion { found: u32 },
    /// The record belongs to another mission.
    MissionMismatch {
        expected: ContentId,
        found: ContentId,
    },
    /// A variable in the record is not declared by the program.
    UnknownVariable { symbol: SymbolId },
    /// A latched objective in the record is not declared by the program.
    UnknownObjective { symbol: SymbolId },
    /// A record value's type differs from the program's declaration, so the
    /// restore would install a value no condition can compare.
    TypeMismatch {
        symbol: SymbolId,
        expected: ValueType,
        found: ValueType,
    },
    /// The record is internally inconsistent.
    Corrupt { defect: RestoreDefect },
    /// The session took more draws than [`MAX_RNG_REPLAY_DRAWS`], so rewinding
    /// its RNG stream would exceed the restore work bound.
    RngReplayTooLong { draws: u64 },
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotVersion { found } => {
                write!(
                    f,
                    "snapshot version {found} unsupported (expected {SNAPSHOT_VERSION})"
                )
            }
            Self::MissionMismatch { expected, found } => {
                write!(f, "snapshot is for {found}, not {expected}")
            }
            Self::UnknownVariable { symbol } => {
                write!(
                    f,
                    "snapshot variable #{} is not declared by the program",
                    symbol.0
                )
            }
            Self::UnknownObjective { symbol } => {
                write!(
                    f,
                    "snapshot objective #{} is not declared by the program",
                    symbol.0
                )
            }
            Self::TypeMismatch {
                symbol,
                expected,
                found,
            } => write!(
                f,
                "snapshot variable #{} holds {found:?}, program declares {expected:?}",
                symbol.0
            ),
            Self::Corrupt { defect } => match defect {
                RestoreDefect::DuplicateOrdinal { ordinal } => {
                    write!(f, "two pending items share ordinal {ordinal}")
                }
                RestoreDefect::OrdinalNotAllocated { ordinal } => {
                    write!(f, "pending item ordinal {ordinal} was never allocated")
                }
                RestoreDefect::ActionCursor { next, actions } => {
                    write!(f, "pending item resumes at action {next} of {actions}")
                }
                RestoreDefect::PendingOrder { previous, given } => write!(
                    f,
                    "pending queue is not due-ordered: tick {} follows tick {}",
                    given.0, previous.0
                ),
                RestoreDefect::ForeignExecutionKey { session } => {
                    write!(f, "execution key belongs to session {}", session.0)
                }
            },
            Self::RngReplayTooLong { draws } => write!(
                f,
                "{draws} RNG draws exceed the restore replay bound {MAX_RNG_REPLAY_DRAWS}"
            ),
        }
    }
}

impl std::error::Error for RestoreError {}

/// Event-key sequence space one pending work item owns. An objective's own
/// events use `0..=MAX_ACTIONS_PER_OBJECTIVE`; an item's events live above
/// that, packed by the item's session-unique ordinal, so two items from one
/// source can never collide and a restore-replay re-emits identical keys.
const SEQS_PER_ITEM: u32 = MAX_ACTIONS_PER_OBJECTIVE as u32 + 1;

fn item_sequence(ordinal: u32, action_index: usize) -> Option<u32> {
    ordinal
        .checked_add(1)?
        .checked_mul(SEQS_PER_ITEM)?
        .checked_add(action_index as u32 + 1)
}

/// One queued work item: a validated action list eligible from `due`,
/// resumable after a bound stop (`next` is the first action not yet run).
#[derive(Clone, Debug, PartialEq)]
struct PendingWork {
    /// The program symbol the item's events and diagnostics attribute to —
    /// the objective (or item) that scheduled it.
    source: SymbolId,
    /// Unique within the session; separates this item's event keys from every
    /// other item sharing `source`.
    ordinal: u32,
    /// First tick the item is eligible on; a later tick still drains it.
    due: Tick,
    /// First action not yet executed (`0` for a fresh item).
    next: usize,
    actions: Vec<Action>,
}

/// The work one tick is in the middle of: buffered state writes, terminal
/// requests and the due-item queue being drained.
struct TickRun {
    writes: Vec<(SymbolId, Value)>,
    requested: BTreeSet<Outcome>,
    ready: VecDeque<PendingWork>,
    work: u64,
}

/// Mutable execution state, kept apart from the program.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionState {
    session: SessionGeneration,
    variables: BTreeMap<SymbolId, Value>,
    completed: BTreeSet<SymbolId>,
    consumed: BTreeSet<ExecutionKey>,
    terminal: TerminalState,
    last_tick: Option<Tick>,
    policy: PrecedencePolicy,
    /// Scheduled work, keyed by eligibility tick.
    pending: BTreeMap<Tick, VecDeque<PendingWork>>,
    /// Count of items stored in `pending` (the memory cap's account).
    pending_len: usize,
    /// Session-unique ordinal for the next scheduled item.
    next_item_ordinal: u32,
    limits: WorkLimits,
    /// The mission's explicit RNG stream, seeded from the session so a replay
    /// of one session reproduces every draw bit-for-bit.
    rng: SplitMix64,
    /// Draws taken from `rng`; a restore replays them (see
    /// [`MAX_RNG_REPLAY_DRAWS`]).
    rng_draws: u64,
}

impl MissionState {
    /// Initial state of a validated program.
    pub fn new(program: &ValidatedProgram, session: SessionGeneration) -> Self {
        Self {
            session,
            variables: program
                .program()
                .variables
                .iter()
                .map(|v| (v.id, v.initial.clone()))
                .collect(),
            completed: BTreeSet::new(),
            consumed: BTreeSet::new(),
            terminal: TerminalState::Running,
            last_tick: None,
            policy: PrecedencePolicy::SyntheticConservative,
            pending: BTreeMap::new(),
            pending_len: 0,
            next_item_ordinal: 0,
            limits: WorkLimits::default(),
            rng: SplitMix64::for_domain(session.0 as u64, MISSION_EVALUATOR_DOMAIN),
            rng_draws: 0,
        }
    }

    pub fn terminal(&self) -> TerminalState {
        self.terminal
    }

    /// Overrides the work/queue bounds; the default is [`WorkLimits::default`].
    pub fn set_limits(&mut self, limits: WorkLimits) {
        self.limits = limits;
    }

    /// Scheduled items still waiting for their eligibility tick.
    pub fn queued_items(&self) -> usize {
        self.pending_len
    }

    pub fn variable(&self, id: SymbolId) -> Option<&Value> {
        self.variables.get(&id)
    }

    pub fn is_completed(&self, objective: SymbolId) -> bool {
        self.completed.contains(&objective)
    }

    /// The last evaluated tick, or `None` before the session's first step.
    pub fn last_tick(&self) -> Option<Tick> {
        self.last_tick
    }

    /// The pending queue as [`PendingTimer`]s, in `(due, enqueue)` order.
    /// Each carries the exact remaining ticks of one scheduled item.
    pub fn pending_timers(&self) -> Vec<PendingTimer> {
        self.pending
            .values()
            .flatten()
            .map(|item| {
                pending_timer(
                    item.source,
                    item.due,
                    item.next,
                    item.actions.len(),
                    self.last_tick,
                )
            })
            .collect()
    }

    /// The versioned save record of this state (F37-C). Everything that can
    /// change a later observation is in it, so
    /// [`MissionState::restore`] reproduces the session exactly.
    pub fn snapshot(&self, program: &ValidatedProgram) -> MissionStateSnapshot {
        let pending = self
            .pending
            .values()
            .flatten()
            .map(|item| ScheduledWork {
                source: item.source,
                ordinal: item.ordinal,
                due: item.due,
                next: item.next,
                actions: item.actions.clone(),
            })
            .collect();
        MissionStateSnapshot {
            version: SNAPSHOT_VERSION,
            mission: program.program().mission.clone(),
            session: self.session,
            variables: self
                .variables
                .iter()
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            completed: self.completed.iter().copied().collect(),
            consumed: self.consumed.iter().copied().collect(),
            terminal: self.terminal,
            last_tick: self.last_tick,
            policy: self.policy,
            pending,
            next_item_ordinal: self.next_item_ordinal,
            limits: self.limits,
            rng_draws: self.rng_draws,
        }
    }

    /// Rebuilds execution state from a [`MissionStateSnapshot`].
    ///
    /// # Errors
    ///
    /// [`RestoreError`]: a foreign, older or internally inconsistent record is
    /// refused with the precise defect, never partially applied.
    pub fn restore(
        program: &ValidatedProgram,
        snapshot: MissionStateSnapshot,
    ) -> Result<Self, RestoreError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(RestoreError::SnapshotVersion {
                found: snapshot.version,
            });
        }
        let expected = &program.program().mission;
        if &snapshot.mission != expected {
            return Err(RestoreError::MissionMismatch {
                expected: expected.clone(),
                found: snapshot.mission.clone(),
            });
        }
        let declared = |symbol: SymbolId| {
            program
                .program()
                .variables
                .iter()
                .find(|v| v.id == symbol)
                .map(|v| v.initial.value_type())
        };
        let mut variables = BTreeMap::new();
        for (symbol, value) in &snapshot.variables {
            let Some(ty) = declared(*symbol) else {
                return Err(RestoreError::UnknownVariable { symbol: *symbol });
            };
            if value.value_type() != ty {
                return Err(RestoreError::TypeMismatch {
                    symbol: *symbol,
                    expected: ty,
                    found: value.value_type(),
                });
            }
            variables.insert(*symbol, value.clone());
        }
        for symbol in &snapshot.completed {
            if !program.program().objectives.iter().any(|o| o.id == *symbol) {
                return Err(RestoreError::UnknownObjective { symbol: *symbol });
            }
        }
        for key in &snapshot.consumed {
            if key.session != snapshot.session {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::ForeignExecutionKey {
                        session: key.session,
                    },
                });
            }
        }
        if snapshot.rng_draws > MAX_RNG_REPLAY_DRAWS {
            return Err(RestoreError::RngReplayTooLong {
                draws: snapshot.rng_draws,
            });
        }
        let mut pending = BTreeMap::new();
        let mut pending_len = 0usize;
        let mut seen_ordinals = BTreeSet::new();
        let mut previous = None;
        for item in &snapshot.pending {
            if item.next > item.actions.len() {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::ActionCursor {
                        next: item.next,
                        actions: item.actions.len(),
                    },
                });
            }
            if !seen_ordinals.insert(item.ordinal) {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::DuplicateOrdinal {
                        ordinal: item.ordinal,
                    },
                });
            }
            if item.ordinal >= snapshot.next_item_ordinal {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::OrdinalNotAllocated {
                        ordinal: item.ordinal,
                    },
                });
            }
            if let Some(before) = previous
                && before > item.due
            {
                return Err(RestoreError::Corrupt {
                    defect: RestoreDefect::PendingOrder {
                        previous: before,
                        given: item.due,
                    },
                });
            }
            previous = Some(item.due);
            pending
                .entry(item.due)
                .or_insert_with(VecDeque::new)
                .push_back(PendingWork {
                    source: item.source,
                    ordinal: item.ordinal,
                    due: item.due,
                    next: item.next,
                    actions: item.actions.clone(),
                });
            pending_len += 1;
        }
        let mut rng = SplitMix64::for_domain(snapshot.session.0 as u64, MISSION_EVALUATOR_DOMAIN);
        for _ in 0..snapshot.rng_draws {
            rng.next_u64();
        }
        Ok(Self {
            session: snapshot.session,
            variables,
            completed: snapshot.completed.iter().copied().collect(),
            consumed: snapshot.consumed.iter().copied().collect(),
            terminal: snapshot.terminal,
            last_tick: snapshot.last_tick,
            policy: snapshot.policy,
            pending,
            pending_len,
            next_item_ordinal: snapshot.next_item_ordinal,
            limits: snapshot.limits,
            rng,
            rng_draws: snapshot.rng_draws,
        })
    }

    /// Drops every queued work item and returns how many were dropped.
    ///
    /// Teardown: once the mission is over, deferred program work must not fire
    /// on a later tick, and leaving it queued would keep it alive in memory and
    /// in the save record. Returns the count so a caller can report it.
    pub fn teardown(&mut self) -> usize {
        let dropped = self.pending_len;
        self.pending.clear();
        self.pending_len = 0;
        dropped
    }

    /// Resolves one tick. See the module docs for the phases and bounds.
    ///
    /// # Errors
    ///
    /// [`TickError::NotAdvancing`] when `tick` is not after the last one.
    /// Bound violations are not errors: they stop the tick early and are
    /// reported in [`TickResult::stop`].
    pub fn step(
        &mut self,
        program: &ValidatedProgram,
        facts: &MissionFacts,
        tick: Tick,
    ) -> Result<TickResult, TickError> {
        if let Some(last) = self.last_tick
            && tick <= last
        {
            return Err(TickError::NotAdvancing { last, given: tick });
        }
        self.last_tick = Some(tick);
        let mut result = TickResult {
            tick,
            events: Vec::new(),
            terminal: self.terminal,
            stop: None,
        };
        if self.terminal != TerminalState::Running {
            return Ok(result);
        }
        let mut run = TickRun {
            writes: Vec::new(),
            requested: BTreeSet::new(),
            // Every item already due — in (due, enqueue) order.
            ready: self.take_due(tick),
            work: 0,
        };
        let budget = self.limits.max_work_per_tick;

        // Observe: all conditions against the start-of-tick state.
        let firing: Vec<_> = program
            .program()
            .objectives
            .iter()
            .filter(|o| !self.completed.contains(&o.id) && self.holds(&o.condition, facts))
            .collect();

        // Queue + resolve objective actions, in program order. A budget stop
        // latches the interrupted objective (it *did* fire), defers its
        // unexecuted actions as pending work and stops the tick; objectives
        // after it never fired and stay unfired for a later tick.
        for o in firing {
            if run.work >= budget {
                self.completed.insert(o.id);
                self.emit(
                    &mut result,
                    EventKey {
                        session: self.session,
                        tick,
                        source: o.id,
                        sequence: 0,
                    },
                    EventKind::ObjectiveCompleted,
                );
                self.defer(tick, &mut run, o.id, o.actions.to_vec());
                result.stop = Some(StopReason::WorkBudget {
                    at: self.locator(program, o.id, &["objective fire"]),
                    spent: run.work,
                });
                break;
            }
            run.work += 1;
            self.completed.insert(o.id);
            let session = self.session;
            let key = |sequence| EventKey {
                session,
                tick,
                source: o.id,
                sequence,
            };
            self.emit(&mut result, key(0), EventKind::ObjectiveCompleted);
            for (i, action) in o.actions.iter().enumerate() {
                if run.work >= budget {
                    self.defer(tick, &mut run, o.id, o.actions[i..].to_vec());
                    result.stop = Some(StopReason::WorkBudget {
                        at: self.locator(program, o.id, &[&format!("action {i}")]),
                        spent: run.work,
                    });
                    break;
                }
                run.work += 1;
                if let Some(stop) = self.run_action(
                    program,
                    action,
                    key(i as u32 + 1),
                    tick,
                    o.actions.as_slice(),
                    i,
                    &mut run,
                    &mut result,
                ) {
                    // The action did not complete (e.g. its `Schedule` could
                    // not enqueue); defer the list from that action on so it
                    // is retried, never skipped.
                    if !matches!(stop, StopReason::WorkBudget { .. }) {
                        self.defer(tick, &mut run, o.id, o.actions[i..].to_vec());
                    }
                    result.stop = Some(stop);
                    break;
                }
            }
            if result.stop.is_some() {
                break;
            }
        }

        // Drain pending work: FIFO, one action at a time. Zero-delay items
        // scheduled in this tick append to `run.ready`'s back, so they still
        // run — but never before work already queued, and never unbounded:
        // the per-tick budget applies here too.
        while result.stop.is_none()
            && let Some(mut item) = run.ready.pop_front()
        {
            let item_source = item.source;
            if run.work >= budget {
                run.ready.push_front(item);
                result.stop = Some(StopReason::WorkBudget {
                    at: self.locator(program, item_source, &["pending dequeue"]),
                    spent: run.work,
                });
                break;
            }
            run.work += 1;
            while item.next < item.actions.len() {
                let action_index = item.next;
                if run.work >= budget {
                    run.ready.push_front(item);
                    result.stop = Some(StopReason::WorkBudget {
                        at: self.locator(
                            program,
                            item_source,
                            &[&format!("pending action {action_index}")],
                        ),
                        spent: run.work,
                    });
                    break;
                }
                run.work += 1;
                let Some(sequence) = item_sequence(item.ordinal, action_index) else {
                    run.ready.push_front(item);
                    result.stop = Some(StopReason::SequenceExhausted {
                        at: self.locator(
                            program,
                            item_source,
                            &[&format!("pending action {action_index}")],
                        ),
                    });
                    break;
                };
                item.next = action_index + 1;
                let key = EventKey {
                    session: self.session,
                    tick,
                    source: item_source,
                    sequence,
                };
                if let Some(stop) = self.run_action(
                    program,
                    &item.actions[action_index],
                    key,
                    tick,
                    item.actions.as_slice(),
                    action_index,
                    &mut run,
                    &mut result,
                ) {
                    // Every `run_action` stop means the action did not
                    // complete (its `Schedule`/`Reschedule` did not enqueue);
                    // resume at that action so it is retried, never skipped.
                    // On `PendingLimit` the item goes to the back so the
                    // items still queued for this tick run first and can
                    // free the cap.
                    item.next = action_index;
                    if matches!(stop, StopReason::PendingLimit { .. }) {
                        run.ready.push_back(item);
                    } else {
                        run.ready.push_front(item);
                    }
                    result.stop = Some(stop);
                    break;
                }
            }
        }

        // Anything not reached this tick stays pending; it is already due, so
        // the next tick drains it first.
        if !run.ready.is_empty() {
            let remaining = run.ready.len();
            self.pending.entry(tick).or_default().extend(run.ready);
            self.pending_len += remaining;
        }
        for (variable, value) in run.writes {
            self.variables.insert(variable, value);
        }
        if let Some(outcome) = self.policy.pick(&run.requested) {
            self.terminal = outcome.into();
        }
        result.events.sort_by_key(|e| e.key);
        result.terminal = self.terminal;
        Ok(result)
    }

    /// Moves every item eligible at `tick` out of `pending`, preserving
    /// (due, enqueue) order.
    fn take_due(&mut self, tick: Tick) -> VecDeque<PendingWork> {
        let keys: Vec<Tick> = self.pending.range(..=tick).map(|(t, _)| *t).collect();
        let mut ready = VecDeque::new();
        for key in keys {
            if let Some(items) = self.pending.remove(&key) {
                self.pending_len -= items.len();
                ready.extend(items);
            }
        }
        ready
    }

    /// Enqueues a scheduled work item. `delay == 0` appends to the queue being
    /// drained this tick — never stored, so exempt from the pending cap and
    /// bounded by the work budget instead; a positive delay stores it in
    /// `pending` and counts against the cap. Both paths are bounded: the
    /// pending cap and the session's ordinal space.
    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &mut self,
        program: &ValidatedProgram,
        run: &mut TickRun,
        tick: Tick,
        source: SymbolId,
        delay: u64,
        actions: Vec<Action>,
        action_index: usize,
    ) -> Option<StopReason> {
        if delay > 0 && self.pending_len >= self.limits.max_pending_items {
            return Some(StopReason::PendingLimit {
                at: self.locator(
                    program,
                    source,
                    &[&format!("schedule at action {action_index}")],
                ),
                queued: self.pending_len,
            });
        }
        let Some(ordinal) = self.alloc_ordinal() else {
            return Some(StopReason::SequenceExhausted {
                at: self.locator(
                    program,
                    source,
                    &[&format!("schedule at action {action_index}")],
                ),
            });
        };
        let item = PendingWork {
            source,
            ordinal,
            due: Tick(tick.0.saturating_add(delay)),
            next: 0,
            actions,
        };
        if delay == 0 {
            run.ready.push_back(item);
        } else {
            self.pending.entry(item.due).or_default().push_back(item);
            self.pending_len += 1;
        }
        None
    }

    /// The session-unique ordinal for the next scheduled item. `None` once
    /// the sequence space is exhausted (~66 million items): the check
    /// guarantees every action index still fits in [`item_sequence`].
    fn alloc_ordinal(&mut self) -> Option<u32> {
        let ordinal = self.next_item_ordinal;
        (ordinal + 1)
            .checked_mul(SEQS_PER_ITEM)
            .and_then(|base| base.checked_add(SEQS_PER_ITEM - 1))?;
        self.next_item_ordinal += 1;
        Some(ordinal)
    }

    /// Turns the unexecuted rest of a stopped action list into pending work
    /// due this tick: it drains first on the next tick, so nothing already
    /// executed repeats and nothing not yet executed is skipped. Remainder
    /// items are exempt from the pending cap — their number is bounded by the
    /// work the tick already spent, and dropping one would lose program
    /// instructions.
    fn defer(&mut self, tick: Tick, run: &mut TickRun, source: SymbolId, actions: Vec<Action>) {
        // `alloc_ordinal` failing is a pathological session; the fallback
        // ordinal overflows `item_sequence`, so the item stops its tick with
        // `SequenceExhausted` instead of emitting under a colliding key.
        let ordinal = self.alloc_ordinal().unwrap_or(u32::MAX);
        run.ready.push_back(PendingWork {
            source,
            ordinal,
            due: tick,
            next: 0,
            actions,
        });
    }

    /// Emits one event unless its execution key was already consumed — the
    /// exactly-once guard for save/restore and retry (non-negotiable 3).
    fn emit(&mut self, result: &mut TickResult, key: EventKey, kind: EventKind) {
        if self.consumed.insert(key.execution_key()) {
            result.events.push(MissionEvent { key, kind });
        }
    }

    /// Runs one action of `enclosing` (the objective's list or the pending
    /// item's list — `Reschedule` re-queues whichever list the action lives
    /// in). `key` is the event key this action's emissions use.
    ///
    /// Returns `Some(stop)` when a bound refuses: `PendingLimit` means the
    /// action did not enqueue and must be retried by its owner next tick.
    #[allow(clippy::too_many_arguments)]
    fn run_action(
        &mut self,
        program: &ValidatedProgram,
        action: &Action,
        key: EventKey,
        tick: Tick,
        enclosing: &[Action],
        action_index: usize,
        run: &mut TickRun,
        result: &mut TickResult,
    ) -> Option<StopReason> {
        match action {
            Action::SetVariable { variable, value } => {
                run.writes.push((*variable, value.clone()));
            }
            Action::Draw { variable, min, max } => {
                // Validation proved `min <= max`, so the span fits u64 and
                // the drawn value is in `[min, max]`, hence in `i32`.
                let span = (*max as i64 - *min as i64 + 1) as u64;
                let drawn = (*min as i64 + (self.rng.next_u64() % span) as i64) as i32;
                self.rng_draws += 1;
                run.writes.push((*variable, Value::Int(drawn)));
            }
            Action::Finish(outcome) => {
                run.requested.insert(*outcome);
                self.emit(result, key, EventKind::TerminalRequested(*outcome));
            }
            Action::GrantReward { reward } => {
                self.emit(result, key, EventKind::RewardGranted(reward.clone()));
            }
            Action::Schedule {
                delay_ticks,
                actions,
            } => {
                return self.enqueue(
                    program,
                    run,
                    tick,
                    key.source,
                    *delay_ticks,
                    actions.clone(),
                    action_index,
                );
            }
            Action::Reschedule { delay_ticks } => {
                return self.enqueue(
                    program,
                    run,
                    tick,
                    key.source,
                    *delay_ticks,
                    enclosing.to_vec(),
                    action_index,
                );
            }
            // Validation rejects Unknown; reaching it here is a program that
            // bypassed `validate`, which is impossible through
            // `ValidatedProgram`.
            Action::Unknown { .. } => unreachable!("validated program"),
        }
        None
    }

    /// The contract's budget diagnostic: mission id, the owning symbol and a
    /// short trace of where work stopped.
    fn locator(
        &self,
        program: &ValidatedProgram,
        source: SymbolId,
        trace: &[&str],
    ) -> ProgramLocator {
        ProgramLocator {
            mission: program.program().mission.to_string(),
            objective: Some(source),
            trace: trace.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn holds(&self, condition: &Condition, facts: &MissionFacts) -> bool {
        match condition {
            Condition::Const(b) => *b,
            Condition::ActorIs { actor, state } => facts.actors.get(actor) == Some(state),
            Condition::Not(c) => !self.holds(c, facts),
            Condition::All(cs) => cs.iter().all(|c| self.holds(c, facts)),
            Condition::Any(cs) => cs.iter().any(|c| self.holds(c, facts)),
            Condition::Compare {
                variable,
                op,
                value,
            } => self
                .variables
                .get(variable)
                .is_some_and(|v| compare(v, *op, value)),
            Condition::Unknown { .. } => unreachable!("validated program"),
        }
    }
}

fn compare(left: &Value, op: CompareOp, right: &Value) -> bool {
    use std::cmp::Ordering;
    let ordering = match (left, right) {
        (Value::Int(a), Value::Int(b)) => a.cmp(b),
        (Value::Float(a), Value::Float(b)) => match a.partial_cmp(b) {
            Some(o) => o,
            None => return false,
        },
        _ => {
            return match op {
                CompareOp::Eq => left == right,
                CompareOp::Ne => left != right,
                _ => false,
            };
        }
    };
    match op {
        CompareOp::Eq => ordering == Ordering::Equal,
        CompareOp::Ne => ordering != Ordering::Equal,
        CompareOp::Lt => ordering == Ordering::Less,
        CompareOp::Le => ordering != Ordering::Greater,
        CompareOp::Gt => ordering == Ordering::Greater,
        CompareOp::Ge => ordering != Ordering::Less,
    }
}
