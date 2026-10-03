//! Measured host-binding families and the coverage gate (F38-B).
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! stage `### F38-B`; shared contract `docs/contracts/SCRIPT-MISSION.md`.
//!
//! F38-A's [`crate::bindings`] registry ships **no** call names, because no
//! original call name had been measured. F38-B closes that gap for the one
//! program family the installation exposes readably: the `ASSETS/SCRIPTS`
//! members of `crimson.rof`, whose text `cs_formats::script_raw::ui_host_calls`
//! measures into a [`HostCallCorpus`](corpus docs). This module turns those
//! measurements into **families** and gives every family an explicit
//! disposition:
//!
//! * a family whose measured signature an engine operation can express is
//!   [`ObservedDisposition::Bound`] and is registered in the registry with
//!   [`BindingProvenance::Observed`];
//! * every other family is [`ObservedDisposition::Unimplemented`] with the
//!   reason, and it is **counted, never stubbed**: a binding that returned
//!   success would be a fabricated original behaviour (sheet non-negotiable #2).
//!
//! That is the honest outcome for the measured corpus today: the corpus's
//! families carry presentation, dialogue and control semantics that the mission
//! IR has no action for, and their argument expressions have no measured
//! meaning. The gate therefore reports *not covered*, and
//! [`ObservedCoverage::campaign_ready`] is false while a single measured family
//! is unimplemented (AC04's rule, applied to this batch).
//!
//! ## Layering
//!
//! `cs_script` may depend on `cs_types` only
//! (`docs/01-ARCHITECTURE.md`), so it cannot name `cs_formats`' types. The
//! measured row crossing the boundary is [`MeasuredCallRow`], built from
//! primitives; [`crate::bindings::measured`] documents the mechanical mapping a
//! consumer performs. The shape codes are a wire vocabulary shared with
//! `cs_formats::script_raw::ui_host_calls::ArgShape::code`, and
//! [`MeasuredShape::from_code`] is total for every code that build knows, so a
//! stale or unknown code is refused rather than read as a different shape.

use std::collections::BTreeMap;
use std::fmt;

use crate::bindings::{
    ArgDomain, BindingProvenance, BindingSpec, HostBindingRegistry, HostFamily, Lowering,
    MAX_CALL_ARGS, RegistryError, Repeatability,
};

/// The measured class of one argument expression of an observed host call.
///
/// A mirror of the wire codes `cs_formats::script_raw::ui_host_calls::ArgShape`
/// publishes, kept here because `cs_script` may not depend on `cs_formats`. The
/// variants name **shapes**, never meanings: `IntegerLiteral` says the site
/// spells an integer, not that the integer is a particular value in a
/// particular domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeasuredShape {
    /// A decimal integer literal.
    IntegerLiteral = 0,
    /// A floating-point literal.
    FloatLiteral = 1,
    /// A `"…"` string literal.
    StringLiteral = 2,
    /// `$$name$$`, a named reference.
    NameRef = 3,
    /// `@scope@class`, a widget-class reference.
    WidgetClassRef = 4,
    /// A dotted path such as `a.b.c`.
    MemberRef = 5,
    /// An expression containing an index such as `a[i]`.
    IndexedRef = 6,
    /// Any other expression; deliberately not evaluated.
    Unevaluated = 7,
}

impl MeasuredShape {
    /// Every shape this build knows, ascending by code.
    pub const ALL: [MeasuredShape; 8] = [
        MeasuredShape::IntegerLiteral,
        MeasuredShape::FloatLiteral,
        MeasuredShape::StringLiteral,
        MeasuredShape::NameRef,
        MeasuredShape::WidgetClassRef,
        MeasuredShape::MemberRef,
        MeasuredShape::IndexedRef,
        MeasuredShape::Unevaluated,
    ];

    /// The wire code this shape is carried as.
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The shape a wire code names; `None` for a code this build does not know,
    /// so a caller on a newer measurement is refused rather than silently
    /// mis-read.
    pub const fn from_code(code: u8) -> Option<MeasuredShape> {
        match code {
            0 => Some(MeasuredShape::IntegerLiteral),
            1 => Some(MeasuredShape::FloatLiteral),
            2 => Some(MeasuredShape::StringLiteral),
            3 => Some(MeasuredShape::NameRef),
            4 => Some(MeasuredShape::WidgetClassRef),
            5 => Some(MeasuredShape::MemberRef),
            6 => Some(MeasuredShape::IndexedRef),
            7 => Some(MeasuredShape::Unevaluated),
            _ => None,
        }
    }

    /// Stable lowercase label for reports; identical to the measured vocabulary
    /// on the `cs_formats` side.
    pub const fn label(self) -> &'static str {
        match self {
            Self::IntegerLiteral => "integer_literal",
            Self::FloatLiteral => "float_literal",
            Self::StringLiteral => "string_literal",
            Self::NameRef => "name_ref",
            Self::WidgetClassRef => "widget_class_ref",
            Self::MemberRef => "member_ref",
            Self::IndexedRef => "indexed_ref",
            Self::Unevaluated => "unevaluated",
        }
    }
}

/// Which measured dispatch form a call was found in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MeasuredForm {
    /// `callback($$handler$$, <call>, <arg>…)`.
    Callback,
    /// `mail(<message>, <recipient>)`.
    Mail,
}

impl MeasuredForm {
    /// Stable lowercase label, identical to the measured vocabulary on the
    /// `cs_formats` side.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Callback => "callback",
            Self::Mail => "mail",
        }
    }

    /// The wire code this form is carried as.
    pub const fn code(self) -> u8 {
        match self {
            Self::Callback => 0,
            Self::Mail => 1,
        }
    }

    /// The form a wire code names; `None` for an unknown code.
    pub const fn from_code(code: u8) -> Option<MeasuredForm> {
        match code {
            0 => Some(MeasuredForm::Callback),
            1 => Some(MeasuredForm::Mail),
            _ => None,
        }
    }
}

/// One measured dispatch value as it crosses into the engine: the raw
/// measurement facts and nothing else.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredCallRow {
    /// The measured dispatch form.
    pub form: MeasuredForm,
    /// The integer the sites spell as their dispatch expression.
    pub native_id: i64,
    /// Sites measured across the corpus.
    pub sites: u32,
    /// Programs that contributed a site.
    pub scripts: u32,
    /// Every argument count observed for this value, ascending.
    pub arities: Vec<u32>,
    /// Per argument position, the shape every observed site agreed on, or `None`
    /// when the sites disagreed (which makes the family unimplemented).
    pub arg_shapes: Vec<Option<MeasuredShape>>,
    /// A summary of how the row was measured and where; never original text.
    pub evidence: String,
}

/// Why a measured row cannot become a binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowError {
    /// The dispatch value is outside the engine's id range.
    IdOutOfRange {
        /// The measured value.
        native_id: i64,
    },
    /// The row claims more arguments than a call may carry.
    TooManyArguments {
        /// The measured value.
        native_id: i64,
        /// Argument positions the row claims.
        args: usize,
        /// The bound in force.
        limit: usize,
    },
    /// Two sites of one value disagreed about an argument position's shape, so
    /// no single domain can be declared for it.
    DisagreeingArgumentShape {
        /// The measured value.
        native_id: i64,
        /// The zero-based argument position.
        position: usize,
    },
    /// Two sites of one value disagreed about the argument count.
    DisagreeingArity {
        /// The measured value.
        native_id: i64,
    },
    /// The row carries no evidence, or an over-long summary.
    NoEvidence {
        /// The measured value.
        native_id: i64,
    },
}

impl RowError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::IdOutOfRange { .. } => "id_out_of_range",
            Self::TooManyArguments { .. } => "too_many_arguments",
            Self::DisagreeingArgumentShape { .. } => "disagreeing_argument_shape",
            Self::DisagreeingArity { .. } => "disagreeing_arity",
            Self::NoEvidence { .. } => "no_evidence",
        }
    }
}

impl fmt::Display for RowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdOutOfRange { native_id } => {
                write!(f, "dispatch value {native_id} outside 0..={}", i32::MAX)
            }
            Self::TooManyArguments {
                native_id,
                args,
                limit,
            } => write!(
                f,
                "dispatch value {native_id} claims {args} arguments, at most {limit}"
            ),
            Self::DisagreeingArgumentShape {
                native_id,
                position,
            } => write!(
                f,
                "dispatch value {native_id}: argument {position} shapes disagree"
            ),
            Self::DisagreeingArity { native_id } => {
                write!(f, "dispatch value {native_id}: argument counts disagree")
            }
            Self::NoEvidence { native_id } => {
                write!(f, "dispatch value {native_id}: no measurement evidence")
            }
        }
    }
}

impl std::error::Error for RowError {}

/// One measured call, validated.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasuredCall {
    /// The measured dispatch form.
    pub form: MeasuredForm,
    /// The dispatch value.
    pub native_id: i64,
    /// Sites measured across the corpus.
    pub sites: u32,
    /// Programs that contributed a site.
    pub scripts: u32,
    /// The one argument count every site agreed on.
    pub arity: usize,
    /// The agreed shape per argument position.
    pub arg_shapes: Vec<MeasuredShape>,
    /// A summary of how the call was measured and where.
    pub evidence: String,
}

impl MeasuredCall {
    /// The registry spelling this call is registered under.
    ///
    /// A **derived** spelling, not an original symbol: the corpus contains the
    /// dispatch form and an integer, and no name. It is stable, unambiguous and
    /// built only from measured facts.
    pub fn spelling(&self) -> String {
        format!("{}#{}", self.form.label(), self.native_id)
    }

    /// Validates one measured row.
    ///
    /// # Errors
    ///
    /// [`RowError`] for a dispatch value out of range, an argument count over
    /// [`MAX_CALL_ARGS`], sites that disagree about arity or about a position's
    /// shape, or a row with no evidence.
    pub fn from_row(row: &MeasuredCallRow) -> Result<Self, RowError> {
        if row.native_id < 0 || row.native_id > i64::from(i32::MAX) {
            return Err(RowError::IdOutOfRange {
                native_id: row.native_id,
            });
        }
        if row.arg_shapes.len() > MAX_CALL_ARGS {
            return Err(RowError::TooManyArguments {
                native_id: row.native_id,
                args: row.arg_shapes.len(),
                limit: MAX_CALL_ARGS,
            });
        }
        if row.evidence.trim().is_empty() {
            return Err(RowError::NoEvidence {
                native_id: row.native_id,
            });
        }
        if row.arities.len() != 1 {
            return Err(RowError::DisagreeingArity {
                native_id: row.native_id,
            });
        }
        let arity = row.arities[0] as usize;
        if arity != row.arg_shapes.len() {
            return Err(RowError::DisagreeingArity {
                native_id: row.native_id,
            });
        }
        let mut arg_shapes = Vec::with_capacity(arity);
        for (position, shape) in row.arg_shapes.iter().enumerate() {
            arg_shapes.push(shape.ok_or(RowError::DisagreeingArgumentShape {
                native_id: row.native_id,
                position,
            })?);
        }
        Ok(Self {
            form: row.form,
            native_id: row.native_id,
            sites: row.sites,
            scripts: row.scripts,
            arity,
            arg_shapes,
            evidence: row.evidence.clone(),
        })
    }

    /// The declared argument domains a measured shape implies.
    ///
    /// A shape is a **measurement**, not a domain, so this conversion is
    /// deliberately conservative and its limits are the point:
    ///
    /// - [`MeasuredShape::IntegerLiteral`] becomes an unrestricted
    ///   [`ArgDomain::IntRange`] — the corpus's integer arguments are literals
    ///   of unknown meaning, and inventing a narrower domain would be a guess;
    /// - [`MeasuredShape::StringLiteral`] becomes a [`ArgDomain::Str`] capped at
    ///   the largest string a binding may carry;
    /// - every other shape has **no** domain, because the engine has no value
    ///   that stands for "a member reference into an original widget object".
    ///
    /// Returns `None` for a shape without a domain.
    pub fn arg_domain(shape: MeasuredShape) -> Option<ArgDomain> {
        match shape {
            MeasuredShape::IntegerLiteral => Some(ArgDomain::IntRange {
                min: i32::MIN,
                max: i32::MAX,
            }),
            MeasuredShape::StringLiteral => Some(ArgDomain::Str {
                max_bytes: MAX_CALL_ARGS * 8,
            }),
            _ => None,
        }
    }

    /// The domains this call's measured shapes imply, or the position that has
    /// none.
    pub fn arg_domains(&self) -> Result<Vec<ArgDomain>, usize> {
        let mut domains = Vec::with_capacity(self.arg_shapes.len());
        for (position, shape) in self.arg_shapes.iter().enumerate() {
            match Self::arg_domain(*shape) {
                Some(domain) => domains.push(domain),
                None => return Err(position),
            }
        }
        Ok(domains)
    }
}

/// Why a measured family is not implemented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnimplementedReason {
    /// An argument position's measured shape has no engine value that stands
    /// for it.
    ArgumentShapeHasNoDomain {
        /// The zero-based argument position.
        position: usize,
        /// The measured shape at that position.
        shape: MeasuredShape,
    },
    /// No engine operation expresses what the family does. A family whose
    /// meaning has not been measured is *not* bound to a convenient operation.
    MeaningNotMeasured {
        /// What is missing.
        detail: &'static str,
    },
}

/// What became of one measured family.
#[derive(Clone, Debug, PartialEq)]
pub enum ObservedDisposition {
    /// The family lowers to an engine operation and is registered under
    /// [`MeasuredCall::spelling`].
    Bound {
        /// The registry spelling it is registered as.
        spelling: String,
        /// The operation it lowers to.
        lowering: Lowering,
    },
    /// The family is measured, counted and refused before flight. It is never
    /// replaced by a stub that reports success.
    Unimplemented {
        /// Why it is not implemented.
        reason: UnimplementedReason,
        /// How it was measured.
        evidence: String,
    },
}

/// One measured family: the calls of one dispatch form and one dispatch value.
#[derive(Clone, Debug, PartialEq)]
pub struct ObservedFamily {
    /// The measured call.
    pub call: MeasuredCall,
    /// What became of it.
    pub disposition: ObservedDisposition,
}

impl ObservedFamily {
    /// Sites this family covers.
    pub fn sites(&self) -> u32 {
        self.call.sites
    }

    /// Whether this family lowers to an engine operation.
    pub fn is_bound(&self) -> bool {
        matches!(self.disposition, ObservedDisposition::Bound { .. })
    }
}

/// The coverage of one measured corpus over the engine's bindings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObservedCoverage {
    /// Families measured.
    pub families: usize,
    /// Families registered as bindings.
    pub bound_families: usize,
    /// Families refused.
    pub unimplemented_families: usize,
    /// Sites measured across all families.
    pub sites: u32,
    /// Sites covered by a registered binding.
    pub bound_sites: u32,
    /// Sites of refused families.
    pub unimplemented_sites: u32,
}

impl ObservedCoverage {
    /// Whether every measured family is implemented.
    ///
    /// This is AC04's rule for this batch: while one measured dispatch value is
    /// unimplemented, the corpus is **not** covered and nothing downstream may
    /// call itself complete. It is `false` for the measured UI corpus today.
    pub fn complete(&self) -> bool {
        self.unimplemented_families == 0
    }

    /// The gate itself: `true` only when the corpus is fully implemented.
    pub fn campaign_ready(&self) -> bool {
        self.complete()
    }
}

/// Why a measured corpus was refused as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoverageError {
    /// Two rows claim the same dispatch form and value.
    DuplicateCall {
        /// The measured value.
        native_id: i64,
    },
    /// A row is not a valid measured call.
    BadRow(RowError),
    /// A family the table was asked to bind cannot be registered: the derived
    /// spelling is refused or collides.
    RegistryRefused {
        /// The registry spelling.
        spelling: String,
        /// The registry's refusal.
        reason: String,
    },
}

impl CoverageError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DuplicateCall { .. } => "duplicate_call",
            Self::BadRow(_) => "bad_row",
            Self::RegistryRefused { .. } => "registry_refused",
        }
    }
}

impl fmt::Display for CoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateCall { native_id } => {
                write!(f, "dispatch value {native_id} measured twice")
            }
            Self::BadRow(error) => write!(f, "{error}"),
            Self::RegistryRefused { spelling, reason } => {
                write!(f, "`{spelling}` refused by the registry: {reason}")
            }
        }
    }
}

impl std::error::Error for CoverageError {}

/// The measured host-binding families of one corpus, with their dispositions and
/// the registry the bound families live in.
///
/// A family is bound only when a **declared** rule maps its measured signature
/// to an engine operation. There is no such rule for any measured family of the
/// UI script corpus today, so this table reports a coverage of zero and refuses
/// the rest — which is the correct outcome, not a missing one: the corpus's
/// families carry presentation, dialogue and control semantics that neither the
/// mission IR nor the contract's measured argument domains can express yet, and
/// binding them to a convenient operation would fabricate original behaviour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObservedBindingTable {
    families: Vec<ObservedFamily>,
    registry: HostBindingRegistry,
    coverage: ObservedCoverage,
}

impl ObservedBindingTable {
    /// An empty table: no measured family, no binding.
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates every row and records one family per row, in row order.
    ///
    /// No family is bound: see the type documentation. Every row is still
    /// measured and counted, so the coverage denominator is the corpus's.
    ///
    /// # Errors
    ///
    /// [`CoverageError`] for a duplicate dispatch value or a row that is not a
    /// valid measured call. A refused corpus yields no table at all, so a
    /// partial corpus is never mistaken for a whole one.
    pub fn measure(rows: &[MeasuredCallRow]) -> Result<Self, CoverageError> {
        let mut seen = BTreeMap::<(MeasuredForm, i64), ()>::new();
        let mut families = Vec::with_capacity(rows.len());
        for row in rows {
            if seen
                .insert((row.form, row.native_id), ())
                .is_some_and(|()| true)
            {
                return Err(CoverageError::DuplicateCall {
                    native_id: row.native_id,
                });
            }
            let call = MeasuredCall::from_row(row).map_err(CoverageError::BadRow)?;
            let disposition = classify(&call);
            families.push(ObservedFamily { call, disposition });
        }
        let coverage = coverage_of(&families);
        Ok(Self {
            families,
            registry: HostBindingRegistry::new(),
            coverage,
        })
    }

    /// Every measured family, in row order.
    pub fn families(&self) -> &[ObservedFamily] {
        &self.families
    }

    /// The family for one dispatch form and value.
    pub fn family(&self, form: MeasuredForm, native_id: i64) -> Option<&ObservedFamily> {
        self.families
            .iter()
            .find(|f| f.call.form == form && f.call.native_id == native_id)
    }

    /// The registry holding the bound families.
    pub fn registry(&self) -> &HostBindingRegistry {
        &self.registry
    }

    /// The coverage of this corpus over the engine's bindings.
    pub fn coverage(&self) -> ObservedCoverage {
        self.coverage
    }

    /// The families refused before flight, in row order.
    pub fn unimplemented(&self) -> impl Iterator<Item = &ObservedFamily> {
        self.families.iter().filter(|f| !f.is_bound())
    }

    /// The families of one measured dispatch form, in row order.
    pub fn families_of(&self, form: MeasuredForm) -> impl Iterator<Item = &ObservedFamily> {
        self.families.iter().filter(move |f| f.call.form == form)
    }
}

/// Decides one measured call's disposition.
///
/// This is the only place a measured family could become a binding, and it binds
/// nothing today: no original family has a measured meaning that an engine
/// operation states. A rule added here must name the measurement it rests on and
/// may only map a family onto an operation the contract lists; anything else
/// stays [`ObservedDisposition::Unimplemented`].
fn classify(call: &MeasuredCall) -> ObservedDisposition {
    // An argument shape with no engine value cannot be validated, let alone
    // executed: refuse at the first such position rather than dropping it.
    if let Err(position) = call.arg_domains() {
        return ObservedDisposition::Unimplemented {
            reason: UnimplementedReason::ArgumentShapeHasNoDomain {
                position,
                shape: call.arg_shapes[position],
            },
            evidence: call.evidence.clone(),
        };
    }
    ObservedDisposition::Unimplemented {
        reason: UnimplementedReason::MeaningNotMeasured {
            detail: "no original observation states what this dispatch value does",
        },
        evidence: call.evidence.clone(),
    }
}

fn coverage_of(families: &[ObservedFamily]) -> ObservedCoverage {
    let mut coverage = ObservedCoverage {
        families: families.len(),
        bound_families: 0,
        unimplemented_families: 0,
        sites: 0,
        bound_sites: 0,
        unimplemented_sites: 0,
    };
    for family in families {
        coverage.sites += family.call.sites;
        if family.is_bound() {
            coverage.bound_families += 1;
            coverage.bound_sites += family.call.sites;
        } else {
            coverage.unimplemented_families += 1;
            coverage.unimplemented_sites += family.call.sites;
        }
    }
    coverage
}

/// Registers one measured family under its derived spelling with an
/// **explicitly supplied** lowering.
///
/// The lowering is the caller's, never this module's: a binding whose meaning
/// has not been measured must not be given one here. This exists so the registry
/// path a bound family takes is production code and not a test-only shortcut.
///
/// # Errors
///
/// [`RegistryError`] when the derived spelling or the declared domains are
/// refused by the registry.
pub fn register_measured(
    registry: &mut HostBindingRegistry,
    call: &MeasuredCall,
    family: HostFamily,
    lowering: Lowering,
    repeatability: Repeatability,
) -> Result<BindingSpec, RegistryError> {
    let args = call
        .arg_domains()
        .map_err(|_| RegistryError::SignatureMismatch {
            name: call.spelling(),
        })?;
    let spec = BindingSpec {
        name: call.spelling(),
        family,
        args,
        lowering,
        repeatability,
        provenance: BindingProvenance::Observed {
            evidence: call.evidence.clone(),
        },
    };
    registry.register(spec.clone())?;
    Ok(spec)
}
