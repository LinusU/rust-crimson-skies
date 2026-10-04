//! The simulation-side mission session and its authoritative host effects
//! (F37-A, F37-B, F37-C).
//!
//! Owns one launched mission: refuses to launch a program that fails
//! validation (an unsupported instruction means the mission is
//! [`TerminalState::Unsupported`], with no progression and no reward), drives
//! [`MissionState`] one integer tick at a time under its [`WorkLimits`], and
//! applies the effects that state asks for through [`HostLedger`].
//!
//! The split is the contract's: `cs_script` decides *what* happened, this
//! module decides *what the world does about it*. The producer is
//! [`MissionState::step`], which emits events carrying exactly-once
//! [`ExecutionKey`]s; the consumer is [`HostLedger::apply`], the only thing in
//! the process that turns a reward intent into an authoritative grant and a
//! resolved outcome into the session's one recorded result.
//!
//! Three rules make the consumer authoritative:
//!
//! - An effect is applied at most once per [`ExecutionKey`], so a replayed
//!   tick, a retry of a refused application or a restored session can never
//!   grant a reward twice (contract: "save/restore and retry cannot repeat
//!   rewards or captures").
//! - A reward the host was not told to apply is **refused**, never silently
//!   accepted: a mission may declare a reward the installation cannot grant,
//!   and an unappliable grant must surface as an error, not as progress.
//! - The recorded outcome is the one the runtime's
//!   [`cs_script::runtime::PrecedencePolicy`] resolved, not one program's
//!   request, and a second different outcome for the same session is refused.
//!   Success cannot coexist with failure.
//!
//! Teardown: once the session is terminal, [`MissionState::teardown`] drops the
//! deferred work queue and [`HostLedger::teardown`] refuses every *newly
//! offered* effect, so a stale result or a late save cannot touch the world
//! after the mission is over. Effects already earned before teardown stay
//! outstanding for [`HostLedger::retry`]; refusing them is the one thing
//! teardown does not do, because an earned reward must not evaporate with the
//! session.
//!
//! The fact side: [`MissionSession::advance`] reads a [`MissionFacts`] the
//! caller builds, but nothing decides what `actors` it holds. That is
//! [`ActorFactTable`] — the writer F39-E7 recorded as missing (its unknown #7).
//! It keeps the same once-per-kind record the damage ledger's lifecycle set
//! keeps: registration admits an actor [`ActorState::Alive`], one
//! [`LifecycleKind`] transition is recorded at most once per actor, and a
//! terminal transition closes the record. What each transition *does* to the
//! row is decided in exactly one place, [`lifecycle_fact_effect`] — measured
//! or refused by name, the way `CountKind::from_lifecycle` is the gate for
//! the counted categories; no state is written because the variant exists.
//! [`MissionSession::advance_observed`] is the wired path: the caller hands
//! one tick's [`ActorFactInput`] — the same `(actor, kind)` surface
//! [`crate::objectives::runtime::TickInput::lifecycles`] carries — and the
//! session folds it, then steps on the populated map.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_script::ir::{
    ActorId, ActorState, MissionProgram, Outcome, ValidatedProgram, ValidationError,
};
use cs_script::runtime::{
    EventKind, ExecutionKey, MissionEvent, MissionFacts, MissionState, MissionStateSnapshot,
    RestoreError, SNAPSHOT_VERSION, SessionGeneration, StopReason, TerminalState, TickError,
    TickResult, WorkLimits,
};
use cs_types::Tick;
use cs_types::content::ContentId;

use crate::damage::LifecycleKind;

/// Most refused effects a host may hold for a retry. A designed bound on the
/// record's size and on the work one save carries, not a measured original
/// limit. Beyond it a refusal is still reported, it is just not queued.
const MAX_OUTSTANDING_EFFECTS: usize = 256;

/// Most applied effects one ledger's save record may hold. A live session
/// cannot reach it — every applied effect cost one work unit, and a tick is
/// bounded — so this only refuses a record that would grow the ledger without
/// bound. A designed bound, not a measured original limit.
const MAX_APPLIED_EFFECTS: usize = 1 << 16;

/// Most reward ids one ledger's save record may declare. Also unreachable for a
/// live ledger that was given a bounded catalog; a designed bound.
const MAX_REWARD_CATALOG: usize = 1 << 12;

/// One launched mission.
#[derive(Debug)]
pub struct MissionSession {
    program: ValidatedProgram,
    state: MissionState,
    host: HostLedger,
    /// The session's actor-fact table: the writer that folds the
    /// simulation's authoritative actor record into the [`MissionFacts`]
    /// `Condition::ActorIs` reads. Empty at launch; actors enter play by
    /// registration.
    facts: ActorFactTable,
}

/// A refused launch: the mission stays Unsupported.
#[derive(Debug, PartialEq, Eq)]
pub struct LaunchRefused {
    pub terminal: TerminalState,
    pub error: ValidationError,
}

impl MissionSession {
    /// Validates and launches `program` with the rewards this host may apply.
    ///
    /// `rewards` is the mission's declared reward catalog: a `GrantReward` for
    /// an id outside it is refused by [`HostLedger::apply`] instead of being
    /// applied. Nothing is inferred from an id's namespace — only what the
    /// caller declared is applicable.
    ///
    /// # Errors
    ///
    /// [`LaunchRefused`] with the precise validation trace.
    pub fn launch(
        program: MissionProgram,
        session: SessionGeneration,
        rewards: impl IntoIterator<Item = ContentId>,
    ) -> Result<Self, LaunchRefused> {
        let program = program.validate().map_err(|error| LaunchRefused {
            terminal: TerminalState::Unsupported,
            error,
        })?;
        let state = MissionState::new(&program, session);
        Ok(Self {
            program,
            host: HostLedger::new(session, rewards),
            state,
            facts: ActorFactTable::new(),
        })
    }

    /// Advances one tick and applies the resulting host effects.
    ///
    /// This is the consumer's path: [`MissionState::step`] is the producer, the
    /// ledger is the consumer, and a session that reaches a terminal state is
    /// torn down before the call returns — deferred work dropped, further
    /// effects refused. The `MissionFacts` are the caller's: a session advanced
    /// on a caller-built map keeps the evaluator's own contract that it never
    /// invents world state. [`Self::advance_observed`] is the path that
    /// populates the map from the session's authoritative [`ActorFactTable`].
    ///
    /// # Errors
    ///
    /// [`TickError`] when the tick does not advance.
    pub fn advance(&mut self, facts: &MissionFacts, tick: Tick) -> Result<MissionTick, TickError> {
        let result = self.state.step(&self.program, facts, tick)?;
        let host = self.host.apply(&result);
        if result.terminal != TerminalState::Running {
            self.finish(tick);
        }
        Ok(MissionTick {
            tick,
            events: result.events,
            stop: result.stop,
            terminal: result.terminal,
            host,
        })
    }

    /// Advances one tick without applying any host effect, for a caller that
    /// only wants to evaluate the program.
    ///
    /// # Errors
    ///
    /// [`TickError`] when the tick does not advance.
    pub fn step(&mut self, facts: &MissionFacts, tick: Tick) -> Result<TickResult, TickError> {
        self.state.step(&self.program, facts, tick)
    }

    /// The session's actor-fact table — the writer that maps the simulation's
    /// authoritative actor state into the [`MissionFacts`] the evaluator
    /// reads (see [`ActorFactTable`]).
    pub fn actor_facts(&self) -> &ActorFactTable {
        &self.facts
    }

    /// The mutable table, for registering actors as they enter play. Actors
    /// may also register through [`ActorFactInput::registered`] on the tick
    /// they enter; the two surfaces reach the same record.
    pub fn actor_facts_mut(&mut self) -> &mut ActorFactTable {
        &mut self.facts
    }

    /// Registers one mission actor that entered play — [`ActorState::Alive`]
    /// traced to its admission event.
    ///
    /// # Errors
    ///
    /// [`FactError::DuplicateActor`] when the actor was already registered:
    /// serials are never recycled inside a session.
    pub fn register_actor(&mut self, actor: ActorId) -> Result<FactObservation, FactError> {
        self.facts.register(actor)
    }

    /// Advances one tick on the session's own facts: `input` is folded into
    /// the [`ActorFactTable`] first — registrations, then transitions,
    /// validated whole — and the evaluator then steps on the populated
    /// [`MissionFacts`]. The caller hands over what the authoritative
    /// simulation recorded this tick, never a hand-built map: this is the
    /// path that makes `Condition::ActorIs` observable.
    ///
    /// A refused fold applies nothing and does not step, so the caller can
    /// fix the producer defect and offer the same tick again. A fold that
    /// succeeds but is followed by a refused tick leaves its records
    /// standing: the table mirrors the authoritative record whether or not
    /// the mission stepped on it, and the next input should not offer
    /// those transitions again.
    ///
    /// # Errors
    ///
    /// [`ObservedError::Facts`] when the input is refused, or
    /// [`ObservedError::Tick`] when the tick does not advance.
    pub fn advance_observed(
        &mut self,
        input: &ActorFactInput<'_>,
        tick: Tick,
    ) -> Result<ObservedTick, ObservedError> {
        let observed = self
            .facts
            .observe_tick(input)
            .map_err(ObservedError::Facts)?;
        let mission = self
            .advance(&self.facts.facts(), tick)
            .map_err(ObservedError::Tick)?;
        Ok(ObservedTick {
            mission,
            facts: observed,
        })
    }

    /// `step`'s observed variant: the tick's actor input folded and the
    /// program evaluated on the populated facts, without host effects.
    ///
    /// # Errors
    ///
    /// [`ObservedError::Facts`] when the input is refused, or
    /// [`ObservedError::Tick`] when the tick does not advance.
    pub fn step_observed(
        &mut self,
        input: &ActorFactInput<'_>,
        tick: Tick,
    ) -> Result<ObservedStep, ObservedError> {
        let observed = self
            .facts
            .observe_tick(input)
            .map_err(ObservedError::Facts)?;
        let result = self
            .state
            .step(&self.program, &self.facts.facts(), tick)
            .map_err(ObservedError::Tick)?;
        Ok(ObservedStep {
            result,
            facts: observed,
        })
    }

    /// Retries every host effect that was refused and is still outstanding.
    ///
    /// The ledger keeps a refused reward intent until it is applied, so a
    /// caller that fixed the cause — by declaring the reward, say — can hand
    /// the same intent back without the program re-emitting it. Applied
    /// effects are skipped by their execution key, so the call is safe to
    /// repeat.
    pub fn retry_host(&mut self) -> HostReport {
        let tick = self.state.last_tick().unwrap_or_default();
        self.host.retry(tick)
    }

    /// Ends the session: further effects are refused and the deferred work
    /// queue is dropped. Idempotent. Returns how many items were dropped.
    pub fn finish(&mut self, tick: Tick) -> usize {
        self.host.teardown(tick);
        self.state.teardown()
    }

    /// Aborts the session at `tick`. The outcome becomes
    /// [`TerminalState::Aborted`] and no further effect is accepted. Idempotent.
    pub fn abort(&mut self, tick: Tick) -> usize {
        self.host.teardown(tick);
        self.state.abort()
    }

    /// Overrides the evaluator's work/queue bounds; the default is
    /// [`WorkLimits::default`].
    pub fn set_limits(&mut self, limits: WorkLimits) {
        self.state.set_limits(limits);
    }

    /// The evaluator state, for inspection.
    pub fn state(&self) -> &MissionState {
        &self.state
    }

    /// The authoritative host record.
    pub fn host(&self) -> &HostLedger {
        &self.host
    }

    /// The host record's mutable view, for declaring rewards and applying
    /// effects a caller produced itself.
    pub fn host_mut(&mut self) -> &mut HostLedger {
        &mut self.host
    }

    /// The session's save record: the evaluator state, the authoritative
    /// host record and the actor-fact table together — a reward the host
    /// already applied must not be applied again after the restore, and a
    /// state an actor already reached must still be what an unfired
    /// `Condition::ActorIs` observes.
    pub fn snapshot(&self) -> MissionSessionSnapshot {
        MissionSessionSnapshot {
            version: SNAPSHOT_VERSION,
            state: self.state.snapshot(&self.program),
            host: self.host.snapshot(),
            facts: self.facts.snapshot(),
        }
    }

    /// Rebuilds a session from a save record.
    ///
    /// # Errors
    ///
    /// [`SessionRestoreError`]: the program must validate (an unvalidated
    /// program may not own restored state), the two records must agree on the
    /// session, and each must be internally consistent.
    pub fn restore(
        program: MissionProgram,
        snapshot: MissionSessionSnapshot,
    ) -> Result<Self, SessionRestoreError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(SessionRestoreError::SnapshotVersion {
                found: snapshot.version,
            });
        }
        let program = program.validate().map_err(SessionRestoreError::Program)?;
        if snapshot.state.session != snapshot.host.session {
            return Err(SessionRestoreError::Host(
                HostRestoreError::SessionMismatch {
                    evaluator: snapshot.state.session,
                    host: snapshot.host.session,
                },
            ));
        }
        let state =
            MissionState::restore(&program, snapshot.state).map_err(SessionRestoreError::State)?;
        let host = HostLedger::restore(snapshot.host).map_err(SessionRestoreError::Host)?;
        let facts = ActorFactTable::restore(snapshot.facts).map_err(SessionRestoreError::Facts)?;
        Ok(Self {
            program,
            state,
            host,
            facts,
        })
    }
}

// ---------------------------------------------------------------------------
// The actor-fact table: `MissionFacts` populated from authoritative state.
// ---------------------------------------------------------------------------

/// The named reason [`ActorState::Disabled`] and [`ActorState::Escaped`]
/// have no producer: no [`LifecycleKind`] transition reports either — the
/// same structural fact
/// [`crate::objectives::counters::CountKind::producer`] answers `None` for,
/// asked from the condition side. The original's own objective records
/// never declare a counted category for either (F39-E4's census), and the
/// mission program that could carry an actor-state report is still
/// undecoded, so nothing measurable writes them. A state is never written
/// because its variant exists.
pub const NO_TRANSITION_WRITES_THIS_STATE: &str = "no_lifecycle_transition_writes_this_actor_state";

/// The named reason [`ActorState::Detached`] has no producer: F39-E7
/// measured over every reader archive in the owner's installation that the
/// original writes a detach as an **event** — `WAKE_ANIM drop_paratroopers`
/// completing `zbd/c2/m05 OBJECTIVE23` — never as a counted category, and
/// no [`LifecycleKind`] transition reports a detach either. The engine
/// represents one as the released payload that keeps its objective
/// ([`crate::world_actors::release::release_payload`], F34 non-negotiable
/// 4); whether that release should also write this state is unmeasured, so
/// the state refuses rather than guess.
pub const DETACHED_IS_AN_EVENT: &str = "detached_is_an_event_not_an_actor_state";

/// The named reason a [`LifecycleKind::PilotBailout`] writes no
/// [`ActorState`]: F29 keeps bailout apart from death — a bailed-out pilot
/// left the airframe without destroying it — and nothing measured maps it
/// to a condition state. It is not the `Escaped` distinction, which has no
/// producer of its own.
pub const PILOT_BAILOUT_WRITES_NO_STATE: &str = "pilot_bailout_writes_no_actor_state";

/// What recording one authoritative [`LifecycleKind`] transition does to an
/// actor's row in the [`ActorFactTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactEffect {
    /// The transition writes this [`ActorState`] — a measured producer.
    Writes(ActorState),
    /// The transition is recorded in the actor's set but writes no state;
    /// the reason names why. The actor's state is unchanged: a bailout
    /// leaves the airframe in play exactly where the damage domain leaves
    /// it ([`PILOT_BAILOUT_WRITES_NO_STATE`]).
    RecordedUnwritten(&'static str),
    /// `LifecycleKind::MissionRemoved`: the actor left mission accounting.
    /// Its row keeps the recorded transition — and stays closed — but the
    /// actor is **absent** from [`ActorFactTable::facts`] from here on, so
    /// no `Condition::ActorIs` observes it again. Absence is the event's
    /// own meaning — "left mission accounting" — not a default state, and
    /// deliberately not `Despawned`: a cinematic removal is not the actor
    /// leaving the world.
    RemovesFromAccounting,
}

/// What one authoritative lifecycle transition does to an actor's fact row
/// — the single place the `LifecycleKind` → `ActorState` mapping is
/// decided, measured or refused by name the way
/// [`crate::objectives::counters::CountKind::from_lifecycle`] is the gate
/// for the counted categories.
///
/// Measured: `Destroyed` writes [`ActorState::Dead`], `OwnershipCaptured`
/// writes [`ActorState::Captured`], `Despawned` writes
/// [`ActorState::Despawned`] — the three transitions F39-E4 measured as the
/// counted categories' producers, asked from the condition side.
///
/// Refused by name: `PilotBailout` records but writes nothing
/// ([`PILOT_BAILOUT_WRITES_NO_STATE`]), and `MissionRemoved` ends the
/// actor's mission accounting rather than writing a state. Neither is
/// defaulted to a nearby meaning: a bailout is not a death and a cinematic
/// removal is not a kill.
#[must_use]
pub const fn lifecycle_fact_effect(kind: LifecycleKind) -> FactEffect {
    match kind {
        LifecycleKind::Destroyed => FactEffect::Writes(ActorState::Dead),
        LifecycleKind::OwnershipCaptured => FactEffect::Writes(ActorState::Captured),
        LifecycleKind::Despawned => FactEffect::Writes(ActorState::Despawned),
        LifecycleKind::PilotBailout => FactEffect::RecordedUnwritten(PILOT_BAILOUT_WRITES_NO_STATE),
        LifecycleKind::MissionRemoved => FactEffect::RemovesFromAccounting,
    }
}

/// The [`ActorState`] one lifecycle transition writes — `None` where
/// [`lifecycle_fact_effect`] records a named refusal or an accounting
/// removal.
#[must_use]
pub const fn actor_state_of(kind: LifecycleKind) -> Option<ActorState> {
    match lifecycle_fact_effect(kind) {
        FactEffect::Writes(state) => Some(state),
        FactEffect::RecordedUnwritten(_) | FactEffect::RemovesFromAccounting => None,
    }
}

/// The authoritative event that writes one [`ActorState`], or the named
/// reason nothing does — the producer question for the whole condition
/// vocabulary, asked state by state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorStateProducer {
    /// Registration — the actor entering play — is the event that writes
    /// `Alive`. Nothing else may: `Alive` is never a default a caller can
    /// reach for.
    Registration,
    /// A lifecycle transition is the measured producer of the state.
    Lifecycle(LifecycleKind),
    /// No event produces the state; the reason names why.
    Unproduced(&'static str),
}

/// The event that writes `state`, or the named reason nothing does.
///
/// Derived from [`lifecycle_fact_effect`] over [`LifecycleKind::ALL`] — the
/// same trick `CountKind::producer` uses — so the answer can never drift
/// from the forward gate.
#[must_use]
pub fn actor_state_producer(state: ActorState) -> ActorStateProducer {
    if state == ActorState::Alive {
        return ActorStateProducer::Registration;
    }
    for kind in LifecycleKind::ALL {
        if let FactEffect::Writes(written) = lifecycle_fact_effect(*kind)
            && written == state
        {
            return ActorStateProducer::Lifecycle(*kind);
        }
    }
    ActorStateProducer::Unproduced(match state {
        ActorState::Detached => DETACHED_IS_AN_EVENT,
        // `Disabled`, `Escaped` and any later variant with no producer.
        _ => NO_TRANSITION_WRITES_THIS_STATE,
    })
}

/// What one observation recorded into an [`ActorFactTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactRecord {
    /// The actor entered play — its admission event.
    Registered,
    /// An authoritative lifecycle transition.
    Lifecycle(LifecycleKind),
}

/// What one accepted observation did to a fact row, in observation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactObservation {
    /// The actor the observation is about — the mission-scoped
    /// `cs_script::ir::ActorId`, `MissionFacts`' own key space.
    pub actor: ActorId,
    /// What was recorded.
    pub recorded: FactRecord,
    /// What it did to the row.
    pub effect: FactEffect,
    /// The actor's [`ActorState`] after the record — `None` once it has
    /// left mission accounting (absent from [`ActorFactTable::facts`]).
    pub state: Option<ActorState>,
}

/// Why an observation was refused — every refusal is a producer defect, so
/// the caller can tell "the transition was offered" from "the row changed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactError {
    /// The actor was already registered — serials are never recycled inside
    /// a session (mirroring `DamageError::DuplicateActor`).
    DuplicateActor {
        /// The actor the record names.
        actor: ActorId,
    },
    /// A transition names an actor the table never registered — the caller
    /// must admit actors as they enter play, the same `UnknownActor`
    /// refusal the damage ledger makes.
    UnknownActor {
        /// The actor the transition names.
        actor: ActorId,
    },
    /// The transition kind is already in the actor's recorded set — a
    /// replay (mirroring `DamageError::DuplicateLifecycle`).
    DuplicateLifecycle {
        /// The actor the record names.
        actor: ActorId,
        /// The kind already recorded.
        kind: LifecycleKind,
    },
    /// The actor's record is closed by a terminal transition — a despawn or
    /// a mission removal already ended it (mirroring
    /// `DamageError::ActorClosed`).
    ActorClosed {
        /// The actor the record names.
        actor: ActorId,
        /// The terminal transition that closed the record.
        terminal: LifecycleKind,
    },
}

impl fmt::Display for FactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateActor { actor } => {
                write!(f, "actor {actor:?} is already registered")
            }
            Self::UnknownActor { actor } => {
                write!(f, "transition names unregistered actor {actor:?}")
            }
            Self::DuplicateLifecycle { actor, kind } => {
                write!(f, "actor {actor:?} already recorded {kind}")
            }
            Self::ActorClosed { actor, terminal } => {
                write!(f, "actor {actor:?} record closed by {terminal}")
            }
        }
    }
}

impl std::error::Error for FactError {}

/// One tick's authoritative actor input: which mission actors entered play
/// and which lifecycle transitions were recorded for them.
///
/// `lifecycles` is the same `(actor, kind)` surface
/// [`crate::objectives::runtime::TickInput::lifecycles`] carries — the host
/// feeds both runtimes one observation. `registered` is the admission
/// event: an actor must enter play here or through
/// [`ActorFactTable::register`] before a transition may name it, so
/// [`ActorState::Alive`] is always traced to the event that caused it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActorFactInput<'a> {
    /// The actors that entered play this tick.
    pub registered: &'a [ActorId],
    /// The lifecycle transitions recorded this tick, in observation order.
    pub lifecycles: &'a [(ActorId, LifecycleKind)],
}

/// The per-session map from mission actors to the [`ActorState`] the
/// simulation's authoritative lifecycle record puts them in — the writer
/// that populates [`MissionFacts::actors`] so `Condition::ActorIs` can
/// observe the contract's distinctions.
///
/// The table keeps the same once-per-kind ledger shape the damage
/// resolver's own record keeps: registration admits an actor `Alive`, one
/// [`LifecycleKind`] per actor is recorded at most once, and a terminal
/// transition closes the record. What a transition *does* is decided in
/// one place, [`lifecycle_fact_effect`] — measured or refused by name, and
/// never because the `ActorState` variant exists.
///
/// Actors are keyed by the mission-scoped [`cs_script::ir::ActorId`] —
/// `MissionFacts`' own key space. The host maps the world's
/// `cs_types::net::ActorId` to it exactly as `TickInput::lifecycles`
/// already does; that identity bridge is the caller's surface, not this
/// table's.
///
/// # Why `NetLifecycle` cannot feed this table
///
/// [`crate::net_state::NetLifecycle`] is the *presentation* summary ("is
/// this actor still here?"): its `Despawned` folds mission removal,
/// capture and unload into one kind — exactly the distinctions the
/// contract demands kept apart. Reading it would write
/// [`ActorState::Despawned`] for a captured actor. The damage lifecycle is
/// the only record that keeps the five transitions separate, so it is the
/// only record this table consumes.
///
/// # The derived state and its designed precedence
///
/// `Condition::ActorIs` asks for the actor's *one* current state, so a row
/// holding several recorded transitions resolves them in a declared order
/// — designed, and deliberately the same question answered the same way
/// [`crate::allies::AlliesRoster::status`] answers it: a terminal
/// transition wins (`MissionRemoved` → absent, `Despawned` →
/// `Despawned`), then `Destroyed` → `Dead`, then `OwnershipCaptured` →
/// `Captured`, otherwise `Alive`. Whether the original's condition would
/// have latched on the earlier state of a same-tick pair is unmeasured —
/// the precedence is the designed reading until it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorFactTable {
    /// Each registered actor's recorded transition set. An absent actor was
    /// never registered; `LifecycleKind::MissionRemoved` in the set means
    /// the row is closed *and* absent from [`Self::facts`].
    actors: BTreeMap<ActorId, BTreeSet<LifecycleKind>>,
}

impl ActorFactTable {
    /// An empty table: no actor has entered play yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The actors the table knows, in id order.
    pub fn actors(&self) -> impl Iterator<Item = ActorId> + '_ {
        self.actors.keys().copied()
    }

    /// Whether the actor entered play — `Alive` or any recorded end.
    #[must_use]
    pub fn is_registered(&self, actor: &ActorId) -> bool {
        self.actors.contains_key(actor)
    }

    /// The transitions recorded for the actor; `None` when unregistered.
    #[must_use]
    pub fn lifecycle(&self, actor: &ActorId) -> Option<&BTreeSet<LifecycleKind>> {
        self.actors.get(actor)
    }

    /// The actor's current [`ActorState`], or `None` when it never entered
    /// play or has left mission accounting. The precedence is the declared
    /// order in the type docs.
    #[must_use]
    pub fn state(&self, actor: &ActorId) -> Option<ActorState> {
        self.actors.get(actor).and_then(Self::state_of)
    }

    /// The one state a recorded set resolves to — the declared precedence:
    /// terminal first, then dead, then captured, else alive.
    fn state_of(set: &BTreeSet<LifecycleKind>) -> Option<ActorState> {
        if set.contains(&LifecycleKind::MissionRemoved) {
            None
        } else if set.contains(&LifecycleKind::Despawned) {
            Some(ActorState::Despawned)
        } else if set.contains(&LifecycleKind::Destroyed) {
            Some(ActorState::Dead)
        } else if set.contains(&LifecycleKind::OwnershipCaptured) {
            Some(ActorState::Captured)
        } else {
            Some(ActorState::Alive)
        }
    }

    /// The [`MissionFacts`] for the next evaluation: every registered actor
    /// still in accounting, at its current state. The whole map is rebuilt
    /// on each call, so what the evaluator reads on a tick is exactly what
    /// the table holds — populated from the authoritative record on every
    /// tick, never accumulated by a caller.
    #[must_use]
    pub fn facts(&self) -> MissionFacts {
        MissionFacts {
            actors: self
                .actors
                .iter()
                .filter_map(|(actor, set)| Self::state_of(set).map(|state| (*actor, state)))
                .collect(),
        }
    }

    /// The admission event: `actor` enters play [`ActorState::Alive`].
    ///
    /// # Errors
    ///
    /// [`FactError::DuplicateActor`] — serials are never recycled inside a
    /// session.
    pub fn register(&mut self, actor: ActorId) -> Result<FactObservation, FactError> {
        if self.actors.contains_key(&actor) {
            return Err(FactError::DuplicateActor { actor });
        }
        self.actors.insert(actor, BTreeSet::new());
        Ok(FactObservation {
            actor,
            recorded: FactRecord::Registered,
            effect: FactEffect::Writes(ActorState::Alive),
            state: Some(ActorState::Alive),
        })
    }

    /// One authoritative lifecycle transition for `actor`.
    ///
    /// # Errors
    ///
    /// [`FactError::UnknownActor`], [`FactError::ActorClosed`] or
    /// [`FactError::DuplicateLifecycle`] — the same three refusals the
    /// damage ledger's `record_lifecycle` makes, so a replay or a late
    /// event can never rewrite what the mission already observed.
    pub fn observe(
        &mut self,
        actor: ActorId,
        kind: LifecycleKind,
    ) -> Result<FactObservation, FactError> {
        let set = self
            .actors
            .get(&actor)
            .ok_or(FactError::UnknownActor { actor })?;
        Self::check(set, actor, kind)?;
        Ok(self.record(actor, kind))
    }

    /// One tick's authoritative input, validated whole before anything is
    /// recorded — a refused input records nothing at all, the same atomic
    /// rule `ObjectiveRuntime` applies to a refused movement, so the caller
    /// may fix the defect and offer the same tick again.
    ///
    /// Registrations are validated first; an actor registered *and*
    /// transitioned in the same input is legal, and a transition offered
    /// after a terminal one — in the record or earlier in the same input —
    /// refuses the whole input.
    ///
    /// # Errors
    ///
    /// [`FactError`] — the first defect the input carries.
    pub fn observe_tick(
        &mut self,
        input: &ActorFactInput<'_>,
    ) -> Result<Vec<FactObservation>, FactError> {
        // Validate the whole input before recording any of it.
        let mut registered = BTreeSet::new();
        for actor in input.registered {
            if self.actors.contains_key(actor) || !registered.insert(*actor) {
                return Err(FactError::DuplicateActor { actor: *actor });
            }
        }
        // Transitions offered by this input, per actor, plus the terminal
        // each has already reached inside it.
        let mut offered: BTreeMap<ActorId, BTreeSet<LifecycleKind>> = BTreeMap::new();
        let mut closed: BTreeMap<ActorId, LifecycleKind> = BTreeMap::new();
        for (actor, kind) in input.lifecycles {
            if let Some(terminal) = closed.get(actor) {
                return Err(FactError::ActorClosed {
                    actor: *actor,
                    terminal: *terminal,
                });
            }
            match self.actors.get(actor) {
                Some(set) => Self::check(set, *actor, *kind)?,
                // An actor this same input registers has an empty record so
                // far; anything else is a producer defect.
                None if !registered.contains(actor) => {
                    return Err(FactError::UnknownActor { actor: *actor });
                }
                None => {}
            }
            if !offered.entry(*actor).or_default().insert(*kind) {
                return Err(FactError::DuplicateLifecycle {
                    actor: *actor,
                    kind: *kind,
                });
            }
            if kind.is_terminal() {
                closed.insert(*actor, *kind);
            }
        }
        // Then record in input order: admissions before transitions.
        let mut out = Vec::with_capacity(input.registered.len() + input.lifecycles.len());
        for actor in input.registered {
            out.push(self.register(*actor).expect("the input was validated"));
        }
        for (actor, kind) in input.lifecycles {
            out.push(self.record(*actor, *kind));
        }
        Ok(out)
    }

    /// The refusals [`Self::observe`] and [`Self::observe_tick`] share for a
    /// stored record: closed first, then the replay — the damage ledger's
    /// own order.
    fn check(
        set: &BTreeSet<LifecycleKind>,
        actor: ActorId,
        kind: LifecycleKind,
    ) -> Result<(), FactError> {
        if let Some(terminal) = set.iter().find(|kind| kind.is_terminal()) {
            return Err(FactError::ActorClosed {
                actor,
                terminal: *terminal,
            });
        }
        if set.contains(&kind) {
            return Err(FactError::DuplicateLifecycle { actor, kind });
        }
        Ok(())
    }

    /// Inserts `kind` and reports what it did. Callers validate first.
    fn record(&mut self, actor: ActorId, kind: LifecycleKind) -> FactObservation {
        let set = self
            .actors
            .get_mut(&actor)
            .expect("the input was validated");
        set.insert(kind);
        FactObservation {
            actor,
            recorded: FactRecord::Lifecycle(kind),
            effect: lifecycle_fact_effect(kind),
            state: Self::state_of(set),
        }
    }

    /// The table's save record.
    #[must_use]
    pub fn snapshot(&self) -> ActorFactSnapshot {
        ActorFactSnapshot {
            version: FACT_SNAPSHOT_VERSION,
            actors: self
                .actors
                .iter()
                .map(|(actor, set)| (*actor, set.iter().copied().collect()))
                .collect(),
        }
    }

    /// Rebuilds a table from a save record.
    ///
    /// # Errors
    ///
    /// [`FactRestoreError`]: the record is checked rather than trusted —
    /// version, one row per actor, and no set holding two terminal kinds,
    /// which the live path can never produce.
    pub fn restore(snapshot: ActorFactSnapshot) -> Result<Self, FactRestoreError> {
        if snapshot.version != FACT_SNAPSHOT_VERSION {
            return Err(FactRestoreError::SnapshotVersion {
                found: snapshot.version,
            });
        }
        let mut actors = BTreeMap::new();
        for (actor, kinds) in snapshot.actors {
            let set: BTreeSet<LifecycleKind> = kinds.into_iter().collect();
            let mut terminals = set.iter().filter(|kind| kind.is_terminal());
            if let (Some(first), Some(second)) = (terminals.next(), terminals.next()) {
                return Err(FactRestoreError::ConflictingTerminal {
                    actor,
                    first: *first,
                    second: *second,
                });
            }
            if actors.insert(actor, set).is_some() {
                return Err(FactRestoreError::DuplicateActor { actor });
            }
        }
        Ok(Self { actors })
    }
}

/// Version of the [`ActorFactSnapshot`] record this crate writes — the fact
/// table's own epoch, separate from the evaluator record's
/// `SNAPSHOT_VERSION`, so an older fact record is refused rather than
/// reinterpreted.
pub const FACT_SNAPSHOT_VERSION: u32 = 1;

/// The fact table's save record: each registered actor's recorded
/// transitions. The table decides pending `Condition::ActorIs` reads, so it
/// is gameplay-relevant and crosses the save beside the evaluator and host
/// records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorFactSnapshot {
    pub version: u32,
    /// Each registered actor's recorded transitions, in actor then kind
    /// order.
    pub actors: Vec<(ActorId, Vec<LifecycleKind>)>,
}

/// Why a fact-table save record was refused: the defect it carries, named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactRestoreError {
    /// The record's version is not the one this build writes.
    SnapshotVersion {
        /// The version the record claims.
        found: u32,
    },
    /// An actor is listed twice.
    DuplicateActor {
        /// The actor listed twice.
        actor: ActorId,
    },
    /// A record's set holds two terminal kinds — unreachable through the
    /// live path (the first terminal closes the record), so the record is
    /// corrupt.
    ConflictingTerminal {
        /// The actor the record names.
        actor: ActorId,
        /// The first terminal kind in the set.
        first: LifecycleKind,
        /// The second terminal kind in the set.
        second: LifecycleKind,
    },
}

impl fmt::Display for FactRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotVersion { found } => {
                write!(f, "actor-fact snapshot version {found} is not supported")
            }
            Self::DuplicateActor { actor } => {
                write!(f, "actor {actor:?} listed twice in the record")
            }
            Self::ConflictingTerminal {
                actor,
                first,
                second,
            } => {
                write!(
                    f,
                    "actor {actor:?} record holds two terminal kinds: {first} and {second}"
                )
            }
        }
    }
}

impl std::error::Error for FactRestoreError {}

/// One tick stepped through the session's fact table: what the input did to
/// the table (in observation order) and the mission tick it produced.
#[derive(Debug, PartialEq)]
pub struct ObservedTick {
    /// The mission tick [`MissionSession::advance`] produced.
    pub mission: MissionTick,
    /// What each record in the input did — the writes, the named refusals
    /// and the accounting removals.
    pub facts: Vec<FactObservation>,
}

/// The evaluation-only half of [`ObservedTick`]: no host effects applied.
#[derive(Debug, PartialEq)]
pub struct ObservedStep {
    /// The tick result [`MissionSession::step`] produced.
    pub result: TickResult,
    /// What each record in the input did.
    pub facts: Vec<FactObservation>,
}

/// Why an observed tick was refused.
#[derive(Debug)]
pub enum ObservedError {
    /// The actor input was a producer defect: the whole fold was refused,
    /// nothing was recorded and the tick did not run.
    Facts(FactError),
    /// The tick did not advance.
    Tick(TickError),
}

impl fmt::Display for ObservedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Facts(error) => write!(f, "actor facts: {error}"),
            Self::Tick(error) => write!(f, "tick: {error:?}"),
        }
    }
}

impl std::error::Error for ObservedError {}

/// One evaluated tick: what the program did and what the host did about it.
#[derive(Debug, PartialEq)]
pub struct MissionTick {
    pub tick: Tick,
    /// Ordered by [`cs_script::runtime::EventKey`].
    pub events: Vec<MissionEvent>,
    /// Set when a bound stopped the tick early; the mission keeps running.
    pub stop: Option<StopReason>,
    pub terminal: TerminalState,
    /// What the host applied or refused for this tick.
    pub host: HostReport,
}

/// A host effect the program asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEffect {
    /// A reward the host applies on the mission's behalf. The ledger records
    /// the grant; the owner of the player's profile does the rest.
    Reward { reward: ContentId },
    /// The program asked for a terminal outcome. The authoritative outcome is
    /// the one the runtime's precedence policy resolved, not this request: two
    /// objectives may request conflicting outcomes on one tick and only one of
    /// them can win.
    TerminalRequested { requested: Outcome },
}

/// Why the host did not apply an effect.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostFault {
    /// The reward is not in the host's declared catalog. The effect stays
    /// outstanding for [`HostLedger::retry`] and the caller is told, because a
    /// reward intent the host cannot grant is a defect, not a no-op.
    UnknownReward { reward: ContentId },
    /// The session is over: its deferred work was dropped and its outcome is
    /// authoritative. A late or replayed effect must not reach the world.
    AfterTeardown,
    /// A second, different terminal outcome for a session that already resolved
    /// one.
    OutcomeConflict {
        settled: TerminalState,
        offered: TerminalState,
    },
    /// The result is another session generation's — either its own
    /// [`TickResult::session`] stamp or an event's execution key names a
    /// session that is not this ledger's. A stale replay of an earlier run of
    /// the same mission, or a crossed record. Refused whole — its effects and
    /// its outcome claim together — because the ledger cannot tell which part
    /// of it belongs to this session. Applying it would grant a reward this
    /// session never earned, settle this session on a stale outcome claim, or
    /// write a foreign key into the record, which is a record nothing can ever
    /// restore.
    ForeignSession { session: SessionGeneration },
}

impl fmt::Display for HostFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownReward { reward } => {
                write!(f, "reward {reward} is not in the declared host catalog")
            }
            Self::AfterTeardown => write!(f, "the session is torn down"),
            Self::OutcomeConflict { settled, offered } => {
                write!(
                    f,
                    "outcome already settled as {settled:?}, refused {offered:?}"
                )
            }
            Self::ForeignSession { session } => {
                write!(f, "result belongs to session {}", session.0)
            }
        }
    }
}

/// One effect's fate, in event-key order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostOutcome {
    /// The host applied it; the execution key is now spent.
    Applied {
        key: ExecutionKey,
        effect: HostEffect,
    },
    /// The host refused the effect of one event.
    Refused { key: ExecutionKey, fault: HostFault },
    /// The host refused something the tick itself asserts rather than one
    /// event: a resolved outcome that contradicts the record.
    SessionRefused { fault: HostFault },
}

/// What applying a tick's events did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostReport {
    /// The tick the report belongs to.
    pub tick: Tick,
    pub outcomes: Vec<HostOutcome>,
    /// Rewards this report applied, in application order.
    pub rewards_granted: Vec<ContentId>,
    /// Faults this report raised, in the order they were met.
    pub faults: Vec<HostFault>,
}

impl HostReport {
    fn new(tick: Tick) -> Self {
        Self {
            tick,
            outcomes: Vec::new(),
            rewards_granted: Vec::new(),
            faults: Vec::new(),
        }
    }

    fn record(&mut self, outcome: HostOutcome) {
        match &outcome {
            HostOutcome::Applied {
                effect: HostEffect::Reward { reward },
                ..
            } => self.rewards_granted.push(reward.clone()),
            HostOutcome::Refused { fault, .. } | HostOutcome::SessionRefused { fault } => {
                self.faults.push(fault.clone())
            }
            HostOutcome::Applied {
                effect: HostEffect::TerminalRequested { .. },
                ..
            } => {}
        }
        self.outcomes.push(outcome);
    }
}

/// The authoritative record of what the world was told about one mission.
///
/// It owns the exactly-once ledger over [`ExecutionKey`]s, the catalog of
/// rewards it may apply, the session's one resolved outcome and its outstanding
/// refusals. Nothing else in the process may grant a mission reward: the
/// program states an intent, this ledger is the authority that honours it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostLedger {
    session: SessionGeneration,
    rewards: BTreeSet<ContentId>,
    applied: BTreeMap<ExecutionKey, HostEffect>,
    /// Refused effects a retry could still apply, in the order they arrived.
    outstanding: Vec<(ExecutionKey, HostEffect)>,
    settled: Option<(Tick, TerminalState)>,
    torn_down: Option<Tick>,
}

impl HostLedger {
    /// A ledger for `session` whose catalog is `rewards`.
    pub fn new(session: SessionGeneration, rewards: impl IntoIterator<Item = ContentId>) -> Self {
        Self {
            session,
            rewards: rewards.into_iter().collect(),
            applied: BTreeMap::new(),
            outstanding: Vec::new(),
            settled: None,
            torn_down: None,
        }
    }

    /// Declares a reward this host may apply, reporting whether it was new.
    /// Grants already applied are unaffected: only effects that have not been
    /// applied yet can newly succeed.
    pub fn declare_reward(&mut self, reward: ContentId) -> bool {
        self.rewards.insert(reward)
    }

    /// Is this reward in the catalog?
    pub fn can_apply(&self, reward: &ContentId) -> bool {
        self.rewards.contains(reward)
    }

    /// The effects already applied, in execution-key order.
    pub fn applied(&self) -> impl Iterator<Item = (&ExecutionKey, &HostEffect)> {
        self.applied.iter()
    }

    /// How many reward grants this host has applied.
    pub fn granted_rewards(&self) -> usize {
        self.applied
            .values()
            .filter(|e| matches!(e, HostEffect::Reward { .. }))
            .count()
    }

    /// Refused effects a retry could still apply.
    pub fn outstanding(&self) -> usize {
        self.outstanding.len()
    }

    /// The session's resolved outcome and the tick it resolved on.
    pub fn settled(&self) -> Option<(Tick, TerminalState)> {
        self.settled
    }

    /// The tick the session was torn down on, if it was.
    pub fn torn_down(&self) -> Option<Tick> {
        self.torn_down
    }

    /// Applies one tick's events, in event-key order.
    ///
    /// The order is recomputed from the keys rather than taken from the order the
    /// caller handed the events over in, so `HostReport`'s claim of key order is
    /// true of this report and not of the input. An event whose execution key is
    /// already spent is skipped: that is the replay and restore guard.
    /// `ObjectiveCompleted` is a mission-state observation rather than a host
    /// effect and is skipped too.
    ///
    /// A result that is not this session's — its [`TickResult::session`] stamp
    /// or an event's execution key names another generation — is refused whole,
    /// before any of it is applied: see [`HostFault::ForeignSession`]. The
    /// stamp is checked first, so a result with no events still carries its
    /// provenance and cannot settle this session.
    pub fn apply(&mut self, result: &TickResult) -> HostReport {
        let mut report = HostReport::new(result.tick);
        if result.session != self.session {
            report.record(HostOutcome::SessionRefused {
                fault: HostFault::ForeignSession {
                    session: result.session,
                },
            });
            return report;
        }
        if let Some(session) = result
            .events
            .iter()
            .find(|event| event.key.session != self.session)
            .map(|event| event.key.session)
        {
            report.record(HostOutcome::SessionRefused {
                fault: HostFault::ForeignSession { session },
            });
            return report;
        }
        let mut order: Vec<usize> = (0..result.events.len()).collect();
        order.sort_by_key(|index| result.events[*index].key);
        let torn_down = self.torn_down.is_some();
        let settled = self.settled;
        for index in order {
            let event = &result.events[index];
            let effect = match &event.kind {
                EventKind::ObjectiveCompleted => continue,
                EventKind::RewardGranted(reward) => HostEffect::Reward {
                    reward: reward.clone(),
                },
                EventKind::TerminalRequested(requested) => HostEffect::TerminalRequested {
                    requested: *requested,
                },
            };
            let key = event.key.execution_key();
            if self.applied.contains_key(&key) {
                // Exactly-once: this effect is already part of the record.
                continue;
            }
            if self.outstanding.iter().any(|(held, _)| *held == key) {
                // Already refused and held for a retry: its fate is decided, so
                // offering it again changes nothing and reports nothing.
                continue;
            }
            if torn_down {
                report.record(HostOutcome::Refused {
                    key,
                    fault: HostFault::AfterTeardown,
                });
                continue;
            }
            if let HostEffect::Reward { reward } = &effect
                && !self.rewards.contains(reward)
            {
                report.record(HostOutcome::Refused {
                    key,
                    fault: HostFault::UnknownReward {
                        reward: reward.clone(),
                    },
                });
                self.hold_for_retry(key, effect);
                continue;
            }
            if let HostEffect::TerminalRequested { requested } = &effect {
                let offered = TerminalState::from(*requested);
                if let Some((_, already)) = settled
                    && already != offered
                {
                    report.record(HostOutcome::Refused {
                        key,
                        fault: HostFault::OutcomeConflict {
                            settled: already,
                            offered,
                        },
                    });
                    continue;
                }
            }
            self.applied.insert(key, effect.clone());
            report.record(HostOutcome::Applied { key, effect });
        }
        // The resolved outcome is the authoritative one; a request that lost
        // the precedence policy stays a request.
        if !torn_down && result.terminal != TerminalState::Running {
            match settled {
                Some((_, already)) if already != result.terminal => {
                    report.record(HostOutcome::SessionRefused {
                        fault: HostFault::OutcomeConflict {
                            settled: already,
                            offered: result.terminal,
                        },
                    });
                }
                Some(_) => {}
                None => self.settled = Some((result.tick, result.terminal)),
            }
        }
        report
    }

    /// Queues a refused effect for a later retry, within the queue's bound and
    /// at most once per execution key.
    fn hold_for_retry(&mut self, key: ExecutionKey, effect: HostEffect) {
        if self.outstanding.len() >= MAX_OUTSTANDING_EFFECTS
            || self.outstanding.iter().any(|(held, _)| *held == key)
        {
            return;
        }
        self.outstanding.push((key, effect));
    }

    /// Re-applies every outstanding refusal, in execution-key order.
    ///
    /// Effects that are already applied, or whose reward is still not in the
    /// catalog, are skipped; a still-unknown reward stays outstanding and is
    /// reported again. Teardown does not stop this: an effect the program
    /// earned before the session ended is still that session's to hand over.
    pub fn retry(&mut self, tick: Tick) -> HostReport {
        let mut report = HostReport::new(tick);
        // Sorted by execution key so the report's order is the order it claims,
        // while a refusal that stays outstanding is handed back in the order it
        // arrived, which is the order the record holds.
        let mut held: Vec<_> = std::mem::take(&mut self.outstanding)
            .into_iter()
            .enumerate()
            .collect();
        held.sort_by_key(|(_, (key, _))| *key);
        let mut still: Vec<(usize, (ExecutionKey, HostEffect))> = Vec::new();
        for (arrival, (key, effect)) in held {
            let HostEffect::Reward { reward } = &effect else {
                // Only a refused reward is ever held: a terminal request has no
                // retry, and the outcome it lost was never owed anything.
                continue;
            };
            if self.applied.contains_key(&key) {
                continue;
            }
            if !self.rewards.contains(reward) {
                report.record(HostOutcome::Refused {
                    key,
                    fault: HostFault::UnknownReward {
                        reward: reward.clone(),
                    },
                });
                still.push((arrival, (key, effect)));
                continue;
            }
            self.applied.insert(key, effect.clone());
            report.record(HostOutcome::Applied { key, effect });
        }
        still.sort_by_key(|(arrival, _)| *arrival);
        self.outstanding = still.into_iter().map(|(_, held)| held).collect();
        report
    }

    /// Refuses every effect offered from this tick on. Idempotent; the first
    /// teardown tick is the one recorded.
    pub fn teardown(&mut self, tick: Tick) {
        if self.torn_down.is_none() {
            self.torn_down = Some(tick);
        }
    }

    /// The ledger's save record.
    pub fn snapshot(&self) -> HostLedgerSnapshot {
        HostLedgerSnapshot {
            version: SNAPSHOT_VERSION,
            session: self.session,
            rewards: self.rewards.iter().cloned().collect(),
            applied: self
                .applied
                .iter()
                .map(|(key, effect)| (*key, effect.clone()))
                .collect(),
            outstanding: self.outstanding.clone(),
            settled: self.settled,
            torn_down: self.torn_down,
        }
    }

    /// Rebuilds a ledger from a save record.
    ///
    /// # Errors
    ///
    /// [`HostRestoreError`] when the record is from another snapshot version,
    /// carries another session's execution keys, or is otherwise inconsistent.
    pub fn restore(snapshot: HostLedgerSnapshot) -> Result<Self, HostRestoreError> {
        if snapshot.version != SNAPSHOT_VERSION {
            return Err(HostRestoreError::SnapshotVersion {
                found: snapshot.version,
            });
        }
        if snapshot.outstanding.len() > MAX_OUTSTANDING_EFFECTS {
            return Err(HostRestoreError::TooManyOutstanding {
                count: snapshot.outstanding.len(),
            });
        }
        if snapshot.applied.len() > MAX_APPLIED_EFFECTS {
            return Err(HostRestoreError::TooManyApplied {
                count: snapshot.applied.len(),
            });
        }
        if snapshot.rewards.len() > MAX_REWARD_CATALOG {
            return Err(HostRestoreError::RewardCatalogTooLong {
                count: snapshot.rewards.len(),
            });
        }
        if let Some((tick, TerminalState::Running)) = snapshot.settled {
            return Err(HostRestoreError::SettledWhileRunning { tick });
        }
        let rewards: BTreeSet<_> = snapshot.rewards.iter().cloned().collect();
        let mut applied: BTreeMap<_, _> = BTreeMap::new();
        for (key, effect) in &snapshot.applied {
            if key.session != snapshot.session {
                return Err(HostRestoreError::ForeignExecutionKey { key: *key });
            }
            if applied.insert(*key, effect.clone()).is_some() {
                return Err(HostRestoreError::DuplicateExecutionKey { key: *key });
            }
        }
        let mut outstanding = Vec::with_capacity(snapshot.outstanding.len());
        for (key, effect) in &snapshot.outstanding {
            if key.session != snapshot.session {
                return Err(HostRestoreError::ForeignExecutionKey { key: *key });
            }
            if applied.contains_key(key) {
                // Both applied and outstanding: a retry could grant it twice.
                return Err(HostRestoreError::OutstandingAlreadyApplied { key: *key });
            }
            outstanding.push((*key, effect.clone()));
        }
        Ok(Self {
            session: snapshot.session,
            rewards,
            applied,
            outstanding,
            settled: snapshot.settled,
            torn_down: snapshot.torn_down,
        })
    }
}

/// The host ledger's save record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostLedgerSnapshot {
    pub version: u32,
    pub session: SessionGeneration,
    /// The catalog this host may apply, in id order.
    pub rewards: Vec<ContentId>,
    /// Applied effects in execution-key order.
    pub applied: Vec<(ExecutionKey, HostEffect)>,
    /// Refused effects a retry could still apply, in the order they arrived.
    pub outstanding: Vec<(ExecutionKey, HostEffect)>,
    pub settled: Option<(Tick, TerminalState)>,
    pub torn_down: Option<Tick>,
}

/// Why a host record could not be restored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostRestoreError {
    SnapshotVersion {
        found: u32,
    },
    /// The record and the evaluator record disagree about which session this
    /// is: applying one session's keys against another's state would let a
    /// reward through that this session never earned.
    SessionMismatch {
        evaluator: SessionGeneration,
        host: SessionGeneration,
    },
    ForeignExecutionKey {
        key: ExecutionKey,
    },
    DuplicateExecutionKey {
        key: ExecutionKey,
    },
    /// An effect is recorded as applied *and* outstanding, so a retry could
    /// grant it a second time.
    OutstandingAlreadyApplied {
        key: ExecutionKey,
    },
    TooManyOutstanding {
        count: usize,
    },
    /// The record claims more effects applied than any bounded session could
    /// have applied, so it would grow the ledger without bound.
    TooManyApplied {
        count: usize,
    },
    /// The record declares a reward catalog past the bound.
    RewardCatalogTooLong {
        count: usize,
    },
    /// The record claims the session settled while it was still `Running`. A
    /// live ledger settles only on a terminal outcome, so this record would make
    /// every later outcome a contradiction and leave the session unable to
    /// settle at all.
    SettledWhileRunning {
        tick: Tick,
    },
}

impl fmt::Display for HostRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotVersion { found } => write!(
                f,
                "host snapshot version {found} unsupported (expected {SNAPSHOT_VERSION})"
            ),
            Self::SessionMismatch { evaluator, host } => write!(
                f,
                "host record is session {} but the evaluator record is session {}",
                host.0, evaluator.0
            ),
            Self::ForeignExecutionKey { key } => write!(
                f,
                "host record holds an execution key from session {}",
                key.session.0
            ),
            Self::DuplicateExecutionKey { key } => write!(
                f,
                "host record holds session {} source {} sequence {} twice",
                key.session.0, key.source.0, key.sequence
            ),
            Self::OutstandingAlreadyApplied { key } => write!(
                f,
                "host record holds session {} source {} sequence {} as applied and outstanding",
                key.session.0, key.source.0, key.sequence
            ),
            Self::TooManyOutstanding { count } => write!(
                f,
                "{count} outstanding effects exceed the bound {MAX_OUTSTANDING_EFFECTS}"
            ),
            Self::TooManyApplied { count } => write!(
                f,
                "{count} applied effects exceed the bound {MAX_APPLIED_EFFECTS}"
            ),
            Self::RewardCatalogTooLong { count } => write!(
                f,
                "{count} catalog rewards exceed the bound {MAX_REWARD_CATALOG}"
            ),
            Self::SettledWhileRunning { tick } => write!(
                f,
                "host record claims the session settled as Running on tick {}",
                tick.0
            ),
        }
    }
}

impl std::error::Error for HostRestoreError {}

/// One mission session's save record.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionSessionSnapshot {
    pub version: u32,
    pub state: MissionStateSnapshot,
    pub host: HostLedgerSnapshot,
    /// The actor-fact table's record: which `ActorState` an unfired
    /// `Condition::ActorIs` would observe is gameplay-relevant, so the
    /// table crosses the save beside the two other records.
    pub facts: ActorFactSnapshot,
}

/// Why a session could not be restored.
#[derive(Debug)]
pub enum SessionRestoreError {
    SnapshotVersion {
        found: u32,
    },
    /// The program does not validate, so it may not own restored state.
    Program(ValidationError),
    State(RestoreError),
    Host(HostRestoreError),
    /// The actor-fact record is checked rather than trusted.
    Facts(FactRestoreError),
}

impl fmt::Display for SessionRestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotVersion { found } => write!(
                f,
                "session snapshot version {found} unsupported (expected {SNAPSHOT_VERSION})"
            ),
            Self::Program(e) => write!(f, "program refused: {e}"),
            Self::State(e) => write!(f, "evaluator record refused: {e}"),
            Self::Host(e) => write!(f, "host record refused: {e}"),
            Self::Facts(e) => write!(f, "actor-fact record refused: {e}"),
        }
    }
}

impl std::error::Error for SessionRestoreError {}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_script::ir::{
        Action, CompareOp, Condition, IR_VERSION, Objective, SymbolId, Value, Variable,
    };
    use cs_types::content::{ContentId, ContentKind};

    /// The session generation every fixture here runs under.
    const SESSION: SessionGeneration = SessionGeneration(4);
    /// The later tick every delayed item fires on.
    const LATER_TICK: u64 = 4;

    fn cid(kind: ContentKind, key: &str) -> ContentId {
        ContentId::from_source(kind, key).unwrap()
    }

    fn reward(key: &str) -> Action {
        Action::GrantReward {
            reward: cid(ContentKind::Blueprint, key),
        }
    }

    fn delay(ticks: u64, actions: Vec<Action>) -> Action {
        Action::Schedule {
            delay_ticks: ticks,
            actions,
        }
    }

    fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
        Objective {
            id: SymbolId(id),
            content: cid(ContentKind::Objective, &format!("synthetic-obj-{id}")),
            condition,
            actions,
            span: None,
        }
    }

    fn program(objectives: Vec<Objective>) -> MissionProgram {
        MissionProgram {
            version: IR_VERSION,
            mission: cid(ContentKind::Mission, "synthetic-f37c"),
            variables: vec![],
            objectives,
        }
    }

    /// A program with one latch-gated variable so a later objective cannot fire
    /// on the same tick as the first.
    fn phased_program() -> MissionProgram {
        MissionProgram {
            version: IR_VERSION,
            mission: cid(ContentKind::Mission, "synthetic-f37c"),
            variables: vec![Variable {
                id: SymbolId(100),
                name: "phase".into(),
                initial: Value::Int(0),
            }],
            objectives: vec![
                objective(
                    1,
                    Condition::Const(true),
                    vec![
                        reward("r-now"),
                        // The variable moves when the delayed item runs, so the
                        // mission ends on the tick after it.
                        delay(
                            LATER_TICK - 1,
                            vec![
                                Action::SetVariable {
                                    variable: SymbolId(100),
                                    value: Value::Int(1),
                                },
                                reward("r-later"),
                            ],
                        ),
                    ],
                ),
                objective(
                    2,
                    Condition::Compare {
                        variable: SymbolId(100),
                        op: CompareOp::Ge,
                        value: Value::Int(1),
                    },
                    vec![Action::Finish(Outcome::Succeeded)],
                ),
            ],
        }
    }

    fn session(objectives: Vec<Objective>, rewards: Vec<ContentId>) -> MissionSession {
        MissionSession::launch(program(objectives), SESSION, rewards).unwrap()
    }

    fn facts() -> MissionFacts {
        MissionFacts::default()
    }

    fn reward_ids(report: &HostReport) -> Vec<ContentId> {
        report.rewards_granted.clone()
    }

    fn terminal_requests(report: &HostReport) -> Vec<Outcome> {
        report
            .outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                HostOutcome::Applied {
                    effect: HostEffect::TerminalRequested { requested },
                    ..
                } => Some(*requested),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn accept_f37_a_session_refuses_unknown_instruction_and_runs_valid_program() {
        let bad = program(vec![objective(
            1,
            Condition::Const(true),
            vec![Action::Unknown {
                instruction: "op".into(),
            }],
        )]);
        let refused = MissionSession::launch(bad, SESSION, []).unwrap_err();
        assert_eq!(refused.terminal, TerminalState::Unsupported);

        let ok = program(vec![objective(
            1,
            Condition::Const(true),
            vec![Action::Finish(Outcome::Succeeded)],
        )]);
        let mut s = MissionSession::launch(ok, SESSION, []).unwrap();
        let r = s.step(&facts(), Tick(1)).unwrap();
        assert_eq!(r.terminal, TerminalState::Succeeded);
        assert_eq!(s.state().terminal(), TerminalState::Succeeded);
    }

    #[test]
    fn accept_f37_b_session_self_schedule_stops_on_budget_not_hang() {
        use cs_script::runtime::StopReason;
        // A zero-delay `Reschedule` re-queues its list forever; the session's
        // work budget is what bounds it.
        let looping = program(vec![objective(
            1,
            Condition::Const(true),
            vec![Action::Reschedule { delay_ticks: 0 }],
        )]);
        let mut s = MissionSession::launch(looping, SESSION, []).unwrap();
        s.set_limits(WorkLimits {
            max_work_per_tick: 8,
            ..WorkLimits::default()
        });
        let r = s.step(&facts(), Tick(1)).unwrap();
        assert!(matches!(r.stop, Some(StopReason::WorkBudget { .. })));
        assert_eq!(s.state().terminal(), TerminalState::Running);
        assert_eq!(s.state().queued_items(), 1);
    }

    /// The wired path: a declared reward reaches the host exactly once, an
    /// undeclared one is refused with a fault instead of being honoured, and
    /// the refusal survives until the cause is fixed.
    #[test]
    fn accept_f37_c_session_applies_a_declared_reward_once_and_retries_a_refused_one() {
        let declared = cid(ContentKind::Blueprint, "r-declared");
        let undeclared = cid(ContentKind::Blueprint, "r-undeclared");
        let mut s = session(
            vec![objective(
                1,
                Condition::Const(true),
                vec![reward("r-declared"), reward("r-undeclared")],
            )],
            vec![declared.clone()],
        );

        // The producer, so the consumer can also be exercised on its own terms.
        let result = s.step(&facts(), Tick(1)).unwrap();
        let report = s.host_mut().apply(&result);
        assert_eq!(reward_ids(&report), vec![declared.clone()]);
        assert_eq!(
            report.faults,
            [HostFault::UnknownReward {
                reward: undeclared.clone()
            }]
        );
        assert_eq!(s.host().granted_rewards(), 1);
        assert_eq!(s.host().outstanding(), 1);
        assert!(!s.host().can_apply(&undeclared));

        // A replay of the same tick grants nothing twice.
        let replay = s.host_mut().apply(&result);
        assert!(replay.outcomes.is_empty(), "{:?}", replay.outcomes);
        assert_eq!(s.host().granted_rewards(), 1);

        // The reward is still not applicable: retried, still refused, still
        // outstanding.
        let again = s.retry_host();
        assert_eq!(
            again.faults,
            [HostFault::UnknownReward {
                reward: undeclared.clone()
            }]
        );
        assert_eq!(s.host().outstanding(), 1);

        // Once the catalog carries it, the same execution key applies once.
        assert!(s.host_mut().declare_reward(undeclared.clone()));
        let fixed = s.retry_host();
        assert_eq!(reward_ids(&fixed), vec![undeclared.clone()]);
        assert!(fixed.faults.is_empty());
        assert_eq!(s.host().outstanding(), 0);
        assert_eq!(s.host().granted_rewards(), 2);
        // And a further retry changes nothing.
        assert!(s.retry_host().outcomes.is_empty());
        assert_eq!(s.host().granted_rewards(), 2);
    }

    /// Two objectives request conflicting outcomes on one tick: both requests
    /// are recorded, the resolved outcome is the one that settles, and the
    /// session tears down so nothing the deferred queue held fires afterwards.
    #[test]
    fn accept_f37_c_session_records_the_resolved_outcome_once_and_tears_down() {
        let mut s = session(
            vec![
                objective(
                    1,
                    Condition::Const(true),
                    vec![
                        Action::Finish(Outcome::Succeeded),
                        delay(LATER_TICK - 1, vec![reward("r-too-late")]),
                    ],
                ),
                objective(
                    2,
                    Condition::Const(true),
                    vec![Action::Finish(Outcome::Failed)],
                ),
            ],
            vec![cid(ContentKind::Blueprint, "r-too-late")],
        );
        let tick = s.advance(&facts(), Tick(1)).unwrap();
        // Both requests reached the host; the precedence policy resolved the
        // failure, and that is what the ledger settled on.
        assert_eq!(
            terminal_requests(&tick.host),
            [Outcome::Succeeded, Outcome::Failed]
        );
        assert_eq!(tick.terminal, TerminalState::Failed);
        assert_eq!(s.host().settled(), Some((Tick(1), TerminalState::Failed)));

        // Teardown dropped the deferred reward rather than leaving it queued.
        assert_eq!(s.state().queued_items(), 0);
        assert_eq!(s.host().torn_down(), Some(Tick(1)));
        for later in 2..=LATER_TICK + 2 {
            let after = s.advance(&facts(), Tick(later)).unwrap();
            assert!(after.events.is_empty(), "tick {later} ran program work");
            assert!(after.host.rewards_granted.is_empty());
            assert_eq!(after.terminal, TerminalState::Failed);
        }
        assert_eq!(s.host().granted_rewards(), 0);
    }

    /// A stale result offered after teardown is refused instead of applied, and
    /// an effect the program had already earned may still be retried.
    #[test]
    fn accept_f37_c_abort_refuses_a_late_effect_but_keeps_an_earned_one() {
        let late = cid(ContentKind::Blueprint, "r-late");
        let earned = cid(ContentKind::Blueprint, "r-earned");
        let mut s = session(
            vec![objective(
                1,
                Condition::Const(true),
                vec![
                    reward("r-late"),
                    reward("r-earned"),
                    delay(LATER_TICK - 1, vec![reward("r-earned")]),
                ],
            )],
            vec![late.clone(), earned.clone()],
        );
        // The producer's result is kept so the same tick can be offered again
        // after the session is over.
        let stale = s.step(&facts(), Tick(1)).unwrap();
        let first = s.host_mut().apply(&stale);
        assert_eq!(reward_ids(&first), vec![late.clone(), earned.clone()]);

        let dropped = s.abort(Tick(1));
        assert_eq!(dropped, 1, "the delayed item was dropped");
        assert_eq!(s.state().terminal(), TerminalState::Aborted);
        assert!(s.state().pending_timers().is_empty());

        // Offering the pre-abort tick again reaches nothing: the two applied
        // effects are skipped and the dropped item's effect is refused.
        let after = s.host_mut().apply(&stale);
        assert!(after.rewards_granted.is_empty(), "{after:?}");
        assert!(after.outcomes.is_empty(), "{after:?}");
        assert_eq!(s.host().granted_rewards(), 2);

        // An effect the host never applied is refused with the teardown reason
        // rather than granted late.
        let mut unapplied = stale.clone();
        unapplied.events = vec![MissionEvent {
            key: cs_script::runtime::EventKey {
                session: SESSION,
                tick: Tick(LATER_TICK),
                source: SymbolId(1),
                sequence: 999,
            },
            kind: EventKind::RewardGranted(late.clone()),
        }];
        let refused = s.host_mut().apply(&unapplied);
        assert_eq!(refused.faults, vec![HostFault::AfterTeardown]);
        assert_eq!(s.host().granted_rewards(), 2);

        // And the mission stays aborted, running no further program work.
        let later = s.advance(&facts(), Tick(LATER_TICK)).unwrap();
        assert!(later.events.is_empty());
        assert_eq!(later.terminal, TerminalState::Aborted);
    }

    /// Save and restore move the evaluator state and the authoritative host
    /// record together: the pending timer's remaining ticks survive, the reward
    /// already granted is not granted again, and the restored run applies
    /// exactly what an uninterrupted run applies.
    #[test]
    fn accept_f37_c_session_snapshot_restores_runtime_and_host_together() {
        let catalog = vec![
            cid(ContentKind::Blueprint, "r-now"),
            cid(ContentKind::Blueprint, "r-later"),
        ];
        let mut live = MissionSession::launch(phased_program(), SESSION, catalog.clone()).unwrap();
        let first = live.advance(&facts(), Tick(1)).unwrap();
        assert_eq!(reward_ids(&first.host), vec![catalog[0].clone()]);
        assert_eq!(live.host().granted_rewards(), 1);
        assert_eq!(live.host().settled(), None);
        let timers = live.state().pending_timers();
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0].remaining, LATER_TICK - 1);

        let mut restored = MissionSession::restore(phased_program(), live.snapshot()).unwrap();
        assert_eq!(restored.state().pending_timers(), timers);
        assert_eq!(restored.state().last_tick(), Some(Tick(1)));
        assert_eq!(restored.host().granted_rewards(), 1);

        // Both runs continue to the end and must agree tick by tick.
        let mut live_reports = vec![reward_ids(&first.host)];
        let mut restored_reports = vec![reward_ids(&first.host)];
        for tick in 2..=LATER_TICK + 1 {
            let a = live.advance(&facts(), Tick(tick)).unwrap();
            let b = restored.advance(&facts(), Tick(tick)).unwrap();
            assert_eq!(a.terminal, b.terminal, "tick {tick}");
            assert_eq!(reward_ids(&a.host), reward_ids(&b.host), "tick {tick}");
            live_reports.push(reward_ids(&a.host));
            restored_reports.push(reward_ids(&b.host));
        }
        assert_eq!(restored_reports, live_reports);
        assert_eq!(
            restored.host().settled(),
            Some((Tick(LATER_TICK + 1), TerminalState::Succeeded))
        );
        // Two grants total across the save: the earlier one is not repeated.
        assert_eq!(restored.host().granted_rewards(), 2, "{restored_reports:?}");
        assert_eq!(
            restored
                .host()
                .applied()
                .filter(|(_, effect)| matches!(
                    effect,
                    HostEffect::Reward { reward } if *reward == catalog[0]
                ))
                .count(),
            1,
            "the reward granted before the save must not be granted again"
        );
    }

    /// The resolved outcome is the session's one answer. A result offered later
    /// that asks for a different one is refused, and it is refused whether the
    /// contradiction arrives as an event or as the tick's own claim.
    #[test]
    fn accept_f37_c_host_refuses_an_outcome_that_contradicts_the_settled_record() {
        let mut s = session(
            vec![objective(
                1,
                Condition::Const(true),
                vec![Action::Finish(Outcome::Succeeded)],
            )],
            vec![],
        );
        // Settled without a teardown, so the record still answers this tick.
        let result = s.step(&facts(), Tick(1)).unwrap();
        let first = s.host_mut().apply(&result);
        assert!(first.faults.is_empty(), "{first:?}");
        assert_eq!(
            s.host().settled(),
            Some((Tick(1), TerminalState::Succeeded))
        );
        assert!(s.host().torn_down().is_none());

        // A request for the outcome that already settled is still a request the
        // host records; it changes nothing.
        let agreeing = TickResult {
            session: SESSION,
            tick: Tick(2),
            events: vec![MissionEvent {
                key: cs_script::runtime::EventKey {
                    session: SESSION,
                    tick: Tick(2),
                    source: SymbolId(1),
                    sequence: 1,
                },
                kind: EventKind::TerminalRequested(Outcome::Succeeded),
            }],
            terminal: TerminalState::Running,
            stop: None,
        };
        assert!(s.host_mut().apply(&agreeing).faults.is_empty());

        // A request for the other outcome is refused: success cannot coexist
        // with failure.
        let conflicting = TickResult {
            session: SESSION,
            tick: Tick(3),
            events: vec![MissionEvent {
                // A fresh execution key: the request above already spent
                // sequence 1, and the exactly-once guard fires first.
                key: cs_script::runtime::EventKey {
                    session: SESSION,
                    tick: Tick(3),
                    source: SymbolId(1),
                    sequence: 2,
                },
                kind: EventKind::TerminalRequested(Outcome::Failed),
            }],
            terminal: TerminalState::Running,
            stop: None,
        };
        let refused = s.host_mut().apply(&conflicting);
        assert_eq!(
            refused.faults,
            [HostFault::OutcomeConflict {
                settled: TerminalState::Succeeded,
                offered: TerminalState::Failed,
            }]
        );

        // And so is a tick that asserts the other outcome outright.
        let contradiction = TickResult {
            session: SESSION,
            tick: Tick(4),
            events: Vec::new(),
            terminal: TerminalState::Failed,
            stop: None,
        };
        let refused = s.host_mut().apply(&contradiction);
        assert_eq!(
            refused.outcomes,
            [HostOutcome::SessionRefused {
                fault: HostFault::OutcomeConflict {
                    settled: TerminalState::Succeeded,
                    offered: TerminalState::Failed,
                }
            }]
        );

        // The record still holds the outcome it settled on, not the one that was
        // offered against it.
        assert_eq!(
            s.host().settled(),
            Some((Tick(1), TerminalState::Succeeded))
        );
    }

    /// A retry applies what the catalog now allows, in the execution-key order
    /// the report claims — not in the order the refusals happened to be held.
    #[test]
    fn accept_f37_c_retry_reports_in_execution_key_order() {
        let r_five = cid(ContentKind::Blueprint, "r-five");
        let r_one = cid(ContentKind::Blueprint, "r-one");
        let mut s = MissionSession::launch(
            MissionProgram {
                version: IR_VERSION,
                mission: cid(ContentKind::Mission, "synthetic-f37c"),
                variables: vec![Variable {
                    id: SymbolId(100),
                    name: "phase".into(),
                    initial: Value::Int(0),
                }],
                objectives: vec![
                    objective(
                        5,
                        Condition::Const(true),
                        vec![
                            Action::SetVariable {
                                variable: SymbolId(100),
                                value: Value::Int(1),
                            },
                            reward("r-five"),
                        ],
                    ),
                    objective(
                        1,
                        Condition::Compare {
                            variable: SymbolId(100),
                            op: CompareOp::Eq,
                            value: Value::Int(1),
                        },
                        vec![reward("r-one")],
                    ),
                ],
            },
            SESSION,
            [],
        )
        .unwrap();
        // Objective #5 fires first, so its refusal (source #5) is held before
        // objective #1's (source #1): the two arrive in the opposite order to
        // the execution keys that order a retry.
        s.advance(&facts(), Tick(1)).unwrap();
        s.advance(&facts(), Tick(2)).unwrap();
        assert_eq!(s.host().outstanding(), 2);
        assert!(!s.host().can_apply(&r_five));

        s.host_mut().declare_reward(r_five.clone());
        s.host_mut().declare_reward(r_one.clone());
        let retry = s.retry_host();
        assert_eq!(reward_ids(&retry), vec![r_one, r_five]);
        assert_eq!(s.host().outstanding(), 0);
        assert_eq!(s.host().granted_rewards(), 2);
        assert!(s.retry_host().outcomes.is_empty());
    }

    /// A record is refused whole, with its defect named: a mismatched version,
    /// records from two different sessions, a foreign execution key, an effect
    /// recorded as both applied and outstanding, and a program that does not
    /// validate.
    #[test]
    fn accept_f37_c_session_restore_refuses_a_mismatched_record() {
        let build = || {
            program(vec![objective(
                1,
                Condition::Const(true),
                vec![reward("r")],
            )])
        };
        let mut s =
            MissionSession::launch(build(), SESSION, vec![cid(ContentKind::Blueprint, "r")])
                .unwrap();
        s.advance(&facts(), Tick(1)).unwrap();
        let good = s.snapshot();
        assert_eq!(good.host.applied.len(), 1);

        let mut older = good.clone();
        older.version = SNAPSHOT_VERSION + 1;
        assert!(matches!(
            MissionSession::restore(build(), older),
            Err(SessionRestoreError::SnapshotVersion { .. })
        ));

        // Two records from different sessions must never be paired.
        let mut crossed = good.clone();
        crossed.host.session = SessionGeneration(99);
        assert!(matches!(
            MissionSession::restore(build(), crossed),
            Err(SessionRestoreError::Host(
                HostRestoreError::SessionMismatch {
                    evaluator: SESSION,
                    host: SessionGeneration(99),
                }
            ))
        ));

        let mut foreign = good.clone();
        foreign.host.applied[0].0 = ExecutionKey {
            session: SessionGeneration(99),
            ..foreign.host.applied[0].0
        };
        assert!(matches!(
            MissionSession::restore(build(), foreign),
            Err(SessionRestoreError::Host(
                HostRestoreError::ForeignExecutionKey { .. }
            ))
        ));

        let mut doubled = good.clone();
        doubled.host.applied.push(doubled.host.applied[0].clone());
        assert!(matches!(
            MissionSession::restore(build(), doubled),
            Err(SessionRestoreError::Host(
                HostRestoreError::DuplicateExecutionKey { .. }
            ))
        ));

        // Applied *and* outstanding: a retry could grant it twice.
        let mut both = good.clone();
        both.host.outstanding = both.host.applied.clone();
        assert!(matches!(
            MissionSession::restore(build(), both),
            Err(SessionRestoreError::Host(
                HostRestoreError::OutstandingAlreadyApplied { .. }
            ))
        ));

        // A program that does not validate may not own restored state.
        let unvalidated = program(vec![objective(
            1,
            Condition::Const(true),
            vec![Action::Unknown {
                instruction: "op".into(),
            }],
        )]);
        assert!(matches!(
            MissionSession::restore(unvalidated, good.clone()),
            Err(SessionRestoreError::Program(_))
        ));

        // The intact record still restores, so the refusals above are the
        // defects and not a record that never restores.
        let back = MissionSession::restore(build(), good).unwrap();
        assert_eq!(back.host().granted_rewards(), 1);
        assert_eq!(back.state().terminal(), TerminalState::Running);
    }

    /// The host record is data from outside the process too: a record whose
    /// ledger is past the bounds a live session could reach is refused instead
    /// of being turned into memory and work.
    #[test]
    fn accept_f37_c_host_restore_refuses_a_record_past_its_bounds() {
        let build = || {
            program(vec![objective(
                1,
                Condition::Const(true),
                vec![reward("r")],
            )])
        };
        let mut s =
            MissionSession::launch(build(), SESSION, vec![cid(ContentKind::Blueprint, "r")])
                .unwrap();
        s.advance(&facts(), Tick(1)).unwrap();
        let good = s.snapshot();

        let key = |sequence: u32| ExecutionKey {
            session: SESSION,
            source: SymbolId(1),
            sequence,
        };

        let mut applied = good.clone();
        applied.host.applied = (0..=MAX_APPLIED_EFFECTS)
            .map(|i| {
                (
                    key(i as u32),
                    HostEffect::Reward {
                        reward: cid(ContentKind::Blueprint, "r"),
                    },
                )
            })
            .collect();
        assert!(matches!(
            MissionSession::restore(build(), applied),
            Err(SessionRestoreError::Host(
                HostRestoreError::TooManyApplied { .. }
            ))
        ));

        let mut catalog = good.clone();
        catalog.host.rewards = (0..=MAX_REWARD_CATALOG)
            .map(|i| cid(ContentKind::Blueprint, &format!("r-{i}")))
            .collect();
        assert!(matches!(
            MissionSession::restore(build(), catalog),
            Err(SessionRestoreError::Host(
                HostRestoreError::RewardCatalogTooLong { .. }
            ))
        ));

        let mut held = good.clone();
        held.host.outstanding = (0..=MAX_OUTSTANDING_EFFECTS)
            .map(|i| {
                (
                    key(i as u32),
                    HostEffect::Reward {
                        reward: cid(ContentKind::Blueprint, "r"),
                    },
                )
            })
            .collect();
        assert!(matches!(
            MissionSession::restore(build(), held),
            Err(SessionRestoreError::Host(
                HostRestoreError::TooManyOutstanding { .. }
            ))
        ));

        // The intact record still restores.
        assert_eq!(
            MissionSession::restore(build(), good)
                .unwrap()
                .host()
                .granted_rewards(),
            1
        );
    }
    /// An adversarial corpus member run through the *wired* path: objectives
    /// declared out of symbol order, each with a delayed reward, so the host is
    /// asked to apply effects whose key order differs from the order they were
    /// produced in.
    fn scrambled_program() -> MissionProgram {
        MissionProgram {
            version: IR_VERSION,
            mission: cid(ContentKind::Mission, "synthetic-f37d"),
            variables: vec![],
            objectives: [9u32, 3, 7, 1, 5]
                .into_iter()
                .map(|id| {
                    objective(
                        id,
                        Condition::Const(true),
                        vec![
                            reward(&format!("r-{id}")),
                            delay(id as u64 % 3, vec![reward(&format!("r-{id}-late"))]),
                        ],
                    )
                })
                .collect(),
        }
    }

    use cs_script::runtime::EventKey;

    /// The event key of every reward the producer emitted, in emission order.
    fn reward_keys(events: &[MissionEvent]) -> Vec<ExecutionKey> {
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::RewardGranted(_)))
            .map(|e| e.key.execution_key())
            .collect()
    }

    /// Every effect the host applied, in the order the report gave it.
    fn applied_rewards(report: &HostReport) -> Vec<ContentId> {
        report.rewards_granted.clone()
    }

    /// AC04 through the wired path: a mission with an undecodable instruction
    /// never launches, so it is Unsupported, grants nothing and progresses
    /// nothing — whatever the reward catalog declares for it.
    #[test]
    fn accept_f37_d_unsupported_mission_never_rewards_or_progresses() {
        // The objectives declared before the undecodable one would reward and
        // finish the mission on tick 1.
        let rewarding = objective(
            1,
            Condition::Const(true),
            vec![reward("r-reward"), Action::Finish(Outcome::Succeeded)],
        );
        let catalog = vec![cid(ContentKind::Blueprint, "r-reward")];
        for (index, undecodable) in [
            objective(
                2,
                Condition::Unknown {
                    instruction: "native 0x1f".into(),
                },
                vec![reward("r-condition")],
            ),
            objective(
                3,
                Condition::Const(true),
                vec![Action::Unknown {
                    instruction: "op 0x77".into(),
                }],
            ),
            objective(
                4,
                Condition::Const(true),
                vec![delay(
                    1,
                    vec![delay(
                        1,
                        vec![Action::Unknown {
                            instruction: "deep op".into(),
                        }],
                    )],
                )],
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let built = MissionProgram {
                version: IR_VERSION,
                mission: cid(ContentKind::Mission, "synthetic-f37d"),
                variables: vec![],
                objectives: vec![rewarding.clone(), undecodable],
            };
            let refused = MissionSession::launch(built, SESSION, catalog.clone())
                .expect_err("an undecodable instruction must refuse the launch");
            assert_eq!(refused.terminal, TerminalState::Unsupported);
            assert!(
                matches!(
                    refused.error,
                    ValidationError::UnsupportedInstruction { .. }
                ),
                "corpus {index} refused for the wrong reason: {:?}",
                refused.error
            );
            // The refusal is not a launch that produced nothing: there is no
            // session, so there is no state, no host ledger and no record.
            assert!(
                refused
                    .error
                    .to_string()
                    .contains("unsupported instruction"),
                "corpus {index} diagnostic: {}",
                refused.error
            );
        }
    }

    /// The reference ordering probe through the host: effects are applied and
    /// reported in the documented key order, whatever order they arrive in, and
    /// the applied ledger keeps that order too.
    #[test]
    fn accept_f37_d_host_orders_effects_by_reference_key_not_arrival_order() {
        let catalog: Vec<ContentId> = ["r-1", "r-3", "r-5", "r-7", "r-9"]
            .into_iter()
            .map(|k| cid(ContentKind::Blueprint, k))
            .collect();
        let mut s = MissionSession::launch(scrambled_program(), SESSION, catalog.clone()).unwrap();
        let tick = s.advance(&facts(), Tick(1)).unwrap();

        // The five immediate rewards are reported in source-symbol order, not in
        // the order their objectives were declared (9, 3, 7, 1, 5) and not in
        // the order the delayed items fire.
        assert_eq!(
            applied_rewards(&tick.host),
            catalog,
            "the host applied effects out of reference key order"
        );
        // The producer emitted them in the same order: the host's report is the
        // reference key order, not an accident of what arrived first.
        let keys = reward_keys(&tick.events);
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(
            keys, sorted,
            "the producer emitted rewards out of key order"
        );

        // A result whose events arrive out of order is still applied and
        // reported in key order: the ledger does not trust the arrival order for
        // the order it claims.
        let mut other =
            MissionSession::launch(scrambled_program(), SESSION, catalog.clone()).unwrap();
        let mut shuffled = other.step(&facts(), Tick(1)).unwrap();
        assert_ne!(
            shuffled.events.first().map(|e| e.key),
            shuffled.events.last().map(|e| e.key),
            "the fixture must not already be in reverse order"
        );
        shuffled.events.reverse();
        let report = other.host_mut().apply(&shuffled);
        assert_eq!(
            applied_rewards(&report),
            catalog,
            "a shuffled result changed the order the host reported"
        );
        assert_eq!(other.host().granted_rewards(), 5);
    }

    /// A stale event from an earlier run of the same mission is refused, and the
    /// refusal leaves the ledger saveable: applying it would grant a reward
    /// this session never earned and write an execution key from another
    /// session into the record, which is a record nothing can ever restore.
    #[test]
    fn accept_f37_d_host_refuses_a_stale_session_event_and_stays_saveable() {
        let stale_reward = cid(ContentKind::Blueprint, "r-stale");
        let live_reward = cid(ContentKind::Blueprint, "r-live");
        let build = || {
            program(vec![objective(
                1,
                Condition::Const(true),
                vec![
                    reward("r-live"),
                    delay(LATER_TICK - 1, vec![reward("r-stale")]),
                ],
            )])
        };
        let mut s = MissionSession::launch(
            build(),
            SESSION,
            vec![live_reward.clone(), stale_reward.clone()],
        )
        .unwrap();
        s.advance(&facts(), Tick(1)).unwrap();
        assert_eq!(s.host().granted_rewards(), 1);

        // The previous run's session generation, replaying a result this run
        // never produced. Its stamp claims this session — the per-event check
        // is what still catches it.
        let stale = TickResult {
            session: SESSION,
            tick: Tick(1),
            events: vec![
                MissionEvent {
                    key: EventKey {
                        session: SessionGeneration(SESSION.0 - 1),
                        tick: Tick(1),
                        source: SymbolId(1),
                        sequence: 1,
                    },
                    kind: EventKind::RewardGranted(stale_reward.clone()),
                },
                MissionEvent {
                    key: EventKey {
                        session: SESSION,
                        tick: Tick(1),
                        source: SymbolId(1),
                        sequence: 2,
                    },
                    kind: EventKind::RewardGranted(stale_reward.clone()),
                },
            ],
            terminal: TerminalState::Succeeded,
            stop: None,
        };
        let report = s.host_mut().apply(&stale);
        assert_eq!(
            report.faults,
            [HostFault::ForeignSession {
                session: SessionGeneration(SESSION.0 - 1)
            }],
            "a result carrying another session's key must be refused whole"
        );
        assert!(applied_rewards(&report).is_empty(), "{report:?}");
        assert_eq!(s.host().granted_rewards(), 1, "{report:?}");
        // The foreign outcome claim is refused with it: a stale result cannot
        // settle this session.
        assert_eq!(s.host().settled(), None);
        assert!(s.host().applied().all(|(key, _)| key.session == SESSION));

        // The session is still saveable, and the run continues to grant the
        // reward this session really earned.
        let record = s.snapshot();
        let mut restored = MissionSession::restore(build(), record).unwrap();
        assert_eq!(restored.host().granted_rewards(), 1);
        let later = restored.advance(&facts(), Tick(LATER_TICK)).unwrap();
        assert_eq!(applied_rewards(&later.host), vec![stale_reward]);
        assert_eq!(restored.host().granted_rewards(), 2);
    }

    /// F37-D-FU1: the result's own stamp is checked before anything else, so an
    /// event-less result from another session cannot settle this ledger — the
    /// hole the per-event check could not see.
    #[test]
    fn accept_f37_d_fu1_eventless_foreign_result_cannot_settle_the_ledger() {
        let foreign = SessionGeneration(SESSION.0 - 1);
        let mut s = session(
            vec![objective(
                1,
                Condition::Const(true),
                vec![Action::Finish(Outcome::Succeeded)],
            )],
            vec![],
        );

        // The previous run's terminal claim, replayed by a caller that
        // hand-builds results — the only path that can produce one, since
        // `MissionSession::advance` applies its own. No event carries the stale
        // session, so only the stamp can.
        let stale = TickResult {
            session: foreign,
            tick: Tick(1),
            events: Vec::new(),
            terminal: TerminalState::Succeeded,
            stop: None,
        };
        let report = s.host_mut().apply(&stale);
        assert_eq!(
            report.outcomes,
            [HostOutcome::SessionRefused {
                fault: HostFault::ForeignSession { session: foreign }
            }],
            "an event-less foreign result must be refused whole"
        );
        assert_eq!(
            s.host().settled(),
            None,
            "the foreign claim must not settle this session"
        );
        assert!(s.host().torn_down().is_none());

        // The refusal touched nothing: this session's own tick still settles
        // it, and the wired path's stamp is accepted.
        let tick = s.advance(&facts(), Tick(1)).unwrap();
        assert_eq!(tick.terminal, TerminalState::Succeeded);
        assert_eq!(
            s.host().settled(),
            Some((Tick(1), TerminalState::Succeeded))
        );
    }

    /// F37-D-FU1: provenance is checked before the outcome is compared, so a
    /// foreign event-less result cannot re-settle the ledger either — not even
    /// with the outcome it already holds.
    #[test]
    fn accept_f37_d_fu1_eventless_foreign_result_cannot_resettle_the_ledger() {
        let foreign = SessionGeneration(SESSION.0 - 1);
        let mut s = session(
            vec![objective(
                1,
                Condition::Const(true),
                vec![Action::Finish(Outcome::Succeeded)],
            )],
            vec![],
        );
        // Settled through the real step/apply path, without the teardown
        // `advance` would have run, so the record still answers applies.
        let result = s.step(&facts(), Tick(1)).unwrap();
        s.host_mut().apply(&result);
        assert_eq!(
            s.host().settled(),
            Some((Tick(1), TerminalState::Succeeded))
        );

        for offered in [TerminalState::Failed, TerminalState::Succeeded] {
            let stale = TickResult {
                session: foreign,
                tick: Tick(2),
                events: Vec::new(),
                terminal: offered,
                stop: None,
            };
            let report = s.host_mut().apply(&stale);
            assert_eq!(
                report.faults,
                [HostFault::ForeignSession { session: foreign }],
                "a foreign {offered:?} claim must name its session, not reach the outcome check"
            );
            assert_eq!(
                s.host().settled(),
                Some((Tick(1), TerminalState::Succeeded)),
                "a foreign result changed the settled record"
            );
        }
    }

    /// F37-D-FU1: `MissionState::step` stamps its own generation, and the stamp
    /// is checked before the per-event scan — a result foreign on both levels
    /// is refused by the stamp it carries, which names that session.
    #[test]
    fn accept_f37_d_fu1_step_stamps_and_the_stamp_is_checked_first() {
        let live_reward = cid(ContentKind::Blueprint, "r-live");
        let mut s = session(
            vec![objective(1, Condition::Const(true), vec![reward("r-live")])],
            vec![live_reward.clone()],
        );
        let live = s.step(&facts(), Tick(1)).unwrap();
        assert_eq!(live.session, SESSION, "step must stamp its own session");
        assert!(s.host_mut().apply(&live).faults.is_empty());
        assert_eq!(s.host().granted_rewards(), 1);

        // Foreign stamp and foreign event keys: the stamp's refusal comes
        // first, so the report names the result's session, not an event's.
        let crossed = TickResult {
            session: SessionGeneration(SESSION.0 + 1),
            tick: Tick(2),
            events: vec![MissionEvent {
                key: EventKey {
                    session: SessionGeneration(SESSION.0 - 1),
                    tick: Tick(2),
                    source: SymbolId(1),
                    sequence: 7,
                },
                kind: EventKind::RewardGranted(live_reward),
            }],
            terminal: TerminalState::Running,
            stop: None,
        };
        let report = s.host_mut().apply(&crossed);
        assert_eq!(
            report.faults,
            [HostFault::ForeignSession {
                session: SessionGeneration(SESSION.0 + 1)
            }],
            "the refusal must name the session the result carries"
        );
        assert_eq!(
            s.host().granted_rewards(),
            1,
            "a crossed result must not apply even its own-session-looking parts"
        );
    }

    /// The host record is data from outside the process: one that claims the
    /// session settled while it was still running is refused, because it would
    /// make every later outcome a contradiction and leave the session unable to
    /// settle at all.
    #[test]
    fn accept_f37_d_host_restore_refuses_a_record_that_settled_on_running() {
        let build = || {
            program(vec![objective(
                1,
                Condition::Const(true),
                vec![reward("r")],
            )])
        };
        let mut s =
            MissionSession::launch(build(), SESSION, vec![cid(ContentKind::Blueprint, "r")])
                .unwrap();
        s.advance(&facts(), Tick(1)).unwrap();
        let good = s.snapshot();
        assert_eq!(good.host.settled, None);

        let mut running = good.clone();
        running.host.settled = Some((Tick(1), TerminalState::Running));
        assert!(matches!(
            MissionSession::restore(build(), running),
            Err(SessionRestoreError::Host(
                HostRestoreError::SettledWhileRunning { .. }
            ))
        ));

        // The intact record still restores.
        assert_eq!(
            MissionSession::restore(build(), good)
                .unwrap()
                .host()
                .granted_rewards(),
            1
        );
    }
}
