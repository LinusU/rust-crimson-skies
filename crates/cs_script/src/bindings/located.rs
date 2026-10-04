//! Source-located diagnostics and the per-site coverage audit (F38-C).
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! stage `### F38-C`; shared contract `docs/contracts/SCRIPT-MISSION.md`.
//!
//! F38-A and F38-B made every refusal *name* a call. This stage makes every
//! refusal *point at the original text*:
//!
//! * [`SourceMap`] maps a lowered call (objective symbol and call index) to the
//!   [`SiteOrigin`] it was read from, and [`lower_program_located`] returns the
//!   [`BindingError`]s of a bad program each with that origin, so a wrong
//!   argument type or range names member, line and column. Nothing here
//!   panics on untrusted input; a call with no mapped origin is reported
//!   without one rather than with an invented one.
//! * [`audit_sites`] judges **every measured site** — not every family — against
//!   an [`ObservedBindingTable`]: bound, refused for a named reason, or a
//!   located disagreement with its own family (arity, argument shape, an
//!   unknown shape code, a dispatch expression that names no id). The
//!   [`SiteAudit`] is the instruction-coverage report: `campaign_ready` only
//!   when the audit is non-empty and every site is bound, so one unimplemented
//!   or malformed site refuses it (AC04's rule, per site).
//!
//! `cs_script` may depend on `cs_types` only, so the producer's types
//! (`cs_formats::script_raw::source_map`) cross as primitives: [`SiteOrigin`]
//! and [`SiteRow`] are filled field for field by the consumer that reads the
//! installation.

use std::collections::BTreeMap;
use std::fmt;

use crate::bindings::observed::{
    MeasuredForm, MeasuredShape, ObservedBindingTable, ObservedDisposition, UnimplementedReason,
};
use crate::bindings::{BindingError, CallSite, HostBindingRegistry, RawProgram, lower_program};
use crate::ir::{MissionProgram, SymbolId};

/// Most sites one audit holds. A safety bound, not a measurement; the retail
/// corpus has 1812 sites.
pub const MAX_AUDIT_SITES: usize = 1 << 18;
/// Most entries one source map holds.
pub const MAX_MAP_ENTRIES: usize = 1 << 18;
/// Longest member spelling kept in an origin.
pub const MAX_MEMBER_BYTES: usize = 256;

/// Where a call lives in its original program. Mirrors
/// `cs_formats::script_raw::source_map::SiteOrigin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiteOrigin {
    pub member: String,
    pub offset: u64,
    pub len: u64,
    pub line: u32,
    pub column: u32,
}

impl fmt::Display for SiteOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{} (0x{:x}+{})",
            self.member, self.line, self.column, self.offset, self.len
        )
    }
}

/// Why a map or an audit cannot be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocatedError {
    /// More entries than the bound.
    TooMany { count: usize, limit: usize },
    /// A member spelling over [`MAX_MEMBER_BYTES`].
    MemberTooLong { len: usize },
    /// Two calls claim one map key.
    DuplicateKey { objective: SymbolId, call: usize },
}

impl fmt::Display for LocatedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooMany { count, limit } => write!(f, "{count} entries exceed the bound {limit}"),
            Self::MemberTooLong { len } => {
                write!(
                    f,
                    "member spelling of {len} bytes exceeds {MAX_MEMBER_BYTES}"
                )
            }
            Self::DuplicateKey { objective, call } => {
                write!(f, "objective#{} call {call} is mapped twice", objective.0)
            }
        }
    }
}

impl std::error::Error for LocatedError {}

/// Maps a lowered call to its origin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    origins: BTreeMap<(SymbolId, usize), SiteOrigin>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records where call `call` of `objective` was read from.
    ///
    /// # Errors
    ///
    /// [`LocatedError`] for a bound overflow, an over-long member or a key
    /// mapped twice.
    pub fn insert(
        &mut self,
        objective: SymbolId,
        call: usize,
        origin: SiteOrigin,
    ) -> Result<(), LocatedError> {
        if origin.member.len() > MAX_MEMBER_BYTES {
            return Err(LocatedError::MemberTooLong {
                len: origin.member.len(),
            });
        }
        if self.origins.len() >= MAX_MAP_ENTRIES {
            return Err(LocatedError::TooMany {
                count: self.origins.len() + 1,
                limit: MAX_MAP_ENTRIES,
            });
        }
        if self.origins.contains_key(&(objective, call)) {
            return Err(LocatedError::DuplicateKey { objective, call });
        }
        self.origins.insert((objective, call), origin);
        Ok(())
    }

    /// The origin of the call a diagnostic names.
    pub fn origin_of(&self, at: &CallSite) -> Option<&SiteOrigin> {
        self.origins.get(&(at.objective, at.call))
    }

    pub fn len(&self) -> usize {
        self.origins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }
}

/// A binding error together with the original text it points at.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatedBindingError {
    pub error: BindingError,
    /// `None` when the call has no mapped origin; never an invented one.
    pub origin: Option<SiteOrigin>,
}

impl fmt::Display for LocatedBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.origin {
            Some(origin) => write!(f, "{origin}: {}", self.error),
            None => write!(f, "{}", self.error),
        }
    }
}

impl std::error::Error for LocatedBindingError {}

/// [`lower_program`] with every refusal attached to its origin.
///
/// # Errors
///
/// Every [`BindingError`] in program order, each with the origin `map` holds
/// for its call. On error no program exists.
pub fn lower_program_located(
    registry: &HostBindingRegistry,
    raw: RawProgram,
    map: &SourceMap,
) -> Result<MissionProgram, Vec<LocatedBindingError>> {
    lower_program(registry, raw).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| LocatedBindingError {
                origin: map.origin_of(error.site()).cloned(),
                error,
            })
            .collect()
    })
}

/// One measured site as the consumer reads it from the producer.
#[derive(Clone, Debug, PartialEq)]
pub struct SiteRow {
    pub form: MeasuredForm,
    /// The dispatch value; `None` when the dispatch expression is not an
    /// integer literal.
    pub native_id: Option<i64>,
    /// `ArgShape::code()` per argument, so a code this build does not know is
    /// refused rather than read as another shape.
    pub arg_shape_codes: Vec<u8>,
    pub origin: SiteOrigin,
}

/// What the table says about one site.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SiteVerdict {
    /// The site's family lowers to an engine operation and the site fits it.
    Bound { spelling: String },
    /// The site's family is measured and refused for a named reason.
    Refused(UnimplementedReason),
    /// The dispatch expression names no integer id, so no family claims it.
    NoNativeId,
    /// No valid family exists for this form and id (for example its sites
    /// disagreed and the row was refused).
    NoFamily,
    /// The site spells a different number of arguments than its family.
    ArityMismatch { expected: usize, found: usize },
    /// An argument's shape differs from its family's.
    ShapeMismatch {
        position: usize,
        expected: MeasuredShape,
        found: MeasuredShape,
    },
    /// An argument's shape code is not one this build knows.
    UnknownShapeCode { position: usize, code: u8 },
}

impl SiteVerdict {
    /// Stable short code for reports.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Bound { .. } => "bound",
            Self::Refused(_) => "refused",
            Self::NoNativeId => "no_native_id",
            Self::NoFamily => "no_family",
            Self::ArityMismatch { .. } => "arity_mismatch",
            Self::ShapeMismatch { .. } => "shape_mismatch",
            Self::UnknownShapeCode { .. } => "unknown_shape_code",
        }
    }
}

/// One site with its verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct LocatedSite {
    pub origin: SiteOrigin,
    pub verdict: SiteVerdict,
}

impl fmt::Display for LocatedSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.origin)?;
        match &self.verdict {
            SiteVerdict::Bound { spelling } => write!(f, "bound as `{spelling}`"),
            SiteVerdict::Refused(UnimplementedReason::ArgumentShapeHasNoDomain {
                position,
                shape,
            }) => write!(
                f,
                "refused: argument {position} ({}) has no engine value",
                shape.label()
            ),
            SiteVerdict::Refused(UnimplementedReason::MeaningNotMeasured { detail }) => {
                write!(f, "refused: {detail}")
            }
            SiteVerdict::NoNativeId => write!(f, "the dispatch expression names no integer id"),
            SiteVerdict::NoFamily => write!(f, "no valid measured family for this call"),
            SiteVerdict::ArityMismatch { expected, found } => {
                write!(
                    f,
                    "takes {expected} arguments in its family, spells {found}"
                )
            }
            SiteVerdict::ShapeMismatch {
                position,
                expected,
                found,
            } => write!(
                f,
                "argument {position}: family measured {}, site spells {}",
                expected.label(),
                found.label()
            ),
            SiteVerdict::UnknownShapeCode { position, code } => {
                write!(f, "argument {position}: unknown shape code {code}")
            }
        }
    }
}

/// The per-site instruction-coverage report.
#[derive(Clone, Debug, PartialEq)]
pub struct SiteAudit {
    pub sites: Vec<LocatedSite>,
}

impl SiteAudit {
    pub fn bound(&self) -> usize {
        self.count(|v| matches!(v, SiteVerdict::Bound { .. }))
    }

    pub fn refused(&self) -> usize {
        self.count(|v| matches!(v, SiteVerdict::Refused(_)))
    }

    /// Sites that disagree with their own family or name no family at all.
    pub fn malformed(&self) -> usize {
        self.sites.len() - self.bound() - self.refused()
    }

    /// The located sites whose verdict is `code`.
    pub fn with_code<'a>(&'a self, code: &'a str) -> impl Iterator<Item = &'a LocatedSite> {
        self.sites.iter().filter(move |s| s.verdict.code() == code)
    }

    /// Whether every audited site is bound. An audit of nothing refuses: it
    /// would be ready only because nothing was looked at.
    pub fn campaign_ready(&self) -> bool {
        !self.sites.is_empty() && self.bound() == self.sites.len()
    }

    fn count(&self, f: impl Fn(&SiteVerdict) -> bool) -> usize {
        self.sites.iter().filter(|s| f(&s.verdict)).count()
    }
}

/// Judges every site against `table`.
///
/// # Errors
///
/// [`LocatedError::TooMany`] / [`LocatedError::MemberTooLong`] for input over a
/// bound. A site that does not fit is a *verdict*, never an error and never a
/// panic.
pub fn audit_sites(
    table: &ObservedBindingTable,
    rows: &[SiteRow],
) -> Result<SiteAudit, LocatedError> {
    if rows.len() > MAX_AUDIT_SITES {
        return Err(LocatedError::TooMany {
            count: rows.len(),
            limit: MAX_AUDIT_SITES,
        });
    }
    let mut sites = Vec::with_capacity(rows.len());
    for row in rows {
        if row.origin.member.len() > MAX_MEMBER_BYTES {
            return Err(LocatedError::MemberTooLong {
                len: row.origin.member.len(),
            });
        }
        sites.push(LocatedSite {
            origin: row.origin.clone(),
            verdict: judge(table, row),
        });
    }
    Ok(SiteAudit { sites })
}

fn judge(table: &ObservedBindingTable, row: &SiteRow) -> SiteVerdict {
    let Some(native_id) = row.native_id else {
        return SiteVerdict::NoNativeId;
    };
    let Some(family) = table.family(row.form, native_id) else {
        return SiteVerdict::NoFamily;
    };
    let mut shapes = Vec::with_capacity(row.arg_shape_codes.len());
    for (position, &code) in row.arg_shape_codes.iter().enumerate() {
        match MeasuredShape::from_code(code) {
            Some(shape) => shapes.push(shape),
            None => return SiteVerdict::UnknownShapeCode { position, code },
        }
    }
    if shapes.len() != family.call.arity {
        return SiteVerdict::ArityMismatch {
            expected: family.call.arity,
            found: shapes.len(),
        };
    }
    for (position, (&found, &expected)) in shapes.iter().zip(&family.call.arg_shapes).enumerate() {
        if found != expected {
            return SiteVerdict::ShapeMismatch {
                position,
                expected,
                found,
            };
        }
    }
    match &family.disposition {
        ObservedDisposition::Bound { spelling, .. } => SiteVerdict::Bound {
            spelling: spelling.clone(),
        },
        ObservedDisposition::Unimplemented { reason, .. } => SiteVerdict::Refused(reason.clone()),
    }
}
