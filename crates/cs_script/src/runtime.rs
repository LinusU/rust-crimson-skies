//! Mutable mission state, stable event ordering and the objective tick (F37-A).
//!
//! Program data is [`crate::ir`]; this module holds the *execution* state and
//! the pure per-tick resolution that the simulation host drives. It is the
//! minimal path for the F37-A scenario: several objectives become true on one
//! tick and the result order is a function of the program, never of map
//! iteration order. The bounded work budget, timers and snapshot/restore are
//! F37-B/F37-C.
//!
//! Phases (`docs/contracts/SCRIPT-MISSION.md`, "Objective event ordering"):
//! 1. **Observe** — every condition is evaluated against the state as it was
//!    at the start of the tick; nothing evaluated in this tick sees a write
//!    made in this tick.
//! 2. **Queue** — firing objectives are taken in program order and their
//!    events get an [`EventKey`].
//! 3. **Resolve** — `State` writes are applied, `Terminal` requests are
//!    resolved by the [`PrecedencePolicy`], `Host` effects are emitted.

use std::collections::{BTreeMap, BTreeSet};

use cs_types::Tick;
use cs_types::content::ContentId;

use crate::ir::{
    Action, ActorId, ActorState, CompareOp, Condition, Outcome, SymbolId, ValidatedProgram, Value,
};

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
}

/// Why a tick was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickError {
    /// The tick is not after the last evaluated one (a replay or retry).
    NotAdvancing { last: Tick, given: Tick },
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
        }
    }

    pub fn terminal(&self) -> TerminalState {
        self.terminal
    }

    pub fn variable(&self, id: SymbolId) -> Option<&Value> {
        self.variables.get(&id)
    }

    pub fn is_completed(&self, objective: SymbolId) -> bool {
        self.completed.contains(&objective)
    }

    /// Resolves one tick. See the module docs for the phases.
    ///
    /// # Errors
    ///
    /// [`TickError::NotAdvancing`] when `tick` is not after the last one.
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
        };
        if self.terminal != TerminalState::Running {
            return Ok(result);
        }

        // Observe: all conditions against the start-of-tick state.
        let firing: Vec<_> = program
            .program()
            .objectives
            .iter()
            .filter(|o| !self.completed.contains(&o.id) && self.holds(&o.condition, facts))
            .collect();

        // Queue + Resolve, in program order.
        let mut writes = Vec::new();
        let mut requested = BTreeSet::new();
        for o in firing {
            self.completed.insert(o.id);
            let key = |sequence| EventKey {
                session: self.session,
                tick,
                source: o.id,
                sequence,
            };
            let mut push =
                |sequence: u32, kind: EventKind, consumed: &mut BTreeSet<ExecutionKey>| {
                    let key = key(sequence);
                    if consumed.insert(key.execution_key()) {
                        result.events.push(MissionEvent { key, kind });
                    }
                };
            push(0, EventKind::ObjectiveCompleted, &mut self.consumed);
            for (i, action) in o.actions.iter().enumerate() {
                let sequence = i as u32 + 1;
                match action {
                    Action::SetVariable { variable, value } => {
                        writes.push((*variable, value.clone()));
                    }
                    Action::Finish(outcome) => {
                        requested.insert(*outcome);
                        push(
                            sequence,
                            EventKind::TerminalRequested(*outcome),
                            &mut self.consumed,
                        );
                    }
                    Action::GrantReward { reward } => {
                        push(
                            sequence,
                            EventKind::RewardGranted(reward.clone()),
                            &mut self.consumed,
                        );
                    }
                    // Validation rejects Unknown; reaching it here is a
                    // program that bypassed `validate`, which is impossible
                    // through `ValidatedProgram`.
                    Action::Unknown { .. } => unreachable!("validated program"),
                }
            }
        }
        for (variable, value) in writes {
            self.variables.insert(variable, value);
        }
        if let Some(outcome) = self.policy.pick(&requested) {
            self.terminal = outcome.into();
        }
        result.events.sort_by_key(|e| e.key);
        result.terminal = self.terminal;
        Ok(result)
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
