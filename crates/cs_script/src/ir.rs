//! The typed, versioned mission IR and its pre-launch validation (F37-A).
//!
//! `specs/F37-mission-ir-and-deterministic-runtime-core.md` and
//! `docs/contracts/SCRIPT-MISSION.md`. This IR is a **new design**: it is not a
//! claim that original mission programs use this structure, and it contains no
//! original opcode numbers. Program *data* lives here; mutable execution state
//! and the evaluator are in [`crate::runtime`], and host effects are applied by
//! the simulation (`cs_sim::mission`).
//!
//! A [`MissionProgram`] only becomes launchable through
//! [`MissionProgram::validate`], which checks symbol uniqueness, references,
//! types and bounds up front and returns a [`ValidatedProgram`]. An
//! instruction or native call the adapter could not decode is carried as an
//! explicit `Unknown` node and makes validation fail with a precise
//! [`ValidationError::UnsupportedInstruction`] trace; it is never a NOP.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};

/// The IR version this crate understands.
pub const IR_VERSION: u32 = 1;
/// Most actions one objective may carry; bounds work per tick (contract:
/// "control flow is explicit and bounded"). The same bound covers the action
/// list of one deferred work item ([`Action::Schedule`]).
pub const MAX_ACTIONS_PER_OBJECTIVE: usize = 64;
/// Deepest condition nesting accepted.
pub const MAX_CONDITION_DEPTH: usize = 16;
/// Deepest `Schedule` action-list nesting accepted (contract: "recursion /
/// stack limits").
pub const MAX_ACTION_NESTING: usize = 16;

/// Stable identity of a mission-scoped actor. Never an entity index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId(pub u32);

/// Stable identity of a variable or objective inside one program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

/// A byte range in the program's source resource, kept so diagnostics can
/// point at the origin of a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: u32,
    pub end: u32,
}

/// The type of a [`Value`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Bool,
    Int,
    Float,
    Str,
    Content,
    Actor,
    Vector,
    OptActor,
}

/// A typed IR value. There is no implicit coercion between variants.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    /// Checked 32-bit integer.
    Int(i32),
    /// Must be finite; validation rejects NaN and infinity.
    Float(f64),
    Str(String),
    Content(ContentId),
    Actor(ActorId),
    Vector([f64; 3]),
    /// A typed optional actor reference.
    OptActor(Option<ActorId>),
}

impl Value {
    /// The value's type.
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::Int(_) => ValueType::Int,
            Self::Float(_) => ValueType::Float,
            Self::Str(_) => ValueType::Str,
            Self::Content(_) => ValueType::Content,
            Self::Actor(_) => ValueType::Actor,
            Self::Vector(_) => ValueType::Vector,
            Self::OptActor(_) => ValueType::OptActor,
        }
    }

    fn is_finite(&self) -> bool {
        match self {
            Self::Float(f) => f.is_finite(),
            Self::Vector(v) => v.iter().all(|c| c.is_finite()),
            _ => true,
        }
    }
}

/// Why an actor is no longer in play — or that it still is. The contract
/// keeps these distinct: an actor removed by a cinematic is not necessarily
/// a kill, a capture is not a destruction, and a detach is an event, not a
/// state (F39-E7). `cs_sim`'s actor-fact table is the writer: a state is
/// populated only where an authoritative event produces it, and refused by
/// name where nothing does — a variant's existence never writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ActorState {
    Alive,
    Disabled,
    Dead,
    Captured,
    Escaped,
    Detached,
    Despawned,
}

/// Comparison operator for [`Condition::Compare`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A side-effect-free boolean expression.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
    Const(bool),
    /// Compares a variable with a literal of the same type.
    Compare {
        variable: SymbolId,
        op: CompareOp,
        value: Value,
    },
    /// True when the actor is in exactly this state.
    ActorIs {
        actor: ActorId,
        state: ActorState,
    },
    Not(Box<Condition>),
    All(Vec<Condition>),
    Any(Vec<Condition>),
    /// An instruction or native call that could not be decoded.
    Unknown {
        instruction: String,
    },
}

/// How a mission ends. Mirrors the contract's terminal vocabulary minus
/// `Running`/`Unsupported`, which are runtime states
/// ([`crate::runtime::TerminalState`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Outcome {
    Succeeded,
    Failed,
    Aborted,
}

/// The documented phase in which an action's effect is resolved. Effects are
/// queued and resolved in this order, never recursively (non-negotiable
/// behavior 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// Variable writes, visible to conditions from the next tick.
    State,
    /// The terminal outcome request.
    Terminal,
    /// Reward intents and presentation cues handed to the host.
    Host,
}

/// An ordered action run when an objective's condition first becomes true, or
/// as part of a deferred work item ([`Action::Schedule`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SetVariable {
        variable: SymbolId,
        value: Value,
    },
    /// Writes the variable with the next draw of the mission's explicit RNG
    /// stream, uniform over the inclusive range `[min, max]` (contract:
    /// "explicit RNG"; determinism comes from the seeded stream, never from
    /// ambient entropy).
    Draw {
        variable: SymbolId,
        min: i32,
        max: i32,
    },
    /// Request the mission's terminal outcome.
    Finish(Outcome),
    /// A reward intent; the host applies it, the runtime emits it once.
    GrantReward {
        reward: ContentId,
    },
    /// Enqueues `actions` as one deferred work item, eligible on the tick
    /// `delay_ticks` after the current one. `delay_ticks == 0` appends to
    /// this tick's work queue, so it still runs this tick — after everything
    /// already queued (non-negotiable behavior 2: no reentrancy).
    Schedule {
        delay_ticks: u64,
        actions: Vec<Action>,
    },
    /// Re-queues the action list containing this action — the objective's
    /// list when reached from an objective, or the scheduled item's list when
    /// reached from a deferred item — eligible `delay_ticks` from now. A
    /// zero-delay `Reschedule` is how a program schedules *itself*; the
    /// per-tick work budget is what bounds it.
    Reschedule {
        delay_ticks: u64,
    },
    /// An instruction or native call that could not be decoded.
    Unknown {
        instruction: String,
    },
}

impl Action {
    /// The phase in which this action resolves.
    pub fn phase(&self) -> Phase {
        match self {
            // `Draw` writes a variable and `Schedule`/`Reschedule` mutate the
            // pending queue; both are runtime state, resolved in place.
            Self::SetVariable { .. }
            | Self::Draw { .. }
            | Self::Schedule { .. }
            | Self::Reschedule { .. } => Phase::State,
            Self::Finish(_) => Phase::Terminal,
            Self::GrantReward { .. } | Self::Unknown { .. } => Phase::Host,
        }
    }
}

/// A mutable program variable with a declared type.
#[derive(Clone, Debug, PartialEq)]
pub struct Variable {
    pub id: SymbolId,
    pub name: String,
    pub initial: Value,
}

/// One objective: a latch that fires its actions once when `condition` holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Objective {
    pub id: SymbolId,
    /// The stable content id (`ContentKind::Objective`).
    pub content: ContentId,
    pub condition: Condition,
    pub actions: Vec<Action>,
    pub span: Option<SourceSpan>,
}

/// The versioned mission program: data only.
///
/// `objectives` contributes two different orders and neither is the other's sort
/// (both are designed policies, pinned by the F37-D corpus):
///
/// - **Execution order** is declaration order. Within one tick the objectives
///   resolve in the order they are declared here, then the deferred work queue
///   drains. When two objectives write the same variable on the same tick, the
///   later declaration's write is the one that lands.
/// - **Observation order** is [`crate::runtime::EventKey`]: session, tick, source
///   symbol, sequence. It does not depend on declaration order at all, so the
///   same objectives declared in any order report the same event sequence.
#[derive(Clone, Debug, PartialEq)]
pub struct MissionProgram {
    pub version: u32,
    /// The mission's stable id (`ContentKind::Mission`).
    pub mission: ContentId,
    pub variables: Vec<Variable>,
    pub objectives: Vec<Objective>,
}

/// Where in the program a diagnostic points: mission id, objective and a
/// short node trace (non-negotiable behavior 4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramLocator {
    pub mission: String,
    pub objective: Option<SymbolId>,
    pub trace: Vec<String>,
}

impl fmt::Display for ProgramLocator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.mission)?;
        if let Some(o) = self.objective {
            write!(f, " objective#{}", o.0)?;
        }
        if !self.trace.is_empty() {
            write!(f, " [{}]", self.trace.join(" > "))?;
        }
        Ok(())
    }
}

/// Why a program cannot launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationError {
    UnsupportedVersion {
        found: u32,
    },
    WrongContentKind {
        at: ProgramLocator,
        expected: ContentKind,
    },
    DuplicateSymbol {
        at: ProgramLocator,
        symbol: SymbolId,
    },
    UnknownVariable {
        at: ProgramLocator,
        symbol: SymbolId,
    },
    TypeMismatch {
        at: ProgramLocator,
        expected: ValueType,
        found: ValueType,
    },
    NonFiniteValue {
        at: ProgramLocator,
    },
    InvalidComparison {
        at: ProgramLocator,
        op: CompareOp,
        ty: ValueType,
    },
    ConditionTooDeep {
        at: ProgramLocator,
    },
    /// A `Schedule` action list nested deeper than [`MAX_ACTION_NESTING`].
    ActionsTooDeep {
        at: ProgramLocator,
    },
    /// A `Draw` whose `min` exceeds its `max`.
    InvalidRange {
        at: ProgramLocator,
    },
    TooManyActions {
        at: ProgramLocator,
        count: usize,
    },
    /// An undecodable instruction or native call: the mission is Unsupported
    /// and must not progress or reward.
    UnsupportedInstruction {
        at: ProgramLocator,
        instruction: String,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion { found } => {
                write!(f, "IR version {found} unsupported (expected {IR_VERSION})")
            }
            Self::WrongContentKind { at, expected } => {
                write!(f, "{at}: id is not a {} id", expected.label())
            }
            Self::DuplicateSymbol { at, symbol } => {
                write!(f, "{at}: duplicate symbol #{}", symbol.0)
            }
            Self::UnknownVariable { at, symbol } => {
                write!(f, "{at}: unknown variable #{}", symbol.0)
            }
            Self::TypeMismatch {
                at,
                expected,
                found,
            } => {
                write!(f, "{at}: expected {expected:?}, found {found:?}")
            }
            Self::NonFiniteValue { at } => write!(f, "{at}: non-finite float"),
            Self::InvalidComparison { at, op, ty } => {
                write!(f, "{at}: {op:?} not defined for {ty:?}")
            }
            Self::ConditionTooDeep { at } => {
                write!(f, "{at}: condition deeper than {MAX_CONDITION_DEPTH}")
            }
            Self::ActionsTooDeep { at } => {
                write!(
                    f,
                    "{at}: scheduled actions deeper than {MAX_ACTION_NESTING}"
                )
            }
            Self::InvalidRange { at } => write!(f, "{at}: draw min exceeds max"),
            Self::TooManyActions { at, count } => {
                write!(
                    f,
                    "{at}: {count} actions exceeds {MAX_ACTIONS_PER_OBJECTIVE}"
                )
            }
            Self::UnsupportedInstruction { at, instruction } => {
                write!(f, "{at}: unsupported instruction `{instruction}`")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

/// A program that passed [`MissionProgram::validate`]. Only this type can be
/// handed to the runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct ValidatedProgram(MissionProgram);

impl ValidatedProgram {
    /// The validated program data.
    pub fn program(&self) -> &MissionProgram {
        &self.0
    }

    /// Checks one standalone action list against this program's declarations:
    /// the same rules [`MissionProgram::validate`] applies to an objective's
    /// list, at the same bounds.
    ///
    /// A deferred work item in a save record is data from outside the process
    /// (`crate::runtime::MissionState::restore`), and the evaluator's actions
    /// are written on the assumption that validation already refused an
    /// `Unknown` node, an empty `Draw` range and an undeclared write. A record
    /// can carry none of those, so its deferred lists are validated here before
    /// they are allowed back into the queue.
    ///
    /// # Errors
    ///
    /// The first [`ValidationError`] in declaration order, with its locator.
    pub fn validate_actions(&self, actions: &[Action]) -> Result<(), ValidationError> {
        let ctx = Ctx {
            program: &self.0,
            objective: None,
        };
        if actions.len() > MAX_ACTIONS_PER_OBJECTIVE {
            return Err(ValidationError::TooManyActions {
                at: ctx.at(&["deferred actions"]),
                count: actions.len(),
            });
        }
        for (i, action) in actions.iter().enumerate() {
            ctx.action(action, i, 0)?;
        }
        Ok(())
    }
}

struct Ctx<'a> {
    program: &'a MissionProgram,
    objective: Option<SymbolId>,
}

impl Ctx<'_> {
    fn at(&self, trace: &[&str]) -> ProgramLocator {
        ProgramLocator {
            mission: self.program.mission.to_string(),
            objective: self.objective,
            trace: trace.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    fn variable_type(&self, id: SymbolId) -> Option<ValueType> {
        self.program
            .variables
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.initial.value_type())
    }

    fn condition(
        &self,
        c: &Condition,
        depth: usize,
        trace: &[&str],
    ) -> Result<(), ValidationError> {
        if depth > MAX_CONDITION_DEPTH {
            return Err(ValidationError::ConditionTooDeep { at: self.at(trace) });
        }
        match c {
            Condition::Const(_) | Condition::ActorIs { .. } => Ok(()),
            Condition::Unknown { instruction } => Err(ValidationError::UnsupportedInstruction {
                at: self.at(trace),
                instruction: instruction.clone(),
            }),
            Condition::Not(inner) => self.condition(inner, depth + 1, trace),
            Condition::All(items) | Condition::Any(items) => items
                .iter()
                .try_for_each(|i| self.condition(i, depth + 1, trace)),
            Condition::Compare {
                variable,
                op,
                value,
            } => {
                let at = || self.at(&[trace, &["compare"]].concat());
                let Some(expected) = self.variable_type(*variable) else {
                    return Err(ValidationError::UnknownVariable {
                        at: at(),
                        symbol: *variable,
                    });
                };
                if !value.is_finite() {
                    return Err(ValidationError::NonFiniteValue { at: at() });
                }
                if value.value_type() != expected {
                    return Err(ValidationError::TypeMismatch {
                        at: at(),
                        expected,
                        found: value.value_type(),
                    });
                }
                let ordered = matches!(expected, ValueType::Int | ValueType::Float);
                let equality = matches!(op, CompareOp::Eq | CompareOp::Ne);
                if !ordered && !equality {
                    return Err(ValidationError::InvalidComparison {
                        at: at(),
                        op: *op,
                        ty: expected,
                    });
                }
                Ok(())
            }
        }
    }

    fn action(&self, a: &Action, index: usize, depth: usize) -> Result<(), ValidationError> {
        let label = format!("action {index}");
        let at = || self.at(&[label.as_str()]);
        if depth > MAX_ACTION_NESTING {
            return Err(ValidationError::ActionsTooDeep { at: at() });
        }
        match a {
            Action::Finish(_) | Action::GrantReward { .. } | Action::Reschedule { .. } => Ok(()),
            Action::Unknown { instruction } => Err(ValidationError::UnsupportedInstruction {
                at: at(),
                instruction: instruction.clone(),
            }),
            Action::Schedule { actions, .. } => {
                if actions.len() > MAX_ACTIONS_PER_OBJECTIVE {
                    return Err(ValidationError::TooManyActions {
                        at: at(),
                        count: actions.len(),
                    });
                }
                for (i, nested) in actions.iter().enumerate() {
                    self.action(nested, i, depth + 1)?;
                }
                Ok(())
            }
            Action::Draw { variable, min, max } => {
                let Some(expected) = self.variable_type(*variable) else {
                    return Err(ValidationError::UnknownVariable {
                        at: at(),
                        symbol: *variable,
                    });
                };
                if expected != ValueType::Int {
                    return Err(ValidationError::TypeMismatch {
                        at: at(),
                        expected: ValueType::Int,
                        found: expected,
                    });
                }
                if min > max {
                    return Err(ValidationError::InvalidRange { at: at() });
                }
                Ok(())
            }
            Action::SetVariable { variable, value } => {
                let Some(expected) = self.variable_type(*variable) else {
                    return Err(ValidationError::UnknownVariable {
                        at: at(),
                        symbol: *variable,
                    });
                };
                if !value.is_finite() {
                    return Err(ValidationError::NonFiniteValue { at: at() });
                }
                if value.value_type() != expected {
                    return Err(ValidationError::TypeMismatch {
                        at: at(),
                        expected,
                        found: value.value_type(),
                    });
                }
                Ok(())
            }
        }
    }
}

impl MissionProgram {
    /// Validates version, ids, references, types and bounds, in declaration
    /// order, returning the first failure with its locator.
    ///
    /// # Errors
    ///
    /// A [`ValidationError`]; in particular an `Unknown` instruction yields
    /// [`ValidationError::UnsupportedInstruction`] rather than being skipped.
    pub fn validate(self) -> Result<ValidatedProgram, ValidationError> {
        let ctx = Ctx {
            program: &self,
            objective: None,
        };
        if self.version != IR_VERSION {
            return Err(ValidationError::UnsupportedVersion {
                found: self.version,
            });
        }
        if self.mission.kind() != ContentKind::Mission {
            return Err(ValidationError::WrongContentKind {
                at: ctx.at(&[]),
                expected: ContentKind::Mission,
            });
        }
        let mut seen = BTreeSet::new();
        for v in &self.variables {
            if !seen.insert(v.id) {
                return Err(ValidationError::DuplicateSymbol {
                    at: ctx.at(&["variables"]),
                    symbol: v.id,
                });
            }
            if !v.initial.is_finite() {
                return Err(ValidationError::NonFiniteValue {
                    at: ctx.at(&["variables"]),
                });
            }
        }
        for o in &self.objectives {
            let ctx = Ctx {
                program: &self,
                objective: Some(o.id),
            };
            if !seen.insert(o.id) {
                return Err(ValidationError::DuplicateSymbol {
                    at: ctx.at(&[]),
                    symbol: o.id,
                });
            }
            if o.content.kind() != ContentKind::Objective {
                return Err(ValidationError::WrongContentKind {
                    at: ctx.at(&[]),
                    expected: ContentKind::Objective,
                });
            }
            if o.actions.len() > MAX_ACTIONS_PER_OBJECTIVE {
                return Err(ValidationError::TooManyActions {
                    at: ctx.at(&[]),
                    count: o.actions.len(),
                });
            }
            ctx.condition(&o.condition, 0, &["condition"])?;
            for (i, a) in o.actions.iter().enumerate() {
                ctx.action(a, i, 0)?;
            }
        }
        Ok(ValidatedProgram(self))
    }
}
