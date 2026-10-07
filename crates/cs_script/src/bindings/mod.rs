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
//! The registry is **empty by default and ships no original names**: F38-B adds
//! the *measured* families in [`observed`], each with its provenance and either
//! a real lowering or an explicit refusal, and the normalized differential
//! traces in [`differential`]. F38-C adds [`located`]: source-located
//! diagnostics and the per-site coverage audit. Call data is untrusted, so name length, argument
//! count and string length are capped before any lookup and no binding reaches
//! anything but the simulation API.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};

use crate::ir::{
    Action, Condition, DirectiveOperation, IR_VERSION, MAX_VALUE_DEPTH, MAX_VALUE_ITEMS,
    MissionProgram, Objective, Outcome, Phase, SourceSpan, SymbolId, Value, ValueType, Variable,
};

/// Longest call name accepted.
pub const MAX_CALL_NAME_BYTES: usize = 64;
/// Most arguments one call may carry.
pub const MAX_CALL_ARGS: usize = 8;

pub mod differential;
pub mod located;
pub mod observed;

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
    IntRange {
        min: i32,
        max: i32,
    },
    FloatRange {
        min: f64,
        max: f64,
    },
    Str {
        max_bytes: usize,
    },
    Content(ContentKind),
    Actor,
    Vector,
    OptActor,
    /// An original argument-**list** node — one list as it appears inside a
    /// call's argument list, not a flattening of its children into
    /// positional arguments. `COMPLETED_STOPPOINT`'s `[[text,int,int]]` and
    /// `ANIM_STATE`'s `[text,[text,[text],text,[text]]]` each carry one;
    /// `children` is the domain of each child in order, so the structure the
    /// site spelled is checked field for field, nested lists included. An
    /// empty `children` is the measured empty list (`MeasuredArg::Empty`).
    List(Vec<ArgDomain>),
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
            Self::List(_) => ValueType::List,
        }
    }

    /// Whether some IR [`Value`] can lie in this domain — the domain's own
    /// bound check, at `depth` enclosing `List`s. A `List` domain deeper or
    /// wider than a value can be matches nothing, so registering it would
    /// declare a signature no call can satisfy.
    fn is_carriable(&self, depth: usize) -> bool {
        match self {
            Self::List(children) => {
                depth < MAX_VALUE_DEPTH
                    && children.len() <= MAX_VALUE_ITEMS
                    && children.iter().all(|c| c.is_carriable(depth + 1))
            }
            _ => true,
        }
    }

    /// `Err(reason)` when `value` is the right type but outside the domain.
    /// For a `List` domain the "range" is the structure itself: child count,
    /// child types and child domains, reported against the child's position.
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
            (Self::List(children), Value::List(items)) => {
                if items.len() != children.len() {
                    return Err(format!(
                        "{} list items, expected {}",
                        items.len(),
                        children.len()
                    ));
                }
                for (index, (child, item)) in children.iter().zip(items).enumerate() {
                    if child.value_type() != item.value_type() {
                        return Err(format!(
                            "child {index}: expected {:?}, found {:?}",
                            child.value_type(),
                            item.value_type()
                        ));
                    }
                    if let Err(reason) = child.check_range(item) {
                        return Err(format!("child {index}: {reason}"));
                    }
                }
                Ok(())
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
    /// A measured directive operation, handed to the host with the bound
    /// call's own arguments — the arguments the site spelled, structure
    /// intact, never reordered and never flattened. Any well-formed
    /// signature fits: the registry verifies the domains, not the
    /// operation↔shape correspondence, which is the declaring adapter's
    /// measured claim. Binding produces [`Action::Directive`], which the
    /// runtime executes as a documented host effect
    /// ([`crate::runtime::MissionState::directives`]); it is never a no-op.
    Directive(DirectiveOperation),
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
    /// Every measured argument signature this name accepts, in declared
    /// order. One signature is the common case; several is how a directive
    /// key whose sites disagree is carried — M01's `SET_HELP_LABEL` spells
    /// `[text,text]` at one site and `[[text,text],text]` at another, and
    /// **both** are represented: no signature is chosen, none is rejected
    /// for disagreeing with another, and declaration order privileges
    /// nothing beyond which refusal a call that fits none reports.
    pub signatures: Vec<Vec<ArgDomain>>,
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
            Lowering::GrantReward | Lowering::Directive(_) => Phase::Host,
        }
    }

    /// Whether `declared`'s types satisfy the lowering — total, derived from
    /// the variant so a spec cannot disagree with its own operation. A
    /// directive lowering carries the call's own arguments, so its only
    /// requirement is that every domain be carriable (checked separately by
    /// [`ArgDomain::is_carriable`]); the lowering places no shape of its own.
    fn signature_fits(&self, declared: &[ValueType]) -> bool {
        match self.lowering {
            // The value after the variable symbol may be any type.
            Lowering::SetVariable => declared.len() == 2 && declared[0] == ValueType::Int,
            Lowering::Finish(_) => declared.is_empty(),
            Lowering::GrantReward => declared == [ValueType::Content],
            Lowering::Directive(_) => true,
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

/// F38-B: a measured original program handed to this crate as the rows
/// [`observed::ObservedBindingTable::measure`] validates.
///
/// `cs_script` may depend on `cs_types` only
/// (`docs/01-ARCHITECTURE.md`), so it cannot name
/// `cs_formats::script_raw::ui_host_calls`' types. A consumer that reads the
/// installation performs the crossing mechanically, field for field, and the
/// mapping is total in both directions:
///
/// | `cs_formats` | `cs_script::bindings::observed` |
/// | --- | --- |
/// | `DispatchForm::Callback` / `::Mail` | `MeasuredForm::Callback` / `::Mail` |
/// | `ArgShape` | `MeasuredShape`, through `ArgShape::code()` and [`MeasuredShape::from_code`] |
/// | `HostCallCorpus::calls[].native_id` | `MeasuredCallRow::native_id` |
/// | `…sites` / `…scripts` | `MeasuredCallRow::sites` / `::scripts` |
/// | `…arities` | `MeasuredCallRow::arities` |
/// | `…arg_shapes[position]` (dominant shape, `None` when sites disagree) | `MeasuredCallRow::arg_shapes[position]` |
/// | `…evidence.note()` | `MeasuredCallRow::evidence` |
///
/// The shape codes are a **wire vocabulary** both crates publish, so neither
/// needs the other's types and a code this build does not know is refused on
/// both sides rather than read as a different shape.
pub mod measured {
    pub use super::observed::{MeasuredCallRow, MeasuredForm, MeasuredShape, RowError};

    /// The engine-side reading of a wire shape code.
    ///
    /// `None` for a code this build does not know, so a caller holding a
    /// measurement from a newer reader is refused rather than silently reading
    /// the wrong shape.
    pub const fn measured_shape_from_code(code: u8) -> Option<MeasuredShape> {
        MeasuredShape::from_code(code)
    }
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
        /// Every argument count the name's signatures accept, ascending with
        /// duplicates removed — several for a key whose measured sites
        /// disagree, so the refusal does not pretend one count was expected.
        expected: Vec<usize>,
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
            } => write!(
                f,
                "{at}: `{name}` takes {} arguments, got {found}",
                expected
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
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
#[derive(Clone, Debug, Default, PartialEq)]
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
        if spec.signatures.is_empty() {
            return Err(RegistryError::SignatureMismatch { name });
        }
        // Every declared signature is checked — accepting a name means
        // accepting every measured shape it was registered with, so one
        // unfit signature makes the spec unfit rather than quietly
        // narrowing what the name accepts.
        for signature in &spec.signatures {
            if signature.len() > MAX_CALL_ARGS {
                return Err(RegistryError::TooManyArgs { name });
            }
            let declared: Vec<ValueType> = signature.iter().map(ArgDomain::value_type).collect();
            let carriable = signature.iter().all(|d| d.is_carriable(0));
            if !carriable || !spec.signature_fits(&declared) {
                return Err(RegistryError::SignatureMismatch { name });
            }
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
        // Every declared signature is tried: a call binds when it satisfies
        // one of the shapes the name was measured to spell, and no signature
        // is chosen over another. When none fits, the refusal is the first
        // arity-matching signature's — a more informative error than an
        // arity report — or an `ArityMismatch` naming every accepted count
        // when no signature's count matched at all.
        let mut shape_error: Option<BindingError> = None;
        for signature in &spec.signatures {
            if call.args.len() != signature.len() {
                continue;
            }
            let mut signature_error: Option<BindingError> = None;
            for (index, (domain, value)) in signature.iter().zip(&call.args).enumerate() {
                if domain.value_type() != value.value_type() {
                    signature_error = Some(BindingError::ArgumentType {
                        at: at.clone(),
                        name: name.clone(),
                        index,
                        expected: domain.value_type(),
                        found: value.value_type(),
                    });
                    break;
                }
                if let Err(reason) = domain.check_range(value) {
                    signature_error = Some(BindingError::ArgumentRange {
                        at: at.clone(),
                        name: name.clone(),
                        index,
                        reason,
                    });
                    break;
                }
            }
            match signature_error {
                None => {
                    return Self::lower_bound_call(spec, call, at, name);
                }
                Some(error) => {
                    shape_error.get_or_insert(error);
                }
            }
        }
        if let Some(error) = shape_error {
            // At least one signature had the right count; its refusal is the
            // one that reports what actually failed.
            return Err(error);
        }
        let mut expected: Vec<usize> = spec.signatures.iter().map(Vec::len).collect();
        expected.sort_unstable();
        expected.dedup();
        Err(BindingError::ArityMismatch {
            at: at.clone(),
            name,
            expected,
            found: call.args.len(),
        })
    }

    /// The [`Action`] a call that satisfied one of `spec`'s signatures
    /// becomes. The final match on the call's own argument shape is what
    /// `signature_fits` guaranteed at registration; a spec that reached the
    /// registry cannot fall through here, but the code fails closed rather
    /// than trusting that — never a no-op, never `Action::Unknown`.
    fn lower_bound_call(
        spec: &BindingSpec,
        call: &RawCall,
        at: &CallSite,
        name: String,
    ) -> Result<Action, BindingError> {
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
            // The directive carries the call's own argument list — nested
            // lists stay nested, exactly as the site spelled them.
            (Lowering::Directive(operation), _) => Action::Directive {
                operation,
                args: call.args.clone(),
            },
            // Unreachable after the checks above; fail closed, never a no-op.
            _ => {
                return Err(BindingError::ArityMismatch {
                    at: at.clone(),
                    name,
                    expected: spec.signatures.iter().map(Vec::len).collect(),
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
