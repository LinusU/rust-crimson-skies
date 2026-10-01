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
/// "control flow is explicit and bounded").
pub const MAX_ACTIONS_PER_OBJECTIVE: usize = 64;
/// Deepest condition nesting accepted.
pub const MAX_CONDITION_DEPTH: usize = 16;

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

/// Why an actor is no longer in play. The contract keeps these distinct: an
/// actor removed by a cinematic is not necessarily a kill.
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

/// An ordered action run when an objective's condition first becomes true.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    SetVariable {
        variable: SymbolId,
        value: Value,
    },
    /// Request the mission's terminal outcome.
    Finish(Outcome),
    /// A reward intent; the host applies it, the runtime emits it once.
    GrantReward {
        reward: ContentId,
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
            Self::SetVariable { .. } => Phase::State,
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
#[derive(Clone, Debug, PartialEq)]
pub struct MissionProgram {
    pub version: u32,
    /// The mission's stable id (`ContentKind::Mission`).
    pub mission: ContentId,
    pub variables: Vec<Variable>,
    /// Declaration order is the program sequence used for stable ordering.
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

    fn action(&self, a: &Action, index: usize) -> Result<(), ValidationError> {
        let label = format!("action {index}");
        let at = || self.at(&[label.as_str()]);
        match a {
            Action::Finish(_) | Action::GrantReward { .. } => Ok(()),
            Action::Unknown { instruction } => Err(ValidationError::UnsupportedInstruction {
                at: at(),
                instruction: instruction.clone(),
            }),
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
                ctx.action(a, i)?;
            }
        }
        Ok(ValidatedProgram(self))
    }
}
