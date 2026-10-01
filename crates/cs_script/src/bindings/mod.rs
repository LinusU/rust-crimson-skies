//! The host-binding registry and the raw-call lowering (F38-A).
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! stage `### F38-A`; shared contract `docs/contracts/SCRIPT-MISSION.md`.
//!
//! An adapter that decodes an original program hands this module its host
//! calls as [`RawCall`]s: a name, typed argument [`Value`]s and a source span.
//! A [`HostBindingRegistry`] maps each *observed* call to one typed engine
//! operation ([`Lowering`]) with checked argument domains, an effect phase, a
//! repeatability policy and provenance. [`lower_program`] turns a
//! [`RawProgram`] into a [`MissionProgram`] only when every call is bound and
//! every argument is in its domain; otherwise it returns every
//! [`BindingError`], each with its source location, and no program exists to
//! launch. A call is never skipped and never lowered to a no-op.
//!
//! The registry is **empty by default and ships no original names**: which
//! calls exist is F13/F38-B measurement. Tests register synthetic names.
//! Call data is untrusted, so name length, argument count and string length are
//! capped before any lookup and no binding reaches anything but the
//! simulation API.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};

use crate::ir::{
    Action, Condition, IR_VERSION, MissionProgram, Objective, Outcome, Phase, SourceSpan, SymbolId,
    Value, ValueType, Variable,
};

/// Longest call name accepted.
pub const MAX_CALL_NAME_BYTES: usize = 64;
/// Most arguments one call may carry.
pub const MAX_CALL_ARGS: usize = 8;

/// The design-boundary family of a binding (contract "Host interface"). A
/// design vocabulary, not an observed original name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HostFamily {
    ActorLifecycle,
    CombatWorld,
    Interaction,
    MissionState,
    Presentation,
}

/// Whether a bound call may take effect more than once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Repeatability {
    /// The runtime emits it once per execution key.
    Once,
    Repeatable,
}

/// The accepted values of one argument.
#[derive(Clone, Debug, PartialEq)]
pub enum ArgDomain {
    Bool,
    IntRange { min: i32, max: i32 },
    FloatRange { min: f64, max: f64 },
    Str { max_bytes: usize },
    Content(ContentKind),
    Actor,
    Vector,
    OptActor,
}

impl ArgDomain {
    fn value_type(&self) -> ValueType {
        match self {
            Self::Bool => ValueType::Bool,
            Self::IntRange { .. } => ValueType::Int,
            Self::FloatRange { .. } => ValueType::Float,
            Self::Str { .. } => ValueType::Str,
            Self::Content(_) => ValueType::Content,
            Self::Actor => ValueType::Actor,
            Self::Vector => ValueType::Vector,
            Self::OptActor => ValueType::OptActor,
        }
    }

    /// `Err(reason)` when `value` is the right type but outside the domain.
    fn check_range(&self, value: &Value) -> Result<(), String> {
        match (self, value) {
            (Self::IntRange { min, max }, Value::Int(v)) if v < min || v > max => {
                Err(format!("{v} outside {min}..={max}"))
            }
            (Self::FloatRange { min, max }, Value::Float(v))
                if !v.is_finite() || v < min || v > max =>
            {
                Err(format!("{v} outside {min}..={max}"))
            }
            (Self::Vector, Value::Vector(v)) if v.iter().any(|c| !c.is_finite()) => {
                Err("non-finite vector component".to_owned())
            }
            (Self::Str { max_bytes }, Value::Str(s)) if s.len() > *max_bytes => {
                Err(format!("{} bytes exceeds {max_bytes}", s.len()))
            }
            (Self::Content(kind), Value::Content(id)) if id.kind() != *kind => {
                Err(format!("id is not a {} id", kind.label()))
            }
            _ => Ok(()),
        }
    }
}

/// The typed engine operation a bound call becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lowering {
    /// Args `[Int variable symbol, value]`; the value's type is checked
    /// against the variable by `MissionProgram::validate`.
    SetVariable,
    /// No args: request this terminal outcome.
    Finish(Outcome),
    /// One `Content` arg: a reward intent.
    GrantReward,
}

/// Where a binding's signature comes from. Never `verified_original` here
/// (AGENTS rule 8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingProvenance {
    /// Authored for tests or as a design placeholder; not an original claim.
    Synthetic { note: String },
    /// Observed in the installation; `evidence` names the evidence record.
    Observed { evidence: String },
}

/// One bound host call.
#[derive(Clone, Debug, PartialEq)]
pub struct BindingSpec {
    pub name: String,
    pub family: HostFamily,
    pub args: Vec<ArgDomain>,
    pub lowering: Lowering,
    pub repeatability: Repeatability,
    pub provenance: BindingProvenance,
}

impl BindingSpec {
    /// The effect phase, derived from the lowering so it cannot disagree.
    pub fn phase(&self) -> Phase {
        match self.lowering {
            Lowering::SetVariable => Phase::State,
            Lowering::Finish(_) => Phase::Terminal,
            Lowering::GrantReward => Phase::Host,
        }
    }

    /// The argument domains the lowering requires, as types.
    fn lowering_signature(&self) -> &'static [ValueType] {
        match self.lowering {
            Lowering::SetVariable => &[ValueType::Int],
            Lowering::Finish(_) => &[],
            Lowering::GrantReward => &[ValueType::Content],
        }
    }
}

/// Why a spec cannot be registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    BadName {
        name: String,
    },
    Duplicate {
        name: String,
    },
    /// The declared domains do not fit the lowering (for example a
    /// `GrantReward` without a content argument).
    SignatureMismatch {
        name: String,
    },
    TooManyArgs {
        name: String,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadName { name } => write!(f, "invalid binding name `{name}`"),
            Self::Duplicate { name } => write!(f, "binding `{name}` already registered"),
            Self::SignatureMismatch { name } => {
                write!(f, "binding `{name}`: arguments do not fit its lowering")
            }
            Self::TooManyArgs { name } => write!(f, "binding `{name}`: too many arguments"),
        }
    }
}

impl std::error::Error for RegistryError {}

/// One host call as decoded from an original program (untrusted data).
#[derive(Clone, Debug, PartialEq)]
pub struct RawCall {
    pub name: String,
    pub args: Vec<Value>,
    /// The call's byte range in the program's source resource.
    pub span: Option<SourceSpan>,
}

/// One objective as the adapter decoded it.
#[derive(Clone, Debug, PartialEq)]
pub struct RawObjective {
    pub id: SymbolId,
    pub content: ContentId,
    pub condition: Condition,
    pub calls: Vec<RawCall>,
    pub span: Option<SourceSpan>,
}

/// A decoded program before its calls are bound.
#[derive(Clone, Debug, PartialEq)]
pub struct RawProgram {
    pub mission: ContentId,
    pub variables: Vec<Variable>,
    pub objectives: Vec<RawObjective>,
}

/// Where a call was found, for every diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    pub mission: String,
    pub objective: SymbolId,
    /// Index of the call within the objective.
    pub call: usize,
    pub span: Option<SourceSpan>,
}

impl fmt::Display for CallSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} objective#{} call {}",
            self.mission, self.objective.0, self.call
        )?;
        if let Some(s) = self.span {
            write!(f, " @0x{:x}..0x{:x}", s.start, s.end)?;
        }
        Ok(())
    }
}

/// Why a call cannot be bound. The name is shown truncated: error text never
/// carries unbounded script data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingError {
    /// No binding is registered under this name.
    UnknownHostCall {
        at: CallSite,
        name: String,
    },
    NameTooLong {
        at: CallSite,
        len: usize,
    },
    ArityMismatch {
        at: CallSite,
        name: String,
        expected: usize,
        found: usize,
    },
    ArgumentType {
        at: CallSite,
        name: String,
        index: usize,
        expected: ValueType,
        found: ValueType,
    },
    ArgumentRange {
        at: CallSite,
        name: String,
        index: usize,
        reason: String,
    },
}

impl BindingError {
    /// The call site the error points at.
    pub fn site(&self) -> &CallSite {
        match self {
            Self::UnknownHostCall { at, .. }
            | Self::NameTooLong { at, .. }
            | Self::ArityMismatch { at, .. }
            | Self::ArgumentType { at, .. }
            | Self::ArgumentRange { at, .. } => at,
        }
    }
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownHostCall { at, name } => {
                write!(f, "{at}: unknown host call `{name}`")
            }
            Self::NameTooLong { at, len } => {
                write!(
                    f,
                    "{at}: call name of {len} bytes exceeds {MAX_CALL_NAME_BYTES}"
                )
            }
            Self::ArityMismatch {
                at,
                name,
                expected,
                found,
            } => write!(f, "{at}: `{name}` takes {expected} arguments, got {found}"),
            Self::ArgumentType {
                at,
                name,
                index,
                expected,
                found,
            } => write!(
                f,
                "{at}: `{name}` argument {index}: expected {expected:?}, found {found:?}"
            ),
            Self::ArgumentRange {
                at,
                name,
                index,
                reason,
            } => write!(f, "{at}: `{name}` argument {index}: {reason}"),
        }
    }
}

impl std::error::Error for BindingError {}

/// The calls a mission program may make, keyed by exact name.
#[derive(Clone, Debug, Default)]
pub struct HostBindingRegistry {
    specs: BTreeMap<String, BindingSpec>,
}

impl HostBindingRegistry {
    /// An empty registry: every call is unknown.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one binding.
    ///
    /// # Errors
    ///
    /// [`RegistryError`] for a bad or duplicate name, too many arguments or
    /// domains that do not fit the lowering.
    pub fn register(&mut self, spec: BindingSpec) -> Result<(), RegistryError> {
        let name = spec.name.clone();
        if name.is_empty()
            || name.len() > MAX_CALL_NAME_BYTES
            || !name.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(RegistryError::BadName { name });
        }
        if spec.args.len() > MAX_CALL_ARGS {
            return Err(RegistryError::TooManyArgs { name });
        }
        let required = spec.lowering_signature();
        let declared: Vec<ValueType> = spec.args.iter().map(ArgDomain::value_type).collect();
        let fits = match spec.lowering {
            // The value after the variable symbol may be any type.
            Lowering::SetVariable => declared.len() == 2 && declared[..1] == *required,
            _ => declared == required,
        };
        if !fits {
            return Err(RegistryError::SignatureMismatch { name });
        }
        if self.specs.contains_key(&name) {
            return Err(RegistryError::Duplicate { name });
        }
        self.specs.insert(name, spec);
        Ok(())
    }

    /// The binding registered under `name`.
    pub fn get(&self, name: &str) -> Option<&BindingSpec> {
        self.specs.get(name)
    }

    /// Number of registered bindings.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether nothing is registered.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// Binds one call to its engine operation.
    ///
    /// # Errors
    ///
    /// [`BindingError`] for an unknown name, wrong arity, wrong argument type
    /// or an argument outside its domain.
    pub fn bind(&self, call: &RawCall, at: &CallSite) -> Result<Action, BindingError> {
        if call.name.len() > MAX_CALL_NAME_BYTES {
            return Err(BindingError::NameTooLong {
                at: at.clone(),
                len: call.name.len(),
            });
        }
        let name = call.name.clone();
        let Some(spec) = self.specs.get(&call.name) else {
            return Err(BindingError::UnknownHostCall {
                at: at.clone(),
                name,
            });
        };
        if call.args.len() != spec.args.len() {
            return Err(BindingError::ArityMismatch {
                at: at.clone(),
                name,
                expected: spec.args.len(),
                found: call.args.len(),
            });
        }
        for (index, (domain, value)) in spec.args.iter().zip(&call.args).enumerate() {
            if domain.value_type() != value.value_type() {
                return Err(BindingError::ArgumentType {
                    at: at.clone(),
                    name: name.clone(),
                    index,
                    expected: domain.value_type(),
                    found: value.value_type(),
                });
            }
            if let Err(reason) = domain.check_range(value) {
                return Err(BindingError::ArgumentRange {
                    at: at.clone(),
                    name: name.clone(),
                    index,
                    reason,
                });
            }
        }
        Ok(match (spec.lowering, call.args.as_slice()) {
            (Lowering::SetVariable, [Value::Int(symbol), value]) => Action::SetVariable {
                // A negative symbol is excluded by the declared domain only
                // if the spec says so; refuse it here regardless.
                variable: match u32::try_from(*symbol) {
                    Ok(s) => SymbolId(s),
                    Err(_) => {
                        return Err(BindingError::ArgumentRange {
                            at: at.clone(),
                            name,
                            index: 0,
                            reason: format!("{symbol} is not a symbol id"),
                        });
                    }
                },
                value: value.clone(),
            },
            (Lowering::Finish(outcome), []) => Action::Finish(outcome),
            (Lowering::GrantReward, [Value::Content(reward)]) => Action::GrantReward {
                reward: reward.clone(),
            },
            // Unreachable after the checks above; fail closed, never a no-op.
            _ => {
                return Err(BindingError::ArityMismatch {
                    at: at.clone(),
                    name,
                    expected: spec.args.len(),
                    found: call.args.len(),
                });
            }
        })
    }
}

/// Lowers every call of `raw` through `registry`.
///
/// # Errors
///
/// Every [`BindingError`] in program order (all unknown calls are reported,
/// not just the first). On error no [`MissionProgram`] is produced, so
/// nothing reaches validation, let alone flight. On success the program still
/// has to pass `MissionProgram::validate`.
pub fn lower_program(
    registry: &HostBindingRegistry,
    raw: RawProgram,
) -> Result<MissionProgram, Vec<BindingError>> {
    let mission = raw.mission.to_string();
    let mut errors = Vec::new();
    let mut objectives = Vec::with_capacity(raw.objectives.len());
    for o in raw.objectives {
        let mut actions = Vec::with_capacity(o.calls.len());
        for (call, c) in o.calls.iter().enumerate() {
            let at = CallSite {
                mission: mission.clone(),
                objective: o.id,
                call,
                span: c.span,
            };
            match registry.bind(c, &at) {
                Ok(a) => actions.push(a),
                Err(e) => errors.push(e),
            }
        }
        objectives.push(Objective {
            id: o.id,
            content: o.content,
            condition: o.condition,
            actions,
            span: o.span,
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(MissionProgram {
        version: IR_VERSION,
        mission: raw.mission,
        variables: raw.variables,
        objectives,
    })
}
